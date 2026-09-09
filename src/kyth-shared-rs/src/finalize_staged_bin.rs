fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    if args.next().is_some() || !matches!(mode.as_str(), "" | "prepare-boot" | "reboot") {
        eprintln!("usage: kyth-finalize-staged [prepare-boot|reboot]");
        return std::process::ExitCode::from(2);
    }
    let result = if mode == "prepare-boot" {
        kyth_shared::system::boot_finalize::prepare_boot().map(|_| String::new())
    } else {
        kyth_shared::system::boot_finalize::finalize_staged(mode == "reboot")
    };
    match result {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("kyth-finalize-staged: {error}");
            std::process::ExitCode::from(1)
        }
    }
}
