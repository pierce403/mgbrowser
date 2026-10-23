//! Authored installed-binary CSSOM method proof on one owned 1x display.
//! The Python owner supplies the exact native window, loopback page endpoint,
//! browser log and NEW evidence directory. This client never renders or spawns.
//! Uses real native X11 input and the existing external CDP subset, never Runtime.
use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Cursor, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig};
use x11rb::{connection::Connection, protocol::xproto::*, rust_connection::RustConnection};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const BASE: &str = "http://127.0.0.1:7878";
const FIXTURE: &str = "http://127.0.0.1:7878/cssom-methods";
const TITLE: &str = "CSSOM methods fixture";
const EVENT: &str =
    "PAGE_EVENT_APPLIED sequence=3 revision=3 click_canceled=Some(false) submit_canceled=None";
const RED: [u8; 3] = [0xaa, 0, 0];
const GREEN: [u8; 3] = [0, 0xaa, 0];
const BLUE: [u8; 3] = [0, 0, 0xaa];
const WHITE: [u8; 3] = [255; 3];
const LOG_LIMIT: u64 = 1024 * 1024;

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
        .filter(|d| !d.is_zero())
        .ok_or_else(|| failure("CSSOM methods client reached its 28-second work cutoff"))
}
fn emit(value: Value) -> Result<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

