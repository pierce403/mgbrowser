//! Retained, externally delivered activation. No synchronous JS dispatch facade.
use super::*;
use crate::page_session::{
    ControlAck, ControlEdit, InputKind, MAX_EDIT_BYTES, MAX_EDITS, MAX_EVENT_BYTES, MAX_EVENTS,
};

struct EventSize(usize);
impl std::io::Write for EventSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_EVENT_BYTES.saturating_sub(self.0) {
            return Err(std::io::Error::other("Event request limit exceeded"));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Window,
    Node(usize),
}
impl Target {
    pub(super) fn value(self) -> Value {
        match self {
            Self::Window => Value::Object(0),
            Self::Node(0) => Value::Host("document".into()),
            Self::Node(id) => Value::Host(format!("node:{id}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    DomReady,
    Load,
    Click,
    Submit,
}
impl Kind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::DomReady => "DOMContentLoaded",
            Self::Load => "load",
            Self::Click => "click",
            Self::Submit => "submit",
        }
    }
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "DOMContentLoaded" => Ok(Self::DomReady),
            "load" => Ok(Self::Load),
            "click" => Ok(Self::Click),
            "submit" => Ok(Self::Submit),
            _ => Err("Unsupported event type".into()),
        }
    }
    fn property(name: &str) -> Option<Self> {
        match name {
            "onclick" => Some(Self::Click),
            "onsubmit" => Some(Self::Submit),
            _ => None,
        }
    }
}

pub(super) struct Listener {
    pub(super) target: Target,
    pub(super) kind: Kind,
    pub(super) callback: Rc<Value>,
    capture: bool,
    property: bool,
    pub(super) active: bool,
}

pub(super) struct EventRecord {
    kind: Kind,
    target: Target,
    current: Option<Target>,
    submitter: Option<usize>,
    phase: u8,
    canceled: bool,
    stopped: bool,
    immediate: bool,
}

fn callable(value: &Value) -> bool {
    matches!(value, Value::Function(_) | Value::Native(_))
}
fn same_callback(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Function(a), Value::Function(b)) => a == b,
        (Value::Native(a), Value::Native(b)) => a == b,
        _ => false,
    }
}

