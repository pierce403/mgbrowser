//! User-level Linux updates. No shell, sudo, page-provided URL or native codec.
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const API: &str = "https://api.github.com/repos/pierce403/mgbrowser/releases/latest";
const BASE: &str = "https://github.com/pierce403/mgbrowser/releases/download";
const ASSET: &str = "mgbrowser-linux-x86_64.tar.gz";
const PAYLOAD: &str = "mgbrowser-linux-x86_64/mgbrowser";
const MAX_UNPACKED: u64 = 64 * 1024 * 1024;
pub const MARKER: &str = ".mgbrowser-auto-update";

fn version(value: &str) -> Result<[u64; 3], String> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|p| {
            p.is_empty()
                || (p.len() > 1 && p.starts_with('0'))
                || !p.bytes().all(|c| c.is_ascii_digit())
        })
    {
        return Err("Expected a stable major.minor.patch release".into());
    }
    Ok([
        parts[0].parse().map_err(|_| "Version overflow")?,
        parts[1].parse().map_err(|_| "Version overflow")?,
        parts[2].parse().map_err(|_| "Version overflow")?,
    ])
}

fn release_tag(bytes: &[u8], current: &str) -> Result<Option<String>, String> {
    let data: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if data["draft"] != false || data["prerelease"] != false {
        return Err("Latest release is not a published stable-channel build".into());
    }
    let tag = data["tag_name"].as_str().ok_or("Missing release tag")?;
    let latest = version(tag.strip_prefix('v').ok_or("Invalid release tag")?)?;
    Ok((latest > version(current)?).then(|| tag.to_owned()))
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let response = mg_chassis::net::fetch_release(url)?;
    if response.status != 200 {
        return Err(format!("Release server returned HTTP {}", response.status));
    }
    Ok(response.body)
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verify(archive: &[u8], checksum: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(checksum).map_err(|_| "Invalid checksum text")?;
    let fields: Vec<_> = text.split_whitespace().collect();
    if fields.len() != 2
        || fields[1] != ASSET
        || fields[0].len() != 64
        || !fields[0].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("Invalid release checksum file".into());
    }
    if !digest(archive).eq_ignore_ascii_case(fields[0]) {
        return Err("Release SHA-256 mismatch; existing installation unchanged".into());
    }
    Ok(())
}

fn octal(bytes: &[u8]) -> Result<usize, String> {
    usize::from_str_radix(
        std::str::from_utf8(bytes)
            .map_err(|_| "Invalid tar number")?
            .trim_matches(['\0', ' ']),
        8,
    )
    .map_err(|_| "Invalid tar number".into())
}

// Read only the exact executable member. Never unpack archive-selected paths,
// links, permissions or devices. GNU/ustar regular files and directories suffice
// for our package-release.sh payload; reject other archive extensions.
fn executable(archive: &[u8]) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    GzDecoder::new(archive)
        .take(MAX_UNPACKED + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > MAX_UNPACKED {
        return Err("Release expands beyond 64 MiB".into());
    }
    let mut offset = 0usize;
    let mut found = None;
    while offset + 512 <= data.len() {
        let header = &data[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            if data[offset..].iter().any(|b| *b != 0) {
                return Err("Trailing archive data".into());
            }
            return found.ok_or_else(|| "Release has no executable".into());
        }
        let sum: usize = header
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    *b as usize
                }
            })
            .sum();
        if octal(&header[148..156])? != sum {
            return Err("Invalid tar header checksum".into());
        }
        if !matches!(header[156], 0 | b'0' | b'5') {
            return Err("Unsupported archive member".into());
        }
        let name = std::str::from_utf8(&header[..100])
            .map_err(|_| "Invalid member name")?
            .trim_end_matches('\0');
        let size = octal(&header[124..136])?;
        let start = offset + 512;
        let end = start
            .checked_add(size)
            .filter(|end| *end <= data.len())
            .ok_or("Truncated release archive")?;
        if name == PAYLOAD && header[345..500].iter().all(|b| *b == 0) {
            if found.is_some() || header[156] == b'5' || size == 0 {
                return Err("Invalid or duplicate executable".into());
            }
            found = Some(data[start..end].to_vec());
        }
        offset = start
            .checked_add(size.div_ceil(512) * 512)
            .ok_or("Archive overflow")?;
    }
    Err("Missing archive end marker".into())
}

struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn lock(target: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(target.with_file_name(".mgbrowser-update.lock"))
        .map_err(|e| e.to_string())?;
    // SAFETY: flock operates only on this owned descriptor; dropping it releases
    // the lock even on process death. Never delete the lock inode while in use.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Another browser is updating; try again later".into());
    }
    Ok(file)
}

fn binary_output(path: &Path, argument: &str) -> Result<String, String> {
    let output_path = path.with_extension("check");
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&output_path)
        .map_err(|e| e.to_string())?;
    let cleanup = Pending(output_path.clone());
    let mut child = Command::new(path)
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(25);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(30)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Updated binary validation timed out or failed: {result:?}"
                ));
            }
        }
    };
    if !status.success() {
        return Err(format!(
            "Updated binary failed {argument}; installation unchanged"
        ));
    }
    let mut value = String::new();
    File::open(&output_path)
        .map_err(|e| e.to_string())?
        .take(128)
        .read_to_string(&mut value)
        .map_err(|e| e.to_string())?;
    drop(cleanup);
    Ok(value.trim().to_owned())
}

fn validate_binary(path: &Path, expected: &str) -> Result<(), String> {
    if binary_output(path, "--version")? != format!("mgbrowser {expected}") {
        return Err("Updated executable version does not match release tag".into());
    }
    binary_output(path, "--script-worker-selftest")?;
    Ok(())
}

