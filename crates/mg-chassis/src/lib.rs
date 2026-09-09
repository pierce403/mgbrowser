//! Chassis: browser state, services, and optional chrome with a host-supplied platform.
pub mod cdp;
mod cdp_browser;
#[cfg(feature = "chrome")]
mod chrome;
pub mod net;
pub mod scripts;
pub use cdp_browser::BrowserCdp;
use mg_sparkle::{
    document::{self, Document, Item},
    page_session::{ControlEdit, DefaultAction, InputKind, RealmState, SessionInput, SessionReply},
    paint::{Canvas, Fonts},
};
use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const TOP: i32 = 108;
const BG: u32 = 0xfafbf8;
#[cfg(feature = "chrome")]
const INK: u32 = 0x26342b;
#[cfg(feature = "chrome")]
const HTTP_CHROME: u32 = 0x9f202b;
const MAX_EDIT_BYTES: usize = 8191;

#[derive(Clone, Debug)]
enum Action {
    Address,
    Back,
    Forward,
    Reload,
    Input(usize),
    Submit(usize),
    Link { node: usize, href: String },
}
#[derive(Clone)]
struct Hit {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    action: Action,
}
impl Hit {
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w as i32 && y < self.y + self.h as i32
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Address,
    Input(usize),
    Page,
}
struct Loaded {
    generation: u64,
    result: Result<net::Response, String>,
    script: Option<Result<ScriptLoad, String>>,
}
struct ScriptLoad {
    session: Option<Box<dyn scripts::ScriptSession>>,
    reply: SessionReply,
}
struct PendingEvent {
    generation: u64,
    session_id: u64,
    sequence: u64,
    revision: u64,
    input: SessionInput,
}
pub struct Browser {
    chrome: bool,
    session: net::Session,
    fonts: Fonts,
    width: u32,
    height: u32,
    address: String,
    status: String,
    document: Document,
    page_url: String,
    page_mime: String,
    page_loader: u64,
    dom_epoch: u64,
    boxes: Vec<mg_sparkle::render::LayoutBox>,
    cdp_enabled: bool,
    cdp_events: Vec<cdp_browser::Event>,
    scripts_enabled: bool,
    child_pool: Arc<dyn scripts::ScriptRuntime>,
    page_session: Option<Box<dyn scripts::ScriptSession>>,
    page_revision: u64,
    pending_event: Option<PendingEvent>,
    script_blocked: bool,
    edit_versions: HashMap<usize, u64>,
    next_edit: u64,
    values: HashMap<usize, String>,
    hits: Vec<Hit>,
    pointer_press: Option<(i32, i32)>,
    focus: Focus,
    select_all: bool,
    scroll: i32,
    content_height: i32,
    history: Vec<String>,
    history_at: usize,
    generation: u64,
    inflight: usize,
    loading: bool,
    tx: mpsc::Sender<Loaded>,
    rx: mpsc::Receiver<Loaded>,
    dirty: bool,
    last_load_ok: bool,
    refresh_count: u8,
    smoke: Option<String>,
    smoke_events: bool,
    event_smoke_stage: u8,
    smoke_stage: u8,
    smoke_since: Instant,
    smoke_failed: bool,
    exit_after_smoke: bool,
    evidence_dir: String,
}

impl Drop for Browser {
    fn drop(&mut self) {
        // Fence network jobs before they can create a late child. The pool
        // retains ownership until every already-started child has been reaped.
        self.child_pool.close();
        if let Some(session) = self.page_session.take() {
            session.cancel();
            drop(session);
        }
        if let Err(error) = self.child_pool.wait_idle() {
            eprintln!("SCRIPT_SHUTDOWN_ERROR {error:?}");
        }
    }
}

