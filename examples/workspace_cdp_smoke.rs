//! Native tab lifecycle plus the public CDP transport against owned fixtures.
//! Run only through tools/workspace-cdp-smoke.sh on its private display/profile.
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
const FIRST: u32 = 0xffefd5;
const SECOND: u32 = 0xd9edff;

struct Fixture {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let base = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (received, finished) = (requests.clone(), stop.clone());
        let thread = thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut line = String::new();
                        if BufReader::new(&stream).read_line(&mut line).is_err() {
                            continue;
                        }
                        let target = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                        let mut records = received.lock().unwrap();
                        if records.len() < 64 {
                            records.push(target.clone());
                        }
                        drop(records);
                        let name = if target.starts_with("/second") {
                            "second"
                        } else {
                            "first"
                        };
                        let color = if name == "second" { SECOND } else { FIRST };
                        let body = format!(
                            "<!doctype html><html><head><title>CDP {name} fixture</title><style>body{{background:#{color:06x};margin:12px;font-size:16px}}</style></head><body><h1 id='identity'>OWNED {name} TARGET</h1><form action='/{name}/submit'><input id='query' name='q'><button>Submit retained edit</button></form><p>Native workspace/CDP fixture only, not a public-site or Playwright claim.</p></body></html>"
                        );
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
            requests,
            stop,
            thread: Some(thread),
        })
    }
    fn saw(&self, target: &str) -> bool {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry == target)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn wait(label: &str, mut ready: impl FnMut() -> Result<bool>) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(12);
    while Instant::now() < until {
        if ready()? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(60));
    }
    Err(format!("Timed out: {label}").into())
}

struct Cdp {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next: u64,
    events: Vec<Value>,
}
impl Cdp {
    fn connect(endpoint: &str) -> Result<Self> {
        let (socket, _) = tungstenite::connect(endpoint)?;
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream.set_read_timeout(Some(Duration::from_secs(8)))?;
        }
        Ok(Self {
            socket,
            next: 0,
            events: Vec::new(),
        })
    }
    fn raw(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.next += 1;
        let mut request = json!({"id":self.next,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::Text(request.to_string().into()))?;
        let until = Instant::now() + Duration::from_secs(12);
        loop {
            if Instant::now() >= until {
                return Err(format!("CDP reply timeout: {method}").into());
            }
            let Message::Text(text) = self.socket.read()? else {
                continue;
            };
            let response: Value = serde_json::from_str(&text)?;
            if response["id"] == self.next {
                assert_eq!(response.get("sessionId").and_then(Value::as_str), session);
                return Ok(response);
            }
            if response.get("method").is_some() {
                if self.events.len() == 64 {
                    self.events.remove(0);
                }
                self.events.push(response);
            }
        }
    }
    fn rpc(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        let response = self.raw(method, params, session)?;
        if response.get("error").is_some() {
            return Err(format!("{method}: {}", response["error"]).into());
        }
        Ok(response["result"].clone())
    }
    fn targets(&mut self) -> Result<Vec<Value>> {
        Ok(
            self.rpc("Target.getTargets", json!({}), None)?["targetInfos"]
                .as_array()
                .ok_or("Missing target list")?
                .clone(),
        )
    }
    fn attach(&mut self, target: &str) -> Result<String> {
        Ok(self.rpc(
            "Target.attachToTarget",
            json!({"targetId":target,"flatten":true}),
            None,
        )?["sessionId"]
            .as_str()
            .ok_or("Missing session")?
            .into())
    }
    fn navigate(&mut self, session: &str, url: &str) -> Result<()> {
        let reply = self.rpc("Page.navigate", json!({"url":url}), Some(session))?;
        if reply.get("errorText").is_some() {
            return Err(format!("Navigation failed: {reply}").into());
        }
        Ok(())
    }
    fn input(&mut self, session: &str) -> Result<(Value, Value)> {
        let root = self.rpc("DOM.getDocument", json!({}), Some(session))?["root"]["nodeId"].clone();
        let node = self.rpc(
            "DOM.querySelector",
            json!({"nodeId":root,"selector":"#query"}),
            Some(session),
        )?["nodeId"]
            .clone();
        assert_ne!(node, 0);
        self.rpc("DOM.focus", json!({"nodeId":node}), Some(session))?;
        Ok((root, node))
    }
    fn insert(&mut self, session: &str, text: &str) -> Result<()> {
        self.rpc("Input.insertText", json!({"text":text}), Some(session))?;
        Ok(())
    }
    fn submit(&mut self, session: &str) -> Result<()> {
        self.rpc(
            "Input.dispatchKeyEvent",
            json!({"type":"keyDown","key":"Enter","windowsVirtualKeyCode":13}),
            Some(session),
        )?;
        Ok(())
    }
    fn expect_closed(&mut self) -> Result<()> {
        if let MaybeTlsStream::Plain(stream) = self.socket.get_ref() {
            stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        }
        wait("closed direct target socket", || match self.socket.read() {
            Ok(Message::Close(_))
            | Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                Ok(true)
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(false)
            }
            Err(_) => Ok(true),
            Ok(_) => Ok(false),
        })
    }
}

