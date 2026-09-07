//! A small, bounded HTML tokenizer/tree builder and readable document projection.
//!
//! This is our initial HTML subset, not the HTML Living Standard tree builder.
//! It deliberately executes no script. DOM inspection supports a documented
//! selector subset; this is separate from a future CSS cascade.

use std::collections::HashMap;

const MAX_SOURCE: usize = 8 * 1024 * 1024;
const MAX_NODES: usize = 50_000;
const MAX_DEPTH: usize = 256;
const MAX_ATTRIBUTE: usize = 16_384;
const MAX_FORMS: usize = 256;
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub title: String,
    pub items: Vec<Item>,
    /// Indices into the actual parser arena, aligned one-to-one with `items`.
    /// Linked text maps to its anchor; merged ordinary text maps to the lowest
    /// common ancestor of its source nodes. Breaks map to their block element.
    pub item_nodes: Vec<usize>,
    /// The actual bounded parser arena. Node zero is the document root.
    pub nodes: Vec<Node>,
    pub forms: Vec<Form>,
    pub base_url: String,
    /// A standard zero-delay HTML meta refresh. Navigation caps belong to the
    /// browser; scripts and non-HTTP refresh destinations are never executed.
    pub refresh: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Text {
        text: String,
        href: Option<String>,
        heading: bool,
    },
    Break,
    Input {
        form: usize,
        name: String,
        value: String,
        kind: String,
    },
    Submit {
        form: usize,
        name: String,
        value: String,
        label: String,
    },
    Image {
        alt: String,
        src: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub action: String,
    pub method: String,
    /// Hidden/default successful controls. Editable values and the activated
    /// submit button are merged by the browser at submission time.
    pub fields: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub tag: String,
    pub attributes: Vec<(String, String)>,
    pub text: String,
    pub parent: usize,
    pub children: Vec<usize>,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        self.attr(name).is_some()
    }
}

impl Document {
    /// Return the first matching descendant in tree order. The root itself is
    /// excluded, as with DOM Element.querySelector. Matching may use ancestors
    /// outside the root's subtree, as ordinary descendant selectors do.
    ///
    /// Supported: tag names, `*`, IDs, classes, attribute presence/equality,
    /// compound combinations, and the whitespace descendant combinator.
    /// Other combinators, selector lists, pseudos, escapes, namespaces, and
    /// attribute operators/flags return an explicit error.
    pub fn query_selector(&self, root: usize, selector: &str) -> Result<Option<usize>, String> {
        Ok(self.query(root, selector, true)?.into_iter().next())
    }

    /// Return every matching descendant in tree order, with no duplicates.
    pub fn query_selector_all(&self, root: usize, selector: &str) -> Result<Vec<usize>, String> {
        self.query(root, selector, false)
    }

    fn query(&self, root: usize, selector: &str, first: bool) -> Result<Vec<usize>, String> {
        let parts = SelectorParser::parse(selector)?;
        let node = self
            .nodes
            .get(root)
            .ok_or("DOM root index is out of range")?;
        let mut pending: Vec<_> = node.children.iter().rev().copied().collect();
        let mut visited = vec![false; self.nodes.len()];
        visited[root] = true;
        let mut result = Vec::new();
        let mut budget = 4_000_000;
        while let Some(id) = pending.pop() {
            let node = self
                .nodes
                .get(id)
                .ok_or("DOM child index is out of range")?;
            if visited[id] {
                return Err("DOM contains a cycle or repeated child".to_owned());
            }
            visited[id] = true;
            if self.matches_selector(id, &parts, &mut budget)? {
                result.push(id);
                if first {
                    return Ok(result);
                }
            }
            pending.extend(node.children.iter().rev().copied());
        }
        Ok(result)
    }

    fn matches_selector(
        &self,
        id: usize,
        parts: &[SelectorPart],
        budget: &mut usize,
    ) -> Result<bool, String> {
        let mut remaining = parts.iter().rev();
        let last = remaining.next().ok_or("Empty selector")?;
        spend_selector_step(budget)?;
        if !last.matches(&self.nodes[id]) {
            return Ok(false);
        }
        let mut id = id;
        for part in remaining {
            loop {
                spend_selector_step(budget)?;
                if id == 0 {
                    return Ok(false);
                }
                id = self.nodes[id].parent;
                let node = self
                    .nodes
                    .get(id)
                    .ok_or("DOM parent index is out of range")?;
                if part.matches(node) {
                    break;
                }
            }
        }
        Ok(true)
    }
}

fn spend_selector_step(budget: &mut usize) -> Result<(), String> {
    *budget = budget
        .checked_sub(1)
        .ok_or("Selector traversal budget exceeded")?;
    Ok(())
}

#[derive(Debug)]
struct SelectorPart {
    tag: Option<String>,
    conditions: Vec<SelectorCondition>,
}

#[derive(Debug)]
enum SelectorCondition {
    Id(String),
    Class(String),
    Attribute(String, Option<String>),
}

