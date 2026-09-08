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
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn argv(program: &str, args: &[String]) -> Vec<String> {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect()
}

fn run(program: &str, args: &[String]) -> io::Result<ExitCode> {
    let output = run_bounded(&argv(program, args), COMMAND_TIMEOUT)?;
    let stdout = redact_sensitive_text(&String::from_utf8_lossy(&output.stdout));
    let stderr = redact_sensitive_text(&String::from_utf8_lossy(&output.stderr));
    if !stdout.is_empty() {
        print!("{stdout}");
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    Ok(if output.status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn flatpak_install(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 1, Some(2));
    let app_id = &args[0];
    validate_token(app_id, "Flatpak application ID")
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let remote = vec![
        "remote-add".into(),
        "--if-not-exists".into(),
        "flathub".into(),
        "https://dl.flathub.org/repo/flathub.flatpakrepo".into(),
    ];
    if run("flatpak", &remote)? != ExitCode::SUCCESS {
        return Ok(ExitCode::from(1));
    }
    let install = vec![
        "install".into(),
        "-y".into(),
        "flathub".into(),
        app_id.clone(),
    ];
    let result = run("flatpak", &install)?;
    if result == ExitCode::SUCCESS {
        if let Some(label) = args.get(1) {
            println!("{label} installed. Launch from the application menu.");
        }
    }
    Ok(result)
}

fn toggle_environment_file(
    file_name: &str,
    content: &[u8],
    enabled: &str,
    disabled: &str,
) -> io::Result<ExitCode> {
    let path = home().join(".config/environment.d").join(file_name);
    if path.is_file() {
        fs::remove_file(path)?;
        println!("{disabled}");
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(&path, content)?;
        println!("{enabled}");
    }
    Ok(ExitCode::SUCCESS)
}

fn setup_sunshine(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(0));
    let install = flatpak_install(&["dev.lizardbyte.app.Sunshine".into(), "Sunshine".into()])?;
    if install != ExitCode::SUCCESS {
        return Ok(install);
    }
    let _ = run(
        "flatpak",
        &[
            "override".into(),
            "--user".into(),
            "--filesystem=host".into(),
            "dev.lizardbyte.app.Sunshine".into(),
        ],
    );
    println!("Sunshine is installed. Launch it with `flatpak run dev.lizardbyte.app.Sunshine`.");
    Ok(ExitCode::SUCCESS)
}

fn run_output(program: &str, args: &[String]) -> io::Result<std::process::Output> {
    run_bounded(&argv(program, args), COMMAND_TIMEOUT)
}

fn command_exists(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    env::var_os("PATH")
        .map(|path| env::split_paths(&path).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

fn setup_kali(args: &[String]) -> io::Result<ExitCode> {
    let tools = args.first().map(String::as_str).unwrap_or("headless");
    if args.len() > 1 || !matches!(tools, "headless" | "gui" | "everything") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: setup-kali-box [headless|gui|everything]",
        ));
    }
    let meta = match tools {
        "headless" => "kali-linux-headless",
        "gui" => "kali-linux-default",
        "everything" => "kali-linux-everything",
        _ => unreachable!(),
    };
    let create = [
        "create",
        "--root",
        "--yes",
        "--image",
        "kalilinux/kali-rolling",
        "--name",
        "kali",
    ];
    let result = run(
        "distrobox",
        &create
            .iter()
            .map(|value| (*value).into())
            .collect::<Vec<_>>(),
    )?;
    if result != ExitCode::SUCCESS {
        return Ok(result);
    }
    let install = [
        "enter",
        "--root",
        "kali",
        "--",
        "apt-get",
        "install",
        "-y",
        "-o",
        "Dpkg::Options::=--force-confold",
        meta,
    ];
    run(
        "distrobox",
        &install
            .iter()
            .map(|value| (*value).into())
            .collect::<Vec<_>>(),
    )
}

fn export_kali_apps(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(0));
    let output = run_output(
        "distrobox",
        &[
            "enter".into(),
            "--root".into(),
            "kali".into(),
            "--".into(),
            "find".into(),
            "/usr/share/applications".into(),
            "-maxdepth".into(),
            "1".into(),
            "-name".into(),
            "*.desktop".into(),
            "-printf".into(),
            "%f\n".into(),
        ],
    )?;
    if !output.status.success() {
        return Ok(ExitCode::from(1));
    }
    let mut exported = 0;
    for desktop in String::from_utf8_lossy(&output.stdout).lines() {
        let app = desktop.strip_suffix(".desktop").unwrap_or(desktop);
        if app.is_empty() {
            continue;
        }
        let result = run(
            "distrobox",
            &[
                "enter".into(),
                "--root".into(),
                "kali".into(),
                "--".into(),
                "distrobox-export".into(),
                "--app".into(),
                app.into(),
            ],
        )?;
        if result == ExitCode::SUCCESS {
            exported += 1;
        }
    }
    println!("Exported {exported} Kali desktop entries.");
    Ok(ExitCode::SUCCESS)
}

fn setup_waydroid(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(1));
    if args.first().is_some_and(|arg| arg == "--status") {
        return run(
            "systemctl",
            &[
                "--user".into(),
                "--no-pager".into(),
                "status".into(),
                "waydroid-container.service".into(),
            ],
        );
    }
    let install = run(
        "sudo",
        &[
            "rpm-ostree".into(),
            "install".into(),
            "--idempotent".into(),
            "waydroid".into(),
        ],
    )?;
    if install != ExitCode::SUCCESS {
        return Ok(install);
    }
    let _ = run(
        "systemctl",
        &[
            "--user".into(),
            "enable".into(),
            "--now".into(),
            "waydroid-container.service".into(),
        ],
    );
    if !Path::new("/var/lib/waydroid/images/system.img").is_file() {
        let initialized = run(
            "sudo",
            &[
                "waydroid".into(),
                "init".into(),
                "-s".into(),
                "GAPPS".into(),
            ],
        )?;
        if initialized != ExitCode::SUCCESS {
            return Ok(initialized);
        }
    }
    println!("Waydroid ready. Launch with: waydroid show-full-ui");
    Ok(ExitCode::SUCCESS)
}

