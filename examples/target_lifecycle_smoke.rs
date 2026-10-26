//! Separate supplied-binary host-mediated native Target lifecycle proof.
//! No child, fixture server, browser, renderer or evaluator is started here.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Cursor, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig};
use x11rb::{connection::Connection, protocol::xproto::*, rust_connection::RustConnection};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const URL_A: &str = "http://127.0.0.1:7878/script-events?runtime-move=A";
const URL_B: &str = "http://127.0.0.1:7878/script-events?runtime-move=B";
const LOG_LIMIT: u64 = 1024 * 1024;
const CREATE_A: &str = "globalThis.runtimeMoveRealm='A';document.title='Runtime move A';document.body.style.margin='0';document.getElementById('state').textContent='';document.getElementById('state').style.cssText='position:absolute;left:20px;top:300px;width:120px;height:60px;margin:0;background:#123456';(()=>{let n=100;return {inc:function(alias){if(this!==alias)throw 41;n++;return [globalThis.runtimeMoveRealm,n];}}})()";
const CREATE_B: &str = "globalThis.runtimeMoveRealm='B';document.title='Runtime move B';document.body.style.margin='0';document.getElementById('state').textContent='';document.getElementById('state').style.cssText='position:absolute;left:20px;top:300px;width:120px;height:60px;margin:0;background:#654321';(()=>{let n=200;return {inc:function(alias){if(this!==alias)throw 41;n++;return [globalThis.runtimeMoveRealm,n];}}})()";
const CREATE_B_FLAT: &str = "(()=>{let n=300;return {inc:function(alias){if(this!==alias)throw 41;n++;return [globalThis.runtimeMoveRealm,n];}}})()";
const INCREMENT: &str = "function(alias){return this.inc(alias);}";
const MARKER: &str = "globalThis.runtimeMoveRealm";

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::other(message.into()).into()
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(failure(message))
    }
}
fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|v| !v.is_zero())
        .ok_or_else(|| failure("Runtime client reached its 28-second work cutoff"))
}
fn emit(value: Value) -> Result<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    require(bytes.len() <= 8 * 1024 * 1024, "PNG exceeds8MiB")?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}
fn read_log(path: &Path) -> Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let info = file.metadata()?;
    require(
        info.is_file() && info.len() <= LOG_LIMIT,
        "Unsafe or oversized browser log",
    )?;
    let mut body = Vec::new();
    (&mut file).take(LOG_LIMIT + 1).read_to_end(&mut body)?;
    require(
        body.len() as u64 <= LOG_LIMIT,
        "Browser log grew beyond bound",
    )?;
    Ok(String::from_utf8(body)?)
}

fn target_number(target: &str) -> Result<u64> {
    let number = target
        .strip_prefix("page-")
        .and_then(|number| number.parse::<u64>().ok())
        .filter(|number| *number > 0)
        .ok_or("Invalid canonical page target")?;
    require(
        target == format!("page-{number}"),
        "Noncanonical page target",
    )?;
    Ok(number)
}
fn target_inventory(result: &Value) -> Result<&[Value]> {
    let values = result["targetInfos"]
        .as_array()
        .ok_or("Missing target inventory")?;
    require((1..=2).contains(&values.len()), "Unexpected target count")?;
    let mut ids = Vec::new();
    for value in values {
        let id = value["targetId"]
            .as_str()
            .ok_or("Missing actual target ID")?;
        target_number(id)?;
        require(
            value["type"] == "page" && !ids.contains(&id),
            "Foreign/duplicate target",
        )?;
        ids.push(id);
    }
    require(ids.contains(&"page-1"), "Initial target disappeared")?;
    let a = values
        .iter()
        .find(|value| value["targetId"] == "page-1")
        .unwrap();
    require(a["url"] == URL_A, "A target URL changed")?;
    Ok(values)
}
fn opened_tab(log: &str) -> Result<Option<u64>> {
    let mut opened = None;
    for line in log.lines().filter(|line| line.starts_with("TAB ")) {
        require(opened.is_none(), "Unexpected second native tab creation")?;
        let (tab, pane) = line
            .strip_prefix("TAB opened=")
            .and_then(|line| line.split_once(" pane="))
            .ok_or("Malformed native tab identity")?;
        let tab = target_number(&format!("page-{tab}"))?;
        target_number(&format!("page-{pane}"))?;
        opened = Some(tab);
    }
    Ok(opened)
}
struct BoundTarget {
    target: String,
    frame: String,
    endpoint: url::Url,
}
fn bind_new_target(
    result: &Value,
    native_tab: Option<u64>,
    endpoint: &url::Url,
) -> Result<Option<BoundTarget>> {
    require(
        native_tab.is_none_or(|number| number > 0),
        "Invalid native tab number",
    )?;
    let values = target_inventory(result)?;
    let Some(other) = values.iter().find(|value| value["targetId"] != "page-1") else {
        return Ok(None);
    };
    require(
        other["url"] == "about:blank" && other["attached"] == false,
        "New native target was not blank/unattached",
    )?;
    let target = other["targetId"].as_str().ok_or("Missing new target ID")?;
    let number = target_number(target)?;
    let Some(native_tab) = native_tab else {
        return Ok(None);
    };
    require(
        number == native_tab,
        "Discovered target differs from actual native tab",
    )?;
    let mut endpoint = endpoint.clone();
    endpoint.set_path(&format!("/devtools/page/{target}"));
    Ok(Some(BoundTarget {
        target: target.into(),
        frame: format!("frame-{number}"),
        endpoint,
    }))
}