impl SelectorPart {
    fn matches(&self, node: &Node) -> bool {
        if node.tag.starts_with('#') || self.tag.as_ref().is_some_and(|tag| *tag != node.tag) {
            return false;
        }
        self.conditions.iter().all(|condition| match condition {
            SelectorCondition::Id(value) => node.attr("id") == Some(value.as_str()),
            SelectorCondition::Class(value) => node.attr("class").is_some_and(|classes| {
                classes.split_ascii_whitespace().any(|class| class == value)
            }),
            SelectorCondition::Attribute(name, None) => node.has(name),
            SelectorCondition::Attribute(name, Some(value)) => {
                node.attr(name) == Some(value.as_str())
            }
        })
    }
}

struct SelectorParser<'a> {
    source: &'a str,
    pos: usize,
}

impl<'a> SelectorParser<'a> {
    fn parse(source: &'a str) -> Result<Vec<SelectorPart>, String> {
        if source.len() > 4096 {
            return Err("Selector exceeds 4096 bytes".to_owned());
        }
        let mut parser = Self { source, pos: 0 };
        let mut result = Vec::new();
        parser.whitespace();
        while parser.peek().is_some() {
            if result.len() == 32 {
                return Err("Selector exceeds 32 descendant components".to_owned());
            }
            result.push(parser.compound()?);
            parser.whitespace();
        }
        if result.is_empty() {
            return Err("Selector must not be empty".to_owned());
        }
        Ok(result)
    }

    fn compound(&mut self) -> Result<SelectorPart, String> {
        let mut part = SelectorPart {
            tag: None,
            conditions: Vec::new(),
        };
        let mut consumed = false;
        if self.peek() == Some('*') {
            self.pos += 1;
            consumed = true;
        } else if self.peek().is_some_and(identifier_start) {
            part.tag = Some(self.identifier()?.to_ascii_lowercase());
            consumed = true;
        }
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() {
                break;
            }
            match ch {
                '#' | '.' => {
                    self.pos += 1;
                    let value = self.identifier()?;
                    part.conditions.push(if ch == '#' {
                        SelectorCondition::Id(value)
                    } else {
                        SelectorCondition::Class(value)
                    });
                }
                '[' => {
                    self.pos += 1;
                    self.whitespace();
                    let name = self.identifier()?.to_ascii_lowercase();
                    self.whitespace();
                    let value = if self.peek() == Some('=') {
                        self.pos += 1;
                        self.whitespace();
                        let value = match self.peek() {
                            Some(quote @ ('\'' | '"')) => {
                                self.pos += 1;
                                let begin = self.pos;
                                while self.peek().is_some_and(|ch| {
                                    ch != quote && ch != '\\' && ch != '\n' && ch != '\r'
                                }) {
                                    self.pos += self.peek().unwrap().len_utf8();
                                }
                                if self.peek() != Some(quote) {
                                    return Err(self.error());
                                }
                                let value = self.source[begin..self.pos].to_owned();
                                self.pos += 1;
                                value
                            }
                            _ => self.identifier()?,
                        };
                        self.whitespace();
                        Some(value)
                    } else {
                        None
                    };
                    if self.peek() != Some(']') {
                        return Err(self.error());
                    }
                    self.pos += 1;
                    part.conditions
                        .push(SelectorCondition::Attribute(name, value));
                }
                _ => return Err(self.error()),
            }
            consumed = true;
        }
        if !consumed {
            return Err(self.error());
        }
        Ok(part)
    }

    fn identifier(&mut self) -> Result<String, String> {
        let begin = self.pos;
        if !self.peek().is_some_and(identifier_start) {
            return Err(self.error());
        }
        while let Some(ch) = self.peek() {
            if !identifier_start(ch) && !ch.is_ascii_digit() {
                break;
            }
            self.pos += ch.len_utf8();
        }
        let value = &self.source[begin..self.pos];
        if value == "-"
            || value.starts_with('-') && value.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
        {
            return Err(self.error());
        }
        Ok(value.to_owned())
    }

    fn whitespace(&mut self) {
        while self.peek().is_some_and(|ch| ch.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn error(&self) -> String {
        format!(
            "Unsupported or invalid selector syntax at byte {}",
            self.pos
        )
    }
}

fn identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_' || ch == '-' || !ch.is_ascii()
}

