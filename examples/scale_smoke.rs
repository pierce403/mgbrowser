//! Native whole-browser scaling acceptance on the wrapper's owned Xvfb only.
use std::{
    error::Error,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection, properties::WmSizeHints, protocol::xproto::*,
    rust_connection::RustConnection, wrapper::ConnectionExt as _,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const HTTP: u32 = 0x9f202b;
const BASE: &str = "http://127.0.0.1:7878";

fn wait(label: &str, mut test: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if test()? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("Timed out: {label}").into())
}
fn physical(logical: i32, percent: u16) -> i16 {
    ((logical * i32::from(percent) + 50) / 100) as i16
}

struct Display {
    conn: RustConnection,
    root: u32,
    settings_window: u32,
    selection: u32,
    setting_atom: u32,
}
impl Display {
    fn new() -> Result<Self> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let settings_window = conn.generate_id()?;
        conn.create_window(
            0,
            settings_window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )?;
        let selection = conn
            .intern_atom(false, format!("_XSETTINGS_S{screen}").as_bytes())?
            .reply()?
            .atom;
        let setting_atom = conn
            .intern_atom(false, b"_XSETTINGS_SETTINGS")?
            .reply()?
            .atom;
        Ok(Self {
            conn,
            root,
            settings_window,
            selection,
            setting_atom,
        })
    }
    fn dpi(&self, dpi: &str) -> Result<()> {
        self.conn.change_property8(
            PropMode::REPLACE,
            self.root,
            AtomEnum::RESOURCE_MANAGER,
            AtomEnum::STRING,
            format!("Xft.dpi: {dpi}\n").as_bytes(),
        )?;
        self.conn.flush()?;
        Ok(())
    }
    fn xsettings(&self, dpi: Option<u32>) -> Result<()> {
        if let Some(dpi) = dpi {
            let mut bytes = vec![0, 0, 0, 0];
            bytes.extend(1u32.to_le_bytes()); // serial
            bytes.extend(1u32.to_le_bytes()); // number of settings
            bytes.extend([0, 0, 7, 0]); // integer, name length
            bytes.extend(b"Xft/DPI\0");
            bytes.extend(1u32.to_le_bytes());
            bytes.extend((dpi * 1024).to_le_bytes());
            self.conn.change_property8(
                PropMode::REPLACE,
                self.settings_window,
                self.setting_atom,
                self.setting_atom,
                &bytes,
            )?;
            self.conn.set_selection_owner(
                self.settings_window,
                self.selection,
                x11rb::CURRENT_TIME,
            )?;
        } else {
            self.conn
                .set_selection_owner(x11rb::NONE, self.selection, x11rb::CURRENT_TIME)?;
        }
        self.conn.flush()?;
        Ok(())
    }
}

struct Frame {
    width: u16,
    height: u16,
    bytes: Vec<u8>,
}
impl Frame {
    fn pixel(&self, x: usize, y: usize) -> u32 {
        let offset = (y * usize::from(self.width) + x) * 4;
        u32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap()) & 0xffffff
    }
    fn save(&self, path: &Path) -> Result<()> {
        let rgb = self
            .bytes
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        image::RgbImage::from_raw(self.width.into(), self.height.into(), rgb)
            .ok_or("bad frame")?
            .save(path)?;
        Ok(())
    }
    fn sharp_two_x_text(&self, logical_rect: (usize, usize, usize, usize)) {
        let (x, y, w, h) = logical_rect;
        let varied = (y..y + h)
            .flat_map(|y| (x..x + w).map(move |x| (2 * x, 2 * y)))
            .filter(|&(x, y)| {
                let first = self.pixel(x, y);
                self.pixel(x + 1, y) != first
                    || self.pixel(x, y + 1) != first
                    || self.pixel(x + 1, y + 1) != first
            })
            .count();
        assert!(
            varied > 10,
            "text appears to be enlarged 1x blocks, not physical rasterization"
        );
    }
}