fn remove_waydroid(args: &[String]) -> io::Result<ExitCode> {
    if args != ["--confirm"] {
        println!(
            "This permanently deletes Waydroid data. Re-run as: ujust remove-waydroid --confirm"
        );
        return Ok(ExitCode::from(2));
    }
    let _ = run(
        "systemctl",
        &[
            "--user".into(),
            "disable".into(),
            "--now".into(),
            "waydroid-container.service".into(),
        ],
    );
    let _ = run(
        "sudo",
        &["waydroid".into(), "container".into(), "stop".into()],
    );
    let _ = run(
        "sudo",
        &["waydroid".into(), "session".into(), "stop".into()],
    );
    let result = run(
        "sudo",
        &[
            "rm".into(),
            "-rf".into(),
            "/var/lib/waydroid".into(),
            home().join(".local/share/waydroid").display().to_string(),
            home().join(".waydroid").display().to_string(),
        ],
    )?;
    if result == ExitCode::SUCCESS {
        println!("Waydroid data removed. The package remains installed for rollback.");
    }
    Ok(result)
}

fn printer_setup(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(0));
    let service = run(
        "sudo",
        &[
            "systemctl".into(),
            "enable".into(),
            "--now".into(),
            "cups".into(),
        ],
    )?;
    if service != ExitCode::SUCCESS {
        return Ok(service);
    }
    if command_exists("kcmshell6") {
        run("kcmshell6", &["kcm_printer_manager".into()])
    } else {
        run("systemsettings", &[])
    }
}

fn install_ms_fonts(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(0));
    let fonts_dir = home().join(".local/share/fonts/msttcorefonts");
    let cache_dir = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
        .join("kyth/ms-fonts");
    fs::create_dir_all(&fonts_dir)?;
    fs::create_dir_all(&cache_dir)?;
    let base = "https://sourceforge.net/projects/corefonts/files/the%20fonts/final";
    let fonts = [
        "andale32.exe",
        "arial32.exe",
        "arialb32.exe",
        "comic32.exe",
        "courie32.exe",
        "georgi32.exe",
        "impact32.exe",
        "times32.exe",
        "trebuc32.exe",
        "verdan32.exe",
        "webdin32.exe",
    ];
    let mut installed = 0;
    for font in fonts {
        let archive = cache_dir.join(font);
        if !archive.is_file() {
            let result = run(
                "curl",
                &[
                    "-fsSL".into(),
                    "--retry".into(),
                    "2".into(),
                    "-o".into(),
                    archive.display().to_string(),
                    format!("{base}/{font}/download"),
                ],
            )?;
            if result != ExitCode::SUCCESS {
                continue;
            }
        }
        let result = run(
            "7z",
            &[
                "e".into(),
                "-y".into(),
                archive.display().to_string(),
                format!("-o{}", fonts_dir.display()),
                "*.ttf".into(),
                "*.TTF".into(),
            ],
        )?;
        if result == ExitCode::SUCCESS {
            installed += 1;
        }
    }
    let _ = run("fc-cache", &["-f".into(), fonts_dir.display().to_string()]);
    println!(
        "Installed Microsoft font archives: {installed}/{}.",
        fonts.len()
    );
    Ok(if installed > 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn install_jetbrains_toolbox(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(0));
    let response = run_output(
        "curl",
        &[
            "-fsSL".into(),
            "https://data.services.jetbrains.com/products/releases?code=TBA&latest=true&type=release".into(),
        ],
    )?;
    if !response.status.success() {
        return Ok(ExitCode::from(1));
    }
    let json = String::from_utf8_lossy(&response.stdout);
    let linux = json
        .find("\"linux\"")
        .and_then(|start| json.get(start..))
        .and_then(|section| section.find("\"link\"").map(|offset| &section[offset..]))
        .and_then(|section| section.find("https://").map(|offset| &section[offset..]))
        .and_then(|section| section.split('"').next())
        .unwrap_or("");
    let download_url = linux.to_string();
    validate_token(&download_url, "Toolbox download URL")
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let install_dir = home().join(".local/share/JetBrains/Toolbox");
    let bin_dir = home().join(".local/bin");
    let cache_dir = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
        .join("kyth/jetbrains-toolbox");
    fs::create_dir_all(&install_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&cache_dir)?;
    let archive = cache_dir.join("toolbox.tar.gz");
    let result = run(
        "curl",
        &[
            "-fsSL".into(),
            "-o".into(),
            archive.display().to_string(),
            download_url,
        ],
    )?;
    if result != ExitCode::SUCCESS {
        return Ok(result);
    }
    let _ = run(
        "tar",
        &[
            "-xzf".into(),
            archive.display().to_string(),
            "-C".into(),
            cache_dir.display().to_string(),
        ],
    )?;
    let extracted = run_output(
        "find",
        &[
            cache_dir.display().to_string(),
            "-type".into(),
            "f".into(),
            "-name".into(),
            "jetbrains-toolbox".into(),
        ],
    )?;
    let extracted_text = String::from_utf8_lossy(&extracted.stdout);
    let source = extracted_text.lines().next().unwrap_or("");
    if source.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Toolbox executable missing from archive",
        ));
    }
    let target = install_dir.join("jetbrains-toolbox");
    fs::copy(source, &target)?;
    let _ = run("chmod", &["0755".into(), target.display().to_string()])?;
    let link = bin_dir.join("jetbrains-toolbox");
    run(
        "ln",
        &[
            "-sfn".into(),
            target.display().to_string(),
            link.display().to_string(),
        ],
    )
}

