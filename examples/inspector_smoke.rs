//! Independent native Inspector input against an owned, ephemeral HTTP fixture.
//! Run through tools/inspector-smoke.sh, never on a user's display/profile.
use serde_json::{Value, json};
use std::{
    error::Error,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};
use x11rb::{connection::Connection, protocol::xproto::*, rust_connection::RustConnection};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const HOME: &str = "<!doctype html><title>Native inspector fixture</title><style>body{margin:12px} #chosen{display:flex;position:relative}</style><link rel='stylesheet' href='/missing.css'><p id='chosen' class='inspection-proof'>INSPECTED ELEMENT: local authored text</p><form action='/search'><label>Query <input name='q'></label><button name='submit' value='go'>Submit</button></form><script>throw new Error('inspector-script-marker');</script><p>Fixture only, not public-site compatibility.</p>";

struct Fixture {
    base: String,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let base = format!("http://{}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (done, received) = (stop.clone(), requests.clone());
        let thread = thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut line = String::new();
                        if BufReader::new(&stream).read_line(&mut line).is_err() {
                            continue;
                        }
                        let target = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                        received.lock().unwrap().push(target.clone());
                        let (status, content_type, body) = if target == "/missing.css" {
                            (
                                "404 Not Found",
                                "text/css",
                                "/* Deliberately missing fixture CSS */",
                            )
                        } else if target.starts_with("/search?") {
                            (
                                "200 OK",
                                "text/html",
                                "<title>Inspector form arrived</title><h1 id='arrived'>NATIVE FORM: inspector closed safely</h1>",
                            )
                        } else if target == "/replacement" {
                            (
                                "200 OK",
                                "text/html",
                                "<title>Inspector replacement</title><p id='replacement'>REPLACEMENT DOCUMENT: old selection is invalid</p>",
                            )
                        } else {
                            ("200 OK", "text/html", HOME)
                        };
                        let _ = write!(
                            stream,
                            "HTTP/1.1 {status}\r\nContent-Type: {content_type}; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            base,
            stop,
            requests,
            thread: Some(thread),
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn wait(label: &str, mut ready: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        if ready()? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(60));
    }
    Err(format!("Timed out: {label}").into())
}

struct Frame {
    width: u16,
    height: u16,
    bytes: Vec<u8>,
}
impl Frame {
    fn pixel(&self, x: usize, y: usize) -> u32 {
        let index = (y * usize::from(self.width) + x) * 4;
        u32::from_le_bytes(self.bytes[index..index + 4].try_into().unwrap()) & 0xffffff
    }
    fn save(&self, path: &Path) -> Result<()> {
        let rgb = self
            .bytes
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[2], pixel[1], pixel[0]])
            .collect();
        image::RgbImage::from_raw(self.width.into(), self.height.into(), rgb)
            .ok_or("Bad native frame")?
            .save(path)?;
        Ok(())
    }
}

