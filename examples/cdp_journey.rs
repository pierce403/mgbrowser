//! External CDP regression client for the explicitly local journey fixture.
//!
//! Usage: cargo run --locked --example cdp_journey -- [page-websocket-url]
//!        [fixture-url] [screenshot-path]
//! Start journey_server and mgbrowser --remote-debugging-port=9222 first.
//! This client uses only the browser's public WebSocket protocol, not App hooks.

use std::{
    collections::VecDeque,
    error::Error,
    fs,
    io::{self, Cursor},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const QUERY: &str = "Rust & café";
const MAX_MESSAGE: usize = 8 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 1024 * 1024;

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::other(message.into()).into()
}

struct Client {
    socket: WebSocket<TcpStream>,
    deadline: Instant,
    next_id: u64,
    session: Option<String>,
    events: VecDeque<(Value, usize)>,
    event_bytes: usize,
}

impl Client {
    fn connect(endpoint: &url::Url, deadline: Instant) -> Result<Self> {
        if endpoint.scheme() != "ws"
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
        {
            return Err(failure(
                "Expected a loopback ws:// debugging endpoint without credentials",
            ));
        }
        let address = SocketAddr::new(
            loopback_host(endpoint)?,
            endpoint.port_or_known_default().unwrap_or(80),
        );
        let timeout = remaining(deadline)?;
        let stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        stream.set_write_timeout(Some(remaining(deadline)?))?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let (socket, _) =
            tungstenite::client::client_with_config(endpoint.as_str(), stream, Some(config))?;
        Ok(Self {
            socket,
            deadline,
            next_id: 1,
            session: None,
            events: VecDeque::new(),
            event_bytes: 0,
        })
    }

    fn timeout(&mut self) -> Result<()> {
        let timeout = remaining(self.deadline)?;
        self.socket.get_mut().set_read_timeout(Some(timeout))?;
        self.socket.get_mut().set_write_timeout(Some(timeout))?;
        Ok(())
    }

    fn read(&mut self) -> Result<(Value, usize)> {
        loop {
            self.timeout()?;
            match self.socket.read()? {
                Message::Text(text) => {
                    return Ok((serde_json::from_str(text.as_str())?, text.len()));
                }
                Message::Ping(_) | Message::Pong(_) => {
                    self.socket.flush()?;
                }
                Message::Close(frame) => {
                    return Err(failure(format!(
                        "CDP connection closed before completion: {frame:?}"
                    )));
                }
                _ => return Err(failure("Expected a JSON text WebSocket message")),
            }
        }
    }

    fn queue_event(&mut self, value: Value, bytes: usize) -> Result<()> {
        if self.events.len() >= 256 || self.event_bytes.saturating_add(bytes) > MAX_EVENT_BYTES {
            return Err(failure(
                "CDP event queue exceeded the regression client's bound",
            ));
        }
        self.event_bytes += bytes;
        self.events.push_back((value, bytes));
        Ok(())
    }

    fn command_raw(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = &self.session {
            message["sessionId"] = json!(session);
        }
        self.timeout()?;
        self.socket
            .send(Message::Text(message.to_string().into()))?;
        loop {
            let (message, bytes) = self.read()?;
            if message.get("id").is_some() {
                if message["id"].as_u64() != Some(id) {
                    return Err(failure(format!(
                        "Unexpected response ID while waiting for {method}"
                    )));
                }
                if message.get("sessionId").and_then(Value::as_str) != self.session.as_deref() {
                    return Err(failure(format!(
                        "Response session did not match request for {method}"
                    )));
                }
                return Ok(message);
            }
            if message.get("method").and_then(Value::as_str).is_none() {
                return Err(failure("CDP message is neither a response nor an event"));
            }
            // Navigation may emit loadEventFired before the command response.
            // Preserve those events so waiting for the response cannot lose it.
            self.queue_event(message, bytes)?;
        }
    }

