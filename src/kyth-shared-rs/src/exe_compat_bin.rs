//! Native offline EXE compatibility lookup.
use std::path::PathBuf;
fn main() {
    let Some(name) = std::env::args().nth(1) else {
        eprintln!("Usage: kyth-exe-compat <exe>");
        std::process::exit(1);
    };
    let path = PathBuf::from(&name);
    let result = kyth_shared::system::exe_compat::check_exe(
        &path,
        &kyth_shared::system::exe_compat::load_compat(
            kyth_shared::system::exe_compat::DEFAULT_COMPAT_PATH,
        ),
    );
    println!("{} via {}: {}", result.status, result.runner, result.reason);
    if result.runner == "Bottles" {
        println!("Run with: bottles-cli run {}  or  Lutris", name);
    }
}
