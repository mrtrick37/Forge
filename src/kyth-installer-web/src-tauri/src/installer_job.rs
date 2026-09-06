//! Native ownership of one installer job.
//!
//! This module deliberately has no HTTP or SSE implementation.  The worker
//! owns the job and appends to the history; consumers only take snapshots or
//! replay history.  That separation is important: a disconnected client must
//! not stop, duplicate, or reorder installation state transitions.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::installer_runtime::{Lifecycle, Phase, RuntimeSnapshot};

/// The message retained for compatibility with the existing installer API.
pub(crate) const CANCELLATION_MESSAGE: &str =
    "Installation cancelled by user. Disk changes may have already started.";

const MAX_EVENT_HISTORY: usize = 1024;
const PHASES: [Phase; 6] = [
    Phase::Prepare,
    Phase::Storage,
    Phase::Image,
    Phase::Configure,
    Phase::SecureBoot,
    Phase::Complete,
];

/// A cancellation handle passed to the phase implementation.
///
/// The executor is expected to observe this token while doing long-running
/// work and return once its child processes and other resources are cleaned
/// up.  The supervisor does not release the job slot until that return.
#[derive(Clone, Debug, Default)]
pub(crate) struct CancellationToken {
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl CancellationToken {
    pub(crate) fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// The only execution capability required by the job supervisor.
///
/// Production code can adapt the typed native executor to this trait.  Tests
/// inject a deterministic fake, which keeps ownership and event semantics
/// testable without disks, firmware, or a live image.
pub(crate) trait PhaseExecutor: Send + Sync + 'static {
    fn execute_phase(&self, phase: Phase, cancellation: &CancellationToken) -> Result<(), String>;

    /// Give an executor the supervisor-owned correlation ID before its worker
    /// thread starts. Durable transaction records can therefore be joined to
    /// `/api/report` and the SSE terminal event without a race.
    fn record_job_started(&self, _job_id: u64) -> Result<(), String> {
        Ok(())
    }

    fn record_cancelled(&self, _phase: Option<Phase>) {}

    fn record_failed(&self, _phase: Phase, _message: &str) {}

    /// Return a bounded native Secure Boot state for a successful install.
    /// Error and cancellation events carry their own terminal semantics.
    fn success_mok_state(&self) -> Option<String> {
        None
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum JobEventKind {
    Phase {
        phase: Phase,
    },
    Done {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mok_state: Option<String>,
    },
    Error {
        message: String,
        phase: Option<Phase>,
        cancelled: bool,
    },
}

/// An event with a supervisor-assigned, monotonic ID.
///
/// IDs are assigned while holding the same lock as the runtime state, before
/// subscribers are notified.  Replaying an event therefore never mutates
/// state and every subscriber sees the same ordering.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct JobEvent {
    pub id: u64,
    #[serde(flatten)]
    pub kind: JobEventKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EventReplay {
    pub events: Vec<JobEvent>,
    pub next_event_id: u64,
    pub reset_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JobSnapshot {
    pub job_id: Option<u64>,
    pub runtime: RuntimeSnapshot,
    pub worker_active: bool,
    pub terminal_event_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StartReceipt {
    pub job_id: u64,
    pub first_event_id: u64,
}

struct JobState {
    next_job_id: u64,
    next_event_id: u64,
    job_id: Option<u64>,
    runtime: RuntimeSnapshot,
    cancellation: Option<CancellationToken>,
    worker_active: bool,
    worker_id: Option<thread::ThreadId>,
    terminal_event_id: Option<u64>,
    history: VecDeque<JobEvent>,
}

impl Default for JobState {
    fn default() -> Self {
        Self {
            next_job_id: 0,
            next_event_id: 0,
            job_id: None,
            runtime: RuntimeSnapshot {
                lifecycle: Lifecycle::Idle,
                phase: Phase::Prepare,
                cancel_requested: false,
            },
            cancellation: None,
            worker_active: false,
            worker_id: None,
            terminal_event_id: None,
            history: VecDeque::new(),
        }
    }
}

struct Shared<E> {
    state: Mutex<JobState>,
    changed: Condvar,
    executor: Arc<E>,
}

/// Owns at most one installation worker and its event history.
pub(crate) struct JobSupervisor<E: PhaseExecutor> {
    shared: Arc<Shared<E>>,
}

impl<E: PhaseExecutor> Clone for JobSupervisor<E> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<E: PhaseExecutor> JobSupervisor<E> {
    pub(crate) fn new(executor: E) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(JobState::default()),
                changed: Condvar::new(),
                executor: Arc::new(executor),
            }),
        }
    }

    /// Claim the slot and start one worker.  The claim happens before the
    /// thread is spawned, so concurrent callers cannot both start a job.
    pub(crate) fn start(&self) -> Result<StartReceipt, String> {
        let (job_id, first_event_id) = {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| "installer job state is unavailable".to_string())?;
            if state.worker_active {
                return Err("An installation is already running.".to_string());
            }

            state.next_job_id = state.next_job_id.saturating_add(1);
            let job_id = state.next_job_id;
            state.job_id = Some(job_id);
            state.runtime = RuntimeSnapshot {
                lifecycle: Lifecycle::Validated,
                phase: Phase::Prepare,
                cancel_requested: false,
            };
            state.cancellation = Some(CancellationToken::default());
            state.worker_active = true;
            state.worker_id = None;
            state.terminal_event_id = None;
            (job_id, state.next_event_id.saturating_add(1))
        };
        self.shared.changed.notify_all();

        if let Err(error) = self.shared.executor.record_job_started(job_id) {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| "installer job state is unavailable".to_string())?;
            state.worker_active = false;
            state.worker_id = None;
            state.cancellation = None;
            state.runtime.lifecycle = Lifecycle::Failed;
            state.runtime.cancel_requested = false;
            self.shared.changed.notify_all();
            return Err(format!(
                "could not persist installer job correlation: {error}"
            ));
        }

