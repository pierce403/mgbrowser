//! Atomic, user-local open-tab session store (`session.json`).
//!
//! Persists up to 16 validated HTTP(S) tab URLs and titles with `0o600`
//! permissions, `O_NOFOLLOW | O_NONBLOCK`, regular-file checks, exclusive
//! `.session.lock`, and atomic temp-file rename.

use mg_chassis::SessionTab;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_SESSION_TABS: usize = 16;
const MAX_SESSION_BYTES: u64 = 256 * 1024;
const MAX_URL_BYTES: usize = 8191;
const MAX_TITLE_BYTES: usize = 512;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

struct SessionLock(std::fs::File);

impl Drop for SessionLock {
    fn drop(&mut self) {
        // SAFETY: this guard owns a live, exclusively locked descriptor.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub struct SessionStore {
    path: Option<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u8,
    tabs: Vec<TabEntry>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TabEntry {
    url: String,
    title: String,
}

pub fn sanitize_session_tab(url: &str, title: &str) -> Option<SessionTab> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_URL_BYTES || trimmed.chars().any(char::is_control)
    {
        return None;
    }
    let canonical_url = mg_chassis::bookmarks::Bookmark::new(trimmed, "").ok()?.url;
    if canonical_url.len() > MAX_URL_BYTES {
        return None;
    }
    let mut clean_title = String::new();
    for ch in title.chars() {
        if ch.is_control() {
            if matches!(ch, '\n' | '\r' | '\t') && !clean_title.ends_with(' ') {
                clean_title.push(' ');
            }
            continue;
        }
        if clean_title.len() + ch.len_utf8() > MAX_TITLE_BYTES {
            break;
        }
        clean_title.push(ch);
    }
    Some(SessionTab {
        url: canonical_url,
        title: clean_title.trim().to_owned(),
    })
}

impl SessionStore {
    pub fn user() -> Self {
        let root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join(".config"))
            });
        Self {
            path: root.map(|p| p.join("mgbrowser/session.json")),
        }
    }

    pub fn at_path(path: PathBuf) -> Self {
        Self {
            path: path.is_absolute().then_some(path),
        }
    }

    fn ensure_private_parent(parent: &Path) -> Result<(), String> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder
            .create(parent)
            .map_err(|e| format!("Cannot create session directory: {e}"))
    }

    fn acquire_lock(parent: &Path) -> Result<SessionLock, String> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(parent.join(".session.lock"))
            .map_err(|e| format!("Cannot lock session: {e}"))?;
        if !lock.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Session lock must be a regular file".into());
        }
        // SAFETY: the live file owns this descriptor.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("Session store is busy in another window".into());
        }
        Ok(SessionLock(lock))
    }

    pub fn load(&self) -> Result<Vec<SessionTab>, String> {
        let path = self
            .path
            .as_ref()
            .ok_or("No absolute HOME or XDG_CONFIG_HOME; session cannot be loaded")?;
        if let Ok(meta) = fs::symlink_metadata(path)
            && !meta.is_file()
        {
            return Err("Session must be a regular file".into());
        }
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("Cannot read session: {e}")),
        };
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Session must be a regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_SESSION_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_SESSION_BYTES {
            return Err("Session exceeds 256 KiB; existing file preserved".into());
        }
        let stored: Stored = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid session JSON; existing file preserved")?;
        if stored.version != 1 || stored.tabs.len() > MAX_SESSION_TABS {
            return Err("Unsupported session version or too many tabs".into());
        }
        Ok(stored
            .tabs
            .into_iter()
            .filter_map(|t| sanitize_session_tab(&t.url, &t.title))
            .take(MAX_SESSION_TABS)
            .collect())
    }

    pub fn save(&self, tabs: &[SessionTab]) -> Result<Vec<SessionTab>, String> {
        let path = self
            .path
            .as_ref()
            .ok_or("No absolute HOME or XDG_CONFIG_HOME; session cannot be saved")?;
        let parent = path.parent().ok_or("Invalid session path")?;
        Self::ensure_private_parent(parent)?;
        if let Ok(meta) = fs::symlink_metadata(path)
            && !meta.is_file()
        {
            return Err("Cannot replace non-regular session file".into());
        }
        let _lock = Self::acquire_lock(parent)?;
        let sanitized: Vec<SessionTab> = tabs
            .iter()
            .filter_map(|t| sanitize_session_tab(&t.url, &t.title))
            .take(MAX_SESSION_TABS)
            .collect();
        let stored = Stored {
            version: 1,
            tabs: sanitized
                .iter()
                .map(|t| TabEntry {
                    url: t.url.clone(),
                    title: t.title.clone(),
                })
                .collect(),
        };
        let temporary = parent.join(format!(
            ".session-{}-{}.tmp",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&temporary)
            .map_err(|e| format!("Cannot save session: {e}"))?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            serde_json::to_writer(&mut file, &stored)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Cannot save session: {error}"));
        }
        Ok(sanitized)
    }

    pub fn clear(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Ok(meta) = fs::symlink_metadata(path) {
            if !meta.is_file() {
                return Err("Refusing to remove non-regular session entry".into());
            }
        } else {
            return Ok(());
        }
        if let Some(parent) = path.parent()
            && parent.exists()
        {
            let _lock = Self::acquire_lock(parent)?;
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Cannot clear session: {e}")),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch_store() -> (PathBuf, SessionStore) {
        let dir = std::env::temp_dir().join(format!(
            "mg-session-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let path = dir.join("mgbrowser/session.json");
        (dir, SessionStore::at_path(path))
    }

    #[test]
    fn persists_up_to_16_https_tabs_filters_unsafe_urls_clears_and_rejects_symlinks() {
        let (dir, store) = scratch_store();
        assert!(store.load().unwrap().is_empty());

        let mut raw = vec![
            SessionTab {
                url: "chrome://newtab".into(),
                title: "New Tab".into(),
            },
            SessionTab {
                url: "https://user:secret@example.com/private".into(),
                title: "Secret".into(),
            },
        ];
        for i in 0..20 {
            raw.push(SessionTab {
                url: format!("https://example.com/tab-{i}"),
                title: format!("Tab {i}"),
            });
        }
        let saved = store.save(&raw).unwrap();
        assert_eq!(saved.len(), MAX_SESSION_TABS);
        assert_eq!(saved[0].url, "https://example.com/tab-0");
        assert_eq!(saved[15].url, "https://example.com/tab-15");

        let path = store.path.as_ref().unwrap();
        let meta = fs::symlink_metadata(path).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        assert_eq!(store.load().unwrap(), saved);

        store.clear().unwrap();
        assert!(!path.exists());
        assert!(store.load().unwrap().is_empty());

        std::os::unix::fs::symlink("missing", path).unwrap();
        assert!(store.load().is_err());
        assert!(store.save(&saved).is_err());

        fs::remove_dir_all(&dir).unwrap();
    }
}