struct Client {
    socket: WebSocket<TcpStream>,
    deadline: Instant,
    next_id: u64,
    events: Vec<Value>,
    event_bytes: usize,
    received: usize,
    received_bytes: usize,
}
impl Client {
    fn connect(endpoint: &url::Url, deadline: Instant) -> Result<Self> {
        require(
            endpoint.scheme() == "ws"
                && endpoint.host_str() == Some("127.0.0.1")
                && endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.query().is_none()
                && endpoint.fragment().is_none()
                && endpoint.path() == "/devtools/page/page-1",
            "Expected owned direct-page loopback endpoint",
        )?;
        let port = endpoint
            .port()
            .filter(|port| *port != 0)
            .ok_or_else(|| failure("Missing CDP port"))?;
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
            deadline,
            next_id: 1,
            events: Vec::new(),
            event_bytes: 0,
            received: 0,
            received_bytes: 0,
        })
    }
    fn timeout(&mut self) -> Result<()> {
        let duration = remaining(self.deadline)?;
        self.socket.get_mut().set_read_timeout(Some(duration))?;
        self.socket.get_mut().set_write_timeout(Some(duration))?;
        Ok(())
    }
    fn read(&mut self) -> Result<Value> {
        loop {
            self.timeout()?;
            match self.socket.read()? {
                Message::Text(text) => {
                    self.received += 1;
                    self.received_bytes += text.len();
                    require(
                        self.received <= 4096 && self.received_bytes <= 24 * 1024 * 1024,
                        "CDP aggregate reply bound exceeded",
                    )?;
                    let value: Value = serde_json::from_str(text.as_str())?;
                    require(
                        value.is_object() && value.get("sessionId").is_none(),
                        "Unexpected CDP response/session",
                    )?;
                    return Ok(value);
                }
                Message::Ping(_) | Message::Pong(_) => {
                    self.socket.flush()?;
                }
                _ => return Err(failure("CDP closed or returned a non-JSON frame")),
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
            "CDP event queue bound exceeded",
        )?;
        self.events.push(value);
        Ok(())
    }
    fn command(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.timeout()?;
        self.socket.send(Message::Text(
            json!({"id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))?;
        loop {
            let value = self.read()?;
            if value.get("id").is_none() {
                self.event(value)?;
                continue;
            }
            require(
                value["id"].as_u64() == Some(id)
                    && value.get("error").is_none()
                    && value.get("result").is_some(),
                "CDP command failed or returned an unmatched response",
            )?;
            let result = value["result"].clone();
            require(
                result.get("errorText").is_none(),
                "CDP navigation returned errorText",
            )?;
            return Ok(result);
        }
    }
    fn frame_url(&mut self, expected: &str) -> Result<()> {
        let value = self.command("Page.getFrameTree", json!({}))?;
        require(
            value
                .pointer("/frameTree/frame/url")
                .and_then(Value::as_str)
                == Some(expected),
            "Wrong actual frame URL",
        )
    }
    fn clear_events(&mut self) {
        self.events.clear();
        self.event_bytes = 0;
    }
    fn load(&mut self, expected: &str) -> Result<()> {
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
            .filter_map(|event| {
                event["method"]
                    .as_str()
                    .filter(|method| method.starts_with("Page."))
                    .map(|method| (method, event))
            })
            .collect();
        let methods: Vec<_> = page
            .iter()
            .map(|(method, _)| (*method).to_owned())
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
            "Missing/repeated/out-of-order actual CDP lifecycle",
        )?;
        require(
            page[1]
                .1
                .pointer("/params/frame/url")
                .and_then(Value::as_str)
                == Some(expected),
            "CDP committed a different URL",
        )?;
        let dcl = page[2]
            .1
            .pointer("/params/timestamp")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite());
        let load = page[3]
            .1
            .pointer("/params/timestamp")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite());
        require(
            dcl.zip(load)
                .is_some_and(|(dcl, load)| dcl > 0.0 && load >= dcl),
            "Invalid lifecycle timestamps",
        )?;
        self.frame_url(expected)?;
        emit(json!({"phase":"cdp-load","url":expected,"events":methods}))?;
        self.clear_events();
        Ok(())
    }
    fn screenshot(&mut self, path: &Path) -> Result<image::RgbImage> {
        let value = self.command("Page.captureScreenshot", json!({"format":"png"}))?;
        let bytes = STANDARD.decode(
            value["data"]
                .as_str()
                .ok_or_else(|| failure("Missing PNG"))?,
        )?;
        require(
            bytes.len() <= 8 * 1024 * 1024 && bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "Invalid/oversized PNG",
        )?;
        let (width, height) =
            image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Png)
                .into_dimensions()?;
        require(
            width > 0 && height > 0 && width <= 1920 && height <= 1080,
            "PNG dimensions exceed owned display",
        )?;
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.to_rgb8();
        write_new(path, &bytes)?;
        remaining(self.deadline)?;
        Ok(image)
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}
fn read_log(path: &Path) -> Result<String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    require(
        meta.is_file() && meta.len() <= LOG_LIMIT,
        "Unsafe or oversized browser log",
    )?;
    let mut bytes = Vec::new();
    file.take(LOG_LIMIT + 1).read_to_end(&mut bytes)?;
    require(bytes.len() as u64 <= LOG_LIMIT, "Browser log overflow")?;
    Ok(String::from_utf8(bytes)?)
}
fn reports(text: &str) -> Result<Vec<Value>> {
    text.lines()
        .filter_map(|line| line.strip_prefix("SCRIPT_BOA "))
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}
fn fixture_source_bytes() -> u64 {
    // Exactly one authored inline source, including its literal leading LF.
    let fixture = include_str!("../tests/fixtures/layout/cssom-methods.html");
    fixture
        .split_once("<script>")
        .expect("frozen source start")
        .1
        .split_once("</script>")
        .expect("frozen source end")
        .0
        .len() as u64
}
fn work(report: &Value, executed: u64, pending: u64, scripted: bool) -> Result<()> {
    require(
        report["profile"] == "boa-page-process-v1"
            && report["source_bytes"] == if scripted { fixture_source_bytes() } else { 0 }
            && report["source_admissions"] == u64::from(scripted)
            && report["jobs_admitted"] == 2
            && report["jobs_executed"] == executed
            && report["pending_jobs"] == pending
            && report.get("fatal_reason").is_some_and(Value::is_null),
        "Unexpected actual CSSOM/source/lifecycle worker accounting",
    )
}
fn load_count(log: &str) -> usize {
    log.lines()
        .filter(|line| line.starts_with("LOADED "))
        .count()
}
fn complete(log: &str, expected: &str, title: &str, before: usize) -> Result<bool> {
    require(
        ![
            "SCRIPT_PARTIAL",
            "SCRIPT_DIAGNOSTIC",
            "SCRIPT_ERROR",
            "SCRIPT_REJECTED",
            "SCRIPT_SESSION_CLOSED",
            "SCRIPT_SHUTDOWN_ERROR",
            "LOAD_ERROR",
            "RESOURCE_WARNING",
            "RESOURCE_DIAGNOSTIC",
            "JOURNEY_INCOMPLETE",
        ]
        .iter()
        .any(|marker| log.contains(marker)),
        "Actual script/resource failure in authored CSSOM journey",
    )?;
    let Some((_, tail)) = log.rsplit_once("NAVIGATE GET ") else {
        return Ok(false);
    };
    if tail.lines().next() != Some(expected) || load_count(log) <= before {
        return Ok(false);
    }
    let prefix = format!("LOADED {expected} HTTP 200 title={title:?} ");
    if !tail.lines().any(|line| line.starts_with(&prefix)) {
        return Ok(false);
    }
    let values = reports(tail)?;
    let tasks: Vec<_> = tail
        .lines()
        .filter(|line| line.starts_with("PAGE_TASK_APPLIED "))
        .collect();
    if values.len() < 3 || tasks.len() < 2 {
        return Ok(false);
    }
    require(
        values.len() == 3
            && tasks
                == [
                    "PAGE_TASK_APPLIED sequence=1 revision=1",
                    "PAGE_TASK_APPLIED sequence=2 revision=2",
                ]
            && !tail
                .lines()
                .any(|line| line.starts_with("PAGE_EVENT_APPLIED ")),
        "Unexpected work before click",
    )?;
    for (index, value) in values.iter().enumerate() {
        work(value, index as u64, 2 - index as u64, expected == FIXTURE)?;
    }
    Ok(true)
}
fn click_evidence(before: &str, after: &str, destination: &str) -> Result<()> {
    let suffix = after
        .strip_prefix(before)
        .ok_or_else(|| failure("Browser log changed instead of appending"))?;
    let (event, navigation) = suffix
        .split_once("NAVIGATE GET ")
        .ok_or_else(|| failure("Click did not navigate"))?;
    require(
        navigation.lines().next() == Some(destination),
        "Wrong native/CDP hit target",
    )?;
    let values = reports(event)?;
    require(
        values.len() == 1
            && event
                .lines()
                .filter(|line| line.starts_with("PAGE_EVENT_APPLIED "))
                .collect::<Vec<_>>()
                == [EVENT]
            && !event
                .lines()
                .any(|line| line.starts_with("PAGE_TASK_APPLIED ")),
        "Missing/repeated real click reply",
    )?;
    work(&values[0], 2, 0, true)
}

