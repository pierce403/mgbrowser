//! User-level appearance preferences. Desktop discovery stays in the platform host.
use mg_chassis::{ScalePreference, ThemePreference};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_SETTINGS: u64 = 4096;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// Preferences are saved together so changing scale never resets the theme.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Preferences {
    pub theme: ThemePreference,
    pub scale: ScalePreference,
}

pub struct Settings {
    path: Option<PathBuf>,
}

impl Settings {
    pub fn user() -> Self {
        Self::from_roots(
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
        )
    }

    fn from_roots(config: Option<PathBuf>, home: Option<PathBuf>) -> Self {
        let root = config
            .filter(|p| p.is_absolute())
            .or_else(|| home.filter(|p| p.is_absolute()).map(|p| p.join(".config")));
        Self {
            path: root.map(|p| p.join("mgbrowser/settings.json")),
        }
    }

    pub fn load(&self) -> Result<Preferences, String> {
        let Some(path) = &self.path else {
            return Err("No absolute HOME or XDG_CONFIG_HOME; appearance cannot be saved".into());
        };
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Preferences::default());
            }
            Err(error) => return Err(format!("Cannot read appearance settings: {error}")),
        };
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Appearance settings must be a regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_SETTINGS + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Cannot read appearance settings: {e}"))?;
        if bytes.len() as u64 > MAX_SETTINGS {
            return Err("Appearance settings exceed 4 KiB; using System".into());
        }
        parse(&bytes)
    }

    pub fn save(&self, preferences: Preferences) -> Result<(), String> {
        if !preferences.scale.is_valid() {
            return Err(
                "Invalid appearance scale; expected System or a supported percentage".into(),
            );
        }
        let path = self.path.as_ref().ok_or(
            "No absolute HOME or XDG_CONFIG_HOME; appearance applies to this session only",
        )?;
        let parent = path.parent().ok_or("Invalid appearance settings path")?;
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create settings directory: {e}"))?;
        if let Ok(meta) = fs::symlink_metadata(path)
            && !meta.is_file()
        {
            return Err("Cannot replace non-regular appearance settings".into());
        }
        let temporary = parent.join(format!(
            ".settings-{}-{}.tmp",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|e| format!("Cannot save appearance settings: {e}"))?;
        let result = (|| {
            let name = match preferences.theme {
                ThemePreference::System => "system",
                ThemePreference::Light => "light",
                ThemePreference::Dark => "dark",
            };
            let scale = match preferences.scale {
                ScalePreference::System => "\"system\"".into(),
                ScalePreference::Percent(value) => value.to_string(),
            };
            writeln!(file, "{{\"theme\":\"{name}\",\"scale\":{scale}}}")?;
            file.sync_all()?;
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|e| format!("Cannot save appearance settings: {e}"))
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPreferences {
    theme: String,
    #[serde(default)]
    scale: StoredScale,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum StoredScale {
    Name(String),
    Percent(u16),
}

impl Default for StoredScale {
    fn default() -> Self {
        Self::Name("system".into())
    }
}

fn parse(bytes: &[u8]) -> Result<Preferences, String> {
    if bytes.len() as u64 > MAX_SETTINGS {
        return Err("Appearance settings exceed 4 KiB; using System".into());
    }
    let value: StoredPreferences = serde_json::from_slice(bytes)
        .map_err(|_| "Invalid appearance settings JSON; using System")?;
    let theme = match value.theme.as_str() {
        "system" => ThemePreference::System,
        "light" => ThemePreference::Light,
        "dark" => ThemePreference::Dark,
        _ => return Err("Invalid appearance theme; expected system, light or dark".into()),
    };
    let scale = match value.scale {
        StoredScale::Name(name) if name == "system" => ScalePreference::System,
        StoredScale::Percent(value) if ScalePreference::Percent(value).is_valid() => {
            ScalePreference::Percent(value)
        }
        _ => return Err(
            "Invalid appearance scale; expected system or 75, 100, 125, 150, 175, 200, 250, 300"
                .into(),
        ),
    };
    Ok(Preferences { theme, scale })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mg-theme-settings-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn xdg_path_and_missing_roots() {
        let settings = Settings::from_roots(Some("/config".into()), Some("/home/user".into()));
        assert_eq!(
            settings.path(),
            Some(Path::new("/config/mgbrowser/settings.json"))
        );
        let settings = Settings::from_roots(Some("relative".into()), Some("/home/user".into()));
        assert_eq!(
            settings.path(),
            Some(Path::new("/home/user/.config/mgbrowser/settings.json"))
        );
        let settings = Settings::from_roots(None, None);
        assert!(settings.load().is_err());
        assert!(settings.save(Preferences::default()).is_err());
    }

    #[test]
    fn default_roundtrip_restart_and_invalid_file() {
        let root = scratch();
        let settings = Settings::from_roots(Some(root.clone()), None);
        assert_eq!(settings.load().unwrap(), Preferences::default());
        for theme in [
            ThemePreference::Dark,
            ThemePreference::Light,
            ThemePreference::System,
        ] {
            for scale in [75, 100, 125, 150, 175, 200, 250, 300] {
                let preference = Preferences {
                    theme,
                    scale: ScalePreference::Percent(scale),
                };
                settings.save(preference).unwrap();
                let restarted = Settings::from_roots(Some(root.clone()), None);
                assert_eq!(restarted.load().unwrap(), preference);
            }
            let preference = Preferences {
                theme,
                scale: ScalePreference::System,
            };
            settings.save(preference).unwrap();
            assert_eq!(settings.load().unwrap(), preference);
        }
        fs::write(settings.path().unwrap(), b"invalid").unwrap();
        assert!(settings.load().is_err());
        fs::write(settings.path().unwrap(), vec![b' '; 4097]).unwrap();
        assert!(settings.load().unwrap_err().contains("4 KiB"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_preferences_are_not_silently_accepted() {
        for input in [
            "null",
            "[]",
            "{}",
            "{\"theme\":4}",
            "{\"theme\":\"unknown\"}",
            "{\"theme\":\"dark\",\"theme\":\"light\"}",
            "{\"theme\":\"dark\",\"unknown\":true}",
            "{\"theme\":\"dark\",\"scale\":null}",
            "{\"theme\":\"dark\",\"scale\":true}",
            "{\"theme\":\"dark\",\"scale\":\"200\"}",
            "{\"theme\":\"dark\",\"scale\":200.0}",
            "{\"theme\":\"dark\",\"scale\":-100}",
            "{\"theme\":\"dark\",\"scale\":124}",
            "{\"theme\":\"dark\",\"scale\":301}",
            "{\"theme\":\"dark\",\"scale\":99999999999999999999999}",
            "{\"theme\":\"dark\",\"scale\":100,\"scale\":200}",
        ] {
            assert!(parse(input.as_bytes()).is_err());
        }
    }

    #[test]
    fn old_theme_only_settings_migrate_without_resetting_theme() {
        assert_eq!(
            parse(br#"{"theme":"dark"}"#).unwrap(),
            Preferences {
                theme: ThemePreference::Dark,
                scale: ScalePreference::System,
            }
        );
        let root = scratch();
        let settings = Settings::from_roots(Some(root.clone()), None);
        fs::create_dir_all(root.join("mgbrowser")).unwrap();
        fs::write(settings.path().unwrap(), br#"{"theme":"dark"}"#).unwrap();
        let mut preferences = settings.load().unwrap();
        preferences.scale = ScalePreference::Percent(200);
        settings.save(preferences).unwrap();
        assert_eq!(settings.load().unwrap(), preferences);
        assert_eq!(preferences.theme, ThemePreference::Dark);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(settings.path().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let previous = fs::read(settings.path().unwrap()).unwrap();
        preferences.scale = ScalePreference::Percent(201);
        assert!(settings.save(preferences).is_err());
        assert_eq!(fs::read(settings.path().unwrap()).unwrap(), previous);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_failure_and_symlink_do_not_replace_existing_data() {
        let root = scratch();
        let obstruction = root.join("not-a-directory");
        fs::write(&obstruction, b"keep").unwrap();
        let settings = Settings::from_roots(Some(obstruction.clone()), None);
        assert!(settings.save(Preferences::default()).is_err());
        assert_eq!(fs::read(&obstruction).unwrap(), b"keep");
        let settings = Settings::from_roots(Some(root.clone()), None);
        fs::create_dir_all(root.join("mgbrowser")).unwrap();
        std::os::unix::fs::symlink(&obstruction, settings.path().unwrap()).unwrap();
        assert!(settings.load().is_err());
        assert!(settings.save(Preferences::default()).is_err());
        assert_eq!(fs::read(&obstruction).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }
}