#[derive(Default)]
struct Traffic {
    messages: Cell<usize>,
    bytes: Cell<usize>,
}
struct Client {
    socket: WebSocket<TcpStream>,
    route: &'static str,
    deadline: Instant,
    next: u64,
    pending: Option<(u64, Option<String>)>,
    events: Vec<Value>,
    event_bytes: usize,
    runtime_enabled: Vec<Option<String>>,
    traffic: Rc<Traffic>,
}
impl Client {
    fn connect(
        endpoint: &url::Url,
        expected_path: &str,
        route: &'static str,
        deadline: Instant,
        traffic: Rc<Traffic>,
    ) -> Result<Self> {
        require(
            endpoint.scheme() == "ws"
                && endpoint.host_str() == Some("127.0.0.1")
                && endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.query().is_none()
                && endpoint.fragment().is_none()
                && endpoint.path() == expected_path,
            "Expected explicit owned loopback CDP endpoint",
        )?;
        let port = endpoint
            .port()
            .filter(|p| *p != 0)
            .ok_or("Missing CDP port")?;
        let stream = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            remaining(deadline)?,
        )?;
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        stream.set_write_timeout(Some(remaining(deadline)?))?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(8 * 1024 * 1024))
            .max_frame_size(Some(8 * 1024 * 1024));
        let (socket, _) =
            tungstenite::client::client_with_config(endpoint.as_str(), stream, Some(config))?;
        Ok(Self {
            socket,
            route,
            deadline,
            next: 1,
            pending: None,
            events: vec![],
            event_bytes: 0,
            runtime_enabled: vec![],
            traffic,
        })
    }
    fn timeout(&mut self) -> Result<()> {
        let left = remaining(self.deadline)?;
        self.socket.get_mut().set_read_timeout(Some(left))?;
        self.socket.get_mut().set_write_timeout(Some(left))?;
        Ok(())
    }
    fn read(&mut self) -> Result<Value> {
        loop {
            self.timeout()?;
            match self.socket.read()? {
                Message::Text(text) => {
                    self.traffic.messages.set(self.traffic.messages.get() + 1);
                    self.traffic
                        .bytes
                        .set(self.traffic.bytes.get() + text.len());
                    require(
                        self.traffic.messages.get() <= 4096
                            && self.traffic.bytes.get() <= 24 * 1024 * 1024,
                        "Shared CDP receive bound exceeded",
                    )?;
                    let value: Value = serde_json::from_str(text.as_str())?;
                    require(value.is_object(), "Expected CDP object")?;
                    let mut retained = value.clone();
                    if let Some(data) = retained.pointer_mut("/result/data") {
                        let bytes = data.as_str().ok_or("Nonstring screenshot data")?.len();
                        *data = json!({"base64_characters_omitted":bytes});
                    }
                    emit(json!({"protocol":"receive","route":self.route,"message":retained}))?;
                    return Ok(value);
                }
                Message::Ping(_) | Message::Pong(_) => {
                    self.socket.flush()?;
                }
                _ => return Err(failure("CDP closed or returned a nontext frame")),
            }
        }
    }
    fn event(&mut self, value: Value) -> Result<()> {
        require(
            value.get("id").is_none() && value["method"].as_str().is_some(),
            "Expected CDP event",
        )?;
        self.event_bytes += serde_json::to_vec(&value)?.len();
        require(
            self.events.len() < 256 && self.event_bytes <= 1024 * 1024,
            "CDP event retention bound",
        )?;
        self.events.push(value);
        Ok(())
    }
    fn begin(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<()> {
        require(
            self.pending.is_none() && self.next <= 256,
            "Client request single-flight/ID bound",
        )?;
        let id = self.next;
        self.next += 1;
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        let text = serde_json::to_string(&request)?;
        require(text.len() <= 64 * 1024, "Client request exceeds64KiB")?;
        emit(json!({"protocol":"send","route":self.route,"message":request}))?;
        self.timeout()?;
        self.socket.send(Message::Text(text.into()))?;
        self.pending = Some((id, session.map(str::to_owned)));
        Ok(())
    }
    fn finish(&mut self) -> Result<Value> {
        let (id, session) = self.pending.clone().ok_or("No request awaits response")?;
        loop {
            let value = self.read()?;
            if value.get("id").is_none() {
                self.event(value)?;
                continue;
            }
            require(
                value["id"].as_u64() == Some(id)
                    && value.get("sessionId").and_then(Value::as_str) == session.as_deref()
                    && value.get("result").is_some() != value.get("error").is_some(),
                "Unmatched/ambiguous CDP response",
            )?;
            self.pending = None;
            return Ok(value);
        }
    }
    fn raw(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.begin(method, params, session)?;
        self.finish()
    }
    fn rpc(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        let reply = self.raw(method, params, session)?;
        require(
            reply.get("error").is_none(),
            &format!("{method} failed: {reply}"),
        )?;
        let result = reply["result"].clone();
        require(
            result.get("errorText").is_none(),
            &format!("Navigation failed: {result}"),
        )?;
        Ok(result)
    }

    fn context(&mut self, session: Option<&str>, frame: &str) -> Result<Value> {
        let key = session.map(str::to_owned);
        require(
            !self.runtime_enabled.contains(&key),
            "Context enable must be first for this exact route",
        )?;
        let before = self.events.len();
        require(
            self.rpc("Runtime.enable", json!({}), session)? == json!({}),
            "Enable reply differs",
        )?;
        require(
            !self.events[before..].iter().any(|event| {
                event["method"] == "Runtime.executionContextCreated"
                    && event.get("sessionId").and_then(Value::as_str) == session
            }),
            "Runtime.enable acknowledgment must precede its route's contextCreated",
        )?;
        self.runtime_enabled.push(key);
        loop {
            if let Some(event) = self.events[before..].iter().find(|event| {
                event["method"] == "Runtime.executionContextCreated"
                    && event.get("sessionId").and_then(Value::as_str) == session
            }) {
                let context = &event["params"]["context"];
                let id = context["id"]
                    .as_u64()
                    .filter(|id| *id > 0 && *id <= i32::MAX as u64)
                    .ok_or("Invalid actual context ID")?;
                require(
                    context
                        == &json!({"id":id,"origin":"http://127.0.0.1:7878","name":"",
                    "uniqueId":format!("mg-context-{id}"),
                    "auxData":{"isDefault":true,"type":"default","frameId":frame}}),
                    "Actual context metadata/default/frame/origin differs",
                )?;
                return Ok(context.clone());
            }
            let event = self.read()?;
            self.event(event)?;
        }
    }
    fn stable_contexts(&self, expected: &[(Option<&str>, &Value)]) -> Result<()> {
        let runtime: Vec<_> = self
            .events
            .iter()
            .filter(|event| {
                event["method"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("Runtime."))
            })
            .collect();
        require(
            runtime.len() == expected.len(),
            "Move recreated/destroyed/duplicated a Runtime context",
        )?;
        for (session, context) in expected {
            require(
                runtime
                    .iter()
                    .filter(|event| {
                        event["method"] == "Runtime.executionContextCreated"
                            && event.get("sessionId").and_then(Value::as_str) == *session
                            && &event["params"]["context"] == *context
                    })
                    .count()
                    == 1,
                "Route context identity changed",
            )?;
        }
        Ok(())
    }
    fn frame_url(&mut self, session: Option<&str>, frame: &str, url: &str) -> Result<()> {
        let result = self.rpc("Page.getFrameTree", json!({}), session)?;
        require(
            result["frameTree"]["frame"]["id"] == frame
                && result["frameTree"]["frame"]["url"] == url,
            "Target/frame/URL changed",
        )
    }
    fn targets(&mut self, expected: &[&str], allowed: &[&str]) -> Result<bool> {
        let result = self.rpc("Target.getTargets", json!({}), None)?;
        let values = target_inventory(&result)?;
        for value in values {
            let id = value["targetId"]
                .as_str()
                .ok_or("Missing actual target ID")?;
            require(
                allowed.contains(&id) && value["url"] == if id == "page-1" { URL_A } else { URL_B },
                "Foreign target or changed target URL",
            )?;
        }
        let mut ids: Vec<_> = values
            .iter()
            .map(|value| value["targetId"].as_str().ok_or("Missing actual target ID"))
            .collect::<std::result::Result<_, _>>()?;
        ids.sort_unstable();
        Ok(ids == expected)
    }
    fn attach(&mut self, target: &str) -> Result<String> {
        let result = self.rpc(
            "Target.attachToTarget",
            json!({"targetId":target,"flatten":true}),
            None,
        )?;
        result["sessionId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .map(str::to_owned)
            .ok_or_else(|| failure("Missing bounded actual session ID"))
    }
    fn load_b(&mut self, session: &str, frame: &str) -> Result<()> {
        while !self.events.iter().any(|event| {
            event["method"] == "Page.frameStoppedLoading"
                && event.get("sessionId").and_then(Value::as_str) == Some(session)
        }) {
            let event = self.read()?;
            self.event(event)?;
        }
        let events: Vec<_> = self
            .events
            .iter()
            .filter(|event| {
                event.get("sessionId").and_then(Value::as_str) == Some(session)
                    && event["method"]
                        .as_str()
                        .is_some_and(|name| name.starts_with("Page."))
            })
            .collect();
        require(
            events
                .iter()
                .filter_map(|event| event["method"].as_str())
                .collect::<Vec<_>>()
                == [
                    "Page.frameStartedLoading",
                    "Page.frameNavigated",
                    "Page.domContentEventFired",
                    "Page.loadEventFired",
                    "Page.frameStoppedLoading",
                ],
            "B lifecycle order differs",
        )?;
        require(
            events[1]["params"]["frame"]["url"] == URL_B
                && events[1]["params"]["frame"]["id"] == frame,
            "B navigation identity differs",
        )?;
        self.frame_url(Some(session), frame, URL_B)
    }
    fn screenshot(&mut self, path: &Path) -> Result<image::RgbImage> {
        let result = self.rpc("Page.captureScreenshot", json!({"format":"png"}), None)?;
        let bytes = STANDARD.decode(result["data"].as_str().ok_or("Missing screenshot PNG")?)?;
        require(
            bytes.len() <= 8 * 1024 * 1024 && bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "PNG bounds/header",
        )?;
        let reader = image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Png);
        let (width, height) = reader.into_dimensions()?;
        require(
            width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 16_777_216,
            "PNG dimensions",
        )?;
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.to_rgb8();
        write_new(path, &bytes)?;
        Ok(image)
    }
    fn closed(&mut self) -> Result<()> {
        loop {
            self.timeout()?;
            match self.socket.read() {
                Ok(Message::Close(_))
                | Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed)
                | Err(tungstenite::Error::Protocol(
                    tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
                )) => break,
                Ok(Message::Text(text)) => {
                    self.traffic.messages.set(self.traffic.messages.get() + 1);
                    self.traffic
                        .bytes
                        .set(self.traffic.bytes.get() + text.len());
                    require(
                        self.traffic.messages.get() <= 4096
                            && self.traffic.bytes.get() <= 24 * 1024 * 1024,
                        "Shared CDP receive bound exceeded during close",
                    )?;
                    let event: Value = serde_json::from_str(text.as_str())?;
                    emit(json!({"protocol":"receive","route":self.route,"message":event}))?;
                    self.event(event)?;
                }
                Ok(Message::Ping(_) | Message::Pong(_)) => self.socket.flush()?,
                Err(tungstenite::Error::Io(error))
                    if error.kind() == io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                other => {
                    return Err(failure(format!(
                        "Expected normal closed-target socket, got {other:?}"
                    )));
                }
            }
        }
        emit(json!({"phase":"direct-closed","route":self.route}))
    }
}