struct Native {
    child: Child,
    conn: RustConnection,
    root: u32,
    log: PathBuf,
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Native {
    fn start(payload: &Path, scratch: &Path, url: &str) -> Result<Self> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        for directory in ["home", "config/mgbrowser", "data", "cache", "runtime"] {
            fs::create_dir_all(scratch.join(directory))?;
        }
        fs::write(
            scratch.join("config/mgbrowser/settings.json"),
            "{\"theme\":\"light\",\"scale\":100}\n",
        )?;
        let log = scratch.join("browser.log");
        let output = File::create(&log)?;
        let child = Command::new(payload)
            .args([url, "--no-auto-update", "--remote-debugging-port=0"])
            .env("HOME", scratch.join("home"))
            .env("XDG_CONFIG_HOME", scratch.join("config"))
            .env("XDG_DATA_HOME", scratch.join("data"))
            .env("XDG_CACHE_HOME", scratch.join("cache"))
            .env("XDG_RUNTIME_DIR", scratch.join("runtime"))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}/absent-bus", scratch.display()),
            )
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output)
            .spawn()?;
        let mut native = Self {
            child,
            conn,
            root,
            log,
        };
        wait("owned browser startup", || {
            if let Some(status) = native.child.try_wait()? {
                return Err(format!("Browser exited {status}").into());
            }
            Ok(
                native.windows()?.len() == 1
                    && fs::read_to_string(&native.log)?.contains("LOADED "),
            )
        })?;
        Ok(native)
    }
    fn endpoint(&self) -> Result<String> {
        fs::read_to_string(&self.log)?
            .lines()
            .find_map(|line| {
                line.strip_prefix("CDP listening on ")?
                    .split_whitespace()
                    .next()
                    .map(str::to_owned)
            })
            .ok_or_else(|| "CDP endpoint missing".into())
    }
    fn windows(&self) -> Result<Vec<u32>> {
        let mut windows = Vec::new();
        for window in self.conn.query_tree(self.root)?.reply()?.children {
            let Ok(attributes) = self.conn.get_window_attributes(window)?.reply() else {
                continue;
            };
            if attributes.map_state != MapState::VIEWABLE {
                continue;
            }
            let class = self
                .conn
                .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)?
                .reply()?;
            if class
                .value
                .split(|byte| *byte == 0)
                .any(|part| part == b"mgbrowser")
            {
                windows.push(window);
            }
        }
        windows.sort_unstable();
        Ok(windows)
    }
    fn place(&self, window: u32, x: i32) -> Result<()> {
        self.conn.configure_window(
            window,
            &ConfigureWindowAux::new().x(x).y(40).width(800).height(634),
        )?;
        self.conn.flush()?;
        Ok(())
    }
    fn key(&self, window: u32, symbol: u32, ctrl: bool) -> Result<()> {
        let first = self.conn.setup().min_keycode;
        let mapping = self
            .conn
            .get_keyboard_mapping(first, self.conn.setup().max_keycode - first + 1)?
            .reply()?;
        let (code, shift) = mapping
            .keysyms
            .chunks(usize::from(mapping.keysyms_per_keycode))
            .enumerate()
            .find_map(|(index, group)| {
                group
                    .iter()
                    .take(2)
                    .position(|&key| key == symbol)
                    .map(|column| (first + index as u8, column == 1))
            })
            .ok_or("Keysym unavailable")?;
        let mut state = KeyButMask::default();
        if ctrl {
            state |= KeyButMask::CONTROL;
        }
        if shift {
            state |= KeyButMask::SHIFT;
        }
        self.conn
            .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)?;
        for (response_type, mask) in [
            (KEY_PRESS_EVENT, EventMask::KEY_PRESS),
            (KEY_RELEASE_EVENT, EventMask::KEY_RELEASE),
        ] {
            self.conn.send_event(
                false,
                window,
                mask,
                KeyPressEvent {
                    response_type,
                    detail: code,
                    sequence: 0,
                    time: 0,
                    root: self.root,
                    event: window,
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
        Ok(())
    }
    fn button(&self, window: u32, point: (i16, i16), pressed: bool) -> Result<()> {
        let local = self
            .conn
            .translate_coordinates(self.root, window, point.0, point.1)?
            .reply()?;
        self.conn
            .warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, point.0, point.1)?;
        self.conn.send_event(
            false,
            window,
            if pressed {
                EventMask::BUTTON_PRESS
            } else {
                EventMask::BUTTON_RELEASE
            },
            ButtonPressEvent {
                response_type: if pressed {
                    BUTTON_PRESS_EVENT
                } else {
                    BUTTON_RELEASE_EVENT
                },
                detail: 1,
                sequence: 0,
                time: 0,
                root: self.root,
                event: window,
                child: 0,
                root_x: point.0,
                root_y: point.1,
                event_x: local.dst_x,
                event_y: local.dst_y,
                state: if pressed {
                    KeyButMask::default()
                } else {
                    KeyButMask::BUTTON1
                },
                same_screen: true,
            },
        )?;
        self.conn.flush()?;
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }
    fn motion(&self, window: u32, point: (i16, i16)) -> Result<()> {
        let local = self
            .conn
            .translate_coordinates(self.root, window, point.0, point.1)?
            .reply()?;
        self.conn
            .warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, point.0, point.1)?;
        self.conn.send_event(
            false,
            window,
            EventMask::POINTER_MOTION,
            MotionNotifyEvent {
                response_type: MOTION_NOTIFY_EVENT,
                detail: Motion::NORMAL,
                sequence: 0,
                time: 0,
                root: self.root,
                event: window,
                child: 0,
                root_x: point.0,
                root_y: point.1,
                event_x: local.dst_x,
                event_y: local.dst_y,
                state: KeyButMask::BUTTON1,
                same_screen: true,
            },
        )?;
        self.conn.flush()?;
        thread::sleep(Duration::from_millis(80));
        Ok(())
    }
    fn detach_second(&self, window: u32) -> Result<()> {
        // At width800, both tab labels occupy200 logical pixels. Stay clear of X.
        let from = self
            .conn
            .translate_coordinates(window, self.root, 250, 16)?
            .reply()?;
        self.button(window, (from.dst_x, from.dst_y), true)?;
        self.motion(window, (from.dst_x + 20, from.dst_y))?;
        self.motion(window, (950, 180))?;
        self.button(window, (950, 180), false)
    }
    fn capture(&self, window: u32) -> Result<(u16, u16, Vec<u8>)> {
        let geometry = self.conn.get_geometry(window)?.reply()?;
        let bytes = self
            .conn
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
            .data;
        if bytes.len() != usize::from(geometry.width) * usize::from(geometry.height) * 4 {
            return Err("Expected32-bit Xvfb image".into());
        }
        Ok((geometry.width, geometry.height, bytes))
    }
    fn frame(&self, window: u32, color: u32, path: &Path) -> Result<()> {
        // Require the requested page color plus its real navigation button,
        // not merely identical stale/empty pixels while a paint is pending.
        wait("native target pixels", || {
            let (width, height, bytes) = self.capture(window)?;
            if width != 800 || height != 634 {
                return Ok(false);
            }
            let pixel = |x: usize, y: usize| {
                let start = (y * usize::from(width) + x) * 4;
                u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()) & 0xffffff
            };
            Ok(pixel(80, 300) == color && pixel(12, 50) == 0xe3e9df)
        })?;
        let (width, height, bytes) = self.capture(window)?;
        let rgb = bytes
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        image::RgbImage::from_raw(width.into(), height.into(), rgb)
            .ok_or("Invalid native image")?
            .save(path)?;
        Ok(())
    }
}

