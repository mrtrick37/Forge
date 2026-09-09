//! Pure release-publication metadata and command planning.
//!
//! GitHub release creation, artifact upload, and notes-file writes remain
//! build/release orchestration. This module makes their validated presentation
//! and static argv reusable without starting `gh`.

use serde::Serialize;

pub const PUBLIC_ASSET_BASE_URL: &str = "https://pub-9a3cc72972ea44c4ae7504ee7cda1fa6.r2.dev";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleasePresentation {
    pub channel_title: String,
    pub immutable_title: String,
    pub prerelease: bool,
    pub latest: bool,
}

pub fn presentation(source_tag: &str, release_id: &str) -> ReleasePresentation {
    if source_tag == "testing" {
        ReleasePresentation {
            channel_title: "Kyth Live ISO - Testing".into(),
            immutable_title: format!("Kyth Live ISO - Testing - {release_id}"),
            prerelease: true,
            latest: false,
        }
    } else {
        ReleasePresentation {
            channel_title: "Kyth Live ISO - Stable".into(),
            immutable_title: format!("Kyth Live ISO - Stable - {release_id}"),
            prerelease: false,
            latest: true,
        }
    }
}

pub fn asset_url(basename: &str) -> String {
    format!("{PUBLIC_ASSET_BASE_URL}/{basename}")
}

pub fn immutable_release_url(repository: &str, immutable_tag: &str) -> String {
    format!("https://github.com/{repository}/releases/tag/{immutable_tag}")
}

pub fn create_command(
    tag: &str,
    target: &str,
    title: &str,
    notes_file: &str,
    prerelease: bool,
    latest: bool,
) -> Vec<String> {
    let mut command = vec![
        "gh".into(),
        "release".into(),
        "create".into(),
        tag.into(),
        "--target".into(),
        target.into(),
        "--title".into(),
        title.into(),
        "--notes-file".into(),
        notes_file.into(),
    ];
    if prerelease {
        command.push("--prerelease".into());
    }
    if latest {
        command.push("--latest".into());
    }
    command
}

pub fn upload_command(
    tag: &str,
    files: impl IntoIterator<Item = impl Into<String>>,
    clobber: bool,
) -> Vec<String> {
    let mut command = vec!["gh".into(), "release".into(), "upload".into(), tag.into()];
    if clobber {
        command.push("--clobber".into());
    }
    command.extend(files.into_iter().map(Into::into));
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testing_and_stable_presentation_match_release_policy() {
        let testing = presentation("testing", "iso-testing-abc");
        assert!(testing.prerelease);
        assert!(!testing.latest);
        assert_eq!(
            testing.immutable_title,
            "Kyth Live ISO - Testing - iso-testing-abc"
        );
        let stable = presentation("stable", "iso-stable-abc");
        assert!(!stable.prerelease);
        assert!(stable.latest);
    }

    #[test]
    fn command_projection_keeps_flags_and_files_as_separate_argv() {
        let create = create_command("testing", "sha256:abc", "Testing", "notes.md", true, false);
        assert_eq!(
            create,
            vec![
                "gh",
                "release",
                "create",
                "testing",
                "--target",
                "sha256:abc",
                "--title",
                "Testing",
                "--notes-file",
                "notes.md",
                "--prerelease"
            ]
        );
        let upload = upload_command("testing", ["iso.sig", "iso.bundle"], true);
        assert_eq!(
            upload,
            vec![
                "gh",
                "release",
                "upload",
                "testing",
                "--clobber",
                "iso.sig",
                "iso.bundle"
            ]
        );
    }
}
