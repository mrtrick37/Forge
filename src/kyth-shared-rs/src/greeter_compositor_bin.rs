//! Exec kwin_wayland with the bounded Kyth software-composition policy.

use std::os::unix::process::CommandExt;
use std::{env, path::PathBuf, process::Command};

fn find_binary(name: &str) -> Option<PathBuf> {
    env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(|dir| PathBuf::from(dir).join(name))
        .find(|path| path.is_file())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let binary_name = "kwin_wayland";
    let Some(binary) = find_binary(binary_name) else {
        eprintln!("kyth-greeter-compositor: {binary_name} not found");
        std::process::exit(127);
    };
    if !kyth_shared::system::wayland::has_drm_card("/dev/dri") {
        eprintln!("kyth-greeter-compositor: no DRM card; kwin_wayland may fail. Ctrl+Alt+F3, then journalctl -u plasmalogin -b. Reboot without nomodeset if the GPU works.");
    }
    let mut command = Command::new(&binary);
    command.args(if args.is_empty() {
        kyth_shared::system::wayland::compositor_argv(&[])[1..].to_vec()
    } else {
        args
    });
    if kyth_shared::system::wayland::needs_software_compose(
        "/dev/dri",
        env::var("KYTH_CMDLINE").ok().as_deref(),
    ) {
        command.envs(kyth_shared::system::wayland::software_compose_env());
    }
    let error = command.exec();
    eprintln!("kyth-greeter-compositor: could not exec {binary_name}: {error}");
    std::process::exit(126);
}
