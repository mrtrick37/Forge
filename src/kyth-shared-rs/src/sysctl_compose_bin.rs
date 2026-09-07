//! Native sysctl tier composer and legacy-file cleanup owner.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::sysctl_compose::{compose, load_inputs, write_tiers, TIERS};

const DEST: &str = "/etc/sysctl.d";

fn config_dir(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    for path in [
        PathBuf::from("build_files/config/sysctl"),
        PathBuf::from("/ctx/config/sysctl"),
        PathBuf::from("/usr/share/kyth/config/sysctl"),
    ] {
        if path.is_dir() {
            return path;
        }
    }
    PathBuf::from("build_files/config/sysctl")
}

fn remove_legacy(destination: &Path) -> Result<(), String> {
    let names = [
        "99-kyth.conf",
        "99-kyth-vm-compaction.conf",
        "99-kyth-network-qdisc.conf",
        "99-kyth-swappiness.conf",
        "99-kyth-compaction.conf",
        "99-kyth-dirty-expire.conf",
        "99-kyth-dirty-ratio.conf",
        "99-kyth-inotify-watches.conf",
        "99-kyth-max-map-count.conf",
        "99-kyth-sched-autogroup.conf",
        "99-kyth-vfs-cache.conf",
        "99-kyth-vm-stat.conf",
        "99-kyth-vm-watermark.conf",
        "99-kyth-vm-compaction.conf",
        "99-kyth-net-tune.conf",
        "99-kyth-net-backlog.conf",
        "99-kyth-rmem-max.conf",
        "99-kyth-wmem-max.conf",
        "99-kyth-busy-poll.conf",
        "99-kyth-busy-read.conf",
        "99-kyth-psi-poll.conf",
        "99-kyth-page-cluster.conf",
    ];
    for name in names {
        let _ = fs::remove_file(destination.join(name));
    }
    let tunables = config_dir(None)
        .parent()
        .map(|path| path.join("tunables.toml"));
    if let Some(path) = tunables.filter(|path| path.is_file()) {
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(value) = raw.parse::<toml::Value>() {
                if let Some(table) = value.get("tunables").and_then(toml::Value::as_table) {
                    for (name, spec) in table {
                        if spec.get("kind").and_then(toml::Value::as_str) == Some("sysctl") {
                            let _ =
                                fs::remove_file(destination.join(format!("99-kyth-{name}.conf")));
                        }
                    }
                }
            }
        }
    }
    atomic_write_text("/etc/modules-load.d/bbr.conf", "tcp_bbr\n", Some(0o644))
        .map_err(|error| error.to_string())
}

fn usage() {
    eprintln!("usage: kyth-sysctl-compose [--check|--emit-all|--cat TIER] [--config-dir PATH] [--dest-root PATH]");
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let config = args
        .iter()
        .position(|value| value == "--config-dir")
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from);
    let dest = args
        .iter()
        .position(|value| value == "--dest-root")
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEST));
    let source = config_dir(config);
    let result = if let Some(index) = args.iter().position(|value| value == "--cat") {
        let tier = args.get(index + 1).cloned().unwrap_or_default();
        if !TIERS.contains(&tier.as_str()) {
            Err("unknown sysctl tier".into())
        } else {
            compose(&source)
                .map(|rendered| {
                    print!("{}", rendered[&tier]);
                })
                .map_err(|duplicates| duplicates.join("\n"))
        }
    } else if args.iter().any(|value| value == "--check") {
        let inputs = load_inputs(&source);
        let duplicates = kyth_shared::system::sysctl_compose::duplicate_keys(&inputs);
        if duplicates.is_empty() {
            println!("sysctl_compose: no duplicate keys");
            Ok(())
        } else {
            Err(duplicates.join("\n"))
        }
    } else if args.iter().any(|value| value == "--emit-all")
        || args.iter().any(|value| value == "--dest-root")
    {
        if dest == Path::new(DEST) && unsafe { libc::geteuid() } != 0 {
            Err("sysctl emission must run as root".into())
        } else {
            write_tiers(&source, &dest)
                .map_err(|error| error.to_string())
                .and_then(|_| remove_legacy(&dest))
        }
    } else {
        let inputs = load_inputs(&source);
        if kyth_shared::system::sysctl_compose::duplicate_keys(&inputs).is_empty() {
            println!("sysctl_compose: ok (use --emit-all to write)");
            Ok(())
        } else {
            Err("duplicate sysctl keys".into())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kyth-sysctl-compose: {error}");
            usage();
            ExitCode::from(1)
        }
    }
}
