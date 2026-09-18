//! Small, bounded author stylesheet and image loading. No external scripts,
//! imports, fonts or data URLs are enabled. Stylesheets remain same-origin;
//! cross-origin HTTPS images are credential-free, including later redirects.
//! All fetches use the browser session's verified Rust TLS transport.

use crate::net::{ResourceKind, Response, Session};
use mg_sparkle::document::{Document, ResourceData, StylesheetRequest, StylesheetSource};
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

/// Populate source-ordered author CSS and small image resources while keeping
/// failures nonfatal. Cancellation stops scheduling; an already running bounded
/// socket/resolver operation may finish before cancellation is observed.
pub fn load(
    session: &Session,
    document: &mut Document,
    page_url: &str,
    cancelled: impl Fn() -> bool,
) {
    let requests = document.stylesheet_requests();
    document.stylesheets.clear();
    document.resources.clear();
    document.resource_warnings.clear();
    let mut loader = Loader {
        session,
        page_url,
        cancelled: &cancelled,
        deadline: Instant::now() + RESOURCE_TIMEOUT,
        requests: 0,
        bytes: 0,
        warnings: Vec::new(),
        omitted_warnings: 0,
    };
    let mut image_urls = Vec::new();
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
    if loader.omitted_warnings > 0 {
        // Retain the existing count bound while making overflow visible.
        if loader.warnings.len() == MAX_WARNINGS {
            loader.warnings.pop();
            loader.omitted_warnings = loader.omitted_warnings.saturating_add(1);
        }
        loader.warnings.push(format!(
            "{} additional resource diagnostics omitted (limit {MAX_WARNINGS})",
            loader.omitted_warnings
        ));
    }
    document.resource_warnings = loader.warnings;
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
}

impl<F: Fn() -> bool> Loader<'_, F> {
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
        match self.session.fetch_resource(
            url,
            self.page_url,
            limit.min(MAX_TOTAL - self.bytes),
            self.deadline,
            kind,
        ) {
            Ok(response) if (200..300).contains(&response.status) => {
                if self.stopped() || !self.admit(response.body.len()) {
                    None
                } else {
                    Some(response)
                }
            }
            Ok(response) => {
                self.admit(response.body.len());
                self.warn(format!(
                    "Resource returned HTTP {}; skipped: {}",
                    response.status,
                    label(url)
                ));
                None
            }
            Err(error) => {
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
}
