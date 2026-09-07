//! Explicit browser capabilities for the isolated original JavaScript interpreter.
use crate::{
    document::{self, Document, Node},
    js::runtime::{Host, Runtime, Value},
};
use serde::{Deserialize, Serialize};

pub const MAX_SOURCE: usize = 1024 * 1024;
const MAX_DOM_BYTES: usize = 4 * 1024 * 1024;
const MAX_NODES: usize = 50_000;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub url: String,
    pub html: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub applied: bool,
    pub html: String,
    pub navigation: Option<String>,
    pub errors: Vec<String>,
    pub scripts_executed: usize,
}

struct BrowserHost {
    document: Document,
    url: url::Url,
    navigation: Option<String>,
    allocated: usize,
    collections: Vec<Vec<usize>>,
    listeners: Vec<(String, Value, Value)>,
    ready: &'static str,
}

/// Called inside the restricted worker in the browser, or on owned local fixtures
/// in unit tests. This function has no filesystem/network/process capabilities.
pub fn execute(request: Request) -> Reply {
    let mut reply = Reply {
        applied: false,
        html: request.html.clone(),
        navigation: None,
        errors: Vec::new(),
        scripts_executed: 0,
    };
    if request.html.len() > MAX_SOURCE || request.url.len() > 16_384 {
        reply
            .errors
            .push("Script document exceeds the source/URL limit".into());
        return reply;
    }
    let url = match url::Url::parse(&request.url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url,
        _ => {
            reply
                .errors
                .push("Script document requires an HTTP(S) origin".into());
            return reply;
        }
    };
    let document = document::parse_with_scripting(&request.html, url.as_str(), true);
    let mut scripts = Vec::new();
    let mut script_count = 0;
    for (index, node) in document.nodes.iter().enumerate() {
        if node.tag != "script" || inert(&document.nodes, index) {
            continue;
        }
        let kind = node.attr("type").unwrap_or("").trim().to_ascii_lowercase();
        if matches!(
            kind.as_str(),
            "" | "module"
                | "text/javascript"
                | "application/javascript"
                | "text/ecmascript"
                | "application/ecmascript"
        ) {
            script_count += 1;
            if script_count > 32 {
                reply.errors.push("Script count limit reached".into());
                break;
            }
        }
        if !matches!(
            kind.as_str(),
            "" | "text/javascript"
                | "application/javascript"
                | "text/ecmascript"
                | "application/ecmascript"
        ) {
            if kind == "module" {
                reply
                    .errors
                    .push("Module scripts are not implemented".into());
            }
            continue;
        }
        if node.attr("src").is_some() {
            reply
                .errors
                .push("External script loading is not implemented".into());
            continue;
        }
        scripts.push(text_content(&document.nodes, index));
    }
    let mut host = BrowserHost {
        document,
        url,
        navigation: None,
        allocated: request.html.len(),
        collections: Vec::new(),
        listeners: Vec::new(),
        ready: "loading",
    };
    let mut runtime = Runtime::new();
    let global = runtime.global_object();
    runtime.set_global("window", global.clone());
    runtime.set_global("self", global.clone());
    runtime.set_global("globalThis", global.clone());
    runtime.set_global("document", Value::Host("document".into()));
    runtime.set_global("location", Value::Host("location".into()));
    runtime.set_global_setter("location", "location", "href");
    runtime.set_global("navigator", Value::Host("navigator".into()));
    runtime.set_global("console", Value::Host("console".into()));
    runtime.set_global(
        "addEventListener",
        Value::Native("host.window.addEventListener".into()),
    );
    for (index, source) in scripts.iter().enumerate() {
        match runtime.execute(source, &mut host) {
            Ok(_) => reply.scripts_executed += 1,
            Err(error) => reply.errors.push(format!(
                "Inline script {}: {}",
                index + 1,
                bounded(&error, 512)
            )),
        }
        if host.navigation.is_some() {
            break;
        }
    }
    if host.navigation.is_none() {
        host.ready = "interactive";
        for event in ["DOMContentLoaded", "load"] {
            if event == "load" {
                host.ready = "complete";
            }
            let callbacks: Vec<_> = host
                .listeners
                .iter()
                .filter(|(name, _, _)| name == event)
                .cloned()
                .collect();
            for (_, callback, target) in callbacks {
                if let Err(error) = runtime.invoke(
                    callback,
                    target,
                    vec![Value::Host(format!("event:{event}"))],
                    &mut host,
                ) {
                    reply
                        .errors
                        .push(format!("{event} handler: {}", bounded(&error, 512)));
                }
                if host.navigation.is_some() {
                    break;
                }
            }
            if host.navigation.is_some() {
                break;
            }
        }
        let onload = runtime.get_global("onload");
        if matches!(onload, Value::Function(_) | Value::Native(_)) && host.navigation.is_none() {
            if let Err(error) = runtime.invoke(
                onload,
                global,
                vec![Value::Host("event:load".into())],
                &mut host,
            ) {
                reply
                    .errors
                    .push(format!("load handler: {}", bounded(&error, 512)));
            }
        }
    }
    match host.serialize(0, true) {
        Ok(html) => {
            reply.html = html;
            reply.applied = true;
            reply.navigation = host.navigation;
        }
        Err(error) => reply.errors.push(error),
    }
    reply.errors.truncate(64);
    reply
}

