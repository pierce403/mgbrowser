//! Small, bounded author stylesheet, image, font, and classic external script
//! loading. No CSS @imports or data URLs are enabled. Stylesheets remain
//! same-origin; cross-origin HTTPS images and scripts are credential-free,
//! including later redirects. All fetches use the browser session's verified
//! Rust TLS transport.

use crate::net::{ResourceKind, Response, Session};
use mg_sparkle::document::{Document, Node, ResourceData, StylesheetRequest, StylesheetSource};
use mg_sparkle::js_browser::{
    ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource,
    MAX_CLASSIC_SOURCE_URL_BYTES, MAX_EXTERNAL_SCRIPTS, MAX_EXTERNAL_SOURCE, MAX_SCRIPTS,
    MAX_SOURCE, MAX_TYPED_REQUEST_BYTES, SourceWireMode, classic_script_type,
    external_request_json_bytes, external_source_json_bytes_with_provenance,
    unsupported_external_attribute,
};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use url::Url;

const MAX_SHEETS: usize = 16;
const MAX_REQUESTS: usize = 32;
const MAX_IMAGE: usize = 512 * 1024;
const MAX_TOTAL: usize = 2 * 1024 * 1024;
// A single stylesheet may consume the existing shared 2 MiB resource budget.
// This changes its partition, not the aggregate, request, sheet or time bounds.
const MAX_CSS: usize = MAX_TOTAL;
const MAX_WARNINGS: usize = 32;
const MAX_WARNING_BYTES: usize = 1024;
const RESOURCE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourceTiming {
    pub kind: &'static str,
    pub url: String,
    pub status: Option<u16>,
    pub bytes: usize,
    pub duration_ms: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourceLoadProfile {
    pub total_ms: f64,
    pub requests: usize,
    pub bytes: usize,
    pub timings: Vec<ResourceTiming>,
    deadline: Option<Instant>,
    omitted_warnings: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptFetchProfile {
    pub total_ms: f64,
    /// Admitted external source elements, including repeated URL executions.
    pub loaded: usize,
    /// Resource-ledger charge, including downloaded bodies rejected after fetch
    /// and the separate retained copies required by duplicate source elements.
    pub bytes: usize,
    pub timings: Vec<ResourceTiming>,
}

/// Populate source-ordered author CSS and small image resources while keeping
/// failures nonfatal. Cancellation stops scheduling; an already running bounded
/// socket/resolver operation may finish before cancellation is observed.
pub fn load(
    session: &Session,
    document: &mut Document,
    page_url: &str,
    cancelled: impl Fn() -> bool,
) {
    let _ = load_with_profile(session, document, page_url, cancelled);
}

pub fn load_with_profile(
    session: &Session,
    document: &mut Document,
    page_url: &str,
    cancelled: impl Fn() -> bool,
) -> ResourceLoadProfile {
    let started = Instant::now();
    let requests = document.stylesheet_requests();
    document.stylesheets.clear();
    document.resources.clear();
    document.resource_warnings.clear();
    let mut loader = Loader {
        session,
        page_url,
        cancelled: &cancelled,
        deadline: started + RESOURCE_TIMEOUT,
        requests: 0,
        bytes: 0,
        warnings: Vec::new(),
        omitted_warnings: 0,
        timings: Vec::new(),
    };
    let mut image_urls = Vec::new();
    let mut font_urls = Vec::new();
    let mut css_cache = HashMap::<String, usize>::new();
    for (index, request) in requests.into_iter().enumerate() {
        if index == MAX_SHEETS {
            loader.warn("Stylesheet count exceeds 16; remaining styles are skipped".into());
            break;
        }
        if loader.stopped() {
            break;
        }
        let sheet = match request {
            StylesheetRequest::Inline(sheet) => {
                if sheet.css.len() > MAX_CSS {
                    loader.warn(format!(
                        "Inline stylesheet exceeds resource size budget: sheet {} at {} has {} bytes, limit {MAX_CSS}",
                        index + 1, label(&sheet.base_url), sheet.css.len()
                    ));
                    continue;
                }
                if !loader.admit(sheet.css.len()) {
                    loader.warn(format!(
                        "Inline stylesheet {} at {} skipped after total budget rejection",
                        index + 1,
                        label(&sheet.base_url)
                    ));
                    continue;
                }
                sheet
            }
            StylesheetRequest::Linked { url, media } => {
                if let Some(&index) = css_cache.get(&url) {
                    let previous = &document.stylesheets[index];
                    // Repeated author sheets still participate in source order;
                    // charge the retained duplicate as well as the first source.
                    if !loader.admit(previous.css.len()) {
                        continue;
                    }
                    StylesheetSource {
                        css: previous.css.clone(),
                        base_url: previous.base_url.clone(),
                        media,
                    }
                } else {
                    let Some(response) = loader.fetch(&url, MAX_CSS, ResourceKind::Stylesheet)
                    else {
                        continue;
                    };
                    if mime(&response.content_type) != "text/css" {
                        loader.warn(format!(
                            "Stylesheet has unsupported content type: {} at {}",
                            label(&response.content_type),
                            label(&url)
                        ));
                        continue;
                    }
                    let css = match String::from_utf8(response.body) {
                        Ok(css) => css,
                        Err(_) => {
                            loader
                                .warn(format!("Stylesheet is not UTF-8; skipped: {}", label(&url)));
                            continue;
                        }
                    };
                    let base_url = response.url.to_string();
                    css_cache.insert(url, document.stylesheets.len());
                    css_cache.insert(base_url.clone(), document.stylesheets.len());
                    StylesheetSource {
                        css,
                        base_url,
                        media,
                    }
                }
            }
        };
        let (urls, imports) = css_image_urls(&sheet.css);
        if imports {
            loader.warn(format!(
                "CSS @import is not loaded in this preview: {}",
                label(&sheet.base_url)
            ));
        }
        for value in urls {
            if image_urls.len() < MAX_REQUESTS * 2 {
                image_urls.push((sheet.base_url.clone(), value));
            }
        }
        for (_, value) in mg_sparkle::document::font_face_rules(&sheet.css) {
            if font_urls.len() < MAX_REQUESTS * 2 {
                font_urls.push((sheet.base_url.clone(), value));
            }
        }
        document.stylesheets.push(sheet);
    }
    for id in document.resource_nodes() {
        if image_urls.len() == MAX_REQUESTS * 2 {
            loader.warn("Image candidate count exceeds 64; remaining images are skipped".into());
            break;
        }
        let node = &document.nodes[id];
        if node.tag == "img"
            && let Some(src) = node.attr("src")
        {
            image_urls.push((document.base_url.clone(), src.to_owned()));
        }
        if let Some(style) = node.attr("style") {
            for value in css_image_urls(style).0 {
                if image_urls.len() < MAX_REQUESTS * 2 {
                    image_urls.push((document.base_url.clone(), value));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    for (base, reference) in font_urls {
        if loader.stopped() {
            break;
        }
        let Ok(mut url) = Url::parse(&base).and_then(|base| base.join(reference.trim())) else {
            loader.warn(format!(
                "Invalid font resource URL; skipped: {} at {}",
                label(&reference),
                label(&base)
            ));
            continue;
        };
        url.set_fragment(None);
        let url = url.to_string();
        if !seen.insert(url.clone()) {
            continue;
        }
        let Some(response) = loader.fetch(&url, MAX_IMAGE, ResourceKind::Stylesheet) else {
            continue;
        };
        if !matches!(
            mime(&response.content_type).as_str(),
            "font/ttf"
                | "font/otf"
                | "application/font-sfnt"
                | "application/x-font-ttf"
                | "application/octet-stream"
        ) {
            loader.warn(format!(
                "Unsupported page font type: {} at {}",
                label(&response.content_type),
                label(&url)
            ));
            continue;
        }
        document.resources.insert(
            url,
            ResourceData {
                bytes: response.body,
                content_type: response.content_type,
            },
        );
    }
    for (base, reference) in image_urls {
        if loader.stopped() {
            break;
        }
        let Ok(mut url) = Url::parse(&base).and_then(|base| base.join(reference.trim())) else {
            loader.warn(format!(
                "Invalid image resource URL; skipped: {} at {}",
                label(&reference),
                label(&base)
            ));
            continue;
        };
        url.set_fragment(None);
        let url = url.to_string();
        if !seen.insert(url.clone()) {
            continue;
        }
        let Some(response) = loader.fetch(&url, MAX_IMAGE, ResourceKind::Image) else {
            continue;
        };
        if !matches!(
            mime(&response.content_type).as_str(),
            "image/png" | "image/gif" | "image/jpeg" | "image/svg+xml"
        ) {
            loader.warn(format!(
                "Unsupported page image type: {} at {}",
                label(&response.content_type),
                label(&url)
            ));
            continue;
        }
        document.resources.insert(
            url,
            ResourceData {
                bytes: response.body,
                content_type: response.content_type,
            },
        );
    }
    loader.compact_warnings();
    document.resource_warnings = loader.warnings;
    ResourceLoadProfile {
        total_ms: started.elapsed().as_secs_f64() * 1000.0,
        requests: loader.requests,
        bytes: loader.bytes,
        timings: loader.timings,
        deadline: Some(loader.deadline),
        omitted_warnings: loader.omitted_warnings,
    }
}

fn inert_script(nodes: &[Node], mut id: usize) -> bool {
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

/// Fetch only parser-discovered classic sources under the same resource ledger
/// and deadline as CSS, fonts and images. Sources cross the worker protocol as
/// typed parent metadata; comments in the untrusted HTML remain inert HTML.
pub fn prepare_script_sources(
    session: &Session,
    document: &mut Document,
    raw_html: &str,
    page_url: &str,
    cancelled: impl Fn() -> bool,
    resources: &ResourceLoadProfile,
    mode: SourceWireMode,
) -> (Vec<ExternalScriptSource>, ScriptFetchProfile) {
    let started = Instant::now();
    let mut profile = ScriptFetchProfile::default();
    let mut entries: Vec<ExternalScriptSource> = Vec::new();
    let mut source_bytes = raw_html.len();
    let mut wire_bytes = external_request_json_bytes(page_url, raw_html).unwrap_or(usize::MAX);
    let mut warnings = std::mem::take(&mut document.resource_warnings);
    // The earlier phase already counted the diagnostic displaced by its
    // summary. Restore only actual messages before continuing that same ledger.
    if resources.omitted_warnings > 0
        && warnings.last() == Some(&warning_summary(resources.omitted_warnings))
    {
        warnings.pop();
    }
    let mut loader = Loader {
        session,
        page_url,
        cancelled: &cancelled,
        deadline: resources.deadline.unwrap_or(started + RESOURCE_TIMEOUT),
        requests: resources.requests,
        bytes: resources.bytes,
        warnings,
        omitted_warnings: resources.omitted_warnings,
        timings: Vec::new(),
    };
    // Cache only indexes into admitted sources; no second retained source copy.
    let mut cache = HashMap::<String, usize>::new();
    let mut eligible_count = 0usize;
    let mut external_count = 0usize;
    for (index, node) in document.nodes.iter().enumerate() {
        if node.tag != "script" || inert_script(&document.nodes, index) {
            continue;
        }
        let kind = node.attr("type").unwrap_or("").trim().to_ascii_lowercase();
        if kind != "module" && !classic_script_type(&kind) {
            continue;
        }
        eligible_count += 1;
        if eligible_count > MAX_SCRIPTS {
            break;
        }
        if kind == "module" {
            continue;
        }
        let Some(src) = node.attr("src") else {
            continue;
        };
        external_count += 1;
        if external_count > MAX_EXTERNAL_SCRIPTS {
            loader
                .warn("External script count limit reached (16); remaining sources skipped".into());
            break;
        }
        if let Some(attribute) = unsupported_external_attribute(node) {
            loader.warn(format!(
                "External script attribute {attribute} is not implemented; source skipped: {}",
                label(src)
            ));
            continue;
        }
        if loader.stopped() {
            break;
        }
        if source_bytes >= MAX_SOURCE || wire_bytes >= MAX_TYPED_REQUEST_BYTES {
            loader.warn("External scripts exceed the 1 MiB combined document/source or 2 MiB wire budget; remaining sources skipped".into());
            break;
        }
        let Ok(mut resolved) =
            Url::parse(&document.base_url).and_then(|base| base.join(src.trim()))
        else {
            loader.warn(format!(
                "Invalid external script URL; skipped: {}",
                label(src)
            ));
            continue;
        };
        resolved.set_fragment(None);
        let url = resolved.to_string();
        if let Some(&previous) = cache.get(&url) {
            let previous = &entries[previous];
            let source = &previous.source;
            let additional_wire = external_source_json_bytes_with_provenance(
                index,
                source,
                previous.provenance.as_ref(),
            )
            .unwrap_or(usize::MAX)
            .saturating_add(usize::from(!entries.is_empty()));
            if source.len() > MAX_SOURCE.saturating_sub(source_bytes)
                || additional_wire > MAX_TYPED_REQUEST_BYTES.saturating_sub(wire_bytes)
            {
                loader.warn(format!(
                    "External script exceeds the combined source/wire budget; skipped: {}",
                    label(&url)
                ));
                continue;
            }
            // A repeated execution retains a separate request source, just as
            // repeated CSS retains separate source-ordered sheets.
            if !loader.admit(source.len()) {
                continue;
            }
            let source = source.clone();
            let provenance = previous.provenance.clone();
            source_bytes += source.len();
            wire_bytes += additional_wire;
            entries.push(ExternalScriptSource {
                node: index,
                source,
                provenance,
            });
            continue;
        }
        let limit = MAX_EXTERNAL_SOURCE.min(MAX_SOURCE.saturating_sub(source_bytes));
        let Some(response) = loader.fetch(&url, limit, ResourceKind::Script) else {
            continue;
        };
        // A conservative MIME policy also refuses text/plain + nosniff without
        // needing to discard the server's no-sniff instruction.
        let content_type = mime(&response.content_type);
        if !classic_script_type(&content_type) || content_type.is_empty() {
            loader.warn(format!(
                "External script has unsupported content type: {} at {}",
                label(&response.content_type),
                label(&url)
            ));
            continue;
        }
        let Ok(source) = String::from_utf8(response.body) else {
            loader.warn(format!(
                "External script is not UTF-8; skipped: {}",
                label(&url)
            ));
            continue;
        };
        let provenance = if mode == SourceWireMode::Modern {
            if response.url.as_str().len() > MAX_CLASSIC_SOURCE_URL_BYTES {
                loader.warn(format!("External script response URL exceeds the 16384 byte metadata limit; skipped: {}", label(&url)));
                continue;
            }
            Some(ClassicSourceProvenance {
                // Move the transport's final serialization, rather than retain
                // a second URL copy before encoded-wire admission.
                response_url: response.url.into(),
                error_visibility: if response.resource_same_origin_chain {
                    ClassicErrorVisibility::SameOrigin
                } else {
                    ClassicErrorVisibility::Muted
                },
            })
        } else {
            None
        };
        let additional_wire =
            external_source_json_bytes_with_provenance(index, &source, provenance.as_ref())
                .unwrap_or(usize::MAX)
                .saturating_add(usize::from(!entries.is_empty()));
        if additional_wire > MAX_TYPED_REQUEST_BYTES.saturating_sub(wire_bytes) {
            loader.warn(format!(
                "External script exceeds the existing 2 MiB wire budget; skipped: {}",
                label(&url)
            ));
            continue;
        }
        source_bytes += source.len();
        wire_bytes += additional_wire;
        cache.insert(url, entries.len());
        entries.push(ExternalScriptSource {
            node: index,
            source,
            provenance,
        });
    }
    profile.total_ms = started.elapsed().as_secs_f64() * 1000.0;
    profile.loaded = entries.len();
    profile.bytes = loader.bytes.saturating_sub(resources.bytes);
    loader.compact_warnings();
    profile.timings = loader.timings;
    document.resource_warnings = loader.warnings;
    (entries, profile)
}

fn warning_summary(omitted: usize) -> String {
    format!("{omitted} additional resource diagnostics omitted (limit {MAX_WARNINGS})")
}

fn bounded_text(value: &str, limit: usize) -> String {
    const MARKER: &str = "[truncated]";
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit.saturating_sub(MARKER.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARKER}", &value[..end])
}

fn label(value: &str) -> String {
    bounded_text(value, 384)
}

fn mime(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

struct Loader<'a, F: Fn() -> bool> {
    session: &'a Session,
    page_url: &'a str,
    cancelled: &'a F,
    deadline: Instant,
    requests: usize,
    bytes: usize,
    warnings: Vec<String>,
    omitted_warnings: usize,
    timings: Vec<ResourceTiming>,
}

impl<F: Fn() -> bool> Loader<'_, F> {
    fn compact_warnings(&mut self) {
        if self.omitted_warnings == 0 {
            return;
        }
        // The summary occupies one of the existing 32 slots. A displaced real
        // diagnostic joins the cumulative omitted count, never the next phase's
        // message stream.
        while self.warnings.len() >= MAX_WARNINGS {
            self.warnings.pop();
            self.omitted_warnings = self.omitted_warnings.saturating_add(1);
        }
        self.warnings.push(warning_summary(self.omitted_warnings));
    }

    fn warn(&mut self, text: String) {
        let text = bounded_text(&text, MAX_WARNING_BYTES);
        if self.warnings.contains(&text) {
            return;
        }
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(text);
        } else {
            self.omitted_warnings = self.omitted_warnings.saturating_add(1);
        }
    }

    fn stopped(&mut self) -> bool {
        if (self.cancelled)() {
            return true;
        }
        if Instant::now() >= self.deadline {
            self.warn("Page resource deadline exceeded; remaining resources are skipped".into());
            return true;
        }
        false
    }

    fn admit(&mut self, bytes: usize) -> bool {
        if bytes > MAX_TOTAL.saturating_sub(self.bytes) {
            self.warn("Page resources exceed the 2 MiB total byte budget".into());
            false
        } else {
            self.bytes += bytes;
            true
        }
    }

    fn fetch(&mut self, url: &str, limit: usize, kind: ResourceKind) -> Option<Response> {
        if self.stopped() {
            return None;
        }
        if self.requests == MAX_REQUESTS {
            self.warn("Page resource request limit reached (32)".into());
            return None;
        }
        if self.bytes == MAX_TOTAL {
            self.warn("Page resources exceed the 2 MiB total byte budget".into());
            return None;
        }
        self.requests += 1;
        let kind_label = match kind {
            ResourceKind::Stylesheet => "css/font",
            ResourceKind::Image => "image",
            ResourceKind::Script => "script",
        };
        let req_start = Instant::now();
        let result = self.session.fetch_resource(
            url,
            self.page_url,
            limit.min(MAX_TOTAL - self.bytes),
            self.deadline,
            kind,
        );
        let duration_ms = req_start.elapsed().as_secs_f64() * 1000.0;
        match result {
            Ok(response) if (200..300).contains(&response.status) => {
                self.timings.push(ResourceTiming {
                    kind: kind_label,
                    url: label(url),
                    status: Some(response.status),
                    bytes: response.body.len(),
                    duration_ms,
                });
                if self.stopped() || !self.admit(response.body.len()) {
                    None
                } else {
                    Some(response)
                }
            }
            Ok(response) => {
                self.timings.push(ResourceTiming {
                    kind: kind_label,
                    url: label(url),
                    status: Some(response.status),
                    bytes: response.body.len(),
                    duration_ms,
                });
                self.admit(response.body.len());
                self.warn(format!(
                    "Resource returned HTTP {}; skipped: {}",
                    response.status,
                    label(url)
                ));
                None
            }
            Err(error) => {
                self.timings.push(ResourceTiming {
                    kind: kind_label,
                    url: label(url),
                    status: None,
                    bytes: 0,
                    duration_ms,
                });
                self.warn(format!(
                    "Resource unavailable: {} at {} (byte limit {})",
                    label(&error),
                    label(url),
                    limit.min(MAX_TOTAL - self.bytes)
                ));
                None
            }
        }
    }
}

/// Bounded CSS URL discovery, not a cascade parser. It skips comments, ordinary
/// strings and @import statements. Quoted/unquoted url() tokens support CSS
/// escapes; malformed tokens are ignored. Computed-style matching still decides
/// whether a fetched image is painted.
fn css_image_urls(css: &str) -> (Vec<String>, bool) {
    let mut urls = Vec::new();
    let mut imports = false;
    let mut at = 0;
    let mut declaration = String::new();
    while at < css.len() && urls.len() < MAX_REQUESTS * 2 {
        let rest = &css[at..];
        if rest.starts_with("/*") {
            at = rest.find("*/").map_or(css.len(), |end| at + end + 2);
            continue;
        }
        let ch = rest.chars().next().unwrap();
        if matches!(ch, '\'' | '"') {
            skip_string(css, &mut at, ch);
            continue;
        }
        if rest
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("@import"))
            && rest
                .get(7..)
                .and_then(|s| s.chars().next())
                .is_none_or(|c| !identifier(c))
        {
            imports = true;
            at += 7;
            while at < css.len() {
                if css[at..].starts_with("/*") {
                    at = css[at..].find("*/").map_or(css.len(), |end| at + end + 2);
                    continue;
                }
                let ch = css[at..].chars().next().unwrap();
                if matches!(ch, '\'' | '"') {
                    skip_string(css, &mut at, ch);
                } else {
                    at += ch.len_utf8();
                    if ch == ';' || ch == '}' {
                        break;
                    }
                }
            }
            continue;
        }
        if identifier(ch) {
            let start = at;
            while at < css.len() && css[at..].chars().next().is_some_and(identifier) {
                at += css[at..].chars().next().unwrap().len_utf8();
            }
            let name = &css[start..at];
            let mut after = at;
            css_space(css, &mut after);
            if css[after..].starts_with(':') {
                declaration = name.to_ascii_lowercase();
            }
            if css[start..at].eq_ignore_ascii_case("url") && css[at..].starts_with('(') {
                at += 1;
                if let Some(value) = url_token(css, &mut at)
                    && matches!(declaration.as_str(), "background" | "background-image")
                {
                    urls.push(value);
                }
            }
            continue;
        }
        if matches!(ch, ';' | '{' | '}') {
            declaration.clear();
        }
        at += ch.len_utf8();
    }
    (urls, imports)
}

fn identifier(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '-' | '_') || !ch.is_ascii()
}

fn skip_string(css: &str, at: &mut usize, quote: char) {
    *at += 1;
    while *at < css.len() {
        let ch = css[*at..].chars().next().unwrap();
        *at += ch.len_utf8();
        if ch == quote {
            return;
        }
        if ch == '\\' && *at < css.len() {
            *at += css[*at..].chars().next().unwrap().len_utf8();
        }
    }
}

fn css_space(css: &str, at: &mut usize) {
    while *at < css.len() && css[*at..].chars().next().unwrap().is_ascii_whitespace() {
        *at += 1;
    }
}

fn url_token(css: &str, at: &mut usize) -> Option<String> {
    css_space(css, at);
    let quote = css[*at..]
        .chars()
        .next()
        .filter(|ch| matches!(ch, '\'' | '"'));
    if quote.is_some() {
        *at += 1;
    }
    let mut value = String::new();
    while *at < css.len() && value.len() <= 16_384 {
        let ch = css[*at..].chars().next()?;
        *at += ch.len_utf8();
        if Some(ch) == quote || quote.is_none() && (ch == ')' || ch.is_ascii_whitespace()) {
            if ch == ')' {
                return (!value.is_empty()).then_some(value);
            }
            css_space(css, at);
            if css[*at..].starts_with(')') {
                *at += 1;
                return (!value.is_empty()).then_some(value);
            }
            return None;
        }
        if ch == '\\' {
            let start = *at;
            let mut digits = 0;
            while *at < css.len() && digits < 6 && css.as_bytes()[*at].is_ascii_hexdigit() {
                *at += 1;
                digits += 1;
            }
            if digits > 0 {
                let code = u32::from_str_radix(&css[start..*at], 16).ok()?;
                value.push(
                    char::from_u32(code)
                        .filter(|c| *c != '\0')
                        .unwrap_or('\u{fffd}'),
                );
                if *at < css.len() && css.as_bytes()[*at].is_ascii_whitespace() {
                    *at += 1;
                }
            } else {
                let escaped = css[*at..].chars().next()?;
                if matches!(escaped, '\n' | '\r' | '\x0c') {
                    return None;
                }
                *at += escaped.len_utf8();
                value.push(escaped);
            }
        } else if ch.is_control() || quote.is_none() && matches!(ch, '(' | '\'' | '"') {
            return None;
        } else {
            value.push(ch);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mg_sparkle::document::parse;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn server<S: AsRef<str> + Send + 'static>(
        replies: Vec<(S, S, S)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        server_with_lengths(
            replies
                .into_iter()
                .map(|(status, mime, body)| (status, mime, body, None))
                .collect(),
        )
    }

    fn server_with_lengths<S: AsRef<str> + Send + 'static>(
        replies: Vec<(S, S, S, Option<usize>)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut paths = Vec::new();
            for (status, content_type, body, declared_length) in replies {
                let (status, content_type, body) =
                    (status.as_ref(), content_type.as_ref(), body.as_ref());
                let mut socket = loop {
                    assert!(Instant::now() < deadline, "Missing resource request");
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(1))
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = String::new();
                let mut reader = BufReader::new(socket.try_clone().unwrap());
                reader.read_line(&mut request).unwrap();
                paths.push(request.split_whitespace().nth(1).unwrap().to_owned());
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                }
                write!(socket, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", declared_length.unwrap_or(body.len())).unwrap();
            }
            paths
        });
        (base, worker)
    }

    #[test]
    fn css_url_discovery_skips_comments_strings_imports_and_handles_escapes() {
        let (urls, imports) = css_image_urls(
            r#"
            /* url(fake.svg) */ @import url(import.css) screen;
            @font-face { src: url(font.woff); }
            a { content: "url(string.svg)"; background: linear-gradient(transparent, transparent), URL('arrow\2e svg'); }
            b { background-image: url(logo.svg); other: xurl(not-url.svg); }
            c { background: url("broken.svg" invalid); }
        "#,
        );
        assert!(imports);
        assert_eq!(urls, ["arrow.svg", "logo.svg"]);
    }

    #[test]
    fn source_order_final_base_urls_and_image_deduplication() {
        let (base, server) = server(vec![
            ("302 Found\r\nLocation: /assets/theme.css", "text/css", ""),
            (
                "200 OK",
                "text/css; charset=utf-8",
                "a{background:url(arrow.svg)}",
            ),
            ("200 OK", "image/svg+xml", "<svg/>"),
            ("200 OK", "image/gif", "GIF89a"),
        ]);
        let html = "<style>a{color:red}</style><link rel=stylesheet href=/theme><style media=screen>a{color:blue}</style><link rel=stylesheet href=/assets/theme.css media=print><img src=/space.gif><img src=/space.gif><script src=/must-not-fetch.js></script>";
        let mut doc = parse(html, &base);
        load(&Session::new(), &mut doc, &base, || false);
        assert!(
            doc.resource_warnings.is_empty(),
            "{:?}",
            doc.resource_warnings
        );
        assert_eq!(doc.stylesheets.len(), 4);
        assert_eq!(doc.stylesheets[0].css, "a{color:red}");
        assert_eq!(
            doc.stylesheets[1].base_url,
            format!("{base}/assets/theme.css")
        );
        assert_eq!(doc.stylesheets[2].media, "screen");
        assert_eq!(doc.stylesheets[3].css, doc.stylesheets[1].css);
        assert_eq!(doc.stylesheets[3].media, "print");
        assert!(
            doc.resources
                .contains_key(&format!("{base}/assets/arrow.svg"))
        );
        assert_eq!(doc.resources[&format!("{base}/space.gif")].bytes, b"GIF89a");
        assert_eq!(
            server.join().unwrap(),
            [
                "/theme",
                "/assets/theme.css",
                "/assets/arrow.svg",
                "/space.gif"
            ]
        );
    }

    #[test]
    fn failures_keep_readable_document_and_reject_new_origins_and_schemes() {
        let (base, server) = server(vec![("404 Not Found", "text/css", "missing")]);
        let mut doc = parse(
            "<p>Still readable</p><link rel=stylesheet href=/missing><img src=http://127.0.0.1:1/private><img src=file:///etc/passwd><img src=data:image/png,private>",
            &base,
        );
        load(&Session::new(), &mut doc, &base, || false);
        assert!(doc.stylesheets.is_empty());
        assert!(doc.resources.is_empty());
        assert_eq!(doc.resource_warnings.len(), 4);
        assert!(doc.items.iter().any(|item| matches!(item, mg_sparkle::document::Item::Text {text,..} if text == "Still readable")));
        assert_eq!(server.join().unwrap(), ["/missing"]);
    }

    #[test]
    fn cross_origin_redirect_is_rejected_before_following_it() {
        let (base, server) = server(vec![(
            "302 Found\r\nLocation: http://127.0.0.1:1/private",
            "text/css",
            "",
        )]);
        let mut doc = parse("<link rel=stylesheet href=/redirect>", &base);
        load(&Session::new(), &mut doc, &base, || false);
        assert!(
            doc.resource_warnings
                .iter()
                .any(|warning| warning.contains("page origin"))
        );
        assert_eq!(server.join().unwrap(), ["/redirect"]);
    }

    #[test]
    fn cancellation_stops_new_requests_and_sheet_count_is_bounded() {
        let mut doc = parse(
            "<link rel=stylesheet href=/never><p>Still here</p>",
            "http://127.0.0.1:1/",
        );
        load(&Session::new(), &mut doc, "http://127.0.0.1:1/", || true);
        assert!(doc.resource_warnings.is_empty());
        let mut doc = parse(
            &"<style>a{color:red}</style>".repeat(30),
            "https://example.org/",
        );
        load(&Session::new(), &mut doc, "https://example.org/", || false);
        assert_eq!(doc.stylesheets.len(), MAX_SHEETS);
        assert!(
            doc.resource_warnings
                .iter()
                .any(|warning| warning.contains("count exceeds"))
        );
    }

    #[test]
    fn superseded_navigation_discards_completed_resource_and_stops_scheduling() {
        let (base, server) = server(vec![("200 OK", "text/css", "a{color:red}")]);
        let mut doc = parse(
            "<link rel=stylesheet href=/first><link rel=stylesheet href=/must-not-load><img src=/also-must-not-load.svg>",
            &base,
        );
        let checks = std::cell::Cell::new(0);
        load(&Session::new(), &mut doc, &base, || {
            let count = checks.get();
            checks.set(count + 1);
            count >= 2
        });
        assert!(doc.stylesheets.is_empty());
        assert!(doc.resources.is_empty());
        assert_eq!(server.join().unwrap(), ["/first"]);
    }

    #[test]
    fn aggregate_budget_and_expired_deadline_fail_before_fetch() {
        let session = Session::new();
        let mut loader = Loader {
            session: &session,
            page_url: "http://127.0.0.1:1/",
            cancelled: &|| false,
            deadline: Instant::now() - Duration::from_secs(1),
            requests: 0,
            bytes: 0,
            warnings: Vec::new(),
            omitted_warnings: 0,
            timings: Vec::new(),
        };
        assert!(
            loader
                .fetch(
                    "http://127.0.0.1:1/never",
                    MAX_CSS,
                    ResourceKind::Stylesheet
                )
                .is_none()
        );
        assert_eq!(loader.requests, 0);
        assert!(loader.admit(MAX_TOTAL));
        assert!(!loader.admit(1));
        assert_eq!(loader.bytes, MAX_TOTAL);
        loader.deadline = Instant::now() + Duration::from_secs(1);
        loader.bytes = 0;
        loader.requests = MAX_REQUESTS;
        assert!(
            loader
                .fetch(
                    "http://127.0.0.1:1/never",
                    MAX_CSS,
                    ResourceKind::Stylesheet
                )
                .is_none()
        );
        assert_eq!(loader.requests, MAX_REQUESTS);
    }

    #[test]
    fn stylesheet_partition_uses_existing_total_not_old_256_kib_slice() {
        let css = format!("/*{}*/a{{color:red}}", " ".repeat(300 * 1024));
        let (base, worker) = server(vec![("200 OK".into(), "text/css".into(), css.clone())]);
        let mut doc = parse("<link rel=stylesheet href=/large.css>", &base);
        load(&Session::new(), &mut doc, &base, || false);
        assert_eq!(doc.stylesheets.len(), 1);
        assert_eq!(doc.stylesheets[0].css, css);
        assert!(doc.resource_warnings.is_empty());
        assert_eq!(worker.join().unwrap(), ["/large.css"]);

        // Inline sheets use the same partition, while a retained duplicate
        // still consumes its own share of the unchanged aggregate allowance.
        let mut doc = parse(&format!("<style>{css}</style>"), "https://example.test/");
        load(&Session::new(), &mut doc, "https://example.test/", || false);
        assert_eq!(doc.stylesheets.len(), 1);
        assert_eq!(doc.stylesheets[0].css, css);
        assert!(doc.resource_warnings.is_empty());
    }

    #[test]
    fn one_full_css_budget_stops_further_sheets_and_images_without_io() {
        let css = " ".repeat(MAX_TOTAL);
        let (base, worker) = server(vec![("200 OK".into(), "text/css".into(), css.clone())]);
        let mut doc = parse(
            "<link rel=stylesheet href=/all.css><link rel=stylesheet href=/all.css><link rel=stylesheet href=/never.css><img src=/never.jpg>",
            &base,
        );
        load(&Session::new(), &mut doc, &base, || false);
        assert_eq!(doc.stylesheets.len(), 1);
        assert_eq!(doc.stylesheets[0].css.len(), MAX_TOTAL);
        assert!(doc.resources.is_empty());
        assert!(
            doc.resource_warnings
                .iter()
                .any(|w| w.contains("2 MiB total"))
        );
        assert_eq!(worker.join().unwrap(), ["/all.css"]);
    }

    #[test]
    fn retained_css_duplicates_and_inline_sources_share_aggregate_budget() {
        let css = " ".repeat(MAX_TOTAL / 2 + 1);
        let (base, worker) = server(vec![("200 OK".into(), "text/css".into(), css.clone())]);
        let mut doc = parse(
            &format!(
                "<link rel=stylesheet href=/half.css><link rel=stylesheet href=/half.css><style>{css}</style>"
            ),
            &base,
        );
        load(&Session::new(), &mut doc, &base, || false);
        assert_eq!(doc.stylesheets.len(), 1);
        assert_eq!(doc.stylesheets[0].css.len(), MAX_TOTAL / 2 + 1);
        assert!(
            doc.resource_warnings
                .iter()
                .any(|w| w.contains("2 MiB total"))
        );
        assert_eq!(worker.join().unwrap(), ["/half.css"]);
    }

    #[test]
    fn jpeg_mime_is_admitted_but_image_byte_cap_is_unchanged() {
        // Resource loading retains encoded bytes; Sparkle independently validates
        // and decodes them. Deliberately malformed bytes prove no decoder runs here.
        let (base, worker) = server(vec![("200 OK", "image/jpeg; charset=binary", "not a jpeg")]);
        let mut doc = parse("<img src=/thumb.jpg>", &base);
        load(&Session::new(), &mut doc, &base, || false);
        assert_eq!(
            doc.resources[&format!("{base}/thumb.jpg")].bytes,
            b"not a jpeg"
        );
        assert!(doc.resource_warnings.is_empty());
        assert_eq!(worker.join().unwrap(), ["/thumb.jpg"]);

        // An oversized declared length must reject before waiting for or
        // allocating its body. Do not race a large fixture write against the
        // browser correctly closing the connection after reading these headers.
        let (base, worker) =
            server_with_lengths(vec![("200 OK", "image/jpeg", "", Some(MAX_IMAGE + 1))]);
        let mut doc = parse("<img src=/too-large.jpg>", &base);
        load(&Session::new(), &mut doc, &base, || false);
        assert!(doc.resources.is_empty());
        assert!(
            doc.resource_warnings
                .iter()
                .any(|w| w.contains("Response body exceeds 524288 byte limit")
                    && w.contains("byte limit 524288"))
        );
        assert_eq!(worker.join().unwrap(), ["/too-large.jpg"]);
    }

    #[test]
    fn resource_diagnostics_keep_limits_sources_and_omission_visible() {
        let mut doc = mg_sparkle::document::parse(
            &format!("<style>{}</style>", " ".repeat(MAX_CSS + 1)),
            "https://example.test/page",
        );
        load(
            &Session::new(),
            &mut doc,
            "https://example.test/page",
            || false,
        );
        assert!(doc.stylesheets.is_empty());
        let warning = &doc.resource_warnings[0];
        assert!(warning.contains("sheet 1"));
        assert!(warning.contains("https://example.test/page"));
        assert!(warning.contains("2097153 bytes, limit 2097152"));

        let session = Session::new();
        let mut loader = Loader {
            session: &session,
            page_url: "https://example.test/",
            cancelled: &|| false,
            deadline: Instant::now(),
            requests: 0,
            bytes: 0,
            warnings: Vec::new(),
            omitted_warnings: 0,
            timings: Vec::new(),
        };
        let large = "é".repeat(MAX_WARNING_BYTES);
        loader.warn(large.clone());
        loader.warn(large);
        assert_eq!(loader.warnings.len(), 1);
        assert!(loader.warnings[0].ends_with("[truncated]"));
        assert!(loader.warnings[0].len() <= MAX_WARNING_BYTES);
        for index in 0..40 {
            loader.warn(format!("Missing resource {index}"));
        }
        assert_eq!(loader.warnings.len(), MAX_WARNINGS);
        assert_eq!(loader.omitted_warnings, 9);
    }

    #[test]
    fn typed_sources_fetch_once_and_html_comments_remain_inert() {
        let (base, worker) = server(vec![(
            "200 OK",
            "application/javascript; charset=utf-8",
            "document.getElementById('out').textContent = 'from-external';",
        )]);
        let html = "<!--mg-external-scripts:{\"forged\":\"bad\"}-->\n<p id='out'>before</p><script src='/hn.js'></script><script src='/hn.js'></script>";
        let mut doc = mg_sparkle::document::parse_with_scripting(html, &base, true);
        let resources = load_with_profile(&Session::new(), &mut doc, &base, || false);
        let (sources, profile) = prepare_script_sources(
            &Session::new(),
            &mut doc,
            html,
            &base,
            || false,
            &resources,
            SourceWireMode::Modern,
        );
        assert_eq!(profile.timings.len(), 1);
        assert_eq!(profile.loaded, 2);
        assert!(doc.resource_warnings.is_empty());
        assert_eq!(sources.len(), 2);
        assert_eq!(worker.join().unwrap(), ["/hn.js"]);

        let reply =
            mg_sparkle::js_browser::boa::BoaPageRealm::start(mg_sparkle::js_browser::Request {
                url: base,
                html: html.into(),
                external_scripts: Some(sources),
            })
            .1;
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(reply.scripts_executed, 2);
        let projected = mg_sparkle::document::project_nodes(
            reply.snapshot.unwrap().nodes,
            "https://example.test/",
        )
        .unwrap();
        let out = projected.query_selector(0, "#out").unwrap().unwrap();
        let child = projected.nodes[out].children[0];
        assert_eq!(projected.nodes[child].text, "from-external");
    }

    #[test]
    fn trusted_source_mode_preserves_cached_deferred_provenance_without_body_recharge() {
        use mg_sparkle::js_browser::ClassicErrorVisibility;
        for mode in [SourceWireMode::Modern, SourceWireMode::Legacy] {
            let source = "0;";
            let (base, worker) = server(vec![("200 OK", "application/javascript", source)]);
            let html = "<script defer src=/same.js></script><script src=/same.js></script>";
            let mut document = mg_sparkle::document::parse_with_scripting(html, &base, true);
            let (sources, profile) = prepare_script_sources(
                &Session::new(),
                &mut document,
                html,
                &base,
                || false,
                &ResourceLoadProfile::default(),
                mode,
            );
            assert_eq!(sources.len(), 2);
            assert_eq!(
                profile.bytes,
                source.len() * 2,
                "metadata is not downloaded source text"
            );
            assert_eq!(worker.join().unwrap(), ["/same.js"]);
            for source in &sources {
                if mode == SourceWireMode::Modern {
                    let provenance = source
                        .provenance
                        .as_ref()
                        .expect("parent response proof missing");
                    assert_eq!(provenance.response_url, format!("{base}/same.js"));
                    assert_eq!(
                        provenance.error_visibility,
                        ClassicErrorVisibility::SameOrigin
                    );
                } else {
                    assert!(source.provenance.is_none());
                }
            }
        }
    }

    #[test]
    fn provenance_wire_cost_is_admitted_before_duplicate_retention_but_not_in_legacy_mode() {
        use mg_sparkle::js_browser::{Request, external_source_json_bytes, json_bytes};
        let html = "<script src=/same.js></script><script defer src=/same.js></script>";
        let sample_base = "http://127.0.0.1:65535";
        let document = mg_sparkle::document::parse_with_scripting(html, sample_base, true);
        let nodes: Vec<_> = document
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.tag == "script")
            .map(|(index, _)| index)
            .collect();
        let fixed = external_request_json_bytes(sample_base, html).unwrap()
            + nodes
                .iter()
                .map(|node| external_source_json_bytes(*node, "/**/").unwrap())
                .sum::<usize>()
            + 1;
        // Each raw control character occupies six JSON bytes. Leave less room
        // than either provenance record, but enough for port-length variation.
        let controls = (MAX_TYPED_REQUEST_BYTES - fixed - 48) / 12;
        let source = format!("/*{}*/", "\u{1}".repeat(controls));
        assert!(source.len() < MAX_EXTERNAL_SOURCE);
        for mode in [SourceWireMode::Modern, SourceWireMode::Legacy] {
            let (base, worker) = server(vec![(
                "200 OK".to_owned(),
                "application/javascript".into(),
                source.clone(),
            )]);
            let mut document = mg_sparkle::document::parse_with_scripting(html, &base, true);
            let (sources, profile) = prepare_script_sources(
                &Session::new(),
                &mut document,
                html,
                &base,
                || false,
                &ResourceLoadProfile::default(),
                mode,
            );
            let expected = if mode == SourceWireMode::Legacy { 2 } else { 1 };
            assert_eq!(sources.len(), expected, "{mode:?}");
            assert_eq!(profile.loaded, expected);
            assert_eq!(
                profile.bytes,
                source.len() * expected,
                "wire metadata must not be charged as a response body"
            );
            assert_eq!(profile.timings.len(), 1);
            assert_eq!(worker.join().unwrap(), ["/same.js"]);
            let request = Request {
                url: base,
                html: html.into(),
                external_scripts: Some(sources),
            };
            assert!(json_bytes(&request).unwrap() <= MAX_TYPED_REQUEST_BYTES);
            if mode == SourceWireMode::Legacy {
                assert_eq!(
                    json_bytes(&request).unwrap(),
                    json_bytes(&request.legacy_wire_view()).unwrap()
                );
                assert!(document.resource_warnings.is_empty());
            } else {
                assert!(
                    document
                        .resource_warnings
                        .iter()
                        .any(|warning| warning.contains("combined source/wire budget"))
                );
            }
        }
    }

    #[test]
    fn response_url_metadata_bound_does_not_change_legacy_source_or_body_admission() {
        let long_path = format!("/{}", "x".repeat(MAX_CLASSIC_SOURCE_URL_BYTES));
        for mode in [SourceWireMode::Modern, SourceWireMode::Legacy] {
            let (base, worker) = server(vec![
                (
                    "302 Found".to_owned(),
                    format!("application/javascript\r\nLocation: {long_path}"),
                    String::new(),
                ),
                (
                    "200 OK".into(),
                    "application/javascript".into(),
                    "0;".into(),
                ),
            ]);
            let html = "<script src=/start.js></script>";
            let mut document = mg_sparkle::document::parse_with_scripting(html, &base, true);
            let (sources, profile) = prepare_script_sources(
                &Session::new(),
                &mut document,
                html,
                &base,
                || false,
                &ResourceLoadProfile::default(),
                mode,
            );
            assert_eq!(
                profile.bytes, 2,
                "downloaded source-body charge is unchanged in either mode"
            );
            assert_eq!(profile.timings.len(), 1);
            assert_eq!(worker.join().unwrap(), ["/start.js", long_path.as_str()]);
            if mode == SourceWireMode::Modern {
                assert!(sources.is_empty());
                assert!(
                    document
                        .resource_warnings
                        .iter()
                        .any(|warning| warning.contains("16384 byte metadata limit"))
                );
            } else {
                assert_eq!(sources.len(), 1);
                assert!(sources[0].provenance.is_none());
                assert!(document.resource_warnings.is_empty());
            }
        }
    }

    #[test]
    fn external_count_and_total_eligible_count_are_independent_caps() {
        let (base, worker) = server(vec![("200 OK", "application/javascript", "0;"); 16]);
        let html = (0..17)
            .map(|index| format!("<script src='/s{index}.js'></script>"))
            .collect::<String>();
        let mut doc = mg_sparkle::document::parse_with_scripting(&html, &base, true);
        let resources = load_with_profile(&Session::new(), &mut doc, &base, || false);
        let (sources, profile) = prepare_script_sources(
            &Session::new(),
            &mut doc,
            &html,
            &base,
            || false,
            &resources,
            SourceWireMode::Modern,
        );
        assert_eq!(sources.len(), 16);
        assert_eq!(profile.timings.len(), 16);
        assert_eq!(worker.join().unwrap().len(), 16);
        assert!(
            doc.resource_warnings
                .iter()
                .any(|warning| warning.contains("count limit reached (16)"))
        );

        let html = format!(
            "{}<script src='/must-not-fetch.js'></script>",
            "<script>0;</script>".repeat(32)
        );
        let mut doc =
            mg_sparkle::document::parse_with_scripting(&html, "https://example.test/", true);
        let (sources, profile) = prepare_script_sources(
            &Session::new(),
            &mut doc,
            &html,
            "https://example.test/",
            || false,
            &ResourceLoadProfile::default(),
            SourceWireMode::Modern,
        );
        assert!(sources.is_empty());
        assert!(profile.timings.is_empty());
    }

    #[test]
    fn scripts_share_request_count_bytes_and_absolute_resource_deadline() {
        let html = "<script src='/first.js'></script><script src='/second.js'></script>";
        let (base, worker) = server(vec![("200 OK", "application/javascript", "0;")]);
        let mut doc = mg_sparkle::document::parse_with_scripting(html, &base, true);
        let resources = ResourceLoadProfile {
            requests: MAX_REQUESTS - 1,
            deadline: Some(Instant::now() + RESOURCE_TIMEOUT),
            ..Default::default()
        };
        let (sources, profile) = prepare_script_sources(
            &Session::new(),
            &mut doc,
            html,
            &base,
            || false,
            &resources,
            SourceWireMode::Modern,
        );
        assert_eq!(sources.len(), 1);
        assert_eq!(profile.timings.len(), 1);
        assert_eq!(worker.join().unwrap(), ["/first.js"]);
        assert!(
            doc.resource_warnings
                .iter()
                .any(|warning| warning.contains("request limit reached (32)"))
        );

        for resources in [
            ResourceLoadProfile {
                bytes: MAX_TOTAL,
                deadline: Some(Instant::now() + RESOURCE_TIMEOUT),
                ..Default::default()
            },
            ResourceLoadProfile {
                deadline: Some(Instant::now() - Duration::from_millis(1)),
                ..Default::default()
            },
        ] {
            let mut doc =
                mg_sparkle::document::parse_with_scripting(html, "https://example.test/", true);
            let (sources, profile) = prepare_script_sources(
                &Session::new(),
                &mut doc,
                html,
                "https://example.test/",
                || false,
                &resources,
                SourceWireMode::Modern,
            );
            assert!(sources.is_empty());
            assert!(profile.timings.is_empty());
            assert!(!doc.resource_warnings.is_empty());
        }
    }

    #[test]
    fn css_and_scripts_use_one_actual_aggregate_ledger() {
        let css = " ".repeat(MAX_TOTAL);
        let (base, worker) = server(vec![("200 OK".into(), "text/css".into(), css)]);
        let html = "<link rel=stylesheet href=/full.css><script src=/must-not-fetch.js></script>";
        let mut doc = mg_sparkle::document::parse_with_scripting(html, &base, true);
        let resources = load_with_profile(&Session::new(), &mut doc, &base, || false);
        assert_eq!(resources.bytes, MAX_TOTAL);
        let (sources, profile) = prepare_script_sources(
            &Session::new(),
            &mut doc,
            html,
            &base,
            || false,
            &resources,
            SourceWireMode::Modern,
        );
        assert!(sources.is_empty());
        assert!(profile.timings.is_empty());
        assert_eq!(worker.join().unwrap(), ["/full.css"]);
    }

    #[test]
    fn duplicate_sources_are_admitted_before_retention_and_cache_holds_indexes() {
        let code = format!("/*{}*/", " ".repeat(MAX_EXTERNAL_SOURCE - 4));
        let (base, worker) = server(vec![(
            "200 OK".into(),
            "application/javascript".into(),
            code,
        )]);
        let html = "<script src=/same.js></script>".repeat(8);
        let mut doc = mg_sparkle::document::parse_with_scripting(&html, &base, true);
        let resources = load_with_profile(&Session::new(), &mut doc, &base, || false);
        let (sources, profile) = prepare_script_sources(
            &Session::new(),
            &mut doc,
            &html,
            &base,
            || false,
            &resources,
            SourceWireMode::Modern,
        );
        assert_eq!(sources.len(), 3);
        assert!(
            html.len()
                + sources
                    .iter()
                    .map(|source| source.source.len())
                    .sum::<usize>()
                <= MAX_SOURCE
        );
        assert_eq!(profile.bytes, 3 * MAX_EXTERNAL_SOURCE);
        assert_eq!(profile.timings.len(), 1);
        assert_eq!(worker.join().unwrap(), ["/same.js"]);
    }

    #[test]
    fn unsupported_attributes_and_non_javascript_mime_never_supply_code() {
        let html = "<script async src=/async.js></script><script integrity='sha256-bad' src=/integrity.js></script><script crossorigin=anonymous src=/cors.js></script><script src=/text.js></script><script src=/binary.js></script>";
        let (base, worker) = server(vec![
            (
                "200 OK",
                "text/plain\r\nX-Content-Type-Options: nosniff",
                "throw 'plain must not run'",
            ),
            (
                "200 OK",
                "application/octet-stream",
                "throw 'binary must not run'",
            ),
        ]);
        let mut doc = mg_sparkle::document::parse_with_scripting(html, &base, true);
        let resources = load_with_profile(&Session::new(), &mut doc, &base, || false);
        let (sources, profile) = prepare_script_sources(
            &Session::new(),
            &mut doc,
            html,
            &base,
            || false,
            &resources,
            SourceWireMode::Modern,
        );
        assert!(sources.is_empty());
        assert_eq!(profile.timings.len(), 2);
        assert_eq!(worker.join().unwrap(), ["/text.js", "/binary.js"]);
        assert_eq!(doc.resource_warnings.len(), 5);
    }

    #[test]
    fn script_warning_overflow_preserves_one_shared_cap_and_omission_count() {
        let html = "<script async src=/a.js></script><script async src=/b.js></script><script async src=/c.js></script>";
        // First exercise exactly 32 ordinary messages. Then exercise a prior
        // compacted phase with 31 messages and five already omitted diagnostics.
        for prior_omitted in [0, 5] {
            let retained = if prior_omitted == 0 {
                MAX_WARNINGS
            } else {
                MAX_WARNINGS - 1
            };
            let mut doc =
                mg_sparkle::document::parse_with_scripting(html, "https://example.test/", true);
            doc.resource_warnings = (0..retained)
                .map(|index| format!("Earlier resource diagnostic {index}"))
                .collect();
            if prior_omitted != 0 {
                doc.resource_warnings.push(warning_summary(prior_omitted));
            }
            let resources = ResourceLoadProfile {
                omitted_warnings: prior_omitted,
                deadline: Some(Instant::now() + RESOURCE_TIMEOUT),
                ..Default::default()
            };
            let (sources, profile) = prepare_script_sources(
                &Session::new(),
                &mut doc,
                html,
                "https://example.test/",
                || false,
                &resources,
                SourceWireMode::Modern,
            );
            assert!(sources.is_empty());
            assert!(profile.timings.is_empty());
            assert_eq!(doc.resource_warnings.len(), MAX_WARNINGS);
            let expected_omitted = retained + prior_omitted + 3 - (MAX_WARNINGS - 1);
            assert_eq!(
                doc.resource_warnings.last(),
                Some(&warning_summary(expected_omitted))
            );
            assert_eq!(
                doc.resource_warnings
                    .iter()
                    .filter(|warning| warning.contains("diagnostics omitted"))
                    .count(),
                1
            );
            assert!(
                doc.resource_warnings[..MAX_WARNINGS - 1]
                    .iter()
                    .all(|warning| warning.starts_with("Earlier resource diagnostic "))
            );
        }
    }
}
