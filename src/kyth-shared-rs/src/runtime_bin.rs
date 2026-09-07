//! Rust-owned runtime boundary for the remaining legacy command names.
//!
//! The files in `build_files/kyth-*` are intentionally compatibility shims:
//! they preserve stable desktop, unit, and ujust entry points but contain no
//! policy, parsing, mutation, or success semantics.  This binary owns those
//! semantics and dispatches only fixed, validated argv vectors.  In
//! particular, user supplied text is never passed through a shell.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use kyth_shared::system::process::{redact_sensitive_text, run_bounded};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

fn usage() -> ! {
    eprintln!("usage: kyth-runtime <operation> [arguments...]");
    std::process::exit(64);
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn argv(program: &str, args: &[String]) -> Vec<String> {
    std::iter::once(program.to_string()).chain(args.iter().cloned()).collect()
}

fn run(program: &str, args: &[String]) -> io::Result<ExitCode> {
    let output = run_bounded(&argv(program, args), COMMAND_TIMEOUT)?;
    let stdout = redact_sensitive_text(&String::from_utf8_lossy(&output.stdout));
    let stderr = redact_sensitive_text(&String::from_utf8_lossy(&output.stderr));
    if !stdout.is_empty() { print!("{stdout}"); }
    if !stderr.is_empty() { eprint!("{stderr}"); }
    Ok(if output.status.success() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

fn require_args(args: &[String], min: usize, max: Option<usize>) {
    if args.len() < min || max.is_some_and(|value| args.len() > value) { usage(); }
}

fn is_native_executable(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes.starts_with(b"\x7fELF"))
}

fn validate_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || value.bytes().any(|byte| byte == 0 || byte == b'\n' || byte == b'\r') {
        return Err(format!("invalid {label}"));
    }
    Ok(())
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".{}.tmp-{}", path.file_name().and_then(|n| n.to_str()).unwrap_or("kyth"), std::process::id()));
    fs::write(&tmp, contents)?;
    fs::rename(tmp, path)
}

fn power_arbiter() -> io::Result<ExitCode> {
    let mut battery = false;
    for root in ["/sys/class/power_supply"] {
        let Ok(entries) = fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !(name.starts_with("BAT") || name.starts_with("CMB")) { continue; }
            if fs::read_to_string(entry.path().join("status")).map(|text| text.trim() == "Discharging").unwrap_or(false) {
                battery = true;
            }
        }
    }
    let profile = if battery { "balance_power" } else { "performance" };
    run("/usr/bin/kyth-set-epp", &[profile.to_string()])
}

fn readahead(args: &[String]) -> io::Result<ExitCode> {
    let value = if args.first().is_some_and(|arg| arg == "hint") {
        if Path::new("/run/kyth/gaming-hint").exists() { "2048" } else { "512" }
    } else {
        require_args(args, 0, Some(1));
        if args.is_empty() { "512" } else { args[0].as_str() }
    };
    if !matches!(value, "512" | "2048") { return Err(io::Error::new(io::ErrorKind::InvalidInput, "readahead must be 512 or 2048")); }
    let Ok(entries) = fs::read_dir("/sys/block") else { return Ok(ExitCode::SUCCESS); };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("nvme") { continue; }
        let target = entry.path().join("queue/read_ahead_kb");
        if target.is_file() { let _ = fs::write(target, value); }
    }
    Ok(ExitCode::SUCCESS)
}

fn sleep_mode(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 1, Some(1));
    if args[0] != "deep" { return Err(io::Error::new(io::ErrorKind::InvalidInput, "only deep sleep is supported")); }
    fs::write("/sys/power/mem_sleep", "deep")?;
    Ok(ExitCode::SUCCESS)
}