fn setup_boot_windows_steam(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(0));
    let output = run_output("efibootmgr", &["-v".into()])?;
    if !output.status.success() {
        return Ok(ExitCode::from(1));
    }
    let entry = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            if !line.to_ascii_lowercase().contains("windows") {
                return None;
            }
            let marker = line.find("Boot")? + 4;
            let candidate = line.get(marker..marker + 4)?;
            candidate
                .chars()
                .all(|char| char.is_ascii_hexdigit())
                .then(|| candidate.to_string())
        })
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no Windows EFI boot entry found")
        })?;
    let temp = env::temp_dir().join(format!("kyth-boot-windows-{}", std::process::id()));
    let sudoers_temp = env::temp_dir().join(format!("kyth-boot-windows-sudoers-{}", std::process::id()));
    write_atomic(
        &temp,
        format!(
            "#!/bin/sh\nexec sudo /usr/sbin/efibootmgr -n {entry} && sudo /usr/bin/systemctl reboot\n"
        )
        .as_bytes(),
    )?;
    write_atomic(
        &sudoers_temp,
        format!(
            "%wheel ALL=(root) NOPASSWD: /usr/sbin/efibootmgr -n {entry}, /usr/bin/systemctl reboot\n"
        )
        .as_bytes(),
    )?;
    let helper_install = run(
        "sudo",
        &[
            "install".into(),
            "-Dm0755".into(),
            temp.display().to_string(),
            "/usr/local/bin/boot-windows".into(),
        ],
    );
    let _ = fs::remove_file(&temp);
    let helper_result = helper_install?;
    if helper_result != ExitCode::SUCCESS {
        let _ = fs::remove_file(&sudoers_temp);
        return Ok(helper_result);
    }
    let sudoers_install = run(
        "sudo",
        &[
            "install".into(),
            "-Dm0440".into(),
            sudoers_temp.display().to_string(),
            "/etc/sudoers.d/kyth-boot-windows".into(),
        ],
    );
    let _ = fs::remove_file(&sudoers_temp);
    let sudoers_result = match sudoers_install {
        Ok(result) => result,
        Err(error) => {
            let _ = run(
                "sudo",
                &[
                    "rm".into(),
                    "-f".into(),
                    "/usr/local/bin/boot-windows".into(),
                ],
            );
            return Err(error);
        }
    };
    if sudoers_result != ExitCode::SUCCESS {
        let _ = run(
            "sudo",
            &[
                "rm".into(),
                "-f".into(),
                "/usr/local/bin/boot-windows".into(),
            ],
        );
        return Ok(sudoers_result);
    }
    println!(
        "Installed /usr/local/bin/boot-windows and /etc/sudoers.d/kyth-boot-windows for EFI entry {entry}."
    );
    Ok(ExitCode::SUCCESS)
}

fn firmware_update(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(0));
    match kyth_shared::system::firmware::firmware_update_recipe() {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => Err(io::Error::other(error)),
    }
}

fn launch_lutris(args: &[String], uri: &str) -> io::Result<ExitCode> {
    require_args(args, 0, Some(0));
    if !command_exists("lutris") {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Lutris is not installed",
        ));
    }
    run("lutris", &[uri.into()])
}

fn update_channel(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, None);
    let channel = args
        .iter()
        .find(|arg| arg.as_str() != "--dry-run")
        .map(String::as_str)
        .unwrap_or("stable");
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let base = match channel {
        "stable" | "latest" => "latest",
        "testing" => "testing",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "channel must be stable, latest, or testing",
            ));
        }
    };
    if dry_run {
        println!("Dry-run: would stage KythOS :{base}.");
        return Ok(ExitCode::SUCCESS);
    }
    let flavor = fs::read_to_string("/usr/share/kyth/kernel-flavor").unwrap_or_default();
    let suffix = if flavor.trim() == "cachy" {
        "-cachy"
    } else {
        ""
    };
    run(
        "sudo",
        &[
            "/usr/bin/kyth-bootc-guard".into(),
            format!("switch-{base}{suffix}"),
        ],
    )
}

fn rebase_image(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(1));
    let input = args.first().map(String::as_str).unwrap_or("kyth:latest");
    validate_token(input, "image reference")
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let reference = if input.contains('/') {
        input.to_string()
    } else if input.starts_with("kyth:") || input == "kyth" {
        let tag = if input == "kyth" {
            "latest"
        } else {
            &input[5..]
        };
        format!("ghcr.io/kyth-os/kyth:{tag}")
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "image must be kyth:<tag> or a full image reference",
        ));
    };
    println!("Rebasing to: {reference}");
    let switched = run("sudo", &["bootc".into(), "switch".into(), reference])?;
    if switched != ExitCode::SUCCESS {
        return Ok(switched);
    }
    run(
        "sudo",
        &["/usr/bin/kyth-finalize-staged".into()],
    )
}

fn switch_kernel(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 0, Some(1));
    let flavor = args.first().map(String::as_str).unwrap_or("fedora");
    match flavor {
        "fedora" | "stock" | "cachy" | "cachyos" => {}
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "kernel flavor must be fedora or cachy",
            ));
        }
    };
    let current_branch = kyth_shared::system::bootc_query::image_reference()
        .and_then(|reference| {
            kyth_shared::system::bootc_policy::branch_from_ref(Some(&reference))
        });
    let target = kyth_shared::system::bootc_policy::image_tag_for_kernel(flavor, current_branch.as_deref());
    run(
        "sudo",
        &[
            "/usr/bin/kyth-bootc-guard".into(),
            format!("switch-{target}"),
        ],
    )
}

fn require_args(args: &[String], min: usize, max: Option<usize>) {
    if args.len() < min || max.is_some_and(|value| args.len() > value) {
        usage();
    }
}

fn is_native_executable(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes.starts_with(b"\x7fELF"))
}

fn validate_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 256
        || value
            .bytes()
            .any(|byte| byte == 0 || byte == b'\n' || byte == b'\r')
    {
        return Err(format!("invalid {label}"));
    }
    Ok(())
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("kyth"),
        std::process::id()
    ));
    fs::write(&tmp, contents)?;
    fs::rename(tmp, path)
}

fn power_arbiter() -> io::Result<ExitCode> {
    let mut battery = false;
    for root in ["/sys/class/power_supply"] {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !(name.starts_with("BAT") || name.starts_with("CMB")) {
                continue;
            }
            if fs::read_to_string(entry.path().join("status"))
                .map(|text| text.trim() == "Discharging")
                .unwrap_or(false)
            {
                battery = true;
            }
        }
    }
    let profile = if battery {
        "balance_power"
    } else {
        "performance"
    };
    run("/usr/bin/kyth-set-epp", &[profile.to_string()])
}

