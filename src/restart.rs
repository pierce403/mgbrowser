//! User-requested relaunch of the installed executable, not the running inode.
use std::{
    path::Path,
    process::{Child, Command},
};

/// Preserve launch policy and reopen the committed page with a fresh session.
/// Debug ports and test-journey arguments are deliberately not replayed.
pub fn launch(target: &Path, url: &str, scripts: bool, auto_update: bool) -> Result<Child, String> {
    if !target.is_absolute() {
        return Err("Restart target must be an absolute executable path".into());
    }
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("No loaded HTTP(S) page to reopen".into());
    }
    let mut command = Command::new(target);
    command.arg(url);
    if scripts {
        command.arg("--enable-scripts");
    }
    if !auto_update {
        command.arg("--no-auto-update");
    }
    command
        .spawn()
        .map_err(|e| format!("Restart failed; this window remains open: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_target_and_option_like_urls_without_launching() {
        assert!(launch(Path::new("mgbrowser"), "https://example.com/", false, false).is_err());
        assert!(
            launch(
                Path::new("/missing-mgbrowser"),
                "--script-worker",
                false,
                false
            )
            .is_err()
        );
        assert!(
            launch(
                Path::new("/missing-mgbrowser"),
                "https://example.com/",
                false,
                false
            )
            .unwrap_err()
            .contains("remains open")
        );
    }
}
