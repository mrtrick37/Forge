//! Pure container-wrapper generation.
//!
//! The generated wrapper still performs the existing caller-owned Distrobox
//! checks and launch.  This module only owns deterministic text generation;
//! it never creates a container or executes a command.

/// Render the wrapper used for tools managed in the Kyth AI developer box.
pub fn render_distrobox_wrapper(tool: &str, description: &str, box_name: &str) -> String {
    format!(
        "#!/usr/bin/env bash\nset -euo pipefail\n\ntool=\"{tool}\"\ndesc=\"{description}\"\nbox=\"${{KYTH_AI_DEV_BOX:-{box_name}}}\"\n\nif [[ -x \"${{HOME}}/.local/bin/${{tool}}\" ]]; then\n\texec \"${{HOME}}/.local/bin/${{tool}}\" \"$@\"\nfi\n\nif command -v distrobox >/dev/null 2>&1 && distrobox list --no-color 2>/dev/null | awk '{{print $3}}' | grep -qx \"${{box}}\"; then\n\texec distrobox enter \"${{box}}\" -- \"${{tool}}\" \"$@\"\nfi\n\necho \"${{desc}} is managed in the KythOS AI Developer container (${{box}}).\"\necho \"Initializing ${{box}} environment...\"\nkyth-ai-dev setup\nexec distrobox enter \"${{box}}\" -- \"${{tool}}\" \"$@\"\n",
        tool = tool,
        description = description,
        box_name = box_name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_is_deterministic_and_uses_runtime_box_override() {
        let wrapper = render_distrobox_wrapper("ollama", "Ollama", "kyth-ai-dev");
        assert!(wrapper.starts_with("#!/usr/bin/env bash\nset -euo pipefail\n"));
        assert!(wrapper.contains("box=\"${KYTH_AI_DEV_BOX:-kyth-ai-dev}\""));
        assert!(wrapper.contains("distrobox enter \"${box}\" -- \"${tool}\" \"$@\""));
        assert_eq!(
            wrapper,
            render_distrobox_wrapper("ollama", "Ollama", "kyth-ai-dev")
        );
    }
}
