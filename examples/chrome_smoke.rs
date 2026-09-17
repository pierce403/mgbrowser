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
    let (conn, screen) = x11rb::connect(None)?;
    let geometry = conn.get_geometry(window)?.reply()?;
    let click = |x, y| -> Result<(), Box<dyn Error>> {
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
    if args.get(3).is_none() {
        // LOADED is logged before the framebuffer upload. WM_NAME follows it.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let title = conn
                .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
                .reply()?;
            if String::from_utf8_lossy(&title.value).contains("Local browser journey fixture") {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Initial fixture paint did not complete".into());
            }
            thread::sleep(Duration::from_millis(30));
        }
    }
    let before = capture()?;
    if args.get(3).is_some_and(|mode| mode == "restart") {
        let rgb: Vec<_> = before
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        image::RgbImage::from_raw(geometry.width.into(), geometry.height.into(), rgb)
            .ok_or("bad screenshot size")?
            .save(path)?;
        let x = ((u32::from(geometry.width)
            - u32::from(geometry.width).saturating_sub(40).min(700))
            / 2) as i16;
        click(x + 80, 380)?;
        println!("NATIVE_RESTART_CLICK_OK");
        return Ok(());
    }
    click(geometry.width as i16 - 28, 31)?;
    if args.get(3).is_some_and(|mode| mode == "update") {
        click(geometry.width as i16 - 140, 108)?;
        return Ok(());
    }
    click(geometry.width as i16 - 140, 74)?;
    let about = capture()?;
    assert_ne!(before, about, "About did not open");
    let rgb: Vec<_> = about
        .chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0]])
        .collect();
    image::RgbImage::from_raw(geometry.width.into(), geometry.height.into(), rgb)
        .ok_or("bad screenshot size")?
        .save(path)?;
    click(
        ((u32::from(geometry.width) - u32::from(geometry.width).saturating_sub(40).min(700)) / 2)
            as i16
            + 248,
        380,
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let after = capture()?;
        if before == after {
            break;
        }
        if Instant::now() >= deadline {
            assert_eq!(before, after, "Close did not restore page");
        }
        thread::sleep(Duration::from_millis(30));
    }
    println!("NATIVE_ABOUT_MENU_OK {path}");
    Ok(())
}
