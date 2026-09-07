//! Safe Windows-installer inspection and Bottles planning.
//!
//! This is the read-only half of `desktop.windows_installer`: it validates PE
//! and MSI headers, captures a file identity/hash, assesses compatibility, and
//! projects a deterministic bottle plan. Staging, Flatpak installation, and
//! launching a Windows program remain explicit caller-owned actions.

use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const BOTTLES_ID: &str = "com.usebottles.bottles";
pub const FLATHUB_URL: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallerKind {
    Exe,
    Msi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Compatibility {
    Likely,
    Unknown,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowFailureKind {
    InvalidFile,
    FileChanged,
    BottlesInstall,
    BottleCreate,
    FileStage,
    Launch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallerInspectionError {
    pub kind: WorkflowFailureKind,
    pub message: String,
}

impl Display for InstallerInspectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for InstallerInspectionError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified_ns: i128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallerRequest {
    pub source: PathBuf,
    pub kind: InstallerKind,
    pub architecture: String,
    pub identity: FileIdentity,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityAssessment {
    pub level: Compatibility,
    pub summary: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BottlePlan {
    pub name: String,
    pub environment: String,
    pub architecture: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedInstaller {
    pub host_path: PathBuf,
    pub sandbox_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchResult {
    pub bottle: BottlePlan,
    pub staged: StagedInstaller,
}

fn invalid(message: impl Into<String>) -> InstallerInspectionError {
    InstallerInspectionError {
        kind: WorkflowFailureKind::InvalidFile,
        message: message.into(),
    }
}

fn failure(kind: WorkflowFailureKind, message: impl Into<String>) -> InstallerInspectionError {
    InstallerInspectionError {
        kind,
        message: message.into(),
    }
}

fn inspect_pe(source: &mut File) -> Result<String, InstallerInspectionError> {
    let mut dos_header = [0_u8; 64];
    source
        .read_exact(&mut dos_header)
        .map_err(|_| invalid("The file does not contain a valid Windows executable header."))?;
    if &dos_header[..2] != b"MZ" {
        return Err(invalid(
            "The file does not contain a valid Windows executable header.",
        ));
    }
    let pe_offset = u32::from_le_bytes(dos_header[0x3c..0x40].try_into().unwrap()) as u64;
    if pe_offset < 64 || pe_offset > 64 * 1024 * 1024 {
        return Err(invalid(
            "The Windows executable header points outside a safe inspection range.",
        ));
    }
    source
        .seek(SeekFrom::Start(pe_offset))
        .map_err(|_| invalid("The Windows executable header could not be inspected."))?;
    let mut header = [0_u8; 6];
    source.read_exact(&mut header).map_err(|_| {
        invalid("The Windows executable header points outside a safe inspection range.")
    })?;
    if &header[..4] != b"PE\0\0" {
        return Err(invalid(
            "The file has a DOS header but no valid PE executable header.",
        ));
    }
    Ok(match u16::from_le_bytes([header[4], header[5]]) {
        0x014c => "win32",
        0x8664 => "win64",
        0xaa64 => "arm64",
        _ => "unknown",
    }
    .into())
}

fn identity(path: &Path) -> Result<FileIdentity, InstallerInspectionError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("The installer could not be read: {error}")))?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
        modified_ns: i128::from(metadata.mtime()) * 1_000_000_000
            + i128::from(metadata.mtime_nsec()),
    })
}

fn sha256(path: &Path) -> Result<String, InstallerInspectionError> {
    let mut source = File::open(path)
        .map_err(|error| invalid(format!("The installer could not be read: {error}")))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|error| invalid(format!("The installer could not be read: {error}")))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn inspect_installer(
    path: impl AsRef<Path>,
) -> Result<InstallerRequest, InstallerInspectionError> {
    let path = path.as_ref();
    if path.is_symlink() || !path.is_file() {
        return Err(invalid(
            "Choose a regular, non-symbolic-link installer file.",
        ));
    }
    let resolved = path
        .canonicalize()
        .map_err(|error| invalid(format!("The installer could not be read: {error}")))?;
    let kind = match resolved
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("exe") => InstallerKind::Exe,
        Some("msi") => InstallerKind::Msi,
        _ => {
            return Err(invalid(
                "Kyth currently supports Windows .exe and .msi installers only.",
            ))
        }
    };
    let mut source = File::open(&resolved)
        .map_err(|error| invalid(format!("The installer could not be read: {error}")))?;
    let architecture = match kind {
        InstallerKind::Exe => inspect_pe(&mut source)?,
        InstallerKind::Msi => {
            let mut header = [0_u8; 8];
            source.read_exact(&mut header).map_err(|_| {
                invalid("The file does not contain a valid MSI compound-document header.")
            })?;
            if header == *b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1" {
                "win64".into()
            } else {
                return Err(invalid(
                    "The file does not contain a valid MSI compound-document header.",
                ));
            }
        }
    };
    Ok(InstallerRequest {
        source: resolved.clone(),
        kind,
        architecture,
        identity: identity(&resolved)?,
        sha256: sha256(&resolved)?,
    })
}