/// Parse a bounded document. Oversized input, excessive nodes, or excessive
/// nesting produce a readable prefix; malformed input never requires recursion
/// deeper than MAX_DEPTH. Network byte limits remain a separate layer.
pub fn parse(html: &str, page_url: &str) -> Document {
    let nodes = build_tree(html);
    let base_url = nodes
        .iter()
        .filter(|node| node.tag == "base")
        .find_map(|node| resolve(page_url, node.attr("href")?))
        .unwrap_or_else(|| page_url.to_owned());
    let title = nodes
        .iter()
        .position(|node| node.tag == "title")
        .map(|id| collapse(&descendant_text(&nodes, id)).trim().to_owned())
        .unwrap_or_default();
    let mut document = Document {
        title,
        items: Vec::new(),
        item_nodes: Vec::new(),
        nodes: Vec::new(),
        forms: Vec::new(),
        base_url,
        refresh: nodes.iter().find_map(|node| {
            if node.tag != "meta" || !node.attr("http-equiv")?.eq_ignore_ascii_case("refresh") {
                return None;
            }
            let mut ancestor = node.parent;
            while ancestor != 0 {
                if nodes[ancestor].tag == "template" {
                    return None;
                }
                ancestor = nodes[ancestor].parent;
            }
            meta_refresh(node.attr("content")?, page_url)
        }),
    };
    let mut form_nodes = HashMap::new();
    let mut form_ids = HashMap::new();
    for (id, node) in nodes.iter().enumerate() {
        if node.tag != "form" || document.forms.len() == MAX_FORMS {
            continue;
        }
        let form = document.forms.len();
        let action = node.attr("action").unwrap_or("");
        let action = if action.trim().is_empty() {
            page_url.to_owned()
        } else {
            resolve(&document.base_url, action).unwrap_or_else(|| action.to_owned())
        };
        document.forms.push(Form {
            action,
            method: if node
                .attr("method")
                .is_some_and(|s| s.eq_ignore_ascii_case("post"))
            {
                "post".to_owned()
            } else {
                "get".to_owned()
            },
            fields: Vec::new(),
        });
        form_nodes.insert(id, form);
        if let Some(name) = node.attr("id") {
            form_ids.entry(name).or_insert(form);
        }
    }
    let mut output = Projection {
        nodes: &nodes,
        document,
        form_nodes,
        form_ids,
        bytes: 0,
    };
    output.walk(0, None, false, false, false);
    while matches!(output.document.items.last(), Some(Item::Break)) {
        output.document.items.pop();
        output.document.item_nodes.pop();
    }
    let mut document = output.document;
    document.nodes = nodes;
    document
}

fn resolve(base: &str, reference: &str) -> Option<String> {
    let url = url::Url::parse(base)
        .and_then(|base| base.join(reference.trim()))
        .or_else(|_| url::Url::parse(reference.trim()))
        .ok()?;
    if matches!(url.scheme(), "javascript" | "vbscript") {
        None
    } else {
        Some(url.into())
    }
}

fn meta_refresh(content: &str, page_url: &str) -> Option<String> {
    let (delay, destination) = content.split_once(';')?;
    if delay.trim().parse::<f64>().ok()? != 0.0 {
        return None;
    }
    let (key, destination) = destination.trim().split_once('=')?;
    if !key.trim().eq_ignore_ascii_case("url") {
        return None;
    }
    let destination = destination.trim();
    let destination = if destination.starts_with('"') && destination.ends_with('"')
        || destination.starts_with('\'') && destination.ends_with('\'')
    {
        destination.get(1..destination.len().checked_sub(1)?)?
    } else {
        destination
    };
    let resolved = resolve(page_url, destination)?;
    matches!(url::Url::parse(&resolved).ok()?.scheme(), "http" | "https").then_some(resolved)
}

fn build_tree(source: &str) -> Vec<Node> {
    let mut end = source.len().min(MAX_SOURCE);
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let source = &source[..end];
    let mut nodes = vec![Node {
        tag: "#document".to_owned(),
        attributes: Vec::new(),
        text: String::new(),
        parent: 0,
        children: Vec::new(),
    }];
    let mut stack = vec![0];
    let mut pos = 0;
    while pos < source.len() && nodes.len() < MAX_NODES {
        if !source[pos..].starts_with('<') {
            let end = source[pos..].find('<').map_or(source.len(), |n| pos + n);
            add_text(
                &mut nodes,
                *stack.last().unwrap(),
                decode_entities(&source[pos..end]),
            );
            pos = end;
            continue;
        }
        if source[pos..].starts_with("<!--") {
            pos = source[pos + 4..]
                .find("-->")
                .map_or(source.len(), |n| pos + 4 + n + 3);
            continue;
        }
        let Some(end) = tag_end(source, pos + 1) else {
            add_text(
                &mut nodes,
                *stack.last().unwrap(),
                decode_entities(&source[pos..]),
            );
            break;
        };
        let inside = &source[pos + 1..end];
        pos = end + 1;
        if inside.starts_with(['!', '?']) {
            continue;
        }
        let closing = inside.starts_with('/');
        let inside = if closing { &inside[1..] } else { inside };
        let name_end = inside
            .bytes()
            .position(|byte| !byte.is_ascii_alphanumeric() && byte != b'-' && byte != b':')
            .unwrap_or(inside.len());
        if name_end == 0 || !inside.as_bytes()[0].is_ascii_alphabetic() {
            add_text(&mut nodes, *stack.last().unwrap(), format!("<{}>", inside));
            continue;
        }
        let tag = inside[..name_end].to_ascii_lowercase();
        if closing {
            if let Some(index) = stack.iter().rposition(|id| nodes[*id].tag == tag) {
                stack.truncate(index.max(1));
            }
            continue;
        }
        // A few common optional-end-tag rules keep ordinary malformed pages
        // readable. This does not implement foster parenting or adoption agency.
        if tag == "li" || tag == "p" || tag == "a" || tag == "button" || tag == "option" {
            if let Some(index) = stack.iter().rposition(|id| nodes[*id].tag == tag) {
                stack.truncate(index.max(1));
            }
        } else if is_block(&tag) && stack.last().is_some_and(|id| nodes[*id].tag == "p") {
            stack.pop();
        }
        if tag == "form" && stack.iter().any(|id| nodes[*id].tag == "form") {
            continue;
        }
        if stack.len() >= MAX_DEPTH {
            break;
        }
        let parent = *stack.last().unwrap();
        let id = nodes.len();
        let attributes = attributes(&inside[name_end..]);
        nodes.push(Node {
            tag: tag.clone(),
            attributes,
            text: String::new(),
            parent,
            children: Vec::new(),
        });
        nodes[parent].children.push(id);
        if matches!(tag.as_str(), "script" | "style" | "textarea" | "title") {
            let end = raw_end(source, pos, &tag).unwrap_or(source.len());
            let text = if tag == "textarea" || tag == "title" {
                decode_entities(&source[pos..end])
            } else {
                source[pos..end].to_owned()
            };
            add_text(&mut nodes, id, text);
            pos = if end < source.len() {
                tag_end(source, end + 2).map_or(source.len(), |n| n + 1)
            } else {
                source.len()
            };
        } else if !is_void(&tag) {
            // HTML self-closing syntax does not close ordinary HTML elements.
            stack.push(id);
        }
    }
    nodes
}

