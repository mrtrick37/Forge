//! Session snapshots: environment diagnostics plus config notes.
//!
//! Mirrors `kyth_shared.session.generate_session_snapshot`: a header with
//! local timestamps, command-capture sections, existing gaming paths, gated
//! KythOS check commands, and static restore notes. The command runner is
//! injected so tests stay deterministic; only the `*_bin.rs` entry point
//! runs real commands.

use std::path::{Path, PathBuf};

pub const SYSTEM_COMMANDS: &[&[&str]] = &[
    &["cat", "/etc/os-release"],
    &["uname", "-a"],
    &["bootc", "status"],
];
pub const DESKTOP_COMMANDS: &[&[&str]] = &[
    &["xdg-user-dir"],
    &[
        "xdg-mime",
        "query",
        "default",
        "application/x-ms-dos-executable",
    ],
    &["xdg-mime", "query", "default", "application/x-msi"],
];
pub const FLATPAK_COMMANDS: &[&[&str]] = &[&[
    "flatpak",
    "list",
    "--app",
    "--columns=application,name,version,branch",
]];
pub const KYTH_COMMANDS: &[&str] = &[
    "kyth-controller-check",
    "kyth-resume-check",
    "kyth-nvidia-status",
];

pub const RESTORE_NOTES: &str = "- Reinstall Flatpaks with: flatpak install flathub APP_ID\n- Keep source code, saves, and documents in /home or synced storage.\n- System image changes are handled through KythOS updates and bootc rollback.\n- Do not paste this file publicly without reviewing paths and usernames.\n";

pub fn default_out_dir(home: &Path) -> PathBuf {
    home.join("Documents/KythOS")
}

pub fn section_header(title: &str) -> String {
    format!("\n== {title} ==\n")
}

pub fn render_header(now_iso: &str, user: &str, host: &str) -> String {
    format!("KythOS Session Snapshot\nGenerated: {now_iso}\nUser: {user}\nHost: {host}\n")
}

pub fn snapshot_name(timestamp: &str) -> String {
    format!("kyth-session-snapshot-{timestamp}.txt")
}

pub fn gaming_paths(home: &Path) -> Vec<PathBuf> {
    [
        ".local/share/Steam/steamapps",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps",
        "Games",
        "Applications",
    ]
    .iter()
    .map(|suffix| home.join(suffix))
    .collect()
}

fn append(out: &Path, text: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
    {
        let _ = file.write_all(text.as_bytes());
    }
}

/// Runs the snapshot, mirroring `generate_session_snapshot` section for
/// section. `runner` returns `(stdout, stderr)` or an error string, exactly
/// like the Python `run_cmd` closure observed it.
pub fn snapshot(
    home: &Path,
    out_dir: Option<&Path>,
    timestamp: &str,
    now_iso: &str,
    user: &str,
    host: &str,
    runner: &dyn Fn(&[String]) -> Result<(String, String), String>,
    on_path: &dyn Fn(&str) -> bool,
) -> PathBuf {
    let out_dir = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_out_dir(home));
    let _ = std::fs::create_dir_all(&out_dir);
    let out = out_dir.join(snapshot_name(timestamp));
    let _ = std::fs::write(&out, render_header(now_iso, user, host));
    let run_cmd = |args: &[&str]| {
        let argv: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        append(&out, &format!("$ {}\n", argv.join(" ")));
        match runner(&argv) {
            Ok((stdout, stderr)) => {
                let mut output = stdout;
                if !stderr.is_empty() {
                    output.push_str(&stderr);
                }
                append(&out, &output);
                if !output.ends_with('\n') {
                    append(&out, "\n");
                }
            }
            Err(error) => append(&out, &format!("Execution failed: {error}\n")),
        }
    };
    append(&out, &section_header("System"));
    for command in SYSTEM_COMMANDS {
        run_cmd(command);
    }
    append(&out, &section_header("Desktop"));
    for command in DESKTOP_COMMANDS {
        run_cmd(command);
    }
    append(&out, &section_header("Installed Flatpaks"));
    for command in FLATPAK_COMMANDS {
        run_cmd(command);
    }
    append(&out, &section_header("Gaming Paths"));
    let mut existing = String::new();
    for path in gaming_paths(home) {
        if path.exists() {
            existing.push_str(&format!("{}\n", path.display()));
        }
    }
    append(&out, &existing);
    append(&out, &section_header("KythOS Checks"));
    for command in KYTH_COMMANDS {
        if on_path(command) {
            run_cmd(&[*command]);
        }
    }
    append(&out, &section_header("Restore Notes"));
    append(&out, RESTORE_NOTES);
    out
}

