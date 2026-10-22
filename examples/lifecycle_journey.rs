//! External, loopback-only CDP lifecycle acceptance. No Runtime execution.
//! Usage: lifecycle_journey ws://127.0.0.1:PORT/devtools/page/page-1
use serde_json::{Value, json};
use std::{
    error::Error,
    io,
    net::{IpAddr, SocketAddr, TcpStream},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const FIXTURE: &str = "http://127.0.0.1:7878/lifecycle-events";
const ORDER: &str = "script:loading,script-job:loading,ready:interactive,ready-job:interactive,defer:interactive,defer-job:interactive,early-message:interactive,message-job:interactive,early-timer:interactive,timer-job:interactive,dcl-capture:interactive,dcl:interactive,dcl-job:interactive,dcl-bubble:interactive,ready:complete,ready-job:complete,load:complete,load-job:complete";

fn fail(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::other(message.into()).into()
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(fail(message))
    }
}
struct Client {
    socket: WebSocket<TcpStream>,
    deadline: Instant,
    next: u64,
    events: Vec<Value>,
    event_bytes: usize,
}
impl Client {
    fn connect(endpoint: &url::Url) -> Result<Self> {
        let address = match endpoint.host() {
            Some(url::Host::Ipv4(ip)) if ip.is_loopback() => IpAddr::V4(ip),
            _ => return Err(fail("Expected an IPv4 loopback CDP endpoint")),
        };
        require(
            endpoint.scheme() == "ws"
                && endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.path() == "/devtools/page/page-1"
                && endpoint.query().is_none()
                && endpoint.fragment().is_none(),
            "Expected the owned page-1 ws endpoint without credentials/query/fragment",
        )?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let stream = TcpStream::connect_timeout(
            &SocketAddr::new(
                address,
                endpoint
                    .port()
                    .ok_or_else(|| fail("Explicit CDP port required"))?,
            ),
            Duration::from_secs(5),
        )?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(8 * 1024 * 1024))
            .max_frame_size(Some(8 * 1024 * 1024));
        let (socket, _) =
            tungstenite::client::client_with_config(endpoint.as_str(), stream, Some(config))?;
        Ok(Self {
            socket,
            deadline,
            next: 1,
            events: Vec::new(),
            event_bytes: 0,
        })
    }
    fn remaining(&self) -> Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|time| !time.is_zero())
            .ok_or_else(|| fail("Lifecycle CDP acceptance exceeded 30 seconds"))
    }
    fn command(&mut self, method: &str, params: Value) -> Result<Value> {
        let timeout = self.remaining()?;
        self.socket.get_mut().set_read_timeout(Some(timeout))?;
        self.socket.get_mut().set_write_timeout(Some(timeout))?;
        let id = self.next;
        self.next += 1;
        self.socket.send(Message::Text(
            json!({"id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))?;
        loop {
            let timeout = self.remaining()?;
            self.socket.get_mut().set_read_timeout(Some(timeout))?;
            match self.socket.read()? {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(text.as_str())?;
                    if let Some(response) = value.get("id") {
                        require(
                            response.as_u64() == Some(id) && value.get("sessionId").is_none(),
                            "Unexpected CDP response identity",
                        )?;
                        require(
                            value.get("error").is_none(),
                            &format!("CDP {method} failed: {value}"),
                        )?;
                        return value
                            .get("result")
                            .cloned()
                            .ok_or_else(|| fail("Missing CDP command result"));
                    }
                    require(
                        value["method"].is_string() && value.get("sessionId").is_none(),
                        "Invalid direct-page CDP event",
                    )?;
                    require(
                        self.events.len() < 256 && self.event_bytes + text.len() <= 1024 * 1024,
                        "Lifecycle CDP event queue exceeded its bounds",
                    )?;
                    self.event_bytes += text.len();
                    self.events.push(value);
                }
                Message::Ping(_) | Message::Pong(_) => self.socket.flush()?,
                _ => {
                    return Err(fail(
                        "CDP closed or returned a non-JSON frame before completion",
                    ));
                }
            }
        }
    }
}
fn attribute(tree: &Value, name: &str, expected: &str, depth: usize) -> bool {
    if depth > 256 {
        return false;
    }
    tree["attributes"].as_array().is_some_and(|values| {
        values
            .chunks_exact(2)
            .any(|pair| pair[0].as_str() == Some(name) && pair[1].as_str() == Some(expected))
    }) || tree["children"].as_array().is_some_and(|children| {
        children
            .iter()
            .any(|child| attribute(child, name, expected, depth + 1))
    })
}
fn validate_events(events: &[Value]) -> Result<Value> {
    let committed: Vec<_> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            event["method"] == "Page.frameNavigated"
                && event.pointer("/params/frame/url").and_then(Value::as_str) == Some(FIXTURE)
        })
        .collect();
    require(
        committed.len() == 1,
        "Expected exactly one actual fixture document commit",
    )?;
    let commit = committed[0].0;
    let begin = events[..commit]
        .iter()
        .rposition(|event| event["method"] == "Page.frameStartedLoading")
        .ok_or_else(|| fail("Missing fixture start before commit"))?;
    let expected = [
        "Page.frameStartedLoading",
        "Page.frameNavigated",
        "Page.domContentEventFired",
        "Page.loadEventFired",
        "Page.frameStoppedLoading",
    ];
    let observed: Vec<_> = events[begin..]
        .iter()
        .filter(|event| expected.contains(&event["method"].as_str().unwrap_or("")))
        .collect();
    require(
        observed
            .iter()
            .map(|event| event["method"].as_str().unwrap())
            .eq(expected),
        "Missing, duplicated, out-of-order or repeated lifecycle events",
    )?;
    let dcl = observed[2]["params"]["timestamp"]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(|| fail("Missing finite DCL timestamp"))?;
    let load = observed[3]["params"]["timestamp"]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= dcl)
        .ok_or_else(|| fail("Missing/nonmonotonic load timestamp"))?;
    let load_index = events
        .iter()
        .position(|event| std::ptr::eq(event, observed[3]))
        .ok_or_else(|| fail("Lost load event identity"))?;
    require(
        events[load_index + 1..]
            .iter()
            .any(|event| event["method"] == "DOM.documentUpdated"),
        "Later task projection did not invalidate DOM after load",
    )?;
    let frame_id = observed[1]
        .pointer("/params/frame/id")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("Commit omitted frame identity"))?;
    require(
        observed[0]["params"]["frameId"].as_str() == Some(frame_id)
            && observed[4]["params"]["frameId"].as_str() == Some(frame_id),
        "Lifecycle events changed frame identity",
    )?;
    Ok(
        json!({"events":expected,"dcl_timestamp":dcl,"load_timestamp":load,"later_dom_projection":true,"frame_id":frame_id}),
    )
}
fn run(endpoint: url::Url) -> Result<()> {
    let mut client = Client::connect(&endpoint)?;
    client.command("Page.enable", json!({}))?;
    client.command("DOM.enable", json!({}))?;
    client.command("Page.getFrameTree", json!({}))?; // Flush earlier initial-page events.
    client.events.clear();
    client.event_bytes = 0;
    let navigate = client.command("Page.navigate", json!({"url":FIXTURE}))?;
    require(
        navigate.get("errorText").is_none(),
        "Fixture navigation did not complete successfully",
    )?;
    loop {
        client.remaining()?;
        let document = client.command("DOM.getDocument", json!({"depth":-1}))?;
        if attribute(
            &document["root"],
            "data-lifecycle-later",
            "after-load-task-and-checkpoint",
            0,
        ) {
            require(
                attribute(&document["root"], "data-lifecycle-order", ORDER, 0),
                "Queued/deferred readiness order proof missing",
            )?;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // A command round trip is an event-delivery barrier after the later snapshot.
    let current = client.command("Page.getFrameTree", json!({}))?;
    require(
        current
            .pointer("/frameTree/frame/url")
            .and_then(Value::as_str)
            == Some(FIXTURE),
        "Fixture unexpectedly navigated",
    )?;
    let evidence = validate_events(&client.events)?;
    println!(
        "CDP_LIFECYCLE_OK {}",
        json!({"scope":"authored queued lifecycle, not WPT/testharness completion","order":ORDER,"evidence":evidence})
    );
    client.socket.close(None)?;
    Ok(())
}
fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let endpoint = arguments
        .next()
        .ok_or_else(|| fail("Usage: lifecycle_journey ws://127.0.0.1:PORT/devtools/page/page-1"))?;
    require(
        arguments.next().is_none(),
        "Exactly one owned CDP endpoint is required",
    )?;
    run(url::Url::parse(&endpoint)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn events() -> Vec<Value> {
        vec![
            json!({"method":"Page.frameStartedLoading","params":{"frameId":"frame-1"}}),
            json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame-1","url":FIXTURE}}}),
            json!({"method":"Page.domContentEventFired","params":{"timestamp":1.0}}),
            json!({"method":"Page.loadEventFired","params":{"timestamp":2.0}}),
            json!({"method":"Page.frameStoppedLoading","params":{"frameId":"frame-1"}}),
            json!({"method":"DOM.documentUpdated","params":{}}),
        ]
    }
    #[test]
    fn exact_events_accept_later_projection_without_repeating_load() {
        validate_events(&events()).unwrap();
    }
    #[test]
    fn missing_duplicate_regressing_or_changed_identity_events_reject() {
        for index in 0..6 {
            let mut input = events();
            input.remove(index);
            assert!(validate_events(&input).is_err());
        }
        let mut duplicate = events();
        duplicate.push(events()[3].clone());
        assert!(validate_events(&duplicate).is_err());
        let mut reversed = events();
        reversed.swap(2, 3);
        assert!(validate_events(&reversed).is_err());
        let mut time = events();
        time[3]["params"]["timestamp"] = json!(0.5);
        assert!(validate_events(&time).is_err());
        let mut identity = events();
        identity[4]["params"]["frameId"] = json!("frame-2");
        assert!(validate_events(&identity).is_err());
    }
}