struct Native {
    conn: RustConnection,
    root: u32,
    window: u32,
    scale: u32,
    deadline: Instant,
}
impl Native {
    fn new(window: u32, scale: u32, deadline: Instant) -> Result<Self> {
        remaining(deadline)?;
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        require(
            conn.setup().image_byte_order == ImageOrder::LSB_FIRST,
            "Unsupported owned Xvfb image order",
        )?;
        Ok(Self {
            conn,
            root,
            window,
            scale,
            deadline,
        })
    }
    fn frame(&self) -> Result<image::RgbImage> {
        remaining(self.deadline)?;
        let geometry = self.conn.get_geometry(self.window)?.reply()?;
        require(
            geometry.depth == 24
                && geometry.width > 0
                && u32::from(geometry.height) > 93 * self.scale
                && geometry.width <= 1920
                && geometry.height <= 1080,
            "Unexpected owned native geometry",
        )?;
        require(
            self.conn.setup().pixmap_formats.iter().any(|format| {
                format.depth == 24 && format.bits_per_pixel == 32 && format.scanline_pad == 32
            }),
            "Unexpected native pixel format",
        )?;
        remaining(self.deadline)?;
        let image = self
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
            .reply()?;
        require(
            image.data.len() == usize::from(geometry.width) * usize::from(geometry.height) * 4,
            "Native image length mismatch",
        )?;
        let rgb = image
            .data
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        remaining(self.deadline)?;
        image::RgbImage::from_raw(geometry.width.into(), geometry.height.into(), rgb)
            .ok_or_else(|| failure("Invalid native frame"))
    }
    fn title(&self, expected: &str) -> Result<bool> {
        remaining(self.deadline)?;
        let title = self
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
        remaining(self.deadline)?;
        Ok(String::from_utf8_lossy(&title.value).contains(expected))
    }
    fn click(&self, x: i16, y: i16) -> Result<()> {
        remaining(self.deadline)?;
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
                    state: KeyButMask::default(),
                    same_screen: true,
                },
            )?;
        }
        self.conn.flush()?;
        remaining(self.deadline)?;
        Ok(())
    }
}