struct Native {
    conn: RustConnection,
    root: u32,
    window: u32,
    deadline: Instant,
}
impl Native {
    fn new(window: u32, deadline: Instant) -> Result<Self> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        require(
            conn.setup().image_byte_order == ImageOrder::LSB_FIRST,
            "Unsupported Xvfb byte order",
        )?;
        let class = conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)?
            .reply()?;
        require(
            class.format == 8 && class.value.as_slice() == b"mgbrowser\0mgbrowser\0",
            "Supplied native window has the wrong actual WM_CLASS",
        )?;
        // PID binding comes from the owner's WINDOW id/pid log and listener
        // ownership. Desktop does not publish an X11 _NET_WM_PID property.
        Ok(Self {
            conn,
            root,
            window,
            deadline,
        })
    }
    fn frame(&self) -> Result<image::RgbImage> {
        remaining(self.deadline)?;
        let geometry = self.conn.get_geometry(self.window)?.reply()?;
        require(
            geometry.depth == 24
                && geometry.width > 0
                && geometry.width <= 1920
                && geometry.height > 93
                && geometry.height <= 1080,
            "Unexpected private window geometry",
        )?;
        require(
            self.conn
                .setup()
                .pixmap_formats
                .iter()
                .any(|f| f.depth == 24 && f.bits_per_pixel == 32 && f.scanline_pad == 32),
            "Unexpected X11 pixels",
        )?;
        let data = self
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
        require(
            data.len() == usize::from(geometry.width) * usize::from(geometry.height) * 4,
            "Native pixel size",
        )?;
        let rgb = data
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        remaining(self.deadline)?;
        image::RgbImage::from_raw(geometry.width.into(), geometry.height.into(), rgb)
            .ok_or_else(|| failure("Native RGB size"))
    }
    fn title(&self, title: &str) -> Result<bool> {
        remaining(self.deadline)?;
        let bytes = self
            .conn
            .get_property(
                false,
                self.window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                0,
                1024,
            )?
            .reply()?
            .value;
        Ok(String::from_utf8_lossy(&bytes).contains(title))
    }
    fn key(&self, symbol: u32, ctrl: bool) -> Result<()> {
        remaining(self.deadline)?;
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
                    .position(|key| *key == symbol)
                    .map(|column| (first + index as u8, column == 1))
            })
            .ok_or("Native keysym unavailable")?;
        let mut state = KeyButMask::default();
        if ctrl {
            state |= KeyButMask::CONTROL;
        }
        if shift {
            state |= KeyButMask::SHIFT;
        }
        self.conn
            .set_input_focus(InputFocus::PARENT, self.window, x11rb::CURRENT_TIME)?;
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
        Ok(())
    }
}
fn save_native(image: &image::RgbImage, path: &Path) -> Result<()> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png)?;
    write_new(path, bytes.get_ref())
}
fn pixels(image: &image::RgbImage, top: u32, color: [u8; 3]) -> bool {
    [
        (30, 310, color),
        (130, 350, color),
        (19, 310, [255; 3]),
        (140, 350, [255; 3]),
    ]
    .into_iter()
    .all(|(x, y, rgb)| {
        x < image.width() && y + top < image.height() && image.get_pixel(x, y + top).0 == rgb
    })
}
fn value(reply: &Value, kind: &str, expected: Value) -> Result<()> {
    require(
        reply.get("exceptionDetails").is_none()
            && reply["result"]["type"] == kind
            && reply["result"]["value"] == expected,
        &format!("Wrong Runtime value: {reply}"),
    )
}
fn object(reply: &Value) -> Result<String> {
    require(
        reply.get("exceptionDetails").is_none() && reply["result"]["type"] == "object",
        "Expected actual object root",
    )?;
    reply["result"]["objectId"]
        .as_str()
        .filter(|id| id.len() <= 128)
        .map(str::to_owned)
        .ok_or_else(|| failure("Missing real objectId"))
}
fn protocol_failure(reply: &Value, code: i64) -> Result<()> {
    require(
        reply["error"]["code"].as_i64() == Some(code) && reply.get("result").is_none(),
        &format!("Expected specific protocol rejection: {reply}"),
    )
}