fn install(target: &Path, archive: &[u8], checksum: &[u8], expected: &str) -> Result<(), String> {
    let before = fs::symlink_metadata(target).map_err(|e| e.to_string())?;
    // SAFETY: geteuid has no arguments or memory accesses.
    if !before.is_file() || before.uid() != unsafe { libc::geteuid() } {
        return Err(
            "Self-update requires a user-owned regular executable; use the installer without sudo"
                .into(),
        );
    }
    verify(archive, checksum)?;
    let bytes = executable(archive)?;
    let pending_path = target.with_file_name(format!(".mgbrowser-update-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&pending_path)
        .map_err(|e| e.to_string())?;
    let pending = Pending(pending_path);
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    validate_binary(&pending.0, expected)?;
    let now = fs::symlink_metadata(target).map_err(|e| e.to_string())?;
    if (now.dev(), now.ino()) != (before.dev(), before.ino()) {
        return Err("Installation changed during update; try again".into());
    }
    fs::set_permissions(&pending.0, fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    fs::rename(&pending.0, target).map_err(|e| format!("Cannot replace executable: {e}"))?;
    Ok(())
}

/// True only for installs explicitly enabled by install.sh, never worker entrypoints.
pub fn automatic_enabled(target: &Path) -> bool {
    std::env::var_os("MGBROWSER_NO_AUTO_UPDATE").is_none()
        && target.with_file_name(MARKER).is_file()
}

pub fn update(target: &Path) -> Result<String, String> {
    let _lock = lock(target)?;
    let installed = binary_output(target, "--version")?;
    let current = installed
        .strip_prefix("mgbrowser ")
        .ok_or("Cannot identify installed version")?;
    let Some(tag) = release_tag(&download(API)?, current)? else {
        if version(current)? > version(env!("CARGO_PKG_VERSION"))? {
            return Ok(format!(
                "Installed v{current}. Restart mgbrowser to use the new build."
            ));
        }
        return Ok(format!("mgbrowser {current}: no newer stable release"));
    };
    let base = format!("{BASE}/{tag}/{ASSET}");
    let checksum = download(&format!("{base}.sha256"))?;
    let archive = download(&base)?;
    install(target, &archive, &checksum, &tag[1..])?;
    Ok(format!(
        "Installed {tag}. Restart mgbrowser to use the new build."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_versions_never_downgrade_or_accept_untrusted_tags() {
        for tag in ["v0.1.1", "v0.2.0"] {
            assert_eq!(
                release_tag(
                    format!(r#"{{"tag_name":"{tag}","draft":false,"prerelease":false}}"#)
                        .as_bytes(),
                    "0.2.0"
                )
                .unwrap(),
                None
            );
        }
        for tag in ["v1.0.0-rc1", "../../bad", "v01.0.0", "v1.0", "v1.0.0/path"] {
            assert!(
                release_tag(
                    format!(r#"{{"tag_name":"{tag}","draft":false,"prerelease":false}}"#)
                        .as_bytes(),
                    "0.2.0"
                )
                .is_err()
            );
        }
        assert_eq!(
            release_tag(
                br#"{"tag_name":"v0.10.0","draft":false,"prerelease":false}"#,
                "0.2.0"
            )
            .unwrap()
            .as_deref(),
            Some("v0.10.0")
        );
        assert!(
            release_tag(
                br#"{"tag_name":"v9.0.0","draft":false,"prerelease":true}"#,
                "0.2.0"
            )
            .is_err()
        );
    }
    #[test]
    fn checksum_is_required_and_exact() {
        let sum = format!("{}  {ASSET}\n", digest(b"archive"));
        assert!(verify(b"archive", sum.as_bytes()).is_ok());
        assert!(verify(b"corrupt", sum.as_bytes()).is_err());
        assert!(verify(b"archive", sum.replace(ASSET, "other").as_bytes()).is_err());
        assert!(verify(b"archive", format!("{sum}extra").as_bytes()).is_err());
        assert!(executable(b"not gzip").is_err());
    }
    fn archive(name: &str, kind: u8, content: &[u8], duplicate: bool) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[124..136].copy_from_slice(format!("{:011o}\0", content.len()).as_bytes());
        header[148..156].fill(b' ');
        header[156] = kind;
        let sum: usize = header.iter().map(|b| *b as usize).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut tar = Vec::new();
        for _ in 0..if duplicate { 2 } else { 1 } {
            tar.extend(header);
            tar.extend(content);
            tar.resize(tar.len().div_ceil(512) * 512, 0);
        }
        tar.extend([0u8; 1024]);
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&tar).unwrap();
        gzip.finish().unwrap()
    }
    #[test]
    fn archive_only_accepts_one_exact_regular_executable() {
        assert_eq!(
            executable(&archive(PAYLOAD, b'0', b"executable", false)).unwrap(),
            b"executable"
        );
        assert!(executable(&archive(PAYLOAD, b'2', b"link", false)).is_err());
        assert!(executable(&archive(PAYLOAD, b'0', b"duplicate", true)).is_err());
        assert!(executable(&archive("../../mgbrowser", b'0', b"escape", false)).is_err());
        assert!(executable(&archive(PAYLOAD, b'0', b"", false)).is_err());
        assert!(executable(&archive(PAYLOAD, b'0', b"truncated", false)[..24]).is_err());
    }
    #[test]
    #[ignore = "requires MGBROWSER_TEST_RELEASE pointing at the packaged release"]
    fn packaged_update_smoke() {
        // CI also runs this against the actual release archive, never a fake
        // success executable. Normal unit tests do not require a prebuilt release.
        let path = std::env::var_os("MGBROWSER_TEST_RELEASE").expect("set MGBROWSER_TEST_RELEASE");
        let archive = fs::read(&path).unwrap();
        let checksum = fs::read(format!("{}.sha256", Path::new(&path).display())).unwrap();
        let root = std::env::temp_dir().join(format!("mg-update-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let target = root.join("mgbrowser");
        fs::write(&target, b"old executable").unwrap();
        let guard = lock(&target).unwrap();
        assert!(lock(&target).is_err());
        assert!(install(&target, &archive, b"bad", env!("CARGO_PKG_VERSION")).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old executable");
        assert!(install(&target, &archive, &checksum, "999.0.0").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old executable");
        install(&target, &archive, &checksum, env!("CARGO_PKG_VERSION")).unwrap();
        validate_binary(&target, env!("CARGO_PKG_VERSION")).unwrap();
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "post-publication gate requiring MGBROWSER_TEST_OLD_BINARY"]
    fn public_update_smoke() {
        // Post-publication gate: use the actual prior public binary and real
        // release transport. No test endpoint or version bypass in production.
        let old =
            std::env::var_os("MGBROWSER_TEST_OLD_BINARY").expect("set MGBROWSER_TEST_OLD_BINARY");
        let root =
            std::env::temp_dir().join(format!("mg-public-update-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let target = root.join("mgbrowser");
        fs::copy(old, &target).unwrap();
        let before = fs::metadata(&target).unwrap().ino();
        assert!(update(&target).unwrap().starts_with("Installed "));
        assert_ne!(before, fs::metadata(&target).unwrap().ino());
        validate_binary(&target, env!("CARGO_PKG_VERSION")).unwrap();
        let installed = fs::metadata(&target).unwrap().ino();
        assert!(update(&target).unwrap().contains("no newer stable release"));
        assert_eq!(installed, fs::metadata(&target).unwrap().ino());
        fs::remove_dir_all(root).unwrap();
    }
}
