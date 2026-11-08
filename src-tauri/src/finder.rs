//! Handing a track to macOS: show it in Finder, open it in another app (spec: "Reveal in
//! Finder" and "Open in…" instead of editing tags in KHole).

use std::path::{Path, PathBuf};
use std::process::Command;

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSFileManager, NSString, NSURL};
use serde::Serialize;

use crate::error::{Error, Result};

/// An app that can open a file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    /// Finder's display name ("Music", "Audacity").
    pub name: String,
    /// The `.app` bundle.
    pub path: String,
    /// The app a double-click in Finder would use.
    pub is_default: bool,
}

fn existing(path: &Path) -> Result<()> {
    if path.exists() {
        Ok(())
    } else {
        Err(Error::NotFound(format!(
            "{} (moved or deleted meanwhile)",
            path.display()
        )))
    }
}

/// Opens a Finder window with the file selected.
pub fn reveal(path: &Path) -> Result<()> {
    existing(path)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| Error::Invalid(e.to_string()))
}

/// The apps macOS offers for the file (Finder's "Open With"), the default one first, then by
/// name. Duplicates (the same app in two places) are listed once.
pub fn apps_for(path: &Path) -> Result<Vec<App>> {
    existing(path)?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let workspace = NSWorkspace::sharedWorkspace();
    let default = workspace
        .URLForApplicationToOpenURL(&url)
        .and_then(|u| url_path(&u));
    let files = NSFileManager::defaultManager();

    let mut apps: Vec<App> = Vec::new();
    for app_url in workspace.URLsForApplicationsToOpenURL(&url).iter() {
        let Some(app_path) = url_path(&app_url) else {
            continue;
        };
        let name = files
            .displayNameAtPath(&NSString::from_str(&app_path))
            .to_string();
        let name = name.strip_suffix(".app").unwrap_or(&name).to_owned();
        let is_default = default.as_deref() == Some(app_path.as_str());
        if let Some(same) = apps.iter_mut().find(|a| a.name == name) {
            // The same app twice (e.g. /Applications and a backup volume): keep the default.
            if is_default {
                *same = App {
                    name,
                    path: app_path,
                    is_default,
                };
            }
            continue;
        }
        apps.push(App {
            name,
            path: app_path,
            is_default,
        });
    }
    apps.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(apps)
}

fn url_path(url: &NSURL) -> Option<String> {
    url.path().map(|p| p.to_string())
}

/// Opens the file in `app` (one of [`apps_for`]), or in its default app.
pub fn open_with(path: &Path, app: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("/usr/bin/open");
    if let Some(app) = app {
        // Only an app macOS itself offers for this file, nothing the UI made up.
        if !apps_for(path)?.iter().any(|a| a.path == app) {
            return Err(Error::Invalid(format!("{app} cannot open this file.")));
        }
        cmd.arg("-a").arg(app);
    } else {
        existing(path)?;
    }
    let status = cmd.arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "Could not open {}.",
            PathBuf::from(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_not_found() {
        let p = Path::new("/nonexistent/khole/a.mp3");
        assert!(matches!(reveal(p), Err(Error::NotFound(_))));
        assert!(matches!(apps_for(p), Err(Error::NotFound(_))));
        assert!(matches!(open_with(p, None), Err(Error::NotFound(_))));
    }

    /// LaunchServices knows apps for a WAV file (at least Music / QuickTime on any Mac).
    #[test]
    fn apps_for_a_wav_file() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("a.wav");
        crate::library::testutil::write_wav(&wav, 0.1, None);
        let apps = apps_for(&wav).unwrap();
        assert!(!apps.is_empty());
        assert!(apps.iter().all(|a| a.path.ends_with(".app")), "{apps:?}");
        assert!(apps.iter().filter(|a| a.is_default).count() <= 1);
        assert!(!apps.iter().any(|a| a.name.ends_with(".app")));
        // Refused: an app that is not offered for the file.
        assert!(matches!(
            open_with(&wav, Some("/System/Applications/Calculator.app")),
            Err(Error::Invalid(_))
        ));
    }
}