impl Native {
    fn windows(&self) -> Result<Vec<u32>> {
        remaining(self.deadline)?;
        let mut result = Vec::new();
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
            if class.value.as_slice() == b"mgbrowser\0mgbrowser\0" {
                result.push(window);
            }
        }
        result.sort_unstable();
        require(result.len() <= 2, "Unexpected third native window")?;
        Ok(result)
    }
    fn place(&self, x: i32) -> Result<()> {
        remaining(self.deadline)?;
        self.conn.configure_window(
            self.window,
            &ConfigureWindowAux::new().x(x).y(40).width(800).height(634),
        )?;
        self.conn.flush()?;
        Ok(())
    }
    fn root_point(&self, window: u32, x: i16, y: i16) -> Result<(i16, i16)> {
        remaining(self.deadline)?;
        let point = self
            .conn
            .translate_coordinates(window, self.root, x, y)?
            .reply()?;
        Ok((point.dst_x, point.dst_y))
    }
    fn button_root(&self, point: (i16, i16), pressed: bool) -> Result<()> {
        remaining(self.deadline)?;
        let local = self
            .conn
            .translate_coordinates(self.root, self.window, point.0, point.1)?
            .reply()?;
        self.conn
            .warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, point.0, point.1)?;
        self.conn.send_event(
            false,
            self.window,
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
                event: self.window,
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
        thread::sleep(remaining(self.deadline)?.min(Duration::from_millis(50)));
        Ok(())
    }
    fn motion_root(&self, point: (i16, i16)) -> Result<()> {
        remaining(self.deadline)?;
        let local = self
            .conn
            .translate_coordinates(self.root, self.window, point.0, point.1)?
            .reply()?;
        self.conn
            .warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, point.0, point.1)?;
        self.conn.send_event(
            false,
            self.window,
            EventMask::POINTER_MOTION,
            MotionNotifyEvent {
                response_type: MOTION_NOTIFY_EVENT,
                detail: Motion::NORMAL,
                sequence: 0,
                time: 0,
                root: self.root,
                event: self.window,
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
        thread::sleep(remaining(self.deadline)?.min(Duration::from_millis(80)));
        Ok(())
    }
    fn drag(&self, from: (i16, i16), to: (i16, i16), label: &str) -> Result<()> {
        let start = self.root_point(self.window, from.0, from.1)?;
        self.button_root(start, true)?;
        self.motion_root((start.0 + 20, start.1))?;
        self.motion_root(to)?;
        self.button_root(to, false)?;
        emit(json!({"phase":"native-drag","label":label,"window":self.window,"from":start,"to":to}))
    }
}

fn window_claims(log: &str, pid: u32) -> Result<Vec<u32>> {
    let mut ids = Vec::new();
    for line in log.lines().filter(|line| line.starts_with("WINDOW ")) {
        let (id, owner) = line
            .strip_prefix("WINDOW id=")
            .and_then(|s| s.split_once(" pid="))
            .ok_or("Malformed WINDOW identity")?;
        let id = id.parse::<u32>()?;
        require(
            id != 0 && owner.parse::<u32>()? == pid && !ids.contains(&id),
            "Foreign/duplicate WINDOW identity",
        )?;
        ids.push(id);
    }
    require(ids.len() <= 2, "Unexpected native window creation")?;
    Ok(ids)
}
fn wait_windows(native: &Native, log: &Path, pid: u32, count: usize) -> Result<Vec<u32>> {
    loop {
        let claims = window_claims(&read_log(log)?, pid)?;
        let actual = native.windows()?;
        if actual.len() == count && actual.iter().all(|id| claims.contains(id)) {
            return Ok(actual);
        }
        thread::sleep(remaining(native.deadline)?.min(Duration::from_millis(15)));
    }
}
fn wait_targets(client: &mut Client, expected: &[&str], allowed: &[&str]) -> Result<()> {
    while !client.targets(expected, allowed)? {
        thread::sleep(remaining(client.deadline)?.min(Duration::from_millis(15)));
    }
    Ok(())
}
fn wait_new_target(client: &mut Client, log: &Path, endpoint: &url::Url) -> Result<BoundTarget> {
    loop {
        let result = client.rpc("Target.getTargets", json!({}), None)?;
        let live_log = read_log(log)?;
        let complete = &live_log[..live_log.rfind('\n').map_or(0, |end| end + 1)];
        if let Some(bound) = bind_new_target(&result, opened_tab(complete)?, endpoint)? {
            return Ok(bound);
        }
        thread::sleep(remaining(client.deadline)?.min(Duration::from_millis(15)));
    }
}
fn paint(
    native: &Native,
    client: &mut Client,
    output: &Path,
    label: &str,
    title: &str,
    color: [u8; 3],
) -> Result<()> {
    let mut last = None;
    loop {
        if let Err(error) = remaining(native.deadline) {
            if let Some(image) = last {
                save_native(&image, &output.join("failure-native.png"))?;
            }
            return Err(error);
        }
        if native.title(title)? {
            let image = native.frame()?;
            if image.width() == 800 && image.height() == 634 && pixels(&image, 98, color) {
                save_native(&image, &output.join(format!("native-{label}.png")))?;
                let cdp = client.screenshot(&output.join(format!("cdp-{label}.png")))?;
                require(
                    cdp.width() == 800 && cdp.height() == 507 && pixels(&cdp, 0, color),
                    "Native/CDP move paint/viewport mismatch",
                )?;
                emit(
                    json!({"phase":"paint","label":label,"window":native.window,"color":color,
                    "native_page_top":98,"scale":1,"native_size":[800,634],"cdp_size":[800,507]}),
                )?;
                return Ok(());
            }
            last = Some(image);
        }
        thread::sleep(remaining(native.deadline)?.min(Duration::from_millis(25)));
    }
}