// Literal border rectangles and paint probes frozen before running this client.
// Authored expectations, never derived from layout or queried BoxModel.
const BOXES: [(&str, [u32; 4]); 3] = [
    ("native-link", [20, 10, 120, 20]),
    ("cdp-link", [20, 40, 80, 20]),
    ("ready", [20, 70, 160, 10]),
];
const PROBES: &[(u32, u32, [u8; 3])] = &[
    (19, 20, WHITE),
    (21, 11, RED),
    (80, 20, RED),
    (138, 28, RED),
    (140, 20, WHITE),
    (30, 35, WHITE),
    (19, 50, WHITE),
    (21, 41, GREEN),
    (60, 50, GREEN),
    (98, 58, GREEN),
    (100, 50, WHITE),
    (30, 65, WHITE),
    (19, 75, WHITE),
    (21, 71, BLUE),
    (100, 75, BLUE),
    (178, 78, BLUE),
    (180, 75, WHITE),
    (30, 85, WHITE),
];
fn pixel(image: &image::RgbImage, x: u32, y: u32, expected: [u8; 3]) -> bool {
    x < image.width() && y < image.height() && image.get_pixel(x, y).0 == expected
}
fn fixture_pixels(image: &image::RgbImage, top: u32, scale: u32) -> bool {
    image.width() >= 200 * scale
        && image.height() >= (top + 100) * scale
        && PROBES
            .iter()
            .all(|&(x, y, color)| pixel(image, x * scale, (y + top) * scale, color))
}
fn save_native(image: &image::RgbImage, path: &Path) -> Result<()> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png)?;
    require(
        bytes.get_ref().len() <= 8 * 1024 * 1024,
        "Native PNG exceeded bound",
    )?;
    write_new(path, bytes.get_ref())
}
fn failure_probes(image: &image::RgbImage, color: Option<[u8; 3]>, scale: u32) -> Vec<Value> {
    let mut probes = Vec::with_capacity(PROBES.len());
    let mut add = |x, y, expected: [u8; 3]| {
        let x = x * scale;
        let y = (y + 64) * scale;
        let actual = (x < image.width() && y < image.height()).then(|| image.get_pixel(x, y).0);
        probes.push(json!({"x":x,"y":y,"expected_rgb":expected,"actual_rgb":actual}));
    };
    if let Some(color) = color {
        add(10, 10, color);
        add(170, 70, color);
    } else {
        for &(x, y, color) in PROBES {
            add(x, y, color);
        }
    }
    probes
}
fn wait_native(
    native: &Native,
    log: &Path,
    url: &str,
    title: &str,
    before: usize,
    color: Option<[u8; 3]>,
    output: &Path,
) -> Result<(String, image::RgbImage)> {
    // Keep only the last mismatching frame after this navigation's exact
    // lifecycle and native title gates. Failure recording performs no X11 I/O.
    let mut last = None;
    let result: Result<(String, image::RgbImage)> = (|| {
        loop {
            remaining(native.deadline)?;
            let text = read_log(log)?;
            if complete(&text, url, title, before)? && native.title(title)? {
                let image = native.frame()?;
                let correct = color.map_or_else(
                    || fixture_pixels(&image, 64, native.scale),
                    |color| {
                        pixel(&image, 10 * native.scale, 74 * native.scale, color)
                            && pixel(&image, 170 * native.scale, 134 * native.scale, color)
                    },
                );
                if correct {
                    return Ok((text, image));
                }
                last = Some(image);
            }
            thread::sleep(remaining(native.deadline)?.min(Duration::from_millis(25)));
        }
    })();
    if let Err(error) = result {
        let error_summary: String = error.to_string().chars().take(512).collect();
        let diagnostic = (|| -> Result<()> {
            if let Some(image) = last {
                save_native(&image, &output.join("failure-native.png"))?;
                emit(
                    json!({"phase":"native-wait-failure","url":url,"title":title,
                    "correlated_native_frame":true,"observed_pixel_mismatch":true,
                    "failure_class":"pixel-wait-failed-with-correlated-frame",
                    "image":"failure-native.png","width":image.width(),"height":image.height(),
                    "probes":failure_probes(&image,color,native.scale),"error":error_summary}),
                )?;
            } else {
                emit(
                    json!({"phase":"native-wait-failure","url":url,"title":title,
                    "correlated_native_frame":false,"observed_pixel_mismatch":false,
                    "failure_class":"startup-or-native-state-unknown","error":error_summary}),
                )?;
            }
            Ok(())
        })();
        return match diagnostic {
            Ok(()) => Err(error),
            Err(record) => Err(failure(format!(
                "{error}; failure evidence could not be recorded: {record}"
            ))),
        };
    }
    result
}
fn scale_and_boxes(
    client: &mut Client,
    native: &image::RgbImage,
    image: &image::RgbImage,
    scale: u32,
) -> Result<()> {
    let metrics = client.command("Page.getLayoutMetrics", json!({}))?;
    let width = metrics
        .pointer("/cssLayoutViewport/clientWidth")
        .and_then(Value::as_u64);
    let height = metrics
        .pointer("/cssLayoutViewport/clientHeight")
        .and_then(Value::as_u64);
    require(
        width.and_then(|value| value.checked_mul(u64::from(scale))) == Some(image.width().into())
            && height.and_then(|value| value.checked_mul(u64::from(scale)))
                == Some(image.height().into())
            && metrics
                .pointer("/layoutViewport/clientWidth")
                .and_then(Value::as_u64)
                == Some(image.width().into())
            && metrics
                .pointer("/layoutViewport/clientHeight")
                .and_then(Value::as_u64)
                == Some(image.height().into())
            && image.width() == native.width()
            && image.height() + 93 * scale == native.height()
            && metrics
                .pointer("/cssVisualViewport/scale")
                .and_then(Value::as_f64)
                == Some(1.0)
            && metrics
                .pointer("/cssLayoutViewport/pageX")
                .and_then(Value::as_f64)
                == Some(0.0)
            && metrics
                .pointer("/cssLayoutViewport/pageY")
                .and_then(Value::as_f64)
                == Some(0.0),
        "Unexpected physical/logical scale, viewport, scroll or chrome placement",
    )?;
    let root = client.command("DOM.getDocument", json!({"depth":-1}))?["root"]["nodeId"]
        .as_u64()
        .ok_or_else(|| failure("No DOM root"))?;
    for (selector, attribute, expected) in [
        ("body", "data-cssom-ready", "load-methods-ready"),
        ("#native-link", "href", "/cssom-methods"),
        ("#cdp-link", "href", "/cssom-methods"),
    ] {
        let node = client.command(
            "DOM.querySelector",
            json!({"nodeId":root,"selector":selector}),
        )?["nodeId"]
            .as_u64()
            .filter(|id| *id != 0)
            .ok_or_else(|| failure("Missing actual ready/link node"))?;
        let attributes = client.command("DOM.getAttributes", json!({"nodeId":node}))?;
        let attributes = attributes["attributes"]
            .as_array()
            .ok_or_else(|| failure("Missing real attribute array"))?;
        require(
            attributes.len() % 2 == 0
                && attributes
                    .chunks_exact(2)
                    .any(|pair| pair[0] == attribute && pair[1] == expected),
            "Actual LoadComplete readiness or pre-handler href differs",
        )?;
    }
    for (name, rect) in BOXES {
        let selector = format!("#{name}");
        let node = client.command(
            "DOM.querySelector",
            json!({"nodeId":root,"selector":selector}),
        )?["nodeId"]
            .as_u64()
            .filter(|id| *id != 0)
            .ok_or_else(|| failure("Missing authored CSSOM box"))?;
        let model = client.command("DOM.getBoxModel", json!({"nodeId":node}))?;
        let [x, y, w, h] = rect;
        require(
            model.pointer("/model/border")
                == Some(&json!([x, y, x + w, y, x + w, y + h, x, y + h]))
                && model.pointer("/model/width").and_then(Value::as_u64) == Some(w.into())
                && model.pointer("/model/height").and_then(Value::as_u64) == Some(h.into()),
            &format!("Secondary DOM border box differs from literal oracle: {name}"),
        )?;
    }
    emit(
        json!({"phase":"fixture-pixels-boxes","rows":2,"boxes":3,"links":2,
        "pixel_probes":PROBES.len(),"width":image.width(),"height":image.height(),
        "logical_width":width,"logical_height":height,"native_page_top":64*scale,
        "scale":scale,"hidden_rendering_diagnostics_observed":false}),
    )
}