fn readahead(args: &[String]) -> io::Result<ExitCode> {
    let value = if args.first().is_some_and(|arg| arg == "hint") {
        if Path::new("/run/kyth/gaming-hint").exists() {
            "2048"
        } else {
            "512"
        }
    } else {
        require_args(args, 0, Some(1));
        if args.is_empty() {
            "512"
        } else {
            args[0].as_str()
        }
    };
    if !matches!(value, "512" | "2048") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "readahead must be 512 or 2048",
        ));
    }
    let Ok(entries) = fs::read_dir("/sys/block") else {
        return Ok(ExitCode::SUCCESS);
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("nvme") {
            continue;
        }
        let target = entry.path().join("queue/read_ahead_kb");
        if target.is_file() {
            let _ = fs::write(target, value);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn sleep_mode(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 1, Some(1));
    if args[0] != "deep" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "only deep sleep is supported",
        ));
    }
    fs::write("/sys/power/mem_sleep", "deep")?;
    Ok(ExitCode::SUCCESS)
}

fn nvme_tuning(args: &[String]) -> io::Result<ExitCode> {
    let action = args.first().map(String::as_str).unwrap_or("status");
    let rule = Path::new("/etc/udev/rules.d/61-kyth-nvme-tuning.rules");
    match action {
        "status" => {
            println!(
                "Selected profile: {}",
                if rule.is_file() { "kyth" } else { "default" }
            );
            let Ok(entries) = fs::read_dir("/sys/block") else {
                return Ok(ExitCode::SUCCESS);
            };
            for entry in entries.flatten() {
                if !entry.file_name().to_string_lossy().starts_with("nvme") {
                    continue;
                }
                let queue = entry.path().join("queue");
                let scheduler = fs::read_to_string(queue.join("scheduler"))
                    .unwrap_or_else(|_| "unavailable".into());
                let read_ahead = fs::read_to_string(queue.join("read_ahead_kb"))
                    .unwrap_or_else(|_| "unavailable".into());
                println!(
                    "{}: scheduler={} read_ahead_kb={}",
                    entry.file_name().to_string_lossy(),
                    scheduler.trim(),
                    read_ahead.trim()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        "kyth" | "default" => {
            if unsafe { libc::geteuid() } != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "nvme profile changes require root",
                ));
            }
            if action == "kyth" {
                write_atomic(rule, b"# Managed by kyth-runtime\nACTION==\"add|change\", KERNEL==\"nvme[0-9]*n[0-9]*\", ATTR{queue/scheduler}=\"none\"\nACTION==\"add|change\", KERNEL==\"nvme[0-9]*n[0-9]*\", ATTR{queue/read_ahead_kb}=\"2048\"\nACTION==\"add|change\", KERNEL==\"nvme[0-9]*n[0-9]*\", ATTR{queue/wbt_lat_usec}=\"0\"\n")?;
            } else {
                let _ = fs::remove_file(rule);
            }
            let _ = run("udevadm", &["control".into(), "--reload-rules".into()]);
            let _ = run(
                "udevadm",
                &[
                    "trigger".into(),
                    "--action=change".into(),
                    "--subsystem-match=block".into(),
                ],
            );
            Ok(ExitCode::SUCCESS)
        }
        "help" | "-h" | "--help" => {
            println!("usage: kyth-nvme-tuning <status|kyth|default>");
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown NVMe profile",
        )),
    }
}

fn find_named_file(root: &Path, extension: &str) -> Option<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            for nested in fs::read_dir(path).ok()?.flatten() {
                let candidate = nested.path();
                if candidate.is_file()
                    && candidate
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
                {
                    found.push(candidate);
                }
            }
        } else if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        {
            found.push(path);
        }
    }
    found.into_iter().next()
}

fn davinci_install(args: &[String]) -> io::Result<ExitCode> {
    let zip = args
        .first()
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("XDG_DOWNLOAD_DIR")
                .map(PathBuf::from)
                .and_then(|path| find_named_file(&path, "zip"))
        })
        .or_else(|| find_named_file(&home().join("Downloads"), "zip"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no DaVinci Resolve Linux ZIP was found",
            )
        })?;
    if !zip.is_file()
        || !zip
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().contains("davinci"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "selected archive is not a DaVinci Resolve ZIP",
        ));
    }
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
        .join("kyth/davinci-resolve");
    let source = cache.join("resolve-flatpak");
    let work = cache.join("work");
    let build = cache.join("build-dir");
    fs::create_dir_all(&cache)?;
    if !source.join(".git").is_dir() {
        if run(
            "git",
            &[
                "clone".into(),
                "--recurse-submodules".into(),
                "https://github.com/pobthebuilder/resolve-flatpak.git".into(),
                source.display().to_string(),
            ],
        )? != ExitCode::SUCCESS
        {
            return Ok(ExitCode::from(1));
        }
    }
    let commit = "000efab2df0cc781a47dff13321bfdb688aad14f";
    if run(
        "git",
        &[
            "-C".into(),
            source.display().to_string(),
            "checkout".into(),
            commit.into(),
        ],
    )? != ExitCode::SUCCESS
    {
        return Ok(ExitCode::from(1));
    }
    let _ = fs::remove_dir_all(&work);
    let _ = fs::remove_dir_all(&build);
    fs::create_dir_all(&work)?;
    fs::create_dir_all(&build)?;
    if run(
        "unzip",
        &[
            "-qo".into(),
            zip.display().to_string(),
            "-d".into(),
            work.display().to_string(),
        ],
    )? != ExitCode::SUCCESS
    {
        return Ok(ExitCode::from(1));
    }
    let run_file = find_named_file(&work, "run").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Resolve ZIP contains no Linux installer",
        )
    })?;
    let run_name = run_file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("DaVinci_Resolve_Linux.run")
        .to_string();
    fs::copy(&run_file, source.join(&run_name))?;
    let manifest = if run_name.to_ascii_lowercase().contains("studio") {
        "com.blackmagic.ResolveStudio.yaml"
    } else {
        "com.blackmagic.Resolve.yaml"
    };
    run(
        "flatpak-builder",
        &[
            "--user".into(),
            "--install".into(),
            "--install-deps-from=flathub".into(),
            "--force-clean".into(),
            build.display().to_string(),
            source.join(manifest).display().to_string(),
        ],
    )
}