/// Local ISO-8601 timestamp with microseconds and colon offset, mirroring
/// `datetime.now().astimezone().isoformat()`.
pub fn now_iso() -> String {
    let mut tv = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    unsafe { libc::gettimeofday(&mut tv, std::ptr::null_mut()) };
    let mut broken = unsafe { std::mem::zeroed::<libc::tm>() };
    unsafe { libc::localtime_r(&tv.tv_sec, &mut broken) };
    let offset = broken.tm_gmtoff;
    let sign = if offset < 0 { '-' } else { '+' };
    let offset = offset.abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}{}{:02}:{:02}",
        broken.tm_year + 1900,
        broken.tm_mon + 1,
        broken.tm_mday,
        broken.tm_hour,
        broken.tm_min,
        broken.tm_sec,
        tv.tv_usec as i64,
        sign,
        offset / 3600,
        (offset % 3600) / 60
    )
}

pub fn current_user() -> String {
    for variable in ["LOGNAME", "USER", "LNAME", "USERNAME"] {
        if let Ok(value) = std::env::var(variable) {
            if !value.is_empty() {
                return value;
            }
        }
    }
    let mut buffer = vec![0u8; 256];
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut entry = unsafe { std::mem::zeroed::<libc::passwd>() };
    let found = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buffer.as_mut_ptr() as *mut libc::c_char,
            buffer.len(),
            &mut result,
        )
    };
    if found == 0 && !result.is_null() {
        let name = unsafe { std::ffi::CStr::from_ptr((*result).pw_name) }
            .to_string_lossy()
            .into_owned();
        if !name.is_empty() {
            return name;
        }
    }
    "unknown".to_string()
}

pub fn current_host() -> String {
    let mut buffer = vec![0u8; 256];
    if unsafe { libc::gethostname(buffer.as_mut_ptr() as *mut libc::c_char, buffer.len()) } == 0 {
        let end = buffer
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(buffer.len());
        let host = String::from_utf8_lossy(&buffer[..end]).into_owned();
        if !host.is_empty() {
            return host;
        }
    }
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn renders_snapshot_structure_with_stubbed_commands() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("Games")).unwrap();
        let out = snapshot(
            &home,
            Some(dir.path()),
            "20240101-000000",
            "2024-01-01T00:00:00.000000+00:00",
            "tester",
            "testhost",
            &|argv| Ok((format!("out:{}\n", argv.join(" ")), String::new())),
            &|_| false,
        );
        assert_eq!(
            out.file_name().unwrap(),
            "kyth-session-snapshot-20240101-000000.txt"
        );
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.starts_with("KythOS Session Snapshot\nGenerated: 2024-01-01T00:00:00.000000+00:00\nUser: tester\nHost: testhost\n"));
        for section in [
            "System",
            "Desktop",
            "Installed Flatpaks",
            "Gaming Paths",
            "KythOS Checks",
            "Restore Notes",
        ] {
            assert!(text.contains(&format!("\n== {section} ==\n")), "{section}");
        }
        assert!(text.contains("Games\n"));
        assert!(text.contains("$ flatpak list --app --columns=application,name,version,branch\n"));
        assert!(text.contains(RESTORE_NOTES));
    }

    #[test]
    fn records_runner_failures_like_python() {
        let dir = tempdir().unwrap();
        let out = snapshot(
            dir.path(),
            Some(dir.path()),
            "20240101-000000",
            "now",
            "u",
            "h",
            &|_| Err("boom".to_string()),
            &|_| true,
        );
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("Execution failed: boom\n"));
        assert!(text.contains("$ kyth-controller-check\n"));
    }
}