impl BrowserHost {
    fn event_target(&self, value: &Value) -> Result<Target, String> {
        match value {
            Value::Object(0) => Ok(Target::Window),
            Value::Host(handle) if handle == "window" => Ok(Target::Window),
            Value::Host(handle) => self.node(handle).map(Target::Node),
            _ => Err("EventTarget receiver is not supported".into()),
        }
    }
    fn listener_capacity(&mut self) -> Result<(), String> {
        if self.listeners.len() >= 32 {
            return Err("Listener limit exceeded".into());
        }
        if self.listeners.len() == self.listeners.capacity() {
            let old = self.listeners.capacity();
            let next = (old.max(1) * 2).min(32);
            self.charge(
                (next - old) * std::mem::size_of::<Listener>() + usize::from(old == 0) * 16,
            )?;
            self.listeners.reserve_exact(next - self.listeners.len());
        }
        Ok(())
    }
    fn retain_callback(&mut self, callback: Value) -> Result<Rc<Value>, String> {
        // The incoming Value is moved. Only the actually allocated Rc block is
        // new storage; Native payloads are not cloned during registration.
        self.charge(std::mem::size_of::<Value>() + 2 * std::mem::size_of::<usize>() + 16)?;
        Ok(Rc::new(callback))
    }
    pub(super) fn event_property_get(
        &mut self,
        target: Target,
        key: &str,
    ) -> Option<Result<Value, String>> {
        let kind = Kind::property(key)?;
        let callback = self
            .listeners
            .iter()
            .find(|l| l.active && l.property && l.target == target && l.kind == kind)
            .map(|l| Rc::clone(&l.callback));
        Some((|| {
            let Some(callback) = callback else {
                return Ok(Value::Null);
            };
            if let Value::Native(name) = callback.as_ref() {
                self.charge(name.len())?;
            }
            Ok(callback.as_ref().clone())
        })())
    }
    pub(super) fn event_property_set(
        &mut self,
        target: Target,
        key: &str,
        value: Value,
    ) -> Result<bool, String> {
        let Some(kind) = Kind::property(key) else {
            return Ok(false);
        };
        let index = self
            .listeners
            .iter()
            .position(|l| l.active && l.property && l.target == target && l.kind == kind);
        // Event handler IDL properties clear for non-callable values. Object
        // listener options and handleEvent objects remain explicitly unsupported.
        if !callable(&value) {
            if let Some(index) = index {
                self.listeners[index].active = false;
            }
            return Ok(true);
        }
        if index.is_none() {
            self.listener_capacity()?;
        }
        let callback = self.retain_callback(value)?;
        if let Some(index) = index {
            self.listeners[index].callback = callback;
        } else {
            self.listeners.push(Listener {
                target,
                kind,
                callback,
                capture: false,
                property: true,
                active: true,
            });
        }
        Ok(true)
    }
    pub(super) fn listener_call(
        &mut self,
        name: &str,
        this: Value,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let target = self.event_target(&this)?;
        let mut args = args.into_iter();
        let kind = Kind::parse(&args.next().unwrap_or(Value::Undefined).as_dom_text()?)?;
        let callback = args.next().unwrap_or(Value::Undefined);
        let option = args.next().unwrap_or(Value::Undefined);
        // Preserve the existing startup approximation: its unused third
        // argument is neither interpreted nor coerced. Genuine later click /
        // submit dispatch accepts only the separately adopted boolean option.
        let capture = if matches!(kind, Kind::DomReady | Kind::Load) {
            false
        } else {
            match option {
                Value::Undefined => false,
                Value::Bool(value) => value,
                _ => return Err("Only boolean listener capture is implemented".into()),
            }
        };
        if matches!(callback, Value::Null | Value::Undefined) {
            return Ok(Value::Undefined);
        }
        if !callable(&callback) {
            return Err("Listener must be a function".into());
        }
        let index = self.listeners.iter().position(|l| {
            l.active
                && !l.property
                && l.target == target
                && l.kind == kind
                && l.capture == capture
                && same_callback(&l.callback, &callback)
        });
        if name.ends_with("removeEventListener") {
            if let Some(index) = index {
                self.listeners[index].active = false;
            }
        } else if index.is_none() {
            self.listener_capacity()?;
            let callback = self.retain_callback(callback)?;
            self.listeners.push(Listener {
                target,
                kind,
                callback,
                capture,
                property: false,
                active: true,
            });
        }
        Ok(Value::Undefined)
    }
    fn new_event(
        &mut self,
        kind: Kind,
        target: Target,
        submitter: Option<usize>,
    ) -> Result<usize, String> {
        if self.events.len() >= MAX_EVENTS {
            return Err("Event record limit exceeded".into());
        }
        if self.events.len() == self.events.capacity() {
            let old = self.events.capacity();
            let next = (old.max(1) * 2).min(MAX_EVENTS);
            self.charge(
                (next - old) * std::mem::size_of::<EventRecord>() + usize::from(old == 0) * 16,
            )?;
            self.events.reserve_exact(next - self.events.len());
        }
        let id = self.events.len();
        self.events.push(EventRecord {
            kind,
            target,
            current: None,
            submitter,
            phase: 0,
            canceled: false,
            stopped: false,
            immediate: false,
        });
        Ok(id)
    }
    pub(super) fn event_get(&self, id: usize, key: &str) -> Result<Value, String> {
        let event = self.events.get(id).ok_or("Stale event handle")?;
        Ok(match key {
            "type" => Value::text(event.kind.name()),
            "target" => event.target.value(),
            "currentTarget" => event.current.map(Target::value).unwrap_or(Value::Null),
            "eventPhase" => Value::Number(event.phase as f64),
            "defaultPrevented" => Value::Bool(event.canceled),
            "bubbles" | "cancelable" => Value::Bool(true),
            "submitter" if event.kind == Kind::Submit => event
                .submitter
                .map(|id| Target::Node(id).value())
                .unwrap_or(Value::Null),
            "preventDefault" | "stopPropagation" | "stopImmediatePropagation" => {
                Value::Native(format!("host.event.{key}"))
            }
            _ => Value::Undefined,
        })
    }
    pub(super) fn event_call(&mut self, name: &str, this: Value) -> Result<Value, String> {
        let Value::Host(handle) = this else {
            return Err("Invalid Event receiver".into());
        };
        let id = handle
            .strip_prefix("event:")
            .and_then(|id| id.parse::<usize>().ok())
            .ok_or("Invalid Event receiver")?;
        let event = self.events.get_mut(id).ok_or("Stale event handle")?;
        match name {
            "host.event.preventDefault" => event.canceled = true,
            "host.event.stopPropagation" => event.stopped = true,
            "host.event.stopImmediatePropagation" => {
                event.stopped = true;
                event.immediate = true;
            }
            _ => return Err("Unsupported Event method".into()),
        }
        Ok(Value::Undefined)
    }
    fn connected_path(&self, mut node: usize) -> Result<([Target; 258], usize), String> {
        let mut path = [Target::Window; 258];
        for depth in 0..=256 {
            let entry = self
                .document
                .nodes
                .get(node)
                .ok_or("Invalid event target")?;
            path[depth] = Target::Node(node);
            if node == 0 {
                path[depth + 1] = Target::Window;
                return Ok((path, depth + 2));
            }
            if entry.parent == node {
                return Err("Detached event target".into());
            }
            node = entry.parent;
        }
        Err("Event target depth limit exceeded".into())
    }
    fn is_submitter(&self, id: usize) -> bool {
        let Some(node) = self.document.nodes.get(id) else {
            return false;
        };
        !node.has("disabled")
            && match node.tag.as_str() {
                "button" => node
                    .attr("type")
                    .is_none_or(|kind| kind.eq_ignore_ascii_case("submit")),
                "input" => node
                    .attr("type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("submit")),
                _ => false,
            }
    }
    fn form_owner(&self, node: usize) -> Option<usize> {
        if let Some(name) = self.document.nodes[node].attr("form") {
            return self.find_connected(|n| n.tag == "form" && n.attr("id") == Some(name));
        }
        let (path, len) = self.connected_path(node).ok()?;
        path[..len].iter().find_map(|target| match target {
            Target::Node(id) if self.document.nodes[*id].tag == "form" => Some(*id),
            _ => None,
        })
    }
    fn validate_input(&self, input: &SessionInput) -> Result<(), String> {
        // Count the actual JSON escaping without creating a second source-sized
        // buffer. Transport additionally includes its envelope in this bound.
        serde_json::to_writer(EventSize(0), input)
            .map_err(|_| "Event request limit exceeded".to_string())?;
        match input.kind {
            InputKind::Click { target } => {
                self.connected_path(target)?;
            }
            InputKind::Submit { form, submitter } => {
                self.connected_path(form)?;
                if self.document.nodes[form].tag != "form" {
                    return Err("Submit target is not a form".into());
                }
                if let Some(submitter) = submitter {
                    self.connected_path(submitter)?;
                    if !self.is_submitter(submitter) || self.form_owner(submitter) != Some(form) {
                        return Err("Invalid form submitter".into());
                    }
                }
            }
        }
        if input.edits.len() > MAX_EDITS {
            return Err("Control edit count limit exceeded".into());
        }
        let mut bytes = 0usize;
        let mut nodes = 0usize;
        for (position, edit) in input.edits.iter().enumerate() {
            if edit.value.len() > MAX_EDIT_BYTES
                || input.edits[..position]
                    .iter()
                    .any(|old| old.node == edit.node)
            {
                return Err("Invalid control edit".into());
            }
            let (_, depth) = self.connected_path(edit.node)?;
            let node = &self.document.nodes[edit.node];
            if node.has("disabled") || node.has("readonly") {
                return Err("Control is not editable".into());
            }
            match node.tag.as_str() {
                "textarea" => {
                    if depth > 257 {
                        return Err("Edited textarea depth limit exceeded".into());
                    }
                    nodes += 1;
                    bytes = bytes.saturating_add(133 + edit.value.len());
                }
                "input" => {
                    if node.attr("type").is_some_and(|kind| {
                        [
                            "hidden", "button", "submit", "reset", "file", "checkbox", "radio",
                            "image",
                        ]
                        .iter()
                        .any(|blocked| kind.eq_ignore_ascii_case(blocked))
                    }) {
                        return Err("Control is not editable".into());
                    }
                    if !node.has("value") && node.attributes.len() >= 256 {
                        return Err("Attribute count limit exceeded".into());
                    }
                    bytes = bytes.saturating_add(5 + edit.value.len());
                }
                _ => return Err("Edit target is not an input control".into()),
            }
        }
        if self.document.nodes.len().saturating_add(nodes) > MAX_NODES
            || self.allocated.saturating_add(bytes) > MAX_DOM_BYTES
        {
            return Err("Control edit DOM limit exceeded".into());
        }
        Ok(())
    }
}

impl PageRealm {
    fn listeners_at(
        &mut self,
        event: usize,
        target: Target,
        capture: bool,
        phase: u8,
        errors: &mut Vec<String>,
    ) {
        let mut ids = [0usize; 32];
        let mut count = 0;
        let kind = self.host.events[event].kind;
        for (id, listener) in self.host.listeners.iter().enumerate() {
            if listener.active
                && listener.kind == kind
                && listener.target == target
                && listener.capture == capture
            {
                ids[count] = id;
                count += 1;
            }
        }
        self.host.events[event].current = Some(target);
        self.host.events[event].phase = phase;
        for id in &ids[..count] {
            if self.runtime.is_fatal() || self.host.events[event].immediate {
                break;
            }
            let listener = &self.host.listeners[*id];
            if !listener.active {
                continue;
            }
            let callback = Rc::clone(&listener.callback);
            let property = listener.property;
            match self.runtime.invoke_retained(
                &callback,
                target.value(),
                vec![Value::Host(format!("event:{event}"))],
                &mut self.host,
            ) {
                Ok(Value::Bool(false)) if property => self.host.events[event].canceled = true,
                Ok(_) => {}
                Err(error) if errors.len() < 64 => {
                    errors.push(format!("{} handler: {}", kind.name(), bounded(&error, 512)))
                }
                Err(_) => {}
            }
        }
    }
    fn emit(
        &mut self,
        kind: Kind,
        target: usize,
        submitter: Option<usize>,
        errors: &mut Vec<String>,
    ) -> Result<bool, String> {
        let (path, len) = self.host.connected_path(target)?;
        let id = self.host.new_event(kind, Target::Node(target), submitter)?;
        for owner in path[1..len].iter().rev() {
            if self.runtime.is_fatal() || self.host.events[id].stopped {
                break;
            }
            self.listeners_at(id, *owner, true, 1, errors);
        }
        if !self.runtime.is_fatal() && !self.host.events[id].stopped {
            self.listeners_at(id, path[0], true, 2, errors);
            if !self.runtime.is_fatal() && !self.host.events[id].immediate {
                self.listeners_at(id, path[0], false, 2, errors);
            }
        }
        for owner in &path[1..len] {
            if self.runtime.is_fatal() || self.host.events[id].stopped {
                break;
            }
            self.listeners_at(id, *owner, false, 3, errors);
        }
        self.host.events[id].current = None;
        self.host.events[id].phase = 0;
        Ok(self.host.events[id].canceled)
    }
    fn submit_activation(
        &mut self,
        form: usize,
        submitter: Option<usize>,
        outcome: &mut EventOutcome,
        errors: &mut Vec<String>,
    ) -> Result<DefaultAction, String> {
        if self.runtime.is_fatal() || self.host.navigation.is_some() {
            return Ok(DefaultAction::None);
        }
        if self.host.connected_path(form).is_err() {
            return Ok(DefaultAction::None);
        }
        let canceled = self.emit(Kind::Submit, form, submitter, errors)?;
        outcome.submit_canceled = Some(canceled);
        // Conservative retained-slice policy, not full HTML submission: keep
        // successful handler mutations but suppress an activation whose button
        // is no longer an enabled submit control belonging to this form.
        let stale_submitter = submitter.is_some_and(|node| {
            self.host.connected_path(node).is_err()
                || !self.host.is_submitter(node)
                || self.host.form_owner(node) != Some(form)
        });
        if canceled
            || self.runtime.is_fatal()
            || self.host.navigation.is_some()
            || self.host.connected_path(form).is_err()
            || stale_submitter
        {
            Ok(DefaultAction::None)
        } else {
            Ok(DefaultAction::SubmitForm { form, submitter })
        }
    }
    fn click_activation(
        &mut self,
        target: usize,
        outcome: &mut EventOutcome,
        errors: &mut Vec<String>,
    ) -> Result<DefaultAction, String> {
        // The activation owner is selected from the original path. Its current
        // attributes and form owner are inspected only after handler execution.
        let (path, len) = self.host.connected_path(target)?;
        let activation = path[..len].iter().find_map(|target| match target {
            Target::Node(id)
                if self.host.is_submitter(*id)
                    || self.host.document.nodes[*id].tag == "a"
                        && self.host.document.nodes[*id].has("href") =>
            {
                Some(*id)
            }
            _ => None,
        });
        let canceled = self.emit(Kind::Click, target, None, errors)?;
        outcome.click_canceled = Some(canceled);
        if canceled || self.runtime.is_fatal() || self.host.navigation.is_some() {
            return Ok(DefaultAction::None);
        }
        let Some(node) = activation else {
            return Ok(DefaultAction::None);
        };
        if self.host.connected_path(node).is_err() {
            return Ok(DefaultAction::None);
        }
        if self.host.is_submitter(node) {
            if let Some(form) = self.host.form_owner(node) {
                return self.submit_activation(form, Some(node), outcome, errors);
            }
        } else if self.host.document.nodes[node].tag == "a"
            && self.host.document.nodes[node].has("href")
        {
            return Ok(DefaultAction::FollowLink { node });
        }
        Ok(DefaultAction::None)
    }
    pub fn dispatch(&mut self, input: SessionInput) -> SessionReply {
        let mut reply = SessionReply {
            revision: self.revision,
            snapshot: None,
            outcome: EventOutcome::default(),
            default_action: DefaultAction::None,
            navigation: None,
            errors: Vec::new(),
            scripts_executed: 0,
            allocations: Some(self.runtime.allocation_report()),
            state: RealmState::Closed,
            acknowledgements: Vec::new(),
        };
        if self.closed || self.runtime.is_fatal() {
            self.closed = true;
            reply.state = if self.runtime.is_fatal() {
                RealmState::Fatal
            } else {
                RealmState::Closed
            };
            reply.errors.push("Page realm is no longer active".into());
            return reply;
        }
        if let Err(error) = self.host.validate_input(&input) {
            self.closed = true;
            reply.errors.push(error);
            return reply;
        }
        self.revision += 1;
        reply.revision = self.revision;
        self.host.navigation = None;
        let result = (|| {
            for ControlEdit {
                node,
                version,
                value,
            } in input.edits
            {
                if self.host.document.nodes[node].tag == "textarea" {
                    self.host.replace_text(node, value)?;
                } else {
                    self.host.attr(node, "value", value)?;
                }
                reply.acknowledgements.push(ControlAck { node, version });
            }
            match input.kind {
                InputKind::Click { target } => {
                    self.click_activation(target, &mut reply.outcome, &mut reply.errors)
                }
                InputKind::Submit {
                    submitter: Some(target),
                    ..
                } => self.click_activation(target, &mut reply.outcome, &mut reply.errors),
                InputKind::Submit {
                    form,
                    submitter: None,
                } => self.submit_activation(form, None, &mut reply.outcome, &mut reply.errors),
            }
        })();
        match result {
            Ok(default) => reply.default_action = default,
            Err(error) => {
                self.closed = true;
                reply.errors.push(error);
            }
        }
        reply.allocations = Some(self.runtime.allocation_report());
        reply.state = if self.runtime.is_fatal() {
            RealmState::Fatal
        } else if self.closed {
            RealmState::Closed
        } else {
            RealmState::Ready
        };
        if reply.state != RealmState::Ready {
            reply.default_action = DefaultAction::None;
            self.closed = true;
        }
        // Serialization/projection is bounded temporary IPC storage, not retained
        // DOM allocation. The parent validates this full arena before adoption.
        if let Err(error) = self.host.serialize(0, true) {
            self.closed = true;
            reply.state = RealmState::Closed;
            reply.default_action = DefaultAction::None;
            reply.errors.push(error);
        } else {
            reply.snapshot = Some(ArenaSnapshot {
                nodes: self.host.document.nodes.clone(),
            });
            if reply.state == RealmState::Ready {
                reply.navigation = self.host.navigation.take();
            }
        }
        reply
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn realm(script: &str) -> PageRealm {
        let (realm, reply) = PageRealm::start(Request {
            url: "https://fixture.test/".into(),
            html: format!(
                "<html><body><p id=state></p><input id=q value=old><textarea id=text>old</textarea><a id=a href=/next>Link</a><script>{script}</script></body></html>"
            ),
        });
        assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        realm.unwrap()
    }
    fn id(realm: &PageRealm, name: &str) -> usize {
        realm
            .host
            .document
            .query_selector(0, &format!("#{name}"))
            .unwrap()
            .unwrap()
    }
    fn click(realm: &mut PageRealm, target: usize) -> SessionReply {
        realm.dispatch(SessionInput {
            kind: InputKind::Click { target },
            edits: vec![],
        })
    }

    #[test]
    fn layouts_bootstrap_and_transient_callback_copy_are_accounted() {
        eprintln!(
            "SESSION_LAYOUT listener={} event={} value={} runtime={}",
            std::mem::size_of::<Listener>(),
            std::mem::size_of::<EventRecord>(),
            std::mem::size_of::<Value>(),
            std::mem::size_of::<Runtime>()
        );
        let raw = Runtime::new().allocation_report();
        assert_eq!(raw.phases.bootstrap, 25_999);
        let mut empty = realm("");
        eprintln!("SESSION_EMPTY {:?}", empty.runtime.allocation_report());
        assert_eq!(
            empty.runtime.allocation_report().phases.bootstrap,
            raw.phases.bootstrap
        );
        let mut old = Runtime::new();
        for name in ["window", "self", "globalThis"] {
            old.set_global(name, Value::Object(0));
        }
        old.set_global("document", Value::Host("document".into()));
        old.set_global("location", Value::Host("location".into()));
        old.set_global_setter("location", "location", "href");
        old.set_global("navigator", Value::Host("navigator".into()));
        old.set_global("console", Value::Host("console".into()));
        old.set_global(
            "addEventListener",
            Value::Native("host.window.addEventListener".into()),
        );
        let new_setup = empty.runtime.allocation_report().phases.runtime;
        let old_setup = old.allocation_report().phases.runtime;
        eprintln!(
            "SESSION_HOST_SETUP old={old_setup} new={new_setup} added={}",
            new_setup - old_setup
        );
        // A real remove method (178), click getter/property + paired setter
        // (306), and submit counterpart (310); no language bootstrap change.
        assert_eq!(new_setup - old_setup, 178 + 306 + 310);
        let target = id(&empty, "a");
        let before = empty.runtime.allocation_report();
        let dom = empty.host.allocated;
        assert_eq!(
            click(&mut empty, target).default_action,
            DefaultAction::FollowLink { node: target }
        );
        assert_eq!(
            empty.runtime.allocation_report(),
            before,
            "no callback means no new interpreter work"
        );
        assert_eq!(
            empty.host.allocated - dom,
            2 * std::mem::size_of::<EventRecord>() + 16
        );
        assert_eq!(empty.host.events.len(), 1);
    }

    #[test]
    fn startup_and_later_callbacks_share_one_realm_without_replaying() {
        let mut page = realm(
            "var starts=1;var count=0;document.addEventListener('DOMContentLoaded',function(){count++;});document.getElementById('a').onclick=function(e){count++;document.getElementById('state').textContent=starts+':'+count;e.preventDefault();};",
        );
        let target = id(&page, "a");
        for expected in ["1:2", "1:3"] {
            let reply = click(&mut page, target);
            assert!(reply.errors.is_empty(), "{:?}", reply.errors);
            assert_eq!(reply.default_action, DefaultAction::None);
            assert_eq!(
                text_content(&page.host.document.nodes, id(&page, "state")),
                expected
            );
        }
    }

    #[test]
    fn global_accessors_have_real_metadata_and_page_read_write_identity() {
        let mut page = realm(
            "function handler(){document.getElementById('state').textContent='window';return false;}onclick=handler;if(onclick!==handler||window.onclick!==handler)throw 'identity';if(!Object.prototype.hasOwnProperty.call(window,'onclick')||delete window.onclick)throw 'metadata';",
        );
        let target = id(&page, "a");
        let reply = click(&mut page, target);
        assert_eq!(reply.outcome.click_canceled, Some(true));
        assert_eq!(
            text_content(&page.host.document.nodes, id(&page, "state")),
            "window"
        );
        assert!(
            page.runtime
                .execute("function onclick(){}", &mut page.host)
                .unwrap_err()
                .contains("conflicts with host global")
        );
        assert!(
            page.runtime
                .execute(
                    "onclick=null;if(window.onclick!==null)throw 'clear';",
                    &mut page.host
                )
                .is_ok()
        );
        assert_eq!(click(&mut page, target).outcome.click_canceled, Some(false));
    }

    #[test]
    fn invalid_or_oversized_edits_leave_the_entire_arena_untouched() {
        for mode in 0..4 {
            let mut page = realm("");
            let target = id(&page, "a");
            let q = id(&page, "q");
            let before = page.host.document.nodes.clone();
            let mut edits = vec![ControlEdit {
                node: q,
                version: 1,
                value: "new".into(),
            }];
            match mode {
                0 => edits.push(ControlEdit {
                    node: usize::MAX,
                    version: 2,
                    value: "bad".into(),
                }),
                1 => edits[0].value = "x".repeat(MAX_EDIT_BYTES + 1),
                2 => edits.push(ControlEdit {
                    node: q,
                    version: 2,
                    value: "duplicate".into(),
                }),
                3 => {
                    // Encoded control escapes count, not just raw string bytes.
                    edits[0].value = "\0".repeat(MAX_EDIT_BYTES);
                    edits.push(ControlEdit {
                        node: id(&page, "text"),
                        version: 2,
                        value: "\0".repeat(MAX_EDIT_BYTES),
                    });
                }
                _ => unreachable!(),
            }
            let reply = page.dispatch(SessionInput {
                kind: InputKind::Click { target },
                edits,
            });
            assert_eq!(reply.state, RealmState::Closed);
            assert_eq!(reply.revision, 0);
            assert!(reply.snapshot.is_none() && reply.acknowledgements.is_empty());
            assert_eq!(page.host.document.nodes, before);
        }
    }

    #[test]
    fn textarea_edit_admission_and_stable_nodes_are_atomic() {
        let mut page = realm("");
        let target = id(&page, "a");
        let q = id(&page, "q");
        let text = id(&page, "text");
        let old_child = page.host.document.nodes[text].children[0];
        let reply = page.dispatch(SessionInput {
            kind: InputKind::Click { target },
            edits: vec![
                ControlEdit {
                    node: q,
                    version: 17,
                    value: "Rust & café".into(),
                },
                ControlEdit {
                    node: text,
                    version: 18,
                    value: "new text".into(),
                },
            ],
        });
        assert_eq!(reply.state, RealmState::Ready);
        assert_eq!(
            reply.acknowledgements,
            vec![
                ControlAck {
                    node: q,
                    version: 17
                },
                ControlAck {
                    node: text,
                    version: 18
                }
            ]
        );
        assert_eq!(
            page.host.document.nodes[q].attr("value"),
            Some("Rust & café")
        );
        assert_eq!(text_content(&page.host.document.nodes, text), "new text");
        assert_eq!(page.host.document.nodes[old_child].parent, old_child);
        let before = page.host.document.nodes.clone();
        page.host.allocated = MAX_DOM_BYTES - 10;
        let rejected = page.dispatch(SessionInput {
            kind: InputKind::Click { target },
            edits: vec![
                ControlEdit {
                    node: q,
                    version: 19,
                    value: "x".into(),
                },
                ControlEdit {
                    node: text,
                    version: 20,
                    value: "y".into(),
                },
            ],
        });
        assert_eq!(rejected.state, RealmState::Closed);
        assert_eq!(page.host.document.nodes, before);
    }

    #[test]
    fn removed_registrations_never_recycle_lifetime_slots() {
        let mut page = realm("");
        let target = Target::Node(id(&page, "a"));
        for _ in 0..32 {
            page.host
                .listener_call(
                    "host.dom.addEventListener",
                    target.value(),
                    vec![Value::text("click"), Value::Native("Math.abs".into())],
                )
                .unwrap();
            page.host
                .listener_call(
                    "host.dom.removeEventListener",
                    target.value(),
                    vec![Value::text("click"), Value::Native("Math.abs".into())],
                )
                .unwrap();
        }
        assert_eq!(page.host.listeners.len(), 32);
        assert!(page.host.listeners.iter().all(|l| !l.active));
        let old = page.host.allocated;
        assert!(
            page.host
                .listener_call(
                    "host.dom.addEventListener",
                    target.value(),
                    vec![Value::text("click"), Value::Native("Math.abs".into())]
                )
                .unwrap_err()
                .contains("Listener limit")
        );
        assert_eq!(page.host.allocated, old);
    }

    #[test]
    fn event_slots_are_prepaid_before_any_callback_and_never_reused() {
        let mut page = realm(
            "document.getElementById('a').onclick=function(){document.getElementById('state').textContent='wrong';};",
        );
        let target = id(&page, "a");
        page.host.allocated = MAX_DOM_BYTES;
        let report = page.runtime.allocation_report();
        let reply = click(&mut page, target);
        assert_eq!(reply.state, RealmState::Closed);
        assert_eq!(reply.default_action, DefaultAction::None);
        assert!(page.host.events.is_empty());
        assert_eq!(page.runtime.allocation_report(), report);
        assert_eq!(
            text_content(&page.host.document.nodes, id(&page, "state")),
            ""
        );
        let mut page = realm("");
        let target = id(&page, "a");
        for _ in 0..MAX_EVENTS {
            assert_eq!(click(&mut page, target).state, RealmState::Ready);
        }
        assert_eq!(page.host.events.len(), MAX_EVENTS);
        assert_eq!(click(&mut page, target).state, RealmState::Closed);
        assert_eq!(page.host.events.len(), MAX_EVENTS);
    }

    #[test]
    fn event_path_is_fixed_while_default_owner_is_rechecked() {
        let mut page = realm(
            "var a=document.getElementById('a');var old=document.body;var out='';old.addEventListener('click',function(){out+='B';document.getElementById('state').textContent=out;});a.onclick=function(e){out+='A';document.body.removeChild(a);};",
        );
        let target = id(&page, "a");
        let reply = click(&mut page, target);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(
            text_content(&page.host.document.nodes, id(&page, "state")),
            "AB"
        );
        assert_eq!(reply.default_action, DefaultAction::None);
        assert_eq!(page.host.document.nodes[target].parent, target);
        assert_eq!(click(&mut page, target).state, RealmState::Closed);
    }

    #[test]
    fn fatal_initialization_suppresses_earlier_navigation_intent() {
        let (_, reply) = PageRealm::start(Request {
            url: "https://fixture.test/".into(),
            html: "<script>location.href='/must-not-navigate';while(true){}</script>".into(),
        });
        assert_eq!(reply.state, RealmState::Fatal);
        assert!(reply.navigation.is_none());
        assert_eq!(reply.default_action, DefaultAction::None);
    }

    #[test]
    fn post_handler_stale_submitter_suppresses_only_the_default() {
        for mutation in [
            "f.removeChild(b);",
            "b.setAttribute('disabled','');",
            "b.setAttribute('form','other');",
        ] {
            let (page, initial) = PageRealm::start(Request {
                url: "https://fixture.test/".into(),
                html: format!(
                    "<html><body><p id=state></p><form id=f action=/submit><button id=b>Go</button></form><form id=other></form><script>var f=document.getElementById('f'),b=document.getElementById('b');f.onsubmit=function(){{{mutation}document.getElementById('state').textContent='accepted mutation';}};</script></body></html>"
                ),
            });
            assert!(initial.errors.is_empty(), "{:?}", initial.errors);
            let mut page = page.unwrap();
            let target = id(&page, "b");
            let reply = click(&mut page, target);
            assert_eq!(
                reply.state,
                RealmState::Ready,
                "{mutation}: {:?}",
                reply.errors
            );
            assert!(reply.errors.is_empty(), "{:?}", reply.errors);
            assert_eq!(reply.revision, 1);
            assert_eq!(reply.outcome.submit_canceled, Some(false));
            assert_eq!(reply.default_action, DefaultAction::None);
            assert!(reply.snapshot.is_some());
            assert_eq!(
                text_content(&page.host.document.nodes, id(&page, "state")),
                "accepted mutation"
            );
            assert!(!page.closed);
        }
    }

    #[test]
    fn startup_third_arguments_remain_unused_while_later_options_are_strict() {
        let page = realm(
            "var out='';var unused={toString:function(){throw 'coerced unused';}};document.addEventListener('DOMContentLoaded',function(){out+='D';document.getElementById('state').textContent=out;},unused);window.addEventListener('load',function(){out+='L';document.getElementById('state').textContent=out;},unused);var rejected=0;try{document.addEventListener('click',function(){},unused);}catch(e){rejected++;}try{document.addEventListener('submit',function(){},unused);}catch(e){rejected++;}if(rejected!==2)throw 'later options accepted';",
        );
        assert_eq!(
            text_content(&page.host.document.nodes, id(&page, "state")),
            "DL"
        );
        assert_eq!(page.host.listeners.len(), 2);
        assert!(page.host.listeners.iter().all(|listener| !listener.capture));
    }
}