fn run(
    endpoint: url::Url,
    window: u32,
    log: &Path,
    output: &Path,
    scale: u32,
    deadline: Instant,
) -> Result<()> {
    fs::create_dir(output)?;
    let native = Native::new(window, scale, deadline)?;
    let mut client = Client::connect(&endpoint, deadline)?;
    client.command("Page.enable", json!({}))?;
    let (before, native_fixture) = wait_native(&native, log, FIXTURE, TITLE, 0, None, output)?;
    client.frame_url(FIXTURE)?;
    let native_page = image::imageops::crop_imm(
        &native_fixture,
        0,
        64 * scale,
        native_fixture.width(),
        native_fixture.height() - 93 * scale,
    )
    .to_image();
    scale_and_boxes(&mut client, &native_fixture, &native_page, scale)?;
    save_native(&native_fixture, &output.join("native-fixture.png"))?;
    // BoxModel is secondary; native input follows the actual uploaded X11 pixels.
    native.click((80 * scale) as i16, (84 * scale) as i16)?;
    let native_url = format!("{BASE}/cssom-methods-native-hit");
    let (after, native_destination) = wait_native(
        &native,
        log,
        &native_url,
        "CSSOM native hit",
        load_count(&before),
        Some([0x12, 0x34, 0x56]),
        output,
    )?;
    click_evidence(&before, &after, &native_url)?;
    save_native(&native_destination, &output.join("native-destination.png"))?;
    client.frame_url(&native_url)?; // Drain preceding native-navigation events before resetting the event ledger.
    client.clear_events();
    emit(
        json!({"phase":"native-hit","point":[80*scale,84*scale],"page_point":[80,20],"scale":scale,"url":native_url,"event":EVENT}),
    )?;
    client.command("Page.navigate", json!({"url":FIXTURE}))?;
    client.load(FIXTURE)?;
    let (before, _) = wait_native(
        &native,
        log,
        FIXTURE,
        TITLE,
        load_count(&after),
        None,
        output,
    )?;
    // Capture anew after the new navigation; no old PNG can satisfy this phase.
    let image = client.screenshot(&output.join("cdp-fixture.png"))?;
    require(
        image.dimensions() == native_page.dimensions() && fixture_pixels(&image, 0, scale),
        "New fixture raster mismatch",
    )?;
    emit(json!({"phase":"new-cdp-fixture","width":image.width(),"height":image.height()}))?;
    scale_and_boxes(&mut client, &native.frame()?, &image, scale)?;
    client.frame_url(FIXTURE)?;
    client.clear_events();
    for (kind, buttons) in [("mousePressed", 1), ("mouseReleased", 0)] {
        client.command(
            "Input.dispatchMouseEvent",
            json!({"type":kind,"x":60,"y":50,"button":"left","buttons":buttons,"clickCount":1}),
        )?;
    }
    let cdp_url = format!("{BASE}/cssom-methods-cdp-hit");
    client.load(&cdp_url)?;
    let (after, _) = wait_native(
        &native,
        log,
        &cdp_url,
        "CSSOM CDP hit",
        load_count(&before),
        Some([0x65, 0x43, 0x21]),
        output,
    )?;
    click_evidence(&before, &after, &cdp_url)?;
    let image = client.screenshot(&output.join("cdp-destination.png"))?;
    require(
        pixel(&image, 10 * scale, 10 * scale, [0x65, 0x43, 0x21])
            && pixel(&image, 170 * scale, 70 * scale, [0x65, 0x43, 0x21]),
        "CDP destination pixels mismatch",
    )?;
    emit(json!({"phase":"cdp-hit","point":[60,50],"scale":scale,"url":cdp_url,"event":EVENT}))?;
    client.timeout()?;
    client.socket.close(None)?;
    remaining(deadline)?;
    println!("CSSOM_METHODS_SMOKE_OK");
    remaining(deadline)?;
    Ok(())
}

