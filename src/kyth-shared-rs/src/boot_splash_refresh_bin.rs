//! Native replacement for the Python `kyth-refresh-boot-splash-initramfs`
//! launcher (installed to `/usr/libexec`, run by its systemd unit).
//!
//! Refreshes Plymouth initramfs images via `dracut` when the fingerprint,
//! marker, or image inspection says they are stale; `verify IMAGE`
//! reports inspection errors. Exit codes mirror the Python CLI (`0`
//! clean, `1` on errors, `2` for CLI misuse). `plymouth.py` stays as the
//! Phase 3 fixture.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use kyth_shared::system::plymouth::{
    DRACUT_CONF_PATH, FINGERPRINT_FILE, FINGERPRINT_INPUTS, MARKER_FILE, STATE_DIR, WATERMARK_SOURCE,
    collect_images, dracut_image_argv, dracut_regenerate_all_argv, ensure_dracut_config, fingerprint,
    inspect_listing, prepare_include,
};
use kyth_shared::system::process::run_bounded;

const DRACUT_TIMEOUT: Duration = Duration::from_secs(600);
const INSPECT_TIMEOUT: Duration = Duration::from_secs(60);

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn run_text(argv: &[String], timeout: Duration) -> Option<(bool, String)> {
    run_bounded(argv, timeout).ok().map(|output| {
        (output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned())
    })
}

fn inspect_image(image: &Path) -> Vec<String> {
    if !on_path("lsinitrd") {
        return vec!["lsinitrd is unavailable".to_string()];
    }
    let image_str = image.to_string_lossy().into_owned();
    let listing = run_text(&["lsinitrd".to_string(), image_str.clone()], INSPECT_TIMEOUT);
    let defaults = run_text(
        &[
            "lsinitrd".to_string(),
            "-f".to_string(),
            "/usr/share/plymouth/plymouthd.defaults".to_string(),
            image_str.clone(),
        ],
        INSPECT_TIMEOUT,
    );
    let logo = run_bounded(
        &[
            "lsinitrd".to_string(),
            "-f".to_string(),
            "/usr/share/pixmaps/system-logo-white.png".to_string(),
            image_str,
        ],
        INSPECT_TIMEOUT,
    )
    .ok();
    let Some((listing_ok, listing_text)) = listing else {
        return vec![format!("unable to inspect refreshed initramfs: {}", image.display())];
    };
    if !listing_ok {
        return vec![format!("unable to inspect refreshed initramfs: {}", image.display())];
    }
    let defaults_text = defaults.and_then(|(ok, text)| ok.then_some(text));
    // Mirror run_optional: a spawned-but-failed logo probe reports the
    // missing-logo error (not the watermark comparison), so only a
    // successful probe contributes bytes; the empty watermark sentinel
    // keeps inspect_listing on the missing-logo branch.
    let (logo_bytes, watermark_bytes) = match logo.filter(|output| output.status.success()) {
        Some(output) => (Some(output.stdout), std::fs::read(WATERMARK_SOURCE).ok()),
        None => (None, Some(Vec::new())),
    };
    inspect_listing(
        image,
        &listing_text,
        defaults_text.as_deref(),
        logo_bytes.as_deref(),
        watermark_bytes.as_deref(),
    )
}

fn image_needs_refresh(image: &Path) -> bool {
    !inspect_image(image).is_empty()
}