pub fn assess_compatibility(request: &InstallerRequest) -> CompatibilityAssessment {
    let unsupported = RegexBuilder::new(r"(?:^|[-_. ])(?:anti[-_. ]?cheat|battleye|easyanti(?:cheat)?|driver|firmware|bios|chipset|microsoft[-_. ]?store|windows[-_. ]?update)(?:$|[-_. ])").case_insensitive(true).build().expect("static compatibility pattern");
    if request.architecture == "arm64" {
        return CompatibilityAssessment { level: Compatibility::Unsupported, summary: "ARM Windows installer".into(), detail: "This installer targets Windows on ARM, which this Kyth compatibility path does not support.".into() };
    }
    let stem = request
        .source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if unsupported.is_match(stem) {
        return CompatibilityAssessment { level: Compatibility::Unsupported, summary: "System-level Windows component".into(), detail: "Drivers, firmware tools, kernel anti-cheat, and Windows system components generally cannot run through Wine.".into() };
    }
    if matches!(request.architecture.as_str(), "win32" | "win64") {
        return CompatibilityAssessment {
            level: Compatibility::Likely,
            summary: "Standard Windows installer".into(),
            detail:
                "Many conventional desktop installers work, but compatibility is not guaranteed."
                    .into(),
        };
    }
    CompatibilityAssessment {
        level: Compatibility::Unknown,
        summary: "Unknown Windows architecture".into(),
        detail:
            "Kyth can try this installer, but its architecture could not be identified reliably."
                .into(),
    }
}

pub fn plan_bottle(request: &InstallerRequest) -> BottlePlan {
    let source_stem = request
        .source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("windows-app")
        .to_ascii_lowercase();
    let separators = regex::Regex::new(r"[^a-z0-9]+").expect("static bottle name pattern");
    let mut stem = separators
        .replace_all(&source_stem, "-")
        .trim_matches('-')
        .to_string();
    for token in ["setup", "installer", "install", "update", "updater"] {
        let pattern = regex::Regex::new(&format!(r"(?:^|-){token}(?:-|$)"))
            .expect("static bottle wrapper pattern");
        stem = pattern
            .replace_all(&stem, "-")
            .trim_matches('-')
            .to_string();
    }
    stem.truncate(36);
    let architecture = matches!(request.architecture.as_str(), "win32" | "win64")
        .then_some(request.architecture.as_str())
        .unwrap_or("win64");
    let gaming = RegexBuilder::new(
        r"(?:game|gaming|steam|battle[-_. ]?net|blizzard|gog|epic|launcher|ubisoft|uplay)",
    )
    .case_insensitive(true)
    .build()
    .expect("static gaming pattern")
    .is_match(&source_stem);
    BottlePlan {
        name: format!(
            "Kyth-{}-{}",
            if stem.is_empty() {
                "windows-app"
            } else {
                &stem
            },
            &request.sha256[..request.sha256.len().min(8)]
        ),
        environment: if gaming { "gaming" } else { "application" }.into(),
        architecture: architecture.into(),
    }
}