        let shared = Arc::clone(&self.shared);
        let spawn_result = thread::Builder::new()
            .name("kyth-installer-job".to_string())
            .spawn(move || run_worker(shared, job_id));
        if let Err(error) = spawn_result {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| "installer job state is unavailable".to_string())?;
            state.worker_active = false;
            state.worker_id = None;
            state.cancellation = None;
            state.runtime.lifecycle = Lifecycle::Failed;
            state.runtime.cancel_requested = false;
            return Err(format!("could not start installer worker: {error}"));
        }
        Ok(StartReceipt {
            job_id,
            first_event_id,
        })
    }

    /// Request cancellation without depending on an SSE subscriber.
    pub(crate) fn cancel(&self) -> Result<(), String> {
        let token = {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| "installer job state is unavailable".to_string())?;
            if !state.worker_active {
                return Err("No installation is running to cancel.".to_string());
            }
            state.runtime.cancel_requested = true;
            state.cancellation.clone()
        };
        if let Some(token) = token {
            token.cancel();
        }
        self.shared.changed.notify_all();
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> Result<JobSnapshot, String> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| "installer job state is unavailable".to_string())?;
        Ok(JobSnapshot {
            job_id: state.job_id,
            runtime: state.runtime.clone(),
            worker_active: state.worker_active,
            terminal_event_id: state.terminal_event_id,
        })
    }

    /// Return all retained events after `last_event_id`.
    pub(crate) fn replay(&self, last_event_id: u64) -> Result<EventReplay, String> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| "installer job state is unavailable".to_string())?;
        Ok(replay_locked(&state, last_event_id))
    }

    /// Wait until a newer event is available or the timeout expires.
    pub(crate) fn wait_for_events(
        &self,
        last_event_id: u64,
        timeout: Duration,
    ) -> Result<EventReplay, String> {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| "installer job state is unavailable".to_string())?;
        loop {
            let replay = replay_locked(&state, last_event_id);
            if !replay.events.is_empty() || replay.reset_required {
                return Ok(replay);
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(replay);
            };
            let (next_state, wait_result) = self
                .shared
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "installer job state is unavailable".to_string())?;
            state = next_state;
            if wait_result.timed_out() {
                return Ok(replay_locked(&state, last_event_id));
            }
        }
    }

    /// Wait for the worker to release the slot, useful for deterministic
    /// tests and for a caller that needs cleanup completion before retrying.
    pub(crate) fn wait_for_terminal(&self, timeout: Duration) -> Result<JobSnapshot, String> {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| "installer job state is unavailable".to_string())?;
        while state.worker_active {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            let (next_state, wait_result) = self
                .shared
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "installer job state is unavailable".to_string())?;
            state = next_state;
            if wait_result.timed_out() {
                break;
            }
        }
        Ok(JobSnapshot {
            job_id: state.job_id,
            runtime: state.runtime.clone(),
            worker_active: state.worker_active,
            terminal_event_id: state.terminal_event_id,
        })
    }
}

