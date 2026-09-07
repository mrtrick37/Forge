//! Native replacement for the Python `kyth-sched` launcher.
//!
//! Automatic sched-ext profile switcher: polls gaming activity, enables
//! the gaming scheduler (plus performance-mode integration) while gaming,
//! and restores the desktop scheduler otherwise. SIGHUP reloads config,
//! SIGUSR1 forces gaming, SIGUSR2 clears the override. Always exits `0`.
//! `gaming.py` and `daemon.py` stay as fixtures.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kyth_shared::system::process::{run_bounded, run_bounded_command};
use kyth_shared::system::sched_daemon::{
    GamingCache, SchedEffect, SchedState, current_scheduler, gamescope_active,
    load_sched_config, poll_step, proc_gaming_active, session_uids, set_scheduler,
    write_status,
};

static RUNNING: AtomicBool = AtomicBool::new(true);
static WAKE: AtomicBool = AtomicBool::new(false);
static RELOAD: AtomicBool = AtomicBool::new(false);
static OVERRIDE: AtomicU8 = AtomicU8::new(0);

extern "C" fn on_hup(_signum: libc::c_int) {
    RELOAD.store(true, Ordering::SeqCst);
    WAKE.store(true, Ordering::SeqCst);
}

extern "C" fn on_term(_signum: libc::c_int) {
    RUNNING.store(false, Ordering::SeqCst);
    WAKE.store(true, Ordering::SeqCst);
}

extern "C" fn on_force_gaming(_signum: libc::c_int) {
    OVERRIDE.store(1, Ordering::SeqCst);
    WAKE.store(true, Ordering::SeqCst);
}

extern "C" fn on_force_desktop(_signum: libc::c_int) {
    OVERRIDE.store(2, Ordering::SeqCst);
    WAKE.store(true, Ordering::SeqCst);
}

fn log(message: &str) {
    eprintln!("kyth-sched: {message}");
}

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn run(argv: &[String], timeout_secs: u64) -> Option<(i32, String)> {
    run_bounded(argv, Duration::from_secs(timeout_secs)).ok().map(|output| {
        (
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    })
}

fn query_gamemode(uid: u32) -> bool {
    if !on_path("busctl") {
        return false;
    }
    let address = format!("unix:path=/run/user/{uid}/bus");
    let mut command = std::process::Command::new("busctl");
    command
        .args([
            "--user",
            "--address",
            &address,
            "call",
            "com.feralinteractive.GameMode",
            "/com/feralinteractive/GameMode",
            "com.feralinteractive.GameMode",
            "QueryStatus",
            "i",
            "0",
        ])
        .env("DBUS_SESSION_BUS_ADDRESS", &address);
    match run_bounded_command(command, Duration::from_secs(3)) {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            text.split_whitespace().last().and_then(|value| value.parse::<i64>().ok()).is_some_and(|value| value > 0)
        }
        _ => false,
    }
}

fn current_uid() -> u32 {
    unsafe { libc::geteuid() as u32 }
}

fn runtime_dir() -> PathBuf {
    if let Some(dir) = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        dir
    } else {
        PathBuf::from(format!("/run/user/{}", current_uid()))
    }
}

fn spawn_perf_mode(mode: &str) {
    if !Path::new("/usr/bin/kyth-performance-mode").exists() && !on_path("kyth-performance-mode") {
        return;
    }
    let _ = std::process::Command::new("/usr/bin/kyth-performance-mode")
        .arg(mode)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn enter_gaming_perf_mode() {
    if on_path("kyth-performance-mode") || Path::new("/usr/bin/kyth-performance-mode").exists() {
        if let Ok(mut child) = std::process::Command::new("/usr/bin/kyth-performance-mode")
            .arg("save")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            let started = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if started.elapsed() <= Duration::from_secs(5) => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
    }
    spawn_perf_mode("gaming");
}

fn gaming_detected(cache: &mut GamingCache, now: f64) -> bool {
    let uid = current_uid();
    let mut uids = session_uids(&run);
    if !uids.contains(&uid) {
        uids.push(uid);
    }
    let gamescope: Vec<u32> = uids.iter().copied().filter(|id| gamescope_active(*id)).collect();
    let gamemode: Vec<u32> = uids.iter().copied().filter(|id| query_gamemode(*id)).collect();
    let proc_active = proc_gaming_active();
    cache.check(now, Some(uid), false, &uids, &gamescope, &gamemode, proc_active, uid).is_some()
}

fn main() -> std::process::ExitCode {
    unsafe {
        libc::signal(libc::SIGHUP, on_hup as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_term as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as libc::sighandler_t);
        libc::signal(libc::SIGUSR1, on_force_gaming as libc::sighandler_t);
        libc::signal(libc::SIGUSR2, on_force_desktop as libc::sighandler_t);
    }
    let mut config = load_sched_config();
    log("Configuration loaded/reloaded.");
    log("Starting kyth-sched...");
    set_scheduler(&run, &config.desktop_scheduler);
    let poll = config.poll_interval;
    log(&format!(
        "Started — desktop={}  gaming={}  poll={}s",
        config.desktop_scheduler,
        config.gaming_scheduler,
        poll as i64
    ));
    let runtime = runtime_dir();
    let mut state = SchedState::new();
    let mut cache = GamingCache::new();
    let started = Instant::now();
    while RUNNING.load(Ordering::SeqCst) {
        if RELOAD.swap(false, Ordering::SeqCst) {
            log("SIGHUP received. Reloading configuration...");
            config = load_sched_config();
            log("Configuration loaded/reloaded.");
        }
        match OVERRIDE.swap(0, Ordering::SeqCst) {
            1 => {
                log("Manual override → gaming (SIGUSR1)");
                state.manual_override = Some("gaming".to_string());
            }
            2 => {
                log("Manual override cleared (SIGUSR2)");
                state.manual_override = None;
            }
            _ => {}
        }
        WAKE.store(false, Ordering::SeqCst);
        let gaming_now = gaming_detected(&mut cache, started.elapsed().as_secs_f64());
        for effect in poll_step(&mut state, &config, gaming_now) {
            match effect {
                SchedEffect::SetScheduler(name) => {
                    if set_scheduler(&run, &name) {
                        log(&format!("Scheduler → {name}"));
                    } else {
                        log(&format!("kyth-scx set {name} failed — scx_loader may not be running"));
                    }
                    if gaming_now {
                        log(&format!("→ gaming profile ({})", config.gaming_scheduler));
                    } else {
                        log(&format!("→ desktop profile ({})", config.desktop_scheduler));
                    }
                }
                SchedEffect::EnterGamingPerf => enter_gaming_perf_mode(),
                SchedEffect::RestorePerf => spawn_perf_mode("restore"),
            }
        }
        let scheduler = current_scheduler(
            || {
                std::fs::read_to_string("/sys/kernel/sched_ext/root/ops")
                    .ok()
                    .map(|text| text.trim().to_string())
            },
            &run,
        );
        let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|span| span.as_secs()).unwrap_or(0);
        write_status(&runtime, &state.profile, &scheduler, gaming_now, state.manual_override.is_some(), now_secs);
        let steps = (config.poll_interval * 2.0) as i64;
        for _ in 0..steps.max(1) {
            if !RUNNING.load(Ordering::SeqCst) || WAKE.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    log("kyth-sched stopped.");
    std::process::ExitCode::SUCCESS
}