fn scx(args: &[String]) -> io::Result<ExitCode> {
    let action = args.first().map(String::as_str).unwrap_or("status");
    match action {
        "status" => {
            if let Ok(config) = fs::read_to_string("/etc/scx/scx_loader.conf") {
                println!("{config}");
            }
            let _ = run(
                "systemctl",
                &[
                    "--no-pager".into(),
                    "--quiet".into(),
                    "is-active".into(),
                    "scx_loader.service".into(),
                ],
            );
            Ok(ExitCode::SUCCESS)
        }
        "list" => {
            let output = run_bounded(
                &argv(
                    "/usr/bin/find",
                    &[
                        "/usr/bin".into(),
                        "-maxdepth".into(),
                        "1".into(),
                        "-type".into(),
                        "f".into(),
                        "-name".into(),
                        "scx_*".into(),
                    ],
                ),
                COMMAND_TIMEOUT,
            )?;
            io::stdout().write_all(&output.stdout)?;
            Ok(if output.status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        "set" => {
            require_args(args, 2, Some(2));
            let scheduler = if args[1].starts_with("scx_") {
                args[1].clone()
            } else {
                format!("scx_{}", args[1])
            };
            if !scheduler
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid scheduler",
                ));
            }
            let binary = Path::new("/usr/bin").join(&scheduler);
            if !binary.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "scheduler is not installed",
                ));
            }
            write_atomic(
                Path::new("/etc/scx/scx_loader.conf"),
                format!("SCX_SCHEDULER={scheduler}\n").as_bytes(),
            )?;
            let _ = run("systemctl", &["enable".into(), "scx_loader.service".into()]);
            run(
                "systemctl",
                &["restart".into(), "scx_loader.service".into()],
            )
        }
        "restart" => {
            require_args(args, 1, Some(1));
            run(
                "systemctl",
                &["restart".into(), "scx_loader.service".into()],
            )
        }
        "stop" => {
            require_args(args, 1, Some(1));
            let result = run(
                "systemctl",
                &[
                    "disable".into(),
                    "--now".into(),
                    "scx_loader.service".into(),
                ],
            );
            let _ = fs::remove_file("/etc/scx/scx_loader.conf");
            result
        }
        "help" | "-h" | "--help" => {
            println!("usage: kyth-scx <status|list|set|restart|stop> [scheduler]");
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown kyth-scx action",
        )),
    }
}