impl Browser {
    pub fn new(fonts: Fonts, scripts: Arc<dyn scripts::ScriptRuntime>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            chrome: cfg!(feature = "chrome"),
            session: net::Session::default(),
            fonts,
            width: 1100,
            height: 820,
            address: String::new(),
            status: "Experimental browser · Rust document flow · JavaScript is opt-in".into(),
            document: document::parse(
                "<title>mgbrowser</title><h1>mgbrowser</h1><p>Enter a URL above to begin browsing.</p>",
                "https://mgbrowser.org/",
            ),
            values: HashMap::new(),
            page_url: "about:blank".into(),
            page_mime: "text/html".into(),
            page_loader: 0,
            dom_epoch: 1,
            boxes: Vec::new(),
            cdp_enabled: false,
            cdp_events: Vec::new(),
            scripts_enabled: false,
            child_pool: scripts,
            page_session: None,
            page_revision: 0,
            pending_event: None,
            script_blocked: false,
            edit_versions: HashMap::new(),
            next_edit: 0,
            hits: Vec::new(),
            pointer_press: None,
            focus: if cfg!(feature = "chrome") {
                Focus::Address
            } else {
                Focus::Page
            },
            select_all: false,
            scroll: 0,
            content_height: 0,
            history: Vec::new(),
            history_at: 0,
            generation: 0,
            inflight: 0,
            loading: false,
            tx,
            rx,
            dirty: true,
            last_load_ok: false,
            refresh_count: 0,
            smoke: None,
            smoke_events: false,
            event_smoke_stage: 0,
            smoke_stage: 0,
            smoke_since: Instant::now(),
            smoke_failed: false,
            exit_after_smoke: false,
            evidence_dir: "tmp/journey".into(),
        }
    }
    pub fn navigate(&mut self, target: String, body: Option<String>, add_history: bool) {
        self.navigate_inner(target, body, add_history, false);
    }
    fn navigate_inner(
        &mut self,
        target: String,
        body: Option<String>,
        add_history: bool,
        automatic: bool,
    ) {
        let target = if target.contains("://") {
            target
        } else {
            format!("https://{target}")
        };
        if self.inflight >= 2 {
            self.status = "Two requests are still finishing; please wait.".into();
            self.dirty = true;
            return;
        }
        self.generation += 1;
        if let Err(error) = self.child_pool.set_generation(self.generation) {
            self.block_session(&format!("Script child ownership error: {error}"));
            return;
        }
        let retired = self.page_session.take();
        if let Some(session) = &retired {
            session.cancel();
        }
        self.pending_event = None;
        self.pointer_press = None;
        if retired.is_some() {
            self.script_blocked = true;
        }
        if self.cdp_enabled {
            self.cdp_events.push(cdp_browser::Event::Started);
        }
        self.inflight += 1;
        self.loading = true;
        self.address = target.clone();
        self.focus = Focus::Page;
        self.status = format!("Loading {target}");
        self.dirty = true;
        if !automatic {
            self.refresh_count = 0;
        }
        if add_history {
            if !self.history.is_empty() {
                self.history.truncate(self.history_at + 1);
            }
            self.history.push(target.clone());
            self.history_at = self.history.len() - 1;
        }
        let generation = self.generation;
        let tx = self.tx.clone();
        let session = self.session.clone();
        let scripts_enabled = self.scripts_enabled;
        let pool = self.child_pool.clone();
        eprintln!(
            "NAVIGATE {} {}",
            if body.is_some() { "POST" } else { "GET" },
            target
        );
        thread::spawn(move || {
            // Join/reap a retired manager on this already bounded navigation job,
            // not by blocking the window thread or creating per-event threads.
            drop(retired);
            let result = session.submit(&target, body.as_deref());
            let script = if scripts_enabled {
                result
                    .as_ref()
                    .ok()
                    .filter(|response| {
                        response.content_type.contains("html") || response.content_type.is_empty()
                    })
                    .map(|response| {
                        pool.start(
                            mg_sparkle::js_browser::Request {
                                url: response.url.to_string(),
                                html: decode_text(&response.body, &response.content_type),
                            },
                            generation,
                        )
                        .map(|(session, reply)| ScriptLoad {
                            session: Some(session),
                            reply,
                        })
                    })
            } else {
                None
            };
            let _ = tx.send(Loaded {
                generation,
                result,
                script,
            });
        });
    }
    pub fn poll(&mut self) {
        while let Ok(loaded) = self.rx.try_recv() {
            self.inflight = self.inflight.saturating_sub(1);
            if loaded.generation != self.generation {
                continue;
            }
            self.loading = false;
            self.scroll = 0;
            self.values.clear();
            self.edit_versions.clear();
            self.pending_event = None;
            self.page_revision = 0;
            self.script_blocked = loaded.script.is_some();
            self.focus = Focus::Page;
            self.select_all = false;
            self.last_load_ok = false;
            let mut load_error = None;
            let mut script_navigation = None;
            match loaded.result {
                Ok(response) => {
                    self.last_load_ok = (200..300).contains(&response.status);
                    self.address = response.url.to_string();
                    self.page_mime = response
                        .content_type
                        .split(';')
                        .next()
                        .unwrap_or("text/html")
                        .to_string();
                    if !self.history.is_empty() {
                        self.history[self.history_at] = self.address.clone();
                    }
                    let source = decode_text(&response.body, &response.content_type);
                    let mut scripting = false;
                    let mut projected = None;
                    let script_status = match loaded.script {
                        Some(Ok(mut startup)) => {
                            let started = Instant::now();
                            let validation = if startup.reply.revision != 0
                                || !startup.reply.acknowledgements.is_empty()
                                || startup.reply.default_action != DefaultAction::None
                            {
                                Err("Invalid initial script session reply".into())
                            } else {
                                Self::project_reply(&mut startup.reply, &self.address, None)
                            };
                            let timed = startup
                                .session
                                .as_mut()
                                .ok_or_else(|| "Missing script session ownership".to_string())
                                .and_then(|session| session.charge_active(started.elapsed()));
                            match validation.and_then(|document| timed.map(|_| document)) {
                                Ok(document) => {
                                    projected = Some(document);
                                    scripting = true;
                                    self.page_revision = startup.reply.revision;
                                    self.script_blocked = startup.reply.state != RealmState::Ready;
                                    if !self.script_blocked {
                                        script_navigation = startup.reply.navigation.take();
                                        self.page_session = startup.session.take();
                                    } else if let Some(session) = &startup.session {
                                        session.cancel();
                                    }
                                    Self::script_summary(&startup.reply)
                                }
                                Err(error) => {
                                    if let Some(session) = &startup.session {
                                        session.cancel();
                                    }
                                    eprintln!("SCRIPT_REJECTED {error:?}");
                                    format!("JS rejected; original document retained: {error}")
                                }
                            }
                        }
                        Some(Err(error)) => {
                            eprintln!("SCRIPT_ERROR {error:?}");
                            format!("JS worker failed: {error}")
                        }
                        None => "JavaScript disabled".into(),
                    };
                    self.document = if let Some(document) = projected {
                        document
                    } else if response.content_type.contains("html")
                        || response.content_type.is_empty()
                    {
                        document::parse_with_scripting(&source, &self.address, scripting)
                    } else {
                        document::parse(
                            &format!(
                                "<title>Resource</title><h1>Resource received</h1><p>{}</p><p>This content type is not rendered yet.</p>",
                                escape(&response.content_type)
                            ),
                            &self.address,
                        )
                    };
                    self.status = format!(
                        "HTTP {} · {} bytes · {} links · {script_status} · CSS partial",
                        response.status,
                        response.body.len(),
                        self.document
                            .items
                            .iter()
                            .filter(|i| matches!(i, Item::Text { href: Some(_), .. }))
                            .count()
                    );
                    eprintln!(
                        "LOADED {} HTTP {} title={:?} items={} forms={}",
                        self.address,
                        response.status,
                        self.document.title,
                        self.document.items.len(),
                        self.document.forms.len()
                    );
                }
                Err(error) => {
                    load_error = Some(error.clone());
                    self.page_mime = "text/html".into();
                    self.status = format!("Load failed: {error}");
                    self.document = document::parse(
                        &format!(
                            "<title>Load failed</title><h1>Could not load this page</h1><p>{}</p>",
                            escape(&error)
                        ),
                        &self.address,
                    );
                    eprintln!("LOAD_ERROR {error}");
                }
            }
            self.page_url = self.address.clone();
            self.page_loader = loaded.generation;
            self.dom_epoch += 1;
            if self.cdp_enabled {
                self.cdp_events.push(cdp_browser::Event::Finished {
                    generation: loaded.generation,
                    frame: cdp_browser::frame(self),
                    error: load_error,
                });
            }
            self.dirty = true;
            if let Some(target) = script_navigation.or_else(|| self.document.refresh.clone()) {
                if self.refresh_count < 4 {
                    self.refresh_count += 1;
                    self.navigate_inner(target, None, false, true);
                } else {
                    self.status = "Automatic navigation limit reached; navigation stopped.".into();
                }
            }
        }
        self.poll_page_session();
    }
    fn script_summary(reply: &SessionReply) -> String {
        if let Some(report) = &reply.allocations {
            if let Ok(json) = serde_json::to_string(report) {
                eprintln!("SCRIPT_ALLOCATION {json}");
            }
        }
        for (index, diagnostic) in reply.errors.iter().enumerate() {
            eprintln!(
                "SCRIPT_DIAGNOSTIC index={} message={diagnostic:?}",
                index + 1
            );
        }
        if let Some(error) = reply.errors.first() {
            let error: String = error.chars().take(240).collect();
            eprintln!(
                "SCRIPT_PARTIAL executed={} errors={} first={error:?}",
                reply.scripts_executed,
                reply.errors.len()
            );
            format!(
                "JS: {} scripts, {} errors ({error})",
                reply.scripts_executed,
                reply.errors.len()
            )
        } else {
            eprintln!("SCRIPT_COMPLETE executed={}", reply.scripts_executed);
            format!("JS: {} inline scripts", reply.scripts_executed)
        }
    }
    fn checked_navigation(target: &str) -> Result<String, String> {
        let url = url::Url::parse(target).map_err(|_| "Invalid script navigation URL")?;
        if target.len() > 16_384
            || !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Script navigation must be credential-free HTTP(S)".into());
        }
        Ok(url.into())
    }
    fn project_reply(
        reply: &mut SessionReply,
        url: &str,
        previous: Option<&Document>,
    ) -> Result<Document, String> {
        if reply.errors.len() > 64
            || reply.errors.iter().any(|error| error.len() > 4096)
            || reply.scripts_executed > 32
            || reply.acknowledgements.len() > 128
            || reply
                .allocations
                .as_ref()
                .is_none_or(|report| !report.is_valid())
        {
            return Err("Invalid script session report limits".into());
        }
        if let Some(target) = &reply.navigation {
            Self::checked_navigation(target)?;
        }
        if (reply.state != RealmState::Ready
            || reply.outcome.click_canceled == Some(true)
            || reply.outcome.submit_canceled == Some(true))
            && reply.default_action != DefaultAction::None
        {
            return Err("Invalid default action for canceled/closed event".into());
        }
        if reply.state != RealmState::Ready && reply.navigation.is_some() {
            return Err("Closed script session proposed navigation".into());
        }
        let snapshot = reply
            .snapshot
            .take()
            .ok_or("Script session has no accepted snapshot")?;
        if previous.is_some_and(|old| {
            snapshot.nodes.len() < old.nodes.len()
                || old
                    .nodes
                    .iter()
                    .zip(&snapshot.nodes)
                    .any(|(a, b)| a.tag != b.tag)
        }) {
            return Err("Script snapshot reused or removed stable arena identities".into());
        }
        document::project_nodes(snapshot.nodes, url)
    }
    fn connected(document: &Document, mut node: usize) -> bool {
        for _ in 0..=256 {
            if node == 0 {
                return true;
            }
            let Some(item) = document.nodes.get(node) else {
                return false;
            };
            if item.parent == node {
                return false;
            }
            node = item.parent;
        }
        false
    }
    fn input_item(&self, node: usize) -> Option<usize> {
        self.document
            .item_nodes
            .iter()
            .enumerate()
            .find_map(|(i, &id)| {
                (id == node && matches!(self.document.items[i], Item::Input { .. })).then_some(i)
            })
    }
    fn block_session(&mut self, error: &str) {
        if let Some(session) = &self.page_session {
            session.cancel();
        }
        self.pending_event = None;
        self.script_blocked = true;
        self.status = format!("Page script session unavailable; activation canceled: {error}");
        eprintln!("SCRIPT_SESSION_CLOSED {error:?}");
        self.dirty = true;
    }
    // Ok(true) means queued; Ok(false) permits native-only activation. Immediate
    // rejection must remain observable to CDP, never a successful dropped click.
    fn dispatch_page(&mut self, kind: InputKind) -> Result<bool, String> {
        if self.loading || self.script_blocked {
            self.status =
                "Page activation unavailable while loading or after script failure".into();
            self.dirty = true;
            return Err(self.status.clone());
        }
        if self.pending_event.is_some() {
            self.status = "A page event is still running; activation not queued".into();
            self.dirty = true;
            return Err(self.status.clone());
        }
        if self.page_session.is_none() {
            return Ok(false);
        }
        // Admit count and raw payload before cloning any edit. Framing performs
        // the separate exact encoded-byte check (JSON escaping may expand it).
        let mut selected = Vec::new();
        let mut bytes = 0usize;
        for (&node, &version) in &self.edit_versions {
            if let Some(value) = self
                .values
                .get(&node)
                .filter(|_| self.input_item(node).is_some())
            {
                bytes = bytes.saturating_add(value.len());
                if selected.len() == 128 || bytes > 64 * 1024 {
                    self.status = "Page event edit bound exceeded; activation canceled".into();
                    self.dirty = true;
                    return Err(self.status.clone());
                }
                selected.push((node, version, value));
            }
        }
        if selected.iter().any(|(_, _, value)| value.len() > 8191) {
            self.status = "Page event control value exceeds 8191 bytes".into();
            self.dirty = true;
            return Err(self.status.clone());
        }
        let mut edits: Vec<_> = selected
            .into_iter()
            .map(|(node, version, value)| ControlEdit {
                node,
                version,
                value: value.clone(),
            })
            .collect();
        edits.sort_by_key(|edit| edit.node);
        let input = SessionInput { kind, edits };
        let session = self.page_session.as_mut().unwrap();
        match session.try_dispatch(input.clone(), self.page_revision) {
            Ok(sequence) => {
                self.pending_event = Some(PendingEvent {
                    generation: self.generation,
                    session_id: session.id(),
                    sequence,
                    revision: self.page_revision,
                    input,
                });
                self.status = "Running page event".into();
                self.dirty = true;
            }
            Err(error) => {
                self.block_session(&error);
                return Err(error);
            }
        }
        Ok(true)
    }
    fn validate_default(
        document: &Document,
        input: &InputKind,
        action: &DefaultAction,
    ) -> Result<(), String> {
        match action {
            DefaultAction::None => Ok(()),
            DefaultAction::FollowLink { node } => {
                if !matches!(input, InputKind::Click { target } if target == node)
                    || !Self::connected(document, *node)
                    || !document
                        .items
                        .iter()
                        .zip(&document.item_nodes)
                        .any(|(item, id)| {
                            id == node && matches!(item, Item::Text { href: Some(_), .. })
                        })
                {
                    Err("Unrelated or stale link default".into())
                } else {
                    Ok(())
                }
            }
            DefaultAction::SubmitForm { form, submitter } => {
                let Some(index) = document.form_nodes.iter().position(|node| node == form) else {
                    return Err("Unknown form default".into());
                };
                if !Self::connected(document, *form) {
                    return Err("Detached form default".into());
                }
                if let Some(node) = submitter {
                    if !document
                        .items
                        .iter()
                        .zip(&document.item_nodes)
                        .any(|(item, id)| {
                            id == node
                                && matches!(item, Item::Submit { form, .. } if *form == index)
                        })
                    {
                        return Err("Unrelated submitter default".into());
                    }
                }
                match input {
                    InputKind::Click { target } if Some(*target) == *submitter => Ok(()),
                    InputKind::Submit {
                        form: original,
                        submitter: original_button,
                    } if original_button == submitter
                        && (submitter.is_some() || original == form) =>
                    {
                        Ok(())
                    }
                    _ => Err("Unrelated form default".into()),
                }
            }
        }
    }
    fn poll_page_session(&mut self) {
        if self.script_blocked {
            return;
        }
        let update = self
            .page_session
            .as_mut()
            .and_then(|session| session.try_recv());
        let Some(update) = update else {
            return;
        };
        let Some(session) = &self.page_session else {
            return;
        };
        if update.generation != self.generation || update.session_id != session.id() {
            self.block_session("Stale script completion identity");
            return;
        }
        let mut reply = match update.reply {
            Ok(reply) => reply,
            Err(error) => {
                self.block_session(&error);
                return;
            }
        };
        let Some(pending) = self.pending_event.take() else {
            self.block_session("Script session expired or sent an unsolicited completion");
            return;
        };
        if pending.generation != self.generation
            || pending.session_id != update.session_id
            || update.sequence != Some(pending.sequence)
            || self.page_revision != pending.revision
            || reply.revision != pending.revision.checked_add(1).unwrap_or(u64::MAX)
        {
            self.block_session("Stale script event sequence/revision");
            return;
        }
        let started = Instant::now();
        let expected: Vec<_> = pending
            .input
            .edits
            .iter()
            .map(|edit| (edit.node, edit.version))
            .collect();
        let mut ack: Vec<_> = reply
            .acknowledgements
            .iter()
            .map(|edit| (edit.node, edit.version))
            .collect();
        ack.sort_unstable();
        let projection = if ack != expected {
            Err("Invalid control-edit acknowledgement".into())
        } else {
            Self::project_reply(&mut reply, &self.page_url, Some(&self.document))
        }
        .and_then(|document| {
            Self::validate_default(&document, &pending.input.kind, &reply.default_action)
                .map(|_| document)
        });
        let timed = self
            .page_session
            .as_mut()
            .unwrap()
            .charge_active(started.elapsed());
        let document = match projection.and_then(|document| timed.map(|_| document)) {
            Ok(document) => document,
            Err(error) => {
                self.block_session(&error);
                return;
            }
        };
        if reply.state != RealmState::Ready {
            // Later failed transactions are atomic from the parent's point of
            // view. Diagnostics survive, but the previous accepted document,
            // edit versions, focus, and CDP epoch remain unchanged.
            let _ = Self::script_summary(&reply);
            self.block_session("Realm became fatal or closed; previous page retained");
            return;
        }
        self.acknowledge_projection(document, &ack);
        self.page_revision = reply.revision;
        self.dom_epoch += 1;
        if self.cdp_enabled {
            self.cdp_events.push(cdp_browser::Event::DocumentUpdated);
        }
        let _ = Self::script_summary(&reply);
        self.status = format!("Page event completed · {} diagnostics", reply.errors.len());
        eprintln!(
            "PAGE_EVENT_APPLIED sequence={} revision={} click_canceled={:?} submit_canceled={:?}",
            pending.sequence,
            reply.revision,
            reply.outcome.click_canceled,
            reply.outcome.submit_canceled
        );
        self.dirty = true;
        if let Some(target) = reply.navigation {
            if self.refresh_count < 4 {
                self.refresh_count += 1;
                self.navigate_inner(target, None, false, true);
            } else {
                self.block_session("Automatic navigation limit reached");
            }
            return;
        }
        match reply.default_action {
            DefaultAction::None => {}
            DefaultAction::FollowLink { node } => {
                let target = self
                    .document
                    .items
                    .iter()
                    .zip(&self.document.item_nodes)
                    .find_map(|(item, id)| {
                        if *id == node {
                            if let Item::Text {
                                href: Some(href), ..
                            } = item
                            {
                                Some(href.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    });
                if let Some(target) = target {
                    match Self::checked_navigation(&target) {
                        Ok(target) => self.navigate(target, None, true),
                        Err(error) => self.block_session(&error),
                    }
                }
            }
            DefaultAction::SubmitForm { form, submitter } => {
                self.submit_default(form, submitter, false)
            }
        }
    }
    fn acknowledge_projection(&mut self, document: Document, ack: &[(usize, u64)]) {
        self.document = document;
        for &(node, version) in ack {
            if self.edit_versions.get(&node) == Some(&version) {
                self.edit_versions.remove(&node);
                self.values.remove(&node);
            }
        }
        let editable: HashSet<_> = self
            .document
            .items
            .iter()
            .zip(&self.document.item_nodes)
            .filter_map(|(item, &node)| matches!(item, Item::Input { .. }).then_some(node))
            .collect();
        self.values.retain(|node, _| editable.contains(node));
        self.edit_versions.retain(|node, _| editable.contains(node));
        if let Focus::Input(node) = self.focus {
            if !editable.contains(&node) {
                self.focus = Focus::Page;
                self.select_all = false;
            }
        }
        // Pointer-down coordinates belong to the old accepted layout.
        self.pointer_press = None;
    }
    fn field_value(&self, i: usize, default: &str) -> String {
        self.values
            .get(self.document.item_nodes.get(i).unwrap_or(&usize::MAX))
            .cloned()
            .unwrap_or_else(|| default.into())
    }
    fn submit(&mut self, form: usize, button: Option<usize>) -> Result<(), String> {
        let Some(&form_node) = self.document.form_nodes.get(form) else {
            return Ok(());
        };
        let submitter = button.and_then(|item| self.document.item_nodes.get(item).copied());
        if !self.dispatch_page(InputKind::Submit {
            form: form_node,
            submitter,
        })? {
            self.submit_default(form_node, submitter, true);
        }
        Ok(())
    }
    fn submit_default(&mut self, form_node: usize, button_node: Option<usize>, use_edits: bool) {
        let Some(form) = self
            .document
            .form_nodes
            .iter()
            .position(|node| *node == form_node)
        else {
            return;
        };
        let button =
            button_node.and_then(|node| self.document.item_nodes.iter().position(|id| *id == node));
        let Some(form_data) = self.document.forms.get(form) else {
            return;
        };
        let mut fields = form_data.fields.clone();
        for (i, item) in self.document.items.iter().enumerate() {
            if let Item::Input {
                form: f,
                name,
                value,
                ..
            } = item
            {
                if *f == form && !name.is_empty() {
                    fields.push((
                        name.clone(),
                        if use_edits {
                            self.field_value(i, value)
                        } else {
                            value.clone()
                        },
                    ));
                }
            }
        }
        if let Some(i) = button {
            if let Some(Item::Submit { name, value, .. }) = self.document.items.get(i) {
                if !name.is_empty() {
                    fields.push((name.clone(), value.clone()));
                }
            }
        }
        let encoded = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish();
        let action = form_data.action.clone();
        let method = form_data.method.clone();
        if !use_edits && let Err(error) = Self::checked_navigation(&action) {
            self.block_session(&error);
            return;
        }
        if method == "post" {
            self.navigate(action, Some(encoded), true)
        } else if let Ok(mut target) = url::Url::parse(&action) {
            target.set_query(Some(&encoded));
            self.navigate(target.to_string(), None, true)
        }
    }
    fn activate(&mut self, action: Action) {
        let _ = self.activate_checked(action);
    }
    fn activate_checked(&mut self, action: Action) -> Result<(), String> {
        self.select_all = false;
        match action {
            Action::Address => {
                self.focus = Focus::Address;
                self.select_all = true;
            }
            Action::Input(node) => {
                if self.input_item(node).is_some() {
                    self.focus = Focus::Input(node);
                    self.dispatch_page(InputKind::Click { target: node })?;
                }
            }
            Action::Submit(node) => {
                if !self.dispatch_page(InputKind::Click { target: node })? {
                    if let Some(i) = self.document.item_nodes.iter().position(|id| *id == node) {
                        if let Item::Submit { form, .. } = self.document.items[i] {
                            self.submit_default(self.document.form_nodes[form], Some(node), true);
                        }
                    }
                }
            }
            Action::Link { node, href } => {
                if !self.dispatch_page(InputKind::Click { target: node })? {
                    self.navigate(href, None, true);
                }
            }
            Action::Reload => self.navigate(self.address.clone(), None, false),
            Action::Back => {
                if self.history_at > 0 && self.inflight < 2 {
                    self.history_at -= 1;
                    self.navigate(self.history[self.history_at].clone(), None, false);
                }
            }
            Action::Forward => {
                if self.history_at + 1 < self.history.len() && self.inflight < 2 {
                    self.history_at += 1;
                    self.navigate(self.history[self.history_at].clone(), None, false);
                }
            }
        }
        self.dirty = true;
        Ok(())
    }
    fn click(&mut self, x: i32, y: i32) {
        let _ = self.click_checked(x, y);
    }
    fn click_checked(&mut self, x: i32, y: i32) -> Result<(), String> {
        if let Some(hit) = self.hits.iter().rev().find(|h| h.contains(x, y)) {
            self.activate_checked(hit.action.clone())?;
        } else {
            self.focus = Focus::Page;
            self.dirty = true;
        }
        Ok(())
    }
    pub fn type_text(&mut self, text: &str) {
        if matches!(self.focus, Focus::Input(_)) && self.next_edit == u64::MAX {
            return;
        }
        let default = if let Focus::Input(node) = self.focus {
            self.input_item(node)
                .and_then(|i| self.document.items.get(i))
                .and_then(|item| {
                    if let Item::Input { value, .. } = item {
                        Some(value.clone())
                    } else {
                        None
                    }
                })
        } else {
            None
        };
        let dest = match self.focus {
            Focus::Address => Some(&mut self.address),
            Focus::Input(node) if default.is_some() => {
                Some(self.values.entry(node).or_insert(default.unwrap()))
            }
            Focus::Input(_) => None,
            Focus::Page => None,
        };
        if let Some(dest) = dest {
            let retained = if self.select_all { 0 } else { dest.len() };
            if retained.saturating_add(text.len()) > MAX_EDIT_BYTES {
                return;
            }
            if self.select_all {
                dest.clear();
            }
            dest.push_str(text);
            if let Focus::Input(node) = self.focus {
                self.next_edit = self.next_edit.saturating_add(1);
                self.edit_versions.insert(node, self.next_edit);
            }
        }
        self.select_all = false;
        self.dirty = true;
    }
    fn enter_checked(&mut self) -> Result<(), String> {
        match self.focus {
            Focus::Address => self.navigate(self.address.clone(), None, true),
            Focus::Input(node) => {
                if let Some(i) = self.input_item(node) {
                    if let Some(Item::Input { form, .. }) = self.document.items.get(i) {
                        let form = *form;
                        let button = self.document.items.iter().position(
                            |item| matches!(item, Item::Submit {form:f, ..} if *f == form),
                        );
                        self.submit(form, button)?;
                    }
                }
            }
            Focus::Page => {}
        }
        Ok(())
    }
    fn key(&mut self, sym: u32, ctrl: bool, shift: bool, alt: bool) {
        if self.chrome && ctrl && (sym == b'l' as u32 || sym == b'L' as u32) {
            self.activate(Action::Address);
            return;
        }
        if ctrl && (sym == b'a' as u32 || sym == b'A' as u32) {
            self.select_all = true;
            self.dirty = true;
            return;
        }
        if alt && sym == 0xff51 {
            self.activate(Action::Back);
            return;
        }
        if sym == 0xffc2 || (ctrl && (sym == b'r' as u32 || sym == b'R' as u32)) {
            self.activate(Action::Reload);
            return;
        }
        match sym {
            0xff0d => {
                let _ = self.enter_checked();
            }
            0xff08 => {
                if self.select_all {
                    self.type_text("");
                } else {
                    match self.focus {
                        Focus::Address => {
                            self.address.pop();
                        }
                        Focus::Input(node) => {
                            if self.next_edit == u64::MAX {
                                return;
                            }
                            let Some(i) = self.input_item(node) else {
                                return;
                            };
                            let default = match self.document.items.get(i) {
                                Some(Item::Input { value, .. }) => value.clone(),
                                _ => String::new(),
                            };
                            self.values.entry(node).or_insert(default).pop();
                            self.next_edit = self.next_edit.saturating_add(1);
                            self.edit_versions.insert(node, self.next_edit);
                        }
                        Focus::Page => {}
                    }
                }
                self.dirty = true;
            }
            0xff09 => {
                let fields: Vec<_> = self
                    .document
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, item)| {
                        if matches!(item, Item::Input { .. }) {
                            Some(self.document.item_nodes[i])
                        } else {
                            None
                        }
                    })
                    .collect();
                if !fields.is_empty() {
                    let current = if let Focus::Input(i) = self.focus {
                        fields.iter().position(|v| *v == i)
                    } else {
                        None
                    };
                    let next = match current {
                        None => 0,
                        Some(i) if shift => (i + fields.len() - 1) % fields.len(),
                        Some(i) => (i + 1) % fields.len(),
                    };
                    self.focus = Focus::Input(fields[next]);
                    self.select_all = true;
                    self.dirty = true;
                }
            }
            0xff55 => self.scroll_by(-500),
            0xff56 => self.scroll_by(500),
            0xff52 if self.focus == Focus::Page => self.scroll_by(-70),
            0xff54 if self.focus == Focus::Page => self.scroll_by(70),
            _ => {
                if !ctrl && !alt {
                    let code = if sym & 0xff000000 == 0x01000000 {
                        sym & 0xffffff
                    } else {
                        sym
                    };
                    if let Some(ch) = char::from_u32(code) {
                        if !ch.is_control() && code < 0xff00 {
                            self.type_text(&ch.to_string());
                        }
                    }
                }
            }
        }
    }
    pub fn scroll_by(&mut self, amount: i32) {
        self.scroll =
            (self.scroll + amount).clamp(0, (self.content_height - self.height as i32 + 40).max(0));
        self.dirty = true;
    }
    #[cfg(test)]
    fn hit(&mut self, x: i32, y: i32, w: u32, h: u32, action: Action) {
        let top = y.max(self.page_top());
        let bottom = (y + h as i32).min(self.page_top() + self.viewport_height() as i32 - 1);
        if bottom > top {
            self.hits.push(Hit {
                x,
                y: top,
                w,
                h: (bottom - top) as u32,
                action,
            });
        }
    }
    fn is_http_page(&self) -> bool {
        url::Url::parse(&self.page_url).is_ok_and(|url| url.scheme() == "http")
    }
    pub fn visible_title(&self) -> String {
        if self.is_http_page() {
            format!("HTTP: Not secure | {}", self.document.title)
        } else {
            self.document.title.clone()
        }
    }
    pub fn paint(&mut self) -> Canvas {
        let top = self.page_top();
        let controls = mg_sparkle::render::Controls {
            values: Some(&self.values),
            focused_input: if let Focus::Input(node) = self.focus {
                Some(node)
            } else {
                None
            },
            select_all: self.select_all,
        };
        let viewport = mg_sparkle::render::Viewport {
            width: self.width,
            height: self.viewport_height(),
            scroll: self.scroll,
        };
        let frame =
            mg_sparkle::render::render(&self.document, &mut self.fonts, viewport, &controls);
        self.boxes = frame
            .boxes
            .into_iter()
            .map(|mut rect| {
                rect.y += top;
                rect
            })
            .collect();
        self.hits = frame
            .hits
            .into_iter()
            .map(|hit| Hit {
                x: hit.x,
                y: hit.y + top,
                w: hit.w,
                h: hit.h,
                action: match hit.action {
                    mg_sparkle::render::Action::Input(node) => Action::Input(node),
                    mg_sparkle::render::Action::Submit(node) => Action::Submit(node),
                    mg_sparkle::render::Action::Link { node, href } => Action::Link { node, href },
                },
            })
            .collect();
        self.content_height = frame.content_height + top;
        if !self.chrome {
            self.dirty = false;
            return frame.canvas;
        }
        let mut canvas = Canvas::new(self.width, self.height, BG);
        let offset = top as usize * self.width as usize;
        canvas.pixels[offset..offset + frame.canvas.pixels.len()]
            .copy_from_slice(&frame.canvas.pixels);
        #[cfg(feature = "chrome")]
        if self.chrome {
            self.paint_chrome(&mut canvas);
        }
        self.dirty = false;
        canvas
    }
    fn smoke_step(&mut self, canvas: &Canvas) {
        if self.smoke_events {
            self.smoke_event_step(canvas);
            return;
        }
        if self.smoke.is_none()
            || self.loading
            || self.pending_event.is_some()
            || self.smoke_since.elapsed() < Duration::from_millis(400)
        {
            return;
        }
        if !self.last_load_ok {
            self.fail_smoke("The page did not load successfully");
            return;
        }
        if self.smoke_stage == 0 {
            let _ = std::fs::create_dir_all(&self.evidence_dir);
            let _ = canvas.save_png(&format!("{}/01-home.png", self.evidence_dir));
            if let Some(i) = self
                .document
                .items
                .iter()
                .position(|item| matches!(item,Item::Input{name,..} if name=="q"))
            {
                let node = self.document.item_nodes[i];
                if self.focus != Focus::Input(node) {
                    if let Some(hit) = self
                        .hits
                        .iter()
                        .find(|h| matches!(h.action,Action::Input(j) if j==node))
                        .cloned()
                    {
                        self.click(hit.x + 4, hit.y + (hit.h / 2) as i32);
                    } else {
                        let previous = self.scroll;
                        self.scroll_by(400);
                        if self.scroll == previous {
                            self.fail_smoke("Search field has no visible click target");
                        }
                        return;
                    }
                    return;
                }
                self.select_all = true;
                self.type_text(&self.smoke.clone().unwrap());
                let typed = self.paint();
                let _ = typed.save_png(&format!("{}/02-query.png", self.evidence_dir));
                self.key(0xff0d, false, false, false);
                self.smoke_stage = 1;
                self.smoke_since = Instant::now();
            } else {
                self.fail_smoke("No editable search field in the actual homepage");
            }
        } else if self.smoke_stage == 1 {
            let _ = canvas.save_png(&format!("{}/03-search.png", self.evidence_dir));
            // Result headings come from the actual document, not fabricated search URLs.
            if let Some(href) = self.document.items.iter().find_map(|item| {
                if let Item::Text {
                    href: Some(h),
                    heading: true,
                    ..
                } = item
                {
                    Some(h.clone())
                } else {
                    None
                }
            }) {
                if let Some(hit) = self
                    .hits
                    .iter()
                    .find(|h| matches!(&h.action,Action::Link { href: h, .. } if h==&href))
                    .cloned()
                {
                    self.click(hit.x + 1, hit.y + (hit.h / 2) as i32);
                } else {
                    let previous = self.scroll;
                    self.scroll_by(400);
                    if self.scroll == previous {
                        self.fail_smoke("Result link has no visible click target");
                    }
                    return;
                }
                self.smoke_stage = 2;
                self.smoke_since = Instant::now();
            } else {
                self.fail_smoke("Search response has no result-heading links; JavaScript or another unsupported behavior may be required");
            }
        } else if self.smoke_stage == 2 {
            let _ = canvas.save_png(&format!("{}/04-destination.png", self.evidence_dir));
            eprintln!(
                "JOURNEY_DESTINATION {} title={:?}",
                self.address, self.document.title
            );
            self.smoke_stage = 3;
            self.smoke = None;
        }
    }
    fn smoke_event_step(&mut self, canvas: &Canvas) {
        if self.smoke.is_none()
            || self.loading
            || self.pending_event.is_some()
            || self.smoke_since.elapsed() < Duration::from_millis(400)
        {
            return;
        }
        if !self.last_load_ok || self.script_blocked {
            self.fail_smoke("Event journey lost its loaded retained realm");
            return;
        }
        let state_is = |document: &Document, expected: &str| {
            let Some(node) = document.query_selector(0, "#state").ok().flatten() else {
                return false;
            };
            let mut text = String::new();
            let mut stack = vec![node];
            while let Some(id) = stack.pop() {
                text.push_str(&document.nodes[id].text);
                stack.extend(document.nodes[id].children.iter().rev().copied());
            }
            text == expected
        };
        let hit_for = |app: &App, selector: &str| {
            let node = app.document.query_selector(0, selector).ok().flatten()?;
            app.hits
                .iter()
                .find(|hit| match hit.action {
                    Action::Input(id) | Action::Submit(id) | Action::Link { node: id, .. } => {
                        id == node
                    }
                    _ => false,
                })
                .cloned()
        };
        let _ = std::fs::create_dir_all(&self.evidence_dir);
        match self.event_smoke_stage {
            0 => {
                let Some(query) = self.document.query_selector(0, "#query").ok().flatten() else {
                    self.fail_smoke("Event journey has no real query control");
                    return;
                };
                if self.focus != Focus::Input(query) {
                    if let Some(hit) = hit_for(self, "#query") {
                        self.click(hit.x + 4, hit.y + 4);
                    } else {
                        self.fail_smoke("Event query is not a visible native click target");
                    }
                    return;
                }
                self.select_all = true;
                self.type_text("Rust & café");
                let typed = self.paint();
                let _ = typed.save_png(&format!("{}/01-event-query.png", self.evidence_dir));
                if let Some(hit) = hit_for(self, "#cancel") {
                    self.click(hit.x + 1, hit.y + 1);
                } else {
                    self.fail_smoke("Cancel link is not a visible native click target");
                    return;
                }
                self.event_smoke_stage = 1;
            }
            1 => {
                if !state_is(&self.document, "link canceled") {
                    self.fail_smoke("Actual later link handler did not cancel and update state");
                    return;
                }
                let _ = canvas.save_png(&format!("{}/02-link-canceled.png", self.evidence_dir));
                self.key(0xff0d, false, false, false);
                self.event_smoke_stage = 2;
            }
            2 => {
                if !state_is(&self.document, "first submit canceled") {
                    self.fail_smoke(
                        "First actual submit handler did not cancel and retain closure state",
                    );
                    return;
                }
                let _ = canvas.save_png(&format!("{}/03-submit-canceled.png", self.evidence_dir));
                let Some(query) = self.document.query_selector(0, "#query").ok().flatten() else {
                    self.fail_smoke("Moved query control disappeared");
                    return;
                };
                if self.focus != Focus::Input(query) {
                    self.fail_smoke("Moving the query lost its stable focused identity");
                    return;
                }
                self.select_all = true;
                self.type_text("Rust & café again");
                self.key(0xff0d, false, false, false);
                self.event_smoke_stage = 3;
            }
            3 => {
                let Ok(url) = url::Url::parse(&self.address) else {
                    self.fail_smoke("Invalid event search URL");
                    return;
                };
                let fields: HashMap<_, _> = url.query_pairs().collect();
                if url.path() != "/event-search"
                    || fields.get("q").map(|v| v.as_ref()) != Some("Rust & café again")
                    || fields.get("proof").map(|v| v.as_ref()) != Some("retained-1-2")
                    || fields.get("submit").map(|v| v.as_ref()) != Some("events")
                {
                    self.fail_smoke(
                        "Post-handler form default did not send the real acknowledged fields",
                    );
                    return;
                }
                let _ = canvas.save_png(&format!("{}/04-event-results.png", self.evidence_dir));
                if let Some(hit) = hit_for(self, "#result") {
                    self.click(hit.x + 1, hit.y + 1);
                } else {
                    self.fail_smoke("Event result has no visible native click target");
                    return;
                }
                self.event_smoke_stage = 4;
            }
            4 => {
                let Ok(url) = url::Url::parse(&self.address) else {
                    self.fail_smoke("Invalid event destination");
                    return;
                };
                if url.path() != "/event-destination"
                    || !url
                        .query_pairs()
                        .any(|(key, value)| key == "proof" && value == "clicked")
                {
                    self.fail_smoke(
                        "Result click did not use its later handler's updated destination",
                    );
                    return;
                }
                let _ = canvas.save_png(&format!("{}/05-event-destination.png", self.evidence_dir));
                eprintln!(
                    "JOURNEY_EVENTS_COMPLETE {} title={:?}",
                    self.address, self.document.title
                );
                self.smoke_stage = 3;
                self.smoke = None;
            }
            _ => self.fail_smoke("Invalid event journey stage"),
        }
        self.smoke_since = Instant::now();
    }
    fn fail_smoke(&mut self, message: &str) {
        eprintln!("JOURNEY_INCOMPLETE {message}");
        self.status = message.into();
        self.smoke_failed = true;
        self.smoke_stage = 3;
        self.smoke = None;
        self.dirty = true;
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn decode_text(bytes: &[u8], content_type: &str) -> String {
    if content_type
        .to_ascii_lowercase()
        .contains("charset=iso-8859-1")
    {
        bytes.iter().map(|b| char::from(*b)).collect()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}
#[cfg(feature = "chrome")]
fn fit_tail(fonts: &mut Fonts, text: &str, size: f32, width: f32) -> String {
    let chars: Vec<_> = text.chars().rev().take(512).collect();
    let mut low = 0;
    let mut high = chars.len();
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate: String = chars[..mid].iter().rev().collect();
        if fonts.width(&candidate, size) <= width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    chars[..low].iter().rev().collect()
}
#[cfg(feature = "chrome")]
fn fit_head(fonts: &mut Fonts, text: &str, size: f32, width: f32) -> String {
    let mut value = text.chars().take(250).collect::<String>();
    while !value.is_empty() && fonts.width(&value, size) > width {
        value.pop();
    }
    value
}

#[cfg(all(test, feature = "chrome"))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    #[test]
    fn http_warning_follows_loaded_url_not_location_edits() {
        let mut app = test_app();
        app.generation = 1;
        for (url, expected_http) in [
            ("http://example.test/", true),
            ("https://example.test/", false),
            ("http://example.test/redirect-destination", true),
        ] {
            app.tx
                .send(Loaded {
                    generation: 1,
                    script: None,
                    result: Ok(net::Response {
                        url: url::Url::parse(url).unwrap(),
                        status: 200,
                        content_type: "text/html".into(),
                        body: b"<title>Loaded page</title><p>Readable</p>".to_vec(),
                    }),
                })
                .unwrap();
            app.poll();
            assert_eq!(app.is_http_page(), expected_http);
            assert_eq!(
                app.visible_title().starts_with("HTTP: Not secure"),
                expected_http
            );
            // Editing a different scheme must not relabel the displayed content.
            app.key(b'l' as u32, true, false, false);
            app.type_text(if expected_http {
                "https://typed-but-not-loaded.test/"
            } else {
                "http://typed-but-not-loaded.test/"
            });
            let canvas = app.paint();
            assert_eq!(canvas.pixels[0] == HTTP_CHROME, expected_http);
        }
    }

    #[test]
    fn ctrl_l_selects_location_and_enter_loads_plain_http() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}/location", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "location navigation did not arrive"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            line
        });
        let mut app = test_app();
        for (sym, focus) in [(b'l', Focus::Page), (b'L', Focus::Input(0))] {
            app.address = "https://previous.test/".into();
            app.focus = focus;
            app.dirty = false;
            app.key(u32::from(sym), true, sym == b'L', false);
            assert!(app.focus == Focus::Address && app.select_all && app.dirty);
            app.type_text(&address);
            assert_eq!(app.address, address);
        }
        app.key(0xff0d, false, false, false);
        assert_eq!(server.join().unwrap(), "GET /location HTTP/1.1\r\n");
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.loading {
            assert!(Instant::now() < deadline);
            app.poll();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(app.last_load_ok && app.is_http_page());
    }

    #[test]
    fn visible_form_enter_encodes_real_request_and_hidden_fields() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            line
        });
        let mut app = test_app();
        app.document = document::parse(
            include_str!("../../../tests/fixtures/journey/home.html"),
            &address,
        );
        let _ = app.paint();
        let hit = app
            .hits
            .iter()
            .find(|h| matches!(h.action, Action::Input(_)))
            .unwrap()
            .clone();
        app.click(hit.x + 5, hit.y + 5);
        app.type_text("Rust & café");
        app.key(0xff0d, false, false, false);
        let line = server.join().unwrap();
        let target = line.split_whitespace().nth(1).unwrap();
        let url = url::Url::parse(&format!("{address}{}", target.trim_start_matches('/'))).unwrap();
        let pairs: HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(pairs.get("q").map(|v| v.as_ref()), Some("Rust & café"));
        assert_eq!(pairs.get("source").map(|v| v.as_ref()), Some("fixture"));
        assert_eq!(pairs.get("submit").map(|v| v.as_ref()), Some("search"));
    }

    #[test]
    fn navigation_clears_stale_focus_and_hits_respect_chrome() {
        let mut app = test_app();
        app.focus = Focus::Input(900);
        app.generation = 1;
        app.tx
            .send(Loaded {
                generation: 1,
                script: None,
                result: Ok(net::Response {
                    url: url::Url::parse("https://example.org/").unwrap(),
                    status: 200,
                    content_type: "text/html".into(),
                    body: b"<p>new page</p>".to_vec(),
                }),
            })
            .unwrap();
        app.poll();
        assert!(app.focus == Focus::Page);
        app.key(0xff08, false, false, false);
        app.hit(
            1,
            TOP - 10,
            40,
            20,
            Action::Link {
                node: 0,
                href: "https://example.org/".into(),
            },
        );
        let hit = app.hits.last().unwrap();
        assert_eq!(hit.y, TOP);
        assert!(!hit.contains(2, TOP - 1));
    }

    #[test]
    fn failed_network_response_cannot_complete_journey() {
        let mut app = test_app();
        app.smoke = Some("test".into());
        app.smoke_stage = 2;
        app.smoke_since = Instant::now() - Duration::from_secs(1);
        let canvas = app.paint();
        app.smoke_step(&canvas);
        assert!(app.smoke_failed);
        assert!(app.status.contains("did not load"));
    }

    #[test]
    fn script_rejection_or_worker_failure_retains_noscript_and_drops_navigation() {
        let html = "<html><body><noscript><p>Readable fallback</p></noscript></body></html>";
        let mut invalid_report = mg_butane::runtime::Runtime::new().allocation_report();
        invalid_report.accepted_bytes += 1;
        for script in [
            Ok(ScriptLoad {
                session: None,
                reply: SessionReply {
                    revision: 0,
                    snapshot: None,
                    outcome: Default::default(),
                    default_action: DefaultAction::None,
                    state: RealmState::Closed,
                    acknowledgements: vec![],
                    navigation: Some("https://example.test/unwanted".into()),
                    errors: vec!["Rejected source".into()],
                    scripts_executed: 0,
                    allocations: None,
                },
            }),
            Ok(ScriptLoad {
                session: None,
                reply: SessionReply {
                    revision: 0,
                    snapshot: Some(mg_sparkle::page_session::ArenaSnapshot {
                        nodes: document::parse(
                            "<p>Do not apply invalid diagnostics</p>",
                            "https://example.test/",
                        )
                        .nodes,
                    }),
                    outcome: Default::default(),
                    default_action: DefaultAction::None,
                    state: RealmState::Ready,
                    acknowledgements: vec![],
                    navigation: Some("https://example.test/unwanted".into()),
                    errors: vec![],
                    scripts_executed: 0,
                    allocations: Some(invalid_report),
                },
            }),
            Err("Worker deadline".into()),
        ] {
            let mut app = test_app();
            app.generation = 1;
            app.tx
                .send(Loaded {
                    generation: 1,
                    script: Some(script),
                    result: Ok(net::Response {
                        url: url::Url::parse("https://example.test/").unwrap(),
                        status: 200,
                        content_type: "text/html".into(),
                        body: html.as_bytes().to_vec(),
                    }),
                })
                .unwrap();
            app.poll();
            assert!(
                app.document
                    .items
                    .iter()
                    .any(|i| matches!(i,Item::Text{text,..} if text=="Readable fallback"))
            );
            assert_eq!(app.generation, 1);
            assert_eq!(app.address, "https://example.test/");
            assert!(app.last_load_ok);
            assert!(!app.loading);
        }
    }

    #[test]
    fn manual_navigation_resets_automatic_chain_budget_even_without_history_entry() {
        for automatic in [false, true] {
            let mut app = test_app();
            app.refresh_count = 4;
            app.navigate_inner("unsupported://local-fixture".into(), None, false, automatic);
            assert_eq!(app.refresh_count, if automatic { 4 } else { 0 });
            let deadline = Instant::now() + Duration::from_secs(2);
            while app.loading && Instant::now() < deadline {
                app.poll();
                thread::sleep(Duration::from_millis(1));
            }
            assert!(!app.loading);
        }
    }

    #[test]
    fn stale_script_reply_cannot_replace_document_or_request_navigation() {
        let mut app = test_app();
        app.generation = 2;
        app.loading = true;
        app.inflight = 1;
        app.address = "https://example.test/current".into();
        app.document = document::parse("<title>Current</title>", &app.address);
        app.tx
            .send(Loaded {
                generation: 1,
                script: Some(Ok(ScriptLoad {
                    session: None,
                    reply: SessionReply {
                        revision: 0,
                        snapshot: Some(mg_sparkle::page_session::ArenaSnapshot {
                            nodes: document::parse("<title>Stale</title>", "https://example.test/")
                                .nodes,
                        }),
                        outcome: Default::default(),
                        default_action: DefaultAction::None,
                        state: RealmState::Ready,
                        acknowledgements: vec![],
                        navigation: Some("https://example.test/unwanted".into()),
                        errors: vec![],
                        scripts_executed: 1,
                        allocations: None,
                    },
                })),
                result: Ok(net::Response {
                    url: url::Url::parse("https://example.test/old").unwrap(),
                    status: 200,
                    content_type: "text/html".into(),
                    body: vec![],
                }),
            })
            .unwrap();
        app.poll();
        assert_eq!(app.generation, 2);
        assert_eq!(app.address, "https://example.test/current");
        assert_eq!(app.document.title, "Current");
        assert!(app.loading);
        assert_eq!(app.inflight, 0);
    }

    #[test]
    fn older_edit_ack_keeps_new_typing_but_default_uses_accepted_snapshot() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            line
        });
        let mut app = test_app();
        app.document = document::parse(
            "<form action='/search'><input id='q' name='q' value='accepted version one'><button name='submit' value='yes'>Go</button></form>",
            &address,
        );
        let node = app.document.query_selector(0, "#q").unwrap().unwrap();
        app.values.insert(node, "newer version two".into());
        app.edit_versions.insert(node, 2);
        app.focus = Focus::Input(node);
        app.acknowledge_projection(app.document.clone(), &[(node, 1)]);
        let item = app.input_item(node).unwrap();
        assert_eq!(app.field_value(item, "unused"), "newer version two");
        assert_eq!(app.edit_versions.get(&node), Some(&2));
        assert!(app.focus == Focus::Input(node));
        let form = app.document.form_nodes[0];
        let button = app.document.query_selector(0, "button").unwrap();
        app.submit_default(form, button, false);
        let line = server.join().unwrap();
        let url = url::Url::parse(&format!(
            "http://fixture{}",
            line.split_whitespace().nth(1).unwrap()
        ))
        .unwrap();
        let fields: HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(
            fields.get("q").map(|v| v.as_ref()),
            Some("accepted version one")
        );
        assert_eq!(fields.get("submit").map(|v| v.as_ref()), Some("yes"));
    }

    #[test]
    fn post_click_submitter_can_move_forms_but_unrelated_defaults_are_rejected() {
        let mut document = document::parse(
            "<form id='a'><button id='go'>Go</button></form><form id='b'></form><a id='link' href='/ok'>Link</a>",
            "https://fixture.test/",
        );
        let first = document.query_selector(0, "#a").unwrap().unwrap();
        let second = document.query_selector(0, "#b").unwrap().unwrap();
        let button = document.query_selector(0, "#go").unwrap().unwrap();
        let link = document.query_selector(0, "#link").unwrap().unwrap();
        document.nodes[first].children.retain(|id| *id != button);
        document.nodes[second].children.push(button);
        document.nodes[button].parent = second;
        let projected = document::project_nodes(document.nodes, "https://fixture.test/").unwrap();
        let input = InputKind::Submit {
            form: first,
            submitter: Some(button),
        };
        assert!(
            App::validate_default(
                &projected,
                &input,
                &DefaultAction::SubmitForm {
                    form: second,
                    submitter: Some(button)
                }
            )
            .is_ok()
        );
        assert!(
            App::validate_default(
                &projected,
                &input,
                &DefaultAction::SubmitForm {
                    form: first,
                    submitter: Some(button)
                }
            )
            .is_err()
        );
        assert!(
            App::validate_default(
                &projected,
                &input,
                &DefaultAction::FollowLink { node: link }
            )
            .is_err()
        );
        assert!(
            App::validate_default(
                &projected,
                &InputKind::Submit {
                    form: first,
                    submitter: None
                },
                &DefaultAction::SubmitForm {
                    form: second,
                    submitter: None
                }
            )
            .is_err()
        );
    }

    #[test]
    fn closed_script_realm_never_falls_through_to_native_activation() {
        let mut app = test_app();
        app.document = document::parse(
            "<a href='/trap'>Link</a><form action='/trap'><input name='q'><button>Go</button></form>",
            "https://fixture.test/",
        );
        app.script_blocked = true;
        let node = app.document.query_selector(0, "a").unwrap().unwrap();
        app.activate(Action::Link {
            node,
            href: "https://fixture.test/trap".into(),
        });
        assert!(app.submit(0, None).is_err());
        assert_eq!(app.generation, 0);
        assert_eq!(app.inflight, 0);
        assert!(!app.loading);
        assert!(app.script_blocked);
    }
}