fn add_text(nodes: &mut Vec<Node>, parent: usize, text: String) {
    if text.is_empty() || nodes.len() >= MAX_NODES {
        return;
    }
    let id = nodes.len();
    nodes.push(Node {
        tag: "#text".to_owned(),
        attributes: Vec::new(),
        text,
        parent,
        children: Vec::new(),
    });
    nodes[parent].children.push(id);
}

fn tag_end(source: &str, start: usize) -> Option<usize> {
    let mut quote = 0;
    for (offset, byte) in source.as_bytes()[start..].iter().copied().enumerate() {
        if quote != 0 {
            if byte == quote {
                quote = 0;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = byte;
        } else if byte == b'>' {
            return Some(start + offset);
        }
    }
    None
}

fn raw_end(source: &str, mut start: usize, name: &str) -> Option<usize> {
    while let Some(offset) = source[start..].find("</") {
        let pos = start + offset;
        let begin = pos + 2;
        let end = begin + name.len();
        if source
            .as_bytes()
            .get(begin..end)
            .is_some_and(|s| s.eq_ignore_ascii_case(name.as_bytes()))
            && source
                .as_bytes()
                .get(end)
                .is_some_and(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
        {
            return Some(pos);
        }
        start = begin;
    }
    None
}

fn attributes(source: &str) -> Vec<(String, String)> {
    let bytes = source.as_bytes();
    let mut pos = 0;
    let mut result = Vec::new();
    while pos < bytes.len() {
        while pos < bytes.len() && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b'/') {
            pos += 1;
        }
        let start = pos;
        while pos < bytes.len()
            && !bytes[pos].is_ascii_whitespace()
            && !matches!(bytes[pos], b'=' | b'/')
        {
            pos += 1;
        }
        if start == pos {
            pos += 1;
            continue;
        }
        let key = source[start..pos].to_ascii_lowercase();
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let mut value = "";
        if bytes.get(pos) == Some(&b'=') {
            pos += 1;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if let Some(quote @ (b'\'' | b'"')) = bytes.get(pos).copied() {
                pos += 1;
                let start = pos;
                while pos < bytes.len() && bytes[pos] != quote {
                    pos += 1;
                }
                value = &source[start..pos];
                pos = (pos + 1).min(bytes.len());
            } else {
                let start = pos;
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                value = &source[start..pos];
            }
        }
        if result.len() < 256
            && key.len() <= 256
            && value.len() <= MAX_ATTRIBUTE
            && !result.iter().any(|(old, _)| *old == key)
        {
            result.push((key, decode_entities(value)));
        }
    }
    result
}

fn is_void(tag: &str) -> bool {
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

fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "body"
            | "br"
            | "dd"
            | "details"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "summary"
            | "table"
            | "tr"
            | "ul"
    )
}

fn collapse(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
            if !space {
                result.push(' ');
            }
            space = true;
        } else {
            result.push(ch);
            space = false;
        }
    }
    result
}

fn descendant_text(nodes: &[Node], root: usize) -> String {
    let mut result = String::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if nodes[id].tag == "#text" {
            result.push_str(&nodes[id].text);
        } else if !matches!(nodes[id].tag.as_str(), "script" | "style" | "template") {
            stack.extend(nodes[id].children.iter().rev().copied());
        }
    }
    result
}

struct Projection<'a> {
    nodes: &'a [Node],
    document: Document,
    form_nodes: HashMap<usize, usize>,
    form_ids: HashMap<&'a str, usize>,
    bytes: usize,
}

