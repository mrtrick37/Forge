//! Pure display-output parsers shared by diagnostics and guarded actions.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DisplayOutput {
    pub name: String,
    pub connected: bool,
    pub enabled: bool,
}

/// Parse `kscreen-doctor -o` without interpreting arbitrary command output as
/// an executable identifier. Names are validated separately by the action
/// that may use them.
pub fn parse_kscreen_outputs(text: &str) -> Vec<DisplayOutput> {
    let mut outputs = Vec::new();
    let mut current: Option<DisplayOutput> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("Output:") {
            if let Some(output) = current.take() {
                outputs.push(output);
            }
            let parts: Vec<_> = rest.split_whitespace().collect();
            let name = parts
                .get(1)
                .or_else(|| parts.first())
                .copied()
                .unwrap_or_default()
                .to_string();
            current = Some(DisplayOutput {
                name,
                connected: false,
                enabled: false,
            });
            continue;
        }
        let Some(output) = current.as_mut() else {
            continue;
        };
        match line.to_ascii_lowercase().as_str() {
            "connected" => output.connected = true,
            "disconnected" => output.connected = false,
            "enabled" => output.enabled = true,
            "disabled" => output.enabled = false,
            _ => {}
        }
    }
    if let Some(output) = current {
        outputs.push(output);
    }
    outputs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_kscreen_outputs() {
        let outputs = parse_kscreen_outputs(
            "Output: HDMI-1\n  connected\n  disabled\nOutput: DP-1\n  connected\n  enabled\n",
        );
        assert_eq!(
            outputs,
            vec![
                DisplayOutput {
                    name: "HDMI-1".into(),
                    connected: true,
                    enabled: false
                },
                DisplayOutput {
                    name: "DP-1".into(),
                    connected: true,
                    enabled: true
                },
            ]
        );
    }
}