fn bounded(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
fn inert(nodes: &[Node], mut id: usize) -> bool {
    for _ in 0..=256 {
        if matches!(nodes[id].tag.as_str(), "template" | "noscript") {
            return true;
        }
        if id == 0 {
            return false;
        }
        id = nodes[id].parent;
    }
    true
}
fn text_content(nodes: &[Node], root: usize) -> String {
    let mut output = String::new();
    let mut pending = vec![root];
    for _ in 0..MAX_NODES {
        let Some(id) = pending.pop() else { break };
        if nodes[id].tag == "#text" {
            output.push_str(&nodes[id].text);
        } else {
            pending.extend(nodes[id].children.iter().rev().copied());
        }
        if output.len() > MAX_DOM_BYTES {
            break;
        }
    }
    output
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':'))
}
fn void(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

impl BrowserHost {
    fn charge(&mut self, bytes: usize) -> Result<(), String> {
        self.allocated = self.allocated.saturating_add(bytes);
        if self.allocated > MAX_DOM_BYTES {
            Err("DOM allocation limit exceeded".into())
        } else {
            Ok(())
        }
    }
    fn node(&self, handle: &str) -> Result<usize, String> {
        if handle == "document" {
            return Ok(0);
        }
        let id = handle
            .strip_prefix("node:")
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or("Invalid DOM handle")?;
        if id >= self.document.nodes.len() {
            return Err("Stale DOM handle".into());
        }
        Ok(id)
    }
    fn element(&self, id: usize) -> Value {
        Value::Host(format!("node:{id}"))
    }
    fn find_tag(&self, tag: &str) -> Value {
        self.document
            .query_selector(0, tag)
            .ok()
            .flatten()
            .map(|i| self.element(i))
            .unwrap_or(Value::Null)
    }
    fn find_connected(&self, predicate: impl Fn(&Node) -> bool) -> Option<usize> {
        let mut pending = vec![0];
        for _ in 0..MAX_NODES {
            let id = pending.pop()?;
            if predicate(&self.document.nodes[id]) {
                return Some(id);
            }
            pending.extend(self.document.nodes[id].children.iter().rev().copied());
        }
        None
    }
    fn base_url(&self) -> url::Url {
        self.find_connected(|n| n.tag == "base" && n.attr("href").is_some())
            .and_then(|i| self.document.nodes[i].attr("href"))
            .and_then(|href| self.url.join(href).ok())
            .unwrap_or_else(|| self.url.clone())
    }
    fn collection(&mut self, ids: Vec<usize>) -> Result<Value, String> {
        if self.collections.len() >= 1024 {
            return Err("DOM collection limit exceeded".into());
        }
        self.charge(ids.len() * std::mem::size_of::<usize>())?;
        let id = self.collections.len();
        self.collections.push(ids);
        Ok(Value::Host(format!("collection:{id}")))
    }
    fn attr(&mut self, id: usize, name: &str, value: String) -> Result<(), String> {
        if !valid_name(name) || value.len() > 16_384 {
            return Err("Invalid or oversized attribute".into());
        }
        self.charge(name.len() + value.len())?;
        let attrs = &mut self.document.nodes[id].attributes;
        if let Some(pair) = attrs.iter_mut().find(|(n, _)| n == name) {
            pair.1 = value;
        } else {
            if attrs.len() >= 256 {
                return Err("Attribute count limit exceeded".into());
            }
            attrs.push((name.into(), value));
        }
        Ok(())
    }
    fn new_node(&mut self, tag: String, text: String) -> Result<usize, String> {
        if self.document.nodes.len() >= MAX_NODES {
            return Err("DOM node limit exceeded".into());
        }
        self.charge(tag.len() + text.len() + 128)?;
        let id = self.document.nodes.len();
        self.document.nodes.push(Node {
            tag,
            text,
            attributes: vec![],
            parent: id,
            children: vec![],
        });
        Ok(id)
    }
    fn append(&mut self, parent: usize, child: usize) -> Result<(), String> {
        if child == 0
            || self.document.nodes[parent].tag == "#text"
            || void(&self.document.nodes[parent].tag)
        {
            return Err("Invalid DOM append target".into());
        }
        let mut ancestor = parent;
        let mut parent_depth = 0;
        for depth in 0..=256 {
            if ancestor == child {
                return Err("DOM append would create a cycle".into());
            }
            let next = self.document.nodes[ancestor].parent;
            if ancestor == 0 || next == ancestor {
                break;
            }
            if depth == 256 {
                return Err("DOM depth limit exceeded".into());
            }
            ancestor = next;
            parent_depth = depth + 1;
        }
        let mut pending = vec![(child, 1)];
        let mut visited = 0;
        while let Some((id, depth)) = pending.pop() {
            visited += 1;
            if visited > MAX_NODES || parent_depth + depth > 256 {
                return Err("DOM subtree depth limit exceeded".into());
            }
            pending.extend(
                self.document.nodes[id]
                    .children
                    .iter()
                    .map(|i| (*i, depth + 1)),
            );
        }
        let old = self.document.nodes[child].parent;
        if old != child {
            self.document.nodes[old].children.retain(|i| *i != child);
        }
        self.document.nodes[child].parent = parent;
        self.document.nodes[parent].children.push(child);
        Ok(())
    }
    fn replace_text(&mut self, id: usize, text: String) -> Result<(), String> {
        if id == 0 || void(&self.document.nodes[id].tag) {
            return Err("Unsupported textContent target".into());
        }
        if self.document.nodes[id].tag == "#text" {
            let parent = self.document.nodes[id].parent;
            if matches!(self.document.nodes[parent].tag.as_str(), "script" | "style") {
                return Err("Changing script/style text is not implemented".into());
            }
            self.charge(text.len())?;
            self.document.nodes[id].text = text;
            return Ok(());
        }
        if matches!(self.document.nodes[id].tag.as_str(), "script" | "style") {
            return Err("Changing script/style text is not implemented".into());
        }
        let child = self.new_node("#text".into(), text)?;
        for old in std::mem::take(&mut self.document.nodes[id].children) {
            self.document.nodes[old].parent = old;
        }
        self.append(id, child)
    }
    fn replace_html(&mut self, id: usize, source: String) -> Result<(), String> {
        if id == 0
            || void(&self.document.nodes[id].tag)
            || matches!(
                self.document.nodes[id].tag.as_str(),
                "#text" | "script" | "style"
            )
        {
            return Err("Unsupported innerHTML target".into());
        }
        if source.len() > MAX_SOURCE {
            return Err("innerHTML source limit exceeded".into());
        }
        let fragment = document::parse_with_scripting(&source, self.url.as_str(), true);
        if self.document.nodes.len() + fragment.nodes.len() > MAX_NODES {
            return Err("DOM node limit exceeded".into());
        }
        let mut parent = id;
        let mut parent_depth = 0;
        while parent != 0 && self.document.nodes[parent].parent != parent {
            parent_depth += 1;
            if parent_depth > 256 {
                return Err("DOM depth limit exceeded".into());
            }
            parent = self.document.nodes[parent].parent;
        }
        let mut pending: Vec<_> = fragment.nodes[0].children.iter().map(|i| (*i, 1)).collect();
        while let Some((node, depth)) = pending.pop() {
            if parent_depth + depth > 256 {
                return Err("DOM subtree depth limit exceeded".into());
            }
            pending.extend(
                fragment.nodes[node]
                    .children
                    .iter()
                    .map(|i| (*i, depth + 1)),
            );
        }
        self.charge(source.len() + fragment.nodes.len() * 128)?;
        let offset = self.document.nodes.len() - 1;
        let children: Vec<_> = fragment.nodes[0]
            .children
            .iter()
            .map(|i| i + offset)
            .collect();
        for mut node in fragment.nodes.into_iter().skip(1) {
            node.parent = if node.parent == 0 {
                id
            } else {
                node.parent + offset
            };
            for child in &mut node.children {
                *child += offset;
            }
            self.document.nodes.push(node);
        }
        for old in std::mem::replace(&mut self.document.nodes[id].children, children) {
            self.document.nodes[old].parent = old;
        }
        Ok(())
    }
    fn serialize(&self, root: usize, include_root: bool) -> Result<String, String> {
        let mut output = String::new();
        let mut pending = if include_root {
            vec![(root, false, 0)]
        } else {
            self.document.nodes[root]
                .children
                .iter()
                .rev()
                .map(|i| (*i, false, 0))
                .collect()
        };
        let mut visited = 0;
        while let Some((id, close, depth)) = pending.pop() {
            visited += 1;
            if depth > 256 || visited > MAX_NODES * 2 {
                return Err("Serialized DOM traversal limit exceeded".into());
            }
            let node = &self.document.nodes[id];
            if close {
                output.push_str(&format!("</{}>", node.tag));
            } else if node.tag == "#text" {
                if matches!(
                    self.document.nodes[node.parent].tag.as_str(),
                    "script" | "style"
                ) {
                    output.push_str(&node.text);
                } else {
                    output.push_str(&escape(&node.text));
                }
            } else {
                if node.tag != "#document" {
                    output.push('<');
                    output.push_str(&node.tag);
                    for (name, value) in &node.attributes {
                        output.push_str(&format!(" {}=\"{}\"", name, escape(value)));
                    }
                    output.push('>');
                    if !void(&node.tag) {
                        pending.push((id, true, depth));
                    }
                }
                pending.extend(node.children.iter().rev().map(|i| (*i, false, depth + 1)));
            }
            if output.len() > MAX_SOURCE * 2 {
                return Err("Serialized DOM byte limit exceeded".into());
            }
        }
        Ok(output)
    }
    fn navigate(&mut self, target: &str) -> Result<(), String> {
        if target.len() > 16_384 {
            return Err("Script navigation URL too long".into());
        }
        let url = self.url.join(target).map_err(|e| e.to_string())?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Only credential-free HTTP(S) script navigation is supported".into());
        }
        self.navigation = Some(url.to_string());
        Ok(())
    }
}

