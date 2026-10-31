//! Safe user-local file writer for browser downloads (`~/Downloads` / `XDG_DOWNLOAD_DIR`).
//!
//! Uses `create_new(true)` + `O_NOFOLLOW` + `0o600` permissions + `(1)`, `(2)`
//! collision numbering so downloads never overwrite existing files or follow symlinks.

use mg_chassis::downloads::{MAX_DOWNLOAD_BYTES, PendingDownload, sanitize_download_filename};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

pub struct DownloadStore {
    dir: Option<PathBuf>,
}

impl DownloadStore {
    pub fn user() -> Self {
        let dir = std::env::var_os("MGBROWSER_DOWNLOAD_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("XDG_DOWNLOAD_DIR")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
            })
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join("Downloads"))
            });
        Self { dir }
    }

    pub fn from_dir(dir: Option<PathBuf>) -> Self {
        Self {
            dir: dir.filter(|p| p.is_absolute()),
        }
    }

    pub fn save(&self, pending: &PendingDownload) -> Result<PathBuf, String> {
        if pending.body.len() > MAX_DOWNLOAD_BYTES {
            return Err("Download exceeds 8 MiB limit".into());
        }
        let dir = self
            .dir
            .as_ref()
            .ok_or("No absolute Downloads directory configured (set HOME or XDG_DOWNLOAD_DIR)")?;
        fs::create_dir_all(dir)
            .map_err(|e| format!("Cannot create Downloads directory {}: {e}", dir.display()))?;
        let meta = fs::symlink_metadata(dir)
            .map_err(|e| format!("Cannot inspect Downloads directory: {e}"))?;
        if !meta.is_dir() {
            return Err("Downloads target must be a real directory, not a symlink".into());
        }
        let safe_name = sanitize_download_filename(
            Some(&pending.suggested_filename),
            &pending.url,
            &pending.mime,
        );
        let (stem, ext) = split_stem_ext(&safe_name);
        for attempt in 0..1000u32 {
            let candidate_name = if attempt == 0 {
                safe_name.clone()
            } else if ext.is_empty() {
                format!("{stem} ({attempt})")
            } else {
                format!("{stem} ({attempt}).{ext}")
            };
            let target = dir.join(&candidate_name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&target)
            {
                Ok(mut file) => {
                    let write_res = (|| -> std::io::Result<()> {
                        file.write_all(&pending.body)?;
                        file.sync_all()
                    })();
                    if let Err(err) = write_res {
                        let _ = fs::remove_file(&target);
                        return Err(format!("Failed writing download: {err}"));
                    }
                    return Ok(target);
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    return Err(format!(
                        "Cannot create download file {}: {err}",
                        target.display()
                    ));
                }
            }
        }
        Err("Too many colliding download filenames in Downloads directory".into())
    }

    pub fn directory(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn save_exact_named(dir: &Path, exact_name: &str, body: &[u8]) -> Result<PathBuf, String> {
        if body.len() > MAX_DOWNLOAD_BYTES {
            return Err("Download exceeds 8 MiB limit".into());
        }
        if !dir.is_absolute() {
            return Err("Downloads target must be an absolute directory".into());
        }
        if exact_name.is_empty()
            || exact_name == "."
            || exact_name == ".."
            || exact_name.contains('/')
            || exact_name.contains('\\')
            || exact_name.chars().any(char::is_control)
        {
            return Err("Invalid download target filename".into());
        }
        fs::create_dir_all(dir)
            .map_err(|e| format!("Cannot create Downloads directory {}: {e}", dir.display()))?;
        let meta = fs::symlink_metadata(dir)
            .map_err(|e| format!("Cannot inspect Downloads directory: {e}"))?;
        if !meta.is_dir() {
            return Err("Downloads target must be a real directory, not a symlink".into());
        }
        let target = dir.join(exact_name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&target)
            .map_err(|err| format!("Cannot create download file {}: {err}", target.display()))?;
        let write_res = (|| -> std::io::Result<()> {
            file.write_all(body)?;
            file.sync_all()
        })();
        if let Err(err) = write_res {
            let _ = fs::remove_file(&target);
            return Err(format!("Failed writing download: {err}"));
        }
        Ok(target)
    }
}

fn split_stem_ext(name: &str) -> (&str, &str) {
    if let Some(stem) = name.strip_suffix(".tar.gz") {
        return (stem, "tar.gz");
    }
    match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s, e),
        _ => (name, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn saves_without_overwriting_and_numbers_collisions() {
        let dir = std::env::temp_dir().join(format!(
            "mg-downloads-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let store = DownloadStore::from_dir(Some(dir.clone()));
        let dl = PendingDownload {
            id: 1,
            url: "https://example.com/report.pdf".into(),
            suggested_filename: "../../report.pdf".into(),
            mime: "application/pdf".into(),
            body: b"%PDF-1.7 test".to_vec(),
        };
        let p1 = store.save(&dl).unwrap();
        let p2 = store.save(&dl).unwrap();
        assert_eq!(p1.file_name().unwrap(), "report.pdf");
        assert_eq!(p2.file_name().unwrap(), "report (1).pdf");
        assert_eq!(fs::read(&p1).unwrap(), b"%PDF-1.7 test");
        assert_eq!(fs::read(&p2).unwrap(), b"%PDF-1.7 test");
        fs::remove_dir_all(&dir).unwrap();
    }
}
