//! Minimal desktop research browser: own HTML document flow and software paint.
mod cdp_browser;
mod script_worker;
use mg_deps::{
    document::{self, Document, Item},
    net,
    page_session::{ControlEdit, DefaultAction, InputKind, RealmState, SessionInput, SessionReply},
    paint::{Canvas, Fonts},
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{Event, xproto::*},
    wrapper::ConnectionExt as _,
};

const TOP: i32 = 108;
const BG: u32 = 0xfafbf8;
const INK: u32 = 0x26342b;
const LINK: u32 = 0x174ea6;
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
    session: Option<script_worker::Session>,
    reply: SessionReply,
}
struct PendingEvent {
    generation: u64,
    session_id: u64,
    sequence: u64,
    revision: u64,
    input: SessionInput,
}
struct App {
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
    boxes: Vec<cdp_browser::LayoutBox>,
    cdp_enabled: bool,
    cdp_events: Vec<cdp_browser::Event>,
    scripts_enabled: bool,
    child_pool: script_worker::ChildPool,
    page_session: Option<script_worker::Session>,
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

impl Drop for App {
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

impl App {
    fn new() -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            session: net::Session::default(),
            fonts: Fonts::load()?,
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
            child_pool: script_worker::ChildPool::new(),
            page_session: None,
            page_revision: 0,
            pending_event: None,
            script_blocked: false,
            edit_versions: HashMap::new(),
            next_edit: 0,
            hits: Vec::new(),
            pointer_press: None,
            focus: Focus::Address,
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
        })
    }
    fn navigate(&mut self, target: String, body: Option<String>, add_history: bool) {
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
                        script_worker::Session::start(
                            mg_deps::js_browser::Request {
                                url: response.url.to_string(),
                                html: decode_text(&response.body, &response.content_type),
                            },
                            generation,
                            pool,
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
    fn poll(&mut self) {
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
    fn type_text(&mut self, text: &str) {
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
        if ctrl && (sym == b'l' as u32 || sym == b'L' as u32) {
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
    fn scroll_by(&mut self, amount: i32) {
        self.scroll =
            (self.scroll + amount).clamp(0, (self.content_height - self.height as i32 + 40).max(0));
        self.dirty = true;
    }
    fn hit(&mut self, x: i32, y: i32, w: u32, h: u32, action: Action) {
        let top = y.max(TOP);
        let bottom = (y + h as i32).min(self.height as i32 - 30);
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
    fn button(&mut self, c: &mut Canvas, x: i32, y: i32, w: u32, text: &str, action: Action) {
        c.rect(x, y, w, 34, 0xe3e9df);
        c.text(&mut self.fonts, x + 10, y + 7, text, 15., INK);
        self.hits.push(Hit {
            x,
            y,
            w,
            h: 34,
            action,
        });
    }
    fn paint(&mut self) -> Canvas {
        let mut c = Canvas::new(self.width, self.height, BG);
        self.hits.clear();
        self.boxes.clear();
        let left = 32;
        let right = self.width as i32 - 38;
        let mut x = left;
        let mut y = TOP + 24 - self.scroll;
        let mut row = 27;
        for i in 0..self.document.items.len() {
            let item = self.document.items[i].clone();
            let node = self.document.item_nodes[i];
            match item {
                Item::Break => {
                    if x > left {
                        y += row;
                        x = left;
                    } else {
                        y += 8;
                    }
                    row = 27;
                }
                Item::Text {
                    text,
                    href,
                    heading,
                } => {
                    let size = if heading { 24. } else { 17. };
                    row = row.max(if heading { 34 } else { 27 });
                    for word in text.split_whitespace() {
                        let space = self.fonts.width(" ", size).ceil() as i32;
                        let ww = self.fonts.width(word, size).ceil() as i32;
                        if x > left && x + ww > right {
                            x = left;
                            y += row;
                        }
                        self.layout_box(i, x, y, ww.max(1) as u32, row as u32);
                        if y + row > TOP && y < self.height as i32 - 30 {
                            c.text(
                                &mut self.fonts,
                                x,
                                y,
                                word,
                                size,
                                if href.is_some() { LINK } else { INK },
                            );
                            if let Some(ref url) = href {
                                self.hit(
                                    x,
                                    y,
                                    ww.max(1) as u32,
                                    row as u32,
                                    Action::Link {
                                        node,
                                        href: url.clone(),
                                    },
                                );
                            }
                        }
                        x += ww + space;
                    }
                }
                Item::Input { value, kind, .. } => {
                    if x > left {
                        y += row;
                        x = left;
                    }
                    let w = (right - left).clamp(120, 550) as u32;
                    self.layout_box(i, x, y, w, 40);
                    if y + 40 > TOP && y < self.height as i32 - 30 {
                        c.rect(
                            x,
                            y,
                            w,
                            40,
                            if self.focus == Focus::Input(node) {
                                0x277453
                            } else {
                                0x9ca79a
                            },
                        );
                        c.rect(x + 2, y + 2, w - 4, 36, 0xffffff);
                        let mut value = self.field_value(i, &value);
                        if kind == "password" {
                            value = "•".repeat(value.chars().count());
                        }
                        let value = fit_tail(&mut self.fonts, &value, 17., w as f32 - 22.);
                        if self.focus == Focus::Input(node) && self.select_all {
                            c.rect(
                                x + 7,
                                y + 7,
                                self.fonts.width(&value, 17.) as u32 + 2,
                                25,
                                0xc6dfed,
                            );
                        }
                        c.text(&mut self.fonts, x + 9, y + 8, &value, 17., INK);
                        self.hit(x, y, w, 40, Action::Input(node));
                    }
                    y += 50;
                    row = 27;
                }
                Item::Submit { label, .. } => {
                    let w = (self.fonts.width(&label, 16.) as u32 + 28)
                        .min((right - left).max(80) as u32);
                    if x > left && x + w as i32 > right {
                        x = left;
                        y += 42;
                    }
                    self.layout_box(i, x, y, w, 36);
                    if y + 38 > TOP && y < self.height as i32 - 30 {
                        c.rect(x, y, w, 36, 0xe0e8f3);
                        c.text(&mut self.fonts, x + 12, y + 7, &label, 16., LINK);
                        self.hit(x, y, w, 36, Action::Submit(node));
                    }
                    x += w as i32 + 12;
                    row = 46;
                }
                Item::Image { alt, .. } => {
                    let label = if alt.is_empty() {
                        "[image unsupported]".to_string()
                    } else {
                        format!("[image: {alt}]")
                    };
                    let ww = self.fonts.width(&label, 14.).ceil() as i32;
                    if x + ww > right {
                        x = left;
                        y += row;
                    }
                    self.layout_box(i, x, y, ww.max(1) as u32, row as u32);
                    if y + row > TOP {
                        c.text(&mut self.fonts, x, y, &label, 14., 0x667164);
                    }
                    x += ww + 8;
                }
            }
        }
        self.content_height = y + self.scroll + row;
        // Paint browser chrome last so scrolled content cannot overpaint it.
        c.rect(0, 0, self.width, TOP as u32, 0xeef1e9);
        self.button(&mut c, 14, 14, 54, "Back", Action::Back);
        self.button(&mut c, 76, 14, 70, "Next", Action::Forward);
        self.button(&mut c, 154, 14, 76, "Reload", Action::Reload);
        let aw = self.width.saturating_sub(255);
        c.rect(240, 12, aw, 39, 0xffffff);
        if self.focus == Focus::Address && self.select_all {
            c.rect(247, 20, aw.saturating_sub(14), 25, 0xc6dfed);
        }
        let address = fit_tail(&mut self.fonts, &self.address, 16., aw as f32 - 20.);
        c.text(&mut self.fonts, 250, 22, &address, 16., INK);
        self.hits.push(Hit {
            x: 240,
            y: 12,
            w: aw,
            h: 39,
            action: Action::Address,
        });
        c.text(&mut self.fonts, 18, 66, "mgbrowser", 19., 0x345c36);
        let title = fit_head(
            &mut self.fonts,
            &self.document.title,
            16.,
            self.width as f32 - 185.,
        );
        c.text(&mut self.fonts, 160, 68, &title, 16., INK);
        c.rect(0, TOP - 1, self.width, 1, 0xc4cebd);
        c.rect(0, self.height as i32 - 29, self.width, 29, 0xeef1e9);
        let status = fit_head(&mut self.fonts, &self.status, 12., self.width as f32 - 20.);
        c.text(
            &mut self.fonts,
            10,
            self.height as i32 - 22,
            &status,
            12.,
            0x53614f,
        );
        if self.content_height > self.height as i32 {
            let area = (self.height as i32 - TOP - 30).max(1);
            let thumb = (area * area / (self.content_height - TOP).max(1)).max(20);
            let sy = TOP + self.scroll * area / (self.content_height - TOP).max(1);
            c.rect(self.width as i32 - 9, sy, 6, thumb as u32, 0x8b9c83);
        }
        self.dirty = false;
        c
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
fn fit_head(fonts: &mut Fonts, text: &str, size: f32, width: f32) -> String {
    let mut value = text.chars().take(250).collect::<String>();
    while !value.is_empty() && fonts.width(&value, size) > width {
        value.pop();
    }
    value
}

fn main() -> Result<(), Box<dyn Error>> {
    // Worker dispatch must precede fonts, display, networking and debug-server setup.
    match std::env::args().nth(1).as_deref() {
        Some("--script-worker") => script_worker::worker_entry(),
        Some("--script-session") => script_worker::session_entry(),
        Some("--script-session-selftest") => {
            script_worker::session_selftest()?;
            return Ok(());
        }
        Some("--script-worker-selftest") => {
            script_worker::selftest()?;
            return Ok(());
        }
        _ => {}
    }
    let mut app = App::new()?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut initial = "https://www.google.com/".to_string();
    let mut debug_port: Option<u16> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--enable-scripts" => app.scripts_enabled = true,
            "--disable-scripts" => app.scripts_enabled = false,
            "--remote-debugging-port" => {
                i += 1;
                debug_port = Some(
                    args.get(i)
                        .ok_or("--remote-debugging-port requires a port")?
                        .parse()?,
                );
            }
            value if value.starts_with("--remote-debugging-port=") => {
                debug_port = Some(value.split_once('=').unwrap().1.parse()?);
            }
            "--smoke-search" => {
                i += 1;
                app.smoke = Some(
                    args.get(i)
                        .ok_or("--smoke-search requires a query")?
                        .clone(),
                );
            }
            "--smoke-events" => {
                app.smoke_events = true;
                app.smoke = Some("Rust & café".into());
            }
            "--exit-after-smoke" => app.exit_after_smoke = true,
            "--evidence-dir" => {
                i += 1;
                app.evidence_dir = args
                    .get(i)
                    .ok_or("--evidence-dir requires a directory")?
                    .clone();
            }
            "--help" => {
                println!(
                    "mgbrowser [URL] [--enable-scripts] [--remote-debugging-port PORT] [--smoke-search QUERY | --smoke-events] [--exit-after-smoke] [--evidence-dir DIR]\nOwn JavaScript interpreter is experimental and opt-in; see docs/JAVASCRIPT.md.\nCDP is opt-in, loopback-only, partial; port 0 selects an available port. See docs/CDP.md.\nCtrl+L address; Enter navigate/submit; Tab fields; mouse click links; wheel scroll; Alt+Left back.\nRequires X11/XWayland and a font file (MGBROWSER_FONT can override)."
                );
                return Ok(());
            }
            value if value.starts_with('-') => {
                return Err(format!("Unknown option: {value}").into());
            }
            value => initial = value.to_string(),
        }
        i += 1;
    }
    let mut cdp = debug_port.map(cdp_browser::BrowserCdp::bind).transpose()?;
    app.cdp_enabled = cdp.is_some();
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let depth = screen.root_depth;
    let format = conn
        .setup()
        .pixmap_formats
        .iter()
        .find(|f| f.depth == depth)
        .ok_or("Unsupported X11 visual")?;
    if format.bits_per_pixel != 32 || conn.setup().image_byte_order != ImageOrder::LSB_FIRST {
        return Err("Initial window backend requires a 32-bit little-endian pixel surface".into());
    }
    let window = conn.generate_id()?;
    let gc = conn.generate_id()?;
    conn.create_window(
        depth,
        window,
        screen.root,
        40,
        40,
        app.width as u16,
        app.height as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().background_pixel(BG).event_mask(
            EventMask::EXPOSURE
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::KEY_PRESS
                | EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE,
        ),
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"mgbrowser - research browser",
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"mgbrowser\0mgbrowser\0",
    )?;
    let protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
    let close = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        protocols,
        AtomEnum::ATOM,
        &[close],
    )?;
    conn.create_gc(gc, window, &CreateGCAux::new())?;
    conn.map_window(window)?;
    conn.flush()?;
    let first = conn.setup().min_keycode;
    let mapping = conn
        .get_keyboard_mapping(first, conn.setup().max_keycode - first + 1)?
        .reply()?;
    eprintln!("WINDOW id={window} pid={}", std::process::id());
    app.navigate(initial, None, true);
    loop {
        while let Some(event) = conn.poll_for_event()? {
            match event {
                Event::Expose(_) => app.dirty = true,
                Event::ConfigureNotify(e) => {
                    app.width = (e.width as u32).clamp(360, 2400);
                    app.height = (e.height as u32).clamp(240, 1800);
                    app.dirty = true;
                }
                Event::ButtonPress(e) => match e.detail {
                    1 => app.pointer_press = Some((e.event_x as i32, e.event_y as i32)),
                    4 => app.scroll_by(-100),
                    5 => app.scroll_by(100),
                    _ => {}
                },
                Event::ButtonRelease(e) if e.detail == 1 => {
                    if let Some((x, y)) = app.pointer_press.take() {
                        let release = (e.event_x as i32, e.event_y as i32);
                        let press_hit = app.hits.iter().position(|hit| hit.contains(x, y));
                        let release_hit = app
                            .hits
                            .iter()
                            .position(|hit| hit.contains(release.0, release.1));
                        if press_hit.is_some() && press_hit == release_hit {
                            app.click(release.0, release.1);
                        }
                    }
                }
                Event::KeyPress(e) => {
                    let shift = e.state.contains(KeyButMask::SHIFT);
                    let ctrl = e.state.contains(KeyButMask::CONTROL);
                    let alt = e.state.contains(KeyButMask::MOD1);
                    let base = (e.detail.saturating_sub(first)) as usize
                        * mapping.keysyms_per_keycode as usize;
                    let mut sym = *mapping.keysyms.get(base + usize::from(shift)).unwrap_or(&0);
                    if sym == 0 {
                        sym = *mapping.keysyms.get(base).unwrap_or(&0);
                    }
                    app.key(sym, ctrl, shift, alt);
                }
                Event::ClientMessage(e) if e.data.as_data32()[0] == close => return Ok(()),
                Event::DestroyNotify(_) => return Ok(()),
                _ => {}
            }
        }
        app.poll();
        if let Some(cdp) = &mut cdp {
            cdp.tick(&mut app);
        }
        if app.dirty {
            let canvas = app.paint();
            // Split uploads below the core X11 request-size limit.
            let rows = (200_000 / (app.width as usize * 4)).max(1);
            for (chunk, pixels) in canvas.pixels.chunks(rows * app.width as usize).enumerate() {
                let data: Vec<_> = pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
                conn.put_image(
                    ImageFormat::Z_PIXMAP,
                    window,
                    gc,
                    app.width as u16,
                    (pixels.len() / app.width as usize) as u16,
                    0,
                    (chunk * rows) as i16,
                    0,
                    depth,
                    &data,
                )?;
            }
            conn.flush()?;
            let title = format!("{} - mgbrowser", app.document.title);
            conn.change_property8(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                title.as_bytes(),
            )?;
            app.smoke_step(&canvas);
        } else if app.smoke.is_some() && !app.loading {
            app.dirty = true;
        }
        if app.exit_after_smoke && app.smoke_stage == 3 {
            if app.smoke_failed {
                drop(app);
                std::process::exit(2);
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

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
        let mut app = App::new().unwrap();
        app.document = document::parse(
            include_str!("../tests/fixtures/journey/home.html"),
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
        let mut app = App::new().unwrap();
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
        let mut app = App::new().unwrap();
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
        let mut invalid_report = mg_deps::js::runtime::Runtime::new().allocation_report();
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
                    snapshot: Some(mg_deps::page_session::ArenaSnapshot {
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
            let mut app = App::new().unwrap();
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
            let mut app = App::new().unwrap();
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
        let mut app = App::new().unwrap();
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
                        snapshot: Some(mg_deps::page_session::ArenaSnapshot {
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
        let mut app = App::new().unwrap();
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
        let mut app = App::new().unwrap();
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