/// Build a research TLS client with the caller's trusted roots.
/// Certificate and hostname verification remain rustls's normal defaults.
pub fn tls_client_config(
    roots: rustls::RootCertStore,
) -> Result<rustls::ClientConfig, rustls::Error> {
    Ok(
        rustls::ClientConfig::builder_with_provider(rustls_rustcrypto::provider().into())
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

type App = Browser;

/// Platform-neutral keyboard input. Text entry may also use Browser::type_text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Character(char),
    Enter,
    Backspace,
    Tab,
    Left,
    Up,
    Down,
    PageUp,
    PageDown,
    Reload,
}

/// Existing fixture-journey controls used by the desktop acceptance harness.
#[doc(hidden)]
pub struct JourneyOptions {
    pub query: Option<String>,
    pub events: bool,
    pub exit_after: bool,
    pub evidence_dir: String,
}
impl Default for JourneyOptions {
    fn default() -> Self {
        Self {
            query: None,
            events: false,
            exit_after: false,
            evidence_dir: "tmp/journey".into(),
        }
    }
}
impl Browser {
    pub fn back(&mut self) {
        self.activate(Action::Back);
    }
    pub fn forward(&mut self) {
        self.activate(Action::Forward);
    }
    pub fn reload(&mut self) {
        self.activate(Action::Reload);
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn last_load_succeeded(&self) -> bool {
        self.last_load_ok
    }
    pub fn is_loading(&self) -> bool {
        self.loading
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn page_url(&self) -> &str {
        &self.page_url
    }
    pub fn request_redraw(&mut self) {
        self.dirty = true;
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.clamp(360, 2400);
        self.height = height.clamp(240, 1800);
        self.dirty = true;
    }
    /// Scripts remain opt-in and require a host-provided isolated runtime.
    pub fn set_scripts_enabled(&mut self, enabled: bool) {
        self.scripts_enabled = enabled;
    }
    pub fn set_debugging(&mut self, enabled: bool) {
        self.cdp_enabled = enabled;
    }
    /// Disable the toolbar for embedding. Enabling it requires the chrome feature.
    pub fn set_chrome(&mut self, enabled: bool) {
        self.chrome = enabled && cfg!(feature = "chrome");
        self.hits.clear();
        self.boxes.clear();
        self.focus = Focus::Page;
        self.select_all = false;
        self.pointer_press = None;
        self.dirty = true;
    }
    fn page_top(&self) -> i32 {
        if self.chrome { TOP } else { 0 }
    }
    fn page_footer(&self) -> u32 {
        if self.chrome { 29 } else { 0 }
    }
    pub fn pointer_down(&mut self, x: i32, y: i32) {
        self.pointer_press = Some((x, y));
    }
    pub fn pointer_up(&mut self, x: i32, y: i32) {
        if let Some((px, py)) = self.pointer_press.take() {
            let press = self.hits.iter().position(|hit| hit.contains(px, py));
            let release = self.hits.iter().position(|hit| hit.contains(x, y));
            if press.is_some() && press == release {
                self.click(x, y);
            }
        }
    }
    pub fn handle_key(&mut self, key: Key, ctrl: bool, shift: bool, alt: bool) {
        let sym = match key {
            Key::Character(ch) => ch as u32,
            Key::Enter => 0xff0d,
            Key::Backspace => 0xff08,
            Key::Tab => 0xff09,
            Key::Left => 0xff51,
            Key::Up => 0xff52,
            Key::Down => 0xff54,
            Key::PageUp => 0xff55,
            Key::PageDown => 0xff56,
            Key::Reload => 0xffc2,
        };
        self.key(sym, ctrl, shift, alt);
    }
    #[doc(hidden)]
    pub fn configure_journey(&mut self, options: JourneyOptions) {
        self.smoke = options.query;
        self.smoke_events = options.events;
        self.exit_after_smoke = options.exit_after;
        self.evidence_dir = options.evidence_dir;
    }
    #[doc(hidden)]
    pub fn advance_journey(&mut self, canvas: &Canvas) {
        self.smoke_step(canvas);
    }
    #[doc(hidden)]
    pub fn journey_needs_redraw(&self) -> bool {
        self.smoke.is_some() && !self.loading
    }
    #[doc(hidden)]
    pub fn journey_result(&self) -> Option<bool> {
        (self.exit_after_smoke && self.smoke_stage == 3).then_some(!self.smoke_failed)
    }
}
#[cfg(test)]
fn test_app() -> Browser {
    let path = std::env::var("MGBROWSER_FONT")
        .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
    Browser::new(
        Fonts::from_bytes(std::fs::read(path).unwrap()).unwrap(),
        Arc::new(scripts::DisabledScripts),
    )
}