    fn command(&mut self, method: &str, params: Value) -> Result<Value> {
        let response = self.command_raw(method, params)?;
        if let Some(error) = response.get("error") {
            return Err(failure(format!("{method} failed: {error}")));
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| failure(format!("Missing result for {method}")))
    }

    fn matches_event(&self, message: &Value, method: &str) -> bool {
        message["method"].as_str() == Some(method)
            && message.get("sessionId").and_then(Value::as_str) == self.session.as_deref()
    }

    fn await_event(&mut self, method: &str) -> Result<Value> {
        loop {
            if let Some(index) = self
                .events
                .iter()
                .position(|(message, _)| self.matches_event(message, method))
            {
                let (event, bytes) = self.events.remove(index).unwrap();
                self.event_bytes -= bytes;
                return Ok(event);
            }
            let (message, bytes) = self.read()?;
            if message.get("id").is_some() {
                return Err(failure(format!(
                    "Unexpected response while awaiting {method}"
                )));
            }
            self.queue_event(message, bytes)?;
        }
    }

    fn clear_load_events(&mut self) {
        let session = self.session.as_deref();
        self.events.retain(|(message, _)| {
            message["method"].as_str() != Some("Page.loadEventFired")
                || message.get("sessionId").and_then(Value::as_str) != session
        });
        self.event_bytes = self.events.iter().map(|(_, bytes)| bytes).sum();
    }

    fn frame_url(&mut self) -> Result<url::Url> {
        let frame = self.command("Page.getFrameTree", json!({}))?;
        let value = frame
            .pointer("/frameTree/frame/url")
            .and_then(Value::as_str)
            .ok_or_else(|| failure("Page.getFrameTree omitted the main frame URL"))?;
        Ok(url::Url::parse(value)?)
    }

    fn wait_for_path(&mut self, fixture: &url::Url, path: &str) -> Result<url::Url> {
        loop {
            self.await_event("Page.loadEventFired")?;
            let current = self.frame_url()?;
            if current.origin() == fixture.origin() && current.path() == path {
                return Ok(current);
            }
        }
    }

    fn root(&mut self) -> Result<u64> {
        let response = self.command("DOM.getDocument", json!({"depth": -1}))?;
        response
            .pointer("/root/nodeId")
            .and_then(Value::as_u64)
            .ok_or_else(|| failure("DOM.getDocument omitted its root node ID"))
    }

    fn query(&mut self, root: u64, selector: &str) -> Result<u64> {
        let response = self.command(
            "DOM.querySelector",
            json!({"nodeId": root, "selector": selector}),
        )?;
        response["nodeId"]
            .as_u64()
            .filter(|id| *id != 0)
            .ok_or_else(|| failure(format!("No DOM node matched {selector}")))
    }

    fn attach(&mut self) -> Result<()> {
        let response = self.command(
            "Target.attachToTarget",
            json!({"targetId": "page-1", "flatten": true}),
        )?;
        self.session = Some(
            response["sessionId"]
                .as_str()
                .filter(|session| !session.is_empty())
                .ok_or_else(|| failure("Target.attachToTarget omitted sessionId"))?
                .to_owned(),
        );
        Ok(())
    }

    fn close(&mut self) {
        if self.timeout().is_ok() {
            let _ = self.socket.close(None);
        }
    }
}

fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| failure("CDP journey exceeded its 30-second deadline"))
}

fn loopback_host(url: &url::Url) -> Result<IpAddr> {
    match url.host() {
        Some(url::Host::Ipv4(ip)) if ip.is_loopback() => Ok(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) if ip.is_loopback() => Ok(IpAddr::V6(ip)),
        Some(url::Host::Domain("localhost")) => Ok(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        _ => Err(failure(
            "This regression client requires loopback endpoints and fixture URLs",
        )),
    }
}

fn center_of_box(model: &Value) -> Result<(f64, f64)> {
    let quad = model
        .pointer("/model/content")
        .and_then(Value::as_array)
        .filter(|quad| quad.len() == 8)
        .ok_or_else(|| failure("DOM.getBoxModel omitted a content quadrilateral"))?;
    let mut x = 0.0;
    let mut y = 0.0;
    for pair in quad.chunks_exact(2) {
        x += pair[0]
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| failure("Invalid box x coordinate"))?;
        y += pair[1]
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| failure("Invalid box y coordinate"))?;
    }
    Ok((x / 4.0, y / 4.0))
}

