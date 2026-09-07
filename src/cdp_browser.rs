//! CDP application semantics. Transport never mutates window state directly.
use super::{App, Focus, Item, TOP};
use base64::Engine;
use mg_deps::cdp::{Command, Server};
use serde_json::{Value, json};
use std::{collections::HashMap, io::Cursor, time::Instant};

const FRAME: &str = "frame-1";
const TARGET: &str = "page-1";
const NODE_STRIDE: u64 = 100_000;

pub struct LayoutBox {
    node: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub enum Event {
    Started,
    Finished {
        generation: u64,
        frame: Value,
        error: Option<String>,
    },
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct Route {
    client: u64,
    session: Option<String>,
}
#[derive(Default)]
struct Scope {
    page: bool,
    dom: bool,
    pressed: Option<(i32, i32)>,
}
struct Pending {
    route: Route,
    id: Value,
    generation: u64,
}
pub struct BrowserCdp {
    server: Server,
    scopes: HashMap<Route, Scope>,
    pending: Vec<Pending>,
    next_session: u64,
    started: Instant,
}

type Reply = Result<Value, (i32, String)>;
fn invalid(message: impl Into<String>) -> (i32, String) {
    (-32602, message.into())
}
fn failed(message: impl Into<String>) -> (i32, String) {
    (-32000, message.into())
}
fn string<'a>(p: &'a Value, key: &str) -> Result<&'a str, (i32, String)> {
    p.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{key} must be a string")))
}
fn keys(p: &Value, supported: &[&str]) -> Result<(), (i32, String)> {
    let object = p
        .as_object()
        .ok_or_else(|| invalid("params must be an object"))?;
    for key in object.keys() {
        if !supported.contains(&key.as_str()) {
            return Err(invalid(format!("Unsupported parameter: {key}")));
        }
    }
    Ok(())
}
fn boolean(p: &Value, key: &str, default: bool) -> Result<bool, (i32, String)> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| invalid(format!("{key} must be a boolean"))),
    }
}

pub fn frame(app: &App) -> Value {
    let origin = url::Url::parse(&app.page_url)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or("null".into());
    let (url, fragment) = app
        .page_url
        .split_once('#')
        .map(|(url, fragment)| (url, Some(format!("#{fragment}"))))
        .unwrap_or((&app.page_url, None));
    let mut frame = json!({"id":FRAME,"loaderId":app.page_loader.to_string(),"url":url,
        "securityOrigin":origin,"mimeType":app.page_mime});
    if let Some(fragment) = fragment {
        frame["urlFragment"] = json!(fragment);
    }
    frame
}

