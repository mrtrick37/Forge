//! Pure Plymouth policy and initramfs inspection helpers.
//!
//! The Python Plymouth module still owns filesystem mutation and `dracut`
//! execution. This module keeps deterministic inputs and inspection rules
//! reusable by Rust callers without starting privileged processes.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const THEME: &str = "kyth";
pub const FALLBACK_THEMES: &[&str] = &["bgrt-fedora", "bgrt", "spinner"];
pub const REQUIRED_ENTRIES: &[&str] = &[
    "usr/share/plymouth/themes/kyth/kyth.plymouth",
    "usr/share/plymouth/themes/kyth/kyth.script",
    "usr/share/plymouth/themes/kyth/kyth-logo.png",
    "usr/share/plymouth/themes/default.plymouth",
];
pub const PLYMOUTH_CONFIG: &str =
    "[Daemon]\nTheme=kyth\nShowDelay=0\nDeviceTimeout=8\nUseFirmwareBackground=false\n";
pub const DRACUT_CONF_PATH: &str = "/etc/dracut.conf.d/99-kyth.conf";
pub const STATE_DIR: &str = "/var/lib/kyth";
pub const FINGERPRINT_FILE: &str = "/var/lib/kyth/boot-splash-initramfs.sha256";
pub const MARKER_FILE: &str = "/var/lib/kyth/boot-splash-initramfs-v17";
pub const FINGERPRINT_INPUTS: &[&str] = &[
    "/usr/lib/dracut/modules.d/99kyth-plymouth/module-setup.sh",
    "/usr/libexec/kyth-plymouth-branding-guard",
    "/etc/dracut.conf.d/99-kyth.conf",
    "/etc/plymouth/plymouthd.conf",
    "/usr/share/plymouth/plymouthd.defaults",
    "/usr/share/kyth/branding/transparent-watermark.png",
    "/usr/share/pixmaps/system-logo-white.png",
    "/usr/share/plymouth/themes/kyth/kyth.plymouth",
    "/usr/share/plymouth/themes/kyth/kyth.script",
    "/usr/share/plymouth/themes/kyth/kyth-logo.png",
];
pub const THEMES_SOURCE: &str = "/usr/share/plymouth/themes/kyth";
pub const WATERMARK_SOURCE: &str = "/usr/share/kyth/branding/transparent-watermark.png";
/// Transparent 1x1 PNG fallback, byte-identical to the Python base64 blob.
pub const TRANSPARENT_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 96, 96, 0, 0, 0, 3, 0, 1, 43,
    9, 77, 132, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// Return the stable Plymouth fingerprint used to decide whether a refresh is
/// needed. Missing files are represented explicitly.
pub fn fingerprint(paths: &[&Path]) -> String {
    let mut digest = Sha256::new();
    for path in paths {
        match std::fs::read(path) {
            Ok(content) => {
                let item = Sha256::digest(content);
                digest.update(format!("{:x}  {}\n", item, path.display()).as_bytes());
            }
            Err(_) => digest.update(format!("MISSING  {}\n", path.display()).as_bytes()),
        }
    }
    format!("{:x}", digest.finalize())
}

fn kernel_name(path: &Path) -> Option<&str> {
    path.file_name()?
        .to_str()?
        .strip_prefix("initramfs-")?
        .strip_suffix(".img")
}