fn save_screenshot(client: &mut Client, path: &Path) -> Result<(u32, u32)> {
    let response = client.command("Page.captureScreenshot", json!({"format": "png"}))?;
    let data = response["data"]
        .as_str()
        .ok_or_else(|| failure("Screenshot response omitted base64 data"))?;
    let bytes = STANDARD.decode(data)?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(failure("Screenshot was not a PNG"));
    }
    let dimensions = image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Png)
        .into_dimensions()?;
    if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 4096 || dimensions.1 > 4096 {
        return Err(failure(
            "Screenshot dimensions exceed regression-client bounds",
        ));
    }
    let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.to_rgba8();
    if !decoded
        .pixels()
        .any(|pixel| pixel != decoded.get_pixel(0, 0))
    {
        return Err(failure(
            "Screenshot is a single solid color, not the rendered fixture",
        ));
    }
    remaining(client.deadline)?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(dimensions)
}

fn run(endpoint: url::Url, fixture: url::Url, evidence: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loopback_host(&fixture)?;
    if !matches!(fixture.scheme(), "http" | "https") {
        return Err(failure("Fixture URL must use HTTP(S)"));
    }
    let mut client = Client::connect(&endpoint, deadline)?;
    if endpoint.path().starts_with("/devtools/browser/") {
        client.attach()?;
    }
    client.command("Page.enable", json!({}))?;
    let unsupported = client.command_raw("Runtime.evaluate", json!({"expression": "1 + 1"}))?;
    if unsupported.pointer("/error/code").and_then(Value::as_i64) != Some(-32601) {
        return Err(failure(
            "Unimplemented Runtime.evaluate did not return -32601",
        ));
    }
    client.clear_load_events();
    client.command("Page.navigate", json!({"url": fixture.as_str()}))?;
    client.wait_for_path(&fixture, fixture.path())?;
    println!("CDP homepage loaded");

    let root = client.root()?;
    let input = client.query(root, "input[name=q]")?;
    client.command("DOM.focus", json!({"nodeId": input}))?;
    client.command("Input.insertText", json!({"text": QUERY}))?;
    client.clear_load_events();
    client.command(
        "Input.dispatchKeyEvent",
        json!({
            "type": "keyDown", "key": "Enter", "code": "Enter",
            "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13,
        }),
    )?;
    let search = client.wait_for_path(&fixture, "/search")?;
    let fields: Vec<_> = search.query_pairs().collect();
    if !fields
        .iter()
        .any(|(name, value)| name == "q" && value == QUERY)
        || !fields
            .iter()
            .any(|(name, value)| name == "source" && value == "fixture")
    {
        return Err(failure(
            "Search URL did not contain the typed Unicode query and hidden field",
        ));
    }
    let stale = client.command_raw("DOM.focus", json!({"nodeId": input}))?;
    let code = stale.pointer("/error/code").and_then(Value::as_i64);
    let message = stale
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if code != Some(-32000) || !(message.contains("node") || message.contains("stale")) {
        return Err(failure(
            "A node ID from the previous document was not rejected as stale",
        ));
    }
    println!("CDP form submitted with Unicode query; stale node rejected");

    let root = client.root()?;
    let link = client.query(root, "a")?;
    let model = client.command("DOM.getBoxModel", json!({"nodeId": link}))?;
    let (x, y) = center_of_box(&model)?;
    client.clear_load_events();
    client.command(
        "Input.dispatchMouseEvent",
        json!({
            "type": "mousePressed", "x": x, "y": y, "button": "left", "buttons": 1, "clickCount": 1,
        }),
    )?;
    client.command("Input.dispatchMouseEvent", json!({
        "type": "mouseReleased", "x": x, "y": y, "button": "left", "buttons": 0, "clickCount": 1,
    }))?;
    let destination = client.wait_for_path(&fixture, "/destination")?;
    let dimensions = save_screenshot(&mut client, evidence)?;
    println!(
        "CDP first result opened {destination}; decoded PNG {}x{} at {}",
        dimensions.0,
        dimensions.1,
        evidence.display()
    );

    // Exercise a separate browser-endpoint connection and flattened session.
    // It inspects the already loaded real target without another navigation.
    let mut browser_endpoint = endpoint;
    browser_endpoint.set_path("/devtools/browser/browser-1");
    browser_endpoint.set_query(None);
    browser_endpoint.set_fragment(None);
    let mut browser = Client::connect(&browser_endpoint, deadline)?;
    browser.attach()?;
    browser.command("Page.enable", json!({}))?;
    if browser.frame_url()? != destination {
        return Err(failure(
            "Flattened browser session did not inspect the same page target",
        ));
    }
    let root = browser.root()?;
    browser.query(root, "h1")?;
    let session = browser.session.take().unwrap();
    browser.command("Target.detachFromTarget", json!({"sessionId": session}))?;
    println!("CDP flattened browser session attached, inspected and detached");
    browser.close();
    client.close();
    remaining(deadline)?;
    println!("CDP_JOURNEY_OK");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() > 3 || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "Usage: cdp_journey [ws://127.0.0.1:9222/devtools/page/page-1] [http://127.0.0.1:7878/] [tmp/cdp-journey.png]"
        );
        return if args.len() > 3 {
            Err(failure("Too many arguments"))
        } else {
            Ok(())
        };
    }
    let endpoint = url::Url::parse(
        args.first()
            .map(String::as_str)
            .unwrap_or("ws://127.0.0.1:9222/devtools/page/page-1"),
    )?;
    let fixture = url::Url::parse(
        args.get(1)
            .map(String::as_str)
            .unwrap_or("http://127.0.0.1:7878/"),
    )?;
    let evidence = Path::new(
        args.get(2)
            .map(String::as_str)
            .unwrap_or("tmp/cdp-journey.png"),
    );
    run(endpoint, fixture, evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    #[test]
    fn delayed_response_does_not_consume_the_navigation_event() {
        for session in [None, Some("session-fixture")] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let address = listener.local_addr().unwrap();
            let server = thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let request: Value =
                    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(request["method"], "Page.navigate");
                let mut event =
                    json!({"method": "Page.loadEventFired", "params": {"timestamp": 1.0}});
                let mut response = json!({"id": request["id"], "result": {"frameId": "page-1"}});
                if let Some(session) = session {
                    assert_eq!(request["sessionId"], session);
                    event["sessionId"] = json!(session);
                    response["sessionId"] = json!(session);
                }
                socket
                    .send(Message::Text(event.to_string().into()))
                    .unwrap();
                socket
                    .send(Message::Text(response.to_string().into()))
                    .unwrap();
                // Closing now means await_event must use the queued event,
                // rather than accidentally waiting for a second load event.
            });
            let endpoint =
                url::Url::parse(&format!("ws://{address}/devtools/page/page-1")).unwrap();
            let mut client =
                Client::connect(&endpoint, Instant::now() + Duration::from_secs(2)).unwrap();
            client.session = session.map(str::to_owned);
            assert_eq!(
                client
                    .command("Page.navigate", json!({"url": "http://127.0.0.1/"}))
                    .unwrap()["frameId"],
                "page-1"
            );
            assert_eq!(
                client.await_event("Page.loadEventFired").unwrap()["params"]["timestamp"],
                1.0
            );
            assert!(client.events.is_empty());
            server.join().unwrap();
        }
    }
}
