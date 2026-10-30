//! Atomic, user-local browsing history store (`history.json`).
//!
//! Mirrors `src/bookmarks.rs` security invariants: `O_NOFOLLOW`, regular-file
//! checks, `0o600` permissions, exclusive `.history.lock` with explicit
//! `flock(LOCK_UN)` in `Drop`, and `2 MiB` bounded reads.

use mg_chassis::history::{HistoryChange, HistoryEntry, MAX_HISTORY, apply_in_memory};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

struct HistoryLock(std::fs::File);

impl Drop for HistoryLock {
    fn drop(&mut self) {
        // SAFETY: this guard owns a live, exclusively locked descriptor.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub struct HistoryStore {
    path: Option<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u8,
    entries: Vec<Entry>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    url: String,
    title: String,
    timestamp_secs: u64,
    visits: u32,
}

impl HistoryStore {
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
            path: root.map(|p| p.join("mgbrowser/history.json")),
        }
    }

    pub fn load(&self) -> Result<Vec<HistoryEntry>, String> {
        let path = self
            .path
            .as_ref()
            .ok_or("No absolute HOME or XDG_CONFIG_HOME; history cannot be saved")?;
        if let Ok(meta) = fs::symlink_metadata(path)
            && !meta.is_file()
        {
            return Err("History must be a regular file".into());
        }
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(format!("Cannot read history: {e}")),
        };
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("History must be a regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("History exceeds 2 MiB; existing file preserved".into());
        }
        let stored: Stored = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid history JSON; existing file preserved")?;
        if stored.version != 1 || stored.entries.len() > MAX_HISTORY {
            return Err("Unsupported history version or too many entries".into());
        }
        let mut result = Vec::with_capacity(stored.entries.len());
        for entry in stored.entries {
            let item =
                HistoryEntry::new(&entry.url, &entry.title, entry.timestamp_secs, entry.visits)?;
            if result.iter().any(|h: &HistoryEntry| h.url == item.url) {
                continue;
            }
            result.push(item);
        }
        Ok(result)
    }

    pub fn apply_batch(&self, changes: &[HistoryChange]) -> Result<Vec<HistoryEntry>, String> {
        if changes.is_empty() {
            return self.load();
        }
        let path = self
            .path
            .as_ref()
            .ok_or("No absolute HOME or XDG_CONFIG_HOME; history cannot be saved")?;
        let parent = path.parent().ok_or("Invalid history path")?;
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create history directory: {e}"))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(parent.join(".history.lock"))
            .map_err(|e| format!("Cannot lock history: {e}"))?;
        if !lock.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("History lock must be a regular file".into());
        }
        // SAFETY: the live file owns this descriptor.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("History is busy in another window; try again".into());
        }
        let _lock = HistoryLock(lock);
        let mut entries = self.load()?;
        for change in changes {
            apply_in_memory(&mut entries, change);
        }
        let stored = Stored {
            version: 1,
            entries: entries
                .iter()
                .map(|h| Entry {
                    url: h.url.clone(),
                    title: h.title.clone(),
                    timestamp_secs: h.timestamp_secs,
                    visits: h.visits,
                })
                .collect(),
        };
        let temporary = parent.join(format!(
            ".history-{}-{}.tmp",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|e| format!("Cannot save history: {e}"))?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            serde_json::to_writer(&mut file, &stored)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Cannot save history: {error}"));
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_store() -> HistoryStore {
        let dir = std::env::temp_dir().join(format!(
            "mg-history-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        HistoryStore {
            path: Some(dir.join("history.json")),
        }
    }

    #[test]
    fn persists_deduplicates_clears_and_rejects_symlinks() {
        let store = scratch_store();
        assert!(store.load().unwrap().is_empty());
        let entries = store
            .apply_batch(&[
                HistoryChange::Record {
                    url: "https://example.com/1".into(),
                    title: "One".into(),
                    timestamp_secs: 100,
                },
                HistoryChange::Record {
                    url: "https://example.com/2".into(),
                    title: "Two".into(),
                    timestamp_secs: 110,
                },
                HistoryChange::Record {
                    url: "https://example.com/1".into(),
                    title: "One Revisited".into(),
                    timestamp_secs: 120,
                },
            ])
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://example.com/1");
        assert_eq!(entries[0].visits, 2);
        assert_eq!(store.load().unwrap(), entries);

        let cleared = store.apply_batch(&[HistoryChange::ClearAll]).unwrap();
        assert!(cleared.is_empty());
        assert!(store.load().unwrap().is_empty());

        let path = store.path.as_ref().unwrap();
        fs::remove_file(path).unwrap();
        std::os::unix::fs::symlink("missing", path).unwrap();
        assert!(store.load().is_err());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