fn windows_import(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 1, Some(1));
    let source = Path::new(&args[0]);
    if !source.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NTFS source must be an absolute device path",
        ));
    }
    let mount = PathBuf::from(format!("/tmp/kyth-win-{}", std::process::id()));
    fs::create_dir_all(&mount)?;
    let mounted = run(
        "mount",
        &[
            "-o".into(),
            "ro".into(),
            source.display().to_string(),
            mount.display().to_string(),
        ],
    )?;
    if mounted != ExitCode::SUCCESS {
        let _ = fs::remove_dir(&mount);
        return Ok(mounted);
    }
    let result = (|| {
        let target = home().join("WindowsImport");
        fs::create_dir_all(&target)?;
        let users = mount.join("Users");
        for user in fs::read_dir(users).into_iter().flatten().flatten() {
            for name in ["Documents", "Pictures"] {
                let source = user.path().join(name);
                if source.is_dir() {
                    let destination =
                        target.join(format!("{}-{}", user.file_name().to_string_lossy(), name));
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
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else if from.is_file() {
            let _ = fs::copy(from, to)?;
        }
    }
    Ok(())
}

fn gamescope(args: &[String]) -> io::Result<ExitCode> {
    require_args(args, 1, None);
    let preset = &args[0];
    validate_token(preset, "gamescope preset")
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if !matches!(
        preset.as_str(),
        "quality" | "hdr" | "balanced" | "performance"
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown gamescope preset",
        ));
    }
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
        "_ai-dev" => ("ai-dev", forwarded.to_vec()),
        "ai-dev-status" => ("ai-dev", vec!["status".into()]),
        "ai-dev-setup" => ("ai-dev", vec!["setup".into()]),
        "ai-dev-enter" => ("ai-dev", vec!["enter".into()]),
        "ai-dev-start" => ("ai-dev", vec!["start".into()]),
        "ai-dev-stop" => ("ai-dev", vec!["stop".into()]),
        "ai-dev-remove" => ("ai-dev", vec!["remove".into()]),
        "setup-kali-box" => ("setup-kali-box", forwarded.to_vec()),
        "export-kali-apps" => ("export-kali-apps", forwarded.to_vec()),
        "setup-waydroid" => ("setup-waydroid", forwarded.to_vec()),
        "remove-waydroid" => ("remove-waydroid", forwarded.to_vec()),
        "_install-flatpak" => ("install-flatpak", forwarded.to_vec()),
        "install-boxbuddy" => (
            "install-flatpak",
            vec!["io.github.dvlv.boxbuddyrs".into(), "BoxBuddy".into()],
        ),
        "install-steam" => (
            "install-flatpak",
            vec!["com.valvesoftware.Steam".into(), "Steam".into()],
        ),
        "install-lutris" => (
            "install-flatpak",
            vec!["net.lutris.Lutris".into(), "Lutris".into()],
        ),
        "install-heroic" => (
            "install-flatpak",
            vec!["com.heroicgameslauncher.hgl".into(), "Heroic".into()],
        ),
        "install-bottles" => (
            "install-flatpak",
            vec!["com.usebottles.bottles".into(), "Bottles".into()],
        ),
        "install-prismlauncher" => (
            "install-flatpak",
            vec![
                "org.prismlauncher.PrismLauncher".into(),
                "Prism Launcher".into(),
            ],
        ),
        "install-itch" => (
            "install-flatpak",
            vec!["io.itch.itch".into(), "Itch.io".into()],
        ),
        "install-retroarch" => (
            "install-flatpak",
            vec!["org.libretro.RetroArch".into(), "RetroArch".into()],
        ),
        "install-ludusavi" => (
            "install-flatpak",
            vec!["com.github.mtkennerly.ludusavi".into(), "Ludusavi".into()],
        ),
        "install-lact" => (
            "install-flatpak",
            vec!["io.github.ilya_zlobintsev.LACT".into(), "LACT".into()],
        ),
        "install-piper" => (
            "install-flatpak",
            vec!["org.freedesktop.Piper".into(), "Piper".into()],
        ),
        "install-openrgb" => (
            "install-flatpak",
            vec!["org.openrgb.OpenRGB".into(), "OpenRGB".into()],
        ),
        "install-solaar" => (
            "install-flatpak",
            vec!["io.github.pwr_solaar.solaar".into(), "Solaar".into()],
        ),
        "install-oversteer" => (
            "install-flatpak",
            vec!["org.berarma.Oversteer".into(), "Oversteer".into()],
        ),
        "install-vesktop" => (
            "install-flatpak",
            vec!["dev.vencord.Vesktop".into(), "Vesktop".into()],
        ),
        "install-gpu-screen-recorder" => (
            "install-flatpak",
            vec![
                "com.dec05eba.gpu_screen_recorder".into(),
                "GPU Screen Recorder".into(),
            ],
        ),
        "install-goverlay" => (
            "install-flatpak",
            vec!["io.github.benjamimgois.goverlay".into(), "GOverlay".into()],
        ),
        "install-mangojuice" => (
            "install-flatpak",
            vec!["io.github.radiolamp.mangojuice".into(), "MangoJuice".into()],
        ),
        "install-obs" => (
            "install-flatpak",
            vec!["com.obsproject.Studio".into(), "OBS Studio".into()],
        ),
        "startup-apps" => ("startup-apps", forwarded.to_vec()),
        "install-ms-fonts" => ("install-ms-fonts", forwarded.to_vec()),
        "setup-printer" => ("setup-printer", forwarded.to_vec()),
        "firmware-update" => ("firmware-update", forwarded.to_vec()),
        "setup-kyth-dev-box" => ("ai-dev", vec!["setup".into()]),
        "install-vscode" => ("install-vscode", forwarded.to_vec()),
        "install-jetbrains-toolbox" => ("install-jetbrains-toolbox", forwarded.to_vec()),
        "setup-boot-windows-steam" => ("setup-boot-windows-steam", forwarded.to_vec()),
        "dualboot-status" => ("dualboot-status", forwarded.to_vec()),
        "reclaim-windows" => ("reclaim-windows", forwarded.to_vec()),
        "fix-dualboot-clock" => ("fix-dualboot-clock", forwarded.to_vec()),
        "install-battlenet" => ("lutris-battlenet", forwarded.to_vec()),
        "install-epic-launcher" => ("lutris-epic", forwarded.to_vec()),
        "install-ea-app" => ("lutris-ea", forwarded.to_vec()),
        "install-ubisoft-connect" => ("lutris-ubisoft", forwarded.to_vec()),
        "corectrl" => ("corectrl", forwarded.to_vec()),
        "install-racing-wheel-drivers" => ("install-racing-wheel-drivers", forwarded.to_vec()),
        "install-asus-tools" => ("install-asus-tools", forwarded.to_vec()),
        "install-lsfg-vk" => ("install-lsfg-vk", forwarded.to_vec()),
        "deploy-opticscaler" => ("deploy-opticscaler", forwarded.to_vec()),
        "install-umu" => ("install-umu", forwarded.to_vec()),
        "gaming-stack-status" => ("gaming-stack-status", forwarded.to_vec()),
        "game-performance" => ("game-performance", forwarded.to_vec()),
        "game-performance-profile" => ("game-performance-profile", forwarded.to_vec()),
        "zink-run" => ("zink-run", forwarded.to_vec()),
        "low-latency" => ("low-latency", forwarded.to_vec()),
        "enable-bpftune" => ("enable-bpftune", forwarded.to_vec()),
        "disable-bpftune" => ("disable-bpftune", forwarded.to_vec()),
        "setup-vr" => ("setup-vr", forwarded.to_vec()),
        "retry-quarantined-update" => ("retry-quarantined-update", forwarded.to_vec()),
        "rebase" => ("rebase", forwarded.to_vec()),
        "switch-channel" => ("switch-channel", forwarded.to_vec()),
        "switch-channel-impl" => ("switch-channel", forwarded.to_vec()),
        "switch-kernel" => ("switch-kernel", forwarded.to_vec()),
        "install-nvidia-driver" => ("install-nvidia-driver", forwarded.to_vec()),
        "install-displaylink" => ("install-displaylink", forwarded.to_vec()),
        "hardware-inventory" => ("hardware-policy", vec!["inventory".into()]),
        "hardware-policy-apply" => ("hardware-policy", vec!["apply".into(), "--force".into()]),
        "export-steam-games" => ("steam-game-export", forwarded.to_vec()),
        "setup-sunshine" => ("setup-sunshine", forwarded.to_vec()),
        "setup-tailscale" => ("apply-tailscale", forwarded.to_vec()),
        "apply-preset" => ("apply-role-preset", forwarded.to_vec()),
        "update-proton-cachyos" => ("proton-cachyos-update", forwarded.to_vec()),
        "toggle-fsr4" => ("toggle-fsr4", forwarded.to_vec()),
        "toggle-nvapi" => ("toggle-nvapi", forwarded.to_vec()),
        "enable-obs-capture" => ("enable-obs-capture", forwarded.to_vec()),
        "install-coolercontrol" => ("install-coolercontrol", forwarded.to_vec()),
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
        "game-hdr" => (
            "gamescope",
            std::iter::once("hdr".into())
                .chain(forwarded.iter().cloned())
                .collect(),
        ),
        "gaming-mode" => ("performance-mode", vec!["gaming".into()]),
        "balanced-mode" => ("performance-mode", vec!["balanced".into()]),
        "scx" => ("scx", forwarded.to_vec()),
        "nvme-tuning" => ("nvme-tuning", forwarded.to_vec()),
        "readahead-run" => ("readahead-run", forwarded.to_vec()),
        "preheat-shaders" => ("shader-preheat", forwarded.to_vec()),
        "fix-ntfs-drives" => ("storage-gate", forwarded.to_vec()),
        "game-boost" => ("game-boost", forwarded.to_vec()),
        "health-check" => ("health-check", forwarded.to_vec()),
        "list-presets" => {
            println!("Available presets: everyday, gaming, dev, creator");
            return Ok(ExitCode::SUCCESS);
        }
        _ => {
            let binary_name = format!("/usr/bin/kyth-{name}");
            if !is_native_executable(Path::new(&binary_name)) {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("recipe {name} has no Rust owner"),
                ));
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
            for (program, command_args) in steps {
                if run(program, &command_args)? != ExitCode::SUCCESS {
                    failed = true;
                }
            }
            Ok(if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        "perf-gate" => run("/usr/bin/kyth-perf-gate-rs", args),
        "windows-friendly-defaults" => run("/usr/bin/kyth-user-polish", args),
        "storage-gate" => run("/usr/bin/kyth-storage-sense", args),
        "apply-hardware" | "retry-hardware" => run("/usr/bin/kyth-privileged", args),
        "power-arbiter" => power_arbiter(),
        "readahead-hint" => readahead(&["hint".into()]),
        "readahead-run" => {
            let split = args.iter().position(|arg| arg == "--");
            let (path, command) = match split {
                Some(index) => (args.first(), &args[index + 1..]),
                None => (args.first(), &[][..]),
            };
            if let Some(path) = path {
                if Path::new(path).is_dir() {
                    let _ = readahead(&[]);
                }
            }
            if command.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "readahead-run requires -- command",
                ));
            }
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
        "isolate-game" => {
            require_args(args, 2, None);
            let mut command = vec![
                "--user".into(),
                "--scope".into(),
                "--slice=gaming.slice".into(),
                "--property".into(),
                "PrivateTmp=yes".into(),
                "--property".into(),
                "SystemCallFilter=@system-service".into(),
                "--".into(),
            ];
            command.extend_from_slice(&args[1..]);
            run("systemd-run", &command)
        }
        "distrobox-root-launch" => {
            require_args(args, 2, None);
            validate_token(
                &args[if args[0] == "--root" { 1 } else { 0 }],
                "distrobox name",
            )
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            let mut command = vec!["enter".into()];
            if args[0] == "--root" {
                command.push("--root".into());
                command.extend_from_slice(&args[1..]);
            } else {
                command.extend_from_slice(args);
            }
            run("distrobox", &command)
        }
        "device-info" | "perf-report" | "report" | "kerver" | "snappy-bench" => {
            println!("KythOS {name} report is owned by the Rust runtime.");
            run("/usr/bin/kyth-probe", &["--print-only".into()])
        }
        "ai-dev" => run("/usr/bin/kyth-ai-dev", args),
        "setup-kali-box" => setup_kali(args),
        "export-kali-apps" => export_kali_apps(args),
        "setup-waydroid" => setup_waydroid(args),
        "remove-waydroid" => remove_waydroid(args),
        "startup-apps" => {
            require_args(args, 0, Some(0));
            if command_exists("kcmshell6") {
                run("kcmshell6", &["autostart".into()])
            } else {
                run("systemsettings", &[])
            }
        }
        "install-ms-fonts" => install_ms_fonts(args),
        "setup-printer" => printer_setup(args),
        "firmware-update" => firmware_update(args),
        "install-vscode" => run("/usr/bin/kyth-vscode-wallet", args),
        "install-jetbrains-toolbox" => install_jetbrains_toolbox(args),
        "setup-boot-windows-steam" => setup_boot_windows_steam(args),
        "dualboot-status" => run("efibootmgr", &["-v".into()]),
        "reclaim-windows" => {
            println!("Use System Hub → Disks to remove the Windows partition and grow KythOS.");
            run(
                "/usr/bin/kyth-welcome-launch",
                &["--page".into(), "This PC".into()],
            )
        }
        "fix-dualboot-clock" => run(
            "sudo",
            &[
                "timedatectl".into(),
                "set-local-rtc".into(),
                "1".into(),
                "--adjust-system-clock".into(),
            ],
        ),
        "lutris-battlenet" => launch_lutris(args, "lutris:install/battlenet"),
        "lutris-epic" => launch_lutris(args, "lutris:install/epic-games-store"),
        "lutris-ea" => launch_lutris(args, "lutris:ea-app-standard"),
        "lutris-ubisoft" => launch_lutris(args, "lutris:ubisoft-connect-latest"),
        "corectrl" => {
            require_args(args, 0, Some(0));
            if !command_exists("corectrl") {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "CoreCtrl is not installed",
                ));
            }
            run("corectrl", &[])
        }
        "install-racing-wheel-drivers" => run(
            "sudo",
            &[
                "rpm-ostree".into(),
                "install".into(),
                "--idempotent".into(),
                "--allow-inactive".into(),
                "akmod-hid-tmff2".into(),
                "akmod-new-lg4ff".into(),
                "akmod-hid-fanatecff".into(),
                "akmod-t150-driver".into(),
            ],
        ),
        "install-asus-tools" => run(
            "sudo",
            &[
                "rpm-ostree".into(),
                "install".into(),
                "--idempotent".into(),
                "asusctl".into(),
                "supergfxctl".into(),
                "rog-control-center".into(),
            ],
        ),
        "install-nvidia-driver" => run(
            "sudo",
            &[
                "rpm-ostree".into(),
                "install".into(),
                "--idempotent".into(),
                "akmod-nvidia".into(),
            ],
        ),
        "install-displaylink" => run(
            "sudo",
            &[
                "rpm-ostree".into(),
                "install".into(),
                "--idempotent".into(),
                "displaylink".into(),
            ],
        ),
        "install-lsfg-vk" | "deploy-opticscaler" | "install-umu" => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this optional vendor-asset recipe is retired and is not part of the supported image contract",
        )),
        "gaming-stack-status" => run("/usr/bin/kyth-probe", &["--print-only".into()]),
        "game-performance" | "game-performance-profile" => run("/usr/bin/kyth-game-boost", args),
        "zink-run" => {
            require_args(args, 1, None);
            let mut command = vec!["MESA_LOADER_DRIVER_OVERRIDE=zink".into()];
            command.extend(args.iter().cloned());
            run("/usr/bin/env", &command)
        }
        "low-latency" => run("/usr/bin/kyth-perf-gate-rs", &["low-latency".into()]),
        "enable-bpftune" => run(
            "sudo",
            &[
                "systemctl".into(),
                "enable".into(),
                "--now".into(),
                "bpftune.service".into(),
            ],
        ),
        "disable-bpftune" => run(
            "sudo",
            &[
                "systemctl".into(),
                "disable".into(),
                "--now".into(),
                "bpftune.service".into(),
            ],
        ),
        "setup-vr" => run(
            "sudo",
            &[
                "rpm-ostree".into(),
                "install".into(),
                "--idempotent".into(),
                "openxr-loader".into(),
            ],
        ),
        "retry-quarantined-update" => {
            require_args(args, 1, Some(1));
            validate_token(&args[0], "digest")
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            run(
                "sudo",
                &[
                    "kyth-boot-health".into(),
                    "clear-quarantine".into(),
                    "--digest".into(),
                    args[0].clone(),
                ],
            )
        }
        "rebase" => rebase_image(args),
        "switch-channel" => update_channel(args),
        "switch-kernel" => switch_kernel(args),
        "install-flatpak" => flatpak_install(args),
        "hardware-policy" => run("/usr/bin/kyth-hardware-policy", args),
        "steam-game-export" => run("/usr/bin/kyth-steam-game-export", args),
        "apply-tailscale" => run("/usr/bin/kyth-apply-tailscale", args),
        "apply-role-preset" => run("/usr/bin/kyth-apply-role-preset", args),
        "proton-cachyos-update" => run("/usr/bin/kyth-proton-cachyos-update", args),
        "setup-sunshine" => setup_sunshine(args),
        "toggle-fsr4" => toggle_environment_file(
            "99-kyth-fsr4.conf",
            b"PROTON_FSR4_UPGRADE=1\n",
            "FSR4 global enabled. Run again to disable.",
            "FSR4 global removed. Restart Steam to apply.",
        ),
        "toggle-nvapi" => toggle_environment_file(
            "99-kyth-nvapi.conf",
            b"PROTON_ENABLE_NVAPI=1\nDXVK_ENABLE_NVAPI=1\n",
            "NVAPI global override enabled. Run again to disable.",
            "NVAPI global override removed. Restart Steam to apply.",
        ),
        "enable-obs-capture" => {
            let path = home().join(".config/environment.d/obs-vkcapture.conf");
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            write_atomic(&path, b"OBS_VKCAPTURE=1\n")?;
            println!("OBS Vulkan capture enabled for new sessions.");
            Ok(ExitCode::SUCCESS)
        }
        "install-coolercontrol" => run(
            "sudo",
            &[
                "rpm-ostree".into(),
                "install".into(),
                "--idempotent".into(),
                "coolercontrol".into(),
            ],
        ),
        "boot-verify" => run("/usr/bin/kyth-bootc-guard", &["status".into()]),
        "boot-branding-guard" | "session-splash-guard" => {
            let marker = home().join(".config/kyth/runtime-last-refresh");
            write_atomic(&marker, b"ok\n")?;
            Ok(ExitCode::SUCCESS)
        }
        "shader-prune" => {
            let cache = env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".cache"))
                .join("mesa_shader_cache");
            if cache.is_dir() {
                let _ = fs::remove_dir_all(&cache);
            }
            Ok(ExitCode::SUCCESS)
        }
        "shader-preheat" => {
            let dir = home().join(".config/environment.d");
            fs::create_dir_all(&dir)?;
            write_atomic(
                &dir.join("kyth-shader-cache.conf"),
                b"MESA_SHADER_CACHE_MAX_SIZE=32G\nRADV_PERF=gpl\n",
            )?;
            Ok(ExitCode::SUCCESS)
        }
        "local-bin-migrate" => {
            let source = home().join(".local/bin");
            fs::create_dir_all(&source)?;
            Ok(ExitCode::SUCCESS)
        }
        "nearby-share" => {
            require_args(args, 0, None);
            run("kdeconnect-app", &[])
        }
        "default-flatpaks" => {
            let apps = [
                "com.valvesoftware.Steam",
                "net.lutris.Lutris",
                "com.heroicgameslauncher.hgl",
                "org.videolan.VLC",
                "com.brave.Browser",
                "org.libreoffice.LibreOffice",
            ];
            run(
                "flatpak",
                &std::iter::once("install".to_string())
                    .chain(std::iter::once("-y".to_string()))
                    .chain(apps.into_iter().map(str::to_string))
                    .collect::<Vec<_>>(),
            )
        }
        "flathub-setup" => run(
            "flatpak",
            &[
                "remote-add".into(),
                "--if-not-exists".into(),
                "--system".into(),
                "flathub".into(),
                "https://dl.flathub.org/repo/flathub.flatpakrepo".into(),
            ],
        ),
        "mok-status" => run("mokutil", &["--list-enrolled".into()]),
        "enroll-mok" => {
            let cert = Path::new("/usr/share/kyth/secureboot/kyth-secureboot.cer");
            if !cert.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "KythOS Secure Boot certificate is not installed",
                ));
            }
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
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported runtime operation: {name}"),
        )),
    }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args_os()
        .skip(1)
        .map(OsString::into_string)
        .collect::<Result<_, _>>()
        .unwrap_or_else(|_| {
            eprintln!("arguments must be UTF-8");
            std::process::exit(64);
        });
    let name = args.first().cloned().unwrap_or_else(|| usage());
    args.remove(0);
    match delegate(&name, &args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("kyth-runtime: {error}");
            ExitCode::from(1)
        }
    }
}
