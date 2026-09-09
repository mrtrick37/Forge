//! Rust implementation of the staged-deployment boot preparation/finalizer.

use std::path::Path;
use std::process::Output;
use std::time::Duration;

fn run(program: &str, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    crate::system::process::run_bounded(&argv, timeout)
        .map_err(|error| format!("{program} could not run: {error}"))
}

fn successful(program: &str, args: &[&str], timeout: Duration) -> bool {
    run(program, args, timeout).is_ok_and(|output| output.status.success())
}

pub fn prepare_boot() -> Result<(), String> {
    let remounted = successful(
        "mount",
        &["-o", "remount,bind,rw", "/boot"],
        Duration::from_secs(15),
    ) || successful(
        "mount",
        &["-o", "remount,rw", "/boot"],
        Duration::from_secs(15),
    );
    if !remounted {
        return Err("could not remount /boot read-write".to_string());
    }
    let sysroot_boot = Path::new("/sysroot/boot");
    if sysroot_boot.is_dir()
        && !successful("findmnt", &["-n", "/sysroot/boot"], Duration::from_secs(5))
    {
        let _ = run(
            "mount",
            &["--bind", "/boot", "/sysroot/boot"],
            Duration::from_secs(15),
        );
    }
    Ok(())
}

pub fn finalize_staged(reboot: bool) -> Result<String, String> {
    if let Err(error) = prepare_boot() {
        eprintln!("kyth-finalize-staged: {error}; trying finalize anyway");
    }
    let output = run(
        "/usr/bin/ostree",
        &["admin", "finalize-staged"],
        Duration::from_secs(120),
    )?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    if reboot {
        let reboot_result = run("/usr/bin/systemctl", &["reboot"], Duration::from_secs(30))?;
        if !reboot_result.status.success() {
            return Err(String::from_utf8_lossy(&reboot_result.stderr)
                .trim()
                .to_string());
        }
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
