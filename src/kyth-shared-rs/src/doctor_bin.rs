//! Native CLI for the read-only KythOS doctor report.

fn render_report(report: &kyth_shared::doctor::DoctorReport) -> String {
    let mut output = format!("KythOS health: {}/100\n", report.score);
    for check in &report.checks {
        output.push_str(" - ");
        output.push_str(check);
        output.push('\n');
    }
    if !report.suggestions.is_empty() {
        output.push_str("\nSuggestions (just):\n");
        for suggestion in &report.suggestions {
            output.push_str("  * ");
            output.push_str(suggestion);
            output.push('\n');
        }
    }
    output
}

fn main() {
    print!("{}", render_report(&kyth_shared::doctor::collect_report()));
}

#[cfg(test)]
mod tests {
    use super::render_report;
    use kyth_shared::doctor::DoctorReport;

    #[test]
    fn preserves_python_human_report_shape() {
        let report = DoctorReport {
            score: 40,
            checks: vec!["kernel: fedora (default)".into(), "zram: no".into()],
            suggestions: vec!["Enable zram: systemctl enable --now kyth-zram-swap.service".into()],
        };
        assert_eq!(
            render_report(&report),
            "KythOS health: 40/100\n - kernel: fedora (default)\n - zram: no\n\nSuggestions (just):\n  * Enable zram: systemctl enable --now kyth-zram-swap.service\n"
        );
    }

    #[test]
    fn omits_suggestions_section_when_empty() {
        let report = DoctorReport {
            score: 100,
            checks: vec!["btrfs: yes".into()],
            suggestions: vec![],
        };
        assert_eq!(
            render_report(&report),
            "KythOS health: 100/100\n - btrfs: yes\n"
        );
    }
}
