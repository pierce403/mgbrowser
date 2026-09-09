//! Linux window/event-loop composition for the Mg components.
use mg_browser::platform::{self, script_worker};
use mg_chassis::{Browser as App, BrowserCdp, JourneyOptions};
use std::{error::Error, thread, time::Duration};
use x11rb::{
    connection::Connection,
    protocol::{Event, xproto::*},
    wrapper::ConnectionExt as _,
};
const BG: u32 = 0xfafbf8;
fn main() -> Result<(), Box<dyn Error>> {
    // Worker dispatch must precede fonts, display, networking and debug-server setup.
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("mgbrowser {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "mgbrowser {} : Experimental Preview\nUsage: mgbrowser [URL] [OPTIONS]\nExample: mgbrowser https://example.com/\n\n  --enable-scripts              Enable incomplete experimental JavaScript\n  --remote-debugging-port PORT  Enable partial loopback CDP (0: free port)\n  --script-worker-selftest      Check restricted worker isolation\n  --version                    Print version\n  --help                       Show this help\n\nRequires Linux x86_64, X11/XWayland and a DejaVu/Liberation font.\nSet MGBROWSER_FONT to a TrueType/OpenType font file if needed.\nCtrl+L address; Enter navigate; Tab fields; Alt+Left back; wheel scroll.\nModern-web compatibility is poor. Do not use for sensitive browsing.",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        Some("--script-worker") => script_worker::worker_entry(),
        Some("--script-session") => script_worker::session_entry(),
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
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut initial = "https://example.com/".to_string();
    let mut debug_port: Option<u16> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--enable-scripts" => app.set_scripts_enabled(true),
            "--disable-scripts" => app.set_scripts_enabled(false),
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
                    "mgbrowser [URL] [--enable-scripts] [--remote-debugging-port PORT] [--smoke-search QUERY | --smoke-events] [--exit-after-smoke] [--evidence-dir DIR]\nOwn JavaScript interpreter is experimental and opt-in; see docs/JAVASCRIPT.md.\nCDP is opt-in, loopback-only, partial; port 0 selects an available port. See docs/CDP.md.\nCtrl+L address; Enter navigate/submit; Tab fields; mouse click links; wheel scroll; Alt+Left back.\nRequires X11/XWayland and a font file (MGBROWSER_FONT can override)."
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
    app.configure_journey(journey);
    let mut cdp = debug_port.map(BrowserCdp::bind).transpose()?;
    app.set_debugging(cdp.is_some());
    let (conn, screen_num) = x11rb::connect(None).map_err(|error| {
        format!("Cannot open X11 display: {error}. Run inside an X11 or XWayland desktop with DISPLAY set.")
    })?;
    let screen = &conn.setup().roots[screen_num];
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
        app.width() as u16,
        app.height() as u16,
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
                    app.resize(e.width as u32, e.height as u32);
                }
                Event::ButtonPress(e) => match e.detail {
                    1 => app.pointer_down(e.event_x as i32, e.event_y as i32),
                    4 => app.scroll_by(-100),
                    5 => app.scroll_by(100),
                    _ => {}
                },
                Event::ButtonRelease(e) if e.detail == 1 => {
                    app.pointer_up(e.event_x as i32, e.event_y as i32);
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
        if let Some(cdp) = &mut cdp {
            cdp.tick(&mut app);
        }
        if app.is_dirty() {
            let canvas = app.paint();
            // Split uploads below the core X11 request-size limit.
            let rows = (200_000 / (app.width() as usize * 4)).max(1);
            for (chunk, pixels) in canvas
                .pixels
                .chunks(rows * app.width() as usize)
                .enumerate()
            {
                let data: Vec<_> = pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
                conn.put_image(
                    ImageFormat::Z_PIXMAP,
                    window,
                    gc,
                    app.width() as u16,
                    (pixels.len() / app.width() as usize) as u16,
                    0,
                    (chunk * rows) as i16,
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