pub fn flatpak_install_commands() -> [Vec<String>; 2] {
    [
        vec![
            "flatpak",
            "remote-add",
            "--if-not-exists",
            "--user",
            "flathub",
            FLATHUB_URL,
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        vec![
            "flatpak",
            "install",
            "-y",
            "--noninteractive",
            "--user",
            "flathub",
            BOTTLES_ID,
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    ]
}

pub fn bottles_cli(args: &[&str]) -> Vec<String> {
    [
        vec!["flatpak", "run", "--command=bottles-cli", BOTTLES_ID]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
        args.iter().map(|arg| (*arg).into()).collect(),
    ]
    .concat()
}

pub fn bottle_names(payload: &str) -> BTreeSet<String> {
    let Ok(mut value) = serde_json::from_str::<Value>(payload) else {
        return payload
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect();
    };
    if let Some(bottles) = value.get("bottles") {
        value = bottles.clone();
    }
    if let Some(object) = value.as_object() {
        return object.keys().cloned().collect();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.as_str().map(String::from).or_else(|| {
                item.get("Name")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)
                    .map(String::from)
            })
        })
        .collect()
}

fn current_identity_matches(request: &InstallerRequest) -> Result<bool, InstallerInspectionError> {
    Ok(identity(&request.source)? == request.identity)
}

/// Stage a validated installer in Bottles' private Flatpak cache. The copy is
/// re-hashed before it becomes visible to the runner, preventing a file swap
/// between inspection and launch.
pub fn stage_installer(
    request: &InstallerRequest,
    home: impl AsRef<Path>,
) -> Result<StagedInstaller, InstallerInspectionError> {
    if !current_identity_matches(request)? {
        return Err(failure(
            WorkflowFailureKind::FileChanged,
            "The installer changed after it was inspected. Reopen it to continue safely.",
        ));
    }
    let safe_name: String = request
        .source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("installer")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ' ' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let directory = home
        .as_ref()
        .join(".var/app")
        .join(BOTTLES_ID)
        .join("cache/kyth-installers")
        .join(&request.sha256[..16]);
    let host_path = directory.join(safe_name);
    std::fs::create_dir_all(&directory).map_err(|error| {
        failure(
            WorkflowFailureKind::FileStage,
            format!("Could not prepare the installer inside the Bottles sandbox: {error}"),
        )
    })?;
    if !host_path.exists() || sha256(&host_path).ok().as_deref() != Some(&request.sha256) {
        let temporary = host_path.with_extension(format!(
            "{}.part",
            host_path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
        ));
        std::fs::copy(&request.source, &temporary).map_err(|error| {
            failure(
                WorkflowFailureKind::FileStage,
                format!("Could not prepare the installer inside the Bottles sandbox: {error}"),
            )
        })?;
        if sha256(&temporary).as_deref() != Ok(request.sha256.as_str()) {
            let _ = std::fs::remove_file(&temporary);
            return Err(failure(
                WorkflowFailureKind::FileChanged,
                "The installer changed while it was being prepared. Reopen it to continue safely.",
            ));
        }
        std::fs::rename(&temporary, &host_path).map_err(|error| {
            failure(
                WorkflowFailureKind::FileStage,
                format!("Could not prepare the installer inside the Bottles sandbox: {error}"),
            )
        })?;
    }
    Ok(StagedInstaller {
        host_path: host_path.clone(),
        sandbox_path: host_path,
    })
}

