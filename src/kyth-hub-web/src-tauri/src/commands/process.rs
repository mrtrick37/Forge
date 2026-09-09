use std::process::Output;
use std::time::Duration;

/// Common error type for helper-process launches from Tauri commands.
#[derive(Debug)]
pub(crate) enum CommandError {
    Spawn(std::io::Error),
    Output(std::io::Error),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "could not start command: {error}"),
            Self::Output(error) => write!(f, "could not read command output: {error}"),
        }
    }
}

/// Run a fixed helper command and keep process launching/error conversion in
/// one place. Callers decide whether non-zero exit status is a failure,
/// because some probes intentionally use exit status as data.
pub(crate) fn output(program: &str, args: &[&str]) -> Result<Output, CommandError> {
    let argv = std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(900)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CommandError::Spawn(error)
        } else {
            CommandError::Output(error)
        }
    })
}

/// Prevent helper output from becoming an unbounded UI error message.
pub(crate) fn bounded_text(bytes: &[u8]) -> String {
    let text = kyth_shared::system::process::strip_ansi(&String::from_utf8_lossy(bytes));
    kyth_shared::system::process::redact_sensitive_text(text.trim())
        .chars()
        .take(400)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{bounded_text, output, CommandError};

    #[test]
    fn bounded_text_trims_and_caps_helper_output() {
        let text = format!("  {}  ", "x".repeat(500));
        let bounded = bounded_text(text.as_bytes());
        assert_eq!(bounded.len(), 400);
        assert!(!bounded.starts_with(' '));
    }

    #[test]
    fn missing_helper_is_a_typed_spawn_error() {
        let error =
            output("/definitely/missing/kyth-helper", &[]).expect_err("missing helper should fail");
        assert!(matches!(error, CommandError::Spawn(_)));
    }
}
