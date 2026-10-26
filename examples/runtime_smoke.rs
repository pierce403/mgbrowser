//! Supplied-binary Runtime proof. The owner supplies one private native window.
//! No child, fixture server, browser, renderer or evaluator is started here.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Cursor, Write},
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
const FIXTURE: &str = "http://127.0.0.1:7878/script-events";
const DESTINATION: &str = "http://127.0.0.1:7878/event-destination?proof=clicked";
const LOG_LIMIT: u64 = 1024 * 1024;
const LOOP_SOURCE: &str = "function(){document.getElementById('query').value='stale-worker';let n=0;for(let i=0;i<10000;i++){n++;}return n;}";
const ORDERING_METHOD: &str = "four-frame-single-flush";
const PANEL: &str = "document.body.style.margin='0';document.getElementById('state').textContent='';document.getElementById('state').style.cssText='position:absolute;left:20px;top:300px;width:120px;height:60px;margin:0;background:#123456';document.title='Runtime native proof';42";
const BROWN_THROW: &str = "document.getElementById('state').style.backgroundColor='#654321';document.getElementById('state').setAttribute('data-runtime','before-throw');throw 17";

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
    let info = fs::symlink_metadata(path)?;
    require(
        info.is_file() && !info.file_type().is_symlink() && info.len() <= LOG_LIMIT,
        "Unsafe or oversized browser log",
    )?;
    let body = fs::read(path)?;
    require(
        body.len() as u64 <= LOG_LIMIT,
        "Browser log grew beyond bound",
    )?;
    Ok(String::from_utf8(body)?)
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
    runtime_enabled: bool,
    traffic: Rc<Traffic>,
}
impl Client {
    fn connect(
        endpoint: &url::Url,
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
                && matches!(
                    endpoint.path(),
                    "/devtools/page/page-1" | "/devtools/browser/browser-1"
                ),
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
            runtime_enabled: false,
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
    fn newer_edit_batch(&mut self, context: u64) -> Result<()> {
        // This one fixed proof primitive, not a general multi-flight mode.
        // Frame order is fixed; server admission can still split across ticks.
        let first = self.next;
        require(
            self.route == "direct"
                && self.pending.is_none()
                && first.checked_add(3).is_some_and(|last| last <= 256),
            "Ordering batch requires idle direct route and four available IDs",
        )?;
        let requests = [
            json!({"id":first,"method":"Runtime.callFunctionOn","params":{"executionContextId":context,"functionDeclaration":LOOP_SOURCE,"arguments":[],"returnByValue":false}}),
            json!({"id":first+1,"method":"Runtime.evaluate","params":{"expression":"1","contextId":context}}),
            json!({"id":first+2,"method":"Input.insertText","params":{"text":"-newer"}}),
            json!({"id":first+3,"method":"Runtime.evaluate","params":{"expression":"1","contextId":context}}),
        ];
        let texts = requests
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let serialized_bytes: usize = texts.iter().map(String::len).sum();
        require(
            serialized_bytes + 4 * 14 < 64 * 1024
                && self.socket.get_config().write_buffer_size >= 64 * 1024,
            "Four serialized masked frames must fit below existing buffer threshold",
        )?;
        self.next += 4;
        for request in &requests {
            emit(json!({"protocol":"send","route":self.route,"message":request}))?;
        }
        self.timeout()?;
        for text in texts {
            self.socket.write(Message::Text(text.into()))?;
        }
        self.socket.flush()?; // All four frames written before the first read.
        for (position, expected) in [first + 1, first + 2, first + 3, first]
            .into_iter()
            .enumerate()
        {
            let reply = loop {
                let reply = self.read()?;
                if reply.get("id").is_none() {
                    self.event(reply)?;
                    continue;
                }
                break reply;
            };
            require(
                reply["id"].as_u64() == Some(expected)
                    && reply.get("sessionId").is_none()
                    && reply.get("result").is_some() != reply.get("error").is_some(),
                "Ordering batch reply IDs/routes/order must be exact and unique",
            )?;
            match position {
                0 | 2 => {
                    protocol_failure(&reply, -32000)?;
                    require(
                        reply["error"]["message"]
                            .as_str()
                            .is_some_and(|message| message.contains("pending")),
                        "Concurrent edit ordering not witnessed; no retry or cap expansion",
                    )?;
                    let phase = if position == 0 {
                        "before-input"
                    } else {
                        "after-input"
                    };
                    emit(
                        json!({"phase":"pending-barrier","position":phase,"request_id":expected,"call_id":first,"error":reply["error"],"fixed_iterations":10000,"ordering_method":ORDERING_METHOD}),
                    )?;
                }
                1 => require(
                    reply == json!({"id":expected,"result":{}}),
                    "Ordering batch input must acknowledge exactly before Call completion",
                )?,
                3 => {
                    require(
                        reply.get("error").is_none(),
                        "Finite concurrent call failed",
                    )?;
                    value(&reply["result"], "number", json!(10000.0))?;
                }
                _ => unreachable!(),
            }
        }
        Ok(())
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
    fn eval(
        &mut self,
        source: &str,
        context: u64,
        group: Option<&str>,
        session: Option<&str>,
    ) -> Result<Value> {
        let mut params = json!({"expression":source,"contextId":context,"returnByValue":false});
        if let Some(group) = group {
            params["objectGroup"] = json!(group);
        }
        self.rpc("Runtime.evaluate", params, session)
    }
    fn context(&mut self, session: Option<&str>) -> Result<u64> {
        let before = self.events.len();
        self.rpc("Runtime.enable", json!({}), session)?;
        if !self.runtime_enabled {
            require(
                !self.events[before..].iter().any(|event| {
                    event["method"] == "Runtime.executionContextCreated"
                        && event.get("sessionId").and_then(Value::as_str) == session
                }),
                "First Runtime.enable acknowledgment must precede contextCreated",
            )?;
        }
        self.runtime_enabled = true;
        loop {
            if let Some(event) = self.events.iter().rev().find(|event| {
                event["method"] == "Runtime.executionContextCreated"
                    && event.get("sessionId").and_then(Value::as_str) == session
            }) {
                let context = &event["params"]["context"];
                let id = context["id"]
                    .as_u64()
                    .filter(|id| *id > 0 && *id <= i32::MAX as u64)
                    .ok_or("Missing bounded actual main-world context ID")?;
                require(
                    context["origin"] == "http://127.0.0.1:7878"
                        && context["name"] == ""
                        && context["uniqueId"] == format!("mg-context-{id}")
                        && context["auxData"]
                            == json!({"isDefault":true,"type":"default","frameId":"frame-1"}),
                    "Actual context origin/default/target-frame/unique identity differs",
                )?;
                return Ok(id);
            }
            // The first enable reply precedes its notification. Read the real
            // bounded socket event; do not synthesize readiness or poll DOM.
            let event = self.read()?;
            self.event(event)?;
        }
    }
    fn capture(&mut self) -> Result<(image::RgbImage, Vec<u8>)> {
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
            "PNG dimension bound",
        )?;
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.to_rgb8();
        Ok((image, bytes))
    }
    fn screenshot(&mut self, path: &Path) -> Result<image::RgbImage> {
        let (image, bytes) = self.capture()?;
        write_new(path, &bytes)?;
        Ok(image)
    }
    fn input_center(&mut self) -> Result<(i16, i16)> {
        let root = self.rpc("DOM.getDocument", json!({}), None)?["root"]["nodeId"].clone();
        let node = self.rpc(
            "DOM.querySelector",
            json!({"nodeId":root,"selector":"#query"}),
            None,
        )?["nodeId"]
            .clone();
        require(
            node.as_u64().is_some_and(|v| v > 0),
            "Missing real query input",
        )?;
        let model = self.rpc("DOM.getBoxModel", json!({"nodeId":node}), None)?;
        let quad = model
            .pointer("/model/content")
            .and_then(Value::as_array)
            .filter(|v| v.len() == 8)
            .ok_or("Missing input quad")?;
        let numbers = quad
            .iter()
            .map(|v| {
                v.as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or("Invalid input quad")
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let x = (numbers[0] + numbers[2] + numbers[4] + numbers[6]) / 4.0;
        let y = (numbers[1] + numbers[3] + numbers[5] + numbers[7]) / 4.0;
        require(
            x >= 0.0 && x < 1900.0 && y >= 0.0 && y < 950.0,
            "Input outside private window",
        )?;
        Ok((x as i16, y as i16 + 64))
    }
    fn load(&mut self, url: &str) -> Result<()> {
        while !self
            .events
            .iter()
            .any(|event| event["method"] == "Page.frameStoppedLoading")
        {
            let event = self.read()?;
            self.event(event)?;
        }
        let page: Vec<_> = self
            .events
            .iter()
            .filter(|event| {
                event["method"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("Page."))
            })
            .collect();
        let methods: Vec<_> = page
            .iter()
            .filter_map(|event| event["method"].as_str())
            .collect();
        require(
            methods
                == [
                    "Page.frameStartedLoading",
                    "Page.frameNavigated",
                    "Page.domContentEventFired",
                    "Page.loadEventFired",
                    "Page.frameStoppedLoading",
                ],
            "Actual lifecycle order/repetition differs",
        )?;
        require(
            page[1].pointer("/params/frame/url").and_then(Value::as_str) == Some(url),
            "Wrong committed destination",
        )?;
        require(
            self.rpc("Page.getFrameTree", json!({}), None)?
                .pointer("/frameTree/frame/url")
                .and_then(Value::as_str)
                == Some(url),
            "Wrong actual destination frame",
        )?;
        Ok(())
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
    fn click(&self, x: i16, y: i16) -> Result<()> {
        remaining(self.deadline)?;
        for (response_type, mask, state) in [
            (
                BUTTON_PRESS_EVENT,
                EventMask::BUTTON_PRESS,
                KeyButMask::default(),
            ),
            (
                BUTTON_RELEASE_EVENT,
                EventMask::BUTTON_RELEASE,
                KeyButMask::BUTTON1,
            ),
        ] {
            self.conn.send_event(
                false,
                self.window,
                mask,
                ButtonPressEvent {
                    response_type,
                    detail: 1,
                    sequence: 0,
                    time: 0,
                    root: self.root,
                    event: self.window,
                    child: 0,
                    root_x: x,
                    root_y: y,
                    event_x: x,
                    event_y: y,
                    state,
                    same_screen: true,
                },
            )?;
        }
        self.conn.flush()?;
        Ok(())
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
fn paint(
    native: &Native,
    client: &mut Client,
    output: &Path,
    label: &str,
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
        if native.title("Runtime native proof")? {
            let image = native.frame()?;
            if pixels(&image, 64, color) {
                save_native(&image, &output.join(format!("native-{label}.png")))?;
                let cdp = client.screenshot(&output.join(format!("cdp-{label}.png")))?;
                require(
                    pixels(&cdp, 0, color),
                    "CDP literal paint differs from Runtime mutation",
                )?;
                require(
                    image.width() == cdp.width() && image.height() == cdp.height() + 93,
                    "Native/CDP1x viewport/chrome mismatch",
                )?;
                emit(
                    json!({"phase":"paint","label":label,"color":color,"literal_probes":4,"native_page_top":64,"scale":1}),
                )?;
                return Ok(());
            }
            last = Some(image);
        }
        thread::sleep(remaining(native.deadline)?.min(Duration::from_millis(25)));
    }
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
fn call(
    client: &mut Client,
    receiver: &str,
    source: &str,
    arguments: Value,
    session: Option<&str>,
) -> Result<Value> {
    client.rpc("Runtime.callFunctionOn",json!({"objectId":receiver,"functionDeclaration":source,"arguments":arguments,"returnByValue":false}),session)
}

fn run(
    endpoint: url::Url,
    window: u32,
    log: &Path,
    output: &Path,
    deadline: Instant,
) -> Result<()> {
    fs::create_dir(output)?;
    let native = Native::new(window, deadline)?;
    // These are the actual accepted lifecycle reports, not a DOM readiness flag.
    loop {
        let text = read_log(log)?;
        let tasks: Vec<_> = text
            .lines()
            .filter(|line| line.starts_with("PAGE_TASK_APPLIED "))
            .collect();
        if tasks
            == [
                "PAGE_TASK_APPLIED sequence=1 revision=1",
                "PAGE_TASK_APPLIED sequence=2 revision=2",
            ]
            && native.title("Retained event fixture")?
        {
            break;
        }
        remaining(deadline)?;
        thread::sleep(Duration::from_millis(10));
    }
    let traffic = Rc::new(Traffic::default());
    let mut direct = Client::connect(&endpoint, "direct", deadline, traffic.clone())?;
    direct.rpc("Page.enable", json!({}), None)?;
    require(
        direct
            .rpc("Page.getFrameTree", json!({}), None)?
            .pointer("/frameTree/frame/url")
            .and_then(Value::as_str)
            == Some(FIXTURE),
        "Wrong initial real fixture",
    )?;
    let context = direct.context(None)?; // Old public19 must fail here normally.
    let mut browser_url = endpoint.clone();
    browser_url.set_path("/devtools/browser/browser-1");
    let mut flat = Client::connect(&browser_url, "flattened", deadline, traffic.clone())?;
    let attached = flat.rpc(
        "Target.attachToTarget",
        json!({"targetId":"page-1","flatten":true}),
        None,
    )?;
    let session = attached["sessionId"]
        .as_str()
        .ok_or("Missing flattened session")?
        .to_owned();
    require(
        flat.context(Some(&session))? == context,
        "Direct/flattened main context differ",
    )?;
    emit(
        json!({"phase":"contexts","context":context,"target":"page-1","session":session,"groups":1}),
    )?;

    let (x, y) = direct.input_center()?;
    native.click(x, y)?;
    native.key(b'a'.into(), true)?;
    for byte in b"native-edit" {
        native.key((*byte).into(), false)?;
    }
    thread::sleep(remaining(deadline)?.min(Duration::from_millis(150)));
    value(
        &direct.eval(
            "document.getElementById('query').value",
            context,
            None,
            None,
        )?,
        "string",
        json!("native-edit"),
    )?;
    emit(
        json!({"phase":"native-input","value":"native-edit","source":"X11 application input; not independent physical hardware","groups":2}),
    )?;

    let rooted=direct.eval("globalThis.runtimeReported=0;window.onerror=function(){runtimeReported++;};globalThis.rt=(()=>{let n=40;return {tag:'native-runtime',inc:function(x){n+=x;return n;}}})()",context,Some("direct-proof"),None)?;
    let root = object(&rooted)?;
    value(
        &call(
            &mut direct,
            &root,
            "function(alias){if(this!==alias)throw 19;return this.inc(2);}",
            json!([{"objectId":root}]),
            None,
        )?,
        "number",
        json!(42.0),
    )?;
    let properties = direct.rpc(
        "Runtime.getProperties",
        json!({"objectId":root,"ownProperties":true}),
        None,
    )?;
    let rows = properties["result"]
        .as_array()
        .ok_or("Missing complete own descriptors")?;
    require(
        rows.len() == 2
            && rows[0]["name"] == "tag"
            && rows[0]["value"]["value"] == "native-runtime"
            && rows[1]["name"] == "inc"
            && rows.iter().all(|r| {
                r["isOwn"] == true
                    && r["enumerable"] == true
                    && r["configurable"] == true
                    && r["writable"] == true
            }),
        "Actual ordinary descriptors differ",
    )?;
    let function = rows[1]["value"]["objectId"]
        .as_str()
        .ok_or("Missing real function handle")?
        .to_owned();
    value(
        &call(
            &mut direct,
            &function,
            "function(){return this(1);}",
            json!([]),
            None,
        )?,
        "number",
        json!(43.0),
    )?;
    emit(
        json!({"phase":"closures-handles","alias_identity":true,"closure_value":43,"complete_own_properties":2,"groups":3}),
    )?;
    value(
        &direct.eval(PANEL, context, None, None)?,
        "number",
        json!(42.0),
    )?;
    paint(&native, &mut direct, output, "blue", [0x12, 0x34, 0x56])?;

    let exception = direct.eval(BROWN_THROW, context, None, None)?;
    require(
        exception["exceptionDetails"]["exception"]["type"] == "number"
            && exception["exceptionDetails"]["exception"]["value"].as_f64() == Some(17.0)
            && exception["result"]["value"].as_f64() == Some(17.0),
        "Expected ordinary caught primitive exception",
    )?;
    value(&direct.eval("document.getElementById('state').getAttribute('data-runtime')+':'+runtimeReported+':'+rt.inc(0)",context,None,None)?,"string",json!("before-throw:0:43"))?;
    emit(
        json!({"phase":"exception-effects","ordinary_exception":17,"page_onerror_calls":0,"groups":5}),
    )?;

    value(
        &flat.eval("rt.inc(1)", context, None, Some(&session))?,
        "number",
        json!(44.0),
    )?;
    let foreign = flat.raw(
        "Runtime.getProperties",
        json!({"objectId":root,"ownProperties":true}),
        Some(&session),
    )?;
    protocol_failure(&foreign, -32000)?;
    let flat_root = object(&flat.eval("rt", context, Some("flattened-proof"), Some(&session))?)?;
    value(
        &call(
            &mut flat,
            &flat_root,
            "function(){return this.inc(1);}",
            json!([]),
            Some(&session),
        )?,
        "number",
        json!(45.0),
    )?;
    flat.rpc(
        "Runtime.releaseObjectGroup",
        json!({"objectGroup":"flattened-proof"}),
        Some(&session),
    )?;
    direct.rpc("Runtime.releaseObject", json!({"objectId":function}), None)?;
    protocol_failure(
        &direct.raw(
            "Runtime.getProperties",
            json!({"objectId":function,"ownProperties":true}),
            None,
        )?,
        -32000,
    )?;
    direct.rpc(
        "Runtime.releaseObjectGroup",
        json!({"objectGroup":"direct-proof"}),
        None,
    )?;
    protocol_failure(
        &direct.raw(
            "Runtime.getProperties",
            json!({"objectId":root,"ownProperties":true}),
            None,
        )?,
        -32000,
    )?;
    value(
        &direct.eval("rt.inc(0)", context, None, None)?,
        "number",
        json!(45.0),
    )?;
    // Keep a newly issued, still-live handle through navigation. The earlier
    // released IDs prove release only, not invalidation of a live old context.
    let navigation_root = object(&direct.eval("rt", context, Some("navigation-proof"), None)?)?;
    emit(
        json!({"phase":"routing-release","same_realm_value":45,"cross_owner_rejected":true,"released_objects_rejected":true,"navigation_live_handle_retained":true,"groups":6}),
    )?;

    // One fixed batch, no timing retry. Both busy replies and the input ACK
    // must actually precede Call completion; queued bytes alone prove nothing.
    direct.newer_edit_batch(context)?;
    value(
        &direct.eval(
            "document.getElementById('query').value",
            context,
            None,
            None,
        )?,
        "string",
        json!("native-edit-newer"),
    )?;
    emit(
        json!({"phase":"newer-edit","before_completion":true,"value":"native-edit-newer","field_glyphs_require_visual_review":true,"ordering_method":ORDERING_METHOD,"groups":7}),
    )?;
    // No source since the caught exception changes the panel's brown style.
    // These existing images now also retain the final field for visual review;
    // literal panel pixels plus a Runtime string are not a field-glyph oracle.
    paint(&native, &mut direct, output, "brown", [0x65, 0x43, 0x21])?;

    direct.events.clear();
    direct.event_bytes = 0;
    direct.rpc("Page.navigate", json!({"url":DESTINATION}), None)?;
    direct.load(DESTINATION)?;
    let next = direct.context(None)?;
    let context_events: Vec<_> = direct
        .events
        .iter()
        .filter(|event| {
            matches!(
                event["method"].as_str(),
                Some("Runtime.executionContextDestroyed" | "Runtime.executionContextCreated")
            )
        })
        .collect();
    require(
        next != context
            && context_events.len() == 2
            && context_events[0]["method"] == "Runtime.executionContextDestroyed"
            && context_events[0].get("sessionId").is_none()
            && context_events[0]["params"]["executionContextId"] == context
            && context_events[0]["params"]["executionContextUniqueId"]
                == format!("mg-context-{context}")
            && context_events[1]["method"] == "Runtime.executionContextCreated"
            && context_events[1].get("sessionId").is_none()
            && context_events[1]["params"]["context"]["id"] == next,
        "Navigation must destroy the exact old context before creating one new context",
    )?;
    protocol_failure(
        &direct.raw(
            "Runtime.evaluate",
            json!({"expression":"1","contextId":context}),
            None,
        )?,
        -32000,
    )?;
    protocol_failure(
        &direct.raw(
            "Runtime.getProperties",
            json!({"objectId":navigation_root,"ownProperties":true}),
            None,
        )?,
        -32000,
    )?;
    value(
        &direct.eval("typeof rt+':'+document.title", next, None, None)?,
        "string",
        json!("undefined:Event destination"),
    )?;
    let (destination, destination_bytes) = direct.capture()?;
    loop {
        remaining(deadline)?;
        if native.title("Event destination")? {
            let image = native.frame()?;
            require(
                image.width() == destination.width() && image.height() == destination.height() + 93,
                "Destination viewport differs",
            )?;
            let crop =
                image::imageops::crop_imm(&image, 0, 64, destination.width(), destination.height())
                    .to_image();
            if crop.as_raw() == destination.as_raw() {
                write_new(&output.join("cdp-destination.png"), &destination_bytes)?;
                save_native(&image, &output.join("native-destination.png"))?;
                break;
            }
        }
        thread::sleep(remaining(deadline)?.min(Duration::from_millis(25)));
    }
    emit(
        json!({"phase":"navigation","old_context":context,"new_context":next,"runtime_events":["Runtime.executionContextDestroyed","Runtime.executionContextCreated"],"origin":"http://127.0.0.1:7878","frame":"frame-1","stale_context_rejected":true,"stale_object_rejected":true,"groups":8}),
    )?;
    direct.timeout()?;
    let _ = direct.socket.close(None);
    flat.timeout()?;
    let _ = flat.socket.close(None);
    remaining(deadline)?;
    emit(
        json!({"phase":"runtime-proof","groups":8,"images":6,"native_input":true,"native_and_cdp_paint":true,"real_worker_closure":true,"measured_newer_edit":true,"ordering_method":ORDERING_METHOD,"newer_field_glyphs_automatically_verified":false,"newer_field_images":["native-brown.png","cdp-brown.png"],"direct_and_flattened":true,"target_moves":false,"target_move_scope":"not part of this frozen core; existing workspace proof is separate","shared_messages":traffic.messages.get(),"shared_reply_bytes":traffic.bytes.get(),"fixed_loop_iterations":10000}),
    )?;
    println!("RUNTIME_SMOKE_OK");
    Ok(())
}
fn main() -> Result<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(28);
    let args: Vec<_> = std::env::args().skip(1).collect();
    require(
        args.len() == 4,
        "Usage: runtime_smoke <direct-page-ws> <owned-window> <browser.log> <NEW-frames-dir>",
    )?;
    let endpoint = url::Url::parse(&args[0])?;
    let window = args[1].parse::<u32>()?;
    require(window > 0, "Invalid owned window")?;
    let result = run(
        endpoint,
        window,
        Path::new(&args[2]),
        &PathBuf::from(&args[3]),
        deadline,
    );
    require(
        started.elapsed() < Duration::from_secs(30),
        "Runtime client exceeded30s whole bound",
    )?;
    result
}
