//! Native read-only Windows migration verification.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args == ["--json"];
    if !args.is_empty() && !json {
        eprintln!("Usage: kyth-windows-verify [--json]");
        std::process::exit(2);
    }
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/root"));
    let report = kyth_shared::system::windows_verify::verify(
        &home,
        std::path::Path::new("/var/home").exists(),
    );
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("verification report serializes")
        );
    } else {
        for (name, value) in [
            ("bookmarks", &report.bookmarks),
            ("drives", &report.drives),
            ("files", &report.files),
            ("onedrive", &report.onedrive),
            ("pwa", &report.pwa),
            ("parity", &report.parity),
        ] {
            println!("{name}: {value}");
        }
    }
    if report.parity == "ok" {
        std::process::exit(0);
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn missing_migration_is_nonzero_contract() {
        let home = tempdir().unwrap();
        let report = kyth_shared::system::windows_verify::verify(home.path(), false);
        assert_eq!(report.parity, "missing migration items");
    }

    #[test]
    fn completed_markers_are_reported_ok() {
        let home = tempdir().unwrap();
        fs::create_dir_all(home.path().join(".mozilla")).unwrap();
        fs::create_dir_all(home.path().join(".local/share/kyth")).unwrap();
        fs::write(home.path().join(".local/share/kyth/files-copy.json"), "{}").unwrap();
        fs::create_dir_all(home.path().join(".config/rclone")).unwrap();
        fs::write(home.path().join(".config/rclone/rclone.conf"), "").unwrap();
        assert_eq!(
            kyth_shared::system::windows_verify::verify(home.path(), true).parity,
            "ok"
        );
    }
}