/// Find initramfs images that have a matching kernel module directory.
pub fn collect_images(boot: &Path, modules: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(boot) {
        candidates.extend(entries.filter_map(Result::ok).map(|entry| entry.path()));
    }
    let ostree = boot.join("ostree");
    if let Ok(revisions) = std::fs::read_dir(ostree) {
        for revision in revisions.filter_map(Result::ok) {
            if let Ok(entries) = std::fs::read_dir(revision.path()) {
                candidates.extend(entries.filter_map(Result::ok).map(|entry| entry.path()));
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
        .into_iter()
        .filter(|path| {
            path.is_file() && kernel_name(path).is_some_and(|kernel| modules.join(kernel).is_dir())
        })
        .collect()
}

/// Inspect already-collected `lsinitrd` output. Process execution is kept
/// outside the shared crate; callers provide command output and optional file
/// contents from their bounded runner.
pub fn inspect_listing(
    image: &Path,
    listing: &str,
    defaults: Option<&str>,
    logo: Option<&[u8]>,
    watermark: Option<&[u8]>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for entry in REQUIRED_ENTRIES {
        if !listing.contains(entry) {
            errors.push(format!(
                "refreshed initramfs is missing {entry}: {}",
                image.display()
            ));
        }
    }
    if FALLBACK_THEMES
        .iter()
        .any(|theme| listing.contains(&format!("usr/share/plymouth/themes/{theme}/")))
    {
        errors.push(format!(
            "Plymouth fallback theme leaked into refreshed initramfs: {}",
            image.display()
        ));
    }
    match defaults {
        Some(defaults) => {
            for setting in ["Theme=kyth", "ShowDelay=0", "DeviceTimeout=8"] {
                if !defaults.lines().any(|line| line == setting) {
                    errors.push(format!(
                        "refreshed initramfs Plymouth defaults are missing {setting}: {}",
                        image.display()
                    ));
                }
            }
        }
        None => errors.push(format!(
            "refreshed initramfs is missing Plymouth defaults: {}",
            image.display()
        )),
    }
    if let Some(logo) = logo {
        match watermark {
            Some(watermark) if logo != watermark => errors.push(format!(
                "refreshed initramfs contains the wrong Plymouth system logo: {}",
                image.display()
            )),
            None => errors.push("transparent Plymouth watermark is unavailable".into()),
            _ => {}
        }
    } else {
        errors.push(format!(
            "refreshed initramfs is missing transparent Plymouth system logo: {}",
            image.display()
        ));
    }
    errors
}

/// Pure dracut-conf reconciliation, mirroring `ensure_dracut_config`.
pub fn reconcile_dracut_config(current: Option<&str>) -> String {
    let mut text = current
        .unwrap_or("add_dracutmodules+=\" ostree drm plymouth kyth-plymouth \"\n")
        .to_string();
    if !text.contains("add_dracutmodules") || !text.contains("kyth-plymouth") {
        text += "\nadd_dracutmodules+=\" kyth-plymouth \"\n";
    }
    let after_force = text
        .find("force_add_dracutmodules")
        .map(|index| &text[index..])
        .unwrap_or("");
    if !text.contains("force_add_dracutmodules") || !after_force.contains("kyth-plymouth") {
        text += "force_add_dracutmodules+=\" kyth-plymouth \"\n";
    }
    text
}

pub fn ensure_dracut_config(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let current = std::fs::read_to_string(path).ok();
    crate::atomic_io::atomic_write_text(path, &reconcile_dracut_config(current.as_deref()), None)
}

fn copy_tree(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            let link = std::fs::read_link(entry.path())?;
            std::os::unix::fs::symlink(link, &target)?;
        } else if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
            std::fs::set_permissions(&target, entry.metadata()?.permissions())?;
        }
    }
    Ok(())
}

