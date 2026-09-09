//! Read-only discovery of third-party downloaded assets.

use std::path::{Path, PathBuf};

pub fn find_latest_davinci_zip(
    download_dir: impl AsRef<Path>,
    home_downloads: impl AsRef<Path>,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for root in [download_dir.as_ref(), home_downloads.as_ref()] {
        let Ok(entries) = root.read_dir() else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains("DaVinci") && name.ends_with(".zip"))
            {
                if let Ok(modified) = path.metadata().and_then(|metadata| metadata.modified()) {
                    candidates.push((modified, path));
                }
            }
        }
    }
    candidates
        .into_iter()
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DaVinciPackage {
    pub zip_path: String,
    pub app_id: String,
    pub manifest: String,
    pub is_studio: bool,
}

pub fn prepare_davinci_resolve(path: impl AsRef<Path>) -> Result<DaVinciPackage, String> {
    let path = path.as_ref();
    if !path.is_file() {
        return Err(format!("ZIP file does not exist: {}", path.display()));
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_studio = name.contains("studio");
    let (app_id, manifest) = if is_studio {
        (
            "com.blackmagic.ResolveStudio",
            "com.blackmagic.ResolveStudio.yaml",
        )
    } else {
        ("com.blackmagic.Resolve", "com.blackmagic.Resolve.yaml")
    };
    Ok(DaVinciPackage {
        zip_path: path.display().to_string(),
        app_id: app_id.into(),
        manifest: manifest.into(),
        is_studio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn identifies_studio_package_without_installing_it() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("DaVinci_Resolve_Studio.zip");
        fs::write(&path, "placeholder").unwrap();
        let package = prepare_davinci_resolve(&path).unwrap();
        assert!(package.is_studio);
        assert_eq!(package.app_id, "com.blackmagic.ResolveStudio");
    }
}