// Every retained byte belongs to one explicit interval. No target identity is
// invented from SCRIPT_BOA, whose existing log format has no target field.
struct Journal {
    path: PathBuf,
    cursor: usize,
    deadline: Instant,
}
impl Journal {
    fn read(&self) -> Result<String> {
        loop {
            let text = read_log(&self.path)?;
            require(text.len() >= self.cursor, "Browser log was truncated")?;
            if text.is_empty() || text.ends_with('\n') {
                return Ok(text);
            }
            thread::sleep(remaining(self.deadline)?.min(Duration::from_millis(5)));
        }
    }
    fn record(
        &mut self,
        kind: &str,
        realm: Option<&str>,
        sequence: Option<u64>,
        label: &str,
        command: Option<(&str, u64)>,
    ) -> Result<()> {
        let text = self.read()?;
        let part = &text[self.cursor..];
        let reports = part
            .lines()
            .filter(|line| line.starts_with("SCRIPT_BOA "))
            .count();
        let tasks: Vec<_> = part
            .lines()
            .filter(|line| line.starts_with("PAGE_TASK_APPLIED "))
            .collect();
        let auto: Vec<_> = part
            .lines()
            .filter(|line| line.starts_with("PAGE_AUTOMATION_APPLIED "))
            .collect();
        let completes: Vec<_> = part
            .lines()
            .filter(|line| line.starts_with("SCRIPT_COMPLETE "))
            .collect();
        require(
            !part
                .lines()
                .any(|line| line.starts_with("PAGE_EVENT_APPLIED ")),
            "Unexpected page activation",
        )?;
        match kind {
            "bootstrap" => require(
                reports == 3
                    && auto.is_empty()
                    && tasks
                        == [
                            "PAGE_TASK_APPLIED sequence=1 revision=1",
                            "PAGE_TASK_APPLIED sequence=2 revision=2",
                        ]
                    && completes
                        == [
                            "SCRIPT_COMPLETE executed=1",
                            "SCRIPT_COMPLETE executed=0",
                            "SCRIPT_COMPLETE executed=0",
                        ],
                "Bootstrap interval is not exactly Init plus two lifecycle tasks",
            )?,
            "automation" => {
                let seq = sequence.ok_or("Missing accepted sequence")?;
                let expected =
                    format!("PAGE_AUTOMATION_APPLIED sequence={seq} revision={seq} terminal=false");
                require(
                    reports == 1
                        && tasks.is_empty()
                        && completes == ["SCRIPT_COMPLETE executed=0"]
                        && auto == [expected.as_str()],
                    "Accepted command interval differs from exact sequence/report",
                )?;
            }
            "quiet" => require(
                reports == 0 && tasks.is_empty() && auto.is_empty() && completes.is_empty(),
                "Unattributed worker work in host-only interval",
            )?,
            _ => return Err(failure("Unknown interval kind")),
        }
        emit(
            json!({"phase":"log-interval","kind":kind,"realm":realm,"sequence":sequence,"label":label,
            "command":command.map(|(route,id)|json!({"route":route,"id":id})),"start":self.cursor,"end":text.len()}),
        )?;
        self.cursor = text.len();
        Ok(())
    }
    fn bootstrap(&mut self, realm: &str) -> Result<()> {
        loop {
            let text = self.read()?;
            if text[self.cursor..]
                .lines()
                .any(|line| line == "PAGE_TASK_APPLIED sequence=2 revision=2")
            {
                break;
            }
            thread::sleep(remaining(self.deadline)?.min(Duration::from_millis(10)));
        }
        self.record("bootstrap", Some(realm), None, "initial-load", None)
    }
    fn quiet(&mut self, label: &str) -> Result<()> {
        self.record("quiet", None, None, label, None)
    }
    fn command(
        &mut self,
        client: &mut Client,
        session: Option<&str>,
        realm: &str,
        sequence: u64,
        label: &str,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        self.quiet("before-command")?;
        let id = client.next;
        let result = client.rpc(method, params, session)?;
        self.record(
            "automation",
            Some(realm),
            Some(sequence),
            label,
            Some((client.route, id)),
        )?;
        Ok(result)
    }
    fn negative(
        &mut self,
        client: &mut Client,
        session: Option<&str>,
        label: &str,
        method: &str,
        params: Value,
    ) -> Result<()> {
        self.quiet("before-negative")?;
        let id = client.next;
        protocol_failure(&client.raw(method, params, session)?, -32000)?;
        self.record("quiet", None, None, label, Some((client.route, id)))
    }
    fn target_negative(
        &mut self,
        client: &mut Client,
        session: Option<&str>,
        label: &str,
        method: &str,
        params: Value,
        code: i64,
    ) -> Result<()> {
        self.quiet("before-target-negative")?;
        let id = client.next;
        protocol_failure(&client.raw(method, params, session)?, code)?;
        self.record("quiet", None, None, label, Some((client.route, id)))
    }
}
fn eval_params(source: &str, context: u64, group: Option<&str>) -> Value {
    let mut params = json!({"expression":source,"contextId":context,"returnByValue":false});
    if let Some(group) = group {
        params["objectGroup"] = json!(group);
    }
    params
}
fn call_params(root: &str) -> Value {
    json!({"functionDeclaration":INCREMENT,"objectId":root,"arguments":[{"objectId":root}],"returnByValue":true})
}
fn increment(
    journal: &mut Journal,
    client: &mut Client,
    session: Option<&str>,
    realm: &str,
    seq: u64,
    label: &str,
    root: &str,
    expected: f64,
) -> Result<()> {
    let result = journal.command(
        client,
        session,
        realm,
        seq,
        label,
        "Runtime.callFunctionOn",
        call_params(root),
    )?;
    require(
        result.get("exceptionDetails").is_none()
            && result["result"]
                == json!({"type":"object","subtype":"array","value":[realm,expected]}),
        "Retained closure/receiver/realm identity differs",
    )
}
fn barriers(
    a: &mut Client,
    b: &mut Client,
    flat: &mut Client,
    sa: &str,
    sb: &str,
    ca: &Value,
    cb: &Value,
    b_target: &BoundTarget,
) -> Result<()> {
    a.frame_url(None, "frame-1", URL_A)?;
    b.frame_url(None, &b_target.frame, URL_B)?;
    flat.frame_url(Some(sa), "frame-1", URL_A)?;
    flat.frame_url(Some(sb), &b_target.frame, URL_B)?;
    require(
        flat.targets(&["page-1", &b_target.target], &["page-1", &b_target.target])?,
        "Move changed target roster",
    )?;
    a.stable_contexts(&[(None, ca)])?;
    b.stable_contexts(&[(None, cb)])?;
    flat.stable_contexts(&[(Some(sa), ca), (Some(sb), cb)])
}