/// Builds the dracut include tree under `root`, mirroring `_prepare_include`.
pub fn prepare_include(root: &Path) -> std::io::Result<()> {
    let config = root.join("etc/plymouth/plymouthd.conf");
    let defaults = root.join("usr/share/plymouth/plymouthd.defaults");
    let logo = root.join("usr/share/pixmaps/system-logo-white.png");
    let themes = root.join("usr/share/plymouth/themes");
    for path in [&config, &defaults, &logo] {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::create_dir_all(&themes)?;
    std::fs::write(&config, PLYMOUTH_CONFIG)?;
    std::fs::copy(&config, &defaults)?;
    let watermark = Path::new(WATERMARK_SOURCE);
    if watermark.is_file() {
        std::fs::copy(watermark, &logo)?;
    } else {
        std::fs::write(&logo, TRANSPARENT_PNG)?;
    }
    copy_tree(Path::new(THEMES_SOURCE), &themes.join("kyth"))?;
    std::os::unix::fs::symlink("kyth/kyth.plymouth", themes.join("default.plymouth"))?;
    Ok(())
}

pub fn dracut_image_argv(image: &Path, include: &Path) -> (Vec<String>, String) {
    let base = image
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let kernel = base.strip_prefix("initramfs-").unwrap_or(&base);
    let kernel = kernel.strip_suffix(".img").unwrap_or(kernel).to_string();
    let include_str = include.to_string_lossy().into_owned();
    let image_str = image.to_string_lossy().into_owned();
    (
        vec![
            "dracut".to_string(),
            "--tmpdir".to_string(),
            "/var/tmp".to_string(),
            "--no-hostonly".to_string(),
            "--kver".to_string(),
            kernel.clone(),
            "--reproducible".to_string(),
            "--force".to_string(),
            "--add".to_string(),
            "drm plymouth ostree kyth-plymouth".to_string(),
            "--include".to_string(),
            format!("{include_str}/etc/plymouth"),
            "/etc/plymouth".to_string(),
            "--include".to_string(),
            format!("{include_str}/usr/share/plymouth"),
            "/usr/share/plymouth".to_string(),
            "--include".to_string(),
            format!("{include_str}/usr/share/pixmaps/system-logo-white.png"),
            "/usr/share/pixmaps/system-logo-white.png".to_string(),
            image_str,
            kernel.clone(),
        ],
        kernel,
    )
}

pub fn dracut_regenerate_all_argv(include: &Path) -> Vec<String> {
    let include_str = include.to_string_lossy().into_owned();
    vec![
        "dracut".to_string(),
        "--tmpdir".to_string(),
        "/var/tmp".to_string(),
        "--regenerate-all".to_string(),
        "--force".to_string(),
        "--add".to_string(),
        "drm plymouth ostree kyth-plymouth".to_string(),
        "--include".to_string(),
        format!("{include_str}/etc/plymouth"),
        "/etc/plymouth".to_string(),
        "--include".to_string(),
        format!("{include_str}/usr/share/plymouth"),
        "/usr/share/plymouth".to_string(),
        "--include".to_string(),
        format!("{include_str}/usr/share/pixmaps/system-logo-white.png"),
        "/usr/share/pixmaps/system-logo-white.png".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fingerprint_changes_for_content_and_missing_files() {
        let directory = tempdir().unwrap();
        let present = directory.path().join("present");
        let missing = directory.path().join("missing");
        std::fs::write(&present, b"one").unwrap();
        let first = fingerprint(&[present.as_path(), missing.as_path()]);
        std::fs::write(&present, b"two").unwrap();
        let second = fingerprint(&[present.as_path(), missing.as_path()]);
        assert_ne!(first, second);
    }

    #[test]
    fn image_collection_is_filtered_and_sorted() {
        let directory = tempdir().unwrap();
        let boot = directory.path().join("boot");
        let modules = directory.path().join("modules");
        std::fs::create_dir_all(boot.join("ostree/rev")).unwrap();
        std::fs::create_dir_all(modules.join("6.1")).unwrap();
        std::fs::write(boot.join("initramfs-6.1.img"), b"").unwrap();
        std::fs::write(boot.join("initramfs-no-module.img"), b"").unwrap();
        std::fs::write(boot.join("ostree/rev/initramfs-6.1.img"), b"").unwrap();
        assert_eq!(collect_images(&boot, &modules).len(), 2);
    }

    #[test]
    fn dracut_config_reconciliation_matches_python() {
        assert_eq!(
            reconcile_dracut_config(None),
            "add_dracutmodules+=\" ostree drm plymouth kyth-plymouth \"\nforce_add_dracutmodules+=\" kyth-plymouth \"\n"
        );
        let current = "add_dracutmodules+=\" ostree \"\n";
        let reconciled = reconcile_dracut_config(Some(current));
        assert!(reconciled.contains("add_dracutmodules+=\" kyth-plymouth \""));
        assert!(reconciled.contains("force_add_dracutmodules+=\" kyth-plymouth \""));
        let stable = "add_dracutmodules+=\" ostree kyth-plymouth \"\nforce_add_dracutmodules+=\" kyth-plymouth \"\n";
        assert_eq!(reconcile_dracut_config(Some(stable)), stable);
    }

    #[test]
    fn dracut_image_argv_orders_flags_like_python() {
        let (argv, kernel) =
            dracut_image_argv(Path::new("/boot/initramfs-6.1.img"), Path::new("/tmp/inc"));
        assert_eq!(kernel, "6.1");
        assert_eq!(argv[0], "dracut");
        assert!(argv.contains(&"--kver".to_string()));
        assert_eq!(argv[argv.len() - 2], "/boot/initramfs-6.1.img");
        assert_eq!(argv[argv.len() - 1], "6.1");
        assert_eq!(
            dracut_regenerate_all_argv(Path::new("/tmp/inc"))[3],
            "--regenerate-all"
        );
    }

    #[test]
    fn listing_inspection_reports_missing_entries_and_fallbacks() {
        let image = Path::new("/boot/initramfs-6.1.img");
        let errors = inspect_listing(
            image,
            "usr/share/plymouth/themes/spinner/",
            None,
            None,
            None,
        );
        assert!(errors.iter().any(|error| error.contains("fallback theme")));
        assert!(errors
            .iter()
            .any(|error| error.contains("Plymouth defaults")));
        assert!(errors.iter().any(|error| error.contains("system logo")));
    }
}