fn main() -> Result<()> {
    let whole_deadline = Instant::now() + Duration::from_secs(30);
    let deadline = whole_deadline - Duration::from_secs(2);
    let args: Vec<_> = std::env::args().skip(1).collect();
    require(
        args.len() == 5,
        "Usage: cssom_methods_smoke <direct-page-ws-url> <fixture-url> <window-id> <browser-log> <NEW-evidence-dir>",
    )?;
    require(
        args[1] == FIXTURE,
        "Only the exact owned fixture URL is admitted",
    )?;
    let window: u32 = args[2].parse()?;
    require(
        window != 0 && window.to_string() == args[2],
        "Expected canonical nonzero window ID",
    )?;
    let scale = 1;
    let log = PathBuf::from(&args[3]);
    let output = PathBuf::from(&args[4]);
    let parent = output
        .parent()
        .ok_or_else(|| failure("Evidence directory needs owned parent"))?;
    require(
        parent.is_dir() && !fs::symlink_metadata(parent)?.file_type().is_symlink(),
        "Unsafe evidence parent",
    )?;
    let result = run(
        url::Url::parse(&args[0])?,
        window,
        &log,
        &output,
        scale,
        deadline,
    );
    require(
        Instant::now() < whole_deadline,
        "CSSOM methods client exceeded its 30-second whole deadline",
    )?;
    result
}