impl Projection<'_> {
    fn charge(&mut self, size: usize) -> bool {
        self.bytes = self.bytes.saturating_add(size);
        self.bytes <= MAX_OUTPUT
    }

    fn text(&mut self, id: usize, text: &str, href: Option<&str>, heading: bool) {
        let mut text = collapse(text);
        if text.is_empty() || !self.charge(text.len() + href.map_or(0, str::len)) {
            return;
        }
        if let Some(Item::Text {
            text: previous,
            href: previous_href,
            heading: previous_heading,
        }) = self.document.items.last_mut()
        {
            if previous_href.as_deref() == href
                && *previous_heading == heading
                && (href.is_none() || self.document.item_nodes.last() == Some(&id))
            {
                if previous.ends_with(' ') && text.starts_with(' ') {
                    text.remove(0);
                }
                previous.push_str(&text);
                if let Some(previous_id) = self.document.item_nodes.last_mut() {
                    *previous_id = common_ancestor(self.nodes, *previous_id, id);
                }
                return;
            }
        }
        if self.document.items.is_empty() || matches!(self.document.items.last(), Some(Item::Break))
        {
            text = text.trim_start_matches(' ').to_owned();
        }
        if !text.is_empty() {
            self.document.items.push(Item::Text {
                text,
                href: href.map(str::to_owned),
                heading,
            });
            self.document.item_nodes.push(id);
        }
    }

    fn line_break(&mut self, id: usize) {
        if let Some(Item::Text { text, .. }) = self.document.items.last_mut() {
            text.truncate(text.trim_end_matches(' ').len());
        }
        if !self.document.items.is_empty()
            && !matches!(self.document.items.last(), Some(Item::Break))
        {
            self.document.items.push(Item::Break);
            self.document.item_nodes.push(id);
        }
    }

    fn owner(&self, mut id: usize) -> Option<usize> {
        if let Some(name) = self.nodes[id].attr("form") {
            return self.form_ids.get(name).copied();
        }
        while id != 0 {
            if let Some(form) = self.form_nodes.get(&id) {
                return Some(*form);
            }
            id = self.nodes[id].parent;
        }
        None
    }

    fn field(&mut self, form: usize, name: &str, value: &str) {
        if !name.is_empty() && self.charge(name.len() + value.len() + 32) {
            self.document.forms[form]
                .fields
                .push((name.to_owned(), value.to_owned()));
        }
    }

    fn walk(&mut self, id: usize, href: Option<&str>, heading: bool, hidden: bool, disabled: bool) {
        if self.bytes > MAX_OUTPUT {
            return;
        }
        let node = &self.nodes[id];
        if matches!(
            node.tag.as_str(),
            "script" | "style" | "template" | "head" | "title"
        ) {
            return;
        }
        let hidden = hidden
            || node.has("hidden")
            || node
                .attr("aria-hidden")
                .is_some_and(|s| s.eq_ignore_ascii_case("true"))
            || node.attr("style").is_some_and(inline_hidden);
        let disabled = disabled || (node.tag == "fieldset" && node.has("disabled"));
        let heading =
            heading || matches!(node.tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
        let link = if node.tag == "a" {
            node.attr("href")
                .and_then(|s| resolve(&self.document.base_url, s))
        } else {
            None
        };
        let href = if node.tag == "a" {
            link.as_deref()
        } else {
            href
        };
        if node.tag == "#text" {
            if !hidden {
                let mut source = id;
                if href.is_some() {
                    while source != 0 && self.nodes[source].tag != "a" {
                        source = self.nodes[source].parent;
                    }
                }
                self.text(source, &node.text, href, heading);
            }
            return;
        }
        if !hidden && is_block(&node.tag) {
            self.line_break(id);
        }
        if matches!(
            node.tag.as_str(),
            "input" | "textarea" | "button" | "select"
        ) {
            if !disabled && !node.has("disabled") {
                if let Some(form) = self.owner(id) {
                    self.control(id, form, hidden);
                }
            }
            return;
        }
        if node.tag == "img" && !hidden {
            let alt = node.attr("alt").unwrap_or("");
            let src = node
                .attr("src")
                .and_then(|s| resolve(&self.document.base_url, s))
                .unwrap_or_default();
            if self.charge(alt.len() + src.len() + 32) {
                self.document.items.push(Item::Image {
                    alt: alt.to_owned(),
                    src,
                });
                self.document.item_nodes.push(id);
            }
        }
        for child in &node.children {
            self.walk(*child, href, heading, hidden, disabled);
        }
        if !hidden && is_block(&node.tag) {
            self.line_break(id);
        }
    }

    fn control(&mut self, id: usize, form: usize, hidden: bool) {
        let node = &self.nodes[id];
        let name = node.attr("name").unwrap_or("");
        let value = node.attr("value").unwrap_or("");
        let kind = node
            .attr("type")
            .unwrap_or(if node.tag == "button" {
                "submit"
            } else {
                "text"
            })
            .to_ascii_lowercase();
        if node.tag == "select" {
            let mut options = Vec::new();
            let mut stack = node.children.clone();
            stack.reverse();
            while let Some(option) = stack.pop() {
                let option_node = &self.nodes[option];
                if option_node.tag == "option" && !option_node.has("disabled") {
                    options.push(option);
                } else {
                    stack.extend(option_node.children.iter().rev().copied());
                }
            }
            let selected: Vec<_> = options
                .iter()
                .copied()
                .filter(|id| self.nodes[*id].has("selected"))
                .collect();
            let selected = if selected.is_empty() && !node.has("multiple") {
                options.into_iter().take(1).collect()
            } else {
                selected
            };
            for option in selected {
                let option_node = &self.nodes[option];
                let text = collapse(&descendant_text(self.nodes, option));
                self.field(form, name, option_node.attr("value").unwrap_or(text.trim()));
            }
            return;
        }
        if matches!(kind.as_str(), "checkbox" | "radio") {
            if node.has("checked") {
                self.field(form, name, node.attr("value").unwrap_or("on"));
            }
            return;
        }
        if matches!(kind.as_str(), "button" | "reset" | "file") {
            return;
        }
        let value = if node.tag == "textarea" {
            descendant_text(self.nodes, id)
        } else {
            value.to_owned()
        };
        if kind == "hidden" || hidden {
            if !matches!(kind.as_str(), "submit" | "image") {
                self.field(form, name, &value);
            }
            return;
        }
        if kind == "submit" || kind == "image" {
            let label = if node.tag == "button" {
                collapse(&descendant_text(self.nodes, id)).trim().to_owned()
            } else if value.is_empty() {
                node.attr("alt").unwrap_or("Submit").to_owned()
            } else {
                value.clone()
            };
            if self.charge(name.len() + value.len() + label.len() + 32) {
                self.document.items.push(Item::Submit {
                    form,
                    name: name.to_owned(),
                    value,
                    label,
                });
                self.document.item_nodes.push(id);
            }
        } else if self.charge(name.len() + value.len() + kind.len() + 32) {
            self.document.items.push(Item::Input {
                form,
                name: name.to_owned(),
                value,
                kind: if node.tag == "textarea" {
                    "text".to_owned()
                } else {
                    kind
                },
            });
            self.document.item_nodes.push(id);
        }
    }
}

