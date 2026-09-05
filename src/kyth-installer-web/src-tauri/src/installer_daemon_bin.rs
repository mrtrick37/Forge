mod installer_accounts;
mod installer_alongside;
mod installer_bootc;
mod installer_configuration;
mod installer_daemon;
mod installer_disk;
mod installer_executor;
mod installer_job;
mod installer_job_executor;
mod installer_journal;
mod installer_manual;
mod installer_mount;
mod installer_orchestration;
mod installer_plan;
mod installer_probe;
mod installer_readonly;
mod installer_recovery;
mod installer_runtime;
mod installer_secure_boot;
mod installer_storage;
mod installer_stream;
mod installer_transaction;

fn main() -> std::process::ExitCode {
    match installer_daemon::run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(1)
        }
    }
}
