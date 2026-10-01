//! External X11 menu/close regression against an owned browser window.
use std::{
    error::Error,
    thread,
    time::{Duration, Instant},
};
use x11rb::{connection::Connection, protocol::xproto::*};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    let window: u32 = args.get(1).ok_or("window ID required")?.parse()?;
    let path = args.get(2).ok_or("screenshot path required")?;
    // Optional physical origin of the Browser surface below host tab chrome.
    // Existing single-page callers keep their exact coordinates by default.
    let surface_top: i16 = args
        .get(4)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(0);
    if !(0..=256).contains(&surface_top) {
        return Err("Surface offset must be 0..256 pixels".into());
    }
    let (conn, screen) = x11rb::connect(None)?;
    let geometry = conn.get_geometry(window)?.reply()?;
    let click = |x, y: i16| -> Result<(), Box<dyn Error>> {
        let y = y.checked_add(surface_top).ok_or("Click offset overflow")?;
        for (response_type, mask) in [
            (BUTTON_PRESS_EVENT, EventMask::BUTTON_PRESS),
            (BUTTON_RELEASE_EVENT, EventMask::BUTTON_RELEASE),
        ] {
            conn.send_event(
                false,
                window,
                mask,
                ButtonPressEvent {
                    response_type,
                    detail: 1,
                    sequence: 0,
                    time: 0,
                    root: conn.setup().roots[screen].root,
                    event: window,
                    child: 0,
                    root_x: x,
                    root_y: y,
                    event_x: x,
                    event_y: y,
                    state: KeyButMask::default(),
                    same_screen: true,
                },
            )?;
        }
        conn.flush()?;
        thread::sleep(Duration::from_millis(180));
        Ok(())
    };
    let capture = || -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                window,
                0,
                0,
                geometry.width,
                geometry.height,
                u32::MAX,
            )?
            .reply()?
            .data)
    };
    let save = |frame: &[u8], path: &str| -> Result<(), Box<dyn Error>> {
        let rgb: Vec<_> = frame
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        image::RgbImage::from_raw(geometry.width.into(), geometry.height.into(), rgb)
            .ok_or("bad screenshot size")?
            .save(path)?;
        Ok(())
    };
    let pixel = |frame: &[u8], x: usize, y: usize| {
        let at = (y * usize::from(geometry.width) + x) * 4;
        u32::from(frame[at + 2]) << 16 | u32::from(frame[at + 1]) << 8 | u32::from(frame[at])
    };
    let ink = |frame: &[u8], x: usize, y: usize, width: usize, height: usize| {
        (y..y + height)
            .flat_map(|y| (x..x + width).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel(frame, x, y) == 0x26342b)
            .count()
    };
    let wait_frame =
        |name: &str, ready: &dyn Fn(&[u8]) -> bool| -> Result<Vec<u8>, Box<dyn Error>> {
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut previous = Vec::new();
            let mut stable_since = Instant::now();
            loop {
                let frame = capture()?;
                if frame != previous || !ready(&frame) {
                    stable_since = Instant::now();
                } else if stable_since.elapsed() >= Duration::from_millis(250) {
                    return Ok(frame);
                }
                if Instant::now() >= deadline {
                    save(&frame, &format!("{path}.{name}-failed.png"))?;
                    return Err(format!("Requested {name} controls did not finish painting").into());
                }
                previous = frame;
                thread::sleep(Duration::from_millis(30));
            }
        };
    if args.get(3).is_none() || args.get(3).is_some_and(|mode| mode == "about") {
        // LOADED is logged before the framebuffer upload. WM_NAME follows it.
        let expected = if surface_top == 0 {
            "Local browser journey fixture"
        } else {
            "Local journey completed"
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let title = conn
                .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
                .reply()?;
            if String::from_utf8_lossy(&title.value).contains(expected) {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Initial fixture paint did not complete".into());
            }
            thread::sleep(Duration::from_millis(30));
        }
    }
    let before = wait_frame("initial-page", &|_| true)?;
    let modal_x = ((u32::from(geometry.width)
        - u32::from(geometry.width).saturating_sub(40).min(700))
        / 2) as usize;
    let top = surface_top as usize;
    let about_ready = |frame: &[u8]| {
        // Require real title and Close button text, not an empty panel color.
        pixel(frame, modal_x + 220, top + 370) == 0xe3e9df
            && ink(frame, modal_x + 16, top + 145, 180, 24) > 20
            && ink(frame, modal_x + 228, top + 374, 55, 24) > 10
    };
    if args.get(3).is_some_and(|mode| mode == "restart") {
        let about = wait_frame("restart-panel", &about_ready)?;
        save(&about, path)?;
        click(modal_x as i16 + 80, 380)?;
        println!("NATIVE_RESTART_CLICK_OK");
        return Ok(());
    }
    save(&before, &format!("{path}.before.png"))?;
    click(geometry.width as i16 - 28, 31)?;
    wait_frame("menu", &|frame| {
        let x = usize::from(geometry.width) - 226;
        pixel(frame, x + 2, top + 62) == 0xe3e9df && ink(frame, x + 10, top + 67, 170, 24) > 20
    })?;
    if args.get(3).is_some_and(|mode| mode == "update") {
        click(geometry.width as i16 - 140, 108)?;
        return Ok(());
    }
    click(geometry.width as i16 - 140, 74)?;
    let about = wait_frame("about", &about_ready)?;
    assert!(before != about, "About did not open");
    save(&about, path)?;
    click(modal_x as i16 + 248, 380)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let after = capture()?;
        if before == after {
            break;
        }
        if Instant::now() >= deadline {
            save(&after, &format!("{path}.after-failed.png"))?;
            assert!(before == after, "Close did not restore page");
        }
        thread::sleep(Duration::from_millis(30));
    }
    println!("NATIVE_ABOUT_MENU_OK {path}");
    Ok(())
}
