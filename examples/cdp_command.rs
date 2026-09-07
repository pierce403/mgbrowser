//! One-shot command batches for mgbrowser's local CDP page endpoint.
//!
//! cdp_command ws://127.0.0.1:PORT/devtools/page/page-1 \
//!   '[{"method":"DOM.getDocument","params":{}}]'
//!
//! Accepts an object or a nonempty array; IDs are assigned by this client.
//! Events and responses are printed as JSON lines. A protocol error stops the
//! batch. --screenshot PATH saves the last successful PNG capture and replaces
//! its base64 output with metadata. Sessions and user-supplied IDs are unsupported.

use std::{
    error::Error,
    fs,
    io::{self, Cursor, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MAX_PAYLOAD: usize = 256 * 1024;
const MAX_REPLY: usize = 8 * 1024 * 1024;
const MAX_BATCH: usize = 64;

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::other(message.into()).into()
}

#[derive(Debug)]
struct Command {
    method: String,
    params: Value,
}

fn commands(payload: &str) -> Result<Vec<Command>> {
    if payload.len() > MAX_PAYLOAD {
        return Err(failure("Command payload exceeds 256 KiB"));
    }
    let value: Value = serde_json::from_str(payload)?;
    let values = match value {
        Value::Object(_) => vec![value],
        Value::Array(values) if !values.is_empty() && values.len() <= MAX_BATCH => values,
        _ => {
            return Err(failure(
                "Expected a command object or an array of 1..64 command objects",
            ));
        }
    };
    values
        .into_iter()
        .map(|value| {
            let object = value
                .as_object()
                .ok_or_else(|| failure("Each command must be an object"))?;
            if object.keys().any(|key| key != "method" && key != "params") {
                return Err(failure(
                    "Commands accept only method and params; user IDs and sessions are unsupported",
                ));
            }
            let method = object
                .get("method")
                .and_then(Value::as_str)
                .filter(|method| {
                    !method.is_empty()
                        && method.len() <= 128
                        && method.contains('.')
                        && method
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_'))
                })
                .ok_or_else(|| {
                    failure("method must be a Domain.command string of at most 128 bytes")
                })?;
            let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
            if !params.is_object() {
                return Err(failure("params must be an object"));
            }
            Ok(Command {
                method: method.to_owned(),
                params,
            })
        })
        .collect()
}

fn address(endpoint: &url::Url) -> Result<SocketAddr> {
    if endpoint.scheme() != "ws"
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(failure(
            "Expected a loopback ws:// endpoint without credentials or a fragment",
        ));
    }
    let host = match endpoint.host() {
        Some(url::Host::Ipv4(ip)) if ip.is_loopback() => IpAddr::V4(ip),
        Some(url::Host::Ipv6(ip)) if ip.is_loopback() => IpAddr::V6(ip),
        Some(url::Host::Domain("localhost")) => IpAddr::V4(Ipv4Addr::LOCALHOST),
        _ => return Err(failure("The debugging endpoint must be on loopback")),
    };
    Ok(SocketAddr::new(
        host,
        endpoint.port_or_known_default().unwrap_or(80),
    ))
}

fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| failure("Command batch exceeded its 30-second deadline"))
}

fn timeout(socket: &mut WebSocket<TcpStream>, deadline: Instant) -> Result<()> {
    let remaining = remaining(deadline)?;
    socket.get_mut().set_read_timeout(Some(remaining))?;
    socket.get_mut().set_write_timeout(Some(remaining))?;
    Ok(())
}

fn json_line(output: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

struct Screenshot {
    id: u64,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

impl Screenshot {
    fn decode(id: u64, data: &str) -> Result<Self> {
        let bytes = STANDARD.decode(data)?;
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(failure("captureScreenshot did not return PNG data"));
        }
        let (width, height) =
            image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Png)
                .into_dimensions()?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_777_216 {
            return Err(failure(
                "Screenshot exceeds the 16-megapixel decoding bound",
            ));
        }
        // Decode the full stream, not only the PNG header, before accepting it.
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?;
        Ok(Self {
            id,
            bytes,
            width,
            height,
        })
    }

    fn metadata(&self) -> Value {
        json!({"format": "png", "bytes": self.bytes.len(), "width": self.width, "height": self.height})
    }

    fn save(&self, path: &Path, output: &mut impl Write) -> Result<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, &self.bytes)?;
        json_line(
            output,
            &json!({"screenshot": {
                "responseId": self.id, "path": path, "format": "png", "bytes": self.bytes.len(),
                "width": self.width, "height": self.height,
            }}),
        )
    }
}

