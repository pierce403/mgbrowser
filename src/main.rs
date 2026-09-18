//! Linux window/event-loop composition for the Mg components.
use mg_browser::platform::{self, script_worker};
use mg_browser::{COMPILED, REVISION, desktop, updater};
use mg_chassis::{Browser as App, JourneyOptions};
use std::error::Error;
#[global_allocator]
static ALLOCATOR: platform::worker_memory::WorkerAllocator =
    platform::worker_memory::WorkerAllocator;
fn main() -> Result<(), Box<dyn Error>> {
    // Worker dispatch must precede fonts, display, networking and debug-server setup.
    match std::env::args().nth(1).as_deref() {
        Some("--about") => {
            println!(
                "mgbrowser {}\nCompiled: {COMPILED}\nCommit: {REVISION}\nExperimental Preview : Apache-2.0",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        Some("--update") => {
            println!("{}", updater::update(&std::env::current_exe()?)?);
            return Ok(());
        }
        Some("--version" | "-V") => {
            println!("mgbrowser {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "Toolbar: Back, Forward, Refresh, Bookmark, URL, Menu.\nAlt+Left/Right: history; Ctrl+R/F5: refresh loaded page.\nCtrl+D: toggle bookmark; Ctrl+Shift+O: saved bookmarks.\nRight-click > Inspect element, F12 or Ctrl+Shift+I: inspector.\n"
            );
            println!(
                "Ctrl+T: new tab; Ctrl+W: close tab; Ctrl+Tab / Ctrl+Shift+Tab: switch tabs.\nDrag tabs to reorder, detach into windows or split left/right.\nMenu > Settings: theme and browser size (saved automatically).\nSize defaults to the desktop DPI; Ctrl+plus/minus changes size, Ctrl+0 restores System.\nMenu > About: version, compile time and commit.\n  --about                      Print build details\n  --update                     Check, verify and install a newer release\n  --no-auto-update             Disable background updates for this launch\nMGBROWSER_NO_AUTO_UPDATE=1 also disables background checks.\n"
            );
            println!(
                "mgbrowser {} : Experimental Preview\nUsage: mgbrowser [URL] [OPTIONS]\nExample: mgbrowser https://example.com/\n\n  --enable-scripts              Enable incomplete experimental JavaScript\n  --remote-debugging-port PORT  Enable partial loopback CDP (0: free port)\n  --script-worker-selftest      Check restricted worker isolation\n  --version                    Print version\n  --help                       Show this help\n\nRequires Linux x86_64, X11/XWayland and a DejaVu/Liberation font.\nSet MGBROWSER_FONT to a TrueType/OpenType font file if needed.\nCtrl+L address; Enter navigate; Tab fields; Alt+Left back; wheel scroll.\nModern-web compatibility is poor. Do not use for sensitive browsing.",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        Some("--script-worker") => script_worker::worker_entry(),
        Some("--script-session") => script_worker::session_entry(),
        #[cfg(all(
            feature = "legacy-test-engine",
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        Some("--legacy-script-worker") => script_worker::legacy_worker_entry(),
        #[cfg(all(
            feature = "legacy-test-engine",
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        Some("--legacy-script-session") => script_worker::legacy_session_entry(),
        Some("--script-session-selftest") => {
            script_worker::session_selftest()?;
            return Ok(());
        }
        Some("--script-worker-selftest") => {
            script_worker::selftest()?;
            return Ok(());
        }
        _ => {}
    }
    let session = mg_chassis::net::Session::new();
    let mut app = App::new_with_session(
        platform::load_fonts()?,
        std::sync::Arc::new(platform::LinuxScripts::default()),
        session.clone(),
    );
    let mut journey = JourneyOptions::default();
    app.set_build_info(env!("CARGO_PKG_VERSION"), COMPILED, REVISION);
    let executable = std::env::current_exe()?;
    let mut auto_update = updater::automatic_enabled(&executable);
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut initial = "https://example.com/".to_string();
    let mut restart_scripts = false;
    let mut restore_tabs = Vec::new();
    let mut debug_port: Option<u16> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-auto-update" => auto_update = false,
            "--restore-tab" => {
                i += 1;
                let url = args.get(i).ok_or("--restore-tab requires an HTTP(S) URL")?;
                if restore_tabs.len() >= 15
                    || url.len() > 8191
                    || !(url.starts_with("http://") || url.starts_with("https://"))
                    || url.chars().any(char::is_control)
                {
                    return Err("Restore supports at most 16 tabs with bounded HTTP(S) URLs".into());
                }
                restore_tabs.push(url.clone());
            }
            "--enable-scripts" => {
                app.set_scripts_enabled(true);
                restart_scripts = true;
            }
            #[cfg(feature = "legacy-test-engine")]
            "--legacy-page-tests" => script_worker::use_legacy_for_tests(),
            "--disable-scripts" => {
                app.set_scripts_enabled(false);
                restart_scripts = false;
            }
            "--remote-debugging-port" => {
                i += 1;
                debug_port = Some(
                    args.get(i)
                        .ok_or("--remote-debugging-port requires a port")?
                        .parse()?,
                );
            }
            value if value.starts_with("--remote-debugging-port=") => {
                debug_port = Some(value.split_once('=').unwrap().1.parse()?);
            }
            "--smoke-search" => {
                i += 1;
                journey.query = Some(
                    args.get(i)
                        .ok_or("--smoke-search requires a query")?
                        .clone(),
                );
            }
            "--smoke-events" => {
                journey.events = true;
                journey.query = Some("Rust & café".into());
            }
            "--exit-after-smoke" => journey.exit_after = true,
            "--evidence-dir" => {
                i += 1;
                journey.evidence_dir = args
                    .get(i)
                    .ok_or("--evidence-dir requires a directory")?
                    .clone();
            }
            "--help" => {
                println!(
                    "mgbrowser [URL] [--enable-scripts] [--remote-debugging-port PORT] [--smoke-search QUERY | --smoke-events] [--exit-after-smoke] [--evidence-dir DIR]\nBoa page JavaScript is experimental and opt-in; see docs/BOA.md.\nCDP is opt-in, loopback-only, partial; port 0 selects an available port. See docs/CDP.md.\nCtrl+L address; Enter navigate/submit; Tab fields; mouse click links; wheel scroll; Alt+Left back.\nRequires X11/XWayland and a font file (MGBROWSER_FONT can override)."
                );
                return Ok(());
            }
            value if value.starts_with('-') => {
                return Err(format!("Unknown option: {value}").into());
            }
            value => initial = value.to_string(),
        }
        i += 1;
    }
    // Test journeys/CDP are deterministic and never initiate unattended updates.
    if journey.query.is_some() || debug_port.is_some() {
        auto_update = false;
    }
    app.configure_journey(journey);
    desktop::run(
        app,
        desktop::Options {
            initial,
            restore_tabs,
            executable,
            scripts: restart_scripts,
            auto_update,
            debug_port,
            session,
        },
    )
}
