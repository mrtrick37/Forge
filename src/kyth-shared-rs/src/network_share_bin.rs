use std::env;
use std::io::Read;

fn main() -> std::process::ExitCode {
    if !rustix::process::getuid().is_root() {
        eprintln!("kyth-network-share must run as root");
        return std::process::ExitCode::from(77);
    }
    let mut args = env::args().skip(1);
    let Some(action) = args.next() else {
        eprintln!("usage: kyth-network-share {{add|remove}}");
        return std::process::ExitCode::from(64);
    };
    if args.next().is_some() || !matches!(action.as_str(), "add" | "remove") {
        eprintln!("usage: kyth-network-share {{add|remove}}");
        return std::process::ExitCode::from(64);
    }
    let mut payload = Vec::new();
    if let Err(error) = std::io::stdin()
        .take(64 * 1024 + 1)
        .read_to_end(&mut payload)
    {
        eprintln!("Network share {action} failed: {error}");
        return std::process::ExitCode::from(1);
    }
    match kyth_shared::network_share::run_payload(&action, &payload) {
        Ok(detail) => {
            println!("{detail}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Network share {action} failed: {error}");
            std::process::ExitCode::from(1)
        }
    }
}
