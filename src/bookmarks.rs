//! Atomic, user-local bookmarks. A lock merges edits from multiple browser windows.
use mg_chassis::bookmarks::{Bookmark, BookmarkChange, MAX_BOOKMARKS};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
pub struct BookmarkStore {
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
}

impl BookmarkStore {
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
            path: root.map(|p| p.join("mgbrowser/bookmarks.json")),
        }
    }
    pub fn load(&self) -> Result<Vec<Bookmark>, String> {
        let path = self
            .path
            .as_ref()
            .ok_or("No absolute HOME or XDG_CONFIG_HOME; bookmarks cannot be saved")?;
        if let Ok(meta) = fs::symlink_metadata(path)
            && !meta.is_file()
        {
            return Err("Bookmarks must be a regular file".into());
        }
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(format!("Cannot read bookmarks: {e}")),
        };
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Bookmarks must be a regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Bookmarks exceed 2 MiB; existing file preserved".into());
        }
        let stored: Stored = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid bookmarks JSON; existing file preserved")?;
        if stored.version != 1 || stored.entries.len() > MAX_BOOKMARKS {
            return Err("Unsupported bookmarks version or too many entries".into());
        }
        let mut result = Vec::<Bookmark>::new();
        for entry in stored.entries {
            let bookmark = Bookmark::new(&entry.url, &entry.title)?;
            if result.iter().any(|b| b.url == bookmark.url) {
                return Err("Duplicate bookmark URL; existing file preserved".into());
            }
            result.push(bookmark);
        }
        Ok(result)
    }
    pub fn apply(&self, change: &BookmarkChange) -> Result<Vec<Bookmark>, String> {
        let path = self
            .path
            .as_ref()
            .ok_or("No absolute HOME or XDG_CONFIG_HOME; bookmarks cannot be saved")?;
        let parent = path.parent().ok_or("Invalid bookmark path")?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create bookmarks directory: {e}"))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(parent.join(".bookmarks.lock"))
            .map_err(|e| format!("Cannot lock bookmarks: {e}"))?;
        if !lock.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Bookmark lock must be a regular file".into());
        }
        // SAFETY: the live file owns this descriptor. Closing it releases the lock.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("Bookmarks are busy in another window; try again".into());
        }
        let mut entries = self.load()?;
        match change {
            BookmarkChange::Add(bookmark) => {
                let bookmark = Bookmark::new(&bookmark.url, &bookmark.title)?;
                if !entries.iter().any(|b| b.url == bookmark.url) {
                    if entries.len() >= MAX_BOOKMARKS {
                        return Err("Bookmark limit reached (128); remove an entry first".into());
                    }
                    entries.push(bookmark);
                }
            }
            BookmarkChange::Remove(url) => entries.retain(|b| &b.url != url),
        }
        let stored = Stored {
            version: 1,
            entries: entries
                .iter()
                .map(|b| Entry {
                    url: b.url.clone(),
                    title: b.title.clone(),
                })
                .collect(),
        };
        let temporary = parent.join(format!(
            ".bookmarks-{}-{}.tmp",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|e| format!("Cannot save bookmarks: {e}"))?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            serde_json::to_writer(&mut file, &stored)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Cannot save bookmarks: {error}"));
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> BookmarkStore {
        let dir = std::env::temp_dir().join(format!(
            "mg-bookmarks-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        BookmarkStore {
            path: Some(dir.join("bookmarks.json")),
        }
    }
    fn add(url: &str) -> BookmarkChange {
        BookmarkChange::Add(Bookmark::new(url, "Title").unwrap())
    }
    #[test]
    fn saves_reloads_deduplicates_merges_and_removes() {
        let store = store();
        assert!(store.load().unwrap().is_empty());
        store.apply(&add("https://example.com/")).unwrap();
        let second = BookmarkStore {
            path: store.path.clone(),
        };
        assert_eq!(second.apply(&add("https://example.com/")).unwrap().len(), 1);
        assert_eq!(second.apply(&add("https://example.org/")).unwrap().len(), 2);
        assert_eq!(
            store
                .apply(&BookmarkChange::Remove("https://example.com/".into()))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(store.load().unwrap()[0].url, "https://example.org/");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(store.path.as_ref().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(store.path.unwrap().parent().unwrap()).unwrap();
    }
    #[test]
    fn malformed_and_symlink_files_are_preserved() {
        let store = store();
        let path = store.path.as_ref().unwrap();
        fs::write(path, b"not JSON").unwrap();
        assert!(store.apply(&add("https://example.com/")).is_err());
        assert_eq!(fs::read(path).unwrap(), b"not JSON");
        fs::remove_file(path).unwrap();
        std::os::unix::fs::symlink("missing", path).unwrap();
        assert!(store.apply(&add("https://example.com/")).is_err());
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn lock_contention_does_not_overwrite() {
        let store = store();
        let path = store.path.as_ref().unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.parent().unwrap().join(".bookmarks.lock"))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert!(
            store
                .apply(&add("https://example.com/"))
                .unwrap_err()
                .contains("busy")
        );
        assert!(!path.exists());
        drop(lock);
        store.apply(&add("https://example.com/")).unwrap();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn limits_and_unknown_versions_preserve_existing_data() {
        let store = store();
        let path = store.path.as_ref().unwrap();
        let entries: Vec<_> = (0..MAX_BOOKMARKS)
            .map(|i| Entry {
                url: format!("https://example.com/{i}"),
                title: "Title".into(),
            })
            .collect();
        let bytes = serde_json::to_vec(&Stored {
            version: 1,
            entries,
        })
        .unwrap();
        fs::write(path, &bytes).unwrap();
        assert!(
            store
                .apply(&add("https://example.org/"))
                .unwrap_err()
                .contains("limit")
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
        for bytes in [
            b"{\"version\":2,\"entries\":[]}".to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            fs::write(path, &bytes).unwrap();
            assert!(store.apply(&add("https://example.com/")).is_err());
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
