//! Atomic, user-local `window.localStorage` persistence (`local_storage.json`).
//!
//! Enforces `O_NOFOLLOW | O_NONBLOCK`, regular-file checks, `0o600` file
//! permissions, exclusive `.local_storage.lock` (`flock`), atomic temp-file
//! rename with `sync_all`, and a strict `<= 512 KiB` total file size cap.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_LOCAL_STORAGE_FILE_BYTES: usize = 512 * 1024;
const MAX_ORIGINS: usize = 256;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

struct StorageLock(std::fs::File);

impl Drop for StorageLock {
    fn drop(&mut self) {
        // SAFETY: this guard owns a live, exclusively locked descriptor.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub struct LocalStorageStore {
    path: Option<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u8,
    origins: BTreeMap<String, Vec<(String, String)>>,
}

impl LocalStorageStore {
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
            path: root.map(|p| p.join("mgbrowser/local_storage.json")),
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
            .map_err(|e| format!("Cannot create localStorage directory: {e}"))
    }

    fn acquire_lock(parent: &Path) -> Result<StorageLock, String> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(parent.join(".local_storage.lock"))
            .map_err(|e| format!("Cannot lock localStorage: {e}"))?;
        if !lock.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("localStorage lock must be a regular file".into());
        }
        // SAFETY: the live file owns this descriptor.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("localStorage is busy in another window".into());
        }
        Ok(StorageLock(lock))
    }

    pub fn load(&self) -> Result<BTreeMap<String, Vec<(String, String)>>, String> {
        let path = self
            .path
            .as_ref()
            .ok_or("No absolute HOME or XDG_CONFIG_HOME; localStorage cannot be loaded")?;
        if let Ok(meta) = fs::symlink_metadata(path)
            && !meta.is_file()
        {
            return Err("localStorage must be a regular file".into());
        }
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(format!("Cannot read localStorage: {e}")),
        };
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("localStorage must be a regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_LOCAL_STORAGE_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_LOCAL_STORAGE_FILE_BYTES {
            return Err("localStorage exceeds 512 KiB; existing file preserved".into());
        }
        let stored: Stored = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid localStorage JSON; existing file preserved")?;
        if stored.version != 1 || stored.origins.len() > MAX_ORIGINS {
            return Err("Unsupported localStorage version or too many origins".into());
        }
        Ok(stored.origins)
    }

    pub fn save_snapshot(
        &self,
        snapshot: &BTreeMap<String, Vec<(String, String)>>,
    ) -> Result<(), String> {
        let path = self
            .path
            .as_ref()
            .ok_or("No absolute HOME or XDG_CONFIG_HOME; localStorage cannot be saved")?;
        let parent = path.parent().ok_or("Invalid localStorage path")?;
        Self::ensure_private_parent(parent)?;
        if let Ok(meta) = fs::symlink_metadata(path)
            && !meta.is_file()
        {
            return Err("Cannot replace non-regular localStorage file".into());
        }
        let _lock = Self::acquire_lock(parent)?;
        let mut origins: BTreeMap<String, Vec<(String, String)>> = snapshot
            .iter()
            .filter(|(_, items)| !items.is_empty())
            .take(MAX_ORIGINS)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut encoded = Vec::new();
        loop {
            encoded.clear();
            let stored = Stored {
                version: 1,
                origins: origins.clone(),
            };
            serde_json::to_writer(&mut encoded, &stored)
                .map_err(|e| format!("Cannot serialize localStorage: {e}"))?;
            encoded.push(b'\n');
            if encoded.len() <= MAX_LOCAL_STORAGE_FILE_BYTES {
                break;
            }
            if origins.pop_last().is_none() {
                return Err("localStorage snapshot exceeds 512 KiB".into());
            }
        }
        let temporary = parent.join(format!(
            ".local-storage-{}-{}.tmp",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&temporary)
            .map_err(|e| format!("Cannot save localStorage: {e}"))?;
        let result = (|| -> Result<(), std::io::Error> {
            file.write_all(&encoded)?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Cannot save localStorage: {error}"));
        }
        Ok(())
    }

    pub fn clear_file(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Ok(meta) = fs::symlink_metadata(path) {
            if !meta.is_file() {
                return Err("Refusing to remove non-regular localStorage entry".into());
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
                Err(e) => return Err(format!("Cannot clear localStorage: {e}")),
            }
        }
        Ok(())
    }
}

impl mg_chassis::net::LocalStoragePersistence for LocalStorageStore {
    fn save(&self, snapshot: &BTreeMap<String, Vec<(String, String)>>) {
        if let Err(error) = self.save_snapshot(snapshot) {
            eprintln!("LOCAL_STORAGE: {error}");
        }
    }

    fn clear(&self) {
        if let Err(error) = self.clear_file() {
            eprintln!("LOCAL_STORAGE: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch_store() -> (PathBuf, LocalStorageStore) {
        let dir = std::env::temp_dir().join(format!(
            "mg-local-storage-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let path = dir.join("mgbrowser/local_storage.json");
        (dir, LocalStorageStore::at_path(path))
    }

    #[test]
    fn persists_with_0600_enforces_512kib_cap_clears_and_rejects_symlinks() {
        let (dir, store) = scratch_store();
        assert!(store.load().unwrap().is_empty());

        let path = store.path.clone().unwrap();
        let session = mg_chassis::net::Session::new();
        let arc_store = std::sync::Arc::new(store);
        session.set_storage_persistence(Some(arc_store.clone()));
        session.apply_local_storage_ops(
            "https://example.com/page",
            &[mg_sparkle::page_session::platform::StorageOp::Set {
                key: "greeting".into(),
                value: "hello".into(),
            }],
        );

        let meta = fs::symlink_metadata(&path).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);

        let reloaded = LocalStorageStore::at_path(path.clone()).load().unwrap();
        assert_eq!(
            reloaded.get("https://example.com"),
            Some(&vec![("greeting".to_string(), "hello".to_string())])
        );

        // Enforce <= 512 KiB file cap even when snapshot contains large multi-origin data.
        let mut big = BTreeMap::new();
        let val = "x".repeat(50 * 1024);
        for i in 0..14 {
            big.insert(
                format!("https://origin-{i}.example"),
                vec![("blob".to_string(), val.clone())],
            );
        }
        arc_store.save_snapshot(&big).unwrap();
        let saved_meta = fs::symlink_metadata(&path).unwrap();
        assert!(saved_meta.len() as usize <= MAX_LOCAL_STORAGE_FILE_BYTES);

        // Clear removes the file cleanly.
        session.clear_local_storage();
        assert!(!path.exists());
        assert!(arc_store.load().unwrap().is_empty());

        // Symlink must be rejected on both load and save.
        std::os::unix::fs::symlink("missing", &path).unwrap();
        assert!(arc_store.load().is_err());
        assert!(arc_store.save_snapshot(&reloaded).is_err());

        fs::remove_dir_all(&dir).unwrap();
    }
}