struct Browser {
    child: Child,
    conn: RustConnection,
    root: u32,
    window: u32,
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next: u64,
    scale: i32,
    surface_top: i32,
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Browser {
    fn start(payload: &Path, scratch: &Path, url: &str, scale: i32, theme: &str) -> Result<Self> {
        fs::create_dir_all(scratch.join("config/mgbrowser"))?;
        fs::write(
            scratch.join("config/mgbrowser/settings.json"),
            format!("{{\"theme\":\"{theme}\",\"scale\":{scale}}}\n"),
        )?;
        let path = scratch.join("browser.log");
        let log = File::create(&path)?;
        let mut child = Command::new(payload)
            .args([
                url,
                "--no-auto-update",
                "--enable-scripts",
                "--remote-debugging-port=0",
            ])
            .env("XDG_CONFIG_HOME", scratch.join("config"))
            .env("XDG_DATA_HOME", scratch.join("data"))
            .env("XDG_CACHE_HOME", scratch.join("cache"))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}/absent-bus", scratch.display()),
            )
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        let mut window = 0;
        let mut endpoint = String::new();
        let ready = wait("native fixture and CDP", || {
            if let Some(status) = child.try_wait()? {
                return Err(format!("Browser exited {status}: {}", path.display()).into());
            }
            let log = fs::read_to_string(&path)?;
            window = log
                .lines()
                .find_map(|line| {
                    line.strip_prefix("WINDOW id=")?
                        .split_whitespace()
                        .next()?
                        .parse()
                        .ok()
                })
                .unwrap_or(0);
            endpoint = log
                .lines()
                .find_map(|line| {
                    line.strip_prefix("CDP listening on ")?
                        .split_whitespace()
                        .next()
                })
                .unwrap_or("")
                .to_owned();
            Ok(window != 0 && !endpoint.is_empty() && log.contains("LOADED "))
        });
        if let Err(error) = ready {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let endpoint = endpoint.replace("/devtools/browser/browser-1", "/devtools/page/page-1");
        let controls = (|| -> Result<_> {
            let (socket, _) = tungstenite::connect(endpoint.as_str())?;
            if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
                stream.set_read_timeout(Some(Duration::from_secs(12)))?;
            }
            let (conn, screen) = x11rb::connect(None)?;
            let root = conn.setup().roots[screen].root;
            Ok((conn, root, socket))
        })();
        let (conn, root, socket) = match controls {
            Ok(controls) => controls,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let browser = Self {
            child,
            conn,
            root,
            window,
            socket,
            next: 0,
            scale,
            surface_top: 0,
        };
        browser.resize(800, 600)?;
        browser.settled()?;
        Ok(browser)
    }
    fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        self.socket.send(Message::Text(
            json!({"id":self.next,"method":method,"params":params})
                .to_string()
                .into(),
        ))?;
        loop {
            let message = self.socket.read()?;
            let Message::Text(text) = message else {
                continue;
            };
            let response: Value = serde_json::from_str(&text)?;
            if response["id"] != self.next {
                continue;
            }
            if response.get("error").is_some() {
                return Err(format!("{method}: {}", response["error"]).into());
            }
            return Ok(response["result"].clone());
        }
    }
    fn locate(&mut self, selector: &str) -> Result<(i32, i32)> {
        let document = self.rpc("DOM.getDocument", json!({}))?;
        let selected = self.rpc(
            "DOM.querySelector",
            json!({"nodeId":document["root"]["nodeId"],"selector":selector}),
        )?;
        if selected["nodeId"] == 0 {
            return Err(format!("Missing fixture node {selector}").into());
        }
        let box_model = self.rpc("DOM.getBoxModel", json!({"nodeId":selected["nodeId"]}))?;
        let quad = &box_model["model"]["content"];
        eprintln!(
            "INSPECTOR_LOCATE {selector} {quad} window={} scale={}",
            self.window, self.scale
        );
        Ok((
            quad[0].as_i64().ok_or("Missing x")? as i32 + 4,
            quad[1].as_i64().ok_or("Missing y")? as i32 + 64 + 4,
        ))
    }
    fn physical(&self, logical: i32) -> i16 {
        ((logical * self.scale + 50) / 100) as i16
    }
    fn button(&self, x: i32, y: i32, detail: u8) -> Result<()> {
        let (x, y) = (self.physical(x), self.physical(y + self.surface_top));
        for (response_type, mask) in [
            (BUTTON_PRESS_EVENT, EventMask::BUTTON_PRESS),
            (BUTTON_RELEASE_EVENT, EventMask::BUTTON_RELEASE),
        ] {
            self.conn.send_event(
                false,
                self.window,
                mask,
                ButtonPressEvent {
                    response_type,
                    detail,
                    sequence: 0,
                    time: 0,
                    root: self.root,
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
        self.conn.flush()?;
        thread::sleep(Duration::from_millis(100));
        Ok(())
    }
    fn key(&self, symbol: u32, ctrl: bool) -> Result<()> {
        let first = self.conn.setup().min_keycode;
        let map = self
            .conn
            .get_keyboard_mapping(first, self.conn.setup().max_keycode - first + 1)?
            .reply()?;
        let (code, shift) = map
            .keysyms
            .chunks(usize::from(map.keysyms_per_keycode))
            .enumerate()
            .find_map(|(i, group)| {
                group
                    .iter()
                    .take(2)
                    .position(|&s| s == symbol)
                    .map(|column| (first + i as u8, column == 1))
            })
            .ok_or("Keysym unavailable")?;
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
            self.conn.send_event(
                false,
                self.window,
                mask,
                KeyPressEvent {
                    response_type,
                    detail: code,
                    sequence: 0,
                    time: 0,
                    root: self.root,
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
        self.conn.flush()?;
        thread::sleep(Duration::from_millis(80));
        Ok(())
    }
    fn resize(&self, width: u32, height: u32) -> Result<()> {
        self.conn.configure_window(
            self.window,
            &ConfigureWindowAux::new()
                .width(width * self.scale as u32 / 100)
                .height((height + self.surface_top as u32) * self.scale as u32 / 100),
        )?;
        self.conn.flush()?;
        thread::sleep(Duration::from_millis(200));
        Ok(())
    }
    fn frame(&self) -> Result<Frame> {
        let geometry = self.conn.get_geometry(self.window)?.reply()?;
        let bytes = self
            .conn
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
        if bytes.len() != usize::from(geometry.width) * usize::from(geometry.height) * 4 {
            return Err("Expected 32-bit Xvfb surface".into());
        }
        Ok(Frame {
            width: geometry.width,
            height: geometry.height,
            bytes,
        })
    }
    fn settled(&self) -> Result<Frame> {
        let mut previous = self.frame()?;
        let mut equal = 0;
        wait("settled Inspector paint", || {
            let next = self.frame()?;
            equal = if previous.bytes == next.bytes {
                equal + 1
            } else {
                0
            };
            previous = next;
            Ok(equal >= 3)
        })?;
        Ok(previous)
    }
    fn expect_panel(&self, theme: &str) -> Result<Frame> {
        wait("Inspector panel pixels", || {
            let frame = self.frame()?;
            let logical_width = i32::from(frame.width) * 100 / self.scale;
            let panel_x = (logical_width - (logical_width - 16).min(860)) / 2;
            let panel = frame.pixel(
                self.physical(panel_x + 6) as usize,
                self.physical(self.surface_top + 20) as usize,
            );
            let control = frame.pixel(
                self.physical(panel_x + 14) as usize,
                self.physical(self.surface_top + 50) as usize,
            );
            Ok(panel == if theme == "dark" { 0x252e28 } else { 0xfafbf8 }
                && control == if theme == "dark" { 0x323c35 } else { 0xe3e9df })
        })?;
        self.settled()
    }
    fn expect_context(&self, x: i32, y: i32, theme: &str) -> Result<Frame> {
        wait("Inspector context menu pixels", || {
            Ok(self.frame()?.pixel(
                self.physical(x.max(6) + 8) as usize,
                self.physical(y + self.surface_top + 8) as usize,
            ) == if theme == "dark" { 0x323c35 } else { 0xe3e9df })
        })?;
        self.settled()
    }
    fn navigate(&mut self, url: &str) -> Result<()> {
        let result = self.rpc("Page.navigate", json!({"url":url}))?;
        assert!(
            result.get("errorText").is_none(),
            "Navigation failed: {result}"
        );
        self.settled()?;
        Ok(())
    }
}

fn exercise(
    payload: &Path,
    scratch: &Path,
    fixture: &Fixture,
    scale: i32,
    theme: &str,
) -> Result<()> {
    let dir = scratch.join(format!("{theme}-{scale}"));
    fs::create_dir_all(&dir)?;
    let mut browser = Browser::start(payload, &dir, &format!("{}/", fixture.base), scale, theme)?;
    let (x, y) = browser.locate("#chosen")?;
    browser.button(x, y, 3)?;
    browser
        .expect_context(x, y, theme)?
        .save(&dir.join("context-menu.png"))?;
    browser.button(x + 70, y + 21, 1)?;
    let elements = browser.expect_panel(theme)?;
    elements.save(&dir.join("elements.png"))?;
    let requests = fixture.requests.lock().unwrap().len();
    let scroll_before =
        browser.rpc("Page.getLayoutMetrics", json!({}))?["cssLayoutViewport"]["pageY"].clone();
    browser.key(b'r' as u32, true)?;
    browser.key(b'x' as u32, false)?;
    browser.key(0xff0d, false)?;
    browser.button(760, 500, 5)?;
    assert_eq!(
        fixture.requests.lock().unwrap().len(),
        requests,
        "Inspector input leaked into page navigation"
    );
    assert_eq!(
        browser.rpc("Page.getLayoutMetrics", json!({}))?["cssLayoutViewport"]["pageY"],
        scroll_before,
        "Inspector wheel scrolled page"
    );
    // Elements has focus initially; the first Tab selects Diagnostics.
    browser.key(0xff09, false)?;
    browser.key(0xff0d, false)?;
    wait("Diagnostics selected underline", || {
        Ok(browser.frame()?.pixel(
            browser.physical(280) as usize,
            browser.physical(79 + browser.surface_top) as usize,
        ) == if theme == "dark" { 0xa7dba6 } else { 0x345c36 })
    })?;
    let diagnostics = browser.expect_panel(theme)?;
    assert!(
        elements.bytes != diagnostics.bytes,
        "Diagnostics tab did not change"
    );
    diagnostics.save(&dir.join("diagnostics.png"))?;
    browser.key(0xff56, false)?;
    browser.settled()?.save(&dir.join("diagnostics-next.png"))?;
    browser.resize(400, 240)?;
    browser
        .expect_panel(theme)?
        .save(&dir.join("compact-diagnostics.png"))?;
    browser.key(0xff1b, false)?;
    browser.resize(800, 600)?;
    let before_form = fixture.requests.lock().unwrap().len();
    let (x, y) = browser.locate("input[name=q]")?;
    browser.button(x, y, 1)?;
    for ch in "inspect".chars() {
        browser.key(ch as u32, false)?;
    }
    browser.key(0xff0d, false)?;
    wait("native form after Inspector close", || {
        Ok(fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .skip(before_form)
            .any(|target| target.starts_with("/search?") && target.contains("q=inspect")))
    })?;
    browser.settled()?.save(&dir.join("form-arrived.png"))?;
    browser.navigate(&format!("{}/", fixture.base))?;
    let (x, y) = browser.locate("#chosen")?;
    browser.button(x, y, 3)?;
    browser.button(x + 70, y + 21, 1)?;
    browser.expect_panel(theme)?;
    browser.navigate(&format!("{}/replacement", fixture.base))?;
    // Navigation must dismiss the old inspector and old selection. F12 now
    // opens the replacement document root, not the previously selected node.
    let closed = browser.settled()?;
    assert_eq!(
        closed.pixel(0, browser.physical(browser.surface_top) as usize),
        0x9f202b
    );
    browser.key(0xffc9, false)?;
    browser
        .expect_panel(theme)?
        .save(&dir.join("replacement-elements.png"))?;
    browser.key(0xffc9, false)?;
    browser.key(b'I' as u32, true)?;
    browser.expect_panel(theme)?;
    browser.key(0xff1b, false)?;
    println!("NATIVE_INSPECTOR_OK {theme} {scale}% {}", dir.display());
    Ok(())
}

fn main() -> Result<()> {
    if std::env::var("MGBROWSER_INSPECTOR_PRIVATE_DISPLAY").as_deref() != Ok("1") {
        return Err("Run via tools/inspector-smoke.sh on its owned display".into());
    }
    let mut args = std::env::args_os().skip(1);
    let payload = PathBuf::from(args.next().ok_or("Expected packaged executable")?);
    let scratch = PathBuf::from(args.next().ok_or("Expected scratch directory")?);
    let fixture = Fixture::start()?;
    for scale in [100, 200] {
        for theme in ["light", "dark"] {
            exercise(&payload, &scratch, &fixture, scale, theme)?;
        }
    }
    fs::write(
        scratch.join("requests.txt"),
        fixture.requests.lock().unwrap().join("\n"),
    )?;
    Ok(())
}
