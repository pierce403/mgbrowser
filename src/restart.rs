//! User-requested relaunch of the installed executable, not the running inode.
use std::{
    path::Path,
    process::{Child, Command},
};

/// Preserve launch policy and reopen the committed page with a fresh session.
/// Debug ports and test-journey arguments are deliberately not replayed.
pub fn launch(target: &Path, url: &str, scripts: bool, auto_update: bool) -> Result<Child, String> {
    launch_tabs(target, &[url.to_owned()], scripts, auto_update)
}

/// Reopen committed pages in a single window. Live sessions and pane placement
/// are deliberately not serialized. Arguments never pass through a shell.
pub fn launch_tabs(
    target: &Path,
    urls: &[String],
    scripts: bool,
    auto_update: bool,
) -> Result<Child, String> {
    restart_command(target, urls, scripts, auto_update)?
        .spawn()
        .map_err(|e| format!("Restart failed; this window remains open: {e}"))
}

fn restart_command(
    target: &Path,
    urls: &[String],
    scripts: bool,
    auto_update: bool,
) -> Result<Command, String> {
    if !target.is_absolute() {
        return Err("Restart target must be an absolute executable path".into());
    }
    if urls.is_empty() || urls.len() > 16 {
        return Err("Restart requires between 1 and 16 committed pages".into());
    }
    for url in urls {
        if (!url.starts_with("https://") && !url.starts_with("http://"))
            || url.chars().any(char::is_control)
            || url.len() > 8191
        {
            return Err("Restart requires HTTP(S) URLs of at most 8191 bytes".into());
        }
    }
    let mut command = Command::new(target);
    command.arg(&urls[0]);
    for url in &urls[1..] {
        command.args(["--restore-tab", url]);
    }
    if scripts {
        command.arg("--enable-scripts");
    }
    if !auto_update {
        command.arg("--no-auto-update");
    }
    Ok(command)
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

    #[test]
    fn preserves_all_tab_urls_and_policy_as_literal_arguments() {
        let urls = vec![
            "https://example.com/?q=a&x=1".into(),
            "http://localhost:7878/search?q=Rust%20%26%20caf%C3%A9".into(),
            "https://example.com/quote'and;semicolon".into(),
        ];
        let command = restart_command(Path::new("/tmp/mgbrowser"), &urls, true, false).unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            args,
            vec![
                urls[0].as_str(),
                "--restore-tab",
                urls[1].as_str(),
                "--restore-tab",
                urls[2].as_str(),
                "--enable-scripts",
                "--no-auto-update",
            ]
        );
        assert!(!args.iter().any(|arg| arg.contains("remote-debugging")));
    }

    #[test]
    fn rejects_invalid_tab_sets_before_spawning() {
        let target = Path::new("/tmp/mgbrowser");
        for urls in [
            vec![],
            vec!["https://example.com/".into(); 17],
            vec!["https://example.com/".into(), "--script-worker".into()],
            vec![format!("https://example.com/{}", "x".repeat(8192))],
            vec!["https://example.com/\0".into()],
            vec!["https://example.com/\n".into()],
        ] {
            assert!(restart_command(target, &urls, false, true).is_err());
        }
        let valid = vec!["https://example.com/".into(); 16];
        assert!(restart_command(target, &valid, false, true).is_ok());
    }
}
