//! Canonical immutable release names used by the ISO build.
//!
//! The transformation is pure and independently testable. Resolving HEAD
//! and appending to GitHub's output file belong to the CLI wrapper.

use serde::Serialize;

pub const SUPPORTED_CHANNELS: &[&str] = &["latest", "testing"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseIdentity {
    pub source_sha: String,
    pub release_id: String,
    pub iso_basename: String,
    pub channel_basename: String,
    pub immutable_tag: String,
    pub artifact_name: String,
}

pub fn build_identity(
    source_tag: &str,
    source_sha: &str,
    run_number: &str,
    run_attempt: &str,
    build_date: Option<&str>,
) -> Result<ReleaseIdentity, String> {
    if !SUPPORTED_CHANNELS.contains(&source_tag) {
        return Err(format!("unsupported release channel: {source_tag}"));
    }
    if source_sha.len() < 8 {
        return Err("source SHA must contain at least eight characters".into());
    }
    let date = build_date
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(current_utc_date);
    let release_id = format!(
        "{}-{}-{}-{}",
        date,
        &source_sha[..8],
        run_number,
        run_attempt
    );
    Ok(ReleaseIdentity {
        source_sha: source_sha.into(),
        release_id: release_id.clone(),
        iso_basename: format!("kyth-live-{source_tag}-{release_id}.iso"),
        channel_basename: format!("kyth-live-{source_tag}.iso"),
        immutable_tag: format!("iso-{source_tag}-{release_id}"),
        artifact_name: format!("kyth-live-iso-{source_tag}-{release_id}"),
    })
}

impl ReleaseIdentity {
    pub fn github_output(&self) -> String {
        format!(
            "source_sha={}\nrelease_id={}\niso_basename={}\nchannel_basename={}\nimmutable_tag={}\nartifact_name={}\n",
            self.source_sha, self.release_id, self.iso_basename, self.channel_basename, self.immutable_tag, self.artifact_name
        )
    }
}

fn current_utc_date() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }).div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524
        - day_of_era / 146_096)
        .div_euclid(365);
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2).div_euclid(153);
    let day = day_of_year - (153 * month_part + 2).div_euclid(5) + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_canonical_testing_identity() {
        let identity =
            build_identity("testing", "0123456789abcdef", "42", "3", Some("20260829")).unwrap();
        assert_eq!(identity.release_id, "20260829-01234567-42-3");
        assert_eq!(
            identity.iso_basename,
            "kyth-live-testing-20260829-01234567-42-3.iso"
        );
        assert_eq!(identity.channel_basename, "kyth-live-testing.iso");
        assert_eq!(identity.immutable_tag, "iso-testing-20260829-01234567-42-3");
    }

    #[test]
    fn rejects_unsupported_channel_and_short_sha() {
        assert!(build_identity("stable", "01234567", "1", "1", Some("20260829")).is_err());
        assert!(build_identity("latest", "short", "1", "1", Some("20260829")).is_err());
    }

    #[test]
    fn github_output_has_stable_field_order() {
        let identity = build_identity("latest", "0123456789", "1", "1", Some("20260829")).unwrap();
        assert_eq!(
            identity.github_output(),
            "source_sha=0123456789\nrelease_id=20260829-01234567-1-1\niso_basename=kyth-live-latest-20260829-01234567-1-1.iso\nchannel_basename=kyth-live-latest.iso\nimmutable_tag=iso-latest-20260829-01234567-1-1\nartifact_name=kyth-live-iso-latest-20260829-01234567-1-1\n"
        );
    }
}
