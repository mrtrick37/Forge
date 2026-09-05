mod installer_accounts;
mod installer_bootc;
mod installer_configuration;
mod installer_daemon;
mod installer_executor;
mod installer_job;
mod installer_job_executor;
mod installer_plan;
mod installer_runtime;
mod installer_secure_boot;
mod installer_storage;
mod installer_stream;

fn main() -> std::process::ExitCode {
    match installer_daemon::run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(1)
        }
    }
}
