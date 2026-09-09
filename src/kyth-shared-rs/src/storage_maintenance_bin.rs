fn main() -> std::process::ExitCode {
    match kyth_shared::system::storage_maintenance::run_maintenance() {
        Ok(detail) => {
            println!("{detail}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("kyth-btrfs-maint: {error}");
            std::process::ExitCode::from(1)
        }
    }
}