fn ids(targets: &[Value]) -> Vec<String> {
    let mut values = targets
        .iter()
        .map(|target| target["targetId"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn run(payload: &Path, scratch: &Path) -> Result<()> {
    let fixture = Fixture::start()?;
    let mut native = Native::start(payload, scratch, &format!("{}/start", fixture.base))?;
    let original = native.windows()?[0];
    native.place(original, 40)?;
    let endpoint = native.endpoint()?;
    let mut cdp = Cdp::connect(&endpoint)?;
    assert_eq!(ids(&cdp.targets()?), ["page-1"]);
    native.key(original, b't'.into(), true)?;
    let mut targets = Vec::new();
    wait("Ctrl+T creates a real second CDP target", || {
        targets = cdp.targets()?;
        Ok(targets.len() == 2)
    })?;
    let original_ids = ids(&targets);
    let second_id = original_ids
        .iter()
        .find(|id| id.as_str() != "page-1")
        .unwrap()
        .clone();
    let first_session = cdp.attach("page-1")?;
    let second_session = cdp.attach(&second_id)?;
    cdp.navigate(&first_session, &format!("{}/first", fixture.base))?;
    cdp.navigate(&second_session, &format!("{}/second", fixture.base))?;
    let named = cdp.targets()?;
    assert!(
        named
            .iter()
            .any(|t| t["targetId"] == "page-1" && t["title"] == "CDP first fixture")
    );
    assert!(
        named
            .iter()
            .any(|t| t["targetId"] == second_id && t["title"] == "CDP second fixture")
    );
    let (first_root, first_input) = cdp.input(&first_session)?;
    cdp.insert(&first_session, "alpha")?;
    let (second_root, second_input) = cdp.input(&second_session)?;
    cdp.insert(&second_session, "bravo")?;
    let direct_endpoint = endpoint.replace(
        "/devtools/browser/browser-1",
        &format!("/devtools/page/{second_id}"),
    );
    let mut direct = Cdp::connect(&direct_endpoint)?;
    assert_eq!(
        direct.rpc("DOM.getDocument", json!({}), None)?["root"]["nodeId"],
        second_root
    );
    native.frame(original, SECOND, &scratch.join("two-tabs.png"))?;
    native.detach_second(original)?;
    wait(
        "native detached window",
        || Ok(native.windows()?.len() == 2),
    )?;
    let detached = native
        .windows()?
        .into_iter()
        .find(|&id| id != original)
        .ok_or("Detached window missing")?;
    native.place(detached, 950)?;
    assert_eq!(ids(&cdp.targets()?), original_ids);
    assert_eq!(
        cdp.rpc("DOM.getDocument", json!({}), Some(&first_session))?["root"]["nodeId"],
        first_root
    );
    assert_eq!(
        cdp.rpc("DOM.getDocument", json!({}), Some(&second_session))?["root"]["nodeId"],
        second_root
    );
    assert_eq!(
        direct.rpc("DOM.getDocument", json!({}), None)?["root"]["nodeId"],
        second_root
    );
    native.frame(original, FIRST, &scratch.join("first-survives-detach.png"))?;
    native.frame(detached, SECOND, &scratch.join("second-detached.png"))?;
    // DOM attributes are not live edited values. Prove the retained input by
    // submitting it through real CDP Input and observing the fixture request.
    cdp.rpc(
        "DOM.focus",
        json!({"nodeId":second_input}),
        Some(&second_session),
    )?;
    cdp.insert(&second_session, "-moved")?;
    cdp.submit(&second_session)?;
    wait("moved target retained its input", || {
        Ok(fixture.saw("/second/submit?q=bravo-moved"))
    })?;
    cdp.rpc(
        "DOM.focus",
        json!({"nodeId":first_input}),
        Some(&first_session),
    )?;
    cdp.submit(&first_session)?;
    wait("other target retained its separate input", || {
        Ok(fixture.saw("/first/submit?q=alpha"))
    })?;
    native.key(detached, b'w'.into(), true)?;
    wait("native closing target removes only its window", || {
        Ok(native.windows()?.len() == 1)
    })?;
    wait("closed target leaves discovery", || {
        Ok(ids(&cdp.targets()?) == ["page-1"])
    })?;
    assert!(
        cdp.events
            .iter()
            .any(|event| event["method"] == "Target.detachedFromTarget"
                && event["params"]["sessionId"] == second_session)
    );
    direct.expect_closed()?;
    let stale = cdp.raw("Page.getFrameTree", json!({}), Some(&second_session))?;
    assert_eq!(stale["error"]["code"], -32000);
    let closed = cdp.raw(
        "Target.attachToTarget",
        json!({"targetId":second_id,"flatten":true}),
        None,
    )?;
    assert_eq!(closed["error"]["code"], -32000);
    match tungstenite::connect(&direct_endpoint) {
        Err(tungstenite::Error::Http(response)) => assert_eq!(response.status().as_u16(), 404),
        _ => return Err("Closed direct endpoint was not HTTP404".into()),
    }
    let survivor = cdp.rpc("Page.getFrameTree", json!({}), Some(&first_session))?;
    assert_eq!(
        survivor["frameTree"]["frame"]["url"],
        format!("{}/first/submit?q=alpha", fixture.base)
    );
    let unsupported = cdp.raw(
        "Runtime.evaluate",
        json!({"expression":"1+1"}),
        Some(&first_session),
    )?;
    assert_eq!(unsupported["error"]["code"], -32601);
    native.frame(original, FIRST, &scratch.join("surviving-target.png"))?;
    fs::write(
        scratch.join("requests.txt"),
        fixture.requests.lock().unwrap().join("\n"),
    )?;
    fs::write(
        scratch.join("targets.json"),
        serde_json::to_string_pretty(
            &json!({"original":named,"afterClose":cdp.targets()?,"closedSessionError":stale,"closedTargetError":closed,"runtimeUnsupported":unsupported}),
        )?,
    )?;
    native.key(original, b'w'.into(), true)?;
    wait("last native tab closes process", || {
        Ok(native
            .child
            .try_wait()?
            .is_some_and(|status| status.success()))
    })?;
    println!(
        "WORKSPACE_CDP_OK {} : named targets, retained edits, native detach/close and stale-route rejection",
        scratch.display()
    );
    Ok(())
}

fn main() -> Result<()> {
    if std::env::var("MGBROWSER_WORKSPACE_CDP_PRIVATE_DISPLAY").as_deref() != Ok("1") {
        return Err("Run tools/workspace-cdp-smoke.sh on its owned display".into());
    }
    let mut args = std::env::args_os().skip(1);
    let payload = PathBuf::from(args.next().ok_or("Expected supplied executable")?);
    let scratch = PathBuf::from(args.next().ok_or("Expected scratch directory")?);
    run(&payload, &scratch)
}