fn closed_endpoint(bound: &BoundTarget, deadline: Instant) -> Result<()> {
    let endpoint = &bound.endpoint;
    let port = endpoint.port().ok_or("Missing owned endpoint port")?;
    let stream = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        remaining(deadline)?,
    )?;
    stream.set_read_timeout(Some(remaining(deadline)?))?;
    stream.set_write_timeout(Some(remaining(deadline)?))?;
    match tungstenite::client::client_with_config(
        endpoint.as_str(),
        stream,
        Some(
            WebSocketConfig::default()
                .max_message_size(Some(8 * 1024 * 1024))
                .max_frame_size(Some(8 * 1024 * 1024)),
        ),
    ) {
        Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response)))
            if response.status().as_u16() == 404 =>
        {
            emit(json!({"phase":"closed-endpoint","target":bound.target,"status":404}))
        }
        _ => Err(failure(
            "Closed page endpoint did not return actual HTTP404",
        )),
    }
}

fn activate(
    flat: &mut Client,
    native: &mut Native,
    target: &str,
    window: u32,
    title: &str,
    label: &str,
    journal: &mut Journal,
) -> Result<()> {
    journal.quiet("before-activation")?;
    require(
        flat.rpc("Target.activateTarget", json!({"targetId":target}), None)? == json!({}),
        "Activation reply differs",
    )?;
    native.window = window;
    loop {
        remaining(native.deadline)?;
        // Do not repair focus with set_input_focus: this is the host's actual result.
        if native.conn.get_input_focus()?.reply()?.focus == window && native.title(title)? {
            emit(
                json!({"phase":"target-activated","label":label,"target":target,
                "window":window,"actual_input_focus":window,"title":title}),
            )?;
            journal.quiet("activation-complete")?;
            return Ok(());
        }
        thread::sleep(remaining(native.deadline)?.min(Duration::from_millis(15)));
    }
}

fn target_negatives(
    journal: &mut Journal,
    a: &mut Client,
    flat: &mut Client,
    sa: &str,
    target: &str,
) -> Result<()> {
    for (label, method, params) in [
        ("create-missing-url", "Target.createTarget", json!({})),
        (
            "create-nonstring-url",
            "Target.createTarget",
            json!({"url":false}),
        ),
        (
            "create-file-url",
            "Target.createTarget",
            json!({"url":"file:///tmp/not-admitted"}),
        ),
        (
            "create-extra-background",
            "Target.createTarget",
            json!({"url":"about:blank","background":false}),
        ),
        (
            "create-foreign-context",
            "Target.createTarget",
            json!({"url":"about:blank","browserContextId":"foreign"}),
        ),
        ("close-missing-target", "Target.closeTarget", json!({})),
        (
            "close-nonstring-target",
            "Target.closeTarget",
            json!({"targetId":7}),
        ),
        (
            "activate-missing-target",
            "Target.activateTarget",
            json!({}),
        ),
        (
            "activate-extra-option",
            "Target.activateTarget",
            json!({"targetId":target,"extra":true}),
        ),
    ] {
        journal.target_negative(flat, None, label, method, params, -32602)?;
    }
    for (label, method) in [
        ("close-unknown-target", "Target.closeTarget"),
        ("activate-unknown-target", "Target.activateTarget"),
    ] {
        journal.target_negative(
            flat,
            None,
            label,
            method,
            json!({"targetId":"page-18446744073709551615"}),
            -32000,
        )?;
    }
    for (suffix, client, session) in [
        ("direct", &mut *a, None),
        ("flattened", &mut *flat, Some(sa)),
    ] {
        for (name, method) in [
            ("create", "Target.createTarget"),
            ("activate", "Target.activateTarget"),
            ("close", "Target.closeTarget"),
        ] {
            let params = if name == "create" {
                json!({"url":"about:blank"})
            } else {
                json!({"targetId":target})
            };
            journal.target_negative(
                client,
                session,
                &format!("{name}-{suffix}-route"),
                method,
                params,
                -32602,
            )?;
        }
    }
    Ok(())
}

fn blank_document(b: &mut Client, journal: &mut Journal, target: &str) -> Result<()> {
    let tree = b.rpc("DOM.getDocument", json!({"depth":-1}), None)?;
    let root = &tree["root"];
    require(
        root["nodeType"] == 9
            && root["nodeName"] == "#document"
            && root["nodeValue"] == ""
            && root["childNodeCount"] == 0
            && root["children"] == json!([])
            && root["documentURL"] == "about:blank"
            && root["baseURL"] == "about:blank",
        "New target is not genuinely empty about:blank",
    )?;
    require(
        b.rpc("DOM.disable", json!({}), None)? == json!({}),
        "Blank DOM disable differs",
    )?;
    emit(json!({"phase":"blank-B","target":target,"root":root}))?;
    journal.quiet("blank-B-no-worker")
}

