//! Rust-owned installer job state and event projection.
//!
//! The privileged daemon owns both the externally visible job lifecycle and
//! the native phase executor. This keeps start/cancel races, terminal
//! classification, and event ordering out of any UI transport adapter.
//! Durable transaction state remains the source of truth across restarts.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Lifecycle {
    Idle,
    Validated,
    Partitioning,
    Installing,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Prepare,
    Storage,
    Image,
    Configure,
    SecureBoot,
    Complete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct RuntimeSnapshot {
    pub lifecycle: Lifecycle,
    pub phase: Phase,
    pub cancel_requested: bool,
}

#[derive(Debug)]
struct RuntimeState {
    lifecycle: Lifecycle,
    phase: Phase,
    cancel_requested: bool,
}

pub(crate) struct RuntimeCoordinator {
    state: Mutex<RuntimeState>,
}

impl Default for RuntimeCoordinator {
    fn default() -> Self {
        Self {
            state: Mutex::new(RuntimeState {
                lifecycle: Lifecycle::Idle,
                phase: Phase::Prepare,
                cancel_requested: false,
            }),
        }
    }
}

impl RuntimeCoordinator {
    fn with_state<T>(
        &self,
        operation: impl FnOnce(&mut RuntimeState) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "installer runtime state is unavailable".to_string())?;
        operation(&mut state)
    }

    pub(crate) fn claim_start(&self) -> Result<(), String> {
        self.with_state(|state| {
            if matches!(
                state.lifecycle,
                Lifecycle::Validated | Lifecycle::Partitioning | Lifecycle::Installing
            ) {
                return Err("An installation is already running.".to_string());
            }
            state.lifecycle = Lifecycle::Validated;
            state.phase = Phase::Prepare;
            state.cancel_requested = false;
            Ok(())
        })
    }

    pub(crate) fn start_accepted(&self) -> Result<(), String> {
        self.with_state(|state| {
            if state.lifecycle != Lifecycle::Validated {
                return Err("installer start response arrived out of order".to_string());
            }
            state.lifecycle = Lifecycle::Installing;
            Ok(())
        })
    }

    pub(crate) fn start_rejected(&self) -> Result<(), String> {
        self.with_state(|state| {
            if state.lifecycle == Lifecycle::Validated {
                state.lifecycle = Lifecycle::Idle;
                state.cancel_requested = false;
            }
            Ok(())
        })
    }

    pub(crate) fn claim_cancel(&self) -> Result<(), String> {
        self.with_state(|state| {
            if !matches!(
                state.lifecycle,
                Lifecycle::Validated | Lifecycle::Installing
            ) {
                return Err("No installation is running to cancel.".to_string());
            }
            state.cancel_requested = true;
            Ok(())
        })
    }

    pub(crate) fn cancel_rejected(&self) -> Result<(), String> {
        self.with_state(|state| {
            state.cancel_requested = false;
            Ok(())
        })
    }

    pub(crate) fn event(&self, event: &serde_json::Value) -> Result<(), String> {
        self.with_state(|state| {
            match event.get("type").and_then(serde_json::Value::as_str) {
                Some("phase") => {
                    let phase = event
                        .get("phase")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| "installer phase event has no phase".to_string())?;
                    if !matches!(
                        phase,
                        "prepare" | "storage" | "image" | "configure" | "secure_boot" | "complete"
                    ) {
                        return Err("installer phase event has an unknown phase".to_string());
                    }
                    if state.lifecycle == Lifecycle::Idle {
                        return Err("installer phase event arrived before start".to_string());
                    }
                    let phase = parse_phase(phase)?;
                    let current_order = phase_order(state.phase);
                    let next_order = phase_order(phase);
                    if next_order < current_order {
                        return Err(format!(
                            "installer phase moved backwards: {} -> {}",
                            phase_name(state.phase),
                            phase_name(phase)
                        ));
                    }
                    if matches!(state.lifecycle, Lifecycle::Done | Lifecycle::Failed) {
                        return Err(
                            "installer phase event arrived after a terminal event".to_string()
                        );
                    }
                    state.phase = phase;
                }
                Some("done") => {
                    if !matches!(
                        state.lifecycle,
                        Lifecycle::Installing | Lifecycle::Validated
                    ) {
                        return Err("installer done event arrived out of order".to_string());
                    }
                    state.lifecycle = Lifecycle::Done;
                    state.phase = Phase::Complete;
                    state.cancel_requested = false;
                }
                Some("error") => {
                    if matches!(
                        state.lifecycle,
                        Lifecycle::Idle | Lifecycle::Done | Lifecycle::Failed
                    ) {
                        return Err("installer error event arrived out of order".to_string());
                    }
                    state.lifecycle = Lifecycle::Failed;
                    state.cancel_requested = false;
                }
                Some("log") | Some("progress") | None => {}
                Some(_) => {}
            }
            Ok(())
        })
    }

    pub(crate) fn snapshot(&self) -> Result<RuntimeSnapshot, String> {
        self.with_state(|state| {
            Ok(RuntimeSnapshot {
                lifecycle: state.lifecycle,
                phase: state.phase,
                cancel_requested: state.cancel_requested,
            })
        })
    }
}