fn replay_locked(state: &JobState, last_event_id: u64) -> EventReplay {
    let reset_required = state
        .history
        .front()
        .is_some_and(|event| last_event_id.saturating_add(1) < event.id);
    EventReplay {
        events: state
            .history
            .iter()
            .filter(|event| event.id > last_event_id)
            .cloned()
            .collect(),
        next_event_id: state.next_event_id,
        reset_required,
    }
}

fn append_event(state: &mut JobState, kind: JobEventKind) -> u64 {
    state.next_event_id = state.next_event_id.saturating_add(1);
    let id = state.next_event_id;
    state.history.push_back(JobEvent { id, kind });
    while state.history.len() > MAX_EVENT_HISTORY {
        state.history.pop_front();
    }
    id
}

fn run_worker<E: PhaseExecutor>(shared: Arc<Shared<E>>, job_id: u64) {
    let token = {
        let Ok(mut state) = shared.state.lock() else {
            return;
        };
        if state.job_id != Some(job_id) || !state.worker_active {
            return;
        }
        state.worker_id = Some(thread::current().id());
        state.runtime.lifecycle = Lifecycle::Installing;
        state.cancellation.clone().unwrap_or_default()
    };
    shared.changed.notify_all();

    for phase in PHASES {
        if token.is_cancelled() {
            shared.executor.record_cancelled(Some(phase));
            finish_cancelled(&shared, job_id, Some(phase));
            return;
        }
        {
            let Ok(mut state) = shared.state.lock() else {
                return;
            };
            if state.job_id != Some(job_id) || !state.worker_active {
                return;
            }
            state.runtime.phase = phase;
            append_event(&mut state, JobEventKind::Phase { phase });
        }
        shared.changed.notify_all();

        let result = shared.executor.execute_phase(phase, &token);
        if token.is_cancelled() {
            shared.executor.record_cancelled(Some(phase));
            finish_cancelled(&shared, job_id, Some(phase));
            return;
        }
        if let Err(error) = result {
            shared.executor.record_failed(phase, &error);
            finish_error(&shared, job_id, phase, error);
            return;
        }
    }

    let Ok(mut state) = shared.state.lock() else {
        return;
    };
    if state.job_id != Some(job_id) || !state.worker_active {
        return;
    }
    // Re-check under the state lock.  A cancel request can arrive after the
    // executor returns and before the worker records success; that request
    // must win instead of being overwritten by `done`.
    if state.runtime.cancel_requested || token.is_cancelled() {
        drop(state);
        shared.executor.record_cancelled(Some(Phase::Complete));
        finish_cancelled(&shared, job_id, Some(Phase::Complete));
        return;
    }
    let mok_state = shared.executor.success_mok_state();
    state.runtime.lifecycle = Lifecycle::Done;
    state.runtime.phase = Phase::Complete;
    state.runtime.cancel_requested = false;
    state.cancellation = None;
    state.worker_active = false;
    state.worker_id = None;
    state.terminal_event_id = Some(append_event(&mut state, JobEventKind::Done { mok_state }));
    shared.changed.notify_all();
}

fn finish_cancelled<E: PhaseExecutor>(shared: &Arc<Shared<E>>, job_id: u64, phase: Option<Phase>) {
    finish_terminal(
        shared,
        job_id,
        JobEventKind::Error {
            message: CANCELLATION_MESSAGE.to_string(),
            phase,
            cancelled: true,
        },
    );
}

fn finish_error<E: PhaseExecutor>(
    shared: &Arc<Shared<E>>,
    job_id: u64,
    phase: Phase,
    error: String,
) {
    finish_terminal(
        shared,
        job_id,
        JobEventKind::Error {
            message: error,
            phase: Some(phase),
            cancelled: false,
        },
    );
}