impl BrowserCdp {
    pub fn bind(port: u16) -> Result<Self, String> {
        let server = Server::bind(port)?;
        eprintln!(
            "CDP listening on ws://127.0.0.1:{}/devtools/browser/browser-1 (experimental subset; local clients control this page)",
            server.port()
        );
        Ok(Self {
            server,
            scopes: HashMap::new(),
            pending: Vec::new(),
            next_session: 1,
            started: Instant::now(),
        })
    }
    pub fn tick(&mut self, app: &mut App) {
        let clients = self.server.client_ids();
        self.scopes
            .retain(|route, _| clients.contains(&route.client));
        self.pending.retain(|p| clients.contains(&p.route.client));
        self.flush_events(app);
        // Bound work per window frame so a busy client cannot monopolize input.
        for _ in 0..16 {
            let Some(command) = self.server.try_recv() else {
                break;
            };
            self.command(app, command);
            self.flush_events(app);
        }
        self.server.update_page(&app.document.title, &app.page_url);
    }
    fn reply(&self, route: &Route, id: Value, result: Reply) {
        let mut message = match result {
            Ok(result) => json!({"id":id,"result":result}),
            Err((code, message)) => json!({"id":id,"error":{"code":code,"message":message}}),
        };
        if let Some(session) = &route.session {
            message["sessionId"] = json!(session);
        }
        self.server.send(route.client, message);
    }
    fn event(&self, route: &Route, method: &str, params: Value) {
        let mut message = json!({"method":method,"params":params});
        if let Some(session) = &route.session {
            message["sessionId"] = json!(session);
        }
        self.server.send(route.client, message);
    }
    fn flush_events(&mut self, app: &mut App) {
        for event in std::mem::take(&mut app.cdp_events) {
            match event {
                Event::Started => {
                    for (route, scope) in &self.scopes {
                        if scope.page {
                            self.event(route, "Page.frameStartedLoading", json!({"frameId":FRAME}));
                        }
                    }
                }
                Event::Finished {
                    generation,
                    frame,
                    error,
                } => {
                    let pending = std::mem::take(&mut self.pending);
                    for p in pending {
                        if p.generation == generation {
                            let mut result =
                                json!({"frameId":FRAME,"loaderId":generation.to_string()});
                            if let Some(error) = &error {
                                result["errorText"] = json!(error);
                            }
                            self.reply(&p.route, p.id, Ok(result));
                        } else {
                            self.pending.push(p);
                        }
                    }
                    for scope in self.scopes.values_mut() {
                        scope.pressed = None;
                    }
                    let timestamp = self.started.elapsed().as_secs_f64();
                    for (route, scope) in &self.scopes {
                        if scope.dom {
                            self.event(route, "DOM.documentUpdated", json!({}));
                        }
                        if scope.page {
                            self.event(
                                route,
                                "Page.frameNavigated",
                                json!({"frame":frame,"type":"Navigation"}),
                            );
                            if error.is_none() {
                                self.event(
                                    route,
                                    "Page.domContentEventFired",
                                    json!({"timestamp":timestamp}),
                                );
                                self.event(
                                    route,
                                    "Page.loadEventFired",
                                    json!({"timestamp":timestamp}),
                                );
                            }
                            self.event(route, "Page.frameStoppedLoading", json!({"frameId":FRAME}));
                        }
                    }
                }
            }
        }
        let pending = std::mem::take(&mut self.pending);
        for p in pending {
            if p.generation != app.generation {
                self.reply(&p.route, p.id, Ok(json!({"frameId":FRAME,"errorText":"net::ERR_ABORTED: superseded navigation"})));
            } else {
                self.pending.push(p);
            }
        }
    }
    fn target_info(&self, app: &App) -> Value {
        json!({"targetId":TARGET,"type":"page","title":app.document.title,"url":app.page_url,
            "attached":!self.scopes.is_empty() || !self.server.page_client_ids().is_empty(),"canAccessOpener":false})
    }
    fn command(&mut self, app: &mut App, command: Command) {
        let message = &command.message;
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let route = Route {
            client: command.client,
            session: message
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        if id.as_u64().is_none()
            || message.get("method").and_then(Value::as_str).is_none()
            || message.get("sessionId").is_some_and(|s| !s.is_string())
        {
            self.reply(
                &route,
                id,
                Err((
                    -32600,
                    "Expected nonnegative integer id, method string, and optional sessionId string"
                        .into(),
                )),
            );
            return;
        }
        if route.session.is_some() && (!command.browser || !self.scopes.contains_key(&route)) {
            self.reply(&route, id, Err(failed("Unknown or detached sessionId")));
            return;
        }
        let method = message["method"].as_str().unwrap();
        let params = message.get("params").cloned().unwrap_or(json!({}));
        let result = self.dispatch(app, &route, command.browser, method, &params);
        match result {
            Ok(result) if method == "Page.navigate" => {
                if result.get("errorText").is_some() {
                    self.reply(&route, id, Ok(result));
                } else {
                    self.pending.push(Pending {
                        route,
                        id,
                        generation: app.generation,
                    });
                }
            }
            result => self.reply(&route, id, result),
        }
    }
    fn dispatch(
        &mut self,
        app: &mut App,
        route: &Route,
        browser: bool,
        method: &str,
        p: &Value,
    ) -> Reply {
        match method {
            "Browser.getVersion" => {
                keys(p, &[])?;
                return Ok(
                    json!({"protocolVersion":"1.3","product":concat!("mgbrowser/",env!("CARGO_PKG_VERSION")),
                    "revision":"experimental-cdp-subset","userAgent":mg_deps::net::USER_AGENT,"jsVersion":""}),
                );
            }
            "Target.getTargets" => {
                keys(p, &[])?;
                return Ok(json!({"targetInfos":[self.target_info(app)]}));
            }
            "Target.getTargetInfo" => {
                keys(p, &["targetId"])?;
                if p.get("targetId").is_some() && string(p, "targetId")? != TARGET {
                    return Err(failed("Unknown target"));
                }
                return Ok(json!({"targetInfo":self.target_info(app)}));
            }
            "Target.attachToTarget" => {
                keys(p, &["targetId", "flatten"])?;
                if !browser || route.session.is_some() {
                    return Err(invalid("Attach on the browser endpoint without sessionId"));
                }
                if string(p, "targetId")? != TARGET {
                    return Err(failed("Unknown target"));
                }
                if !boolean(p, "flatten", false)? {
                    return Err(invalid("Only flatten:true sessions are supported"));
                }
                if self
                    .scopes
                    .keys()
                    .filter(|r| r.client == route.client)
                    .count()
                    >= 8
                {
                    return Err(failed("Session limit reached"));
                }
                let session = format!("session-{}", self.next_session);
                self.next_session += 1;
                self.scopes.insert(
                    Route {
                        client: route.client,
                        session: Some(session.clone()),
                    },
                    Scope::default(),
                );
                self.event(route,"Target.attachedToTarget",json!({"sessionId":session,"targetInfo":self.target_info(app),"waitingForDebugger":false}));
                return Ok(json!({"sessionId":session}));
            }
            "Target.detachFromTarget" => {
                keys(p, &["sessionId"])?;
                if !browser || route.session.is_some() {
                    return Err(invalid(
                        "Detach on the browser endpoint without outer sessionId",
                    ));
                }
                let session = string(p, "sessionId")?;
                let detached = Route {
                    client: route.client,
                    session: Some(session.into()),
                };
                if self.scopes.remove(&detached).is_none() {
                    return Err(failed("Unknown sessionId"));
                }
                self.pending.retain(|p| p.route != detached);
                self.event(
                    route,
                    "Target.detachedFromTarget",
                    json!({"sessionId":session}),
                );
                return Ok(json!({}));
            }
            _ => {}
        }
        let recognized = matches!(
            method,
            "Page.enable"
                | "Page.disable"
                | "Page.navigate"
                | "Page.reload"
                | "Page.getFrameTree"
                | "Page.getLayoutMetrics"
                | "Page.captureScreenshot"
                | "DOM.enable"
                | "DOM.disable"
                | "DOM.getDocument"
                | "DOM.querySelector"
                | "DOM.querySelectorAll"
                | "DOM.getAttributes"
                | "DOM.getBoxModel"
                | "DOM.focus"
                | "Input.dispatchMouseEvent"
                | "Input.dispatchKeyEvent"
                | "Input.insertText"
        );
        if !recognized {
            return Err((-32601, format!("{method} is not implemented by mgbrowser")));
        }
        if browser && route.session.is_none() {
            return Err(failed(
                "Page commands require an attached sessionId on the browser endpoint",
            ));
        }
        let scope = self.scopes.entry(route.clone()).or_default();
        match method {
            "Page.enable" | "Page.disable" => {
                keys(p, &[])?;
                scope.page = method.ends_with("enable");
                Ok(json!({}))
            }
            "DOM.enable" | "DOM.disable" => {
                keys(p, &[])?;
                scope.dom = method.ends_with("enable");
                Ok(json!({}))
            }
            "Page.navigate" => {
                keys(p, &["url", "frameId"])?;
                if p.get("frameId").is_some() && string(p, "frameId")? != FRAME {
                    return Err(failed("Unknown frameId"));
                }
                let target = string(p, "url")?;
                if target.len() > 16_384 {
                    return Err(invalid("URL too long"));
                }
                let url = url::Url::parse(target).map_err(|e| invalid(e.to_string()))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(invalid("Only HTTP and HTTPS navigation is supported"));
                }
                if app.inflight >= 2 {
                    return Err(failed("Navigation busy; two requests are still finishing"));
                }
                app.navigate(url.to_string(), None, true);
                Ok(json!({"frameId":FRAME}))
            }
            "Page.reload" => {
                keys(p, &["ignoreCache"])?;
                boolean(p, "ignoreCache", false)?; // No persistent resource cache exists yet.
                if app.inflight >= 2 {
                    return Err(failed("Navigation busy"));
                }
                let target = if app.loading {
                    &app.address
                } else {
                    &app.page_url
                };
                let url = url::Url::parse(target).map_err(|e| failed(e.to_string()))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(failed("No HTTP(S) page to reload yet"));
                }
                app.navigate(url.to_string(), None, false);
                Ok(json!({}))
            }
            "Page.getFrameTree" => {
                keys(p, &[])?;
                Ok(json!({"frameTree":{"frame":frame(app)}}))
            }
            "Page.getLayoutMetrics" => {
                keys(p, &[])?;
                app.refresh_layout();
                Ok(app.metrics())
            }
            "Page.captureScreenshot" => {
                keys(
                    p,
                    &["format", "fromSurface", "captureBeyondViewport", "clip"],
                )?;
                if p.get("format").is_some() && string(p, "format")? != "png" {
                    return Err(invalid("Only PNG screenshots are supported"));
                }
                boolean(p, "fromSurface", true)?;
                if p.get("clip").is_some() || boolean(p, "captureBeyondViewport", false)? {
                    return Err(invalid(
                        "Only the current viewport is supported; no clip or captureBeyondViewport:true",
                    ));
                }
                app.screenshot().map(|data| json!({"data":data}))
            }
            "DOM.getDocument" => {
                keys(p, &["depth", "pierce"])?;
                if boolean(p, "pierce", false)? {
                    return Err(invalid("Shadow DOM/frame piercing is not implemented"));
                }
                let depth = match p.get("depth") {
                    None => 1,
                    Some(v) => v
                        .as_i64()
                        .ok_or_else(|| invalid("depth must be an integer"))?,
                };
                if !(-1..=256).contains(&depth) {
                    return Err(invalid("depth must be -1 or 0..256"));
                }
                scope.dom = true;
                let root = app.dom_node(0, if depth == -1 { 256 } else { depth as usize });
                if root.to_string().len() > 4 * 1024 * 1024 {
                    return Err(failed(
                        "DOM response exceeds 4 MiB; request a smaller depth",
                    ));
                }
                Ok(json!({"root":root}))
            }
            "DOM.querySelector" | "DOM.querySelectorAll" => {
                keys(p, &["nodeId", "selector"])?;
                let node = app.node_index(p)?;
                let selector = string(p, "selector")?;
                if method.ends_with("All") {
                    let nodes = app
                        .document
                        .query_selector_all(node, selector)
                        .map_err(invalid)?;
                    Ok(
                        json!({"nodeIds":nodes.into_iter().map(|i|app.node_id(i)).collect::<Vec<_>>()}),
                    )
                } else {
                    let node = app
                        .document
                        .query_selector(node, selector)
                        .map_err(invalid)?;
                    Ok(json!({"nodeId":node.map(|i|app.node_id(i)).unwrap_or(0)}))
                }
            }
            "DOM.getAttributes" => {
                keys(p, &["nodeId"])?;
                let node = app.node_index(p)?;
                if app.document.nodes[node].tag.starts_with('#') {
                    return Err(failed("Node is not an element"));
                }
                Ok(json!({"attributes":app.attributes(node)}))
            }
            "DOM.getBoxModel" => {
                keys(p, &["nodeId"])?;
                let node = app.node_index(p)?;
                app.refresh_layout();
                app.box_model(node)
            }
            "DOM.focus" => {
                keys(p, &["nodeId"])?;
                let node = app.node_index(p)?;
                let item = app
                    .document
                    .item_nodes
                    .iter()
                    .enumerate()
                    .find(|(i, n)| {
                        **n == node && matches!(app.document.items[*i], Item::Input { .. })
                    })
                    .map(|(i, _)| i)
                    .ok_or_else(|| failed("Node is not a supported editable input"))?;
                app.focus = Focus::Input(item);
                app.select_all = false;
                app.dirty = true;
                Ok(json!({}))
            }
            "Input.insertText" => {
                keys(p, &["text"])?;
                let text = string(p, "text")?;
                app.insert_cdp_text(text)?;
                Ok(json!({}))
            }
            "Input.dispatchKeyEvent" => {
                dispatch_key(app, p)?;
                Ok(json!({}))
            }
            "Input.dispatchMouseEvent" => {
                keys(
                    p,
                    &[
                        "type",
                        "x",
                        "y",
                        "button",
                        "buttons",
                        "clickCount",
                        "modifiers",
                        "deltaX",
                        "deltaY",
                    ],
                )?;
                let kind = string(p, "type")?;
                if !matches!(
                    kind,
                    "mousePressed" | "mouseReleased" | "mouseMoved" | "mouseWheel"
                ) {
                    return Err(invalid("Unsupported mouse event type"));
                }
                let x = coordinate(p, "x", app.width as f64)?;
                let y = coordinate(p, "y", app.viewport_height() as f64)?;
                let button = p
                    .get("button")
                    .map(|_| string(p, "button"))
                    .transpose()?
                    .unwrap_or("none");
                if !matches!(button, "left" | "none") {
                    return Err(invalid("Only the left mouse button is implemented"));
                }
                if matches!(kind, "mousePressed" | "mouseReleased") && button != "left" {
                    return Err(invalid("Press/release requires button:left"));
                }
                if p.get("clickCount")
                    .is_some_and(|v| !matches!(v.as_u64(), Some(0 | 1)))
                {
                    return Err(invalid("Only single clicks are implemented"));
                }
                if p.get("buttons")
                    .is_some_and(|v| !matches!(v.as_u64(), Some(0 | 1)))
                {
                    return Err(invalid("Only left/none buttons bitmasks are implemented"));
                }
                if p.get("modifiers").is_some_and(|v| v.as_u64() != Some(0)) {
                    return Err(invalid("Modified mouse events are not implemented"));
                }
                let delta_x = p
                    .get("deltaX")
                    .map(|v| v.as_f64().ok_or_else(|| invalid("deltaX must be a number")))
                    .transpose()?
                    .unwrap_or(0.);
                let delta_y = p
                    .get("deltaY")
                    .map(|v| v.as_f64().ok_or_else(|| invalid("deltaY must be a number")))
                    .transpose()?
                    .unwrap_or(0.);
                if delta_x != 0. || !delta_y.is_finite() || delta_y.abs() > 100_000. {
                    return Err(invalid("Only bounded vertical scrolling is implemented"));
                }
                app.refresh_layout();
                match kind {
                    "mousePressed" => scope.pressed = Some((x, y)),
                    "mouseReleased" => {
                        if let Some((px, py)) = scope.pressed.take() {
                            if (px - x).abs() <= 5 && (py - y).abs() <= 5 {
                                app.click(x, y + TOP);
                            }
                        }
                    }
                    "mouseWheel" => app.scroll_by(delta_y.round() as i32),
                    _ => {}
                }
                Ok(json!({}))
            }
            _ => unreachable!(),
        }
    }
}