fn run(
    endpoint: url::Url,
    original: u32,
    pid: u32,
    log: &Path,
    output: &Path,
    deadline: Instant,
) -> Result<()> {
    fs::create_dir(output)?;
    require(
        endpoint.path() == "/devtools/page/page-1",
        "Initial target must be owned page-1",
    )?;
    let mut native = Native::new(original, deadline)?;
    native.place(40)?;
    let mut journal = Journal {
        path: log.to_owned(),
        cursor: 0,
        deadline,
    };
    journal.bootstrap("A")?;
    require(
        wait_windows(&native, log, pid, 1)? == [original],
        "Initial native window identity differs",
    )?;
    let traffic = Rc::new(Traffic::default());
    let mut a = Client::connect(
        &endpoint,
        "/devtools/page/page-1",
        "direct-A",
        deadline,
        traffic.clone(),
    )?;
    a.rpc("Page.enable", json!({}), None)?;
    a.frame_url(None, "frame-1", URL_A)?;
    // Existing public20 has this real context; its feature baseline is createTarget below.
    let ca = a.context(None, "frame-1")?;
    let aid = ca["id"].as_u64().ok_or("A context missing")?;
    let mut browser_url = endpoint.clone();
    browser_url.set_path("/devtools/browser/browser-1");
    let mut flat = Client::connect(
        &browser_url,
        "/devtools/browser/browser-1",
        "flattened",
        deadline,
        traffic.clone(),
    )?;
    require(
        flat.targets(&["page-1"], &["page-1"])?,
        "Initial target inventory differs",
    )?;
    let sa = flat.attach("page-1")?;
    require(
        flat.context(Some(&sa), "frame-1")? == ca,
        "A direct/flattened context mismatch",
    )?;
    emit(json!({"phase":"context-A","context":ca,"session":sa,"target":"page-1"}))?;
    let ar = object(&journal.command(
        &mut a,
        None,
        "A",
        3,
        "create-A",
        "Runtime.evaluate",
        eval_params(CREATE_A, aid, Some("A-direct")),
    )?)?;
    increment(&mut journal, &mut a, None, "A", 4, "A-baseline", &ar, 101.0)?;
    value(
        &journal.command(
            &mut flat,
            Some(&sa),
            "A",
            5,
            "A-flat-baseline",
            "Runtime.evaluate",
            eval_params(MARKER, aid, None),
        )?,
        "string",
        json!("A"),
    )?;

    journal.quiet("before-create")?;
    // First adopted mutation is valid: old public20 must reject this command explicitly.
    let created = flat.rpc("Target.createTarget", json!({"url":"about:blank"}), None)?;
    let created_id = created["targetId"]
        .as_str()
        .ok_or("Create returned no actual target ID")?
        .to_owned();
    require(
        created.as_object().is_some_and(|v| v.len() == 1) && target_number(&created_id)? > 1,
        "Malformed/nonmonotonic create result",
    )?;
    let b_target = wait_new_target(&mut flat, log, &endpoint)?;
    require(
        b_target.target == created_id,
        "Create result, discovery and native TAB differ",
    )?;
    emit(
        json!({"phase":"target-created","target":created_id,"url":"about:blank",
        "native_tab":target_number(&created_id)?}),
    )?;
    let mut b = Client::connect(
        &b_target.endpoint,
        &format!("/devtools/page/{}", b_target.target),
        "direct-B",
        deadline,
        traffic.clone(),
    )?;
    blank_document(&mut b, &mut journal, &b_target.target)?;
    target_negatives(&mut journal, &mut a, &mut flat, &sa, &b_target.target)?;
    let sb = flat.attach(&b_target.target)?;
    require(sa != sb, "Flattened session IDs reused")?;
    flat.rpc("Page.enable", json!({}), Some(&sb))?;
    journal.quiet("blank-B-no-worker")?;
    flat.rpc("Page.navigate", json!({"url":URL_B}), Some(&sb))?;
    flat.load_b(&sb, &b_target.frame)?;
    journal.bootstrap("B")?;
    b.rpc("Page.enable", json!({}), None)?;
    b.frame_url(None, &b_target.frame, URL_B)?;
    let cb = b.context(None, &b_target.frame)?;
    let bid = cb["id"].as_u64().ok_or("B context missing")?;
    require(
        aid != bid && ca["uniqueId"] != cb["uniqueId"],
        "Distinct realms reused a context",
    )?;
    require(
        flat.context(Some(&sb), &b_target.frame)? == cb,
        "B direct/flattened context mismatch",
    )?;
    emit(json!({"phase":"context-B","context":cb,"session":sb,"target":b_target.target}))?;
    let br = object(&journal.command(
        &mut b,
        None,
        "B",
        3,
        "create-B",
        "Runtime.evaluate",
        eval_params(CREATE_B, bid, Some("B-direct")),
    )?)?;
    let bf = object(&journal.command(
        &mut flat,
        Some(&sb),
        "B",
        4,
        "create-B-flat",
        "Runtime.evaluate",
        eval_params(CREATE_B_FLAT, bid, Some("B-flat")),
    )?)?;
    require(
        ar != br && br != bf && ar != bf,
        "Root IDs lost route/context distinction",
    )?;
    increment(&mut journal, &mut b, None, "B", 5, "B-baseline", &br, 201.0)?;
    increment(
        &mut journal,
        &mut flat,
        Some(&sb),
        "B",
        6,
        "B-flat-baseline",
        &bf,
        301.0,
    )?;
    journal.negative(
        &mut flat,
        Some(&sb),
        "foreign-owner-before",
        "Runtime.callFunctionOn",
        call_params(&br),
    )?;
    journal.negative(
        &mut b,
        None,
        "foreign-context-before",
        "Runtime.callFunctionOn",
        call_params(&ar),
    )?;
    barriers(&mut a, &mut b, &mut flat, &sa, &sb, &ca, &cb, &b_target)?;
    emit(json!({"phase":"roots","A":ar,"B_direct":br,"B_flat":bf}))?;
    activate(
        &mut flat,
        &mut native,
        "page-1",
        original,
        "Runtime move A",
        "A-initial",
        &mut journal,
    )?;
    activate(
        &mut flat,
        &mut native,
        &b_target.target,
        original,
        "Runtime move B",
        "B-initial",
        &mut journal,
    )?;

    native.drag((250, 16), (950, 180), "detach")?;
    let windows = wait_windows(&native, log, pid, 2)?;
    let detached = *windows
        .iter()
        .find(|id| **id != original)
        .ok_or("Detached XID missing")?;
    native.window = detached;
    native.place(950)?;
    increment(&mut journal, &mut a, None, "A", 6, "A-detached", &ar, 102.0)?;
    increment(&mut journal, &mut b, None, "B", 7, "B-detached", &br, 202.0)?;
    increment(
        &mut journal,
        &mut flat,
        Some(&sb),
        "B",
        8,
        "B-flat-detached",
        &bf,
        302.0,
    )?;
    barriers(&mut a, &mut b, &mut flat, &sa, &sb, &ca, &cb, &b_target)?;
    activate(
        &mut flat,
        &mut native,
        "page-1",
        original,
        "Runtime move A",
        "A-detached",
        &mut journal,
    )?;
    paint(
        &native,
        &mut a,
        output,
        "A-detached",
        "Runtime move A",
        [0x12, 0x34, 0x56],
    )?;
    activate(
        &mut flat,
        &mut native,
        &b_target.target,
        detached,
        "Runtime move B",
        "B-detached",
        &mut journal,
    )?;
    paint(
        &native,
        &mut b,
        output,
        "B-detached",
        "Runtime move B",
        [0x65, 0x43, 0x21],
    )?;
    journal.quiet("detach-complete")?;

    let to = native.root_point(original, 230, 16)?;
    native.drag((70, 16), to, "redock")?;
    require(
        wait_windows(&native, log, pid, 1)? == [original],
        "Redock did not remove only detached window",
    )?;
    native.window = original;
    increment(&mut journal, &mut a, None, "A", 7, "A-redocked", &ar, 103.0)?;
    increment(&mut journal, &mut b, None, "B", 9, "B-redocked", &br, 203.0)?;
    increment(
        &mut journal,
        &mut flat,
        Some(&sb),
        "B",
        10,
        "B-flat-redocked",
        &bf,
        303.0,
    )?;
    journal.negative(
        &mut flat,
        Some(&sb),
        "foreign-owner-after",
        "Runtime.callFunctionOn",
        call_params(&br),
    )?;
    journal.negative(
        &mut b,
        None,
        "foreign-context-after",
        "Runtime.callFunctionOn",
        call_params(&ar),
    )?;
    barriers(&mut a, &mut b, &mut flat, &sa, &sb, &ca, &cb, &b_target)?;
    activate(
        &mut flat,
        &mut native,
        &b_target.target,
        original,
        "Runtime move B",
        "B-redocked",
        &mut journal,
    )?;
    paint(
        &native,
        &mut b,
        output,
        "B-redocked",
        "Runtime move B",
        [0x65, 0x43, 0x21],
    )?;
    journal.quiet("redock-complete")?;

    journal.quiet("before-close-B")?;
    require(
        flat.rpc(
            "Target.closeTarget",
            json!({"targetId":b_target.target}),
            None,
        )? == json!({"success":true}),
        "Close did not confirm actual named removal",
    )?;
    require(
        wait_windows(&native, log, pid, 1)? == [original],
        "Closing B removed wrong native window",
    )?;
    wait_targets(&mut flat, &["page-1"], &["page-1", &b_target.target])?;
    let runtime: Vec<_> = flat
        .events
        .iter()
        .filter(|e| {
            e["method"]
                .as_str()
                .is_some_and(|m| m.starts_with("Runtime."))
        })
        .collect();
    require(
        runtime.len() == 3
            && runtime[0]["method"] == "Runtime.executionContextCreated"
            && runtime[0]["sessionId"] == sa
            && runtime[0]["params"]["context"] == ca
            && runtime[1]["method"] == "Runtime.executionContextCreated"
            && runtime[1]["sessionId"] == sb
            && runtime[1]["params"]["context"] == cb
            && runtime[2]["method"] == "Runtime.executionContextDestroyed"
            && runtime[2]["sessionId"] == sb
            && runtime[2]["params"]
                == json!({"executionContextId":bid,
            "executionContextUniqueId":format!("mg-context-{bid}")}),
        "Closing B changed wrong Runtime context",
    )?;
    let destroyed = flat
        .events
        .iter()
        .position(|e| e["method"] == "Runtime.executionContextDestroyed")
        .ok_or("B destroy missing")?;
    let detached_event: Vec<_> = flat
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| e["method"] == "Target.detachedFromTarget")
        .collect();
    require(
        detached_event.len() == 1
            && detached_event[0].0 > destroyed
            && detached_event[0].1["params"]["sessionId"] == sb
            && detached_event[0].1.get("sessionId").is_none(),
        "B context destruction must precede actual flattened detach",
    )?;
    b.closed()?;
    closed_endpoint(&b_target, deadline)?;
    journal.negative(
        &mut flat,
        Some(&sb),
        "closed-session",
        "Runtime.callFunctionOn",
        call_params(&bf),
    )?;
    journal.negative(
        &mut flat,
        None,
        "closed-target",
        "Target.attachToTarget",
        json!({"targetId":b_target.target,"flatten":true}),
    )?;
    journal.negative(
        &mut a,
        None,
        "closed-root-foreign",
        "Runtime.callFunctionOn",
        call_params(&br),
    )?;
    for (label, method) in [
        ("close-stale-target", "Target.closeTarget"),
        ("activate-stale-target", "Target.activateTarget"),
    ] {
        journal.target_negative(
            &mut flat,
            None,
            label,
            method,
            json!({"targetId":b_target.target}),
            -32000,
        )?;
    }
    increment(&mut journal, &mut a, None, "A", 8, "A-survivor", &ar, 104.0)?;
    value(
        &journal.command(
            &mut flat,
            Some(&sa),
            "A",
            9,
            "A-flat-survivor",
            "Runtime.evaluate",
            eval_params(MARKER, aid, None),
        )?,
        "string",
        json!("A"),
    )?;
    a.frame_url(None, "frame-1", URL_A)?;
    flat.frame_url(Some(&sa), "frame-1", URL_A)?;
    a.stable_contexts(&[(None, &ca)])?;
    paint(
        &native,
        &mut a,
        output,
        "A-survivor",
        "Runtime move A",
        [0x12, 0x34, 0x56],
    )?;
    journal.quiet("B-closed-A-alive")?;
    emit(
        json!({"phase":"closed-B","destroyed_context":bid,"surviving_context":aid,"original_window":original,"removed_window":detached}),
    )?;
    journal.target_negative(
        &mut flat,
        None,
        "close-last-target",
        "Target.closeTarget",
        json!({"targetId":"page-1"}),
        -32000,
    )?;
    require(
        flat.targets(&["page-1"], &["page-1", &b_target.target])?,
        "Last-target rejection changed survivor",
    )?;
    require(
        native.conn.get_input_focus()?.reply()?.focus == original,
        "Survivor native window lost actual focus",
    )?;
    native.key(b'w'.into(), true)?;
    require(
        wait_windows(&native, log, pid, 0)?.is_empty(),
        "Last native window did not close",
    )?;
    journal.quiet("last-native-window-closed")?;
    // No voluntary disconnect while either live target could admit root cleanup.
    // Last-window host teardown is separately checked/reaped by the owner.
    emit(
        json!({"phase":"target-lifecycle-proof","images":8,"target_moves":true,"host_mutations":7,"mutation_negatives":20,"real_worker_roots":3,
        "automation_transactions":15,"worker_reports":21,"lifecycle_tasks":4,"page_events":0,
        "shared_messages":traffic.messages.get(),"shared_reply_bytes":traffic.bytes.get()}),
    )?;
    println!("TARGET_LIFECYCLE_SMOKE_OK");
    Ok(())
}

fn main() -> Result<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(28);
    let args: Vec<_> = std::env::args().skip(1).collect();
    require(
        args.len() == 5,
        "Usage: target_lifecycle_smoke <page-1-ws> <owned-window> <owned-pid> <browser.log> <NEW-frames-dir>",
    )?;
    let window = args[1].parse::<u32>()?;
    let pid = args[2].parse::<u32>()?;
    require(window > 0 && pid > 0, "Invalid owned native identity")?;
    let result = run(
        url::Url::parse(&args[0])?,
        window,
        pid,
        Path::new(&args[3]),
        Path::new(&args[4]),
        deadline,
    );
    require(
        started.elapsed() < Duration::from_secs(30),
        "Runtime move client exceeded30s whole bound",
    )?;
    result
}