fn finish_terminal<E: PhaseExecutor>(shared: &Arc<Shared<E>>, job_id: u64, event: JobEventKind) {
    let Ok(mut state) = shared.state.lock() else {
        return;
    };
    if state.job_id != Some(job_id) || !state.worker_active {
        return;
    }
    state.runtime.lifecycle = Lifecycle::Failed;
    state.runtime.cancel_requested = false;
    state.cancellation = None;
    state.worker_active = false;
    state.worker_id = None;
    state.terminal_event_id = Some(append_event(&mut state, event));
    shared.changed.notify_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;

    #[derive(Default)]
    struct FakeExecutor {
        calls: Mutex<Vec<Phase>>,
        fail_phase: Mutex<Option<Phase>>,
        block_phase: Mutex<Option<Phase>>,
        entered: Condvar,
        entered_flag: Mutex<bool>,
        release: AtomicBool,
        cleanup_complete: AtomicBool,
        instances: AtomicUsize,
        mok_state: Mutex<Option<String>>,
    }

    impl FakeExecutor {
        fn fail_at(&self, phase: Phase) {
            *self.fail_phase.lock().unwrap() = Some(phase);
        }

        fn block_at(&self, phase: Phase) {
            *self.block_phase.lock().unwrap() = Some(phase);
        }

        fn release(&self) {
            self.release.store(true, Ordering::Release);
            self.entered.notify_all();
        }

        fn set_mok_state(&self, state: &str) {
            *self.mok_state.lock().unwrap() = Some(state.to_string());
        }

        fn wait_until_entered(&self) {
            let mut entered = self.entered_flag.lock().unwrap();
            while !*entered {
                entered = self.entered.wait(entered).unwrap();
            }
        }
    }

    impl PhaseExecutor for FakeExecutor {
        fn execute_phase(
            &self,
            phase: Phase,
            cancellation: &CancellationToken,
        ) -> Result<(), String> {
            self.instances.fetch_add(1, Ordering::Relaxed);
            self.calls.lock().unwrap().push(phase);
            {
                let mut entered = self.entered_flag.lock().unwrap();
                *entered = true;
                self.entered.notify_all();
            }
            if self.block_phase.lock().unwrap().as_ref() == Some(&phase) {
                while !self.release.load(Ordering::Acquire) && !cancellation.is_cancelled() {
                    std::thread::yield_now();
                }
                // Simulate cleanup before returning from a cancelled child.
                self.cleanup_complete.store(true, Ordering::Release);
            }
            if self.fail_phase.lock().unwrap().as_ref() == Some(&phase) {
                return Err(format!("fake failure in {phase:?}"));
            }
            Ok(())
        }
    }

    fn supervisor() -> (JobSupervisor<ArcExecutor>, Arc<FakeExecutor>) {
        let executor = Arc::new(FakeExecutor::default());
        let supervisor = JobSupervisor::new(ArcExecutor(Arc::clone(&executor)));
        (supervisor, executor)
    }

    struct ArcExecutor(Arc<FakeExecutor>);

    impl PhaseExecutor for ArcExecutor {
        fn execute_phase(
            &self,
            phase: Phase,
            cancellation: &CancellationToken,
        ) -> Result<(), String> {
            self.0.execute_phase(phase, cancellation)
        }

        fn success_mok_state(&self) -> Option<String> {
            self.0.mok_state.lock().unwrap().clone()
        }
    }

    fn terminal(supervisor: &JobSupervisor<ArcExecutor>) -> JobSnapshot {
        supervisor
            .wait_for_terminal(Duration::from_secs(2))
            .unwrap()
    }

    #[test]
    fn concurrent_start_has_exactly_one_worker() {
        let (supervisor, executor) = supervisor();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let (sender, receiver) = mpsc::channel();
        for _ in 0..2 {
            let supervisor = supervisor.clone();
            let barrier = Arc::clone(&barrier);
            let sender = sender.clone();
            thread::spawn(move || {
                barrier.wait();
                sender.send(supervisor.start().is_ok()).unwrap();
            });
        }
        barrier.wait();
        let accepted = receiver.recv().unwrap() as usize + receiver.recv().unwrap() as usize;
        executor.release();
        terminal(&supervisor);
        assert_eq!(accepted, 1);
        assert_eq!(executor.calls.lock().unwrap().len(), PHASES.len());
    }

    #[test]
    fn completion_does_not_depend_on_subscribers() {
        let (supervisor, executor) = supervisor();
        let receipt = supervisor.start().unwrap();
        executor.release();
        let snapshot = terminal(&supervisor);
        assert_eq!(snapshot.runtime.lifecycle, Lifecycle::Done);
        assert!(!snapshot.worker_active);
        let replay = supervisor
            .replay(receipt.first_event_id.saturating_sub(1))
            .unwrap();
        assert_eq!(
            replay.events.last().unwrap().kind,
            JobEventKind::Done { mok_state: None }
        );
    }

    #[test]
    fn successful_terminal_event_carries_executor_mok_state() {
        let (supervisor, executor) = supervisor();
        executor.set_mok_state("staged");
        supervisor.start().unwrap();
        executor.release();
        terminal(&supervisor);

        let event = supervisor.replay(0).unwrap().events.pop().unwrap();
        assert_eq!(
            event.kind,
            JobEventKind::Done {
                mok_state: Some("staged".to_string())
            }
        );
        let encoded = serde_json::to_value(event).unwrap();
        assert_eq!(encoded["type"], "done");
        assert_eq!(encoded["mok_state"], "staged");
    }

    #[test]
    fn replay_has_stable_monotonic_ids_and_does_not_reapply_state() {
        let (supervisor, executor) = supervisor();
        supervisor.start().unwrap();
        executor.release();
        terminal(&supervisor);
        let first = supervisor.replay(0).unwrap();
        let second = supervisor.replay(0).unwrap();
        assert_eq!(first, second);
        assert!(first
            .events
            .windows(2)
            .all(|events| events[0].id < events[1].id));
        assert!(first
            .events
            .iter()
            .any(|event| matches!(event.kind, JobEventKind::Done { .. })));
        assert_eq!(
            supervisor.snapshot().unwrap().runtime.lifecycle,
            Lifecycle::Done
        );
    }

    #[test]
    fn cancellation_before_work_emits_one_terminal_event_and_runs_no_phase() {
        let (supervisor, executor) = supervisor();
        supervisor.start().unwrap();
        supervisor.cancel().unwrap();
        let snapshot = terminal(&supervisor);
        assert_eq!(snapshot.runtime.lifecycle, Lifecycle::Failed);
        assert!(!snapshot.runtime.cancel_requested);
        assert!(executor.calls.lock().unwrap().is_empty());
        let events = supervisor.replay(0).unwrap().events;
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.kind, JobEventKind::Error { .. }))
                .count(),
            1
        );
        assert!(matches!(
            events.last().unwrap().kind,
            JobEventKind::Error {
                cancelled: true,
                ..
            }
        ));
    }

    #[test]
    fn cancellation_during_phase_waits_for_executor_cleanup() {
        let (supervisor, executor) = supervisor();
        executor.block_at(Phase::Storage);
        supervisor.start().unwrap();
        executor.wait_until_entered();
        supervisor.cancel().unwrap();
        let snapshot = terminal(&supervisor);
        assert!(!snapshot.worker_active);
        assert!(executor.cleanup_complete.load(Ordering::Acquire));
        assert_eq!(snapshot.runtime.lifecycle, Lifecycle::Failed);
        assert_eq!(
            supervisor
                .replay(0)
                .unwrap()
                .events
                .iter()
                .filter(|event| matches!(event.kind, JobEventKind::Error { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn failure_stops_at_the_failing_phase_and_has_one_terminal_event() {
        let (supervisor, executor) = supervisor();
        executor.fail_at(Phase::Configure);
        supervisor.start().unwrap();
        let snapshot = terminal(&supervisor);
        assert_eq!(snapshot.runtime.lifecycle, Lifecycle::Failed);
        assert_eq!(snapshot.runtime.phase, Phase::Configure);
        assert_eq!(
            executor.calls.lock().unwrap().as_slice(),
            &[
                Phase::Prepare,
                Phase::Storage,
                Phase::Image,
                Phase::Configure
            ]
        );
        let events = supervisor.replay(0).unwrap().events;
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.kind, JobEventKind::Error { .. }))
                .count(),
            1
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event.kind, JobEventKind::Done { .. })));
    }
}