impl Host for BrowserHost {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        if object == "navigator" {
            return Ok(match key {
                "userAgent" => Value::text(crate::net::USER_AGENT),
                "language" => Value::text("en-US"),
                "platform" => Value::text("Linux"),
                _ => Value::Undefined,
            });
        }
        if object == "console" {
            return Ok(match key {
                "log" | "warn" | "error" | "info" => Value::Native("host.console.log".into()),
                _ => Value::Undefined,
            });
        }
        if object == "location" {
            return Ok(match key {
                "href" => Value::text(self.url.as_str()),
                "origin" => Value::text(&self.url.origin().ascii_serialization()),
                "protocol" => Value::text(&format!("{}:", self.url.scheme())),
                "host" => {
                    Value::text(&self.url[url::Position::BeforeHost..url::Position::AfterPort])
                }
                "hostname" => Value::text(self.url.host_str().unwrap_or("")),
                "pathname" => Value::text(self.url.path()),
                "search" => Value::text(
                    &self
                        .url
                        .query()
                        .map(|s| format!("?{s}"))
                        .unwrap_or_default(),
                ),
                "hash" => Value::text(
                    &self
                        .url
                        .fragment()
                        .map(|s| format!("#{s}"))
                        .unwrap_or_default(),
                ),
                "assign" | "replace" | "toString" => Value::Native(format!("host.location.{key}")),
                _ => Value::Undefined,
            });
        }
        if let Some(event) = object.strip_prefix("event:") {
            return Ok(match key {
                "type" => Value::text(event),
                "target" | "currentTarget" => Value::Host("document".into()),
                _ => Value::Undefined,
            });
        }
        if let Some(id) = object
            .strip_prefix("collection:")
            .and_then(|s| s.parse::<usize>().ok())
        {
            let list = self.collections.get(id).ok_or("Stale DOM collection")?;
            return Ok(if key == "length" {
                Value::Number(list.len() as f64)
            } else if key == "item" {
                Value::Native("host.collection.item".into())
            } else {
                key.parse::<usize>()
                    .ok()
                    .and_then(|i| list.get(i))
                    .map(|i| self.element(*i))
                    .unwrap_or(Value::Undefined)
            });
        }
        if let Some(id) = object
            .strip_prefix("style:")
            .and_then(|s| s.parse::<usize>().ok())
        {
            let style = self
                .document
                .nodes
                .get(id)
                .ok_or("Stale style handle")?
                .attr("style")
                .unwrap_or("");
            let value = if key == "cssText" {
                style
            } else {
                style
                    .split(';')
                    .filter_map(|v| v.split_once(':'))
                    .find(|(n, _)| n.trim() == key)
                    .map(|(_, v)| v.trim())
                    .unwrap_or("")
            };
            return Ok(Value::text(value));
        }
        let id = self.node(object)?;
        if object == "document" {
            match key {
                "body" => return Ok(self.find_tag("body")),
                "head" => return Ok(self.find_tag("head")),
                "documentElement" => return Ok(self.find_tag("html")),
                "URL" => return Ok(Value::text(self.url.as_str())),
                "baseURI" => return Ok(Value::text(self.base_url().as_str())),
                "location" => return Ok(Value::Host("location".into())),
                "readyState" => return Ok(Value::text(self.ready)),
                "title" => {
                    return Ok(self
                        .find_connected(|n| n.tag == "title")
                        .map(|i| Value::text(&text_content(&self.document.nodes, i)))
                        .unwrap_or(Value::text("")));
                }
                "cookie" => return Err("document.cookie script access is not implemented".into()),
                "getElementById"
                | "querySelector"
                | "querySelectorAll"
                | "getElementsByTagName"
                | "createElement"
                | "createTextNode"
                | "addEventListener" => return Ok(Value::Native(format!("host.dom.{key}"))),
                _ => {}
            }
        }
        let node = &self.document.nodes[id];
        Ok(match key {
            "nodeType" => Value::Number(if id == 0 {
                9.
            } else if node.tag == "#text" {
                3.
            } else {
                1.
            }),
            "nodeName" | "tagName" => Value::text(&node.tag.to_ascii_uppercase()),
            "textContent" | "innerText" => Value::text(&text_content(&self.document.nodes, id)),
            "innerHTML" => Value::text(&self.serialize(id, false)?),
            "id" | "className" | "name" | "type" | "method" => Value::text(
                node.attr(if key == "className" { "class" } else { key })
                    .unwrap_or(""),
            ),
            "value" => Value::text(&if node.tag == "textarea" {
                text_content(&self.document.nodes, id)
            } else {
                node.attr("value").unwrap_or("").into()
            }),
            "href" | "src" | "action" => Value::text(
                &node
                    .attr(key)
                    .and_then(|v| self.base_url().join(v).ok())
                    .map(|u| u.to_string())
                    .unwrap_or_default(),
            ),
            "style" => Value::Host(format!("style:{id}")),
            "parentNode" => {
                if id == 0 || node.parent == id {
                    Value::Null
                } else {
                    self.element(node.parent)
                }
            }
            "firstChild" => node
                .children
                .first()
                .map(|i| self.element(*i))
                .unwrap_or(Value::Null),
            "children" | "childNodes" => {
                let nodes = node
                    .children
                    .iter()
                    .filter(|i| key == "childNodes" || self.document.nodes[**i].tag != "#text")
                    .copied()
                    .collect();
                return self.collection(nodes);
            }
            "appendChild"
            | "removeChild"
            | "setAttribute"
            | "getAttribute"
            | "removeAttribute"
            | "querySelector"
            | "querySelectorAll"
            | "getElementsByTagName"
            | "addEventListener" => Value::Native(format!("host.dom.{key}")),
            _ => Value::Undefined,
        })
    }
    fn set(&mut self, object: &str, key: &str, value: Value) -> Result<(), String> {
        let text = value.as_text();
        if object == "location" {
            return if key == "href" {
                self.navigate(&text)
            } else {
                Err(format!("Setting location.{key} is not implemented"))
            };
        }
        if let Some(id) = object
            .strip_prefix("style:")
            .and_then(|s| s.parse::<usize>().ok())
        {
            if !valid_name(key) {
                return Err("Unsupported CSS property name".into());
            }
            let old = self
                .document
                .nodes
                .get(id)
                .ok_or("Stale style handle")?
                .attr("style")
                .unwrap_or("");
            let style = if key == "cssText" {
                text
            } else {
                let mut declarations: Vec<_> = old
                    .split(';')
                    .filter(|part| part.split_once(':').is_some_and(|(n, _)| n.trim() != key))
                    .map(str::to_string)
                    .collect();
                declarations.push(format!("{key}:{text}"));
                declarations.join(";")
            };
            return self.attr(id, "style", style);
        }
        let id = self.node(object)?;
        if object == "document" {
            if key == "location" {
                return self.navigate(&text);
            }
            if key == "title" {
                let title = if let Some(id) = self.find_connected(|n| n.tag == "title") {
                    id
                } else {
                    let id = self.new_node("title".into(), String::new())?;
                    let parent = self.find_connected(|n| n.tag == "head").unwrap_or(0);
                    self.append(parent, id)?;
                    id
                };
                return self.replace_text(title, text);
            }
        }
        match key {
            "textContent" | "innerText" => self.replace_text(id, text),
            "innerHTML" => self.replace_html(id, text),
            "value" if self.document.nodes[id].tag == "textarea" => self.replace_text(id, text),
            "id" | "className" | "name" | "type" | "value" | "href" | "src" | "action"
            | "method" => self.attr(id, if key == "className" { "class" } else { key }, text),
            _ => Err(format!("Setting DOM property {key} is not implemented")),
        }
    }
    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String> {
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        let text = first.as_text();
        if name == "host.console.log" {
            return Ok(Value::Undefined);
        }
        if name == "host.location.toString" {
            return Ok(Value::text(self.url.as_str()));
        }
        if matches!(name, "host.location.assign" | "host.location.replace") {
            self.navigate(&text)?;
            return Ok(Value::Undefined);
        }
        if name == "host.window.addEventListener" || name == "host.dom.addEventListener" {
            if !matches!(text.as_str(), "DOMContentLoaded" | "load") {
                return Err(format!("Event {text} is not implemented yet"));
            }
            let callback = args
                .get(1)
                .cloned()
                .ok_or("addEventListener needs a callback")?;
            if !matches!(callback, Value::Function(_) | Value::Native(_)) {
                return Err("Listener must be a function".into());
            }
            if self.listeners.len() >= 32 {
                return Err("Listener limit exceeded".into());
            }
            self.listeners.push((text, callback, this));
            return Ok(Value::Undefined);
        }
        let Value::Host(handle) = this else {
            return Err("DOM method requires its receiver".into());
        };
        if name == "host.collection.item" {
            let id = handle
                .strip_prefix("collection:")
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or("Expected DOM collection")?;
            return Ok(text
                .parse::<usize>()
                .ok()
                .and_then(|i| self.collections.get(id)?.get(i))
                .map(|i| self.element(*i))
                .unwrap_or(Value::Null));
        }
        let root = self.node(&handle)?;
        match name {
            "host.dom.getElementById" => Ok(self
                .find_connected(|n| n.attr("id") == Some(&text))
                .map(|i| self.element(i))
                .unwrap_or(Value::Null)),
            "host.dom.querySelector" => Ok(self
                .document
                .query_selector(root, &text)?
                .map(|i| self.element(i))
                .unwrap_or(Value::Null)),
            "host.dom.querySelectorAll" => {
                let ids = self.document.query_selector_all(root, &text)?;
                self.collection(ids)
            }
            "host.dom.getElementsByTagName" => {
                if text != "*" && !valid_name(&text) {
                    return self.collection(vec![]);
                }
                let ids = self
                    .document
                    .query_selector_all(root, &text.to_ascii_lowercase())?;
                self.collection(ids)
            }
            "host.dom.createElement" => {
                if !valid_name(&text) || !text.as_bytes()[0].is_ascii_alphabetic() {
                    return Err("Invalid element name".into());
                }
                let id = self.new_node(text.to_ascii_lowercase(), String::new())?;
                Ok(self.element(id))
            }
            "host.dom.createTextNode" => {
                let id = self.new_node("#text".into(), text)?;
                Ok(self.element(id))
            }
            "host.dom.appendChild" | "host.dom.removeChild" => {
                let Value::Host(child) = first else {
                    return Err("Expected a DOM node".into());
                };
                let child = self.node(&child)?;
                if name.ends_with("appendChild") {
                    self.append(root, child)?;
                } else {
                    if !self.document.nodes[root].children.contains(&child) {
                        return Err("Node is not a child".into());
                    }
                    self.document.nodes[root].children.retain(|i| *i != child);
                    self.document.nodes[child].parent = child;
                }
                Ok(self.element(child))
            }
            "host.dom.setAttribute" => {
                self.attr(
                    root,
                    &text.to_ascii_lowercase(),
                    args.get(1).cloned().unwrap_or(Value::Undefined).as_text(),
                )?;
                Ok(Value::Undefined)
            }
            "host.dom.getAttribute" => Ok(self.document.nodes[root]
                .attr(&text.to_ascii_lowercase())
                .map(Value::text)
                .unwrap_or(Value::Null)),
            "host.dom.removeAttribute" => {
                self.document.nodes[root]
                    .attributes
                    .retain(|(n, _)| n != &text.to_ascii_lowercase());
                Ok(Value::Undefined)
            }
            _ => Err(format!("Browser method {name} is not implemented")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(script: &str) -> Reply {
        execute(Request {
            url: "http://localhost/fixture".into(),
            html: format!(
                "<html><head><title>Before</title></head><body><p id=output>Old</p><script>{script}</script></body></html>"
            ),
        })
    }
    #[test]
    fn real_script_mutates_dom_and_creates_a_form() {
        let reply = page(
            "document.title='Script ready';var f=document.createElement('form');f.action='/search';var q=document.createElement('input');q.name='q';q.value='Rust & café';f.appendChild(q);document.body.appendChild(f);document.getElementById('output').textContent='Changed';",
        );
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        let doc = document::parse_with_scripting(&reply.html, "http://localhost/fixture", true);
        assert_eq!(doc.title, "Script ready");
        assert!(doc.items.iter().any(
            |i| matches!(i,document::Item::Input{name,value,..} if name=="q"&&value=="Rust & café")
        ));
        assert_eq!(doc.forms[0].action, "http://localhost/search");
    }
    #[test]
    fn scripts_share_a_realm_and_load_callbacks_run() {
        let reply=execute(Request{url:"https://example.org/".into(),html:"<html><head><title>old</title></head><body><script>var word='shared';</script><script>document.addEventListener('DOMContentLoaded',function(){document.title=word;});</script></body></html>".into()});
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(
            document::parse(&reply.html, "https://example.org/").title,
            "shared"
        );
        assert_eq!(reply.scripts_executed, 2);
    }
    #[test]
    fn navigation_is_a_validated_capability_not_direct_network() {
        for statement in [
            "location.href='/next?q=rust'",
            "location.replace('/next?q=rust')",
            "window.location='/next?q=rust'",
        ] {
            let reply = page(statement);
            assert!(reply.errors.is_empty(), "{:?}", reply.errors);
            assert_eq!(
                reply.navigation.as_deref(),
                Some("http://localhost/next?q=rust")
            );
        }
        assert!(
            page("location.href='file:///etc/passwd'")
                .navigation
                .is_none()
        );
    }
    #[test]
    fn noscript_effects_are_disabled_only_in_scripting_projection() {
        let source = "<noscript><meta http-equiv=refresh content='0;url=/disabled'><p>No JS</p></noscript><p>Page</p>";
        assert!(
            document::parse(source, "https://example.org/")
                .refresh
                .is_some()
        );
        let scripted = document::parse_with_scripting(source, "https://example.org/", true);
        assert!(scripted.refresh.is_none());
        assert!(
            !scripted
                .items
                .iter()
                .any(|i| matches!(i,document::Item::Text{text,..} if text.contains("No JS")))
        );
        assert_eq!(
            document::parse_with_scripting(
                "<meta http-equiv=refresh content='0;url=/normal'>",
                "https://example.org/",
                true
            )
            .refresh
            .as_deref(),
            Some("https://example.org/normal")
        );
    }
    #[test]
    fn unsupported_features_errors_and_dom_cycles_are_visible() {
        assert!(
            !page("document.body.appendChild(document.body)")
                .errors
                .is_empty()
        );
        assert!(!page("fetch('/data')").errors.is_empty());
        assert!(!page("while(true){}").errors.is_empty());
        let reply = page(
            "document.getElementById('output').innerHTML='<a href=\"/next\">Rust &amp; research</a>';",
        );
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert!(document::parse(&reply.html,"http://localhost/fixture").items.iter().any(|i|matches!(i,document::Item::Text{text,href:Some(h),..} if text.contains("Rust & research")&&h=="http://localhost/next")));
    }
    #[test]
    fn rejected_projection_preserves_source_and_discards_navigation() {
        let html = format!(
            "<body>{}<script>location.href='/next';</script></body>",
            "\"".repeat(400_000)
        );
        let reply = execute(Request {
            url: "https://example.test/".into(),
            html: html.clone(),
        });
        assert!(!reply.applied);
        assert_eq!(reply.html, html);
        assert!(reply.navigation.is_none());
        assert!(
            reply
                .errors
                .iter()
                .any(|e| e.contains("Serialized DOM byte limit")),
            "{:?}",
            reply.errors
        );
        let reply = execute(Request {
            url: "https://example.test/".into(),
            html: "a".repeat(MAX_SOURCE + 1),
        });
        assert!(!reply.applied);
        assert_eq!(reply.scripts_executed, 0);
    }
}
