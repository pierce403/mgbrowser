//! Opt-in, loopback-only discovery and WebSocket transport for our CDP subset.
//! Application commands are dispatched by the window owner, not this module.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Value, json};
use tungstenite::{
    Message, WebSocket,
    protocol::{Role, WebSocketConfig},
};

const MAX_CLIENTS: usize = 4;
const MAX_HANDSHAKE: usize = 16 * 1024;
const MAX_COMMAND: usize = 256 * 1024;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_QUEUED_BYTES: u64 = 16 * 1024 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

pub struct Command {
    pub client: u64,
    pub browser: bool,
    pub message: Value,
}

pub struct Server {
    shared: Arc<Shared>,
    commands: mpsc::Receiver<Command>,
    listener: Option<thread::JoinHandle<()>>,
}

struct Shared {
    port: u16,
    stop: AtomicBool,
    next_client: AtomicU64,
    page: Mutex<(String, String)>,
    clients: Mutex<HashMap<u64, Client>>,
    commands: mpsc::SyncSender<Command>,
}

struct Client {
    output: mpsc::SyncSender<String>,
    queued_bytes: Arc<AtomicU64>,
    socket: TcpStream,
    endpoint: Option<Endpoint>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Browser,
    Page,
}

impl Server {
    /// Listen only on IPv4 loopback. Port zero selects an available local port.
    pub fn bind(port: u16) -> Result<Self, String> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .map_err(|error| format!("Cannot bind local debugging endpoint: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let (send, commands) = mpsc::sync_channel(64);
        let shared = Arc::new(Shared {
            port,
            stop: AtomicBool::new(false),
            next_client: AtomicU64::new(1),
            page: Mutex::new(("mgbrowser".into(), "about:blank".into())),
            clients: Mutex::new(HashMap::new()),
            commands: send,
        });
        let state = shared.clone();
        let listener = thread::spawn(move || accept_loop(listener, state));
        Ok(Self {
            shared,
            commands,
            listener: Some(listener),
        })
    }

    pub fn port(&self) -> u16 {
        self.shared.port
    }

    pub fn client_ids(&self) -> Vec<u64> {
        self.shared
            .clients
            .lock()
            .map(|clients| clients.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Direct page connections are attached before their first CDP command.
    /// HTTP discovery and browser-level sockets do not attach a page themselves.
    pub fn page_client_ids(&self) -> Vec<u64> {
        self.shared
            .clients
            .lock()
            .map(|clients| {
                clients
                    .iter()
                    .filter_map(|(&id, client)| {
                        (client.endpoint == Some(Endpoint::Page)).then_some(id)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn try_recv(&self) -> Option<Command> {
        self.commands.try_recv().ok()
    }

    /// A slow or oversized client is disconnected instead of dropping responses.
    pub fn send(&self, client: u64, message: Value) {
        let Ok(text) = serde_json::to_string(&message) else {
            return;
        };
        let Ok(mut clients) = self.shared.clients.lock() else {
            return;
        };
        let Some(connection) = clients.get(&client) else {
            return;
        };
        let length = text.len() as u64;
        let reserved = text.len() <= MAX_RESPONSE
            && connection
                .queued_bytes
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bytes| {
                    bytes
                        .checked_add(length)
                        .filter(|total| *total <= MAX_QUEUED_BYTES)
                })
                .is_ok();
        if reserved && connection.output.try_send(text).is_ok() {
            return;
        }
        if reserved {
            connection.queued_bytes.fetch_sub(length, Ordering::AcqRel);
        }
        let _ = connection.socket.shutdown(Shutdown::Both);
        clients.remove(&client);
    }

    pub fn update_page(&self, title: &str, url: &str) {
        if let Ok(mut page) = self.shared.page.lock() {
            *page = (bounded_text(title, 4096), bounded_text(url, 16 * 1024));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Ok(mut clients) = self.shared.clients.lock() {
            for (_, client) in clients.drain() {
                let _ = client.socket.shutdown(Shutdown::Both);
            }
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

fn bounded_text(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((socket, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let Ok(mut clients) = shared.clients.lock() else {
                    break;
                };
                if shared.stop.load(Ordering::Acquire) {
                    break;
                }
                if clients.len() >= MAX_CLIENTS {
                    continue;
                }
                let Ok(control_socket) = socket.try_clone() else {
                    continue;
                };
                let client = shared.next_client.fetch_add(1, Ordering::Relaxed);
                let (send, receive) = mpsc::sync_channel(8);
                let queued_bytes = Arc::new(AtomicU64::new(0));
                clients.insert(
                    client,
                    Client {
                        output: send,
                        queued_bytes: queued_bytes.clone(),
                        socket: control_socket,
                        endpoint: None,
                    },
                );
                drop(clients);
                let state = shared.clone();
                thread::spawn(move || {
                    let _ = handle_client(socket, &state, client, receive, queued_bytes);
                    if let Ok(mut clients) = state.clients.lock() {
                        clients.remove(&client);
                    }
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(_) => break,
        }
    }
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    tail: Vec<u8>,
}

fn read_request(socket: &mut TcpStream) -> Result<Request, String> {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    let mut bytes = Vec::new();
    let end = loop {
        if let Some(offset) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break offset + 4;
        }
        if bytes.len() >= MAX_HANDSHAKE {
            return Err("Debugging handshake exceeds 16 KiB".into());
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Debugging handshake timed out")?;
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        let mut buffer = [0; 1024];
        let count = socket
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("Incomplete debugging handshake".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    };
    if end > MAX_HANDSHAKE {
        return Err("Debugging handshake exceeds 16 KiB".into());
    }
    let text = std::str::from_utf8(&bytes[..end]).map_err(|_| "Invalid HTTP header encoding")?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().ok_or("Missing request line")?.split(' ');
    let method = first.next().ok_or("Missing HTTP method")?.to_owned();
    let path = first.next().ok_or("Missing HTTP target")?.to_owned();
    if first.next() != Some("HTTP/1.1")
        || first.next().is_some()
        || !path.starts_with('/')
        || path.starts_with("//")
    {
        return Err("Invalid HTTP request line".into());
    }
    let mut headers = HashMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        if headers.len() >= 64 {
            return Err("Too many HTTP headers".into());
        }
        let (name, value) = line.split_once(':').ok_or("Invalid HTTP header")?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
            || value
                .bytes()
                .any(|byte| byte < 32 && byte != b'\t' || byte == 127)
        {
            return Err("Invalid HTTP header".into());
        }
        if headers
            .insert(name.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err("Duplicate HTTP header".into());
        }
    }
    Ok(Request {
        method,
        path,
        headers,
        tail: bytes[end..].to_vec(),
    })
}

fn host_allowed(host: &str, port: u16) -> bool {
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
    .iter()
    .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

fn origin_allowed(origin: Option<&String>, port: u16) -> bool {
    origin.is_none_or(|origin| {
        origin
            .strip_prefix("http://")
            .is_some_and(|host| host_allowed(host, port))
    })
}

fn handle_client(
    mut socket: TcpStream,
    shared: &Shared,
    client: u64,
    output: mpsc::Receiver<String>,
    queued_bytes: Arc<AtomicU64>,
) -> Result<(), String> {
    socket
        .set_write_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let request = match read_request(&mut socket) {
        Ok(request) => request,
        Err(_) => {
            http_response(
                &mut socket,
                400,
                "Bad Request",
                "Invalid or incomplete debugging handshake",
            )?;
            return Ok(());
        }
    };
    if !request
        .headers
        .get("host")
        .is_some_and(|host| host_allowed(host, shared.port))
        || !origin_allowed(request.headers.get("origin"), shared.port)
    {
        return http_response(
            &mut socket,
            403,
            "Forbidden",
            "Only local debugging clients are allowed",
        );
    }
    if request.method != "GET" {
        return http_response(
            &mut socket,
            405,
            "Method Not Allowed",
            "Discovery accepts GET only",
        );
    }
    if request.headers.contains_key("transfer-encoding")
        || request
            .headers
            .get("content-length")
            .is_some_and(|length| length != "0")
    {
        return http_response(
            &mut socket,
            400,
            "Bad Request",
            "Discovery requests cannot have bodies",
        );
    }
    let websocket = match request.path.as_str() {
        "/devtools/page/page-1" => Some(false),
        "/devtools/browser/browser-1" => Some(true),
        _ => None,
    };
    if let Some(browser) = websocket {
        let mut builder = tungstenite::http::Request::builder()
            .method("GET")
            .uri(&request.path);
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let upgrade = builder.body(()).map_err(|error| error.to_string())?;
        let valid_key = request
            .headers
            .get("sec-websocket-key")
            .and_then(|key| base64::engine::general_purpose::STANDARD.decode(key).ok())
            .is_some_and(|key| key.len() == 16);
        let response = tungstenite::handshake::server::create_response(&upgrade);
        let response = match response {
            Ok(response) if valid_key => response,
            _ => {
                return http_response(
                    &mut socket,
                    400,
                    "Bad Request",
                    "A valid WebSocket handshake is required",
                );
            }
        };
        {
            let mut clients = shared
                .clients
                .lock()
                .map_err(|_| "Client metadata lock failed")?;
            let connection = clients
                .get_mut(&client)
                .ok_or("Debugging client disconnected")?;
            connection.endpoint = Some(if browser {
                Endpoint::Browser
            } else {
                Endpoint::Page
            });
        }
        tungstenite::handshake::server::write_response(&mut socket, &response)
            .map_err(|error| error.to_string())?;
        socket
            .set_read_timeout(Some(Duration::from_millis(25)))
            .map_err(|error| error.to_string())?;
        let config = WebSocketConfig::default()
            .read_buffer_size(4096)
            .write_buffer_size(0)
            .max_write_buffer_size(MAX_RESPONSE + 1024)
            .max_message_size(Some(MAX_COMMAND))
            .max_frame_size(Some(MAX_COMMAND));
        let websocket =
            WebSocket::from_partially_read(socket, request.tail, Role::Server, Some(config));
        return websocket_loop(websocket, shared, client, browser, output, queued_bytes);
    }
    let page = shared
        .page
        .lock()
        .map_err(|_| "Page metadata lock failed")?
        .clone();
    let address = format!("127.0.0.1:{}", shared.port);
    let body = match request.path.as_str() {
        "/json/version" => json!({
            "Browser": format!("mgbrowser/{}", env!("CARGO_PKG_VERSION")),
            "Protocol-Version": "1.3",
            "User-Agent": crate::net::USER_AGENT,
            "webSocketDebuggerUrl": format!("ws://{address}/devtools/browser/browser-1"),
            "mgbrowserProtocol": "Experimental CDP subset; no Chromium or V8 engine",
        }),
        "/json" | "/json/list" => json!([{
            "id": "page-1", "type": "page", "title": page.0, "url": page.1,
            "description": "mgbrowser experimental HTML-flow page",
            "webSocketDebuggerUrl": format!("ws://{address}/devtools/page/page-1"),
        }]),
        "/json/protocol" => {
            return http_response(
                &mut socket,
                200,
                "OK",
                include_str!("../../../docs/cdp-protocol.json"),
            );
        }
        _ => return http_response(&mut socket, 404, "Not Found", "Unknown debugging endpoint"),
    };
    http_response(&mut socket, 200, "OK", &body.to_string())
}

fn http_response(
    socket: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &str,
) -> Result<(), String> {
    let body = if status == 200 {
        body.to_owned()
    } else {
        json!({"error": body}).to_string()
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket
        .write_all(response.as_bytes())
        .map_err(|error| error.to_string())
}

fn websocket_loop(
    mut websocket: WebSocket<TcpStream>,
    shared: &Shared,
    client: u64,
    browser: bool,
    output: mpsc::Receiver<String>,
    queued_bytes: Arc<AtomicU64>,
) -> Result<(), String> {
    let mut last_command = Instant::now();
    while !shared.stop.load(Ordering::Acquire) && last_command.elapsed() < Duration::from_secs(300)
    {
        for _ in 0..8 {
            let Ok(text) = output.try_recv() else {
                break;
            };
            queued_bytes.fetch_sub(text.len() as u64, Ordering::AcqRel);
            websocket
                .send(Message::Text(text.into()))
                .map_err(|error| error.to_string())?;
        }
        match websocket.read() {
            Ok(Message::Text(text)) => {
                let message =
                    serde_json::from_str::<Value>(&text).map_err(|_| "Invalid debugging JSON")?;
                if !message.is_object() {
                    return Err("Debugging command must be a JSON object".into());
                }
                last_command = Instant::now();
                shared
                    .commands
                    .try_send(Command {
                        client,
                        browser,
                        message,
                    })
                    .map_err(|_| "Debugging command queue full")?;
            }
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                websocket.flush().map_err(|error| error.to_string())?;
            }
            Ok(Message::Close(_)) => {
                let _ = websocket.flush();
                return Ok(());
            }
            Ok(_) => return Err("Only text debugging commands are supported".into()),
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ok(());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let _ = websocket.close(None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tungstenite::client::IntoClientRequest;

    fn http(server: &Server, method: &str, path: &str, headers: &str) -> String {
        let mut socket =
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, server.port())).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        socket
            .write_all(format!("{method} {path} HTTP/1.1\r\n{headers}\r\n").as_bytes())
            .unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn discovery_reports_live_page_and_honest_protocol_subset() {
        let server = Server::bind(0).unwrap();
        assert_ne!(server.port(), 0);
        server.update_page("Example & test", "https://example.org/current");
        let host = format!("Host: 127.0.0.1:{}\r\n", server.port());
        let version = http(&server, "GET", "/json/version", &host);
        assert!(version.starts_with("HTTP/1.1 200"));
        assert!(
            !version
                .to_ascii_lowercase()
                .contains("access-control-allow-origin")
        );
        let version: Value =
            serde_json::from_str(version.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            version["Browser"],
            format!("mgbrowser/{}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(version["Protocol-Version"], "1.3");
        assert_eq!(version["User-Agent"], crate::net::USER_AGENT);
        assert!(
            version["mgbrowserProtocol"]
                .as_str()
                .unwrap()
                .contains("subset")
        );
        assert_eq!(
            version["webSocketDebuggerUrl"],
            format!(
                "ws://127.0.0.1:{}/devtools/browser/browser-1",
                server.port()
            )
        );
        for path in ["/json", "/json/list"] {
            let response = http(&server, "GET", path, &host);
            let pages: Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(pages.as_array().unwrap().len(), 1);
            assert_eq!(pages[0]["id"], "page-1");
            assert_eq!(pages[0]["title"], "Example & test");
            assert_eq!(pages[0]["url"], "https://example.org/current");
        }
        let protocol = http(&server, "GET", "/json/protocol", &host);
        assert!(protocol.starts_with("HTTP/1.1 200"));
        let schema: Value =
            serde_json::from_str(protocol.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(schema["version"], json!({"major":"1", "minor":"3"}));
        let domains = schema["domains"].as_array().unwrap();
        assert_eq!(domains.len(), 5);
        assert!(domains.iter().all(|domain| domain["domain"] != "Runtime"));
        assert!(
            http(&server, "GET", "/json/new?https://example.org", &host)
                .starts_with("HTTP/1.1 404")
        );
        assert!(http(&server, "POST", "/json/list", &host).starts_with("HTTP/1.1 405"));
    }

    #[test]
    fn discovery_rejects_remote_host_origin_and_ambiguous_headers() {
        let server = Server::bind(0).unwrap();
        let host = format!("Host: 127.0.0.1:{}\r\n", server.port());
        for headers in [
            "Host: remote.example:9222\r\n".to_owned(),
            "Host: localhost\r\n".to_owned(),
            format!("{host}Origin: https://remote.example\r\n"),
            format!("{host}Origin: null\r\n"),
            format!(
                "{host}Origin: http://127.0.0.1:{}\r\n",
                server.port().saturating_sub(1)
            ),
        ] {
            assert!(http(&server, "GET", "/json", &headers).starts_with("HTTP/1.1 403"));
        }
        assert!(
            http(&server, "GET", "/json", &format!("{host}{host}")).starts_with("HTTP/1.1 400")
        );
        assert!(
            http(
                &server,
                "GET",
                "/json",
                &format!("{host}Content-Length: 1\r\n")
            )
            .starts_with("HTTP/1.1 400")
        );
        let local = format!("{host}Origin: http://localhost:{}\r\n", server.port());
        assert!(http(&server, "GET", "/json", &local).starts_with("HTTP/1.1 200"));
    }

    fn websocket(server: &Server, path: &str) -> WebSocket<TcpStream> {
        let socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, server.port())).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let url = format!("ws://127.0.0.1:{}{path}", server.port());
        tungstenite::client(url, socket).unwrap().0
    }

    fn command(server: &Server) -> Command {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(command) = server.try_recv() {
                return command;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for local debugging command"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn real_websocket_dispatch_and_response_roundtrip() {
        let server = Server::bind(0).unwrap();
        assert!(server.page_client_ids().is_empty());
        let mut page = websocket(&server, "/devtools/page/page-1");
        let attached = server.page_client_ids();
        assert_eq!(attached.len(), 1, "page endpoint attaches before a command");
        page.send(Message::Text(
            json!({"id":7,"method":"Page.enable"}).to_string().into(),
        ))
        .unwrap();
        let received = command(&server);
        assert!(!received.browser);
        assert_eq!(attached, vec![received.client]);
        assert_eq!(received.message["id"], 7);
        assert_eq!(received.message["method"], "Page.enable");
        server.send(received.client, json!({"id":7,"result":{}}));
        let response: Value =
            serde_json::from_str(page.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(response, json!({"id":7,"result":{}}));
        let mut browser = websocket(&server, "/devtools/browser/browser-1");
        assert_eq!(
            server.page_client_ids(),
            attached,
            "browser socket alone must not attach the page"
        );
        browser
            .send(Message::Text(
                json!({"id":8,"method":"Target.getTargets"})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        let received = command(&server);
        assert!(received.browser);
        server.send(received.client, json!({"id":8,"result":{"targetInfos":[]}}));
        let response: Value =
            serde_json::from_str(browser.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(response["id"], 8);
    }

    #[test]
    fn websocket_rejects_remote_origins_unknown_targets_and_oversized_messages() {
        let server = Server::bind(0).unwrap();
        for (path, origin) in [
            ("/devtools/page/page-1", Some("https://remote.example")),
            ("/devtools/page/unknown", None),
        ] {
            let url = format!("ws://127.0.0.1:{}{path}", server.port());
            let mut request = url.into_client_request().unwrap();
            if let Some(origin) = origin {
                request
                    .headers_mut()
                    .insert("Origin", origin.parse().unwrap());
            }
            let socket =
                TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, server.port())).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            assert!(tungstenite::client(request, socket).is_err());
        }
        let mut socket = websocket(&server, "/devtools/page/page-1");
        let _ = socket.send(Message::Text("x".repeat(MAX_COMMAND + 1).into()));
        assert!(socket.read().is_err());
        assert!(server.try_recv().is_none());
    }

    #[test]
    fn connection_limit_and_drop_release_local_resources() {
        let server = Server::bind(0).unwrap();
        let port = server.port();
        let mut clients = (0..MAX_CLIENTS)
            .map(|_| websocket(&server, "/devtools/page/page-1"))
            .collect::<Vec<_>>();
        assert_eq!(server.client_ids().len(), MAX_CLIENTS);
        let socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        assert!(
            tungstenite::client(
                format!("ws://127.0.0.1:{port}/devtools/page/page-1"),
                socket
            )
            .is_err()
        );
        drop(server);
        for socket in &mut clients {
            assert!(socket.read().is_err());
        }
        assert!(TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_err());
    }
}