fn parse_phase(phase: &str) -> Result<Phase, String> {
    serde_json::from_value(serde_json::Value::String(phase.to_string()))
        .map_err(|_| "installer phase event has an unknown phase".to_string())
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Prepare => "prepare",
        Phase::Storage => "storage",
        Phase::Image => "image",
        Phase::Configure => "configure",
        Phase::SecureBoot => "secure_boot",
        Phase::Complete => "complete",
    }
}

fn phase_order(phase: Phase) -> u8 {
    match phase {
        Phase::Prepare => 0,
        Phase::Storage => 1,
        Phase::Image => 2,
        Phase::Configure => 3,
        Phase::SecureBoot => 4,
        Phase::Complete => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn owns_start_cancel_and_terminal_lifecycle() {
        let runtime = RuntimeCoordinator::default();
        runtime.claim_start().unwrap();
        assert_eq!(runtime.snapshot().unwrap().lifecycle, Lifecycle::Validated);
        runtime.start_accepted().unwrap();
        runtime
            .event(&json!({"type":"phase","phase":"storage"}))
            .unwrap();
        runtime.claim_cancel().unwrap();
        assert!(runtime.snapshot().unwrap().cancel_requested);
        runtime
            .event(&json!({"type":"error","message":"cancelled"}))
            .unwrap();
        assert_eq!(runtime.snapshot().unwrap().lifecycle, Lifecycle::Failed);
        assert!(!runtime.snapshot().unwrap().cancel_requested);
    }

    #[test]
    fn rejects_duplicate_start_and_out_of_order_events() {
        let runtime = RuntimeCoordinator::default();
        runtime.claim_start().unwrap();
        assert!(runtime.claim_start().is_err());
        assert!(runtime
            .event(&json!({"type":"phase","phase":"storage"}))
            .is_ok());
        assert!(runtime
            .event(&json!({"type":"phase","phase":"bogus"}))
            .is_err());
    }

    #[test]
    fn failed_backend_start_returns_to_idle_for_retry() {
        let runtime = RuntimeCoordinator::default();
        runtime.claim_start().unwrap();
        runtime.start_rejected().unwrap();
        assert_eq!(runtime.snapshot().unwrap().lifecycle, Lifecycle::Idle);
        runtime.claim_start().unwrap();
    }

    #[test]
    fn rejects_stale_phase_and_terminal_events() {
        let runtime = RuntimeCoordinator::default();
        runtime.claim_start().unwrap();
        runtime.start_accepted().unwrap();
        runtime
            .event(&json!({"type":"phase","phase":"image"}))
            .unwrap();
        assert!(runtime
            .event(&json!({"type":"phase","phase":"storage"}))
            .is_err());
        runtime.event(&json!({"type":"done"})).unwrap();
        assert!(runtime
            .event(&json!({"type":"phase","phase":"complete"}))
            .is_err());
        assert!(runtime.event(&json!({"type":"done"})).is_err());
    }

    #[test]
    fn rejects_terminal_events_before_a_start() {
        let runtime = RuntimeCoordinator::default();
        assert!(runtime.event(&json!({"type":"done"})).is_err());
        assert!(runtime
            .event(&json!({"type":"error","message":"failed"}))
            .is_err());
    }
}
