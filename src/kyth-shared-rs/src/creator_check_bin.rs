//! Native read-only creator/OBS media stack readiness report.
use kyth_shared::system::{
    process::run_bounded,
    smoke_check::{Level, Report},
};
use std::time::Duration;
fn run(p: &str, a: &[&str]) -> Option<std::process::Output> {
    let mut v = vec![p.into()];
    v.extend(a.iter().map(|s| (*s).into()));
    run_bounded(&v, Duration::from_secs(10)).ok()
}
fn main() {
    let mut r = Report::default();
    for (p, n) in [("pipewire", "PipeWire"), ("wireplumber", "WirePlumber")] {
        let o = run(p, &["--version"]);
        r.record(
            if o.is_some() {
                Level::Pass
            } else {
                Level::Warn
            },
            n,
            if let Some(o) = o {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .next()
                    .unwrap_or("present")
                    .to_string()
            } else {
                "not detected".into()
            },
            "Creator Readiness",
        );
    }
    for (p, n) in [
        ("rpm", "OBS Vulkan Capture"),
        ("vainfo", "VA-API Hardware Video"),
    ] {
        let a = if p == "rpm" {
            vec!["-q", "libobs_vkcapture"]
        } else {
            vec![]
        };
        let o = run(p, &a);
        r.record(
            if o.as_ref().is_some_and(|x| x.status.success()) {
                Level::Pass
            } else {
                Level::Warn
            },
            n,
            if p == "rpm" {
                "libobs_vkcapture installed"
            } else if o.is_some() {
                "hardware encoding/decoding available"
            } else {
                "vainfo command missing"
            },
            "Creator Readiness",
        );
    }
    let obs = run("flatpak", &["info", "com.obsproject.Studio"]);
    r.record(
        if obs.as_ref().is_some_and(|o| o.status.success()) {
            Level::Pass
        } else {
            Level::Warn
        },
        "OBS Studio Flatpak",
        if obs.is_some_and(|o| o.status.success()) {
            "installed"
        } else {
            "not installed"
        },
        "Creator Readiness",
    );
    println!("Creator Check\n");
    for x in &r.results {
        println!(
            "{:5} {:26} {}",
            format!("{:?}", x.level).to_uppercase(),
            x.name,
            x.detail
        );
    }
    println!(
        "\nResult: creator readiness {}.",
        if r.warnings() > 0 {
            "has warnings"
        } else {
            "is clean"
        }
    );
    std::process::exit(r.exit_code_with_strict(true));
}