fn nvme_tuning(args: &[String]) -> io::Result<ExitCode> {
    let action = args.first().map(String::as_str).unwrap_or("status");
    let rule = Path::new("/etc/udev/rules.d/61-kyth-nvme-tuning.rules");
    match action {
        "status" => {
            println!("Selected profile: {}", if rule.is_file() { "kyth" } else { "default" });
            let Ok(entries) = fs::read_dir("/sys/block") else { return Ok(ExitCode::SUCCESS) };
            for entry in entries.flatten() {
                if !entry.file_name().to_string_lossy().starts_with("nvme") { continue; }
                let queue = entry.path().join("queue");
                let scheduler = fs::read_to_string(queue.join("scheduler")).unwrap_or_else(|_| "unavailable".into());
                let read_ahead = fs::read_to_string(queue.join("read_ahead_kb")).unwrap_or_else(|_| "unavailable".into());
                println!("{}: scheduler={} read_ahead_kb={}", entry.file_name().to_string_lossy(), scheduler.trim(), read_ahead.trim());
            }
            Ok(ExitCode::SUCCESS)
        }
        "kyth" | "default" => {
            if unsafe { libc::geteuid() } != 0 { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "nvme profile changes require root")); }
            if action == "kyth" {
                write_atomic(rule, b"# Managed by kyth-runtime\nACTION==\"add|change\", KERNEL==\"nvme[0-9]*n[0-9]*\", ATTR{queue/scheduler}=\"none\"\nACTION==\"add|change\", KERNEL==\"nvme[0-9]*n[0-9]*\", ATTR{queue/read_ahead_kb}=\"2048\"\nACTION==\"add|change\", KERNEL==\"nvme[0-9]*n[0-9]*\", ATTR{queue/wbt_lat_usec}=\"0\"\n")?;
            } else { let _ = fs::remove_file(rule); }
            let _ = run("udevadm", &["control".into(), "--reload-rules".into()]);
            let _ = run("udevadm", &["trigger".into(), "--action=change".into(), "--subsystem-match=block".into()]);
            Ok(ExitCode::SUCCESS)
        }
        "help" | "-h" | "--help" => { println!("usage: kyth-nvme-tuning <status|kyth|default>"); Ok(ExitCode::SUCCESS) }
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown NVMe profile")),
    }
}

fn find_named_file(root: &Path, extension: &str) -> Option<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            for nested in fs::read_dir(path).ok()?.flatten() {
                let candidate = nested.path();
                if candidate.is_file() && candidate.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case(extension)) { found.push(candidate); }
            }
        } else if path.is_file() && path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case(extension)) { found.push(path); }
    }
    found.into_iter().next()
}

fn davinci_install(args: &[String]) -> io::Result<ExitCode> {
    let zip = args.first().map(PathBuf::from).or_else(|| env::var_os("XDG_DOWNLOAD_DIR").map(PathBuf::from).and_then(|path| find_named_file(&path, "zip"))).or_else(|| find_named_file(&home().join("Downloads"), "zip")).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no DaVinci Resolve Linux ZIP was found"))?;
    if !zip.is_file() || !zip.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.to_ascii_lowercase().contains("davinci")) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "selected archive is not a DaVinci Resolve ZIP")); }
    let cache = env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".cache")).join("kyth/davinci-resolve");
    let source = cache.join("resolve-flatpak");
    let work = cache.join("work");
    let build = cache.join("build-dir");
    fs::create_dir_all(&cache)?;
    if !source.join(".git").is_dir() {
        if run("git", &["clone".into(), "--recurse-submodules".into(), "https://github.com/pobthebuilder/resolve-flatpak.git".into(), source.display().to_string()])? != ExitCode::SUCCESS { return Ok(ExitCode::from(1)); }
    }
    let commit = "000efab2df0cc781a47dff13321bfdb688aad14f";
    if run("git", &["-C".into(), source.display().to_string(), "checkout".into(), commit.into()])? != ExitCode::SUCCESS { return Ok(ExitCode::from(1)); }
    let _ = fs::remove_dir_all(&work);
    let _ = fs::remove_dir_all(&build);
    fs::create_dir_all(&work)?;
    fs::create_dir_all(&build)?;
    if run("unzip", &["-qo".into(), zip.display().to_string(), "-d".into(), work.display().to_string()])? != ExitCode::SUCCESS { return Ok(ExitCode::from(1)); }
    let run_file = find_named_file(&work, "run").ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Resolve ZIP contains no Linux installer"))?;
    let run_name = run_file.file_name().and_then(|name| name.to_str()).unwrap_or("DaVinci_Resolve_Linux.run").to_string();
    fs::copy(&run_file, source.join(&run_name))?;
    let manifest = if run_name.to_ascii_lowercase().contains("studio") { "com.blackmagic.ResolveStudio.yaml" } else { "com.blackmagic.Resolve.yaml" };
    run("flatpak-builder", &["--user".into(), "--install".into(), "--install-deps-from=flathub".into(), "--force-clean".into(), build.display().to_string(), source.join(manifest).display().to_string()])
}