struct Browser<'a> {
    display: &'a Display,
    child: Child,
    window: u32,
    log: PathBuf,
    scale: u16,
}
impl Drop for Browser<'_> {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl<'a> Browser<'a> {
    fn start(display: &'a Display, payload: &Path, scratch: &Path, name: &str) -> Result<Self> {
        let log = scratch.join(format!("{name}.log"));
        let file = File::create(&log)?;
        let child = Command::new(payload)
            .args([
                &format!("{BASE}/"),
                "--no-auto-update",
                "--remote-debugging-port=0",
            ])
            .env("XDG_CONFIG_HOME", scratch.join("config"))
            .env("XDG_CACHE_HOME", scratch.join("cache"))
            .env("XDG_DATA_HOME", scratch.join("data"))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}/missing-bus", scratch.display()),
            )
            .stdin(Stdio::null())
            .stdout(file.try_clone()?)
            .stderr(Stdio::from(file))
            .spawn()?;
        let mut browser = Self {
            display,
            child,
            window: 0,
            log,
            scale: 100,
        };
        wait("native window and fixture", || {
            if let Some(status) = browser.child.try_wait()? {
                return Err(format!("browser exited {status}").into());
            }
            let log = fs::read_to_string(&browser.log)?;
            browser.window = log
                .lines()
                .find_map(|line| {
                    line.strip_prefix("WINDOW id=")?
                        .split_whitespace()
                        .next()?
                        .parse()
                        .ok()
                })
                .unwrap_or(0);
            if browser.window == 0 || !log.lines().any(|line| line.starts_with("LOADED ")) {
                return Ok(false);
            }
            let native = display
                .conn
                .get_property(
                    false,
                    browser.window,
                    AtomEnum::WM_NAME,
                    AtomEnum::STRING,
                    0,
                    1024,
                )?
                .reply()?;
            Ok(String::from_utf8_lossy(&native.value).contains("Local browser journey fixture"))
        })?;
        Ok(browser)
    }
    fn frame(&self) -> Result<Frame> {
        let c = &self.display.conn;
        let geometry = c.get_geometry(self.window)?.reply()?;
        let bytes = c
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.window,
                0,
                0,
                geometry.width,
                geometry.height,
                u32::MAX,
            )?
            .reply()?
            .data;
        Ok(Frame {
            width: geometry.width,
            height: geometry.height,
            bytes,
        })
    }
    fn wait_scale(&mut self, percent: u16) -> Result<()> {
        let result = wait(&format!("rendered scale {percent}%"), || {
            let frame = self.frame()?;
            let edge = physical(63, percent) as usize;
            let hints = WmSizeHints::get_normal_hints(&self.display.conn, self.window)?.reply()?;
            Ok(frame.pixel(0, 0) == HTTP
                && frame.pixel(0, edge - 1) == HTTP
                && frame.pixel(0, edge) != HTTP
                && frame.pixel(
                    physical(241, percent) as usize,
                    physical(13, percent) as usize,
                ) != HTTP
                && matches!(
                    frame.pixel(0, usize::from(frame.height) - 2),
                    0x202622 | 0xeef1e9
                )
                && matches!(
                    frame.pixel(
                        physical(12, percent) as usize,
                        physical(16, percent) as usize
                    ),
                    0x323c35 | 0xe3e9df
                )
                && hints.is_some_and(|h| {
                    h.min_size
                        == Some((
                            i32::from(physical(360, percent)),
                            i32::from(physical(240, percent)),
                        ))
                        && h.max_size == Some((4800, 3600))
                }))
        });
        if result.is_err() {
            let path = self.log.with_extension("failure.png");
            self.frame()?.save(&path)?;
            eprintln!("Native failure frame: {}", path.display());
        }
        result?;
        self.scale = percent;
        self.settled()?;
        Ok(())
    }
    fn settled(&self) -> Result<Frame> {
        let mut previous = self.frame()?;
        let mut equal = 0;
        wait("settled native frame", || {
            thread::sleep(Duration::from_millis(100));
            let next = self.frame()?;
            equal = if previous.bytes == next.bytes {
                equal + 1
            } else {
                0
            };
            previous = next;
            Ok(equal >= 2)
        })?;
        Ok(previous)
    }
    fn button(&self, x: i16, y: i16, detail: u8) -> Result<()> {
        self.send_button(x, y, detail)?;
        thread::sleep(Duration::from_millis(100));
        Ok(())
    }
    fn send_button(&self, x: i16, y: i16, detail: u8) -> Result<()> {
        for (response_type, mask) in [
            (BUTTON_PRESS_EVENT, EventMask::BUTTON_PRESS),
            (BUTTON_RELEASE_EVENT, EventMask::BUTTON_RELEASE),
        ] {
            self.display.conn.send_event(
                false,
                self.window,
                mask,
                ButtonPressEvent {
                    response_type,
                    detail,
                    sequence: 0,
                    time: 0,
                    root: self.display.root,
                    event: self.window,
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
        self.display.conn.flush()?;
        Ok(())
    }
    fn scrollbar_top(&self) -> Result<usize> {
        let geometry = self.display.conn.get_geometry(self.window)?.reply()?;
        let pixels = self
            .display
            .conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.window,
                geometry.width as i16 - physical(6, self.scale),
                0,
                1,
                geometry.height,
                u32::MAX,
            )?
            .reply()?
            .data;
        pixels
            .chunks_exact(4)
            .position(|p| {
                matches!(
                    u32::from_le_bytes([p[0], p[1], p[2], 0]),
                    0x8b9c83 | 0x80967f
                )
            })
            .ok_or_else(|| "Scroll thumb not visible".into())
    }
    fn smooth_wheel(&self, scratch: &Path) -> Result<()> {
        self.navigate("/")?;
        self.settled()?;
        let first = self.scrollbar_top()?;
        self.send_button(physical(400, self.scale), physical(175, self.scale), 5)?;
        let start = Instant::now();
        let mut samples = vec![first];
        while start.elapsed() < Duration::from_millis(550) {
            let next = self.scrollbar_top()?;
            if samples.last() != Some(&next) {
                samples.push(next);
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(
            samples.len() >= 3,
            "Wheel jumped without intermediate frames: {samples:?}"
        );
        assert!(
            samples.windows(2).all(|p| p[0] < p[1]),
            "Wheel overshot or reversed: {samples:?}"
        );
        let end = self.scrollbar_top()?;
        thread::sleep(Duration::from_millis(100));
        assert_eq!(self.scrollbar_top()?, end, "Animation failed to settle");
        self.settled()?
            .save(&scratch.join(format!("smooth-wheel-{}.png", self.scale)))?;
        self.send_button(physical(400, self.scale), physical(175, self.scale), 4)?;
        wait("wheel returns to initial position", || {
            Ok(self.scrollbar_top()? == first)
        })?;
        println!("NATIVE_SMOOTH_WHEEL_OK {}% {samples:?}", self.scale);
        Ok(())
    }
    fn click(&self, x: i32, y: i32) -> Result<()> {
        self.button(physical(x, self.scale), physical(y, self.scale), 1)
    }
    fn key(&self, symbol: u32, ctrl: bool) -> Result<()> {
        let c = &self.display.conn;
        let first = c.setup().min_keycode;
        let map = c
            .get_keyboard_mapping(first, c.setup().max_keycode - first + 1)?
            .reply()?;
        let (code, shift) = map
            .keysyms
            .chunks(usize::from(map.keysyms_per_keycode))
            .enumerate()
            .find_map(|(i, v)| {
                v.iter()
                    .take(2)
                    .position(|&s| s == symbol)
                    .map(|column| (first + i as u8, column == 1))
            })
            .ok_or("keysym unavailable")?;
        let mut state = KeyButMask::default();
        if ctrl {
            state |= KeyButMask::CONTROL;
        }
        if shift {
            state |= KeyButMask::SHIFT;
        }
        for (response_type, mask) in [
            (KEY_PRESS_EVENT, EventMask::KEY_PRESS),
            (KEY_RELEASE_EVENT, EventMask::KEY_RELEASE),
        ] {
            c.send_event(
                false,
                self.window,
                mask,
                KeyPressEvent {
                    response_type,
                    detail: code,
                    sequence: 0,
                    time: 0,
                    root: self.display.root,
                    event: self.window,
                    child: 0,
                    root_x: 0,
                    root_y: 0,
                    event_x: 0,
                    event_y: 0,
                    state,
                    same_screen: true,
                },
            )?;
        }
        c.flush()?;
        Ok(())
    }
    fn text(&self, value: &str) -> Result<()> {
        for c in value.chars() {
            self.key(c as u32, false)?;
        }
        Ok(())
    }
    fn loaded_count(&self) -> Result<usize> {
        Ok(fs::read_to_string(&self.log)?
            .lines()
            .filter(|l| l.starts_with("LOADED "))
            .count())
    }
    fn loaded_after(&self, count: usize, fragment: &str) -> Result<()> {
        wait(fragment, || {
            let log = fs::read_to_string(&self.log)?;
            let lines: Vec<_> = log.lines().filter(|l| l.starts_with("LOADED ")).collect();
            let Some(line) = lines
                .last()
                .filter(|line| lines.len() > count && line.contains(fragment))
            else {
                return Ok(false);
            };
            let title = line
                .split_once("title=\"")
                .and_then(|(_, text)| text.split('"').next())
                .ok_or("missing fixture title")?;
            // The host updates WM_NAME after uploading the completed frame;
            // LOADED alone precedes that upload, especially in large debug builds.
            let native = self
                .display
                .conn
                .get_property(
                    false,
                    self.window,
                    AtomEnum::WM_NAME,
                    AtomEnum::STRING,
                    0,
                    1024,
                )?
                .reply()?;
            Ok(String::from_utf8_lossy(&native.value).contains(title))
        })?;
        thread::sleep(Duration::from_millis(150));
        Ok(())
    }
    fn navigate(&self, path: &str) -> Result<()> {
        let count = self.loaded_count()?;
        self.key(u32::from(b'l'), true)?;
        self.text(&format!("{BASE}{path}"))?;
        self.key(0xff0d, false)?;
        self.loaded_after(count, path)
    }
    fn journey(&self) -> Result<()> {
        self.navigate("/")?;
        self.click(100, 196)?;
        self.text(&format!("scale{}", self.scale))?;
        let count = self.loaded_count()?;
        self.click(90, 243)?;
        self.loaded_after(count, "Local fixture results")?;
        // Locate the actual rendered blue link, then deliver independent X11 input.
        let frame = self.frame()?;
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for y in physical(64, self.scale) as usize
            ..usize::from(frame.height) - physical(29, self.scale) as usize
        {
            for x in 0..usize::from(frame.width) - 20 {
                if frame.pixel(x, y) == 0x174ea6 {
                    bounds = Some(match bounds {
                        None => (x, y, x, y),
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    });
                }
            }
        }
        let (x0, y0, x1, y1) = bounds.ok_or("result link not painted")?;
        let link = ((x0 + x1) / 2, (y0 + y1) / 2);
        let count = self.loaded_count()?;
        self.button(link.0 as i16, link.1 as i16, 1)?;
        self.loaded_after(count, "Local journey completed")?;
        println!("NATIVE_SCALE_JOURNEY_OK {}%", self.scale);
        self.navigate("/")
    }
    fn cdp_journey(&self, scratch: &Path) -> Result<()> {
        let log = fs::read_to_string(&self.log)?;
        let endpoint = log
            .lines()
            .find_map(|line| line.strip_prefix("CDP listening on "))
            .and_then(|line| line.split_whitespace().next())
            .ok_or("CDP endpoint missing")?;
        let output = Command::new("target/debug/examples/cdp_journey")
            .arg(endpoint)
            .arg(format!("{BASE}/"))
            .arg(scratch.join(format!("cdp-{}.png", self.scale)))
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "CDP journey at {}%: {}{}",
                self.scale,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        println!("CDP_SCALE_JOURNEY_OK {}%", self.scale);
        self.navigate("/")
    }
    fn logical_size(&self) -> Result<(i32, i32)> {
        let f = self.display.conn.get_geometry(self.window)?.reply()?;
        Ok((
            (u32::from(f.width) * 100).div_ceil(u32::from(self.scale)) as i32,
            (u32::from(f.height) * 100).div_ceil(u32::from(self.scale)) as i32,
        ))
    }
    fn settings_geometry(&self) -> Result<(i32, i32, bool)> {
        let (w, h) = self.logical_size()?;
        let compact = h < 320;
        let panel = if compact { 232 } else { 304 };
        Ok((
            (w - (w - 40).min(600)) / 2,
            125.min((h - panel - 4).max(0)),
            compact,
        ))
    }
    fn settings(&self) -> Result<()> {
        let (width, _) = self.logical_size()?;
        self.click(width - 28, 31)?;
        let menu = wait("painted Menu", || {
            let f = self.frame()?;
            Ok(matches!(
                f.pixel(
                    physical(width - 222, self.scale) as usize,
                    physical(63, self.scale) as usize
                ),
                0x323c35 | 0xe3e9df
            ))
        });
        if menu.is_err() {
            self.frame()?
                .save(&self.log.with_extension("menu-failure.png"))?;
        }
        menu?;
        self.click(width - 140, 142)?;
        let (x, y, _) = self.settings_geometry()?;
        wait("painted Settings", || {
            let f = self.frame()?;
            Ok(matches!(
                f.pixel(
                    physical(x, self.scale) as usize,
                    physical(y, self.scale) as usize
                ),
                0x59675b | 0xc4cebd
            ))
        })
    }
    fn size_button(&self, offset: i32) -> Result<()> {
        let (x, y, compact) = self.settings_geometry()?;
        self.click(x + offset, y + if compact { 135 } else { 162 })
    }
    fn light(&self) -> Result<()> {
        let (w, _) = self.logical_size()?;
        let (x, y, compact) = self.settings_geometry()?;
        let bw = ((w - 40).min(600) - 48) / 3;
        self.click(
            x + 16 + (bw + 8) + bw / 2,
            y + if compact { 74 } else { 90 },
        )
    }
    fn close_settings(&self) -> Result<()> {
        let (x, y, compact) = self.settings_geometry()?;
        self.click(x + 50, y + if compact { 202 } else { 268 })
    }
    fn resize(&self, w: u32, h: u32) -> Result<()> {
        self.display.conn.configure_window(
            self.window,
            &ConfigureWindowAux::new().x(0).y(0).width(w).height(h),
        )?;
        self.display.conn.flush()?;
        wait("native resize", || {
            let g = self.display.conn.get_geometry(self.window)?.reply()?;
            Ok(u32::from(g.width) == w && u32::from(g.height) == h)
        })?;
        thread::sleep(Duration::from_millis(200));
        Ok(())
    }
}

fn saved(scratch: &Path, theme: &str, scale: serde_json::Value) -> Result<()> {
    wait("persisted theme and scale", || {
        let data = fs::read(scratch.join("config/mgbrowser/settings.json"))?;
        let value: serde_json::Value = serde_json::from_slice(&data)?;
        Ok(value["theme"] == theme && value["scale"] == scale)
    })
}

fn main() -> Result<()> {
    if std::env::var("MGBROWSER_SCALE_PRIVATE_DISPLAY").as_deref() != Ok("1") {
        return Err("Use tools/scale-smoke.sh on an owned Xvfb".into());
    }
    let args: Vec<_> = std::env::args_os().collect();
    let payload = PathBuf::from(args.get(1).ok_or("payload required")?);
    let scratch = PathBuf::from(args.get(2).ok_or("scratch required")?);
    fs::create_dir_all(scratch.join("config/mgbrowser"))?;
    // Verify migration from the shipped theme-only settings without losing Dark.
    fs::write(
        scratch.join("config/mgbrowser/settings.json"),
        b"{\"theme\":\"dark\"}\n",
    )?;
    let display = Display::new()?;
    display.dpi("192")?;
    let mut browser = Browser::start(&display, &payload, &scratch, "system-dpi")?;
    browser.wait_scale(200)?;
    let frame = browser.settled()?;
    frame.sharp_two_x_text((206, 22, 200, 20));
    frame.sharp_two_x_text((32, 132, 300, 30));
    frame.save(&scratch.join("native-200.png"))?;
    browser.journey()?;
    browser.cdp_journey(&scratch)?;
    display.dpi("120")?;
    browser.wait_scale(125)?;
    browser.settled()?.save(&scratch.join("native-125.png"))?;
    browser.journey()?;
    browser.cdp_journey(&scratch)?;
    display.dpi("192")?;
    browser.wait_scale(200)?;
    display.xsettings(Some(120))?;
    browser.wait_scale(125)?;
    display.xsettings(None)?;
    browser.wait_scale(200)?;
    display.dpi("120")?;
    browser.wait_scale(125)?;
    browser.key(u32::from(b'='), true)?;
    browser.wait_scale(150)?;
    saved(&scratch, "dark", 150.into())?;
    display.dpi("192")?;
    thread::sleep(Duration::from_millis(2300));
    browser.wait_scale(150)?;
    browser.settings()?;
    browser.size_button(265)?;
    browser.wait_scale(175)?;
    browser.size_button(130)?;
    browser.wait_scale(150)?;
    browser.light()?;
    saved(&scratch, "light", 150.into())?;
    browser.settled()?.save(&scratch.join("settings-150.png"))?;
    browser.close_settings()?;
    drop(browser);
    let mut browser = Browser::start(&display, &payload, &scratch, "manual-restart")?;
    browser.wait_scale(150)?;
    saved(&scratch, "light", 150.into())?;
    browser.key(u32::from(b'+'), true)?;
    browser.wait_scale(175)?;
    browser.key(u32::from(b'-'), true)?;
    browser.wait_scale(150)?;
    browser.key(u32::from(b'0'), true)?;
    browser.wait_scale(200)?;
    saved(&scratch, "light", "system".into())?;
    display.dpi("96")?;
    browser.wait_scale(100)?;
    browser.journey()?;
    browser.settled()?.save(&scratch.join("native-100.png"))?;
    browser.resize(500, 240)?;
    browser.smooth_wheel(&scratch)?;
    browser.resize(1100, 820)?;
    // A compact viewport requires actual wheel input to reveal the original form.
    display.dpi("192")?;
    browser.wait_scale(200)?;
    browser.resize(1000, 480)?;
    browser.wait_scale(200)?;
    browser.smooth_wheel(&scratch)?;
    let before = browser.settled()?.bytes;
    browser.button(800, 350, 5)?;
    wait("wheel scroll repaint", || {
        Ok(before != browser.frame()?.bytes)
    })?;
    browser.settings()?;
    browser
        .settled()?
        .save(&scratch.join("compact-settings-200.png"))?;
    browser.close_settings()?;
    browser.resize(3840, 2160)?;
    browser.wait_scale(200)?;
    let frame = browser.settled()?;
    assert_eq!((frame.width, frame.height), (3840, 2160));
    frame.save(&scratch.join("native-4k-200.png"))?;
    browser.navigate("/")?;
    browser.journey()?;
    // Select manual fractional size, then show Settings can restore System.
    for _ in 0..3 {
        browser.key(u32::from(b'-'), true)?;
        thread::sleep(Duration::from_millis(80));
    }
    browser.wait_scale(125)?;
    saved(&scratch, "light", 125.into())?;
    drop(browser);
    let mut browser = Browser::start(&display, &payload, &scratch, "fractional-restart")?;
    browser.wait_scale(125)?;
    saved(&scratch, "light", 125.into())?;
    browser.settings()?;
    browser.size_button(55)?;
    browser.wait_scale(200)?;
    saved(&scratch, "light", "system".into())?;
    browser.close_settings()?;
    display.dpi("invalid")?;
    browser.wait_scale(100)?;
    browser.key(u32::from(b'-'), true)?;
    browser.wait_scale(75)?;
    browser.key(u32::from(b'-'), true)?;
    browser.wait_scale(75)?;
    for percent in [100, 125, 150, 175, 200, 250, 300] {
        browser.key(u32::from(b'='), true)?;
        browser.wait_scale(percent)?;
    }
    browser.key(u32::from(b'='), true)?;
    browser.wait_scale(300)?;
    saved(&scratch, "light", 300.into())?;
    browser.key(u32::from(b'0'), true)?;
    browser.wait_scale(100)?;
    saved(&scratch, "light", "system".into())?;
    // Reuse the owned display, native input and unchanged local page fixtures.
    // This runs against the packaged/public-installed executable, not cargo run.
    browser.resize(1100, 820)?;
    browser.navigate("/")?;
    browser.key(b'l' as u32, true)?;
    let selected = browser.settled()?;
    assert_eq!(
        selected.pixel(700, 21),
        0xffffff,
        "selection filled blank address space"
    );
    assert_ne!(selected.pixel(205, 21), 0xffffff, "URL selection missing");
    browser.text("https://not-submitted.example/")?;
    browser.click(160, 31)?;
    let bookmarks_path = scratch.join("config/mgbrowser/bookmarks.json");
    wait("bookmark current page", || {
        let Ok(bytes) = fs::read(&bookmarks_path) else {
            return Ok(false);
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        Ok(value["entries"].as_array().unwrap().len() == 1
            && value["entries"][0]["url"] == format!("{BASE}/"))
    })?;
    // Refresh must ignore the edited address, then navigation/history must work.
    let count = browser.loaded_count()?;
    browser.click(116, 31)?;
    browser.loaded_after(count, "Local browser journey fixture")?;
    browser.navigate("/destination")?;
    let count = browser.loaded_count()?;
    browser.click(28, 31)?;
    browser.loaded_after(count, "Local browser journey fixture")?;
    let count = browser.loaded_count()?;
    browser.click(72, 31)?;
    browser.loaded_after(count, "/destination")?;
    drop(browser);
    let mut browser = Browser::start(&display, &payload, &scratch, "bookmark-restart")?;
    browser.wait_scale(100)?;
    browser.navigate("/destination")?;
    browser.click(browser.logical_size()?.0 - 28, 31)?;
    browser.settled()?;
    browser.click(browser.logical_size()?.0 - 140, 176)?;
    wait("bookmark dialog", || {
        Ok(browser.frame()?.pixel(170, 140) == 0xc4cebd)
    })?;
    browser.settled()?.save(&scratch.join("bookmarks.png"))?;
    let count = browser.loaded_count()?;
    browser.click(230, 218)?;
    browser.loaded_after(count, "Local browser journey fixture")?;
    browser.click(browser.logical_size()?.0 - 28, 31)?;
    browser.settled()?;
    browser.click(browser.logical_size()?.0 - 140, 176)?;
    wait("bookmark dialog reopened", || {
        Ok(browser.frame()?.pixel(170, 140) == 0xc4cebd)
    })?;
    browser.click(878, 218)?;
    wait("bookmark removed", || {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&bookmarks_path)?)?;
        Ok(value["entries"].as_array().unwrap().is_empty())
    })?;
    browser.key(0xff1b, false)?;
    browser.key(b'd' as u32, true)?;
    wait("Ctrl+D saves bookmark", || {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&bookmarks_path)?)?;
        Ok(value["entries"].as_array().unwrap().len() == 1)
    })?;
    browser.key(b'd' as u32, true)?;
    wait("Ctrl+D removes bookmark", || {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&bookmarks_path)?)?;
        Ok(value["entries"].as_array().unwrap().is_empty())
    })?;
    println!("NATIVE_BOOKMARK_TOOLBAR_SMOKE_OK {}", scratch.display());
    println!("NATIVE_SCALE_SMOKE_OK {}", scratch.display());
    Ok(())
}