fn refresh_image(image: &Path, include: &Path) -> Result<(), String> {
    let (argv, _kernel) = dracut_image_argv(image, include);
    let output = run_bounded(&argv, DRACUT_TIMEOUT)
        .map_err(|error| format!("dracut failed for {}: {error}", image.display()))?;
    if !output.status.success() {
        return Err(format!("dracut failed for {}", image.display()));
    }
    let errors = inspect_image(image);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn refresh_writable() -> Result<i32, String> {
    std::fs::create_dir_all(STATE_DIR).map_err(|error| error.to_string())?;
    let fp_file = Path::new(FINGERPRINT_FILE);
    let marker = Path::new(MARKER_FILE);
    let _ = run_bounded(
        &["plymouth-set-default-theme".to_string(), "kyth".to_string()],
        Duration::from_secs(60),
    );
    for guard in
        ["/usr/libexec/kyth-boot-branding-guard", "/usr/libexec/kyth-plymouth-branding-guard"]
    {
        if Path::new(guard).is_file() {
            let _ = run_bounded(&[guard.to_string()], Duration::from_secs(120));
        }
    }
    let _ = ensure_dracut_config(Path::new(DRACUT_CONF_PATH));
    let inputs: Vec<&Path> = FINGERPRINT_INPUTS.iter().map(Path::new).collect();
    let current = fingerprint(&inputs);
    let boot = Path::new("/boot");
    let modules = Path::new("/usr/lib/modules");
    let images = collect_images(boot, modules);
    let old = fp_file
        .is_file()
        .then(|| std::fs::read_to_string(fp_file).unwrap_or_default().trim().to_string())
        .unwrap_or_default();
    if marker.exists()
        && old == current
        && !images.is_empty()
        && !images.iter().any(|image| image_needs_refresh(image))
    {
        return Ok(0);
    }
    let _tmp = TempDir::create().map_err(|error| error.to_string())?;
    let include = _tmp.path();
    prepare_include(include).map_err(|error| error.to_string())?;
    if !images.is_empty() {
        for image in &images {
            refresh_image(image, include)?;
        }
    } else {
        let argv = dracut_regenerate_all_argv(include);
        let output = run_bounded(&argv, DRACUT_TIMEOUT).map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err("dracut --regenerate-all failed".to_string());
        }
        for image in collect_images(boot, modules) {
            let errors = inspect_image(&image);
            if !errors.is_empty() {
                return Err(errors.join("\n"));
            }
        }
    }
    std::fs::write(fp_file, format!("{current}\n")).map_err(|error| error.to_string())?;
    std::fs::write(marker, b"").map_err(|error| error.to_string())?;
    Ok(0)
}

/// Best-effort tempdir under `/tmp`, removed on drop (mirrors the Python
/// `TemporaryDirectory(prefix=..., dir="/tmp")`).
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn create() -> std::io::Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path =
            Path::new("/tmp").join(format!("kyth-plymouth-initramfs-{}.{}", std::process::id(), nanos));
        std::fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn boot_is_readonly() -> bool {
    run_text(
        &["findmnt".to_string(), "-no".to_string(), "OPTIONS".to_string(), "/boot".to_string()],
        Duration::from_secs(10),
    )
    .map(|(ok, text)| ok && text.trim().split(',').any(|option| option == "ro"))
    .unwrap_or(false)
}

fn refresh() -> i32 {
    let readonly = boot_is_readonly();
    if readonly {
        let remounted = run_bounded(
            &["mount".to_string(), "-o".to_string(), "remount,bind,rw".to_string(), "/boot".to_string()],
            Duration::from_secs(30),
        )
        .map(|output| output.status.success())
        .unwrap_or(false)
            || run_bounded(
                &["mount".to_string(), "-o".to_string(), "remount,rw".to_string(), "/boot".to_string()],
                Duration::from_secs(30),
            )
            .map(|output| output.status.success())
            .unwrap_or(false);
        if !remounted {
            eprintln!(
                "WARNING: /boot is read-only and could not be remounted; skipping initramfs refresh"
            );
            return 0;
        }
    }
    let result = refresh_writable();
    if readonly {
        let _ = run_bounded(
            &["mount".to_string(), "-o".to_string(), "remount,ro".to_string(), "/boot".to_string()],
            Duration::from_secs(30),
        );
    }
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ERROR: {error}");
            1
        }
    }
}

fn main() -> ExitCode {
    // dracut inherits TMPDIR=/var/tmp from the Python launcher's env.
    env::set_var("TMPDIR", "/var/tmp");
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "refresh".to_string());
    match command.as_str() {
        "verify" => {
            let Some(image) = args.next() else {
                eprintln!("usage: kyth-refresh-boot-splash-initramfs [refresh|verify IMAGE]");
                eprintln!("error: verify requires an image");
                return ExitCode::from(2);
            };
            let errors = inspect_image(Path::new(&image));
            for error in &errors {
                eprintln!("ERROR: {error}");
            }
            ExitCode::from(u8::from(!errors.is_empty()))
        }
        "refresh" => ExitCode::from(refresh() as u8),
        _ => {
            eprintln!("usage: kyth-refresh-boot-splash-initramfs [refresh|verify IMAGE]");
            eprintln!("error: invalid choice");
            ExitCode::from(2)
        }
    }
}