fn coordinate(p: &Value, key: &str, bound: f64) -> Result<i32, (i32, String)> {
    let value = p
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| invalid(format!("{key} must be a number")))?;
    if !value.is_finite() || value < 0. || value >= bound {
        return Err(invalid(format!("{key} is outside the page viewport")));
    }
    Ok(value.floor() as i32)
}
fn dispatch_key(app: &mut App, p: &Value) -> Result<(), (i32, String)> {
    keys(
        p,
        &[
            "type",
            "key",
            "code",
            "text",
            "unmodifiedText",
            "windowsVirtualKeyCode",
            "nativeVirtualKeyCode",
            "modifiers",
            "autoRepeat",
        ],
    )?;
    let kind = string(p, "type")?;
    if !matches!(kind, "keyDown" | "rawKeyDown" | "keyUp" | "char") {
        return Err(invalid("Unsupported key event type"));
    }
    let modifiers = match p.get("modifiers") {
        None => 0,
        Some(v) => v
            .as_u64()
            .ok_or_else(|| invalid("modifiers must be an integer"))?,
    };
    if modifiers & !10 != 0 {
        return Err(invalid(
            "Only Control and Shift key modifiers are implemented",
        ));
    }
    boolean(p, "autoRepeat", false)?;
    for name in ["key", "code", "text", "unmodifiedText"] {
        if p.get(name).is_some() {
            string(p, name)?;
        }
    }
    for name in ["windowsVirtualKeyCode", "nativeVirtualKeyCode"] {
        if p.get(name).is_some_and(|v| v.as_u64().is_none()) {
            return Err(invalid(format!("{name} must be a nonnegative integer")));
        }
    }
    if app.focus == Focus::Address {
        app.focus = Focus::Page;
    }
    if kind == "keyUp" {
        return Ok(());
    }
    if kind == "char" {
        let text = string(p, "text")?;
        app.insert_cdp_text(text)?;
        return Ok(());
    }
    let key = p.get("key").and_then(Value::as_str).unwrap_or("");
    let sym = match key {
        "Enter" => 0xff0d,
        "Backspace" => 0xff08,
        "Tab" => 0xff09,
        "PageUp" => 0xff55,
        "PageDown" => 0xff56,
        "ArrowUp" => 0xff52,
        "ArrowDown" => 0xff54,
        "Shift" | "Control" => return Ok(()),
        "" => match p.get("windowsVirtualKeyCode").and_then(Value::as_u64) {
            Some(13) => 0xff0d,
            Some(8) => 0xff08,
            Some(9) => 0xff09,
            _ => return Err(invalid("Provide a supported key or virtual key code")),
        },
        value if value.chars().count() == 1 => value.chars().next().unwrap() as u32,
        _ => return Err(invalid("Unsupported key")),
    };
    let ctrl = modifiers & 2 != 0;
    if ctrl && !matches!(sym, 65 | 97) {
        return Err(invalid("Only Control+A is implemented for page input"));
    }
    if kind == "rawKeyDown" && !ctrl && sym < 0xff00 {
        return Ok(());
    }
    if !ctrl && sym < 0xff00 && p.get("text").is_some() {
        let text = string(p, "text")?;
        app.insert_cdp_text(text)?;
    } else if !ctrl && sym < 0xff00 {
        app.insert_cdp_text(
            &char::from_u32(sym)
                .ok_or_else(|| invalid("Invalid character key"))?
                .to_string(),
        )?;
    } else {
        app.key(sym, ctrl, modifiers & 8 != 0, false);
    }
    Ok(())
}