fn common_ancestor(nodes: &[Node], first: usize, second: usize) -> usize {
    let mut ancestors = Vec::new();
    let mut id = first;
    loop {
        ancestors.push(id);
        if id == 0 {
            break;
        }
        id = nodes[id].parent;
    }
    let mut id = second;
    while !ancestors.contains(&id) {
        id = nodes[id].parent;
    }
    id
}

fn inline_hidden(style: &str) -> bool {
    style.split(';').any(|declaration| {
        let Some((property, value)) = declaration.split_once(':') else {
            return false;
        };
        let value = value.split('!').next().unwrap_or("").trim();
        (property.trim().eq_ignore_ascii_case("display") && value.eq_ignore_ascii_case("none"))
            || (property.trim().eq_ignore_ascii_case("visibility")
                && value.eq_ignore_ascii_case("hidden"))
    })
}

fn decode_entities(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(amp) = rest.find('&') {
        output.push_str(&rest[..amp]);
        rest = &rest[amp + 1..];
        let end = rest.bytes().take(32).position(|b| b == b';');
        let Some(end) = end else {
            output.push('&');
            continue;
        };
        let name = &rest[..end];
        let decoded = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X"))
        {
            u32::from_str_radix(hex, 16).ok().map(entity_char)
        } else if let Some(decimal) = name.strip_prefix('#') {
            decimal.parse().ok().map(entity_char)
        } else {
            match name {
                "amp" | "AMP" => Some('&'),
                "lt" | "LT" => Some('<'),
                "gt" | "GT" => Some('>'),
                "quot" | "QUOT" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "copy" => Some('©'),
                "reg" => Some('®'),
                "trade" => Some('™'),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "hellip" => Some('…'),
                "lsquo" => Some('‘'),
                "rsquo" => Some('’'),
                "ldquo" => Some('“'),
                "rdquo" => Some('”'),
                "bull" => Some('•'),
                "middot" => Some('·'),
                "laquo" => Some('«'),
                "raquo" => Some('»'),
                "times" => Some('×'),
                "divide" => Some('÷'),
                "euro" => Some('€'),
                "pound" => Some('£'),
                "yen" => Some('¥'),
                "cent" => Some('¢'),
                "lrm" => Some('\u{200e}'),
                "rlm" => Some('\u{200f}'),
                _ => None,
            }
        };
        if let Some(decoded) = decoded {
            output.push(decoded);
            rest = &rest[end + 1..];
        } else {
            output.push('&');
        }
    }
    output.push_str(rest);
    output
}