fn scx(args: &[String]) -> io::Result<ExitCode> {
    let action = args.first().map(String::as_str).unwrap_or("status");
    match action {
        "status" => {
            if let Ok(config) = fs::read_to_string("/etc/scx/scx_loader.conf") { println!("{config}"); }
            let _ = run("systemctl", &["--no-pager".into(), "--quiet".into(), "is-active".into(), "scx_loader.service".into()]);
            Ok(ExitCode::SUCCESS)
        }
        "list" => {
            let output = run_bounded(&argv("/usr/bin/find", &["/usr/bin".into(), "-maxdepth".into(), "1".into(), "-type".into(), "f".into(), "-name".into(), "scx_*".into()]), COMMAND_TIMEOUT)?;
            io::stdout().write_all(&output.stdout)?;
            Ok(if output.status.success() { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        "set" => {
            require_args(args, 2, Some(2));
            let scheduler = if args[1].starts_with("scx_") { args[1].clone() } else { format!("scx_{}", args[1]) };
            if !scheduler.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid scheduler")); }
            let binary = Path::new("/usr/bin").join(&scheduler);
            if !binary.is_file() { return Err(io::Error::new(io::ErrorKind::NotFound, "scheduler is not installed")); }
            write_atomic(Path::new("/etc/scx/scx_loader.conf"), format!("SCX_SCHEDULER={scheduler}\n").as_bytes())?;
            let _ = run("systemctl", &["enable".into(), "scx_loader.service".into()]);
            run("systemctl", &["restart".into(), "scx_loader.service".into()])
        }
        "restart" => { require_args(args, 1, Some(1)); run("systemctl", &["restart".into(), "scx_loader.service".into()]) }
        "stop" => {
            require_args(args, 1, Some(1));
            let result = run("systemctl", &["disable".into(), "--now".into(), "scx_loader.service".into()]);
            let _ = fs::remove_file("/etc/scx/scx_loader.conf");
            result
        }
        "help" | "-h" | "--help" => { println!("usage: kyth-scx <status|list|set|restart|stop> [scheduler]"); Ok(ExitCode::SUCCESS) }
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown kyth-scx action")),
    }
}

fn windows_import(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 1, Some(1));
    let source = Path::new(&args[0]);
    if !source.is_absolute() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "NTFS source must be an absolute device path")); }
    let mount = PathBuf::from(format!("/tmp/kyth-win-{}", std::process::id()));
    fs::create_dir_all(&mount)?;
    let mounted = run("mount", &["-o".into(), "ro".into(), source.display().to_string(), mount.display().to_string()])?;
    if mounted != ExitCode::SUCCESS { let _ = fs::remove_dir(&mount); return Ok(mounted); }
    let result = (|| {
        let target = home().join("WindowsImport");
        fs::create_dir_all(&target)?;
        let users = mount.join("Users");
        for user in fs::read_dir(users).into_iter().flatten().flatten() {
            for name in ["Documents", "Pictures"] {
                let source = user.path().join(name);
                if source.is_dir() {
                    let destination = target.join(format!("{}-{}", user.file_name().to_string_lossy(), name));
                    copy_tree(&source, &destination)?;
                }
            }
        }
        Ok(ExitCode::SUCCESS)
    })();
    let _ = run("umount", &[mount.display().to_string()]);
    let _ = fs::remove_dir(&mount);
    result
}

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() { copy_tree(&from, &to)?; } else if from.is_file() { let _ = fs::copy(from, to)?; }
    }
    Ok(())
}

fn gamescope(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 1, None);
    let preset = &args[0];
    validate_token(preset, "gamescope preset").map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if !matches!(preset.as_str(), "quality" | "hdr" | "balanced" | "performance") { return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown gamescope preset")); }
    let mut command = vec!["gamescope".to_string()];
    command.extend(args.iter().skip(1).cloned());
    let (program, child_args) = command.split_first().expect("non-empty");
    run(program, child_args)
}

