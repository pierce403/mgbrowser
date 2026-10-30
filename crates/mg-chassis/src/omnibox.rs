//! Chrome-style Omnibox URL/search resolution, autocomplete suggestions, and internal `chrome://` pages.

use crate::bookmarks::Bookmark;
use crate::downloads::{DownloadEntry, DownloadState};
use crate::history::HistoryEntry;

pub const MAX_OMNIBOX_SUGGESTIONS: usize = 6;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchEngine {
    #[default]
    DuckDuckGo,
    Google,
    Wikipedia,
}

impl SearchEngine {
    pub fn label(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Google => "Google",
            Self::Wikipedia => "Wikipedia",
        }
    }

    pub fn search_url(self, query: &str) -> String {
        let encoded: String =
            url::form_urlencoded::byte_serialize(query.trim().as_bytes()).collect();
        match self {
            Self::DuckDuckGo => format!("https://html.duckduckgo.com/html/?q={encoded}"),
            Self::Google => format!("https://www.google.com/search?q={encoded}"),
            Self::Wikipedia => format!("https://en.wikipedia.org/w/index.php?search={encoded}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InternalPage {
    Blank,
    NewTab,
    History,
    Bookmarks,
    Downloads,
    Settings,
    Version,
}

impl InternalPage {
    pub fn canonical_url(self) -> &'static str {
        match self {
            Self::Blank => "about:blank",
            Self::NewTab => "chrome://newtab",
            Self::History => "chrome://history",
            Self::Bookmarks => "chrome://bookmarks",
            Self::Downloads => "chrome://downloads",
            Self::Settings => "chrome://settings",
            Self::Version => "chrome://version",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        let lower = trimmed.to_ascii_lowercase();
        let clean = lower.trim_end_matches('/');
        match clean {
            "about:blank" => Some(Self::Blank),
            "about:newtab" | "chrome://newtab" | "mg://newtab" => Some(Self::NewTab),
            "about:history" | "chrome://history" | "mg://history" => Some(Self::History),
            "about:bookmarks" | "chrome://bookmarks" | "mg://bookmarks" => Some(Self::Bookmarks),
            "about:downloads" | "chrome://downloads" | "mg://downloads" => Some(Self::Downloads),
            "about:settings" | "chrome://settings" | "mg://settings" => Some(Self::Settings),
            "about:version" | "about:about" | "chrome://version" | "chrome://about"
            | "mg://version" => Some(Self::Version),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OmniboxResolution {
    Internal(InternalPage),
    Url(String),
}

/// Resolve user input typed into the Omnibox into an internal page, HTTP(S) URL, or web search URL.
pub fn resolve_omnibox_input(raw: &str, engine: SearchEngine) -> Option<OmniboxResolution> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(page) = InternalPage::parse(trimmed) {
        return Some(OmniboxResolution::Internal(page));
    }
    if trimmed.contains("://") {
        return Some(OmniboxResolution::Url(trimmed.to_owned()));
    }
    // Any whitespace means search query.
    if trimmed.chars().any(char::is_whitespace) {
        return Some(OmniboxResolution::Url(engine.search_url(trimmed)));
    }
    // Check host[:port][/path] shapes.
    let host_port = trimmed.split(['/', '?', '#']).next().unwrap_or(trimmed);
    if is_local_host_port(host_port) {
        return Some(OmniboxResolution::Url(format!("http://{trimmed}")));
    }
    if looks_like_domain_or_ip(host_port) {
        return Some(OmniboxResolution::Url(format!("https://{trimmed}")));
    }
    Some(OmniboxResolution::Url(engine.search_url(trimmed)))
}

fn is_local_host_port(host_port: &str) -> bool {
    let lower = host_port.to_ascii_lowercase();
    if lower == "localhost" || lower.starts_with("localhost:") {
        return true;
    }
    if lower.starts_with("[::1]") {
        return true;
    }
    let host = lower.split(':').next().unwrap_or(&lower);
    host == "127.0.0.1" || host == "0.0.0.0"
}

fn looks_like_domain_or_ip(host_port: &str) -> bool {
    if host_port.starts_with('[') && host_port.contains(']') {
        return true;
    }
    let (host, port_opt) = match host_port.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h, Some(p)),
        _ => (host_port, None),
    };
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return false;
    }
    if !host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
    {
        return false;
    }
    if port_opt.is_some() {
        return true;
    }
    if !host.contains('.') {
        return false;
    }
    let tld = host.rsplit('.').next().unwrap_or("");
    // Either IPv4 (all digits) or alphabetic TLD >= 2 chars.
    let is_ipv4 = host
        .split('.')
        .all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_digit()));
    let valid_tld = tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic());
    is_ipv4 || valid_tld
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SuggestionKind {
    Navigate,
    Search,
    Bookmark,
    History,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OmniboxSuggestion {
    pub kind: SuggestionKind,
    pub label: String,
    pub target: String,
}

#[derive(Default)]
pub(crate) struct OmniboxState {
    pub open: bool,
    pub selected: Option<usize>,
    pub suggestions: Vec<OmniboxSuggestion>,
}

pub fn build_suggestions(
    input: &str,
    engine: SearchEngine,
    bookmarks: &[Bookmark],
    history: &[HistoryEntry],
) -> Vec<OmniboxSuggestion> {
    let q = input.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let q_low = q.to_ascii_lowercase();
    let mut out = Vec::with_capacity(MAX_OMNIBOX_SUGGESTIONS);

    if let Some(res) = resolve_omnibox_input(q, engine) {
        match res {
            OmniboxResolution::Internal(page) => {
                let url = page.canonical_url().to_owned();
                out.push(OmniboxSuggestion {
                    kind: SuggestionKind::Internal,
                    label: format!("Open {url}"),
                    target: url,
                });
            }
            OmniboxResolution::Url(url) => {
                let is_search = url.starts_with("https://html.duckduckgo.com/")
                    || url.starts_with("https://www.google.com/search?")
                    || url.starts_with("https://en.wikipedia.org/w/index.php?search=");
                out.push(OmniboxSuggestion {
                    kind: if is_search {
                        SuggestionKind::Search
                    } else {
                        SuggestionKind::Navigate
                    },
                    label: if is_search {
                        format!("Search {} for \"{q}\"", engine.label())
                    } else {
                        url.clone()
                    },
                    target: url,
                });
            }
        }
    }

    if q_low.starts_with("chrome") || q_low.starts_with("about:") || q_low.starts_with("mg:") {
        for page in [
            InternalPage::NewTab,
            InternalPage::History,
            InternalPage::Bookmarks,
            InternalPage::Downloads,
            InternalPage::Settings,
            InternalPage::Version,
        ] {
            if out.len() >= MAX_OMNIBOX_SUGGESTIONS {
                break;
            }
            let url = page.canonical_url();
            if url.contains(&q_low) && !out.iter().any(|s| s.target == url) {
                out.push(OmniboxSuggestion {
                    kind: SuggestionKind::Internal,
                    label: url.to_owned(),
                    target: url.to_owned(),
                });
            }
        }
    }

    for bm in bookmarks {
        if out.len() >= MAX_OMNIBOX_SUGGESTIONS {
            break;
        }
        if (bm.url.to_ascii_lowercase().contains(&q_low)
            || bm.title.to_ascii_lowercase().contains(&q_low))
            && !out.iter().any(|s| s.target == bm.url)
        {
            out.push(OmniboxSuggestion {
                kind: SuggestionKind::Bookmark,
                label: format!("★ {} — {}", bm.title, bm.url),
                target: bm.url.clone(),
            });
        }
    }

    for entry in history {
        if out.len() >= MAX_OMNIBOX_SUGGESTIONS {
            break;
        }
        if (entry.url.to_ascii_lowercase().contains(&q_low)
            || entry.title.to_ascii_lowercase().contains(&q_low))
            && !out.iter().any(|s| s.target == entry.url)
        {
            out.push(OmniboxSuggestion {
                kind: SuggestionKind::History,
                label: format!("{} — {}", entry.title, entry.url),
                target: entry.url.clone(),
            });
        }
    }

    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Generate clean, styled HTML for internal `chrome://` and `about:` pages.
pub fn render_internal_html(
    page: InternalPage,
    about_lines: &[String],
    bookmarks: &[Bookmark],
    history: &[HistoryEntry],
    downloads: &[DownloadEntry],
    engine: SearchEngine,
    theme_summary: &str,
    scale_percent: u16,
    scripts_enabled: bool,
) -> String {
    let style = "<style>
        body { font-family: sans-serif; margin: 28px 36px; background: #fafbf8; color: #1f2a22; line-height: 1.5; }
        h1 { font-size: 24px; margin: 0 0 8px 0; color: #1b3b22; }
        h2 { font-size: 16px; margin: 20px 0 8px 0; color: #2d4a33; }
        p.sub { color: #56665b; margin: 0 0 18px 0; font-size: 14px; }
        .nav { margin: 0 0 20px 0; padding: 10px 14px; background: #eef2ec; border: 1px solid #d5ddd2; }
        .nav a { margin-right: 16px; color: #1d5c2e; text-decoration: none; font-weight: bold; font-size: 13px; }
        .card { background: #ffffff; border: 1px solid #d8e0d5; padding: 12px 16px; margin-bottom: 10px; }
        .muted { color: #5c6b60; font-size: 13px; }
        .badge { display: inline-block; padding: 2px 8px; background: #e5efe6; color: #1f5129; font-size: 12px; font-weight: bold; }
        input { padding: 6px 10px; font-size: 15px; border: 1px solid #b5c2b7; width: 340px; }
        button { padding: 6px 14px; font-size: 14px; background: #2c5e34; color: #ffffff; border: none; }
    </style>";
    let nav = "<div class=\"nav\">\
        <a href=\"chrome://newtab\">New Tab</a>\
        <a href=\"chrome://bookmarks\">Bookmarks</a>\
        <a href=\"chrome://history\">History</a>\
        <a href=\"chrome://downloads\">Downloads</a>\
        <a href=\"chrome://settings\">Settings</a>\
        <a href=\"chrome://version\">Version</a>\
    </div>";

    match page {
        InternalPage::Blank => String::new(),
        InternalPage::NewTab => {
            let mut body = format!(
                "<!doctype html><title>New Tab</title>{style}<body>{nav}\
                <h1>mgbrowser</h1>\
                <p class=\"sub\">Pure-Rust desktop browser · Search with {} or enter any URL in the address bar.</p>\
                <form action=\"https://html.duckduckgo.com/html/\" method=\"get\">\
                  <input name=\"q\" placeholder=\"Search the web...\">\
                  <button type=\"submit\">Search</button>\
                </form>",
                html_escape(engine.label())
            );
            if !bookmarks.is_empty() {
                body.push_str("<h2>Saved Bookmarks</h2>");
                for bm in bookmarks.iter().take(12) {
                    body.push_str(&format!(
                        "<div class=\"card\"><a href=\"{}\">{}</a> <span class=\"muted\">{}</span></div>",
                        html_escape(&bm.url),
                        html_escape(&bm.title),
                        html_escape(&bm.url)
                    ));
                }
            }
            if !history.is_empty() {
                body.push_str("<h2>Recent History</h2>");
                for h in history.iter().take(8) {
                    body.push_str(&format!(
                        "<div class=\"card\"><a href=\"{}\">{}</a> <span class=\"muted\">— {} ({} visit{})</span></div>",
                        html_escape(&h.url),
                        html_escape(&h.title),
                        html_escape(&h.url),
                        h.visits,
                        if h.visits == 1 { "" } else { "s" }
                    ));
                }
            }
            body.push_str("</body>");
            body
        }
        InternalPage::History => {
            let mut body = format!(
                "<!doctype html><title>History</title>{style}<body>{nav}\
                <h1>Browsing History ({})</h1>\
                <p class=\"sub\">Press Ctrl+H for the quick History dialog, or Ctrl+Shift+Delete to clear browsing data.</p>",
                history.len()
            );
            if history.is_empty() {
                body.push_str("<div class=\"card\">No browsing history recorded yet.</div>");
            } else {
                for entry in history.iter().take(120) {
                    body.push_str(&format!(
                        "<div class=\"card\"><a href=\"{}\">{}</a><div class=\"muted\">{} · {} visit{}</div></div>",
                        html_escape(&entry.url),
                        html_escape(&entry.title),
                        html_escape(&entry.url),
                        entry.visits,
                        if entry.visits == 1 { "" } else { "s" }
                    ));
                }
            }
            body.push_str("</body>");
            body
        }
        InternalPage::Bookmarks => {
            let mut body = format!(
                "<!doctype html><title>Bookmarks</title>{style}<body>{nav}\
                <h1>Bookmarks ({})</h1>\
                <p class=\"sub\">Press Ctrl+D to bookmark any HTTP(S) page, Ctrl+Shift+B to toggle the Bookmarks Bar, or Ctrl+Shift+O to manage bookmarks.</p>",
                bookmarks.len()
            );
            if bookmarks.is_empty() {
                body.push_str("<div class=\"card\">No bookmarks saved yet.</div>");
            } else {
                for bm in bookmarks {
                    body.push_str(&format!(
                        "<div class=\"card\"><a href=\"{}\">{}</a><div class=\"muted\">{}</div></div>",
                        html_escape(&bm.url),
                        html_escape(&bm.title),
                        html_escape(&bm.url)
                    ));
                }
            }
            body.push_str("</body>");
            body
        }
        InternalPage::Downloads => {
            let mut body = format!(
                "<!doctype html><title>Downloads</title>{style}<body>{nav}\
                <h1>Downloads ({})</h1>\
                <p class=\"sub\">Press Ctrl+J to open the Downloads manager, or Ctrl+S to save any page.</p>",
                downloads.len()
            );
            if downloads.is_empty() {
                body.push_str("<div class=\"card\">No downloads in this session yet.</div>");
            } else {
                for dl in downloads.iter().rev() {
                    let state_str = match dl.state {
                        DownloadState::InProgress => "Downloading...".to_owned(),
                        DownloadState::Completed => format!(
                            "Saved ({} bytes) → {}",
                            dl.bytes,
                            dl.saved_path.as_deref().unwrap_or("(in-memory)")
                        ),
                        DownloadState::Failed => {
                            format!("Failed: {}", dl.error.as_deref().unwrap_or("unknown error"))
                        }
                    };
                    body.push_str(&format!(
                        "<div class=\"card\"><strong>{}</strong> <span class=\"badge\">{}</span>\
                        <div class=\"muted\">{} · {}</div></div>",
                        html_escape(&dl.filename),
                        html_escape(&dl.mime),
                        html_escape(&state_str),
                        html_escape(&dl.url)
                    ));
                }
            }
            body.push_str("</body>");
            body
        }
        InternalPage::Settings => format!(
            "<!doctype html><title>Settings</title>{style}<body>{nav}\
            <h1>Browser Settings</h1>\
            <p class=\"sub\">Open Menu ≡ → Settings for interactive Theme, Zoom, Search Engine, and Bookmarks Bar controls.</p>\
            <div class=\"card\"><strong>Appearance:</strong> {}</div>\
            <div class=\"card\"><strong>Page &amp; UI Zoom:</strong> {}% (Ctrl++ / Ctrl+- / Ctrl+0)</div>\
            <div class=\"card\"><strong>Default Omnibox Search Engine:</strong> {}</div>\
            <div class=\"card\"><strong>JavaScript Sandbox:</strong> {} (restricted Linux seccomp worker)</div>\
            <div class=\"card\"><strong>Privacy &amp; Data:</strong> Press Ctrl+Shift+Delete to clear History, Cookies, Network log, and Downloads list.</div>\
            </body>",
            html_escape(theme_summary),
            scale_percent,
            html_escape(engine.label()),
            if scripts_enabled {
                "Enabled"
            } else {
                "Disabled"
            }
        ),
        InternalPage::Version => {
            let mut body = format!(
                "<!doctype html><title>About Version</title>{style}<body>{nav}\
                <h1>About mgbrowser</h1>\
                <p class=\"sub\">100% pure-Rust browser engine, Stylo CSS, Boa JS worker sandbox, and native X11 desktop host.</p>"
            );
            for line in about_lines {
                body.push_str(&format!("<div class=\"card\">{}</div>", html_escape(line)));
            }
            body.push_str(&format!(
                "<div class=\"card\"><strong>User-Agent:</strong> {}</div></body>",
                html_escape(crate::net::USER_AGENT)
            ));
            body
        }
    }
}

impl crate::Browser {
    pub fn set_search_engine(&mut self, engine: SearchEngine) {
        if self.search_engine != engine {
            self.search_engine = engine;
            self.dirty = true;
        }
    }

    pub fn search_engine(&self) -> SearchEngine {
        self.search_engine
    }

    pub fn take_search_engine_change(&mut self) -> Option<SearchEngine> {
        self.search_engine_change.take()
    }

    pub fn set_bookmarks_bar(&mut self, visible: bool) {
        if self.bookmarks_bar != visible {
            self.bookmarks_bar = visible;
            self.invalidate_scaled_geometry();
        }
    }

    pub fn bookmarks_bar(&self) -> bool {
        self.bookmarks_bar
    }

    pub fn take_bookmarks_bar_change(&mut self) -> Option<bool> {
        self.bookmarks_bar_change.take()
    }

    pub(crate) fn refresh_omnibox_suggestions(&mut self) {
        if !self.chrome || self.focus != crate::Focus::Address {
            self.omnibox.open = false;
            self.omnibox.selected = None;
            self.omnibox.suggestions.clear();
            return;
        }
        self.omnibox.suggestions = build_suggestions(
            &self.address,
            self.search_engine,
            &self.bookmarks,
            &self.browsing_history,
        );
        self.omnibox.open = !self.omnibox.suggestions.is_empty();
        self.omnibox.selected = None;
    }

    pub(crate) fn submit_omnibox(&mut self, raw: &str) {
        self.omnibox.open = false;
        self.omnibox.selected = None;
        let Some(resolution) = resolve_omnibox_input(raw, self.search_engine) else {
            return;
        };
        match resolution {
            OmniboxResolution::Internal(page) => self.load_internal_page(page, true),
            OmniboxResolution::Url(url) => self.navigate(url, None, true),
        }
    }

    pub(crate) fn load_internal_page(&mut self, page: InternalPage, add_history: bool) {
        self.stop_scroll();
        self.omnibox.open = false;
        self.omnibox.selected = None;
        if self.loading {
            self.finish_initial_load(Some("net::ERR_ABORTED: superseded navigation".into()));
        }
        self.generation += 1;
        self.finish_automation(
            Err("Runtime context destroyed by navigation".into()),
            crate::scripts::SessionContinuation::Terminal,
        );
        self.resource_generation
            .store(self.generation, std::sync::atomic::Ordering::Relaxed);
        let _ = self.child_pool.set_generation(self.generation);
        if let Some(session) = self.page_session.take() {
            session.cancel();
        }
        self.pending_transaction = None;
        self.console_detached_dom = false;
        self.queued_console.clear();
        self.task_wake = None;
        self.last_task_wake_token = 0;
        self.script_blocked = false;
        self.lifecycle = None;
        self.observer_ledger = None;
        self.last_accounting = None;
        self.initial_script = crate::InitialScriptOutcome::new(false);
        self.loading = false;
        self.fetching = false;
        if !self.devtools_is_docked() {
            self.devtools.close();
        }
        let canonical = page.canonical_url().to_owned();
        let theme_summary = format!(
            "{:?} (effective {:?})",
            self.theme_preference,
            self.effective_theme()
        );
        let html = render_internal_html(
            page,
            &self.about_lines,
            &self.bookmarks,
            &self.browsing_history,
            &self.downloads,
            self.search_engine,
            &theme_summary,
            self.effective_scale_percent(),
            self.scripts_enabled,
        );
        self.address = canonical.clone();
        self.selection.address_cursor = self.address.len();
        self.selection.address_anchor = None;
        self.selection.page_sel = None;
        self.selection.active_drag = None;
        self.scroll = 0;
        self.values.clear();
        self.edit_versions.clear();
        self.devtools.new_navigation_to(&canonical);
        self.devtools.set_page_source(&html);
        self.document = mg_sparkle::document::parse(&html, &canonical);
        self.page_url = canonical.clone();
        self.page_mime = "text/html".into();
        self.page_loader = self.generation;
        self.dom_epoch += 1;
        self.frame_cache = None;
        self.style_cache.clear();
        self.last_load_ok = true;
        self.status = format!("Internal page · {canonical}");
        if add_history {
            if !self.history.is_empty() {
                self.history.truncate(self.history_at + 1);
            }
            self.history.push(canonical);
            self.history_at = self.history.len() - 1;
        }
        if !self.devtools.open
            || !matches!(
                self.focus,
                crate::Focus::ConsoleInput | crate::Focus::DevToolsField(_)
            )
        {
            self.focus = crate::Focus::Page;
        }
        self.dirty = true;
    }

    pub fn refresh_current_internal_page(&mut self) {
        let Some(page) = InternalPage::parse(&self.page_url) else {
            return;
        };
        if page == InternalPage::Blank {
            return;
        }
        let canonical = page.canonical_url().to_owned();
        let theme_summary = format!(
            "{:?} (effective {:?})",
            self.theme_preference,
            self.effective_theme()
        );
        let html = render_internal_html(
            page,
            &self.about_lines,
            &self.bookmarks,
            &self.browsing_history,
            &self.downloads,
            self.search_engine,
            &theme_summary,
            self.effective_scale_percent(),
            self.scripts_enabled,
        );
        self.devtools.set_page_source(&html);
        self.document = mg_sparkle::document::parse(&html, &canonical);
        self.dom_epoch += 1;
        self.frame_cache = None;
        self.style_cache.clear();
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omnibox_distinguishes_internal_localhost_domains_and_search_queries() {
        assert_eq!(
            resolve_omnibox_input("chrome://history", SearchEngine::DuckDuckGo),
            Some(OmniboxResolution::Internal(InternalPage::History))
        );
        assert_eq!(
            resolve_omnibox_input("about:blank", SearchEngine::DuckDuckGo),
            Some(OmniboxResolution::Internal(InternalPage::Blank))
        );
        assert_eq!(
            resolve_omnibox_input("localhost:8080/api", SearchEngine::DuckDuckGo),
            Some(OmniboxResolution::Url("http://localhost:8080/api".into()))
        );
        assert_eq!(
            resolve_omnibox_input("127.0.0.1:3000", SearchEngine::DuckDuckGo),
            Some(OmniboxResolution::Url("http://127.0.0.1:3000".into()))
        );
        assert_eq!(
            resolve_omnibox_input("news.ycombinator.com", SearchEngine::DuckDuckGo),
            Some(OmniboxResolution::Url(
                "https://news.ycombinator.com".into()
            ))
        );
        assert_eq!(
            resolve_omnibox_input("rust browser engine", SearchEngine::DuckDuckGo),
            Some(OmniboxResolution::Url(
                "https://html.duckduckgo.com/html/?q=rust+browser+engine".into()
            ))
        );
        assert_eq!(
            resolve_omnibox_input("rustlang", SearchEngine::Google),
            Some(OmniboxResolution::Url(
                "https://www.google.com/search?q=rustlang".into()
            ))
        );
    }
}