fn run_batch(
    endpoint: &url::Url,
    commands: &[Command],
    screenshot_path: Option<&Path>,
    output: &mut impl Write,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let stream = TcpStream::connect_timeout(&address(endpoint)?, remaining(deadline)?)?;
    stream.set_read_timeout(Some(remaining(deadline)?))?;
    stream.set_write_timeout(Some(remaining(deadline)?))?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_REPLY))
        .max_frame_size(Some(MAX_REPLY));
    let (mut socket, _) =
        tungstenite::client::client_with_config(endpoint.as_str(), stream, Some(config))?;
    let mut screenshot = None;
    let mut protocol_error = None;
    let mut received = 0usize;
    for (index, command) in commands.iter().enumerate() {
        let id = index as u64 + 1;
        timeout(&mut socket, deadline)?;
        socket.send(Message::Text(
            json!({"id": id, "method": command.method, "params": command.params})
                .to_string()
                .into(),
        ))?;
        loop {
            timeout(&mut socket, deadline)?;
            let message = match socket.read()? {
                Message::Text(text) => text,
                Message::Ping(_) | Message::Pong(_) => {
                    socket.flush()?;
                    continue;
                }
                Message::Close(_) => {
                    return Err(failure("CDP connection closed before the batch completed"));
                }
                _ => return Err(failure("Expected a CDP JSON text message")),
            };
            received += 1;
            if received > 4096 {
                return Err(failure("Batch exceeded 4096 protocol messages"));
            }
            let mut message: Value = serde_json::from_str(message.as_str())?;
            if message.get("id").is_none() {
                if message.get("method").and_then(Value::as_str).is_none() {
                    return Err(failure("Received neither a CDP response nor an event"));
                }
                // Print notifications immediately. They never satisfy the wait
                // for the outstanding response, even when navigation is delayed.
                json_line(output, &message)?;
                continue;
            }
            if message["id"].as_u64() != Some(id) || message.get("sessionId").is_some() {
                return Err(failure(
                    "Unexpected response ID or unsupported session response",
                ));
            }
            if message.get("result").is_some() == message.get("error").is_some() {
                return Err(failure(
                    "CDP response must contain exactly one of result or error",
                ));
            }
            if command.method == "Page.captureScreenshot"
                && screenshot_path.is_some()
                && message.get("error").is_none()
            {
                let data = message
                    .pointer("/result/data")
                    .and_then(Value::as_str)
                    .ok_or_else(|| failure("captureScreenshot omitted its data"))?;
                let capture = Screenshot::decode(id, data)?;
                message["result"]
                    .as_object_mut()
                    .ok_or_else(|| failure("Screenshot result must be an object"))?
                    .remove("data");
                message["result"]["screenshot"] = capture.metadata();
                screenshot = Some(capture);
            }
            json_line(output, &message)?;
            if message.get("error").is_some() || message.pointer("/result/errorText").is_some() {
                protocol_error = Some(failure(format!(
                    "{} failed; see the JSON response",
                    command.method
                )));
            }
            break;
        }
        if protocol_error.is_some() {
            break;
        }
    }
    remaining(deadline)?;
    if let (Some(capture), Some(path)) = (screenshot, screenshot_path) {
        capture.save(path, output)?;
    }
    timeout(&mut socket, deadline)?;
    let _ = socket.close(None);
    remaining(deadline)?;
    if let Some(error) = protocol_error {
        return Err(error);
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        println!(
            "cdp_command ws://127.0.0.1:PORT/devtools/page/page-1 '<command-object-or-array>' [--screenshot PATH]"
        );
        return Ok(());
    }
    if !(args.len() == 2 || args.len() == 4 && args[2] == "--screenshot") {
        return Err(failure(
            "Usage: cdp_command <local-ws-url> '<command-object-or-array>' [--screenshot PATH]",
        ));
    }
    // Validate the entire batch and destination before creating a connection.
    let endpoint = url::Url::parse(&args[0])?;
    address(&endpoint)?;
    let commands = commands(&args[1])?;
    let screenshot = args.get(3).map(PathBuf::from);
    if screenshot
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(failure("Screenshot path must not be empty"));
    }
    if screenshot.is_some()
        && !commands
            .iter()
            .any(|command| command.method == "Page.captureScreenshot")
    {
        return Err(failure(
            "--screenshot requires a Page.captureScreenshot command",
        ));
    }
    run_batch(
        &endpoint,
        &commands,
        screenshot.as_deref(),
        &mut io::stdout().lock(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    #[test]
    fn rejects_unsupported_batch_envelopes_before_connecting() {
        for payload in [
            "[]",
            "42",
            "[{}]",
            r#"{"method":"DOM.getDocument","id":1}"#,
            r#"{"method":"DOM.getDocument","sessionId":"s"}"#,
            r#"{"method":"DOM.getDocument","params":null}"#,
        ] {
            assert!(commands(payload).is_err(), "accepted {payload}");
        }
        assert_eq!(
            commands(r#"{"method":"DOM.getDocument"}"#).unwrap()[0].params,
            json!({})
        );
        assert!(
            address(&url::Url::parse("ws://example.org/devtools/page/page-1").unwrap()).is_err()
        );
    }

    #[test]
    fn same_connection_batch_prints_events_without_losing_responses() {
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
            for (index, kind) in ["mousePressed", "mouseReleased"].iter().enumerate() {
                let request: Value =
                    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(request["id"], index + 1);
                assert_eq!(request["params"]["type"], *kind);
                socket
                    .send(Message::Text(
                        json!({"method":"DOM.documentUpdated","params":{}})
                            .to_string()
                            .into(),
                    ))
                    .unwrap();
                socket
                    .send(Message::Text(
                        json!({"id": index + 1,"result":{}}).to_string().into(),
                    ))
                    .unwrap();
            }
        });
        let endpoint = url::Url::parse(&format!("ws://{address}/devtools/page/page-1")).unwrap();
        let commands = commands(r#"[{"method":"Input.dispatchMouseEvent","params":{"type":"mousePressed"}},{"method":"Input.dispatchMouseEvent","params":{"type":"mouseReleased"}}]"#).unwrap();
        let mut output = Vec::new();
        run_batch(&endpoint, &commands, None, &mut output).unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0]["method"], "DOM.documentUpdated");
        assert_eq!(lines[1]["id"], 1);
        assert_eq!(lines[2]["method"], "DOM.documentUpdated");
        assert_eq!(lines[3]["id"], 2);
        server.join().unwrap();
    }
}