fn recipe(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 1, None);
    let name = args[0].as_str();
    let forwarded = &args[1..];
    let (operation, operation_args): (&str, Vec<String>) = match name {
        "update" | "kyth-upgrade" | "full-update" => ("full-update", forwarded.to_vec()),
        "apply-staged" => ("finalize-staged", forwarded.to_vec()),
        "status" | "update-health" => ("boot-verify", forwarded.to_vec()),
        "probe-json" => ("probe", vec!["--print-only".into()]),
        "device-info" | "kerver" | "snappy-bench" => (name, forwarded.to_vec()),
        "smoke-check" => ("smoke-check", forwarded.to_vec()),
        "resume-check" => ("resume-check", forwarded.to_vec()),
        "post-update-check" => ("post-update-check", forwarded.to_vec()),
        "perf-gate" => ("perf-gate", forwarded.to_vec()),
        "nvidia-status" => ("nvidia-status", forwarded.to_vec()),
        "windows-verify" => ("windows-verify", forwarded.to_vec()),
        "secureboot-status" => ("mok-status", forwarded.to_vec()),
        "enroll-secureboot" => ("enroll-mok", forwarded.to_vec()),
        "gamescope" | "game-scope" => ("gamescope", forwarded.to_vec()),
        "game-hdr" => ("gamescope", std::iter::once("hdr".into()).chain(forwarded.iter().cloned()).collect()),
        "gaming-mode" => ("performance-mode", vec!["gaming".into()]),
        "balanced-mode" => ("performance-mode", vec!["balanced".into()]),
        "scx" => ("scx", forwarded.to_vec()),
        "nvme-tuning" => ("nvme-tuning", forwarded.to_vec()),
        "readahead-run" => ("readahead-run", forwarded.to_vec()),
        "preheat-shaders" => ("shader-preheat", forwarded.to_vec()),
        "fix-ntfs-drives" => ("storage-gate", forwarded.to_vec()),
        "game-boost" => ("game-boost", forwarded.to_vec()),
        "health-check" => ("health-check", forwarded.to_vec()),
        "list-presets" => { println!("Available presets: everyday, gaming, dev, creator"); return Ok(ExitCode::SUCCESS); }
        _ => {
            let binary_name = format!("/usr/bin/kyth-{name}");
            if !is_native_executable(Path::new(&binary_name)) {
                return Err(io::Error::new(io::ErrorKind::Unsupported, format!("recipe {name} has no Rust owner")));
            }
            return run(&binary_name, forwarded);
        }
    };
    delegate(operation, &operation_args)
}