impl App {
    fn insert_cdp_text(&mut self, text: &str) -> Result<(), (i32, String)> {
        let Focus::Input(i) = self.focus else {
            return Err(failed("No editable page input has focus"));
        };
        let Some(Item::Input { value, .. }) = self.document.items.get(i) else {
            return Err(failed("Focused input is stale"));
        };
        let retained = if self.select_all {
            0
        } else {
            self.values.get(&i).unwrap_or(value).len()
        };
        if text.chars().any(char::is_control)
            || retained.saturating_add(text.len()) > super::MAX_EDIT_BYTES
        {
            return Err(invalid(
                "Input must be printable and its resulting value at most 8191 UTF-8 bytes",
            ));
        }
        self.type_text(text);
        Ok(())
    }
    pub(super) fn layout_box(&mut self, item: usize, x: i32, y: i32, width: u32, height: u32) {
        if self.boxes.len() < 100_000
            && let Some(&node) = self.document.item_nodes.get(item)
        {
            self.boxes.push(LayoutBox {
                node,
                x,
                y,
                width,
                height,
            });
        }
    }
    fn refresh_layout(&mut self) {
        if self.dirty {
            let _ = self.paint();
            self.dirty = true;
        }
    }
    fn viewport_height(&self) -> u32 {
        self.height.saturating_sub(TOP as u32 + 29)
    }
    fn node_id(&self, index: usize) -> u64 {
        self.dom_epoch * NODE_STRIDE + index as u64 + 1
    }
    fn node_index(&self, p: &Value) -> Result<usize, (i32, String)> {
        let id = p
            .get("nodeId")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid("nodeId must be a nonnegative integer"))?;
        let first = self.dom_epoch * NODE_STRIDE + 1;
        if id < first || id - first >= self.document.nodes.len() as u64 {
            return Err(failed(
                "Stale or unknown nodeId; call DOM.getDocument after navigation",
            ));
        }
        Ok((id - first) as usize)
    }
    fn attributes(&self, index: usize) -> Vec<String> {
        self.document.nodes[index]
            .attributes
            .iter()
            .flat_map(|(k, v)| [k.clone(), v.clone()])
            .collect()
    }
    fn dom_node(&self, index: usize, depth: usize) -> Value {
        let node = &self.document.nodes[index];
        let (kind, name, local) = match node.tag.as_str() {
            "#document" => (9, "#document".into(), String::new()),
            "#text" => (3, "#text".into(), String::new()),
            tag => (1, tag.to_ascii_uppercase(), tag.to_string()),
        };
        let mut result = json!({"nodeId":self.node_id(index),"backendNodeId":self.node_id(index),"nodeType":kind,"nodeName":name,"localName":local,"nodeValue":node.text,"childNodeCount":node.children.len()});
        if kind == 1 {
            result["attributes"] = json!(self.attributes(index));
        }
        if index == 0 {
            result["documentURL"] = json!(self.page_url);
            result["baseURL"] = json!(self.document.base_url);
        }
        if depth > 0 {
            result["children"] = json!(
                node.children
                    .iter()
                    .map(|&i| self.dom_node(i, depth - 1))
                    .collect::<Vec<_>>()
            );
        }
        result
    }
    fn metrics(&self) -> Value {
        let layout = json!({"pageX":0,"pageY":self.scroll,"clientWidth":self.width,"clientHeight":self.viewport_height()});
        let visual = json!({"offsetX":0,"offsetY":0,"pageX":0,"pageY":self.scroll,"clientWidth":self.width,"clientHeight":self.viewport_height(),"scale":1,"zoom":1});
        let content = json!({"x":0,"y":0,"width":self.width,"height":(self.content_height-TOP).max(self.viewport_height() as i32)});
        json!({"layoutViewport":layout,"visualViewport":visual,"contentSize":content,"cssLayoutViewport":layout,"cssVisualViewport":visual,"cssContentSize":content})
    }
    fn box_model(&self, node: usize) -> Reply {
        if self.boxes.len() >= 100_000 {
            return Err(failed(
                "Layout geometry limit reached; no partial box model is returned",
            ));
        }
        let mut bounds: Option<(i32, i32, i32, i32)> = None;
        let mut budget = 4_000_000usize;
        for rect in &self.boxes {
            let mut source = rect.node;
            for _ in 0..=256 {
                budget = budget
                    .checked_sub(1)
                    .ok_or_else(|| failed("Layout query traversal limit reached"))?;
                if source == node {
                    let (x, y, right, bottom) = (
                        rect.x,
                        rect.y - TOP,
                        rect.x + rect.width as i32,
                        rect.y - TOP + rect.height as i32,
                    );
                    bounds = Some(match bounds {
                        None => (x, y, right, bottom),
                        Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(right), d.max(bottom)),
                    });
                    break;
                }
                if source == 0 {
                    break;
                }
                source = self.document.nodes[source].parent;
            }
        }
        let (x, y, right, bottom) = bounds
            .ok_or_else(|| failed("Node has no rendered layout box in this browser subset"))?;
        if right <= 0 || x >= self.width as i32 || bottom <= 0 || y >= self.viewport_height() as i32
        {
            return Err(failed("Node is outside the current viewport; scroll first"));
        }
        let quad = json!([x, y, right, y, right, bottom, x, bottom]);
        Ok(
            json!({"model":{"content":quad,"padding":quad,"border":quad,"margin":quad,"width":right-x,"height":bottom-y}}),
        )
    }
    fn screenshot(&mut self) -> Result<String, (i32, String)> {
        let canvas = self.paint();
        self.dirty = true; // Inspection must not consume a pending native-window redraw.
        let height = self.viewport_height();
        let pixels: Vec<u8> = canvas
            .pixels
            .iter()
            .skip(TOP as usize * self.width as usize)
            .take(height as usize * self.width as usize)
            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
            .collect();
        let image = image::RgbImage::from_raw(self.width, height, pixels)
            .ok_or_else(|| failed("Invalid screenshot surface"))?;
        let mut output = Cursor::new(Vec::new());
        image
            .write_to(&mut output, image::ImageFormat::Png)
            .map_err(|e| failed(e.to_string()))?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(output.into_inner());
        if encoded.len() > 7 * 1024 * 1024 {
            return Err(failed("Encoded screenshot exceeds response limit"));
        }
        Ok(encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mg_deps::document;
    use std::{net::TcpStream, time::Duration};

    fn fixture() -> App {
        let mut app = App::new().unwrap();
        app.document = document::parse(
            "<html><head><title>CDP fixture</title></head><body><form><input name=q value=old><button>Go</button></form><a id=first href='/next'>First link</a><script>retained()</script></body></html>",
            "http://localhost/",
        );
        app.page_url = "http://localhost/".into();
        app
    }
    fn route() -> Route {
        Route {
            client: 1,
            session: None,
        }
    }
    fn input(app: &App) -> (usize, u64) {
        let node = app
            .document
            .query_selector(0, "input[name=q]")
            .unwrap()
            .unwrap();
        (node, app.node_id(node))
    }
    #[test]
    fn actual_dom_attributes_selectors_and_stale_nodes() {
        let mut app = fixture();
        let mut cdp = BrowserCdp::bind(0).unwrap();
        let r = route();
        let root = cdp
            .dispatch(&mut app, &r, false, "DOM.getDocument", &json!({"depth":-1}))
            .unwrap();
        assert_eq!(root["root"]["nodeType"], 9);
        assert!(root.to_string().contains("retained()"));
        assert!(cdp.scopes[&r].dom);
        let (_, id) = input(&app);
        let query = cdp
            .dispatch(
                &mut app,
                &r,
                false,
                "DOM.querySelector",
                &json!({"nodeId":root["root"]["nodeId"],"selector":"input[name=q]"}),
            )
            .unwrap();
        assert_eq!(query["nodeId"], id);
        let attrs = cdp
            .dispatch(
                &mut app,
                &r,
                false,
                "DOM.getAttributes",
                &json!({"nodeId":id}),
            )
            .unwrap();
        assert_eq!(attrs["attributes"], json!(["name", "q", "value", "old"]));
        app.dom_epoch += 1;
        assert_eq!(
            cdp.dispatch(&mut app, &r, false, "DOM.focus", &json!({"nodeId":id}))
                .unwrap_err()
                .0,
            -32000
        );
        assert_eq!(
            cdp.dispatch(
                &mut app,
                &r,
                false,
                "Runtime.evaluate",
                &json!({"expression":"1"})
            )
            .unwrap_err()
            .0,
            -32601
        );
    }
    #[test]
    fn input_is_atomic_bounded_and_raw_key_does_not_duplicate_char() {
        let mut app = fixture();
        let mut cdp = BrowserCdp::bind(0).unwrap();
        let r = route();
        let (_, id) = input(&app);
        cdp.dispatch(&mut app, &r, false, "DOM.focus", &json!({"nodeId":id}))
            .unwrap();
        let Focus::Input(i) = app.focus else {
            panic!("input focus")
        };
        app.select_all = true;
        let oversized = "x".repeat(8192);
        for (method, params) in [
            ("Input.insertText", json!({"text":oversized})),
            (
                "Input.dispatchKeyEvent",
                json!({"type":"char","text":oversized}),
            ),
            (
                "Input.dispatchKeyEvent",
                json!({"type":"keyDown","key":"x","text":oversized}),
            ),
        ] {
            assert_eq!(
                cdp.dispatch(&mut app, &r, false, method, &params)
                    .unwrap_err()
                    .0,
                -32602
            );
            assert!(app.select_all);
            assert_eq!(app.field_value(i, "old"), "old");
        }
        cdp.dispatch(
            &mut app,
            &r,
            false,
            "Input.dispatchKeyEvent",
            &json!({"type":"rawKeyDown","key":"x"}),
        )
        .unwrap();
        assert!(app.select_all);
        cdp.dispatch(
            &mut app,
            &r,
            false,
            "Input.dispatchKeyEvent",
            &json!({"type":"char","text":"é"}),
        )
        .unwrap();
        cdp.dispatch(
            &mut app,
            &r,
            false,
            "Input.dispatchKeyEvent",
            &json!({"type":"keyUp","key":"x"}),
        )
        .unwrap();
        assert_eq!(app.field_value(i, "old"), "é");
        app.insert_cdp_text(&"x".repeat(8189)).unwrap();
        assert_eq!(app.field_value(i, "old").len(), 8191);
        assert!(app.insert_cdp_text("y").is_err());
        assert_eq!(app.field_value(i, "old").len(), 8191);
    }
    #[test]
    fn viewport_geometry_scroll_click_and_png_exclude_browser_chrome() {
        let mut app = fixture();
        let mut cdp = BrowserCdp::bind(0).unwrap();
        let r = route();
        let (node, id) = input(&app);
        app.refresh_layout();
        let first = app.box_model(node).unwrap()["model"]["content"].clone();
        app.scroll = 12;
        app.dirty = true;
        let model = cdp
            .dispatch(
                &mut app,
                &r,
                false,
                "DOM.getBoxModel",
                &json!({"nodeId":id}),
            )
            .unwrap();
        let quad = &model["model"]["content"];
        assert_eq!(quad[1].as_i64().unwrap(), first[1].as_i64().unwrap() - 12);
        let x = quad[0].as_i64().unwrap() + 3;
        let y = quad[1].as_i64().unwrap() + 3;
        for kind in ["mousePressed", "mouseReleased"] {
            cdp.dispatch(
                &mut app,
                &r,
                false,
                "Input.dispatchMouseEvent",
                &json!({"type":kind,"x":x,"y":y,"button":"left"}),
            )
            .unwrap();
        }
        assert!(matches!(app.focus, Focus::Input(_)));
        assert!(
            cdp.dispatch(
                &mut app,
                &r,
                false,
                "Input.dispatchMouseEvent",
                &json!({"type":"mousePressed","x":3,"y":-1,"button":"left"})
            )
            .is_err()
        );
        let metrics = app.metrics();
        assert_eq!(metrics["cssLayoutViewport"]["pageY"], 12);
        let png = app.screenshot().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(png)
            .unwrap();
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (app.width, app.height - TOP as u32 - 29)
        );
        assert!(app.dirty);
        assert!(
            cdp.dispatch(
                &mut app,
                &r,
                false,
                "Page.captureScreenshot",
                &json!({"format":"jpeg"})
            )
            .is_err()
        );
        assert!(
            cdp.dispatch(
                &mut app,
                &r,
                false,
                "Page.captureScreenshot",
                &json!({"captureBeyondViewport":true})
            )
            .is_err()
        );
    }
    #[test]
    fn attached_sessions_scope_enablement_and_reject_detached_commands() {
        let mut app = fixture();
        let mut cdp = BrowserCdp::bind(0).unwrap();
        let r = route();
        assert!(
            cdp.dispatch(&mut app, &r, true, "Page.getFrameTree", &json!({}))
                .is_err()
        );
        assert!(
            cdp.dispatch(
                &mut app,
                &r,
                true,
                "Target.attachToTarget",
                &json!({"targetId":TARGET,"flatten":false})
            )
            .is_err()
        );
        let one = cdp
            .dispatch(
                &mut app,
                &r,
                true,
                "Target.attachToTarget",
                &json!({"targetId":TARGET,"flatten":true}),
            )
            .unwrap();
        let two = cdp
            .dispatch(
                &mut app,
                &r,
                true,
                "Target.attachToTarget",
                &json!({"targetId":TARGET,"flatten":true}),
            )
            .unwrap();
        let first = Route {
            client: r.client,
            session: Some(one["sessionId"].as_str().unwrap().into()),
        };
        let second = Route {
            client: r.client,
            session: Some(two["sessionId"].as_str().unwrap().into()),
        };
        cdp.dispatch(&mut app, &first, true, "Page.enable", &json!({}))
            .unwrap();
        assert!(cdp.scopes[&first].page);
        assert!(!cdp.scopes[&second].page);
        cdp.dispatch(&mut app, &r, true, "Target.detachFromTarget", &one)
            .unwrap();
        cdp.command(
            &mut app,
            Command {
                client: r.client,
                browser: true,
                message: json!({"id":7,"method":"Page.enable","sessionId":one["sessionId"]}),
            },
        );
        assert!(!cdp.scopes.contains_key(&first));
        assert!(cdp.scopes.contains_key(&second));
    }
    #[test]
    fn metadata_uses_committed_page_and_every_advertised_method_is_recognized() {
        let mut app = fixture();
        app.page_url = "https://example.org/path#part".into();
        app.address = "https://example.org/loading".into();
        let current = frame(&app);
        assert_eq!(current["url"], "https://example.org/path");
        assert_eq!(current["urlFragment"], "#part");
        let mut cdp = BrowserCdp::bind(0).unwrap();
        let version = cdp
            .dispatch(&mut app, &route(), false, "Browser.getVersion", &json!({}))
            .unwrap();
        assert_eq!(version["userAgent"], mg_deps::net::USER_AGENT);
        let schema: Value =
            serde_json::from_str(include_str!("../docs/cdp-protocol.json")).unwrap();
        for domain in schema["domains"].as_array().unwrap() {
            for command in domain["commands"].as_array().unwrap() {
                let method = format!(
                    "{}.{}",
                    domain["domain"].as_str().unwrap(),
                    command["name"].as_str().unwrap()
                );
                // Intentionally unsupported param prevents mutations; unknown methods
                // must not masquerade as implemented members of our published schema.
                let error = cdp
                    .dispatch(
                        &mut app,
                        &route(),
                        false,
                        &method,
                        &json!({"__invalid":true}),
                    )
                    .unwrap_err();
                assert_ne!(error.0, -32601, "advertised but unimplemented: {method}");
            }
        }
    }
    #[test]
    fn lifecycle_events_are_gated_and_navigation_failure_has_no_load_success() {
        let mut app = fixture();
        let mut cdp = BrowserCdp::bind(0).unwrap();
        let stream = TcpStream::connect(("127.0.0.1", cdp.server.port())).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let (mut socket, _) = tungstenite::client(
            format!("ws://127.0.0.1:{}/devtools/page/page-1", cdp.server.port()),
            stream,
        )
        .unwrap();
        let client = cdp.server.page_client_ids()[0];
        let r = Route {
            client,
            session: None,
        };
        assert_eq!(cdp.target_info(&app)["attached"], true);
        cdp.dispatch(&mut app, &r, false, "Page.disable", &json!({}))
            .unwrap();
        app.cdp_events.push(Event::Started);
        cdp.flush_events(&mut app);
        assert!(
            matches!(socket.read(),Err(tungstenite::Error::Io(e)) if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut))
        );
        cdp.dispatch(&mut app, &r, false, "Page.enable", &json!({}))
            .unwrap();
        cdp.dispatch(&mut app, &r, false, "DOM.enable", &json!({}))
            .unwrap();
        app.generation = 3;
        cdp.pending.push(Pending {
            route: r.clone(),
            id: json!(9),
            generation: 3,
        });
        app.cdp_events.push(Event::Started);
        app.cdp_events.push(Event::Finished {
            generation: 3,
            frame: frame(&app),
            error: Some("fixture transport failure".into()),
        });
        cdp.flush_events(&mut app);
        let mut messages = Vec::new();
        for _ in 0..5 {
            let message = socket.read().unwrap().into_text().unwrap();
            messages.push(serde_json::from_str::<Value>(&message).unwrap());
        }
        assert!(
            messages
                .iter()
                .any(|m| m["id"] == 9 && m["result"]["errorText"] == "fixture transport failure")
        );
        let events: Vec<_> = messages
            .iter()
            .filter_map(|m| m["method"].as_str())
            .collect();
        assert_eq!(
            events,
            vec![
                "Page.frameStartedLoading",
                "DOM.documentUpdated",
                "Page.frameNavigated",
                "Page.frameStoppedLoading"
            ]
        );
        cdp.pending.push(Pending {
            route: r,
            id: json!(10),
            generation: 2,
        });
        cdp.flush_events(&mut app);
        let aborted: Value =
            serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
        assert!(
            aborted["result"]["errorText"]
                .as_str()
                .unwrap()
                .contains("ERR_ABORTED")
        );
    }
}
