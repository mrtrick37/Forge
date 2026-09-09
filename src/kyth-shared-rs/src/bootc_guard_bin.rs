fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(operation) = args.next() else {
        eprintln!("usage: kyth-bootc-guard <status|status-json|switch-latest|switch-testing|switch-latest-cachy|switch-testing-cachy>");
        return std::process::ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("kyth-bootc-guard accepts one operation");
        return std::process::ExitCode::from(2);
    }
    let result = match operation.as_str() {
        "status" => kyth_shared::system::bootc_guard::status(false),
        "status-json" => kyth_shared::system::bootc_guard::status(true),
        "switch-latest" => kyth_shared::system::bootc_guard::switch("latest"),
        "switch-testing" => kyth_shared::system::bootc_guard::switch("testing"),
        "switch-latest-cachy" => kyth_shared::system::bootc_guard::switch("latest-cachy"),
        "switch-testing-cachy" => kyth_shared::system::bootc_guard::switch("testing-cachy"),
        _ => Err("unsupported operation".into()),
    };
    match result {
        Ok(output) => {
            print!("{output}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("kyth-bootc-guard: {error}");
            std::process::ExitCode::from(1)
        }
    }
}