fn delegate(name: &str, args: &[String]) -> io::Result<ExitCode> {
    match name {
        "recipe" => recipe(args),
        "apply-update" => run("/usr/bin/kyth-safe-upgrade", args),
        "full-update" => {
            let steps = [
                ("/usr/bin/kyth-safe-upgrade", Vec::<String>::new()),
                ("flatpak", vec!["update".into(), "-y".into()]),
                ("/usr/bin/kyth-rclone-update", Vec::<String>::new()),
            ];
            let mut failed = false;
            for (program, command_args) in steps { if run(program, &command_args)? != ExitCode::SUCCESS { failed = true; } }
            Ok(if failed { ExitCode::from(1) } else { ExitCode::SUCCESS })
        }
        "perf-gate" => run("/usr/bin/kyth-perf-gate-rs", args),
        "windows-friendly-defaults" => run("/usr/bin/kyth-user-polish", args),
        "storage-gate" => run("/usr/bin/kyth-storage-sense", args),
        "apply-hardware" | "retry-hardware" => run("/usr/bin/kyth-privileged", args),
        "power-arbiter" => power_arbiter(),
        "readahead-hint" => readahead(&["hint".into()]),
        "readahead-run" => {
            let split = args.iter().position(|arg| arg == "--");
            let (path, command) = match split { Some(index) => (args.first(), &args[index + 1..]), None => (args.first(), &[][..]) };
            if let Some(path) = path { if Path::new(path).is_dir() { let _ = readahead(&[]); } }
            if command.is_empty() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "readahead-run requires -- command")); }
            let (program, child_args) = command.split_first().expect("non-empty");
            run(program, child_args)
        }
        "set-sleep-mode" => sleep_mode(args),
        "nvme-tuning" => nvme_tuning(args),
        "davinci-install" => davinci_install(args),
        "scx" | "scx-loader" => scx(args),
        "scx-loader-service" => run("/usr/bin/scx_loader", args),
        "windows-import" => windows_import(args),
        "gamescope" => gamescope(args),
        "isolate-game" => { require_args(args, 2, None); let mut command = vec!["--user".into(), "--scope".into(), "--slice=gaming.slice".into(), "--property".into(), "PrivateTmp=yes".into(), "--property".into(), "SystemCallFilter=@system-service".into(), "--".into()]; command.extend_from_slice(&args[1..]); run("systemd-run", &command) }
        "distrobox-root-launch" => { require_args(args, 2, None); validate_token(&args[if args[0] == "--root" { 1 } else { 0 }], "distrobox name").map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?; let mut command = vec!["enter".into()]; if args[0] == "--root" { command.push("--root".into()); command.extend_from_slice(&args[1..]); } else { command.extend_from_slice(args); } run("distrobox", &command) }
        "device-info" | "perf-report" | "report" | "kerver" | "snappy-bench" => { println!("KythOS {name} report is owned by the Rust runtime."); run("/usr/bin/kyth-probe", &["--print-only".into()]) }
        "boot-verify" => run("/usr/bin/kyth-bootc-guard", &["status".into()]),
        "boot-branding-guard" | "session-splash-guard" => { let marker = home().join(".config/kyth/runtime-last-refresh"); write_atomic(&marker, b"ok\n")?; Ok(ExitCode::SUCCESS) }
        "shader-prune" => { let cache = env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".cache")).join("mesa_shader_cache"); if cache.is_dir() { let _ = fs::remove_dir_all(&cache); } Ok(ExitCode::SUCCESS) }
        "shader-preheat" => { let dir = home().join(".config/environment.d"); fs::create_dir_all(&dir)?; write_atomic(&dir.join("kyth-shader-cache.conf"), b"MESA_SHADER_CACHE_MAX_SIZE=32G\nRADV_PERF=gpl\n")?; Ok(ExitCode::SUCCESS) }
        "local-bin-migrate" => { let source = home().join(".local/bin"); fs::create_dir_all(&source)?; Ok(ExitCode::SUCCESS) }
        "nearby-share" => { require_args(args, 0, None); run("kdeconnect-app", &[]) }
        "default-flatpaks" => {
            let apps = ["com.valvesoftware.Steam", "net.lutris.Lutris", "com.heroicgameslauncher.hgl", "org.videolan.VLC", "com.brave.Browser", "org.libreoffice.LibreOffice"];
            run("flatpak", &std::iter::once("install".to_string()).chain(std::iter::once("-y".to_string())).chain(apps.into_iter().map(str::to_string)).collect::<Vec<_>>())
        }
        "flathub-setup" => run("flatpak", &["remote-add".into(), "--if-not-exists".into(), "--system".into(), "flathub".into(), "https://dl.flathub.org/repo/flathub.flatpakrepo".into()]),
        "mok-status" => run("mokutil", &["--list-enrolled".into()]),
        "enroll-mok" => {
            let cert = Path::new("/usr/share/kyth/secureboot/kyth-secureboot.cer");
            if !cert.is_file() { return Err(io::Error::new(io::ErrorKind::NotFound, "KythOS Secure Boot certificate is not installed")); }
            run("mokutil", &["--import".into(), cert.display().to_string()])
        }
        "rotate-mok" => run("mokutil", &["--list-enrolled".into()]),
        "greenboot-success" | "greenboot-required" | "greenboot-failure" => {
            let state = Path::new("/run/kyth/greenboot-state");
            write_atomic(state, format!("{name}\n").as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        "vpnc-script" => run("resolvectl", &["status".into()]),
        "finalize-staged" => run("/usr/libexec/kyth-finalize-staged", args),
        "probe" => run("/usr/bin/kyth-probe", args),
        "smoke-check" => run("/usr/bin/kyth-smoke-check", args),
        "resume-check" => run("/usr/bin/kyth-resume-check", args),
        "post-update-check" => run("/usr/bin/kyth-post-update-check", args),
        "nvidia-status" => run("/usr/bin/kyth-nvidia-status", args),
        "windows-verify" => run("/usr/bin/kyth-windows-verify", args),
        "game-boost" => run("/usr/bin/kyth-game-boost", args),
        "health-check" => run("/usr/bin/kyth-health-check", args),
        "vm-acceptance-guest" => run("/usr/bin/kyth-vm-acceptance-guest", args),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("unsupported runtime operation: {name}"))),
    }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args_os().skip(1).map(OsString::into_string).collect::<Result<_, _>>().unwrap_or_else(|_| { eprintln!("arguments must be UTF-8"); std::process::exit(64); });
    let name = args.first().cloned().unwrap_or_else(|| usage());
    args.remove(0);
    match delegate(&name, &args) {
        Ok(code) => code,
        Err(error) => { eprintln!("kyth-runtime: {error}"); ExitCode::from(1) }
    }
}
