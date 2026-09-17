//! Linux window/event-loop composition for the Mg components.
use mg_browser::platform::{self, script_worker};
use mg_browser::{
    COMPILED, REVISION,
    settings::{Preferences, Settings},
    updater,
};
use mg_chassis::{Browser as App, BrowserCdp, ColorScheme, JourneyOptions};
use std::{error::Error, thread, time::Duration};
use x11rb::{
    connection::Connection,
    protocol::{Event, xproto::*},
    wrapper::ConnectionExt as _,
};
const BG: u32 = 0xfafbf8;

fn scaled_dimension(logical: u32, percent: u16) -> u32 {
    ((u64::from(logical) * u64::from(percent.clamp(75, 300)) + 50) / 100).min(u64::from(u32::MAX))
        as u32
}

fn logical_coordinate(physical: i16, percent: u16) -> i32 {
    // Floor, rather than truncate, so a negative coordinate stays outside.
    (i32::from(physical) * 100).div_euclid(i32::from(percent.clamp(75, 300)))
}

fn set_size_hints<C: Connection>(
    conn: &C,
    window: Window,
    percent: u16,
) -> Result<(), Box<dyn Error>> {
    x11rb::properties::WmSizeHints {
        min_size: Some((
            scaled_dimension(360, percent) as i32,
            scaled_dimension(240, percent) as i32,
        )),
        max_size: Some((4800, 3600)),
        ..Default::default()
    }
    .set_normal_hints(conn, window)?;
    Ok(())
}
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
                "Toolbar: Menu, Back, Forward, Refresh, Bookmark, URL.\nAlt+Left/Right: history; Ctrl+R/F5: refresh loaded page.\nCtrl+D: toggle bookmark; Ctrl+Shift+O: saved bookmarks.\n"
            );
            println!(
                "Menu > Settings: theme and browser size (saved automatically).\nSize defaults to the desktop DPI; Ctrl+plus/minus changes size, Ctrl+0 restores System.\nMenu > About: version, compile time and commit.\n  --about                      Print build details\n  --update                     Check, verify and install a newer release\n  --no-auto-update             Disable background updates for this launch\nMGBROWSER_NO_AUTO_UPDATE=1 also disables background checks.\n"
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
    let mut app = App::new(
        platform::load_fonts()?,
        std::sync::Arc::new(platform::LinuxScripts::default()),
    );
    let mut journey = JourneyOptions::default();
    app.set_build_info(env!("CARGO_PKG_VERSION"), COMPILED, REVISION);
    let executable = std::env::current_exe()?;
    let mut auto_update = updater::automatic_enabled(&executable);
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut initial = "https://example.com/".to_string();
    let mut restart_scripts = false;
    let mut debug_port: Option<u16> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-auto-update" => auto_update = false,
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
    let settings = Settings::user();
    let bookmarks = mg_browser::bookmarks::BookmarkStore::user();
    match bookmarks.load() {
        Ok(entries) => app.set_bookmarks(entries),
        Err(error) => app.set_bookmark_status(error),
    }
    match settings.load() {
        Ok(preferences) => {
            app.set_theme_preference(preferences.theme);
            app.set_scale_preference(preferences.scale);
        }
        Err(error) => {
            eprintln!("SETTINGS: {error}");
            app.set_settings_status(error);
        }
    }
    let (update_tx, update_rx) = std::sync::mpsc::channel();
    let mut update_running = false;
    let mut restart_ready = false;
    let mut next_update = std::time::Instant::now();
    app.set_update_status(
        if auto_update {
            "Automatic updates enabled"
        } else {
            "Automatic checks off; use Check for updates"
        }
        .into(),
    );
    let mut cdp = debug_port.map(BrowserCdp::bind).transpose()?;
    app.set_debugging(cdp.is_some());
    let (conn, screen_num) = x11rb::connect(None).map_err(|error| {
        format!("Cannot open X11 display: {error}. Run inside an X11 or XWayland desktop with DISPLAY set.")
    })?;
    let screen = &conn.setup().roots[screen_num];
    let system_scale = platform::scaling::read_scale(&conn, screen_num).unwrap_or(100);
    app.set_system_scale(system_scale);
    let mut applied_scale = app.effective_scale_percent();
    let mut surface_width = scaled_dimension(app.width(), applied_scale)
        .min(
            u32::from(screen.width_in_pixels)
                .saturating_sub(80)
                .max(360),
        )
        .min(4800);
    let mut surface_height = scaled_dimension(app.height(), applied_scale)
        .min(
            u32::from(screen.height_in_pixels)
                .saturating_sub(80)
                .max(240),
        )
        .min(3600);
    app.resize_surface(surface_width, surface_height);
    let mut next_scale_check = std::time::Instant::now() + Duration::from_secs(2);
    eprintln!("DISPLAY_SCALE system={system_scale}% effective={applied_scale}%");
    let depth = screen.root_depth;
    let format = conn
        .setup()
        .pixmap_formats
        .iter()
        .find(|f| f.depth == depth)
        .ok_or("Unsupported X11 visual")?;
    if format.bits_per_pixel != 32 || conn.setup().image_byte_order != ImageOrder::LSB_FIRST {
        return Err("Initial window backend requires a 32-bit little-endian pixel surface".into());
    }
    let window = conn.generate_id()?;
    let gc = conn.generate_id()?;
    conn.create_window(
        depth,
        window,
        screen.root,
        40,
        40,
        surface_width as u16,
        surface_height as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().background_pixel(BG).event_mask(
            EventMask::EXPOSURE
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::KEY_PRESS
                | EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE,
        ),
    )?;
    set_size_hints(&conn, window, applied_scale)?;
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"mgbrowser - research browser",
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"mgbrowser\0mgbrowser\0",
    )?;
    let protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
    let theme_atom = conn
        .intern_atom(false, b"_GTK_THEME_VARIANT")?
        .reply()?
        .atom;
    let utf8_atom = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
    let system_themes = platform::appearance::spawn_watcher();
    let mut window_theme = None;
    let icon_atom = conn.intern_atom(false, b"_NET_WM_ICON")?.reply()?.atom;
    let icon = image::load_from_memory_with_format(
        include_bytes!("../assets/mgbrowser-32.png"),
        image::ImageFormat::Png,
    )?
    .into_rgba8();
    let mut icon_data = vec![icon.width(), icon.height()];
    icon_data.extend(icon.pixels().map(|pixel| {
        (u32::from(pixel[3]) << 24)
            | (u32::from(pixel[0]) << 16)
            | (u32::from(pixel[1]) << 8)
            | u32::from(pixel[2])
    }));
    conn.change_property32(
        PropMode::REPLACE,
        window,
        icon_atom,
        AtomEnum::CARDINAL,
        &icon_data,
    )?;
    let close = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        protocols,
        AtomEnum::ATOM,
        &[close],
    )?;
    conn.create_gc(gc, window, &CreateGCAux::new())?;
    conn.map_window(window)?;
    conn.flush()?;
    let first = conn.setup().min_keycode;
    let mapping = conn
        .get_keyboard_mapping(first, conn.setup().max_keycode - first + 1)?
        .reply()?;
    eprintln!("WINDOW id={window} pid={}", std::process::id());
    app.navigate(initial, None, true);
    loop {
        while let Some(event) = conn.poll_for_event()? {
            match event {
                Event::Expose(_) => app.request_redraw(),
                Event::ConfigureNotify(e) => {
                    surface_width = u32::from(e.width).clamp(1, 4800);
                    surface_height = u32::from(e.height).clamp(1, 3600);
                    app.resize_surface(surface_width, surface_height);
                }
                Event::ButtonPress(e) => match e.detail {
                    1 => app.pointer_down(
                        logical_coordinate(e.event_x, app.effective_scale_percent()),
                        logical_coordinate(e.event_y, app.effective_scale_percent()),
                    ),
                    4 => app.scroll_by(-100),
                    5 => app.scroll_by(100),
                    _ => {}
                },
                Event::ButtonRelease(e) if e.detail == 1 => {
                    app.pointer_up(
                        logical_coordinate(e.event_x, app.effective_scale_percent()),
                        logical_coordinate(e.event_y, app.effective_scale_percent()),
                    );
                }
                Event::KeyPress(e) => {
                    let shift = e.state.contains(KeyButMask::SHIFT);
                    let ctrl = e.state.contains(KeyButMask::CONTROL);
                    let alt = e.state.contains(KeyButMask::MOD1);
                    let base = (e.detail.saturating_sub(first)) as usize
                        * mapping.keysyms_per_keycode as usize;
                    let mut sym = *mapping.keysyms.get(base + usize::from(shift)).unwrap_or(&0);
                    if sym == 0 {
                        sym = *mapping.keysyms.get(base).unwrap_or(&0);
                    }
                    if let Some(key) = platform::translate_keysym(sym) {
                        app.handle_key(key, ctrl, shift, alt);
                    }
                }
                Event::ClientMessage(e) if e.data.as_data32()[0] == close => return Ok(()),
                Event::DestroyNotify(_) => return Ok(()),
                _ => {}
            }
        }
        app.poll();
        if app.take_bookmark_refresh() {
            match bookmarks.load() {
                Ok(entries) => app.set_bookmarks(entries),
                Err(error) => app.set_bookmark_status(error),
            }
        }
        if let Some(change) = app.take_bookmark_change() {
            match bookmarks.apply(&change) {
                Ok(entries) => {
                    app.set_bookmarks(entries);
                    app.set_bookmark_status(
                        match change {
                            mg_chassis::bookmarks::BookmarkChange::Add(_) => "Bookmark saved",
                            mg_chassis::bookmarks::BookmarkChange::Remove(_) => "Bookmark removed",
                        }
                        .into(),
                    );
                }
                Err(error) => app.set_bookmark_status(error),
            }
        }
        while let Ok(theme) = system_themes.try_recv() {
            app.set_system_theme(theme);
        }
        if std::time::Instant::now() >= next_scale_check {
            app.set_system_scale(platform::scaling::read_scale(&conn, screen_num).unwrap_or(100));
            next_scale_check = std::time::Instant::now() + Duration::from_secs(2);
        }
        let theme_changed = app.take_theme_change().is_some();
        let scale_changed = app.take_scale_change().is_some();
        if theme_changed || scale_changed {
            match settings.save(Preferences {
                theme: app.theme_preference(),
                scale: app.scale_preference(),
            }) {
                Ok(()) => app.set_settings_status("Settings saved".into()),
                Err(error) => {
                    eprintln!("SETTINGS: {error}");
                    app.set_settings_status(format!("Session only: {error}"));
                }
            }
        }
        let scale = app.effective_scale_percent();
        if scale != applied_scale {
            app.resize_surface(surface_width, surface_height);
            set_size_hints(&conn, window, scale)?;
            // A fractional scale can leave a one-pixel edge after rounding.
            conn.clear_area(false, window, 0, 0, 0, 0)?;
            applied_scale = scale;
            eprintln!("DISPLAY_SCALE effective={scale}%");
        }
        let theme = app.effective_theme();
        if window_theme != Some(theme) {
            // A window-manager hint, not a GTK dependency. Decorations remain WM-owned.
            conn.change_property8(
                PropMode::REPLACE,
                window,
                theme_atom,
                utf8_atom,
                if theme == ColorScheme::Dark {
                    b"dark"
                } else {
                    b""
                },
            )?;
            conn.flush()?;
            window_theme = Some(theme);
        }
        let requested = app.take_update_request();
        if !update_running
            && !restart_ready
            && (requested || (auto_update && std::time::Instant::now() >= next_update))
        {
            update_running = true;
            next_update = std::time::Instant::now() + Duration::from_secs(24 * 60 * 60);
            app.set_update_status("Checking for updates...".into());
            let tx = update_tx.clone();
            let target = executable.clone();
            thread::spawn(move || {
                let result = updater::update_with_progress(&target, &mut |event| {
                    let _ = tx.send(event);
                });
                let _ = tx.send(updater::UpdateEvent::Finished(result));
            });
        }
        while let Ok(event) = update_rx.try_recv() {
            let result = match event {
                updater::UpdateEvent::Status(status) => {
                    app.set_update_status(status.into());
                    continue;
                }
                updater::UpdateEvent::Download { received, total } => {
                    app.set_update_progress(received, total);
                    continue;
                }
                updater::UpdateEvent::Finished(result) => result,
            };
            update_running = false;
            restart_ready = result
                .as_ref()
                .is_ok_and(|message| message.starts_with("Installed "));
            app.set_restart_available(restart_ready);
            let status = result.unwrap_or_else(|error| format!("Update failed: {error}"));
            eprintln!("UPDATE: {status}");
            app.set_update_status(status);
        }
        if app.take_restart_request() && restart_ready {
            let restart_url = if app.page_url().starts_with("http://")
                || app.page_url().starts_with("https://")
            {
                app.page_url()
            } else {
                "https://example.com/"
            };
            match mg_browser::restart::launch(
                &executable,
                restart_url,
                restart_scripts,
                auto_update,
            ) {
                Ok(child) => {
                    eprintln!("RESTART: launched pid={}", child.id());
                    // Returning drops Chassis, cancels/reaps its workers and closes
                    // the old display connection. The new process uses disk bytes.
                    return Ok(());
                }
                Err(error) => app.set_update_status(error),
            }
        }
        if let Some(cdp) = &mut cdp {
            cdp.tick(&mut app);
        }
        if app.is_dirty() {
            let canvas = app.paint();
            // Split uploads below the core X11 request-size limit.
            let width = canvas.width.min(surface_width);
            let height = canvas.height.min(surface_height);
            let rows = (200_000 / (width as usize * 4)).max(1);
            for first_row in (0..height as usize).step_by(rows) {
                let end_row = (first_row + rows).min(height as usize);
                let mut data = Vec::with_capacity((end_row - first_row) * width as usize * 4);
                for row in first_row..end_row {
                    let start = row * canvas.width as usize;
                    data.extend(
                        canvas.pixels[start..start + width as usize]
                            .iter()
                            .flat_map(|p| p.to_le_bytes()),
                    );
                }
                conn.put_image(
                    ImageFormat::Z_PIXMAP,
                    window,
                    gc,
                    width as u16,
                    (end_row - first_row) as u16,
                    0,
                    first_row as i16,
                    0,
                    depth,
                    &data,
                )?;
            }
            conn.flush()?;
            let title = format!("{} : mgbrowser", app.visible_title());
            conn.change_property8(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                title.as_bytes(),
            )?;
            app.advance_journey(&canvas);
        } else if app.journey_needs_redraw() {
            app.request_redraw();
        }
        if let Some(success) = app.journey_result() {
            if !success {
                drop(app);
                std::process::exit(2);
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(16));
    }
}

#[cfg(test)]
mod display_coordinate_tests {
    use super::*;

    #[test]
    fn native_coordinates_and_sizes_follow_the_same_scale() {
        for percent in [75, 100, 125, 150, 175, 200, 250, 300] {
            assert_eq!(scaled_dimension(400, percent), 4 * u32::from(percent));
            assert_eq!(logical_coordinate((4 * percent) as i16, percent), 400);
            assert_eq!(
                logical_coordinate(-1, percent),
                -1 - i32::from(percent == 75)
            );
        }
        assert_eq!(logical_coordinate(501, 200), 250);
        assert_eq!(scaled_dimension(u32::MAX, 300), u32::MAX);
    }
}