fn run(
    command: &[String],
    kind: WorkflowFailureKind,
    message: &str,
    wait: bool,
) -> Result<String, InstallerInspectionError> {
    let Some((program, arguments)) = command.split_first() else {
        return Err(failure(kind, "empty command"));
    };
    let child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| failure(kind, format!("{message}: {error}")))?;
    if !wait {
        return Ok(String::new());
    }
    let output = child
        .wait_with_output()
        .map_err(|error| failure(kind, format!("{message}: {error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        })
        .trim()
        .to_string();
        return Err(failure(
            kind,
            format!(
                "{message}: {}",
                if detail.is_empty() {
                    "unknown error"
                } else {
                    &detail
                }
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn flatpak_info(id: &str) -> bool {
    Command::new("flatpak")
        .args(["info", id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Perform the fixed Bottles workflow. All process arguments are composed
/// from validated installer metadata and fixed command templates; callers do
/// not supply a program or free-form shell string.
pub fn launch_in_bottles(
    request: &InstallerRequest,
    home: impl AsRef<Path>,
) -> Result<LaunchResult, InstallerInspectionError> {
    if !flatpak_info(BOTTLES_ID) {
        let commands = flatpak_install_commands();
        run(
            &commands[0],
            WorkflowFailureKind::BottlesInstall,
            "Could not configure Flathub",
            true,
        )?;
        run(
            &commands[1],
            WorkflowFailureKind::BottlesInstall,
            "Could not install Bottles",
            true,
        )?;
    }
    let bottle = plan_bottle(request);
    let list = bottles_cli(&["--json", "list", "bottles"]);
    if !bottle_names(&run(
        &list,
        WorkflowFailureKind::BottleCreate,
        "Could not list Bottles environments",
        true,
    )?)
    .contains(&bottle.name)
    {
        let create = bottles_cli(&[
            "new",
            "--bottle-name",
            &bottle.name,
            "--environment",
            &bottle.environment,
            "--arch",
            &bottle.architecture,
        ]);
        run(
            &create,
            WorkflowFailureKind::BottleCreate,
            "Could not create the Windows environment",
            true,
        )?;
    }
    let staged = stage_installer(request, home)?;
    let launch = bottles_cli(&[
        "run",
        "-b",
        &bottle.name,
        "-e",
        staged.sandbox_path.to_str().ok_or_else(|| {
            failure(
                WorkflowFailureKind::Launch,
                "The installer path is not valid UTF-8.",
            )
        })?,
    ]);
    run(
        &launch,
        WorkflowFailureKind::Launch,
        "Bottles could not launch the installer",
        false,
    )?;
    Ok(LaunchResult { bottle, staged })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn pe(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0_u8; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&(64_u32).to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&machine.to_le_bytes());
        bytes
    }

    #[test]
    fn inspects_pe_and_msi_headers() {
        let directory = tempdir().unwrap();
        let exe = directory.path().join("Setup Game.exe");
        fs::write(&exe, pe(0x8664)).unwrap();
        let request = inspect_installer(&exe).unwrap();
        assert_eq!(request.architecture, "win64");
        assert_eq!(assess_compatibility(&request).level, Compatibility::Likely);
        assert_eq!(plan_bottle(&request).environment, "gaming");
        let msi = directory.path().join("office.msi");
        fs::write(&msi, b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1payload").unwrap();
        assert_eq!(inspect_installer(&msi).unwrap().kind, InstallerKind::Msi);
    }

    #[test]
    fn rejects_bad_headers_and_system_components() {
        let directory = tempdir().unwrap();
        let exe = directory.path().join("driver.exe");
        fs::write(&exe, b"MZbad").unwrap();
        assert!(inspect_installer(&exe).is_err());
        let request = InstallerRequest {
            source: PathBuf::from("Battleye Setup.exe"),
            kind: InstallerKind::Exe,
            architecture: "win64".into(),
            identity: FileIdentity {
                device: 0,
                inode: 0,
                size: 0,
                modified_ns: 0,
            },
            sha256: "0123456789abcdef".into(),
        };
        assert_eq!(
            assess_compatibility(&request).level,
            Compatibility::Unsupported
        );
    }

    #[test]
    fn parses_bottles_shapes_and_projects_commands() {
        assert_eq!(
            bottle_names(r#"{"bottles":{"Demo":{}}}"#),
            BTreeSet::from(["Demo".into()])
        );
        assert_eq!(
            bottle_names(r#"[{"Name":"Demo"},"Other"]"#),
            BTreeSet::from(["Demo".into(), "Other".into()])
        );
        assert_eq!(
            bottle_names("Demo\nOther\n"),
            BTreeSet::from(["Demo".into(), "Other".into()])
        );
        assert_eq!(bottles_cli(&["list"])[0], "flatpak");
        assert_eq!(flatpak_install_commands()[1].last().unwrap(), BOTTLES_ID);
    }

    #[test]
    fn stages_only_an_unchanged_regular_installer() {
        let directory = tempdir().unwrap();
        let home = tempdir().unwrap();
        let exe = directory.path().join("setup.exe");
        fs::write(&exe, pe(0x8664)).unwrap();
        let request = inspect_installer(&exe).unwrap();
        let staged = stage_installer(&request, home.path()).unwrap();
        assert!(staged
            .host_path
            .starts_with(home.path().join(".var/app").join(BOTTLES_ID)));
        assert_eq!(sha256(&staged.host_path).unwrap(), request.sha256);
        fs::write(&exe, pe(0x014c)).unwrap();
        assert_eq!(
            stage_installer(&request, home.path()).unwrap_err().kind,
            WorkflowFailureKind::FileChanged
        );
    }
}