fn entity_char(value: u32) -> char {
    let value = match value {
        0 => 0xfffd,
        0x80 => 0x20ac,
        0x82 => 0x201a,
        0x83 => 0x0192,
        0x84 => 0x201e,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02c6,
        0x89 => 0x2030,
        0x8a => 0x0160,
        0x8b => 0x2039,
        0x8c => 0x0152,
        0x8e => 0x017d,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201c,
        0x94 => 0x201d,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02dc,
        0x99 => 0x2122,
        0x9a => 0x0161,
        0x9b => 0x203a,
        0x9c => 0x0153,
        0x9e => 0x017e,
        0x9f => 0x0178,
        other => other,
    };
    char::from_u32(value).unwrap_or('\u{fffd}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_dom_and_item_sources_describe_actual_nodes() {
        let doc = parse(
            r#"<html><head><script>if (a < b) text = '&amp;';</script></head><body><form id=search><input name=q><button>Search</button></form><p id=prose>Hello <strong>world</strong></p><a id=first href=/next>Read <b>this</b></a><a id=second href=/next>Other</a><div hidden id=secret>Hidden content</div><img src=/photo.png alt=Photo></body></html>"#,
            "https://example.org/",
        );
        assert_eq!(doc.nodes[0].tag, "#document");
        assert_eq!(doc.nodes[doc.nodes[0].children[0]].tag, "html");
        let script = doc.query_selector(0, "script").unwrap().unwrap();
        assert_eq!(
            doc.nodes[doc.nodes[script].children[0]].text,
            "if (a < b) text = '&amp;';"
        );
        let secret = doc.query_selector(0, "#secret").unwrap().unwrap();
        assert_eq!(
            doc.nodes[doc.nodes[secret].children[0]].text,
            "Hidden content"
        );
        assert_eq!(doc.items.len(), doc.item_nodes.len());
        for id in &doc.item_nodes {
            assert!(*id < doc.nodes.len());
        }
        let input = doc.query_selector(0, "input[name=q]").unwrap().unwrap();
        let button = doc.query_selector(0, "button").unwrap().unwrap();
        let first = doc.query_selector(0, "#first").unwrap().unwrap();
        let second = doc.query_selector(0, "#second").unwrap().unwrap();
        let prose = doc.query_selector(0, "#prose").unwrap().unwrap();
        assert!(
            doc.items
                .iter()
                .zip(&doc.item_nodes)
                .any(|(item, id)| matches!(item, Item::Input { .. }) && *id == input)
        );
        assert!(
            doc.items
                .iter()
                .zip(&doc.item_nodes)
                .any(|(item, id)| matches!(item, Item::Submit { .. }) && *id == button)
        );
        assert!(doc.items.iter().zip(&doc.item_nodes).any(
            |(item, id)| matches!(item, Item::Text { text, .. } if text == "Read this")
                && *id == first
        ));
        assert!(doc.items.iter().zip(&doc.item_nodes).any(
            |(item, id)| matches!(item, Item::Text { text, .. } if text == "Other")
                && *id == second
        ));
        assert!(doc.items.iter().zip(&doc.item_nodes).any(
            |(item, id)| matches!(item, Item::Text { text, .. } if text == "Hello world")
                && *id == prose
        ));
        assert!(!doc.item_nodes.contains(&secret));
    }

    #[test]
    fn selector_subset_returns_tree_order_with_scoped_descendants() {
        let doc = parse(
            r#"<main id=outer><section id=scope class="panel primary"><form id=search><input class="field focus" name=q data-label="Search terms"><input class=field name=other disabled></form><a class="fieldish" href=/next>Next</a></section><input class=field name=outside></main>"#,
            "https://example.org/",
        );
        let scope = doc.query_selector(0, "#scope").unwrap().unwrap();
        let first = doc
            .query_selector(scope, "INPUT.field[name=q][data-label='Search terms']")
            .unwrap()
            .unwrap();
        let second = doc.query_selector(scope, "[disabled]").unwrap().unwrap();
        assert_eq!(
            doc.query_selector_all(scope, ".field").unwrap(),
            [first, second]
        );
        assert_eq!(
            doc.query_selector_all(scope, "section.panel form#search input.field")
                .unwrap(),
            [first, second]
        );
        assert_eq!(
            doc.query_selector(scope, "main input[name=q]").unwrap(),
            Some(first)
        );
        assert_eq!(doc.query_selector(scope, "#scope").unwrap(), None);
        assert_eq!(doc.query_selector(scope, ".FIELD").unwrap(), None);
        assert!(
            doc.query_selector_all(scope, "*")
                .unwrap()
                .iter()
                .all(|id| !doc.nodes[*id].tag.starts_with('#'))
        );
        assert_eq!(doc.query_selector_all(0, "[name]").unwrap().len(), 3);
        assert!(doc.query_selector(doc.nodes.len(), "input").is_err());
    }

    #[test]
    fn unsupported_selectors_return_errors_instead_of_empty_success() {
        let doc = parse("<div><input name=q></div>", "https://example.org/");
        for selector in [
            "",
            " ",
            "div > input",
            "div+input",
            "div~input",
            "div,input",
            ":scope",
            "input:first-child",
            "[name^=q]",
            "[name=q i]",
            "[name",
            "[name='q]",
            "#",
            ".",
            r"#escaped\:id",
            "svg|a",
            "[name='a\\b']",
        ] {
            assert!(
                doc.query_selector(0, selector).is_err(),
                "selector unexpectedly supported: {selector:?}"
            );
            assert!(
                doc.query_selector_all(0, selector).is_err(),
                "selector unexpectedly supported: {selector:?}"
            );
        }
    }

    #[test]
    fn search_form_controls_and_relative_result_link() {
        let doc = parse(
            r#"<!doctype html><title>Research &amp; testing</title><form action="/search"><input type=hidden name=source value=hp><textarea name=q>rust &amp; browsers</textarea><button name=go value=1><b>Search</b></button><input disabled name=omit value=no></form><a href="/url?q=https%3A%2F%2Fexample.org%2F&amp;sa=U"><h3>First <span>result</span></h3></a>"#,
            "https://search.example.org/",
        );
        assert_eq!(doc.title, "Research & testing");
        assert_eq!(
            doc.forms,
            vec![Form {
                action: "https://search.example.org/search".to_owned(),
                method: "get".to_owned(),
                fields: vec![("source".to_owned(), "hp".to_owned())]
            }]
        );
        assert!(doc.items.contains(&Item::Input {
            form: 0,
            name: "q".to_owned(),
            value: "rust & browsers".to_owned(),
            kind: "text".to_owned()
        }));
        assert!(doc.items.contains(&Item::Submit {
            form: 0,
            name: "go".to_owned(),
            value: "1".to_owned(),
            label: "Search".to_owned()
        }));
        assert!(doc.items.iter().any(|item| matches!(item, Item::Text { text, href: Some(href), heading: true } if text == "First result" && href == "https://search.example.org/url?q=https%3A%2F%2Fexample.org%2F&sa=U")));
    }

    #[test]
    fn raw_text_and_hidden_content_do_not_become_links() {
        let doc = parse(
            r#"<head><title>Good</title><script>"<a href='/fake'>fake</a>"</script></head><p>Hello <b>world</b>!</p><div style="DISPLAY: none !important"><a href=/hidden>hidden</a></div><div aria-hidden=true>other hidden</div><template>template hidden</template><noscript><p>Please enable JavaScript. <a href=/help>Help</a></p></noscript>"#,
            "https://example.org/",
        );
        assert_eq!(
            doc.items
                .iter()
                .filter_map(|item| match item {
                    Item::Text {
                        href: Some(href), ..
                    } => Some(href.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            ["https://example.org/help"]
        );
        assert!(doc.items.contains(&Item::Text {
            text: "Hello world!".to_owned(),
            href: None,
            heading: false
        }));
    }

    #[test]
    fn common_entities_and_invalid_numeric_references() {
        assert_eq!(
            decode_entities("a &amp; &lt; &#x1F980; &#169; &#0; &#xD800; &#128; &unknown;"),
            "a & < 🦀 © � � € &unknown;"
        );
        assert_eq!(
            decode_entities("literal & and &amp; &#xnot;"),
            "literal & and & &#xnot;"
        );
    }

    #[test]
    fn base_url_external_control_and_successful_defaults() {
        let doc = parse(
            r#"<base href="https://cdn.example.org/docs/"><form id=f><input type=hidden name=a value=1><input type=checkbox name=c checked><input type=checkbox name=skip><select name=s><option>first<option selected value=two>Second</select></form><input form=f name=q value='&quot;rust&quot;'><a href=../read>Read</a>"#,
            "https://example.org/original",
        );
        assert_eq!(doc.base_url, "https://cdn.example.org/docs/");
        assert_eq!(doc.forms[0].action, "https://example.org/original");
        assert_eq!(
            doc.forms[0].fields,
            [
                ("a".into(), "1".into()),
                ("c".into(), "on".into()),
                ("s".into(), "two".into())
            ]
        );
        assert!(doc.items.contains(&Item::Input {
            form: 0,
            name: "q".into(),
            value: "\"rust\"".into(),
            kind: "text".into()
        }));
    }

    #[test]
    fn malformed_and_deep_input_stays_bounded() {
        let html = format!("<p>before{}tail", "<div>".repeat(10_000));
        let doc = parse(&html, "https://example.org/");
        assert!(doc.items.len() < MAX_DEPTH * 2);
        assert!(
            doc.items
                .iter()
                .any(|item| matches!(item, Item::Text { text, .. } if text == "before"))
        );
        for source in [
            "<",
            "<a href='broken",
            "<p>one<p>two",
            "<input == value>",
            "💛 < x > 💜",
            "<script>unterminated",
        ] {
            let _ = parse(source, "https://example.org/");
        }
    }

    #[test]
    fn standard_noscript_zero_delay_refresh_is_resolved() {
        let doc = parse(
            r#"<noscript><meta content="0;URL='/retry?a=1&amp;b=2'" http-equiv=REFRESH><p>Enable JavaScript</p></noscript>"#,
            "https://example.org/search?q=rust",
        );
        assert_eq!(
            doc.refresh.as_deref(),
            Some("https://example.org/retry?a=1&b=2")
        );
        assert_eq!(meta_refresh("5;url=/later", "https://example.org/"), None);
        assert_eq!(
            meta_refresh("0;url=javascript:alert(1)", "https://example.org/"),
            None
        );
        assert_eq!(
            meta_refresh("0;url=data:text/html,hello", "https://example.org/"),
            None
        );
    }
}
