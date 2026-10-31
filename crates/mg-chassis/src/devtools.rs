//! Native Chromium-parity DevTools panel: Elements, Console, Sources, Network, Performance,
//! and Application, with Right/Bottom docking, draggable splitter, and interactive Element Picker.
//! Operates strictly on parent-owned DOM projections, Stylo computed styles, network exchanges,
//! and isolated worker IPC; never executes untrusted JavaScript in the parent process.
use super::*;

const MAX_MESSAGES: usize = 128;
const MAX_MESSAGE_BYTES: usize = 2048;
const MAX_NETWORK: usize = 64;
const MAX_LINES: usize = 160;
const MAX_CONSOLE_INPUT_BYTES: usize = 4096;
const MAX_CONSOLE_HISTORY: usize = 64;
const MAX_PAGE_SOURCE_BYTES: usize = 256 * 1024;
const MAX_FRAME_HISTORY: usize = 64;
const SIDE_MIN_WIDTH: u32 = 680;
const SIDE_MIN_HEIGHT: u32 = 320;
const BOTTOM_MIN_HEIGHT: u32 = 420;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Elements,
    Console,
    Sources,
    Network,
    Performance,
    Application,
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DockSide {
    #[default]
    Right,
    Bottom,
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ElementsSubTab {
    #[default]
    Styles,
    Computed,
    BoxModel,
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NetworkSubTab {
    #[default]
    Headers,
    Response,
    Preview,
    Timing,
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AppSection {
    #[default]
    Cookies,
    Security,
    Resources,
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldKind {
    ConsoleFilter,
    ElementsSearch,
    InlineStyle,
    SourcesSearch,
    NetworkFilter,
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ConsoleFilter {
    #[default]
    All,
    Errors,
    Warnings,
    Info,
    Verbose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConsoleLevel {
    Error,
    Warning,
    Info,
    Verbose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConsoleEntryKind {
    Diagnostic,
    Command,
    Result,
    Table,
    Group { collapsed: bool },
    NavMarker,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConsoleEntry {
    pub level: ConsoleLevel,
    pub kind: ConsoleEntryKind,
    pub source: &'static str,
    pub message: String,
    pub extra_lines: Vec<String>,
    pub count: u32,
    pub group_depth: u8,
    pub style: Option<String>,
    pub object_id: Option<u64>,
    pub node_id: Option<usize>,
    pub expanded: bool,
    pub properties: Option<Vec<mg_sparkle::page_session::console::ConsolePropertyPreview>>,
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NetFilter {
    #[default]
    All,
    Doc,
    Css,
    Js,
    Font,
    Img,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NetKind {
    Doc,
    Css,
    Js,
    Font,
    Img,
}

impl NetKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Doc => "Doc",
            Self::Css => "CSS",
            Self::Js => "JS",
            Self::Font => "Font",
            Self::Img => "Img",
        }
    }

    fn matches(self, filter: NetFilter) -> bool {
        match filter {
            NetFilter::All => true,
            NetFilter::Doc => self == Self::Doc,
            NetFilter::Css => self == Self::Css,
            NetFilter::Js => self == Self::Js,
            NetFilter::Font => self == Self::Font,
            NetFilter::Img => self == Self::Img,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NetworkEntry {
    pub method: &'static str,
    pub kind: NetKind,
    pub url: String,
    pub status: Option<u16>,
    pub pending: bool,
    pub mime: String,
    pub bytes: usize,
    pub start_ms: f64,
    pub duration_ms: f64,
    pub error: Option<String>,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
    pub body_preview: Option<String>,
}

#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    pub tab: Tab,
    pub dock: DockSide,
    pub custom_side_width: Option<u32>,
    pub custom_bottom_height: Option<u32>,
    pub split_dragging: bool,
    pub pick_mode: bool,
    pub hovered_inspect_node: Option<(u64, usize)>,
    pub elements_subtab: ElementsSubTab,
    pub elements_search: String,
    pub elements_search_cursor: usize,
    pub elements_match_idx: usize,
    pub inline_style_input: String,
    pub inline_style_cursor: usize,
    pub expanded_nodes: HashSet<(u64, usize)>,
    pub collapsed_nodes: HashSet<(u64, usize)>,
    pub console_filter: ConsoleFilter,
    pub console_search: String,
    pub console_search_cursor: usize,
    pub console_input: String,
    pub console_cursor: usize,
    pub console_anchor: Option<usize>,
    pub field_anchor: Option<(FieldKind, usize)>,
    pub console_sel: Option<(usize, usize)>,
    pub sources_sel: Option<(usize, usize)>,
    pub console_history: Vec<String>,
    pub console_history_idx: Option<usize>,
    pub console_draft: String,
    pub preserve_log: bool,
    pub scroll_to_bottom: bool,
    pub clipboard: String,
    pub pending_clipboard_sync: Option<String>,
    pub copy_feedback: Option<String>,
    pub pending_inspect_node: Option<usize>,
    pub current_group_depth: u8,
    pub selected_source: usize,
    pub sources_search: String,
    pub sources_search_cursor: usize,
    pub sources_match_line: Option<usize>,
    pub page_source: String,
    pub net_filter: NetFilter,
    pub net_search: String,
    pub net_search_cursor: usize,
    pub net_preserve_log: bool,
    pub disable_cache: bool,
    pub selected_request: Option<usize>,
    pub net_subtab: NetworkSubTab,
    pub app_section: AppSection,
    pub context: Option<(i32, i32)>,
    pub context_link: Option<String>,
    pub context_kind: crate::clipboard::ContextKind,
    pub console_log_rect: Option<(i32, i32, u32, u32)>,
    pub console_repl_rect: Option<(i32, i32, u32, u32)>,
    pub sources_code_rect: Option<(i32, i32, u32, u32)>,
    pub field_rects: Vec<(FieldKind, (i32, i32, u32, u32))>,
    pub row: usize,
    pub painted_epoch: u64,
    pub(super) selected: Option<(u64, usize)>,
    pub(super) inspected_history: Vec<(u64, usize)>,
    pub(super) console: Vec<ConsoleEntry>,
    omitted: usize,
    pub(super) network: Vec<NetworkEntry>,
    pub frame_history_ms: VecDeque<f64>,
    pub rendering: mg_sparkle::style::StyleDiagnostics,
    pub performance: PerformanceProfile,
}

impl State {
    #[cfg(test)]
    pub fn visible(&self) -> bool {
        self.open || self.context.is_some()
    }

    pub fn close(&mut self) {
        self.open = false;
        self.pick_mode = false;
        self.hovered_inspect_node = None;
        self.pending_inspect_node = None;
        self.split_dragging = false;
        self.context = None;
        self.context_link = None;
        self.context_kind = crate::clipboard::ContextKind::Page;
        self.console_log_rect = None;
        self.console_repl_rect = None;
        self.sources_code_rect = None;
        self.field_rects.clear();
        self.row = 0;
    }

    /// Reset per-document telemetry unconditionally (including Console and Network logs).
    pub fn new_navigation(&mut self) {
        self.context = None;
        self.context_link = None;
        self.context_kind = crate::clipboard::ContextKind::Page;
        self.pick_mode = false;
        self.hovered_inspect_node = None;
        self.pending_inspect_node = None;
        self.row = 0;
        self.selected = None;
        self.inspected_history.clear();
        self.expanded_nodes.clear();
        self.collapsed_nodes.clear();
        self.inline_style_input.clear();
        self.inline_style_cursor = 0;
        self.field_anchor = None;
        self.console_sel = None;
        self.sources_sel = None;
        self.copy_feedback = None;
        self.elements_match_idx = 0;
        self.selected_source = 0;
        self.sources_match_line = None;
        self.page_source.clear();
        self.selected_request = None;
        self.painted_epoch = 0;
        self.console.clear();
        self.omitted = 0;
        self.current_group_depth = 0;
        self.scroll_to_bottom = false;
        self.network.clear();
        self.rendering = Default::default();
        self.performance = Default::default();
    }

    /// Reset per-document telemetry while keeping an already-docked DevTools
    /// panel open across navigations, honoring Console and Network `Preserve log`.
    pub fn new_navigation_to(&mut self, url: &str) {
        if !self.preserve_log && !self.net_preserve_log {
            self.new_navigation();
            return;
        }
        let saved_net = if self.net_preserve_log {
            std::mem::take(&mut self.network)
        } else {
            Vec::new()
        };
        let saved_console = if self.preserve_log {
            std::mem::take(&mut self.console)
        } else {
            Vec::new()
        };
        let saved_omitted = if self.preserve_log { self.omitted } else { 0 };
        self.new_navigation();
        self.network = saved_net;
        if self.preserve_log {
            self.console = saved_console;
            self.omitted = saved_omitted;
            for entry in &mut self.console {
                entry.object_id = None;
                entry.node_id = None;
            }
            self.push_interactive_entry(ConsoleEntry {
                level: ConsoleLevel::Info,
                kind: ConsoleEntryKind::NavMarker,
                source: "Nav",
                message: format!("── Navigated to {} ──", bounded(url, 240)),
                extra_lines: Vec::new(),
                count: 1,
                group_depth: 0,
                style: None,
                object_id: None,
                node_id: None,
                expanded: false,
                properties: None,
            });
            self.scroll_to_bottom = true;
        }
    }

    pub fn set_page_source(&mut self, html: &str) {
        let mut end = html.len().min(MAX_PAGE_SOURCE_BYTES);
        while !html.is_char_boundary(end) {
            end -= 1;
        }
        self.page_source = html[..end].to_owned();
    }

    pub fn select_node(&mut self, epoch: u64, node: usize) {
        self.selected = Some((epoch, node));
        self.inspected_history
            .retain(|&(e, id)| e == epoch && id != node);
        self.inspected_history.insert(0, (epoch, node));
        self.inspected_history.truncate(5);
    }

    pub fn toggle_node_expanded(&mut self, epoch: u64, node: usize) {
        let key = (epoch, node);
        if self.expanded_nodes.remove(&key) {
            self.collapsed_nodes.insert(key);
        } else if self.collapsed_nodes.remove(&key) {
            self.expanded_nodes.insert(key);
        } else if node <= 2 {
            self.collapsed_nodes.insert(key);
        } else {
            self.expanded_nodes.insert(key);
        }
    }

    #[cfg(test)]
    pub fn inspected_nodes_for(&self, epoch: u64) -> Vec<usize> {
        self.inspected_history
            .iter()
            .filter_map(|&(e, id)| (e == epoch).then_some(id))
            .take(5)
            .collect()
    }

    pub fn start_document_request(&mut self, method: &'static str, url: &str) {
        if !self.net_preserve_log {
            self.network.clear();
            self.selected_request = None;
        }
        if self.network.len() >= MAX_NETWORK {
            self.network.remove(0);
        }
        self.network.push(NetworkEntry {
            method,
            kind: NetKind::Doc,
            url: bounded(url, 512),
            status: None,
            pending: true,
            mime: String::new(),
            bytes: 0,
            start_ms: 0.0,
            duration_ms: 0.0,
            error: None,
            request_headers: Vec::new(),
            response_headers: Vec::new(),
            body_preview: None,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_network_load(
        &mut self,
        url: &str,
        status: Option<u16>,
        mime: &str,
        bytes: usize,
        fetch_html_ms: f64,
        parse_html_ms: f64,
        error: Option<&str>,
        resources: &[resources::ResourceTiming],
        resources_total_ms: f64,
        scripts: &[resources::ResourceTiming],
    ) {
        let method = self
            .network
            .iter()
            .rev()
            .find(|e| e.kind == NetKind::Doc)
            .map_or("GET", |entry| entry.method);
        if self.net_preserve_log {
            if let Some(pos) = self
                .network
                .iter()
                .rposition(|e| e.pending && e.kind == NetKind::Doc)
            {
                self.network.remove(pos);
            }
        } else {
            self.network.clear();
        }
        let push_bounded = |list: &mut Vec<NetworkEntry>, entry: NetworkEntry| {
            if list.len() >= MAX_NETWORK {
                list.remove(0);
            }
            list.push(entry);
        };
        push_bounded(
            &mut self.network,
            NetworkEntry {
                method,
                kind: NetKind::Doc,
                url: bounded(url, 512),
                status,
                pending: false,
                mime: bounded(mime, 128),
                bytes,
                start_ms: 0.0,
                duration_ms: fetch_html_ms.max(0.0),
                error: error.map(|text| bounded(text, 256)),
                request_headers: Vec::new(),
                response_headers: Vec::new(),
                body_preview: None,
            },
        );
        let sub_base = (fetch_html_ms + parse_html_ms).max(0.0);
        for item in resources {
            if !self.net_preserve_log && self.network.len() >= MAX_NETWORK {
                break;
            }
            let kind = match item.kind {
                "font" => NetKind::Font,
                "image" => NetKind::Img,
                "script" => NetKind::Js,
                _ => NetKind::Css,
            };
            push_bounded(
                &mut self.network,
                NetworkEntry {
                    method: "GET",
                    kind,
                    url: bounded(&item.url, 512),
                    status: item.status,
                    pending: false,
                    mime: bounded(&item.content_type, 128),
                    bytes: item.bytes,
                    start_ms: sub_base + item.start_ms.max(0.0),
                    duration_ms: item.duration_ms.max(0.0),
                    error: item.error.as_deref().map(|e| bounded(e, 256)),
                    request_headers: Vec::new(),
                    response_headers: Vec::new(),
                    body_preview: None,
                },
            );
        }
        let script_base = sub_base + resources_total_ms.max(0.0);
        for item in scripts {
            if !self.net_preserve_log && self.network.len() >= MAX_NETWORK {
                break;
            }
            push_bounded(
                &mut self.network,
                NetworkEntry {
                    method: "GET",
                    kind: NetKind::Js,
                    url: bounded(&item.url, 512),
                    status: item.status,
                    pending: false,
                    mime: bounded(&item.content_type, 128),
                    bytes: item.bytes,
                    start_ms: script_base + item.start_ms.max(0.0),
                    duration_ms: item.duration_ms.max(0.0),
                    error: item.error.as_deref().map(|e| bounded(e, 256)),
                    request_headers: Vec::new(),
                    response_headers: Vec::new(),
                    body_preview: None,
                },
            );
        }
    }

    pub fn sync_network_exchanges(&mut self, exchanges: &[net::ExchangeRecord]) {
        for entry in &mut self.network {
            if !entry.response_headers.is_empty() {
                continue;
            }
            if let Some(ex) = exchanges.iter().rev().find(|ex| ex.url == entry.url) {
                entry.request_headers = ex.request_headers.clone();
                entry.response_headers = ex.response_headers.clone();
                entry.body_preview = ex.body_preview.clone();
            }
        }
    }

    pub fn clear_performance_metrics(&mut self) {
        self.performance.frames_painted = 0;
        self.performance.frame_cache_hits = 0;
        self.performance.style_cache_hits = 0;
        self.performance.max_frame_ms = 0.0;
        self.frame_history_ms.clear();
    }

    pub fn record_frame_timings(&mut self, timings: mg_sparkle::render::RenderTimings) {
        self.performance.frames_painted = self.performance.frames_painted.saturating_add(1);
        if timings.frame_cached {
            self.performance.frame_cache_hits = self.performance.frame_cache_hits.saturating_add(1);
        } else {
            if timings.style_cached {
                self.performance.style_cache_hits =
                    self.performance.style_cache_hits.saturating_add(1);
            }
            self.performance.last_full_render = timings.clone();
        }
        if timings.total_ms > self.performance.max_frame_ms {
            self.performance.max_frame_ms = timings.total_ms;
        }
        if self.frame_history_ms.len() >= MAX_FRAME_HISTORY {
            self.frame_history_ms.pop_front();
        }
        self.frame_history_ms.push_back(timings.total_ms);
        self.performance.last_frame = timings;
    }

    pub fn invalidate_selection(&mut self, epoch: u64) {
        self.inspected_history.retain(|&(e, _)| e == epoch);
        self.expanded_nodes.retain(|&(e, _)| e == epoch);
        self.collapsed_nodes.retain(|&(e, _)| e == epoch);
        if self.hovered_inspect_node.is_some_and(|(he, _)| he != epoch) {
            self.hovered_inspect_node = None;
        }
        if self
            .selected
            .is_some_and(|(selected_epoch, _)| selected_epoch != epoch)
        {
            self.selected = None;
            self.inline_style_input.clear();
            self.inline_style_cursor = 0;
            self.context = None;
            self.context_link = None;
            self.row = 0;
        }
    }

    pub fn carry_epoch(&mut self, old_epoch: u64, new_epoch: u64, doc: &Document) {
        let valid = |id: usize| id < doc.nodes.len() && (id == 0 || doc.nodes[id].parent != id);
        if let Some((e, id)) = self.selected
            && e == old_epoch
            && valid(id)
        {
            self.selected = Some((new_epoch, id));
        }
        for (e, id) in &mut self.inspected_history {
            if *e == old_epoch && valid(*id) {
                *e = new_epoch;
            }
        }
        self.inspected_history.retain(|&(e, _)| e == new_epoch);
        let exp: Vec<usize> = self
            .expanded_nodes
            .iter()
            .filter_map(|&(e, id)| (e == old_epoch && valid(id)).then_some(id))
            .collect();
        self.expanded_nodes.clear();
        for id in exp {
            self.expanded_nodes.insert((new_epoch, id));
        }
        let col: Vec<usize> = self
            .collapsed_nodes
            .iter()
            .filter_map(|&(e, id)| (e == old_epoch && valid(id)).then_some(id))
            .collect();
        self.collapsed_nodes.clear();
        for id in col {
            self.collapsed_nodes.insert((new_epoch, id));
        }
        if self.painted_epoch == old_epoch {
            self.painted_epoch = new_epoch;
        }
    }

    pub fn remap_nodes_between_documents(
        &mut self,
        epoch: u64,
        old_doc: &Document,
        new_doc: &Document,
    ) {
        let old_dfs = dfs_node_order(old_doc);
        let new_dfs = dfs_node_order(new_doc);
        let map_id = |old_id: usize| -> Option<usize> {
            let old_node = old_doc.nodes.get(old_id)?;
            if let Some(pos) = old_dfs.iter().position(|&id| id == old_id)
                && let Some(&cand) = new_dfs.get(pos)
                && new_doc
                    .nodes
                    .get(cand)
                    .is_some_and(|n| n.tag == old_node.tag)
            {
                return Some(cand);
            }
            if let Some(id_attr) = old_node.attr("id").filter(|s| !s.is_empty())
                && let Some((cand, _)) = new_doc
                    .nodes
                    .iter()
                    .enumerate()
                    .find(|(_, n)| n.tag == old_node.tag && n.attr("id") == Some(id_attr))
            {
                return Some(cand);
            }
            (old_id < new_doc.nodes.len() && new_doc.nodes[old_id].tag == old_node.tag)
                .then_some(old_id)
        };
        if let Some((e, id)) = self.selected
            && e == epoch
        {
            self.selected = map_id(id).map(|nid| (epoch, nid));
        }
        let mut next_hist = Vec::new();
        for &(e, id) in &self.inspected_history {
            if e == epoch
                && let Some(nid) = map_id(id)
                && !next_hist.contains(&(epoch, nid))
            {
                next_hist.push((epoch, nid));
            }
        }
        self.inspected_history = next_hist;
    }

    pub fn record(&mut self, source: &str, message: &str) {
        let (level, tag) = match source {
            "JS"
            | "JavaScript"
            | "JavaScript worker"
            | "JavaScript session"
            | "JavaScript rejected" => (ConsoleLevel::Error, "JS"),
            "Navigation" => (ConsoleLevel::Error, "Net"),
            "Resource" => {
                let is_http_err = message.contains("HTTP 4")
                    || message.contains("HTTP 5")
                    || message.contains("unavailable");
                (
                    if is_http_err {
                        ConsoleLevel::Error
                    } else {
                        ConsoleLevel::Warning
                    },
                    "Net",
                )
            }
            "Info" | "Page" => (ConsoleLevel::Info, "Page"),
            _ => (ConsoleLevel::Warning, "Page"),
        };
        self.push_console(
            level,
            tag,
            &format!("{source}: {}", bounded(message, MAX_MESSAGE_BYTES)),
        );
    }

    pub fn record_info(&mut self, source: &'static str, message: &str) {
        self.push_console(ConsoleLevel::Info, source, message);
    }

    fn push_console(&mut self, level: ConsoleLevel, source: &'static str, raw: &str) {
        let message = bounded(raw, MAX_MESSAGE_BYTES);
        if let Some(existing) = self.console.last_mut().filter(|entry| {
            entry.kind == ConsoleEntryKind::Diagnostic
                && entry.level == level
                && entry.source == source
                && entry.message == message
                && entry.object_id.is_none()
                && entry.extra_lines.is_empty()
        }) {
            existing.count = existing.count.saturating_add(1);
            return;
        }
        if let Some(existing) = self.console.iter_mut().find(|entry| {
            entry.kind == ConsoleEntryKind::Diagnostic
                && entry.level == level
                && entry.source == source
                && entry.message == message
                && entry.object_id.is_none()
                && entry.extra_lines.is_empty()
        }) {
            existing.count = existing.count.saturating_add(1);
            return;
        }
        if self.console.len() == MAX_MESSAGES {
            self.omitted = self.omitted.saturating_add(1);
        } else {
            self.console.push(ConsoleEntry {
                level,
                kind: ConsoleEntryKind::Diagnostic,
                source,
                message,
                extra_lines: Vec::new(),
                count: 1,
                group_depth: self.current_group_depth,
                style: None,
                object_id: None,
                node_id: None,
                expanded: false,
                properties: None,
            });
        }
    }

    pub fn push_interactive_entry(&mut self, entry: ConsoleEntry) {
        if self.console.len() >= MAX_MESSAGES {
            self.console.remove(0);
        }
        self.console.push(entry);
    }

    pub fn ingest_wire_console(
        &mut self,
        entries: Vec<mg_sparkle::page_session::console::ConsoleWireEntry>,
        allow_dom_links: bool,
    ) {
        use mg_sparkle::page_session::console::ConsoleWireLevel;
        if entries.is_empty() {
            return;
        }
        for wire in entries {
            match wire.level {
                ConsoleWireLevel::Clear => {
                    if self.preserve_log {
                        self.push_interactive_entry(ConsoleEntry {
                            level: ConsoleLevel::Info,
                            kind: ConsoleEntryKind::Diagnostic,
                            source: "Console",
                            message: "console.clear() was prevented due to 'Preserve log'".into(),
                            extra_lines: Vec::new(),
                            count: 1,
                            group_depth: self.current_group_depth,
                            style: None,
                            object_id: None,
                            node_id: None,
                            expanded: false,
                            properties: None,
                        });
                    } else {
                        self.clear_console();
                    }
                }
                ConsoleWireLevel::Copy(payload) => {
                    self.clipboard = bounded(
                        &payload,
                        mg_sparkle::page_session::console::MAX_CONSOLE_WIRE_BYTES,
                    );
                    self.pending_clipboard_sync = Some(self.clipboard.clone());
                    self.push_interactive_entry(ConsoleEntry {
                        level: ConsoleLevel::Info,
                        kind: ConsoleEntryKind::Diagnostic,
                        source: "Console",
                        message: format!("Copied {} B to DevTools clipboard", self.clipboard.len()),
                        extra_lines: Vec::new(),
                        count: 1,
                        group_depth: self.current_group_depth,
                        style: None,
                        object_id: None,
                        node_id: None,
                        expanded: false,
                        properties: None,
                    });
                }
                ConsoleWireLevel::InspectNode(nid) => {
                    if allow_dom_links {
                        self.pending_inspect_node = Some(nid);
                    }
                }
                ConsoleWireLevel::GroupEnd => {
                    self.current_group_depth = self.current_group_depth.saturating_sub(1);
                }
                ConsoleWireLevel::Group | ConsoleWireLevel::GroupCollapsed => {
                    let collapsed = wire.level == ConsoleWireLevel::GroupCollapsed;
                    let label = if wire.text.trim().is_empty() {
                        "console.group".into()
                    } else {
                        bounded(&wire.text, 512)
                    };
                    let depth = self.current_group_depth;
                    self.push_interactive_entry(ConsoleEntry {
                        level: ConsoleLevel::Info,
                        kind: ConsoleEntryKind::Group { collapsed },
                        source: "Group",
                        message: label,
                        extra_lines: Vec::new(),
                        count: 1,
                        group_depth: depth,
                        style: None,
                        object_id: wire.object_id,
                        node_id: wire.node_id.filter(|_| allow_dom_links),
                        expanded: !collapsed,
                        properties: None,
                    });
                    self.current_group_depth = self.current_group_depth.saturating_add(1).min(8);
                }
                ConsoleWireLevel::Table => {
                    let mut lines = wire.text.lines();
                    let first = bounded(lines.next().unwrap_or("(table)"), 512);
                    let extra: Vec<String> =
                        lines.take(48).map(|line| bounded(line, 512)).collect();
                    self.push_interactive_entry(ConsoleEntry {
                        level: ConsoleLevel::Info,
                        kind: ConsoleEntryKind::Table,
                        source: "Table",
                        message: first,
                        extra_lines: extra,
                        count: 1,
                        group_depth: self.current_group_depth,
                        style: None,
                        object_id: wire.object_id,
                        node_id: None,
                        expanded: false,
                        properties: None,
                    });
                }
                ConsoleWireLevel::Log
                | ConsoleWireLevel::Info
                | ConsoleWireLevel::Debug
                | ConsoleWireLevel::Warn
                | ConsoleWireLevel::Error
                | ConsoleWireLevel::Dir => {
                    let level = match wire.level {
                        ConsoleWireLevel::Error => ConsoleLevel::Error,
                        ConsoleWireLevel::Warn => ConsoleLevel::Warning,
                        ConsoleWireLevel::Debug => ConsoleLevel::Verbose,
                        _ => ConsoleLevel::Info,
                    };
                    let mut raw_lines = wire.text.lines();
                    let first = bounded(raw_lines.next().unwrap_or(""), MAX_MESSAGE_BYTES);
                    let extra: Vec<String> = raw_lines.take(24).map(|l| bounded(l, 512)).collect();
                    let node_id = wire.node_id.filter(|_| allow_dom_links);
                    if wire.object_id.is_none()
                        && node_id.is_none()
                        && extra.is_empty()
                        && let Some(prev) = self.console.last_mut().filter(|e| {
                            e.kind == ConsoleEntryKind::Diagnostic
                                && e.level == level
                                && e.source == "Console"
                                && e.message == first
                                && e.group_depth == self.current_group_depth
                                && e.object_id.is_none()
                                && e.extra_lines.is_empty()
                        })
                    {
                        prev.count = prev.count.saturating_add(1);
                        continue;
                    }
                    self.push_interactive_entry(ConsoleEntry {
                        level,
                        kind: ConsoleEntryKind::Diagnostic,
                        source: if wire.level == ConsoleWireLevel::Dir {
                            "Dir"
                        } else {
                            "Console"
                        },
                        message: first,
                        extra_lines: extra,
                        count: 1,
                        group_depth: self.current_group_depth,
                        style: None,
                        object_id: wire.object_id,
                        node_id,
                        expanded: false,
                        properties: None,
                    });
                }
            }
        }
        self.scroll_to_bottom = true;
    }

    pub fn toggle_console_group(&mut self, entry_idx: usize) {
        if let Some(entry) = self.console.get_mut(entry_idx)
            && let ConsoleEntryKind::Group { collapsed } = &mut entry.kind
        {
            *collapsed = !*collapsed;
            entry.expanded = !*collapsed;
        }
    }

    pub fn clear_console(&mut self) {
        self.console.clear();
        self.omitted = 0;
        self.current_group_depth = 0;
        self.console_sel = None;
        self.row = 0;
        self.scroll_to_bottom = false;
    }

    pub fn clear_network(&mut self) {
        self.network.clear();
        self.selected_request = None;
        self.row = 0;
    }

    pub fn console_counts(&self) -> (usize, usize) {
        let errors = self
            .console
            .iter()
            .filter(|entry| entry.level == ConsoleLevel::Error)
            .count();
        let warnings = self
            .console
            .iter()
            .filter(|entry| entry.level == ConsoleLevel::Warning)
            .count()
            + self.rendering.entries.len();
        (errors, warnings)
    }
}

/// Plain text only: page strings cannot insert new rows or terminal controls.
pub(super) fn bounded(value: &str, limit: usize) -> String {
    if limit < 3 {
        return String::new();
    }
    let mut result = String::new();
    for ch in value.chars() {
        let ch = if ch.is_control()
            || matches!(
                ch,
                '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
            ) {
            ' '
        } else {
            ch
        };
        if result.len() + ch.len_utf8() > limit.saturating_sub(3) {
            result.push_str("...");
            break;
        }
        result.push(ch);
    }
    result
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn short_resource_name(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return bounded(url, 40);
    };
    let path = parsed.path().trim_end_matches('/');
    let base = path.rsplit('/').next().unwrap_or("");
    if base.is_empty() {
        bounded(parsed.host_str().unwrap_or(url), 40)
    } else if let Some(query) = parsed.query() {
        bounded(&format!("{base}?{query}"), 42)
    } else {
        bounded(base, 42)
    }
}

fn matches_console_search(query: &str, source: &str, text: &str, extra: &[String]) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let src_low = source.to_ascii_lowercase();
    let txt_low = text.to_ascii_lowercase();
    for raw_term in q.split_whitespace().take(8) {
        let (neg, term) = if let Some(rest) = raw_term.strip_prefix('-') {
            (true, rest)
        } else {
            (false, raw_term)
        };
        if term.is_empty() {
            continue;
        }
        let needle = term.to_ascii_lowercase();
        let found = src_low.contains(&needle)
            || txt_low.contains(&needle)
            || extra
                .iter()
                .any(|line| line.to_ascii_lowercase().contains(&needle));
        if neg && found {
            return false;
        }
        if !neg && !found {
            return false;
        }
    }
    true
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
fn parse_css_color_token(val: &str) -> Option<u32> {
    let v = val
        .trim()
        .trim_end_matches("!important")
        .trim()
        .to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let r = u32::from_str_radix(&hex[0..1], 16).ok()?;
                let g = u32::from_str_radix(&hex[1..2], 16).ok()?;
                let b = u32::from_str_radix(&hex[2..3], 16).ok()?;
                Some(((r * 17) << 16) | ((g * 17) << 8) | (b * 17))
            }
            6 | 8 => u32::from_str_radix(&hex[0..6], 16).ok(),
            _ => None,
        };
    }
    match v.as_str() {
        "red" | "crimson" => Some(0xd93025),
        "green" | "forestgreen" => Some(0x188038),
        "lime" | "limegreen" => Some(0x10b981),
        "blue" | "dodgerblue" | "royalblue" => Some(0x2563eb),
        "cyan" | "aqua" | "teal" => Some(0x0891b2),
        "magenta" | "fuchsia" | "purple" | "violet" => Some(0x9333ea),
        "orange" | "darkorange" => Some(0xea580c),
        "yellow" | "gold" => Some(0xca8a04),
        "pink" | "hotpink" => Some(0xdb2777),
        "white" => Some(0xffffff),
        "black" => Some(0x111827),
        "gray" | "grey" | "slategray" => Some(0x6b7280),
        _ => None,
    }
}

fn push_escaped_html(out: &mut String, text: &str, max_bytes: usize) -> bool {
    for ch in text.chars() {
        let esc = match ch {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            _ => "",
        };
        let need = if esc.is_empty() {
            ch.len_utf8()
        } else {
            esc.len()
        };
        if out.len() + need > max_bytes {
            return true;
        }
        if esc.is_empty() {
            out.push(ch);
        } else {
            out.push_str(esc);
        }
    }
    false
}

fn dfs_node_order(doc: &Document) -> Vec<usize> {
    if doc.nodes.is_empty() {
        return Vec::new();
    }
    let mut order = Vec::with_capacity(doc.nodes.len());
    let mut stack = vec![0usize];
    let mut seen = vec![false; doc.nodes.len()];
    while let Some(id) = stack.pop() {
        if id >= doc.nodes.len() || seen[id] {
            continue;
        }
        seen[id] = true;
        order.push(id);
        for &child in doc.nodes[id].children.iter().rev() {
            stack.push(child);
        }
    }
    order
}

fn serialize_document_for_console(doc: &Document, max_bytes: usize) -> (String, bool) {
    enum SerStep {
        Open(usize, usize),
        Close(String),
    }
    if doc.nodes.is_empty() {
        return ("<!doctype html><html><body></body></html>".into(), false);
    }
    let mut out = String::with_capacity(doc.nodes.len().saturating_mul(24).min(max_bytes));
    out.push_str("<!doctype html>");
    let mut stack = vec![SerStep::Open(0, 0)];
    let mut visited = 0usize;
    let mut truncated = false;
    while let Some(step) = stack.pop() {
        match step {
            SerStep::Close(tag) => {
                out.push_str("</");
                out.push_str(&tag);
                out.push('>');
            }
            SerStep::Open(id, depth) => {
                if depth > 96 {
                    truncated = true;
                    continue;
                }
                let Some(node) = doc.nodes.get(id) else {
                    continue;
                };
                if id == 0 || node.tag == "#document" {
                    for &child in node.children.iter().rev() {
                        stack.push(SerStep::Open(child, depth + 1));
                    }
                    continue;
                }
                visited += 1;
                if visited > 7500 || out.len() >= max_bytes {
                    truncated = true;
                    continue;
                }
                if node.tag == "#text" {
                    if push_escaped_html(&mut out, &node.text, max_bytes) {
                        truncated = true;
                    }
                    continue;
                }
                if node.tag.starts_with('#') {
                    continue;
                }
                let tag: String = node
                    .tag
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ':')
                    .take(48)
                    .collect();
                if tag.is_empty() {
                    continue;
                }
                if tag.eq_ignore_ascii_case("script") {
                    out.push_str("<script type=\"text/plain\"></script>");
                    continue;
                }
                out.push('<');
                out.push_str(&tag);
                for (k, v) in node.attributes.iter().take(24) {
                    let key: String = k
                        .chars()
                        .filter(|c| {
                            c.is_ascii_alphanumeric() || matches!(*c, '-' | '_' | ':' | '.')
                        })
                        .take(48)
                        .collect();
                    if key.is_empty() || out.len() + key.len() + 4 >= max_bytes {
                        break;
                    }
                    out.push(' ');
                    out.push_str(&key);
                    out.push_str("=\"");
                    let val_cap = out.len().saturating_add(512).min(max_bytes);
                    let _ = push_escaped_html(&mut out, v, val_cap);
                    out.push('"');
                }
                out.push('>');
                let is_void = matches!(
                    tag.to_ascii_lowercase().as_str(),
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
                );
                if !is_void {
                    stack.push(SerStep::Close(tag));
                    for &child in node.children.iter().rev() {
                        stack.push(SerStep::Open(child, depth + 1));
                    }
                }
            }
        }
    }
    (out, truncated)
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
const CONSOLE_COMPLETIONS: &[&str] = &[
    "$_",
    "$0",
    "$1",
    "$2",
    "$3",
    "$4",
    "$(",
    "$$(",
    "clear()",
    "copy(",
    "dir(",
    "dirxml(",
    "inspect(",
    "keys(",
    "table(",
    "values(",
    "console.log(",
    "console.info(",
    "console.debug(",
    "console.warn(",
    "console.error(",
    "console.table(",
    "console.dir(",
    "console.dirxml(",
    "console.group(",
    "console.groupCollapsed(",
    "console.groupEnd()",
    "console.time(",
    "console.timeLog(",
    "console.timeEnd(",
    "console.count(",
    "console.countReset(",
    "console.trace(",
    "console.assert(",
    "console.clear()",
    "document.querySelector(",
    "document.querySelectorAll(",
    "document.getElementById(",
    "document.createElement(",
    "document.title",
    "document.body",
    "document.head",
    "document.documentElement",
    "document.URL",
    "window.location",
    "window.navigator",
    "window.getComputedStyle(",
    "window.innerWidth",
    "window.innerHeight",
    "location.href",
    "Object.keys(",
    "Object.values(",
    "Object.entries(",
    "Object.assign(",
    "Array.isArray(",
    "Array.from(",
    "JSON.stringify(",
    "JSON.parse(",
    "Math.max(",
    "Math.min(",
    "Math.round(",
    "Math.floor(",
    "Number.isFinite(",
    "Promise.resolve(",
    "setTimeout(",
    "clearTimeout(",
];

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
#[derive(Clone)]
struct ConsoleVisualRow {
    level: ConsoleLevel,
    kind: ConsoleEntryKind,
    source: &'static str,
    text: String,
    entry_idx: Option<usize>,
    full_text: String,
    first_of_entry: bool,
    group_depth: u8,
    toggle_group: Option<usize>,
    toggle_object: Option<(usize, u64)>,
    inspect_node: Option<usize>,
}

#[cfg(feature = "chrome")]
pub(crate) fn blend_rect_pub(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    rgb: u32,
    alpha: u32,
) {
    blend_rect(canvas, x, y, w, h, rgb, alpha);
}

#[cfg(feature = "chrome")]
fn blend_rect(canvas: &mut Canvas, x: i32, y: i32, w: u32, h: u32, rgb: u32, alpha: u32) {
    let prev = canvas.intersect_clip(x, y, w, h);
    let (left, top, right, bottom) = canvas.physical_clip_bounds();
    canvas.restore_clip(prev);
    let inv = 256u32.saturating_sub(alpha);
    let sr = (rgb >> 16) & 0xff;
    let sg = (rgb >> 8) & 0xff;
    let sb = rgb & 0xff;
    let stride = canvas.width as usize;
    for py in top..bottom {
        let row = py as usize * stride;
        for px in left..right {
            let dst = canvas.pixels[row + px as usize];
            let r = (((dst >> 16) & 0xff) * inv + sr * alpha) >> 8;
            let g = (((dst >> 8) & 0xff) * inv + sg * alpha) >> 8;
            let b = ((dst & 0xff) * inv + sb * alpha) >> 8;
            canvas.pixels[row + px as usize] = (r << 16) | (g << 8) | b;
        }
    }
}

#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
fn format_length(len: mg_sparkle::style::Length) -> String {
    use mg_sparkle::style::{CalcOp, Length};
    match len {
        Length::Auto => "auto".into(),
        Length::Px(px) => format!("{px:.1}px").replace(".0px", "px"),
        Length::Percent(p) => format!("{:.1}%", p * 100.0).replace(".0%", "%"),
        Length::Calc {
            px,
            percent,
            bound_px,
            op,
            ..
        } => match op {
            CalcOp::Linear => format!("calc({px:.0}px + {:.0}%)", percent * 100.0),
            CalcOp::MinPx => format!(
                "min(calc({px:.0}px + {:.0}%), {bound_px:.0}px)",
                percent * 100.0
            ),
            CalcOp::MaxPx => format!(
                "max(calc({px:.0}px + {:.0}%), {bound_px:.0}px)",
                percent * 100.0
            ),
        },
    }
}

impl Browser {
    /// Whether DevTools is currently docked to the right or bottom of the window
    /// rather than rendered as a compact full-surface overlay.
    pub(crate) fn devtools_is_docked(&self) -> bool {
        self.devtools_side_width().is_some() || self.devtools_bottom_height().is_some()
    }

    /// Width reserved on the right side of the window when DevTools is docked right.
    /// Surfaces narrower than 680x320 fall back to a full-surface compact overlay.
    pub(crate) fn devtools_side_width(&self) -> Option<u32> {
        if self.chrome
            && self.devtools.open
            && self.devtools.dock == DockSide::Right
            && self.width >= SIDE_MIN_WIDTH
            && self.height >= SIDE_MIN_HEIGHT
        {
            let max_w = self.width.saturating_sub(280).max(320);
            let w = self
                .devtools
                .custom_side_width
                .unwrap_or_else(|| (self.width * 44 / 100).clamp(352, 540));
            Some(w.clamp(320, max_w))
        } else {
            None
        }
    }

    /// Height reserved at the bottom of the window when DevTools is docked bottom.
    pub(crate) fn devtools_bottom_height(&self) -> Option<u32> {
        if self.chrome
            && self.devtools.open
            && self.devtools.dock == DockSide::Bottom
            && self.width >= SIDE_MIN_WIDTH
            && self.height >= BOTTOM_MIN_HEIGHT
        {
            let avail_h = self.height.saturating_sub(self.page_top() as u32 + 29);
            let max_h = avail_h.saturating_sub(140).max(160);
            let h = self
                .devtools
                .custom_bottom_height
                .unwrap_or_else(|| (avail_h * 45 / 100).clamp(180, 380));
            Some(h.clamp(160, max_h))
        } else {
            None
        }
    }

    /// Logical width available to the live web page viewport.
    pub(crate) fn page_viewport_width(&self) -> u32 {
        self.width
            .saturating_sub(self.devtools_side_width().unwrap_or(0))
            .max(1)
    }

    pub(super) fn devtools_panel_rect(&self) -> (i32, i32, u32, u32) {
        if let Some(side_w) = self.devtools_side_width() {
            (
                (self.width - side_w) as i32,
                self.page_top(),
                side_w,
                self.viewport_height(),
            )
        } else if let Some(bot_h) = self.devtools_bottom_height() {
            (
                0,
                self.page_top() + self.viewport_height() as i32,
                self.width,
                bot_h,
            )
        } else {
            let w = self.width.saturating_sub(8);
            let h = self.height.saturating_sub(8);
            (4, 4, w, h)
        }
    }

    pub(super) fn on_devtools_splitter(&self, x: i32, y: i32) -> bool {
        if let Some(side_w) = self.devtools_side_width() {
            let split_x = (self.width - side_w) as i32;
            (x - split_x).abs() <= 4 && y >= self.page_top() && y < self.height as i32 - 29
        } else if self.devtools_bottom_height().is_some() {
            let split_y = self.page_top() + self.viewport_height() as i32;
            (y - split_y).abs() <= 4 && x >= 0 && x < self.width as i32
        } else {
            false
        }
    }

    pub(super) fn drag_devtools_splitter(&mut self, x: i32, y: i32) {
        match self.devtools.dock {
            DockSide::Right if self.width >= SIDE_MIN_WIDTH => {
                let max_w = self.width.saturating_sub(280).max(320);
                let desired = (self.width as i32 - x).max(320) as u32;
                let clamped = desired.clamp(320, max_w);
                if self.devtools.custom_side_width != Some(clamped) {
                    self.devtools.custom_side_width = Some(clamped);
                    self.scroll_reflow_pending = true;
                    self.hits.clear();
                    self.dirty = true;
                }
            }
            DockSide::Bottom if self.height >= BOTTOM_MIN_HEIGHT => {
                let bottom_edge = self.height as i32 - 29;
                let avail_h = self.height.saturating_sub(self.page_top() as u32 + 29);
                let max_h = avail_h.saturating_sub(140).max(160);
                let desired = (bottom_edge - y).max(160) as u32;
                let clamped = desired.clamp(160, max_h);
                if self.devtools.custom_bottom_height != Some(clamped) {
                    self.devtools.custom_bottom_height = Some(clamped);
                    self.scroll_reflow_pending = true;
                    self.hits.clear();
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    pub(super) fn toggle_devtools_dock(&mut self) {
        self.devtools.dock = match self.devtools.dock {
            DockSide::Right => DockSide::Bottom,
            DockSide::Bottom => DockSide::Right,
        };
        self.scroll_reflow_pending = true;
        self.hits.clear();
        self.dirty = true;
    }

    pub(super) fn toggle_element_picker(&mut self) {
        if !self.chrome {
            return;
        }
        self.cancel_pointer_input();
        if !self.devtools.open {
            self.open_devtools_tab(Tab::Elements);
            self.devtools.pick_mode = true;
        } else {
            self.devtools.pick_mode = !self.devtools.pick_mode;
            if self.devtools.pick_mode {
                self.devtools.tab = Tab::Elements;
            }
        }
        self.devtools.hovered_inspect_node = None;
        self.hits.clear();
        self.dirty = true;
    }

    pub(super) fn update_picker_hover(&mut self, x: i32, y: i32) {
        let next = if x >= 0
            && x < self.page_viewport_width() as i32
            && y >= self.page_top()
            && y < self.page_top() + self.viewport_height() as i32
        {
            self.inspected_node_at(x, y).map(|id| (self.dom_epoch, id))
        } else {
            None
        };
        if self.devtools.hovered_inspect_node != next {
            self.devtools.hovered_inspect_node = next;
            self.dirty = true;
        }
    }

    pub(super) fn pick_element_at(&mut self, x: i32, y: i32) {
        if let Some(node) = self.inspected_node_at(x, y) {
            self.select_devtools_node(node);
        }
        self.devtools.pick_mode = false;
        self.devtools.hovered_inspect_node = None;
        self.devtools.tab = Tab::Elements;
        self.modal_focus = 0;
        self.focus = Focus::DevTools;
        self.hits.clear();
        self.dirty = true;
    }

    pub(super) fn select_devtools_node(&mut self, node: usize) {
        self.devtools.select_node(self.dom_epoch, node);
        let inline = self
            .document
            .nodes
            .get(node)
            .and_then(|n| n.attr("style"))
            .unwrap_or("")
            .to_owned();
        self.devtools.inline_style_cursor = inline.len();
        self.devtools.inline_style_input = inline;
        self.dirty = true;
    }

    pub(super) fn expand_or_collapse_selected_node(&mut self, expand: bool) {
        let Some((epoch, id)) = self
            .devtools
            .selected
            .filter(|(epoch, _)| *epoch == self.dom_epoch)
        else {
            return;
        };
        let key = (epoch, id);
        if expand {
            self.devtools.collapsed_nodes.remove(&key);
            self.devtools.expanded_nodes.insert(key);
        } else {
            self.devtools.expanded_nodes.remove(&key);
            self.devtools.collapsed_nodes.insert(key);
        }
        self.dirty = true;
    }

    pub(super) fn toggle_selected_node_visibility(&mut self) {
        let Some((epoch, id)) = self
            .devtools
            .selected
            .filter(|(epoch, _)| *epoch == self.dom_epoch)
        else {
            return;
        };
        let _ = epoch;
        if id == 0 {
            return;
        }
        let Some(node) = self.document.nodes.get_mut(id) else {
            return;
        };
        if node.tag.starts_with('#') {
            return;
        }
        const HIDE_TOKEN: &str = "/*__mg_hide__*/display: none !important;";
        let current = node
            .attributes
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("style"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let next = if current.contains(HIDE_TOKEN) {
            current.replace(HIDE_TOKEN, "").trim().to_owned()
        } else if current.trim().is_empty() {
            HIDE_TOKEN.to_owned()
        } else {
            format!("{}; {HIDE_TOKEN}", current.trim().trim_end_matches(';'))
        };
        Self::set_node_attr_mut(node, "style", &next);
        self.devtools.inline_style_input = next.clone();
        self.devtools.inline_style_cursor = next.len();
        self.invalidate_live_dom_style(id, Some(&next));
    }

    pub(super) fn delete_selected_node(&mut self) {
        let Some((epoch, id)) = self
            .devtools
            .selected
            .filter(|(epoch, _)| *epoch == self.dom_epoch)
        else {
            return;
        };
        let _ = epoch;
        if id == 0 {
            return;
        }
        let Some((tag, parent)) = self
            .document
            .nodes
            .get(id)
            .map(|n| (n.tag.clone(), n.parent))
        else {
            return;
        };
        if matches!(tag.as_str(), "#document" | "html" | "body") || parent == id {
            return;
        }
        if let Some(parent_node) = self.document.nodes.get_mut(parent) {
            parent_node.children.retain(|&c| c != id);
        }
        if let Some(removed) = self.document.nodes.get_mut(id) {
            removed.parent = id;
        }
        self.select_devtools_node(parent);
        self.style_cache.clear();
        self.frame_cache = None;
        self.scroll_reflow_pending = true;
        self.dirty = true;
    }

    fn set_node_attr_mut(node: &mut mg_sparkle::document::Node, key: &str, value: &str) {
        if let Some((_, existing)) = node
            .attributes
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            *existing = value.to_owned();
        } else if !value.is_empty() {
            node.attributes.push((key.to_owned(), value.to_owned()));
        }
    }

    fn invalidate_live_dom_style(&mut self, node_id: usize, sync_style: Option<&str>) {
        self.style_cache.clear();
        self.frame_cache = None;
        self.scroll_reflow_pending = true;
        self.dirty = true;
        if let Some(style_val) = sync_style
            && self.page_session.is_some()
            && !self.script_blocked
            && !self.console_detached_dom
            && let Ok(lit) = serde_json::to_string(style_val)
        {
            let expr = format!("if ($0 && $0.setAttribute) $0.setAttribute('style', {lit});");
            self.enqueue_console_command(
                ConsoleCommand::Evaluate {
                    source: expr,
                    inspected_nodes: vec![node_id],
                },
                ConsoleTarget::Clear,
            );
        }
    }

    fn apply_inline_style_edit(&mut self) {
        let Some((epoch, id)) = self
            .devtools
            .selected
            .filter(|(epoch, _)| *epoch == self.dom_epoch)
        else {
            return;
        };
        let _ = epoch;
        if id == 0 {
            return;
        }
        let next = bounded(self.devtools.inline_style_input.trim(), 1024);
        if let Some(node) = self.document.nodes.get_mut(id) {
            if node.tag.starts_with('#') {
                return;
            }
            Self::set_node_attr_mut(node, "style", &next);
        } else {
            return;
        }
        self.devtools.inline_style_input = next.clone();
        self.devtools.inline_style_cursor = next.len();
        self.invalidate_live_dom_style(id, Some(&next));
    }

    /// Open a native context menu at logical browser-surface coordinates across
    /// the address bar, DevTools panels/inputs, form `<input>` controls, or web page.
    /// Hosts must convert physical input using the current display scale.
    pub fn context_menu_at(&mut self, x: i32, y: i32) {
        if !self.chrome
            || self.menu_open
            || self.about_open
            || self.settings_open
            || self.bookmarks_open
            || self.devtools.context.is_some()
            || x < 0
            || y < 0
            || x >= self.width as i32
            || y >= self.height as i32
        {
            return;
        }
        // 1. Address bar right-click
        if let Some((ax, ay, aw, ah)) = self.address_bar_rect()
            && x >= ax
            && x < ax + aw as i32
            && y >= ay
            && y < ay + ah as i32
        {
            self.cancel_pointer_input();
            if self.focus != Focus::Address {
                self.focus = Focus::Address;
                if !self.address.is_empty() {
                    self.selection.address_anchor = Some(0);
                    self.selection.address_cursor = self.address.len();
                    self.select_all = true;
                }
            }
            self.devtools.context_link = None;
            self.devtools.context_kind = crate::clipboard::ContextKind::AddressBar;
            self.devtools.context = Some((x, y));
            self.modal_focus = 0;
            self.hits.clear();
            self.dirty = true;
            return;
        }
        // 2. Open DevTools panel right-click (docked or overlay)
        if self.devtools.open {
            let (px, py, pw, ph) = self.devtools_panel_rect();
            if x >= px && x < px + pw as i32 && y >= py && y < py + ph as i32 {
                let hit_action = self.topmost_hit(x, y).map(|i| self.hits[i].action.clone());
                self.cancel_pointer_input();
                self.devtools.context_link = None;
                for &(kind, (fx, fy, fw, fh)) in &self.devtools.field_rects.clone() {
                    if x >= fx && x < fx + fw as i32 && y >= fy && y < fy + fh as i32 {
                        if self.focus != Focus::DevToolsField(kind) {
                            self.focus_devtools_field(kind);
                        }
                        self.devtools.context_kind =
                            crate::clipboard::ContextKind::DevToolsField(kind);
                        self.devtools.context = Some((x, y));
                        self.modal_focus = 0;
                        self.hits.clear();
                        self.dirty = true;
                        return;
                    }
                }
                if self.devtools.tab == Tab::Console
                    && let Some((rx, ry, rw, rh)) = self.devtools.console_repl_rect
                    && x >= rx
                    && x < rx + rw as i32
                    && y >= ry
                    && y < ry + rh as i32
                {
                    self.focus = Focus::ConsoleInput;
                    self.devtools.context_kind = crate::clipboard::ContextKind::ConsoleRepl;
                    self.devtools.context = Some((x, y));
                    self.modal_focus = 0;
                    self.hits.clear();
                    self.dirty = true;
                    return;
                }
                self.focus = Focus::DevTools;
                self.devtools.context_kind = match self.devtools.tab {
                    Tab::Console => {
                        if let Some(vrow) = self.console_row_at_point(x, y) {
                            let inside = self
                                .devtools
                                .console_sel
                                .is_some_and(|(a, b)| vrow >= a.min(b) && vrow <= a.max(b));
                            if !inside {
                                self.devtools.console_sel = Some((vrow, vrow));
                            }
                        }
                        crate::clipboard::ContextKind::ConsoleLog
                    }
                    Tab::Elements => {
                        if let Some(Action::DevToolsSelectNode(nid)) = hit_action {
                            self.select_devtools_node(nid);
                        }
                        crate::clipboard::ContextKind::Elements
                    }
                    Tab::Sources => {
                        if let Some(line) = self.sources_line_at_point(x, y) {
                            let inside = self
                                .devtools
                                .sources_sel
                                .is_some_and(|(a, b)| line >= a.min(b) && line <= a.max(b));
                            if !inside {
                                self.devtools.sources_sel = Some((line, line));
                            }
                        }
                        crate::clipboard::ContextKind::Sources
                    }
                    Tab::Network => {
                        if let Some(Action::DevToolsNetSelect(Some(idx))) = hit_action {
                            self.devtools.selected_request = Some(idx);
                        }
                        crate::clipboard::ContextKind::Network
                    }
                    Tab::Performance => crate::clipboard::ContextKind::Performance,
                    Tab::Application => crate::clipboard::ContextKind::Application,
                };
                self.devtools.context = Some((x, y));
                self.modal_focus = 0;
                self.hits.clear();
                self.dirty = true;
                return;
            }
        }
        // 3. Page surface right-click
        if self.popup_modal_open()
            || self.loading
            || x >= self.page_viewport_width() as i32
            || y < self.page_top()
            || y >= self.page_top() + self.viewport_height() as i32
        {
            return;
        }
        let hit_action = self.topmost_hit(x, y).map(|i| self.hits[i].action.clone());
        let inspected = self.inspected_node_at(x, y);
        let input_target = match hit_action {
            Some(Action::Input(node)) => Some(node),
            _ => inspected.filter(|&node| self.input_item(node).is_some()),
        };
        let context_link = self.link_at(x, y).or_else(|| {
            let mut cur = inspected?;
            for _ in 0..256 {
                let node = self.document.nodes.get(cur)?;
                if node.tag == "a"
                    && let Some(raw) = node.attr("href")
                    && !raw.trim().is_empty()
                {
                    let resolved = url::Url::parse(&self.page_url)
                        .and_then(|base| base.join(raw.trim()))
                        .or_else(|_| url::Url::parse(raw.trim()))
                        .ok()?;
                    if matches!(resolved.scheme(), "http" | "https") {
                        return Some(resolved.into());
                    }
                }
                if cur == 0 || node.parent == cur {
                    break;
                }
                cur = node.parent;
            }
            None
        });
        self.cancel_pointer_input();
        if let Some(node) = input_target {
            if self.focus != Focus::Input(node) {
                self.focus = Focus::Input(node);
                let len = self.current_input_value(node).map_or(0, |v| v.len());
                self.selection.input_node = Some(node);
                if len > 0 {
                    self.selection.input_anchor = Some(0);
                    self.selection.input_cursor = len;
                    self.select_all = true;
                }
            }
            self.select_devtools_node(node);
            self.devtools.context_link = None;
            self.devtools.context_kind = crate::clipboard::ContextKind::PageInput(node);
            self.devtools.context = Some((x, y));
            self.modal_focus = 0;
            self.hits.clear();
            self.dirty = true;
            return;
        }
        self.select_all = false;
        self.focus = Focus::Page;
        if let Some(node) = inspected {
            self.select_devtools_node(node);
        } else {
            self.devtools.selected = None;
        }
        self.devtools.context_link = context_link;
        self.devtools.context_kind = crate::clipboard::ContextKind::Page;
        self.devtools.context = Some((x, y));
        self.modal_focus = 0;
        self.hits.clear();
        self.dirty = true;
    }

    /// Toggle the native DevTools side panel (`F12` / `Ctrl+Shift+I`).
    pub fn toggle_devtools(&mut self) {
        if !self.chrome {
            return;
        }
        self.cancel_pointer_input();
        if self.devtools.open {
            self.devtools.close();
            self.focus = Focus::Page;
            self.scroll_reflow_pending = true;
        } else {
            if self.devtools.context.is_none() && self.devtools.selected.is_none() {
                if !self.document.nodes.is_empty() {
                    self.select_devtools_node(0);
                }
            }
            self.open_devtools_tab(Tab::Elements);
        }
        self.hits.clear();
        self.dirty = true;
    }

    /// Toggle a specific DevTools tab (`Ctrl+Shift+J/E/P`), closing only if
    /// DevTools is already open on that exact tab.
    pub(crate) fn toggle_devtools_tab(&mut self, tab: Tab) {
        if !self.chrome {
            return;
        }
        self.cancel_pointer_input();
        if self.devtools.open && self.devtools.tab == tab {
            self.devtools.close();
            self.focus = Focus::Page;
            self.scroll_reflow_pending = true;
        } else {
            if self.devtools.context.is_none() && self.devtools.selected.is_none() {
                if !self.document.nodes.is_empty() {
                    self.select_devtools_node(0);
                }
            }
            self.open_devtools_tab(tab);
        }
        self.hits.clear();
        self.dirty = true;
    }

    /// Toggle the native Performance Dashboard (`Ctrl+Shift+P`).
    pub fn toggle_performance_dashboard(&mut self) {
        self.toggle_devtools_tab(Tab::Performance);
    }

    /// Read the most recent string copied via `copy(...)` in the Console Command Line API.
    pub fn console_clipboard(&self) -> &str {
        &self.devtools.clipboard
    }

    /// Cancel transient input when a host switches or moves the owning pane.
    /// The focused control, page session and native dialogs are preserved.
    pub fn cancel_pointer_input(&mut self) {
        self.stop_scroll();
        self.pointer_press = None;
        self.pointer_position = None;
        self.selection.active_drag = None;
        self.devtools.split_dragging = false;
        if self.hovered_link.take().is_some() {
            self.dirty = true;
        }
    }

    pub(super) fn open_inspector(&mut self) {
        self.open_devtools_tab(Tab::Elements);
    }

    pub(super) fn open_devtools_tab(&mut self, tab: Tab) {
        let was_open = self.devtools.open;
        self.devtools.invalidate_selection(self.dom_epoch);
        if self.devtools.selected.is_none() && !self.document.nodes.is_empty() {
            self.select_devtools_node(0);
        } else if let Some((_, node)) = self.devtools.selected {
            self.select_devtools_node(node);
        }
        self.devtools.open = true;
        self.devtools.tab = tab;
        self.devtools.context = None;
        self.devtools.context_link = None;
        self.devtools.row = 0;
        self.menu_open = false;
        self.about_open = false;
        self.settings_open = false;
        self.bookmarks_open = false;
        self.modal_focus = match tab {
            Tab::Elements => 0,
            Tab::Console => 1,
            Tab::Sources => 2,
            Tab::Network => 3,
            Tab::Performance => 4,
            Tab::Application => 5,
        };
        self.focus = Focus::DevTools;
        self.select_all = false;
        if !was_open {
            self.scroll_reflow_pending = true;
        }
        self.hits.clear();
        self.pointer_press = None;
        self.dirty = true;
    }

    pub(super) fn devtools_search_field_for_tab(&self) -> Option<FieldKind> {
        match self.devtools.tab {
            Tab::Elements => Some(FieldKind::ElementsSearch),
            Tab::Console => Some(FieldKind::ConsoleFilter),
            Tab::Sources => Some(FieldKind::SourcesSearch),
            Tab::Network => Some(FieldKind::NetworkFilter),
            Tab::Performance | Tab::Application => None,
        }
    }

    pub(super) fn focus_devtools_field(&mut self, kind: FieldKind) {
        self.devtools.field_anchor = None;
        match kind {
            FieldKind::ConsoleFilter => {
                self.devtools.console_search_cursor = self.devtools.console_search.len();
            }
            FieldKind::ElementsSearch => {
                self.devtools.elements_search_cursor = self.devtools.elements_search.len();
            }
            FieldKind::InlineStyle => {
                if let Some((epoch, id)) = self.devtools.selected
                    && epoch == self.dom_epoch
                    && self.devtools.inline_style_input.is_empty()
                    && let Some(style) = self.document.nodes.get(id).and_then(|n| n.attr("style"))
                {
                    self.devtools.inline_style_input = style.to_owned();
                }
                self.devtools.inline_style_cursor = self.devtools.inline_style_input.len();
            }
            FieldKind::SourcesSearch => {
                self.devtools.sources_search_cursor = self.devtools.sources_search.len();
            }
            FieldKind::NetworkFilter => {
                self.devtools.net_search_cursor = self.devtools.net_search.len();
            }
        }
        self.focus = Focus::DevToolsField(kind);
        self.dirty = true;
    }

    pub(crate) fn field_state_mut(&mut self, kind: FieldKind) -> (&mut String, &mut usize, usize) {
        match kind {
            FieldKind::ConsoleFilter => (
                &mut self.devtools.console_search,
                &mut self.devtools.console_search_cursor,
                256,
            ),
            FieldKind::ElementsSearch => (
                &mut self.devtools.elements_search,
                &mut self.devtools.elements_search_cursor,
                256,
            ),
            FieldKind::InlineStyle => (
                &mut self.devtools.inline_style_input,
                &mut self.devtools.inline_style_cursor,
                1024,
            ),
            FieldKind::SourcesSearch => (
                &mut self.devtools.sources_search,
                &mut self.devtools.sources_search_cursor,
                256,
            ),
            FieldKind::NetworkFilter => (
                &mut self.devtools.net_search,
                &mut self.devtools.net_search_cursor,
                256,
            ),
        }
    }

    pub(super) fn devtools_field_insert_text(&mut self, kind: FieldKind, text: &str) {
        let filtered: String = text
            .chars()
            .filter(|ch| !ch.is_control() || *ch == ' ')
            .collect();
        if filtered.is_empty() {
            return;
        }
        let mut anc = self
            .devtools
            .field_anchor
            .filter(|(k, _)| *k == kind)
            .map(|(_, a)| a);
        let changed = {
            let (buf, cur, cap) = self.field_state_mut(kind);
            crate::clipboard::edit_replace_selection_or_insert(buf, cur, &mut anc, &filtered, cap)
        };
        if !changed {
            return;
        }
        self.devtools.field_anchor = None;
        match kind {
            FieldKind::ConsoleFilter | FieldKind::NetworkFilter => {
                self.devtools.row = 0;
            }
            FieldKind::ElementsSearch => {
                self.devtools.elements_match_idx = 0;
                self.jump_elements_search(false);
            }
            FieldKind::SourcesSearch => {
                self.jump_sources_search(false);
            }
            FieldKind::InlineStyle => {}
        }
        self.dirty = true;
    }

    pub(super) fn console_insert_text(&mut self, text: &str) {
        let filtered: String = text
            .chars()
            .filter(|ch| !ch.is_control() || *ch == '\n' || *ch == '\t')
            .collect();
        if filtered.is_empty() {
            return;
        }
        let mut cur = self.devtools.console_cursor;
        let mut anc = self.devtools.console_anchor;
        if crate::clipboard::edit_replace_selection_or_insert(
            &mut self.devtools.console_input,
            &mut cur,
            &mut anc,
            &filtered,
            MAX_CONSOLE_INPUT_BYTES,
        ) {
            self.devtools.console_cursor = cur;
            self.devtools.console_anchor = anc;
            self.devtools.console_history_idx = None;
            self.dirty = true;
        }
    }

    fn console_completion_suffix(&self) -> Option<&'static str> {
        if self.devtools.console_cursor != self.devtools.console_input.len()
            || self.devtools.console_anchor.is_some()
        {
            return None;
        }
        let bytes = self.devtools.console_input.as_bytes();
        let mut start = bytes.len();
        while start > 0 {
            let b = bytes[start - 1];
            if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'.') {
                start -= 1;
            } else {
                break;
            }
        }
        let prefix = &self.devtools.console_input[start..];
        if prefix.is_empty() {
            return None;
        }
        CONSOLE_COMPLETIONS
            .iter()
            .copied()
            .find_map(|cand| cand.strip_prefix(prefix).filter(|rest| !rest.is_empty()))
    }

    pub(super) fn handle_console_key(&mut self, sym: u32, ctrl: bool, shift: bool, alt: bool) {
        if !alt {
            let is_copy = (ctrl && !shift && matches!(sym, 0x63 | 0x43))
                || (ctrl && !shift && matches!(sym, 0xff63 | 0xff9e));
            let is_cut = (ctrl && !shift && matches!(sym, 0x78 | 0x58))
                || (!ctrl && shift && matches!(sym, 0xffff | 0xff9f));
            let is_paste = (ctrl && !shift && matches!(sym, 0x76 | 0x56))
                || (!ctrl && shift && matches!(sym, 0xff63 | 0xff9e));
            if is_copy {
                let _ = self.copy_selection();
                return;
            }
            if is_cut {
                let _ = self.cut_selection();
                return;
            }
            if is_paste {
                self.paste_from_source(crate::clipboard::PasteSource::Clipboard, false);
                return;
            }
            if ctrl && !shift && matches!(sym, 0x61 | 0x41) {
                self.select_all_active();
                return;
            }
        }
        if let Focus::DevToolsField(kind) = self.focus {
            self.handle_devtools_field_key(kind, sym, ctrl, shift, alt);
            return;
        }
        if ctrl && !alt && (sym == u32::from(b'l') || sym == u32::from(b'L')) {
            self.clear_console_and_worker_objects();
            self.dirty = true;
            return;
        }
        if ctrl && !alt && (sym == u32::from(b'u') || sym == u32::from(b'U')) {
            self.devtools.console_input.clear();
            self.devtools.console_cursor = 0;
            self.devtools.console_anchor = None;
            self.devtools.console_history_idx = None;
            self.dirty = true;
            return;
        }
        match sym {
            0xff0d => {
                if shift {
                    self.console_insert_text("\n");
                } else {
                    self.devtools.console_anchor = None;
                    self.submit_console_input();
                }
            }
            0xff09 => {
                if let Some(rest) = self.console_completion_suffix() {
                    let owned = rest.to_owned();
                    self.console_insert_text(&owned);
                } else {
                    self.focus = Focus::DevTools;
                    self.dirty = true;
                }
            }
            0xff08 => {
                let mut cur = self.devtools.console_cursor;
                let mut anc = self.devtools.console_anchor;
                if crate::clipboard::edit_delete_backward(
                    &mut self.devtools.console_input,
                    &mut cur,
                    &mut anc,
                    ctrl,
                ) {
                    self.devtools.console_cursor = cur;
                    self.devtools.console_anchor = anc;
                    self.devtools.console_history_idx = None;
                    self.dirty = true;
                }
            }
            0xffff | 0xff9f => {
                let mut cur = self.devtools.console_cursor;
                let mut anc = self.devtools.console_anchor;
                if crate::clipboard::edit_delete_forward(
                    &mut self.devtools.console_input,
                    &mut cur,
                    &mut anc,
                    ctrl,
                ) {
                    self.devtools.console_cursor = cur;
                    self.devtools.console_anchor = anc;
                    self.dirty = true;
                }
            }
            0xff51 => {
                let mut cur = self.devtools.console_cursor;
                let mut anc = self.devtools.console_anchor;
                let target = if !shift
                    && let Some((b0, _)) =
                        crate::clipboard::normalized_range(&self.devtools.console_input, cur, anc)
                {
                    b0
                } else if ctrl {
                    crate::clipboard::prev_word_boundary(&self.devtools.console_input, cur)
                } else {
                    crate::clipboard::prev_char_boundary(&self.devtools.console_input, cur)
                };
                crate::clipboard::edit_move_cursor(
                    &self.devtools.console_input,
                    &mut cur,
                    &mut anc,
                    target,
                    shift,
                );
                self.devtools.console_cursor = cur;
                self.devtools.console_anchor = anc;
                self.dirty = true;
            }
            0xff53 => {
                let mut cur = self.devtools.console_cursor;
                let mut anc = self.devtools.console_anchor;
                if !shift
                    && anc.is_none()
                    && cur >= self.devtools.console_input.len()
                    && let Some(rest) = self.console_completion_suffix()
                {
                    let owned = rest.to_owned();
                    self.console_insert_text(&owned);
                    return;
                }
                let target = if !shift
                    && let Some((_, b1)) =
                        crate::clipboard::normalized_range(&self.devtools.console_input, cur, anc)
                {
                    b1
                } else if ctrl {
                    crate::clipboard::next_word_boundary(&self.devtools.console_input, cur)
                } else {
                    crate::clipboard::next_char_boundary(&self.devtools.console_input, cur)
                };
                crate::clipboard::edit_move_cursor(
                    &self.devtools.console_input,
                    &mut cur,
                    &mut anc,
                    target,
                    shift,
                );
                self.devtools.console_cursor = cur;
                self.devtools.console_anchor = anc;
                self.dirty = true;
            }
            0xff50 => {
                let mut cur = self.devtools.console_cursor;
                let mut anc = self.devtools.console_anchor;
                crate::clipboard::edit_move_cursor(
                    &self.devtools.console_input,
                    &mut cur,
                    &mut anc,
                    0,
                    shift,
                );
                self.devtools.console_cursor = cur;
                self.devtools.console_anchor = anc;
                self.dirty = true;
            }
            0xff57 => {
                let end = self.devtools.console_input.len();
                let mut cur = self.devtools.console_cursor;
                let mut anc = self.devtools.console_anchor;
                crate::clipboard::edit_move_cursor(
                    &self.devtools.console_input,
                    &mut cur,
                    &mut anc,
                    end,
                    shift,
                );
                self.devtools.console_cursor = cur;
                self.devtools.console_anchor = anc;
                self.dirty = true;
            }
            0xff52 => {
                if !self.devtools.console_history.is_empty() {
                    let next_idx = match self.devtools.console_history_idx {
                        None => {
                            self.devtools.console_draft = self.devtools.console_input.clone();
                            self.devtools.console_history.len() - 1
                        }
                        Some(idx) => idx.saturating_sub(1),
                    };
                    self.devtools.console_history_idx = Some(next_idx);
                    self.devtools.console_input = self.devtools.console_history[next_idx].clone();
                    self.devtools.console_cursor = self.devtools.console_input.len();
                    self.devtools.console_anchor = None;
                    self.dirty = true;
                }
            }
            0xff54 => {
                if let Some(idx) = self.devtools.console_history_idx {
                    if idx + 1 < self.devtools.console_history.len() {
                        let next = idx + 1;
                        self.devtools.console_history_idx = Some(next);
                        self.devtools.console_input = self.devtools.console_history[next].clone();
                    } else {
                        self.devtools.console_history_idx = None;
                        self.devtools.console_input =
                            std::mem::take(&mut self.devtools.console_draft);
                    }
                    self.devtools.console_cursor = self.devtools.console_input.len();
                    self.devtools.console_anchor = None;
                    self.dirty = true;
                }
            }
            0xff55 => self.inspector_scroll(-6),
            0xff56 => self.inspector_scroll(6),
            _ => {
                if !ctrl && !alt {
                    let code = if sym & 0xff000000 == 0x01000000 {
                        sym & 0xffffff
                    } else {
                        sym
                    };
                    if let Some(ch) = char::from_u32(code)
                        && !ch.is_control()
                        && code < 0xff00
                    {
                        self.console_insert_text(&ch.to_string());
                    }
                }
            }
        }
    }

    fn handle_devtools_field_key(
        &mut self,
        kind: FieldKind,
        sym: u32,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) {
        if ctrl && !alt && (sym == u32::from(b'u') || sym == u32::from(b'U')) {
            let (buf, cur, _) = self.field_state_mut(kind);
            buf.clear();
            *cur = 0;
            self.devtools.field_anchor = None;
            self.dirty = true;
            return;
        }
        let mut anc = self
            .devtools
            .field_anchor
            .filter(|(k, _)| *k == kind)
            .map(|(_, a)| a);
        match sym {
            0xff0d => {
                match kind {
                    FieldKind::InlineStyle => self.apply_inline_style_edit(),
                    FieldKind::ElementsSearch => self.jump_elements_search(!shift),
                    FieldKind::SourcesSearch => self.jump_sources_search(!shift),
                    FieldKind::ConsoleFilter | FieldKind::NetworkFilter => {
                        self.focus = Focus::DevTools;
                    }
                }
                self.dirty = true;
            }
            0xff09 => {
                self.focus = Focus::DevTools;
                self.dirty = true;
            }
            0xff08 => {
                let changed = {
                    let (buf, cur, _) = self.field_state_mut(kind);
                    crate::clipboard::edit_delete_backward(buf, cur, &mut anc, ctrl)
                };
                if changed {
                    self.devtools.field_anchor = anc.map(|a| (kind, a));
                    if matches!(kind, FieldKind::ConsoleFilter | FieldKind::NetworkFilter) {
                        self.devtools.row = 0;
                    }
                    self.dirty = true;
                }
            }
            0xffff | 0xff9f => {
                let changed = {
                    let (buf, cur, _) = self.field_state_mut(kind);
                    crate::clipboard::edit_delete_forward(buf, cur, &mut anc, ctrl)
                };
                if changed {
                    self.devtools.field_anchor = anc.map(|a| (kind, a));
                    if matches!(kind, FieldKind::ConsoleFilter | FieldKind::NetworkFilter) {
                        self.devtools.row = 0;
                    }
                    self.dirty = true;
                }
            }
            0xff51 => {
                {
                    let (buf, cur, _) = self.field_state_mut(kind);
                    let target = if !shift
                        && let Some((b0, _)) = crate::clipboard::normalized_range(buf, *cur, anc)
                    {
                        b0
                    } else if ctrl {
                        crate::clipboard::prev_word_boundary(buf, *cur)
                    } else {
                        crate::clipboard::prev_char_boundary(buf, *cur)
                    };
                    crate::clipboard::edit_move_cursor(buf, cur, &mut anc, target, shift);
                }
                self.devtools.field_anchor = anc.map(|a| (kind, a));
                self.dirty = true;
            }
            0xff53 => {
                {
                    let (buf, cur, _) = self.field_state_mut(kind);
                    let target = if !shift
                        && let Some((_, b1)) = crate::clipboard::normalized_range(buf, *cur, anc)
                    {
                        b1
                    } else if ctrl {
                        crate::clipboard::next_word_boundary(buf, *cur)
                    } else {
                        crate::clipboard::next_char_boundary(buf, *cur)
                    };
                    crate::clipboard::edit_move_cursor(buf, cur, &mut anc, target, shift);
                }
                self.devtools.field_anchor = anc.map(|a| (kind, a));
                self.dirty = true;
            }
            0xff50 => {
                {
                    let (buf, cur, _) = self.field_state_mut(kind);
                    crate::clipboard::edit_move_cursor(buf, cur, &mut anc, 0, shift);
                }
                self.devtools.field_anchor = anc.map(|a| (kind, a));
                self.dirty = true;
            }
            0xff57 => {
                {
                    let (buf, cur, _) = self.field_state_mut(kind);
                    let end = buf.len();
                    crate::clipboard::edit_move_cursor(buf, cur, &mut anc, end, shift);
                }
                self.devtools.field_anchor = anc.map(|a| (kind, a));
                self.dirty = true;
            }
            _ => {
                if !ctrl && !alt {
                    let code = if sym & 0xff000000 == 0x01000000 {
                        sym & 0xffffff
                    } else {
                        sym
                    };
                    if let Some(ch) = char::from_u32(code)
                        && !ch.is_control()
                        && code < 0xff00
                    {
                        self.devtools_field_insert_text(kind, &ch.to_string());
                    }
                }
            }
        }
    }

    fn elements_search_matches(&self) -> Vec<usize> {
        let q = self.devtools.elements_search.trim();
        if q.is_empty() || self.document.nodes.is_empty() {
            return Vec::new();
        }
        if let Ok(all) = self.document.query_selector_all(0, q)
            && !all.is_empty()
        {
            return all;
        }
        let needle = q.to_ascii_lowercase();
        self.document
            .nodes
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(id, node)| {
                let hit = node.tag.to_ascii_lowercase().contains(&needle)
                    || node.text.to_ascii_lowercase().contains(&needle)
                    || node.attributes.iter().any(|(k, v)| {
                        k.to_ascii_lowercase().contains(&needle)
                            || v.to_ascii_lowercase().contains(&needle)
                    });
                hit.then_some(id)
            })
            .take(128)
            .collect()
    }

    fn jump_elements_search(&mut self, advance: bool) {
        let matches = self.elements_search_matches();
        if matches.is_empty() {
            return;
        }
        if advance {
            self.devtools.elements_match_idx =
                (self.devtools.elements_match_idx + 1) % matches.len();
        } else {
            self.devtools.elements_match_idx %= matches.len();
        }
        let target = matches[self.devtools.elements_match_idx];
        self.select_devtools_node(target);
        let rows = self.dom_tree_rows();
        if let Some(row_pos) = rows.iter().position(|(id, _, _)| *id == target) {
            self.devtools.row = row_pos.saturating_sub(2);
        }
    }

    fn jump_sources_search(&mut self, advance: bool) {
        let q = self.devtools.sources_search.trim().to_ascii_lowercase();
        if q.is_empty() {
            self.devtools.sources_match_line = None;
            return;
        }
        let sources = self.collect_sources();
        let Some((_, _, content)) = sources.get(self.devtools.selected_source) else {
            return;
        };
        let matching_lines: Vec<usize> = content
            .lines()
            .enumerate()
            .filter_map(|(i, line)| line.to_ascii_lowercase().contains(&q).then_some(i))
            .collect();
        if matching_lines.is_empty() {
            self.devtools.sources_match_line = None;
            return;
        }
        let next_line = match self.devtools.sources_match_line {
            Some(cur) if advance => matching_lines
                .iter()
                .copied()
                .find(|&l| l > cur)
                .unwrap_or(matching_lines[0]),
            Some(cur) if matching_lines.contains(&cur) => cur,
            _ => matching_lines[0],
        };
        self.devtools.sources_match_line = Some(next_line);
        self.devtools.row = next_line.saturating_sub(2);
    }

    pub(super) fn clear_console_and_worker_objects(&mut self) {
        self.devtools.clear_console();
        if self.page_session.is_some() && !self.script_blocked {
            self.enqueue_console_command(ConsoleCommand::ClearObjects, ConsoleTarget::Clear);
        }
    }

    fn submit_console_input(&mut self) {
        let raw = self.devtools.console_input.trim().to_owned();
        self.devtools.console_input.clear();
        self.devtools.console_cursor = 0;
        self.devtools.console_history_idx = None;
        self.devtools.console_draft.clear();
        if raw.is_empty() {
            self.dirty = true;
            return;
        }
        if self.devtools.console_history.last() != Some(&raw) {
            if self.devtools.console_history.len() >= MAX_CONSOLE_HISTORY {
                self.devtools.console_history.remove(0);
            }
            self.devtools.console_history.push(raw.clone());
        }
        let mut lines = raw.lines();
        let first = bounded(lines.next().unwrap_or(""), MAX_MESSAGE_BYTES);
        let extra: Vec<String> = lines.take(24).map(|l| bounded(l, 512)).collect();
        self.devtools.push_interactive_entry(ConsoleEntry {
            level: ConsoleLevel::Info,
            kind: ConsoleEntryKind::Command,
            source: "REPL",
            message: format!("> {first}"),
            extra_lines: extra,
            count: 1,
            group_depth: 0,
            style: None,
            object_id: None,
            node_id: None,
            expanded: false,
            properties: None,
        });
        self.devtools.scroll_to_bottom = true;
        self.dirty = true;

        if raw == "clear()" || raw == "console.clear()" {
            self.clear_console_and_worker_objects();
            return;
        }

        if let Err(err) = self.ensure_console_session() {
            self.devtools.push_interactive_entry(ConsoleEntry {
                level: ConsoleLevel::Error,
                kind: ConsoleEntryKind::Result,
                source: "REPL",
                message: format!("Uncaught ConsoleError: {}", bounded(&err, 512)),
                extra_lines: Vec::new(),
                count: 1,
                group_depth: 0,
                style: None,
                object_id: None,
                node_id: None,
                expanded: false,
                properties: None,
            });
            self.devtools.scroll_to_bottom = true;
            return;
        }

        let inspected_nodes: Vec<usize> = if self.console_detached_dom {
            Vec::new()
        } else {
            self.devtools
                .inspected_history
                .iter()
                .filter_map(|&(epoch, nid)| {
                    (epoch == self.dom_epoch && nid < self.document.nodes.len()).then_some(nid)
                })
                .take(mg_sparkle::page_session::console::MAX_CONSOLE_INSPECTED_NODES)
                .collect()
        };
        self.enqueue_console_command(
            ConsoleCommand::Evaluate {
                source: raw,
                inspected_nodes,
            },
            ConsoleTarget::Repl,
        );
    }

    pub(super) fn toggle_console_object(&mut self, entry_idx: usize, object_id: u64) {
        let Some(entry) = self.devtools.console.get_mut(entry_idx) else {
            return;
        };
        if entry.expanded {
            entry.expanded = false;
            self.dirty = true;
            return;
        }
        if entry.properties.is_some() {
            entry.expanded = true;
            self.dirty = true;
            return;
        }
        if object_id == 0 || self.page_session.is_none() || self.script_blocked {
            return;
        }
        self.enqueue_console_command(
            ConsoleCommand::InspectObject { object_id },
            ConsoleTarget::InspectEntry {
                entry_idx,
                object_id,
            },
        );
    }

    fn ensure_console_session(&mut self) -> Result<(), String> {
        if self.loading || self.fetching {
            return Err("Page is still loading; wait for load completion".into());
        }
        if self.page_session.is_some() && !self.script_blocked {
            return Ok(());
        }
        if matches!(
            self.child_pool.observer_expectation(),
            mg_sparkle::page_session::observer::ObserverExpectation::WindowHarness { .. }
        ) {
            return Err("Console ad-hoc worker disabled under WindowHarness observer".into());
        }
        if let Some(old) = self.page_session.take() {
            old.cancel();
        }
        let (html, truncated) = serialize_document_for_console(&self.document, 240 * 1024);
        let detached = truncated || self.document.nodes.len() > 5000;
        let url = if url::Url::parse(&self.page_url)
            .is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
        {
            self.page_url.clone()
        } else {
            "https://mgbrowser.local/".to_owned()
        };
        let external_scripts = match self.child_pool.source_wire_mode() {
            scripts::SourceWireMode::Modern => Some(Vec::new()),
            scripts::SourceWireMode::Legacy => None,
        };
        self.child_pool.set_generation(self.generation)?;
        let started_at = Instant::now();
        let (mut session, mut reply) = self.child_pool.start(
            mg_sparkle::js_browser::Request {
                url: url.clone(),
                html,
                external_scripts,
            },
            self.generation,
        )?;
        let (init_console, _) = session.take_console();
        if let Err(err) = session.charge_active(started_at.elapsed()) {
            session.cancel();
            return Err(err);
        }
        if reply.state != RealmState::Ready {
            session.cancel();
            return Err("Isolated Console worker failed to reach Ready state".into());
        }
        if self.child_pool.source_wire_mode() == scripts::SourceWireMode::Modern {
            let mut ledger = ObserverLedger::new(self.child_pool.observer_expectation())
                .map_err(|e| e.to_string())?;
            let Some(obs) = reply.observer.take() else {
                session.cancel();
                return Err("Modern Console worker omitted observation".into());
            };
            if let Err(err) = ledger.stage(obs).map(|s| s.commit()) {
                session.cancel();
                return Err(err.to_string());
            }
            self.observer_ledger = Some(ledger);
        } else {
            self.observer_ledger = None;
        }
        let lifecycle = match self.validate_lifecycle_reply(&reply, None) {
            Ok(lc) => lc,
            Err(err) => {
                session.cancel();
                return Err(err);
            }
        };
        self.last_task_wake_token = 0;
        let wake = match self.validate_task_wake(&reply, started_at) {
            Ok(w) => w,
            Err(err) => {
                session.cancel();
                return Err(err);
            }
        };
        if !detached {
            match Self::project_reply(&mut reply, &url, None) {
                Ok(projected) => {
                    let old_doc = self.document.clone();
                    self.devtools.remap_nodes_between_documents(
                        self.dom_epoch,
                        &old_doc,
                        &projected,
                    );
                    self.acknowledge_projection(projected, &[]);
                }
                Err(_) => {
                    self.console_detached_dom = true;
                }
            }
        }
        self.console_detached_dom = detached;
        self.lifecycle = lifecycle;
        self.last_accounting = Some(reply.accounting_snapshot());
        self.page_revision = reply.revision;
        self.replace_task_wake(wake);
        self.script_blocked = false;
        self.ingest_wire_console_and_sync(init_console, !detached);
        self.page_session = Some(session);
        Ok(())
    }

    pub(crate) fn ingest_wire_console_and_sync(
        &mut self,
        entries: Vec<mg_sparkle::page_session::console::ConsoleWireEntry>,
        allow_dom_links: bool,
    ) {
        self.devtools.ingest_wire_console(entries, allow_dom_links);
        if let Some(txt) = self.devtools.pending_clipboard_sync.take() {
            self.write_clipboard(txt, false);
        }
    }

    fn enqueue_console_command(&mut self, cmd: ConsoleCommand, target: ConsoleTarget) {
        if self.queued_console.len() >= 16 {
            return;
        }
        self.queued_console.push_back((cmd, target));
        self.pump_queued_console();
    }

    pub(super) fn pump_queued_console(&mut self) {
        if self.queued_console.is_empty()
            || self.fetching
            || self.script_blocked
            || self.pending_transaction.is_some()
            || self.automation_completion.is_some()
        {
            return;
        }
        if self
            .lifecycle
            .is_some_and(|lc| lc.phase != LifecyclePhase::LoadComplete)
            && self.task_wake.is_some()
        {
            self.poll_task_wake(Instant::now());
            return;
        }
        let Some((cmd, target)) = self.queued_console.pop_front() else {
            return;
        };
        let Some(session) = self.page_session.as_mut() else {
            return;
        };
        let session_id = session.id();
        match session.try_console(cmd, self.page_revision) {
            Ok(sequence) => {
                self.task_wake = None;
                self.pending_transaction = Some(PendingTransaction::Console {
                    identity: TransactionIdentity {
                        generation: self.generation,
                        session_id,
                        sequence,
                        revision: self.page_revision,
                    },
                    target,
                });
                self.dirty = true;
            }
            Err(error) => {
                if let Some(ledger) = &mut self.observer_ledger {
                    ledger.fail_parent(ObserverParentFailure::Transport);
                }
                self.block_session(&error);
            }
        }
    }

    pub(super) fn finish_console_transaction(
        &mut self,
        target: ConsoleTarget,
        result: ConsoleResult,
        terminal: bool,
    ) {
        use mg_sparkle::page_session::console::ConsoleValueKind;
        if let Some(txt) = self.devtools.pending_clipboard_sync.take() {
            self.write_clipboard(txt, false);
        }
        if let Some(nid) = self.devtools.pending_inspect_node.take()
            && !self.console_detached_dom
            && nid < self.document.nodes.len()
        {
            self.select_devtools_node(nid);
            self.open_devtools_tab(Tab::Elements);
        }
        match (target, result) {
            (ConsoleTarget::Clear, _) | (_, ConsoleResult::Cleared) => {}
            (
                ConsoleTarget::Repl,
                ConsoleResult::Evaluated {
                    value_kind,
                    preview,
                    object_id,
                    node_id,
                },
            ) => {
                let is_exc = value_kind == ConsoleValueKind::Exception;
                let mut lines = preview.lines();
                let first = bounded(lines.next().unwrap_or("undefined"), MAX_MESSAGE_BYTES);
                let extra: Vec<String> = lines.take(24).map(|l| bounded(l, 512)).collect();
                let prefix = if is_exc { "" } else { "<- " };
                self.devtools.push_interactive_entry(ConsoleEntry {
                    level: if is_exc {
                        ConsoleLevel::Error
                    } else {
                        ConsoleLevel::Info
                    },
                    kind: ConsoleEntryKind::Result,
                    source: "REPL",
                    message: format!("{prefix}{first}"),
                    extra_lines: extra,
                    count: 1,
                    group_depth: 0,
                    style: None,
                    object_id: if terminal { None } else { object_id },
                    node_id: node_id.filter(|_| !self.console_detached_dom),
                    expanded: false,
                    properties: None,
                });
                self.devtools.scroll_to_bottom = true;
            }
            (ConsoleTarget::Repl, ConsoleResult::Rejected { diagnostic }) => {
                self.devtools.push_interactive_entry(ConsoleEntry {
                    level: ConsoleLevel::Error,
                    kind: ConsoleEntryKind::Result,
                    source: "REPL",
                    message: bounded(&diagnostic, MAX_MESSAGE_BYTES),
                    extra_lines: Vec::new(),
                    count: 1,
                    group_depth: 0,
                    style: None,
                    object_id: None,
                    node_id: None,
                    expanded: false,
                    properties: None,
                });
                self.devtools.scroll_to_bottom = true;
            }
            (
                ConsoleTarget::InspectEntry {
                    entry_idx,
                    object_id: req_id,
                },
                ConsoleResult::Properties { object_id, entries },
            ) if req_id == object_id => {
                if let Some(entry) = self.devtools.console.get_mut(entry_idx)
                    && entry.object_id == Some(object_id)
                {
                    entry.properties = Some(entries);
                    entry.expanded = true;
                }
            }
            _ => {}
        }
        self.dirty = true;
    }

    fn inspected_node_at(&self, x: i32, y: i32) -> Option<usize> {
        if self.devtools.painted_epoch != self.dom_epoch || self.boxes.len() >= 100_000 {
            return None;
        }
        let mut best = None;
        let mut work = 0usize;
        for rect in &self.boxes {
            if x < rect.x
                || y < rect.y
                || i64::from(x) >= i64::from(rect.x) + i64::from(rect.width)
                || i64::from(y) >= i64::from(rect.y) + i64::from(rect.height)
            {
                continue;
            }
            let mut id = rect.node;
            let node = self.document.nodes.get(id)?;
            if node.tag == "#text" {
                id = node.parent;
            }
            let mut ancestor = id;
            let mut depth = 0;
            while ancestor != 0 && depth < 256 {
                work += 1;
                if work > 4_000_000 {
                    return None;
                }
                let node = self.document.nodes.get(ancestor)?;
                if node.parent == ancestor {
                    return None;
                }
                ancestor = node.parent;
                depth += 1;
            }
            if best.is_none_or(|(_, previous)| depth >= previous) {
                best = Some((id, depth));
            }
        }
        best.map(|(id, _)| id)
    }

    fn node_painted_bounds(&self, epoch: u64, id: usize) -> Option<(i64, i64, i64, i64)> {
        if self.devtools.painted_epoch != epoch || self.boxes.len() >= 100_000 {
            return None;
        }
        let mut bounds: Option<(i64, i64, i64, i64)> = None;
        let mut work = 0usize;
        for rect in &self.boxes {
            let mut source = rect.node;
            for _ in 0..=256 {
                work += 1;
                if work > 4_000_000 {
                    return None;
                }
                if source == id {
                    let next = (
                        i64::from(rect.x),
                        i64::from(rect.y - self.page_top()),
                        i64::from(rect.x) + i64::from(rect.width),
                        i64::from(rect.y - self.page_top()) + i64::from(rect.height),
                    );
                    bounds = Some(match bounds {
                        None => next,
                        Some(old) => (
                            old.0.min(next.0),
                            old.1.min(next.1),
                            old.2.max(next.2),
                            old.3.max(next.3),
                        ),
                    });
                    break;
                }
                let Some(node) = self.document.nodes.get(source) else {
                    break;
                };
                if source == 0 || source == node.parent {
                    break;
                }
                source = node.parent;
            }
        }
        bounds
    }

    pub(super) fn inspector_scroll(&mut self, rows: i32) {
        if !self.devtools.open {
            return;
        }
        self.devtools.scroll_to_bottom = false;
        let visible_rows = self.inspector_rows();
        let total = match self.devtools.tab {
            Tab::Network if self.devtools_rich_layout() => self.filtered_network_indices().len(),
            Tab::Elements if self.devtools_rich_layout() => self.dom_tree_rows().len(),
            Tab::Sources if self.devtools_rich_layout() => {
                let sources = self.collect_sources();
                sources
                    .get(self.devtools.selected_source)
                    .map_or(1, |(_, _, body)| body.lines().count().max(1))
            }
            Tab::Console if self.devtools_rich_layout() => {
                let (_, _, w, _) = self.devtools_panel_rect();
                self.build_console_visual_rows(w.saturating_sub(92).max(80) as f32)
                    .len()
            }
            _ => self.inspector_display_lines().len(),
        };
        self.devtools.row = self
            .devtools
            .row
            .saturating_add_signed(rows as isize)
            .min(total.saturating_sub(visible_rows));
        self.pointer_press = None;
        self.dirty = true;
    }

    fn devtools_rich_layout(&self) -> bool {
        let (_, _, w, h) = self.devtools_panel_rect();
        w >= 340 && h >= 175
    }

    fn inspector_rows(&self) -> usize {
        let (_, _, _, h) = self.devtools_panel_rect();
        let body_h = h.saturating_sub(98);
        let list_h = match self.devtools.tab {
            Tab::Elements if self.devtools_rich_layout() => body_h * 50 / 100,
            Tab::Console if self.devtools_rich_layout() => body_h.saturating_sub(32),
            Tab::Network
                if self.devtools_rich_layout() && self.devtools.selected_request.is_some() =>
            {
                body_h.saturating_sub(152)
            }
            Tab::Network if self.devtools_rich_layout() => body_h.saturating_sub(22),
            Tab::Performance if self.devtools_rich_layout() => body_h.saturating_sub(74),
            _ => body_h,
        };
        (list_h / 20).clamp(1, 120) as usize
    }

    fn filtered_network_indices(&self) -> Vec<usize> {
        let q = self.devtools.net_search.trim();
        self.devtools
            .network
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                if !entry.kind.matches(self.devtools.net_filter) {
                    return None;
                }
                if !q.is_empty() && !matches_console_search(q, entry.kind.label(), &entry.url, &[])
                {
                    return None;
                }
                Some(idx)
            })
            .collect()
    }

    fn dom_tree_rows(&self) -> Vec<(usize, usize, String)> {
        let mut out = Vec::new();
        if self.document.nodes.is_empty() {
            return out;
        }
        let selected_id = self
            .devtools
            .selected
            .filter(|(epoch, _)| *epoch == self.dom_epoch)
            .map(|(_, id)| id);
        let mut focus_chain = HashSet::new();
        if let Some(mut cur) = selected_id {
            for _ in 0..64 {
                focus_chain.insert(cur);
                let Some(node) = self.document.nodes.get(cur) else {
                    break;
                };
                if cur == 0 || node.parent == cur {
                    break;
                }
                cur = node.parent;
            }
        }
        for (idx, node) in self.document.nodes.iter().enumerate().take(16) {
            if matches!(node.tag.as_str(), "#document" | "html" | "body") {
                focus_chain.insert(idx);
            }
        }
        let mut stack = vec![(0usize, 0usize)];
        while let Some((id, depth)) = stack.pop() {
            if out.len() >= 256 {
                break;
            }
            let Some(node) = self.document.nodes.get(id) else {
                continue;
            };
            if node.tag == "#text" {
                let trimmed = node.text.trim();
                if !trimmed.is_empty() {
                    out.push((id, depth, format!("\"{}\"", bounded(trimmed, 64))));
                }
                continue;
            }
            let key = (self.dom_epoch, id);
            let expanded = if self.devtools.collapsed_nodes.contains(&key) {
                false
            } else if self.devtools.expanded_nodes.contains(&key) {
                true
            } else {
                depth < 2 || focus_chain.contains(&id)
            };
            let marker = if node.children.is_empty() {
                "  "
            } else if expanded {
                "▾ "
            } else {
                "▸ "
            };
            let label = if node.tag == "#document" {
                format!("{marker}#document")
            } else {
                let mut header = format!("{marker}<{}", bounded(&node.tag, 28));
                if let Some(attr_id) = node.attr("id").filter(|s| !s.is_empty()) {
                    header.push_str(&format!(" id=\"{}\"", bounded(attr_id, 28)));
                }
                if let Some(cls) = node.attr("class").filter(|s| !s.is_empty()) {
                    header.push_str(&format!(" class=\"{}\"", bounded(cls, 32)));
                }
                if let Some(href) = node.attr("href").filter(|s| !s.is_empty()) {
                    header.push_str(&format!(" href=\"{}\"", bounded(href, 28)));
                }
                header.push('>');
                let inline_text = node
                    .children
                    .iter()
                    .filter_map(|&cid| self.document.nodes.get(cid))
                    .find(|child| child.tag == "#text" && !child.text.trim().is_empty())
                    .map(|child| bounded(child.text.trim(), 34));
                if let Some(text) = inline_text
                    && (!expanded || node.children.len() == 1)
                {
                    header.push_str(&text);
                    header.push_str(&format!("</{}>", bounded(&node.tag, 20)));
                } else if !expanded && !node.children.is_empty() {
                    header.push_str("…");
                    header.push_str(&format!("</{}>", bounded(&node.tag, 20)));
                }
                if selected_id == Some(id) {
                    header.push_str(" == $0");
                }
                header
            };
            out.push((id, depth, label));
            if expanded {
                for &child in node.children.iter().take(64).rev() {
                    if self
                        .document
                        .nodes
                        .get(child)
                        .is_some_and(|c| c.tag == "#text")
                        && node.children.len() == 1
                    {
                        continue;
                    }
                    stack.push((child, depth.saturating_add(1).min(12)));
                }
            }
        }
        out
    }

    fn collect_sources(&self) -> Vec<(&'static str, String, String)> {
        let mut files = Vec::new();
        let html = if !self.devtools.page_source.is_empty() {
            self.devtools.page_source.clone()
        } else {
            serialize_document_for_console(&self.document, 64 * 1024).0
        };
        let doc_label = if self.page_url == "about:blank" {
            "(document) about:blank".into()
        } else {
            format!("(document) {}", short_resource_name(&self.page_url))
        };
        files.push(("HTML", doc_label, html));

        for (i, sheet) in self.document.stylesheets.iter().take(24).enumerate() {
            let name = if sheet.base_url == self.page_url || sheet.base_url.is_empty() {
                format!("inline <style> #{}", i + 1)
            } else {
                short_resource_name(&sheet.base_url)
            };
            files.push(("CSS", name, sheet.css.clone()));
        }

        let exchanges = self.session.recent_exchanges();
        let mut inline_js = 0usize;
        for (nid, node) in self.document.nodes.iter().enumerate() {
            if !node.tag.eq_ignore_ascii_case("script") {
                continue;
            }
            if let Some(src) = node.attr("src").filter(|s| !s.is_empty()) {
                let resolved = url::Url::parse(&self.page_url)
                    .and_then(|base| base.join(src))
                    .map(|u| u.to_string())
                    .unwrap_or_else(|_| src.to_owned());
                let body = exchanges
                    .iter()
                    .rev()
                    .find(|ex| ex.url == resolved || ex.url.ends_with(src))
                    .and_then(|ex| ex.body_preview.clone())
                    .unwrap_or_else(|| format!("// External script: {resolved}\n// (Body not retained in preview cache)"));
                files.push(("JS", short_resource_name(&resolved), body));
            } else {
                let mut script_text = String::new();
                for &cid in &node.children {
                    if let Some(child) = self.document.nodes.get(cid)
                        && child.tag == "#text"
                    {
                        script_text.push_str(&child.text);
                    }
                }
                if !script_text.trim().is_empty() {
                    inline_js += 1;
                    files.push((
                        "JS",
                        format!("inline <script> #{inline_js} (node {nid})"),
                        script_text,
                    ));
                }
            }
            if files.len() >= 48 {
                break;
            }
        }
        files
    }

    #[cfg_attr(not(feature = "chrome"), allow(dead_code))]
    fn matched_css_rules_for(&self, node_id: usize) -> Vec<(String, String, String)> {
        let Some(node) = self.document.nodes.get(node_id) else {
            return Vec::new();
        };
        if node.tag.starts_with('#') {
            return Vec::new();
        }
        let tag_low = node.tag.to_ascii_lowercase();
        let id_attr = node.attr("id").unwrap_or("").trim();
        let classes: Vec<&str> = node
            .attr("class")
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let mut out = Vec::new();
        for (idx, sheet) in self.document.stylesheets.iter().take(16).enumerate() {
            let origin = if sheet.base_url == self.page_url || sheet.base_url.is_empty() {
                format!("<style> #{}", idx + 1)
            } else {
                short_resource_name(&sheet.base_url)
            };
            for chunk in sheet.css.split('}').take(256) {
                let Some((raw_sel, raw_body)) = chunk.split_once('{') else {
                    continue;
                };
                let sel = raw_sel
                    .rsplit_once('}')
                    .map_or(raw_sel, |(_, tail)| tail)
                    .trim();
                if sel.is_empty() || sel.starts_with('@') || sel.len() > 120 {
                    continue;
                }
                let matched = sel.split(',').any(|part| {
                    let s = part.trim();
                    if s == "*" || s.eq_ignore_ascii_case(&tag_low) {
                        return true;
                    }
                    if !id_attr.is_empty() && s == format!("#{id_attr}") {
                        return true;
                    }
                    if !id_attr.is_empty()
                        && s.eq_ignore_ascii_case(&format!("{tag_low}#{id_attr}"))
                    {
                        return true;
                    }
                    classes.iter().any(|cls| {
                        s == format!(".{cls}")
                            || s.eq_ignore_ascii_case(&format!("{tag_low}.{cls}"))
                    })
                });
                if matched {
                    out.push((
                        bounded(sel, 72),
                        bounded(raw_body.trim(), 220),
                        origin.clone(),
                    ));
                    if out.len() >= 12 {
                        return out;
                    }
                }
            }
        }
        out
    }

    #[cfg_attr(not(feature = "chrome"), allow(dead_code))]
    fn computed_style_pairs(&self, node_id: usize) -> Vec<(&'static str, String, Option<u32>)> {
        let Some(cs) = self.style_cache.style_for(node_id) else {
            return Vec::new();
        };
        let fg_hex = format!("#{:06x}", cs.color.rgb & 0xffffff);
        let bg_str = if cs.background_color.alpha <= 0.001 {
            "rgba(0, 0, 0, 0)".into()
        } else if cs.background_color.alpha >= 0.999 {
            format!("#{:06x}", cs.background_color.rgb & 0xffffff)
        } else {
            let rgb = cs.background_color.rgb;
            format!(
                "rgba({}, {}, {}, {:.2})",
                (rgb >> 16) & 0xff,
                (rgb >> 8) & 0xff,
                rgb & 0xff,
                cs.background_color.alpha
            )
        };
        let bg_swatch = (cs.background_color.alpha > 0.01).then_some(cs.background_color.rgb);
        let families = if cs.font_families.is_empty() {
            "sans-serif".into()
        } else {
            cs.font_families.join(", ")
        };
        let margin_str = format!(
            "{} {} {} {}",
            format_length(cs.margin[0]),
            format_length(cs.margin[1]),
            format_length(cs.margin[2]),
            format_length(cs.margin[3]),
        );
        let padding_str = format!(
            "{} {} {} {}",
            format_length(cs.padding[0]),
            format_length(cs.padding[1]),
            format_length(cs.padding[2]),
            format_length(cs.padding[3]),
        );
        let border_str = format!(
            "{:.0}px {:.0}px {:.0}px {:.0}px",
            cs.border_width[0], cs.border_width[1], cs.border_width[2], cs.border_width[3]
        );
        let mut props = vec![
            (
                "display",
                format!("{:?}", cs.display).to_ascii_lowercase(),
                None,
            ),
            (
                "position",
                format!("{:?}", cs.layout.position).to_ascii_lowercase(),
                None,
            ),
            (
                "box-sizing",
                match cs.layout.box_sizing {
                    mg_sparkle::style::BoxSizing::ContentBox => "content-box".into(),
                    mg_sparkle::style::BoxSizing::BorderBox => "border-box".into(),
                },
                None,
            ),
            ("width", format_length(cs.width), None),
            ("height", format_length(cs.height), None),
            ("margin", margin_str, None),
            ("border-width", border_str, None),
            ("padding", padding_str, None),
            ("color", fg_hex, Some(cs.color.rgb)),
            ("background-color", bg_str, bg_swatch),
            ("font-family", bounded(&families, 64), None),
            (
                "font-size",
                format!("{:.1}px", cs.font_size).replace(".0px", "px"),
                None,
            ),
            ("font-weight", cs.font_weight.to_string(), None),
            (
                "line-height",
                cs.line_height
                    .map_or_else(|| "normal".into(), |lh| format!("{lh:.1}px")),
                None,
            ),
            (
                "overflow",
                format!("{:?} {:?}", cs.layout.overflow[0], cs.layout.overflow[1])
                    .to_ascii_lowercase(),
                None,
            ),
        ];
        if matches!(
            cs.display,
            mg_sparkle::style::Display::Flex | mg_sparkle::style::Display::InlineFlex
        ) {
            props.push((
                "flex-direction",
                format!("{:?}", cs.layout.flex_direction).to_ascii_lowercase(),
                None,
            ));
            props.push((
                "justify-content",
                format!("{:?}", cs.layout.justify_content.keyword).to_ascii_lowercase(),
                None,
            ));
            props.push((
                "align-items",
                format!("{:?}", cs.layout.align_items.keyword).to_ascii_lowercase(),
                None,
            ));
        }
        if let Some(z) = cs.layout.z_index {
            props.push(("z-index", z.to_string(), None));
        }
        props
    }

    // Wrap bounded source text using the same logical font metrics as painting.
    // Long resource URLs must not hide the error at the end of a row.
    fn inspector_display_lines(&mut self) -> Vec<String> {
        let (_, _, panel_w, _) = self.devtools_panel_rect();
        let width = panel_w.saturating_sub(32).max(80) as f32;
        let raw_lines = self.inspector_lines();
        self.wrap_lines(&raw_lines, width)
    }

    fn wrap_lines(&mut self, raw_lines: &[String], width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for raw in raw_lines {
            let clean = bounded(raw, 2048);
            let mut text = clean.as_str();
            if text.is_empty() {
                lines.push(String::new());
            }
            while !text.is_empty() {
                if lines.len() >= 2047 {
                    lines.push("Additional inspector output omitted by row bound.".into());
                    return lines;
                }
                let mut edges: Vec<_> = text.char_indices().map(|(index, _)| index).collect();
                edges.push(text.len());
                let mut low = 1;
                let mut high = edges.len() - 1;
                while low < high {
                    let middle = (low + high).div_ceil(2);
                    if self.fonts.width(&text[..edges[middle]], 13.) <= width {
                        low = middle;
                    } else {
                        high = middle - 1;
                    }
                }
                let mut edge = edges[low];
                if edge < text.len()
                    && let Some(space) = text[..edge].rfind(' ')
                    && space > edge / 2
                {
                    edge = space;
                }
                lines.push(text[..edge].to_owned());
                text = text[edge..].trim_start();
            }
        }
        lines
    }

    pub(super) fn performance_lines(&self) -> Vec<String> {
        let p = &self.devtools.performance;
        let full = &p.last_full_render;
        let last = &p.last_frame;
        let load_total = p.fetch_html_ms
            + p.parse_html_ms
            + p.resources_ms
            + p.external_scripts_ms
            + p.script_worker_ms;
        let mut lines = vec![
            format!("Performance Dashboard : {}", bounded(&self.page_url, 240)),
            "Shortcut: Ctrl+Shift+P (or F12 -> Performance) | CLI: mgbrowser --profile <URL>"
                .into(),
            format!(
                "Page Load Breakdown (measured phases {:.1} ms):",
                load_total
            ),
            format!(
                "  Navigation: {}",
                if self.loading {
                    "loading"
                } else if self.last_load_ok {
                    "successful HTTP response"
                } else {
                    "unsuccessful response or load failure"
                }
            ),
            format!(
                "  HTML fetch: {:.1} ms ({} bytes)",
                p.fetch_html_ms, p.html_bytes
            ),
            format!(
                "  HTML parse: {:.1} ms ({} DOM nodes)",
                p.parse_html_ms,
                self.document.nodes.len()
            ),
            format!(
                "  Subresources (CSS/fonts/images): {:.1} ms ({} requests, {} charged bytes)",
                p.resources_ms, p.resource_requests, p.resource_bytes
            ),
            format!(
                "  External scripts (<script src>): {:.1} ms ({} admitted, {} charged bytes)",
                p.external_scripts_ms, p.external_scripts_loaded, p.external_scripts_bytes
            ),
            format!(
                "  Boa JS worker: {:.1} ms ({} scripts executed, {} opcodes)",
                p.script_worker_ms, p.scripts_executed, p.boa_opcodes,
            ),
            if p.worker_allocation_recorded {
                format!(
                    "  Requested worker allocation: {} KiB cumulative, {} KiB peak outstanding (not GC heap/RSS)",
                    p.boa_alloc_bytes / 1024,
                    p.boa_peak_bytes / 1024
                )
            } else {
                "  Requested worker allocation: unavailable (no worker memory report)".into()
            },
            format!(
                "Render & Frame Cache Pipeline (viewport {}x{} @ {}%):",
                self.page_viewport_width(),
                self.viewport_height(),
                self.effective_scale_percent()
            ),
            format!(
                "  Last full render: {:.2} ms (style {:.2} ms [{}], layout {:.2} ms, paint {:.2} ms)",
                full.total_ms,
                full.style_ms,
                if full.style_cached { "cached" } else { "Stylo" },
                full.layout_ms,
                full.paint_ms
            ),
            format!(
                "  Last completed Browser paint: {:.2} ms [{}] | Peak: {:.2} ms",
                last.total_ms,
                last.cache_label(),
                p.max_frame_ms
            ),
            format!(
                "  Composition/geometry: {:.2} ms | Chrome/Inspector: {:.2} ms",
                last.composition_ms, last.chrome_ms
            ),
            format!(
                "  Frames painted: {} | Frame cache hits: {} | Style cache hits: {}",
                p.frames_painted, p.frame_cache_hits, p.style_cache_hits
            ),
            format!(
                "  Retained snapshot payload: Frame {} KiB / {} KiB; Style {} KiB / {} KiB (per Browser)",
                p.frame_cache_payload_bytes / 1024,
                crate::FRAME_CACHE_PAYLOAD_BYTES / 1024,
                p.style_cache_payload_bytes / 1024,
                mg_sparkle::render::STYLE_CACHE_PAYLOAD_BYTES / 1024
            ),
            format!(
                "  Shaping lookups (Fonts lifetime): {} hits, {} misses, {} evictions",
                p.shaping_cache.hits, p.shaping_cache.misses, p.shaping_cache.evictions
            ),
            format!(
                "  Shaping retained: {} entries, {} KiB across loaded faces; cap 4096 entries / 1024 KiB per face",
                p.shaping_cache.entries,
                p.shaping_cache.payload_bytes / 1024
            ),
            format!(
                "  Scene complexity: {} DOM nodes, {} layout boxes, {} paint ops",
                full.dom_nodes, full.layout_boxes, full.paint_ops
            ),
        ];
        if p.slowest_resources.is_empty() {
            lines.push("Slowest Subresources & External Scripts: (none fetched)".into());
        } else {
            lines.push("Slowest Subresources & External Scripts:".into());
            for r in &p.slowest_resources {
                let status = r
                    .status
                    .map(|code| format!("HTTP {code}"))
                    .unwrap_or_else(|| "err".into());
                lines.push(bounded(
                    &format!(
                        "  {:>6.1} ms  [{:<10}] {:>7} B  {} ({status})",
                        r.duration_ms,
                        r.kind,
                        r.bytes,
                        bounded(&r.url, 120),
                    ),
                    512,
                ));
            }
        }
        lines.truncate(MAX_LINES);
        lines
    }

    fn console_filtered_entries(&self) -> Vec<(ConsoleLevel, &'static str, String)> {
        let mut entries = Vec::new();
        let filter = self.devtools.console_filter;
        let search = self.devtools.console_search.trim();
        for entry in &self.devtools.console {
            let keep = match filter {
                ConsoleFilter::All => true,
                ConsoleFilter::Errors => entry.level == ConsoleLevel::Error,
                ConsoleFilter::Warnings => entry.level == ConsoleLevel::Warning,
                ConsoleFilter::Info => entry.level == ConsoleLevel::Info,
                ConsoleFilter::Verbose => entry.level == ConsoleLevel::Verbose,
            };
            if keep
                && matches_console_search(search, entry.source, &entry.message, &entry.extra_lines)
            {
                let text = if entry.count > 1 {
                    format!("(×{}) {}", entry.count, entry.message)
                } else {
                    entry.message.clone()
                };
                entries.push((entry.level, entry.source, text));
            }
        }
        if matches!(filter, ConsoleFilter::All | ConsoleFilter::Warnings) {
            for entry in &self.devtools.rendering.entries {
                let location = match (entry.line, entry.column) {
                    (Some(line), Some(column)) => format!(":{line}:{column}"),
                    _ => String::new(),
                };
                let node = entry
                    .node
                    .map(|id| format!(" node {id}"))
                    .unwrap_or_default();
                let formatted = bounded(
                    &format!(
                        "{}{} {}{}: {}",
                        entry.kind, node, entry.source, location, entry.message
                    ),
                    1024,
                );
                if matches_console_search(search, "CSS", &formatted, &[]) {
                    entries.push((ConsoleLevel::Warning, "CSS", formatted));
                }
            }
        }
        let omitted = self
            .devtools
            .omitted
            .saturating_add(self.devtools.rendering.omitted);
        if omitted > 0 {
            entries.push((
                ConsoleLevel::Info,
                "Sys",
                format!("{omitted} additional diagnostics omitted by bounds"),
            ));
        }
        entries
    }

    #[cfg_attr(not(feature = "chrome"), allow(dead_code))]
    fn build_console_visual_rows(&mut self, text_w: f32) -> Vec<ConsoleVisualRow> {
        let filter = self.devtools.console_filter;
        let search = self.devtools.console_search.clone();
        let entries_snapshot = self.devtools.console.clone();
        let rendering_snapshot = self.devtools.rendering.clone();
        let omitted_count = self
            .devtools
            .omitted
            .saturating_add(self.devtools.rendering.omitted);

        let mut rows = Vec::new();
        let mut hidden_below_depth: Option<u8> = None;

        for (entry_idx, entry) in entries_snapshot.iter().enumerate() {
            if let Some(hide_depth) = hidden_below_depth {
                if entry.group_depth > hide_depth {
                    continue;
                }
                hidden_below_depth = None;
            }
            if let ConsoleEntryKind::Group { collapsed: true } = entry.kind {
                hidden_below_depth = Some(entry.group_depth);
            }
            let keep = match entry.kind {
                ConsoleEntryKind::Command
                | ConsoleEntryKind::Result
                | ConsoleEntryKind::NavMarker => {
                    filter == ConsoleFilter::All
                        || (filter == ConsoleFilter::Errors && entry.level == ConsoleLevel::Error)
                }
                _ => match filter {
                    ConsoleFilter::All => true,
                    ConsoleFilter::Errors => entry.level == ConsoleLevel::Error,
                    ConsoleFilter::Warnings => entry.level == ConsoleLevel::Warning,
                    ConsoleFilter::Info => entry.level == ConsoleLevel::Info,
                    ConsoleFilter::Verbose => entry.level == ConsoleLevel::Verbose,
                },
            };
            if !keep
                || !matches_console_search(
                    &search,
                    entry.source,
                    &entry.message,
                    &entry.extra_lines,
                )
            {
                continue;
            }

            let arrow = match entry.kind {
                ConsoleEntryKind::Group { collapsed } => {
                    if collapsed {
                        "▶ "
                    } else {
                        "▼ "
                    }
                }
                _ if entry.object_id.is_some() => {
                    if entry.expanded {
                        "▼ "
                    } else {
                        "▶ "
                    }
                }
                _ => "",
            };
            let count_prefix = if entry.count > 1 {
                format!("(×{}) ", entry.count)
            } else {
                String::new()
            };
            let full_entry_text = Self::format_console_entry_for_copy(entry);
            let head = format!("{arrow}{count_prefix}{}", entry.message);
            let wrapped = self.wrap_lines(&[head], text_w);
            for (wi, line) in wrapped.into_iter().enumerate() {
                if rows.len() >= 768 {
                    return rows;
                }
                rows.push(ConsoleVisualRow {
                    level: entry.level,
                    kind: entry.kind,
                    source: entry.source,
                    text: line,
                    entry_idx: Some(entry_idx),
                    full_text: full_entry_text.clone(),
                    first_of_entry: wi == 0,
                    group_depth: entry.group_depth,
                    toggle_group: (wi == 0 && matches!(entry.kind, ConsoleEntryKind::Group { .. }))
                        .then_some(entry_idx),
                    toggle_object: (wi == 0)
                        .then_some(entry.object_id)
                        .flatten()
                        .map(|oid| (entry_idx, oid)),
                    inspect_node: (wi == 0).then_some(entry.node_id).flatten(),
                });
            }
            for extra in &entry.extra_lines {
                if rows.len() >= 768 {
                    return rows;
                }
                rows.push(ConsoleVisualRow {
                    level: entry.level,
                    kind: entry.kind,
                    source: entry.source,
                    text: extra.clone(),
                    entry_idx: Some(entry_idx),
                    full_text: full_entry_text.clone(),
                    first_of_entry: false,
                    group_depth: entry.group_depth,
                    toggle_group: None,
                    toggle_object: None,
                    inspect_node: None,
                });
            }
            if entry.expanded
                && let Some(props) = &entry.properties
            {
                for prop in props.iter().take(48) {
                    if rows.len() >= 768 {
                        return rows;
                    }
                    let marker = if prop.enumerable { "" } else { " [non-enum]" };
                    rows.push(ConsoleVisualRow {
                        level: ConsoleLevel::Info,
                        kind: ConsoleEntryKind::Result,
                        source: "Prop",
                        text: format!("  {}: {}{marker}", prop.name, prop.value),
                        entry_idx: Some(entry_idx),
                        full_text: full_entry_text.clone(),
                        first_of_entry: false,
                        group_depth: entry.group_depth.saturating_add(1),
                        toggle_group: None,
                        toggle_object: None,
                        inspect_node: prop.node_id.filter(|_| !self.console_detached_dom),
                    });
                }
            }
        }

        if matches!(filter, ConsoleFilter::All | ConsoleFilter::Warnings) {
            for (css_idx, entry) in rendering_snapshot.entries.iter().enumerate() {
                let location = match (entry.line, entry.column) {
                    (Some(line), Some(column)) => format!(":{line}:{column}"),
                    _ => String::new(),
                };
                let node = entry
                    .node
                    .map(|id| format!(" node {id}"))
                    .unwrap_or_default();
                let formatted = bounded(
                    &format!(
                        "{}{} {}{}: {}",
                        entry.kind, node, entry.source, location, entry.message
                    ),
                    1024,
                );
                if !matches_console_search(&search, "CSS", &formatted, &[]) {
                    continue;
                }
                let full_css = format!("[WRN CSS] {formatted}");
                for (wi, line) in self
                    .wrap_lines(&[formatted], text_w)
                    .into_iter()
                    .enumerate()
                {
                    if rows.len() >= 768 {
                        return rows;
                    }
                    rows.push(ConsoleVisualRow {
                        level: ConsoleLevel::Warning,
                        kind: ConsoleEntryKind::Diagnostic,
                        source: "CSS",
                        text: line,
                        entry_idx: Some(1_000_000 + css_idx),
                        full_text: full_css.clone(),
                        first_of_entry: wi == 0,
                        group_depth: 0,
                        toggle_group: None,
                        toggle_object: None,
                        inspect_node: (wi == 0).then_some(entry.node).flatten(),
                    });
                }
            }
        }
        if omitted_count > 0 && rows.len() < 768 {
            let sys_txt = format!("{omitted_count} additional diagnostics omitted by bounds");
            rows.push(ConsoleVisualRow {
                level: ConsoleLevel::Info,
                kind: ConsoleEntryKind::Diagnostic,
                source: "Sys",
                text: sys_txt.clone(),
                entry_idx: Some(2_000_000),
                full_text: sys_txt,
                first_of_entry: true,
                group_depth: 0,
                toggle_group: None,
                toggle_object: None,
                inspect_node: None,
            });
        }
        rows
    }

    fn format_console_entry_for_copy(entry: &ConsoleEntry) -> String {
        let mut out = String::new();
        let indent = "  ".repeat(entry.group_depth.min(6) as usize);
        let count_prefix = if entry.count > 1 {
            format!("(×{}) ", entry.count)
        } else {
            String::new()
        };
        match entry.kind {
            ConsoleEntryKind::Command => {
                out.push_str(&format!("{indent}{}", entry.message));
            }
            ConsoleEntryKind::Result if entry.level != ConsoleLevel::Error => {
                out.push_str(&format!("{indent}{}", entry.message));
            }
            ConsoleEntryKind::NavMarker => {
                out.push_str(&entry.message);
            }
            _ => {
                let lvl = match entry.level {
                    ConsoleLevel::Error => "ERR",
                    ConsoleLevel::Warning => "WRN",
                    ConsoleLevel::Verbose => "DBG",
                    ConsoleLevel::Info => "LOG",
                };
                out.push_str(&format!(
                    "{indent}[{lvl} {}] {count_prefix}{}",
                    entry.source, entry.message
                ));
            }
        }
        for extra in &entry.extra_lines {
            out.push('\n');
            out.push_str(&indent);
            out.push_str("  ");
            out.push_str(extra);
        }
        if let Some(props) = &entry.properties {
            for prop in props.iter().take(48) {
                let marker = if prop.enumerable { "" } else { " [non-enum]" };
                out.push('\n');
                out.push_str(&indent);
                out.push_str(&format!("  {}: {}{marker}", prop.name, prop.value));
            }
        }
        out
    }

    pub(crate) fn console_visual_row_count(&mut self) -> usize {
        let (_, _, pw, _) = self.devtools_panel_rect();
        let text_w = pw.saturating_sub(96).max(80) as f32;
        self.build_console_visual_rows(text_w).len()
    }

    pub(crate) fn active_source_line_count(&self) -> usize {
        let sources = self.collect_sources();
        let idx = self
            .devtools
            .selected_source
            .min(sources.len().saturating_sub(1));
        sources
            .get(idx)
            .map_or(0, |(_, _, body)| body.lines().take(4096).count())
    }

    pub(crate) fn console_row_at_point(&mut self, x: i32, y: i32) -> Option<usize> {
        let (lx, ly, lw, lh) = self.devtools.console_log_rect?;
        if x < lx || x >= lx + lw as i32 || y < ly || y >= ly + lh as i32 {
            return None;
        }
        let total = self.console_visual_row_count();
        if total == 0 {
            return None;
        }
        let rel = ((y - ly).max(0) / 20) as usize;
        Some((self.devtools.row + rel).min(total - 1))
    }

    pub(crate) fn sources_line_at_point(&self, x: i32, y: i32) -> Option<usize> {
        let (sx, sy, sw, sh) = self.devtools.sources_code_rect?;
        if x < sx || x >= sx + sw as i32 || y < sy || y >= sy + sh as i32 {
            return None;
        }
        let total = self.active_source_line_count();
        if total == 0 {
            return None;
        }
        let rel = ((y - sy).max(0) / 18) as usize;
        Some((self.devtools.row + rel).min(total - 1))
    }

    pub(crate) fn try_focus_devtools_editable_at(&mut self, x: i32, y: i32) -> bool {
        for &(kind, (fx, fy, fw, fh)) in &self.devtools.field_rects.clone() {
            if x >= fx && x < fx + fw as i32 && y >= fy && y < fy + fh as i32 {
                self.focus_devtools_field(kind);
                let byte = self.hit_devtools_field_byte(kind, x);
                let (_, cur, _) = self.field_state_mut(kind);
                *cur = byte;
                self.devtools.field_anchor = None;
                self.dirty = true;
                return true;
            }
        }
        if self.devtools.tab == Tab::Console
            && let Some((rx, ry, rw, rh)) = self.devtools.console_repl_rect
            && x >= rx
            && x < rx + rw as i32
            && y >= ry
            && y < ry + rh as i32
        {
            self.focus = Focus::ConsoleInput;
            let byte = self.hit_console_repl_byte(x, y);
            self.devtools.console_cursor = byte;
            self.devtools.console_anchor = None;
            self.dirty = true;
            return true;
        }
        false
    }

    pub(crate) fn hit_devtools_field_byte(&mut self, kind: FieldKind, x: i32) -> usize {
        let rect = self
            .devtools
            .field_rects
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, r)| *r);
        let (val, cur) = {
            let (buf, cur, _) = self.field_state_mut(kind);
            (buf.clone(), *cur)
        };
        let Some((fx, _, fw, _)) = rect else {
            return val.len();
        };
        crate::clipboard::hit_byte_in_line(
            &mut self.fonts,
            &val,
            11.5,
            fw.saturating_sub(12) as f32,
            cur,
            (x - (fx + 5)) as f32,
        )
    }

    pub(crate) fn hit_console_repl_byte(&mut self, x: i32, y: i32) -> usize {
        let Some((rx, ry, rw, _)) = self.devtools.console_repl_rect else {
            return self.devtools.console_input.len();
        };
        let input = self.devtools.console_input.clone();
        if input.is_empty() {
            return 0;
        }
        let target_line = ((y - (ry + 5)).max(0) / 16) as usize;
        let mut line_start = 0usize;
        for (li, raw_line) in input.split('\n').take(3).enumerate() {
            if li == target_line || !input[line_start + raw_line.len()..].starts_with('\n') {
                let cur_in_line = self
                    .devtools
                    .console_cursor
                    .saturating_sub(line_start)
                    .min(raw_line.len());
                let rel = crate::clipboard::hit_byte_in_line(
                    &mut self.fonts,
                    raw_line,
                    12.5,
                    rw.saturating_sub(32) as f32,
                    cur_in_line,
                    (x - (rx + 20)) as f32,
                );
                return (line_start + rel).min(input.len());
            }
            line_start += raw_line.len() + 1;
        }
        input.len()
    }

    pub(crate) fn copy_console_rows(&mut self, range: Option<(usize, usize)>) -> Option<String> {
        let (_, _, pw, _) = self.devtools_panel_rect();
        let text_w = pw.saturating_sub(96).max(80) as f32;
        let visual = self.build_console_visual_rows(text_w);
        if visual.is_empty() {
            return None;
        }
        let (r0, r1) = match range {
            Some((a, b)) => (
                a.min(b).min(visual.len() - 1),
                a.max(b).min(visual.len() - 1),
            ),
            None => (0, visual.len() - 1),
        };
        let mut seen_entries = HashSet::new();
        let mut blocks = Vec::new();
        for vrow in &visual[r0..=r1] {
            if let Some(eid) = vrow.entry_idx {
                if seen_entries.insert(eid) && !vrow.full_text.is_empty() {
                    blocks.push(vrow.full_text.clone());
                }
            } else if !vrow.full_text.is_empty() {
                blocks.push(vrow.full_text.clone());
            }
        }
        if blocks.is_empty() {
            None
        } else {
            let count = blocks.len();
            let joined = crate::clipboard::bounded_clipboard(&blocks.join("\n"));
            self.devtools.copy_feedback = Some(format!(
                "Copied {count} console {} ({} B)",
                if count == 1 { "entry" } else { "entries" },
                joined.len()
            ));
            Some(joined)
        }
    }

    pub(crate) fn serialize_outer_html(&self, root_id: usize) -> String {
        fn rec(doc: &Document, id: usize, depth: usize, out: &mut String, budget: &mut usize) {
            if *budget == 0 || depth > 64 {
                return;
            }
            *budget -= 1;
            let Some(node) = doc.nodes.get(id) else {
                return;
            };
            if node.tag == "#document" {
                for &child in &node.children {
                    rec(doc, child, depth, out, budget);
                }
                return;
            }
            if node.tag == "#text" {
                out.push_str(&node.text);
                return;
            }
            out.push('<');
            out.push_str(&node.tag);
            for (k, v) in &node.attributes {
                out.push(' ');
                out.push_str(k);
                out.push_str("=\"");
                out.push_str(&v.replace('"', "&quot;"));
                out.push('"');
            }
            out.push('>');
            let is_void = matches!(
                node.tag.as_str(),
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
                    | "source"
                    | "track"
                    | "wbr"
            );
            if !is_void {
                for &child in &node.children {
                    if out.len() >= crate::clipboard::MAX_CLIPBOARD_BYTES {
                        break;
                    }
                    rec(doc, child, depth + 1, out, budget);
                }
                out.push_str("</");
                out.push_str(&node.tag);
                out.push('>');
            }
        }
        let mut out = String::new();
        let mut budget = 8192usize;
        rec(&self.document, root_id, 0, &mut out, &mut budget);
        crate::clipboard::bounded_clipboard(&out)
    }

    pub(crate) fn css_selector_for_node(&self, mut cur: usize) -> String {
        let mut parts = Vec::new();
        for _ in 0..16 {
            let Some(node) = self.document.nodes.get(cur) else {
                break;
            };
            if node.tag == "#document" || cur == 0 {
                break;
            }
            if let Some(id_attr) = node.attr("id").filter(|s| !s.trim().is_empty()) {
                parts.push(format!("#{}", id_attr.trim()));
                break;
            }
            let mut seg = if node.tag == "#text" {
                "text()".into()
            } else {
                node.tag.clone()
            };
            if let Some(cls) = node.attr("class").filter(|s| !s.trim().is_empty()) {
                for token in cls.split_whitespace().take(2) {
                    seg.push('.');
                    seg.push_str(token);
                }
            }
            parts.push(seg);
            if node.parent == cur {
                break;
            }
            cur = node.parent;
        }
        parts.reverse();
        if parts.is_empty() {
            "html".into()
        } else {
            parts.join(" > ")
        }
    }

    pub(crate) fn styles_summary_for_node(&self, id: usize) -> String {
        let mut lines = Vec::new();
        if let Some(node) = self.document.nodes.get(id)
            && let Some(inline) = node.attr("style").filter(|s| !s.trim().is_empty())
        {
            lines.push(format!("element.style {{ {} }}", inline.trim()));
        }
        for (sel, decls, origin) in self.matched_css_rules_for(id) {
            lines.push(format!("{sel} {{ {decls} }} /* {origin} */"));
        }
        for (name, val, _) in self.computed_style_pairs(id) {
            lines.push(format!("{name}: {val};"));
        }
        crate::clipboard::bounded_clipboard(&lines.join("\n"))
    }

    pub(crate) fn network_copy_url(&self) -> Option<String> {
        let idx = self
            .devtools
            .selected_request
            .or_else(|| self.filtered_network_indices().into_iter().next())?;
        let entry = self.devtools.network.get(idx)?;
        Some(entry.url.clone())
    }

    pub(crate) fn network_copy_details(&self) -> Option<String> {
        let idx = self
            .devtools
            .selected_request
            .or_else(|| self.filtered_network_indices().into_iter().next())?;
        let entry = self.devtools.network.get(idx)?;
        let mut lines = vec![format!(
            "{} {} -> {}",
            entry.method,
            entry.url,
            entry.status.map_or_else(
                || entry.error.clone().unwrap_or_else(|| "pending".into()),
                |s| s.to_string()
            )
        )];
        lines.push(format!(
            "MIME: {} · Bytes: {} · Start: +{:.1} ms · Duration: {:.1} ms",
            entry.mime, entry.bytes, entry.start_ms, entry.duration_ms
        ));
        if !entry.request_headers.is_empty() {
            lines.push("Request Headers:".into());
            for (k, v) in &entry.request_headers {
                lines.push(format!("  {k}: {v}"));
            }
        }
        if !entry.response_headers.is_empty() {
            lines.push("Response Headers:".into());
            for (k, v) in &entry.response_headers {
                lines.push(format!("  {k}: {v}"));
            }
        }
        if let Some(body) = &entry.body_preview {
            lines.push("Response Preview:".into());
            lines.push(body.clone());
        }
        Some(crate::clipboard::bounded_clipboard(&lines.join("\n")))
    }

    pub(crate) fn network_copy_all_urls(&self) -> Option<String> {
        let indices = self.filtered_network_indices();
        if indices.is_empty() {
            return None;
        }
        let urls: Vec<&str> = indices
            .into_iter()
            .filter_map(|i| self.devtools.network.get(i).map(|e| e.url.as_str()))
            .collect();
        (!urls.is_empty()).then(|| crate::clipboard::bounded_clipboard(&urls.join("\n")))
    }

    pub(crate) fn devtools_copy_current_tab(&mut self) -> Option<String> {
        match self.devtools.tab {
            Tab::Console => {
                let sel = self.devtools.console_sel;
                self.copy_console_rows(sel)
            }
            Tab::Elements => {
                let id = self
                    .devtools
                    .selected
                    .filter(|(epoch, _)| *epoch == self.dom_epoch)
                    .map_or(0, |(_, id)| id);
                let html = self.serialize_outer_html(id);
                (!html.is_empty()).then_some(html)
            }
            Tab::Sources => {
                let sources = self.collect_sources();
                let idx = self
                    .devtools
                    .selected_source
                    .min(sources.len().saturating_sub(1));
                let (_, _, content) = sources.get(idx)?;
                if let Some((a, b)) = self.devtools.sources_sel {
                    let all: Vec<&str> = content.lines().take(4096).collect();
                    if all.is_empty() {
                        return None;
                    }
                    let l0 = a.min(b).min(all.len() - 1);
                    let l1 = a.max(b).min(all.len() - 1);
                    Some(crate::clipboard::bounded_clipboard(
                        &all[l0..=l1].join("\n"),
                    ))
                } else {
                    (!content.is_empty()).then(|| crate::clipboard::bounded_clipboard(content))
                }
            }
            Tab::Network => self
                .network_copy_url()
                .or_else(|| self.network_copy_all_urls()),
            Tab::Performance => Some(self.performance_report()),
            Tab::Application => {
                let out = self.inspector_lines().join("\n");
                (!out.is_empty()).then_some(out)
            }
        }
    }

    fn inspector_lines(&self) -> Vec<String> {
        match self.devtools.tab {
            Tab::Performance => self.performance_lines(),
            Tab::Console => {
                let (errors, warnings) = self.devtools.console_counts();
                let mut lines = vec![format!(
                    "Console · JS: {} · {errors} errors · {warnings} warnings",
                    if self.page_session.is_some() && !self.script_blocked {
                        "Boa worker (live)"
                    } else if self.scripts_enabled {
                        "Boa worker"
                    } else {
                        "on-demand REPL"
                    }
                )];
                let filtered = self.console_filtered_entries();
                if filtered.is_empty() {
                    lines.push("No console entries match the current filter.".into());
                } else {
                    for (level, source, text) in filtered {
                        let badge = match level {
                            ConsoleLevel::Error => "[ERR]",
                            ConsoleLevel::Warning => "[WARN]",
                            ConsoleLevel::Info => "[INFO]",
                            ConsoleLevel::Verbose => "[VERB]",
                        };
                        lines.push(format!("{badge} [{source}] {text}"));
                    }
                }
                lines.truncate(MAX_LINES);
                lines
            }
            Tab::Sources => {
                let sources = self.collect_sources();
                let mut lines = vec![format!(
                    "Sources · {} files loaded for {}",
                    sources.len(),
                    bounded(&self.page_url, 120)
                )];
                if let Some((kind, name, content)) = sources
                    .get(self.devtools.selected_source)
                    .or_else(|| sources.first())
                {
                    lines.push(format!("Active [{kind}] {name} ({} B)", content.len()));
                    for (i, line) in content.lines().take(MAX_LINES - 4).enumerate() {
                        lines.push(format!("{:>4} │ {}", i + 1, bounded(line, 240)));
                    }
                }
                lines.truncate(MAX_LINES);
                lines
            }
            Tab::Network => {
                let indices = self.filtered_network_indices();
                let total_bytes: usize = self.devtools.network.iter().map(|e| e.bytes).sum();
                let mut lines = vec![format!(
                    "Network · {} requests · {} transferred",
                    self.devtools.network.len(),
                    format_bytes(total_bytes)
                )];
                if indices.is_empty() {
                    lines.push("No network requests recorded for this filter.".into());
                } else {
                    for idx in indices {
                        let e = &self.devtools.network[idx];
                        let status = if e.pending {
                            "PENDING".into()
                        } else if let Some(code) = e.status {
                            code.to_string()
                        } else {
                            "ERR".into()
                        };
                        lines.push(format!(
                            "{status} {:<4} {:<4} {:>8} {:>6.1}ms {}",
                            e.method,
                            e.kind.label(),
                            format_bytes(e.bytes),
                            e.duration_ms,
                            short_resource_name(&e.url),
                        ));
                    }
                }
                lines.truncate(MAX_LINES);
                lines
            }
            Tab::Application => self.application_lines(),
            Tab::Elements => self.elements_detail_lines(),
        }
    }

    fn application_lines(&self) -> Vec<String> {
        let cookies = self.session.cookies_snapshot();
        let mut lines = vec![
            format!("Application Inspector : {}", bounded(&self.page_url, 200)),
            format!(
                "Origin Security: {} · MIME {} · Cache {}",
                if self.is_http_page() {
                    "HTTP (Not secure)"
                } else if self.page_url.starts_with("https://") {
                    "HTTPS (rustls-rustcrypto verified)"
                } else {
                    "Local / Internal document"
                },
                self.page_mime,
                if self.session.cache_disabled() {
                    "disabled"
                } else {
                    "enabled"
                }
            ),
            format!(
                "Worker Sandbox: {} · {} DOM nodes · {} stylesheets · {} resources",
                if self.page_session.is_some() && !self.script_blocked {
                    "boa-page-process-v1 (active)"
                } else if self.scripts_enabled {
                    "boa-page-process-v1"
                } else {
                    "static / on-demand Console"
                },
                self.document.nodes.len(),
                self.document.stylesheets.len(),
                self.document.resources.len()
            ),
            format!("Session Cookies ({} stored, values masked):", cookies.len()),
        ];
        if cookies.is_empty() {
            lines.push("  (no cookies stored in session jar)".into());
        } else {
            for c in cookies.iter().take(48) {
                let exp = match c.ttl_secs {
                    Some(s) => format!("ttl={s}s"),
                    None if c.session_only => "Session".into(),
                    None => "Expired".into(),
                };
                lines.push(format!(
                    "  {} = <{} B> · domain={} path={} {}{}",
                    bounded(&c.name, 48),
                    c.value_bytes,
                    bounded(&c.domain, 48),
                    bounded(&c.path, 32),
                    if c.secure { "Secure " } else { "" },
                    exp
                ));
            }
        }
        lines.truncate(MAX_LINES);
        lines
    }

    fn elements_detail_lines(&self) -> Vec<String> {
        let Some((epoch, id)) = self
            .devtools
            .selected
            .filter(|(epoch, _)| *epoch == self.dom_epoch)
        else {
            return vec![
                "Selection unavailable or document changed.".into(),
                "Right-click an element or pick a DOM node to inspect.".into(),
            ];
        };
        let Some(node) = self.document.nodes.get(id) else {
            return vec!["Selected node is unavailable.".into()];
        };
        let mut lines = vec![
            format!("Node {id} <{}> : document {epoch}", bounded(&node.tag, 80)),
            format!("Page: {}", bounded(&self.page_url, 512)),
        ];
        let mut ancestors = Vec::new();
        let mut current = id;
        for _ in 0..16 {
            let Some(value) = self.document.nodes.get(current) else {
                break;
            };
            ancestors.push(format!("{}#{}", bounded(&value.tag, 60), current));
            if current == 0 || value.parent == current {
                break;
            }
            current = value.parent;
        }
        ancestors.reverse();
        lines.push(format!("Ancestors: {}", ancestors.join(" > ")));
        if let Some((x, y, right, bottom)) = self.node_painted_bounds(epoch, id) {
            lines.push(format!(
                "Painted union: x={x} y={y} width={} height={} CSS px",
                right - x,
                bottom - y
            ));
        } else if self.devtools.painted_epoch != epoch || self.boxes.len() >= 100_000 {
            lines.push("Painted bounds unavailable (stale geometry or query limit).".into());
        } else {
            lines.push("No painted box for this node.".into());
        }
        if let Some(style) = node.attr("style").filter(|s| !s.trim().is_empty()) {
            lines.push(format!(
                "element.style {{ {} }}",
                bounded(style.trim(), 256)
            ));
        } else {
            lines.push("element.style { /* no inline style declarations */ }".into());
        }
        lines.push("Attributes:".into());
        for (key, value) in node.attributes.iter().take(24) {
            lines.push(format!(
                "  {}=\"{}\"",
                bounded(key, 80),
                bounded(value, 512)
            ));
        }
        if node.attributes.is_empty() {
            lines.push("  (none)".into());
        }
        let mut text = String::new();
        let mut pending = vec![id];
        let mut steps = 0;
        while let Some(current) = pending.pop() {
            steps += 1;
            if steps > 4096 || text.len() >= 320 {
                break;
            }
            let Some(value) = self.document.nodes.get(current) else {
                continue;
            };
            if value.tag == "#text" {
                text.push_str(&bounded(&value.text, 320usize.saturating_sub(text.len())));
            }
            let available = 4096usize.saturating_sub(pending.len());
            pending.extend(value.children.iter().take(available).rev().copied());
        }
        lines.push(format!(
            "Text sample: {}",
            if text.trim().is_empty() {
                "(empty)".into()
            } else {
                bounded(text.trim(), 240)
            }
        ));
        lines.truncate(MAX_LINES);
        lines
    }

    #[cfg(feature = "chrome")]
    pub(super) fn paint_docked_devtools(&mut self, canvas: &mut Canvas) {
        if self.devtools.open && self.devtools_is_docked() {
            self.paint_inspected_highlight(canvas);
            self.paint_devtools_panel(canvas, true);
        }
    }

    #[cfg(feature = "chrome")]
    pub(super) fn paint_devtools_overlay(&mut self, canvas: &mut Canvas) {
        let palette = self.effective_theme().palette();
        if let Some((x, y)) = self.devtools.context {
            self.hits.clear();
            let items = self.context_menu_items();
            let h = items.len() as u32 * 38 + 4;
            let width = self.width.saturating_sub(12).min(226);
            let max_x = (self.width.saturating_sub(width + 6) as i32).max(6);
            let max_y = (self.height.saturating_sub(h + 6) as i32).max(6);
            let x = x.clamp(6, max_x);
            let y = y.clamp(6, max_y);
            canvas.rect(x, y, width, h, palette.border);
            for (idx, (label, action)) in items.into_iter().enumerate() {
                self.inspector_button(
                    canvas,
                    (x + 4, y + 4 + idx as i32 * 38, width - 8, 34),
                    label,
                    action,
                    Some(idx),
                    false,
                );
            }
            return;
        }
        if self.devtools.open && !self.devtools_is_docked() {
            self.hits.clear();
            self.paint_devtools_panel(canvas, false);
        }
    }

    #[cfg(feature = "chrome")]
    fn paint_devtools_panel(&mut self, canvas: &mut Canvas, docked: bool) {
        self.devtools.field_rects.clear();
        self.devtools.console_log_rect = None;
        self.devtools.console_repl_rect = None;
        self.devtools.sources_code_rect = None;
        let palette = self.effective_theme().palette();
        let dark = self.effective_theme() == ColorScheme::Dark;
        let (x, y, width, height) = self.devtools_panel_rect();
        let split_col = if self.devtools.split_dragging {
            palette.accent
        } else {
            palette.border
        };
        if docked {
            if self.devtools_side_width().is_some() {
                canvas.rect(x, y, 2, height, split_col);
                canvas.rect(x + 2, y, width.saturating_sub(2), height, palette.panel);
            } else {
                canvas.rect(x, y, width, 2, split_col);
                canvas.rect(x, y + 2, width, height.saturating_sub(2), palette.panel);
            }
        } else {
            canvas.rect(x, y, width, height, palette.border);
            canvas.rect(
                x + 2,
                y + 2,
                width.saturating_sub(4),
                height.saturating_sub(4),
                palette.panel,
            );
        }

        // Top Chromium-style tab strip: [↖ Picker] [6 Tabs] [◨/⬓ Dock] [× Close]
        canvas.rect(
            x + 2,
            y + 2,
            width.saturating_sub(4),
            32,
            palette.background,
        );
        canvas.rect(x + 2, y + 34, width.saturating_sub(4), 1, palette.border);

        self.inspector_button(
            canvas,
            (x + 4, y + 4, 22, 28),
            "↖",
            Action::DevToolsTogglePicker,
            Some(6),
            self.devtools.pick_mode,
        );
        if self.devtools.pick_mode {
            canvas.rect(x + 4, y + 30, 22, 3, palette.accent);
        }

        let tabs_avail = width.saturating_sub(82);
        let tab_w = (tabs_avail.saturating_sub(10) / 6).clamp(40, 108);
        let (err_count, warn_count) = self.devtools.console_counts();
        let elem_lbl = if tab_w >= 64 { "Elements" } else { "Elem" };
        let cons_lbl = if err_count + warn_count > 0 && tab_w >= 86 {
            format!("Console ({})", err_count + warn_count)
        } else if tab_w >= 64 {
            "Console".into()
        } else {
            "Cons".into()
        };
        let src_lbl = if tab_w >= 64 { "Sources" } else { "Src" };
        let net_lbl = if !self.devtools.network.is_empty() && tab_w >= 88 {
            format!("Network ({})", self.devtools.network.len())
        } else if tab_w >= 64 {
            "Network".into()
        } else {
            "Net".into()
        };
        let perf_lbl = if tab_w >= 84 { "Performance" } else { "Perf" };
        let app_lbl = if tab_w >= 84 { "Application" } else { "App" };

        let tab_specs = [
            (elem_lbl, Tab::Elements),
            (cons_lbl.as_str(), Tab::Console),
            (src_lbl, Tab::Sources),
            (net_lbl.as_str(), Tab::Network),
            (perf_lbl, Tab::Performance),
            (app_lbl, Tab::Application),
        ];
        for (index, (label, tab)) in tab_specs.into_iter().enumerate() {
            let tx = x + 28 + index as i32 * (tab_w as i32 + 2);
            let active = self.devtools.tab == tab;
            self.inspector_button(
                canvas,
                (tx, y + 4, tab_w, 28),
                label,
                Action::DevToolsTab(tab),
                Some(index),
                active,
            );
            if active {
                canvas.rect(tx + 2, y + 30, tab_w.saturating_sub(4), 3, palette.accent);
            }
        }

        let dock_x = x + width as i32 - 52;
        self.inspector_button(
            canvas,
            (dock_x, y + 4, 20, 28),
            "",
            Action::DevToolsToggleDock,
            Some(7),
            false,
        );
        canvas.rect(dock_x + 4, y + 13, 12, 10, palette.ink);
        canvas.rect(dock_x + 5, y + 14, 10, 8, palette.button);
        if self.devtools.dock == DockSide::Right {
            canvas.rect(dock_x + 5, y + 19, 10, 3, palette.accent);
        } else {
            canvas.rect(dock_x + 11, y + 14, 4, 8, palette.accent);
        }
        self.inspector_button(
            canvas,
            (x + width as i32 - 30, y + 4, 26, 28),
            "×",
            Action::CloseMenu,
            Some(8),
            false,
        );

        // Secondary context / filter bar (leaves x+2..x+8 untouched as palette.panel)
        let sub_y = y + 36;
        canvas.rect(x + 2, sub_y, width.saturating_sub(4), 26, palette.panel);
        canvas.rect(
            x + 2,
            sub_y + 26,
            width.saturating_sub(4),
            1,
            palette.border,
        );
        self.paint_subtoolbar(canvas, x + 8, sub_y + 2, width.saturating_sub(16), dark);

        // Main panel body
        let body_top = y + 64;
        let footer_y = y + height as i32 - 30;
        let body_h = (footer_y - 4 - body_top).max(20) as u32;
        if self.devtools_rich_layout() {
            match self.devtools.tab {
                Tab::Elements => {
                    self.paint_elements_rich(canvas, x + 6, body_top, width - 12, body_h, dark)
                }
                Tab::Console => {
                    self.paint_console_rich(canvas, x + 6, body_top, width - 12, body_h, dark)
                }
                Tab::Sources => {
                    self.paint_sources_rich(canvas, x + 6, body_top, width - 12, body_h, dark)
                }
                Tab::Network => {
                    self.paint_network_rich(canvas, x + 6, body_top, width - 12, body_h, dark)
                }
                Tab::Performance => {
                    self.paint_performance_rich(canvas, x + 6, body_top, width - 12, body_h, dark)
                }
                Tab::Application => {
                    self.paint_application_rich(canvas, x + 6, body_top, width - 12, body_h, dark)
                }
            }
        } else {
            self.paint_compact_lines(canvas, x + 10, body_top, width.saturating_sub(20), body_h);
        }

        // Bottom footer bar
        canvas.rect(
            x + 2,
            footer_y - 3,
            width.saturating_sub(4),
            1,
            palette.border,
        );
        canvas.rect(
            x + 2,
            footer_y - 2,
            width.saturating_sub(4),
            30,
            palette.background,
        );
        self.inspector_button(
            canvas,
            (x + 6, footer_y, 50, 24),
            "Prev",
            Action::InspectorScroll(false),
            Some(9),
            false,
        );
        self.inspector_button(
            canvas,
            (x + 60, footer_y, 50, 24),
            "Next",
            Action::InspectorScroll(true),
            Some(10),
            false,
        );
        let summary = self.footer_summary();
        self.inspector_text(
            canvas,
            x + 118,
            footer_y + 5,
            &summary,
            width.saturating_sub(126),
            12.,
            palette.muted,
        );
    }

    /// 4-layer Chromium Stylo box-model overlay:
    /// Margin (orange `0xf6b26b`), Border (yellow `0xffe599`), Padding (green `0x93c47d`),
    /// Content (blue `0x6fa8dc`), plus floating `tag#id.class  W×H` tooltip.
    #[cfg(feature = "chrome")]
    fn paint_inspected_highlight(&mut self, canvas: &mut Canvas) {
        let target = if self.devtools.pick_mode {
            self.devtools
                .hovered_inspect_node
                .filter(|(epoch, _)| *epoch == self.dom_epoch)
                .or_else(|| {
                    self.devtools
                        .selected
                        .filter(|(epoch, _)| *epoch == self.dom_epoch)
                })
        } else if self.devtools.tab == Tab::Elements {
            self.devtools
                .selected
                .filter(|(epoch, _)| *epoch == self.dom_epoch)
        } else {
            None
        };
        let Some((epoch, id)) = target else {
            return;
        };
        if id == 0 {
            return;
        }
        let Some((bx, by, right, bottom)) = self.node_painted_bounds(epoch, id) else {
            return;
        };
        let page_w = self.page_viewport_width();
        let page_top = self.page_top();
        let page_h = self.viewport_height();
        let prev_clip = canvas.intersect_clip(0, page_top, page_w, page_h);

        let border_x = bx as i32;
        let border_y = by as i32 + page_top;
        let border_w = (right - bx).max(0) as i32;
        let border_h = (bottom - by).max(0) as i32;

        let (margin, border, padding) = if let Some(cs) = self.style_cache.style_for(id) {
            let m = [
                cs.margin[0]
                    .resolve(page_w as f32)
                    .unwrap_or(0.0)
                    .max(0.0)
                    .round() as i32,
                cs.margin[1]
                    .resolve(page_w as f32)
                    .unwrap_or(0.0)
                    .max(0.0)
                    .round() as i32,
                cs.margin[2]
                    .resolve(page_w as f32)
                    .unwrap_or(0.0)
                    .max(0.0)
                    .round() as i32,
                cs.margin[3]
                    .resolve(page_w as f32)
                    .unwrap_or(0.0)
                    .max(0.0)
                    .round() as i32,
            ];
            let b = [
                cs.border_width[0].max(0.0).round() as i32,
                cs.border_width[1].max(0.0).round() as i32,
                cs.border_width[2].max(0.0).round() as i32,
                cs.border_width[3].max(0.0).round() as i32,
            ];
            let p = [
                cs.padding[0]
                    .resolve(border_w as f32)
                    .unwrap_or(0.0)
                    .max(0.0)
                    .round() as i32,
                cs.padding[1]
                    .resolve(border_w as f32)
                    .unwrap_or(0.0)
                    .max(0.0)
                    .round() as i32,
                cs.padding[2]
                    .resolve(border_w as f32)
                    .unwrap_or(0.0)
                    .max(0.0)
                    .round() as i32,
                cs.padding[3]
                    .resolve(border_w as f32)
                    .unwrap_or(0.0)
                    .max(0.0)
                    .round() as i32,
            ];
            (m, b, p)
        } else {
            ([0; 4], [0; 4], [0; 4])
        };

        let clamp_blend =
            |canvas: &mut Canvas, rx: i32, ry: i32, rw: i32, rh: i32, rgb: u32, alpha: u32| {
                if rw <= 0 || rh <= 0 {
                    return;
                }
                let x0 = rx.max(0);
                let y0 = ry.max(page_top);
                let x1 = (rx + rw).min(page_w as i32);
                let y1 = (ry + rh).min(page_top + page_h as i32);
                if x1 > x0 && y1 > y0 {
                    blend_rect(
                        canvas,
                        x0,
                        y0,
                        (x1 - x0) as u32,
                        (y1 - y0) as u32,
                        rgb,
                        alpha,
                    );
                }
            };

        // 1. Margin strips (Chromium orange 0xf6b26b)
        let mx = border_x - margin[3];
        let mw = border_w + margin[3] + margin[1];
        clamp_blend(
            canvas,
            mx,
            border_y - margin[0],
            mw,
            margin[0],
            0xf6b26b,
            86,
        );
        clamp_blend(canvas, mx, border_y + border_h, mw, margin[2], 0xf6b26b, 86);
        clamp_blend(canvas, mx, border_y, margin[3], border_h, 0xf6b26b, 86);
        clamp_blend(
            canvas,
            border_x + border_w,
            border_y,
            margin[1],
            border_h,
            0xf6b26b,
            86,
        );

        // 2. Border strips (Chromium yellow 0xffe599)
        let pad_x = border_x + border[3];
        let pad_y = border_y + border[0];
        let pad_w = (border_w - border[3] - border[1]).max(0);
        let pad_h = (border_h - border[0] - border[2]).max(0);
        clamp_blend(
            canvas, border_x, border_y, border_w, border[0], 0xffe599, 108,
        );
        clamp_blend(
            canvas,
            border_x,
            pad_y + pad_h,
            border_w,
            border[2],
            0xffe599,
            108,
        );
        clamp_blend(canvas, border_x, pad_y, border[3], pad_h, 0xffe599, 108);
        clamp_blend(
            canvas,
            pad_x + pad_w,
            pad_y,
            border[1],
            pad_h,
            0xffe599,
            108,
        );

        // 3. Padding strips (Chromium green 0x93c47d)
        let cnt_x = pad_x + padding[3];
        let cnt_y = pad_y + padding[0];
        let cnt_w = (pad_w - padding[3] - padding[1]).max(0);
        let cnt_h = (pad_h - padding[0] - padding[2]).max(0);
        clamp_blend(canvas, pad_x, pad_y, pad_w, padding[0], 0x93c47d, 96);
        clamp_blend(
            canvas,
            pad_x,
            cnt_y + cnt_h,
            pad_w,
            padding[2],
            0x93c47d,
            96,
        );
        clamp_blend(canvas, pad_x, cnt_y, padding[3], cnt_h, 0x93c47d, 96);
        clamp_blend(
            canvas,
            cnt_x + cnt_w,
            cnt_y,
            padding[1],
            cnt_h,
            0x93c47d,
            96,
        );

        // 4. Content box (Chromium blue 0x6fa8dc) + crisp border-box outline
        clamp_blend(canvas, cnt_x, cnt_y, cnt_w, cnt_h, 0x6fa8dc, 88);
        let rx = border_x.max(0);
        let ry = border_y.max(page_top);
        let rr = (border_x + border_w).min(page_w as i32);
        let rb = (border_y + border_h).min(page_top + page_h as i32);
        if rr > rx && rb > ry {
            let rw = (rr - rx) as u32;
            let rh = (rb - ry) as u32;
            let outline = if self.devtools.pick_mode {
                0x0284c7
            } else {
                0x2563eb
            };
            canvas.rect(rx, ry, rw, 1, outline);
            canvas.rect(rx, rb - 1, rw, 1, outline);
            canvas.rect(rx, ry, 1, rh, outline);
            canvas.rect(rr - 1, ry, 1, rh, outline);

            if let Some(node) = self.document.nodes.get(id) {
                let mut badge = bounded(&node.tag, 20);
                if let Some(el_id) = node.attr("id").filter(|s| !s.is_empty()) {
                    badge.push('#');
                    badge.push_str(&bounded(el_id, 18));
                }
                if let Some(cls) = node.attr("class").filter(|s| !s.trim().is_empty())
                    && let Some(first_cls) = cls.split_whitespace().next()
                {
                    badge.push('.');
                    badge.push_str(&bounded(first_cls, 16));
                }
                badge.push_str(&format!("  {}×{}", right - bx, bottom - by));
                let bw = (self.fonts.width(&badge, 11.).ceil() as u32 + 14)
                    .min(page_w.saturating_sub(4));
                let by_tag = if ry - 21 >= page_top {
                    ry - 20
                } else {
                    (rb + 2).min(page_top + page_h as i32 - 20)
                };
                let bx_tag = rx.clamp(2, (page_w.saturating_sub(bw + 2)) as i32);
                canvas.rect(bx_tag, by_tag, bw, 19, 0x0f172a);
                canvas.rect(bx_tag, by_tag, bw, 1, 0x38bdf8);
                self.inspector_text(
                    canvas,
                    bx_tag + 6,
                    by_tag + 3,
                    &badge,
                    bw.saturating_sub(10),
                    11.,
                    0xf8fafc,
                );
            }
        }
        canvas.restore_clip(prev_clip);
    }

    #[cfg(feature = "chrome")]
    fn paint_subtoolbar(&mut self, canvas: &mut Canvas, x: i32, y: i32, width: u32, dark: bool) {
        let palette = self.effective_theme().palette();
        match self.devtools.tab {
            Tab::Elements => {
                let search_w = (width * 42 / 100).clamp(112, 196);
                let search_x = x + width as i32 - search_w as i32;
                let trail_w = (search_x - x - 6).max(40) as u32;
                let trail = if let Some((_, mut cur)) = self
                    .devtools
                    .selected
                    .filter(|(epoch, _)| *epoch == self.dom_epoch)
                {
                    let mut parts = Vec::new();
                    for _ in 0..8 {
                        let Some(node) = self.document.nodes.get(cur) else {
                            break;
                        };
                        let mut part = bounded(&node.tag, 18);
                        if let Some(id_attr) = node.attr("id").filter(|s| !s.is_empty()) {
                            part.push('#');
                            part.push_str(&bounded(id_attr, 14));
                        } else if let Some(cls) = node.attr("class").filter(|s| !s.is_empty())
                            && let Some(first) = cls.split_whitespace().next()
                        {
                            part.push('.');
                            part.push_str(&bounded(first, 12));
                        }
                        parts.push(part);
                        if cur == 0 || node.parent == cur {
                            break;
                        }
                        cur = node.parent;
                    }
                    parts.reverse();
                    parts.join(" › ")
                } else {
                    "Pick (↖) or right-click element".into()
                };
                self.inspector_text(canvas, x, y + 4, &trail, trail_w, 11.5, palette.muted);
                let matches = self.elements_search_matches();
                let placeholder = if matches.is_empty() && !self.devtools.elements_search.is_empty()
                {
                    "0 matches"
                } else {
                    "Find (#id, .cls, tag)"
                };
                self.paint_field_input(
                    canvas,
                    (search_x, y + 1, search_w, 20),
                    FieldKind::ElementsSearch,
                    placeholder,
                );
            }
            Tab::Console => {
                let (errs, warns) = self.devtools.console_counts();
                let err_txt = format!("Err({errs})");
                let warn_txt = format!("Warn({warns})");
                let pills = [
                    ("All", ConsoleFilter::All),
                    (err_txt.as_str(), ConsoleFilter::Errors),
                    (warn_txt.as_str(), ConsoleFilter::Warnings),
                    ("Info", ConsoleFilter::Info),
                    ("Verb", ConsoleFilter::Verbose),
                ];
                let mut cx = x;
                for (label, f) in pills {
                    let pw = (self.fonts.width(label, 11.).ceil() as u32 + 10).min(68);
                    if cx + pw as i32 > x + width as i32 - 110 {
                        break;
                    }
                    self.pill_button(
                        canvas,
                        (cx, y + 1, pw, 20),
                        label,
                        Action::DevToolsConsoleFilter(f),
                        self.devtools.console_filter == f,
                        dark,
                    );
                    cx += pw as i32 + 3;
                }
                let right_end = x + width as i32;
                let clear_w = 42u32;
                let copy_w = if width >= 220 { 40u32 } else { 0u32 };
                let pres_w = if width >= 460 { 64u32 } else { 0u32 };
                let clear_x = right_end - clear_w as i32;
                let copy_x = clear_x - if copy_w > 0 { copy_w as i32 + 3 } else { 0 };
                let pres_x = copy_x - if pres_w > 0 { pres_w as i32 + 3 } else { 0 };
                let filter_avail = (pres_x - cx - 4).max(0) as u32;
                if filter_avail >= 52 {
                    self.paint_field_input(
                        canvas,
                        (cx, y + 1, filter_avail, 20),
                        FieldKind::ConsoleFilter,
                        "Filter (-excl)",
                    );
                }
                if pres_w > 0 {
                    let pres_lbl = if self.devtools.preserve_log {
                        "☑ Presv"
                    } else {
                        "☐ Presv"
                    };
                    self.pill_button(
                        canvas,
                        (pres_x, y + 1, pres_w, 20),
                        pres_lbl,
                        Action::DevToolsConsolePreserve,
                        self.devtools.preserve_log,
                        dark,
                    );
                }
                if copy_w > 0 {
                    let has_sel = self.devtools.console_sel.is_some();
                    self.pill_button(
                        canvas,
                        (copy_x, y + 1, copy_w, 20),
                        "Copy",
                        if has_sel {
                            Action::DevToolsConsoleCopySelected
                        } else {
                            Action::DevToolsConsoleCopyAll
                        },
                        has_sel,
                        dark,
                    );
                }
                if width >= 160 {
                    self.pill_button(
                        canvas,
                        (clear_x, y + 1, clear_w, 20),
                        "Clear",
                        Action::DevToolsConsoleClear,
                        false,
                        dark,
                    );
                }
            }
            Tab::Sources => {
                let sources = self.collect_sources();
                let idx = self
                    .devtools
                    .selected_source
                    .min(sources.len().saturating_sub(1));
                let active_lbl = sources
                    .get(idx)
                    .map(|(k, name, b)| format!("[{k}] {name} · {}", format_bytes(b.len())))
                    .unwrap_or_else(|| "No sources".into());
                let search_w = (width * 40 / 100).clamp(110, 190);
                let search_x = x + width as i32 - search_w as i32;
                self.inspector_text(
                    canvas,
                    x,
                    y + 4,
                    &active_lbl,
                    (search_x - x - 6).max(40) as u32,
                    11.5,
                    palette.muted,
                );
                self.paint_field_input(
                    canvas,
                    (search_x, y + 1, search_w, 20),
                    FieldKind::SourcesSearch,
                    "Find in file (Ctrl+F)",
                );
            }
            Tab::Network => {
                let pills = [
                    ("All", NetFilter::All),
                    ("Doc", NetFilter::Doc),
                    ("CSS", NetFilter::Css),
                    ("JS", NetFilter::Js),
                    ("Font", NetFilter::Font),
                    ("Img", NetFilter::Img),
                ];
                let mut cx = x;
                for (label, f) in pills {
                    let pw = (self.fonts.width(label, 11.).ceil() as u32 + 10).min(48);
                    if cx + pw as i32 > x + width as i32 - 112 {
                        break;
                    }
                    self.pill_button(
                        canvas,
                        (cx, y + 1, pw, 20),
                        label,
                        Action::DevToolsNetFilter(f),
                        self.devtools.net_filter == f,
                        dark,
                    );
                    cx += pw as i32 + 3;
                }
                let right_end = x + width as i32;
                let clear_w = 42u32;
                let nocache_w = if width >= 430 { 72u32 } else { 0u32 };
                let pres_w = if width >= 510 { 64u32 } else { 0u32 };
                let clear_x = right_end - clear_w as i32;
                let nocache_x = clear_x
                    - if nocache_w > 0 {
                        nocache_w as i32 + 3
                    } else {
                        0
                    };
                let pres_x = nocache_x - if pres_w > 0 { pres_w as i32 + 3 } else { 0 };
                let filter_avail = (pres_x - cx - 4).max(0) as u32;
                if filter_avail >= 58 {
                    self.paint_field_input(
                        canvas,
                        (cx, y + 1, filter_avail, 20),
                        FieldKind::NetworkFilter,
                        "Filter URL",
                    );
                }
                if pres_w > 0 {
                    let lbl = if self.devtools.net_preserve_log {
                        "☑ Presv"
                    } else {
                        "☐ Presv"
                    };
                    self.pill_button(
                        canvas,
                        (pres_x, y + 1, pres_w, 20),
                        lbl,
                        Action::DevToolsNetPreserve,
                        self.devtools.net_preserve_log,
                        dark,
                    );
                }
                if nocache_w > 0 {
                    let nc = self.session.cache_disabled();
                    let lbl = if nc { "☑ NoCache" } else { "☐ NoCache" };
                    self.pill_button(
                        canvas,
                        (nocache_x, y + 1, nocache_w, 20),
                        lbl,
                        Action::DevToolsNetDisableCache,
                        nc,
                        dark,
                    );
                }
                if width >= 160 {
                    self.pill_button(
                        canvas,
                        (clear_x, y + 1, clear_w, 20),
                        "Clear",
                        Action::DevToolsNetClear,
                        false,
                        dark,
                    );
                }
            }
            Tab::Performance => {
                let rec_w = 108u32.min(width / 2);
                self.pill_button(
                    canvas,
                    (x, y + 1, rec_w, 20),
                    "● Reload & Profile",
                    Action::DevToolsPerfReload,
                    false,
                    dark,
                );
                let clear_w = 44u32;
                self.pill_button(
                    canvas,
                    (x + rec_w as i32 + 4, y + 1, clear_w, 20),
                    "Clear",
                    Action::DevToolsPerfClear,
                    false,
                    dark,
                );
                let p = &self.devtools.performance;
                let load_ms = p.fetch_html_ms
                    + p.parse_html_ms
                    + p.resources_ms
                    + p.external_scripts_ms
                    + p.script_worker_ms;
                let info_x = x + (rec_w + clear_w + 10) as i32;
                let info_w = (x + width as i32 - info_x).max(0) as u32;
                if info_w >= 60 {
                    let info = format!(
                        "Load {:.1}ms · Frame {:.2}ms · {}f/{}s hits",
                        load_ms, p.last_frame.total_ms, p.frame_cache_hits, p.style_cache_hits
                    );
                    self.inspector_text(canvas, info_x, y + 4, &info, info_w, 11.5, palette.muted);
                }
            }
            Tab::Application => {
                let secs = [
                    ("Cookies", AppSection::Cookies),
                    ("Security", AppSection::Security),
                    ("Resources", AppSection::Resources),
                ];
                let mut cx = x;
                for (lbl, sec) in secs {
                    let pw = (self.fonts.width(lbl, 11.5).ceil() as u32 + 14).min(78);
                    self.pill_button(
                        canvas,
                        (cx, y + 1, pw, 20),
                        lbl,
                        Action::DevToolsAppSection(sec),
                        self.devtools.app_section == sec,
                        dark,
                    );
                    cx += pw as i32 + 4;
                }
                if self.devtools.app_section == AppSection::Cookies && width >= 290 {
                    self.pill_button(
                        canvas,
                        (x + width as i32 - 88, y + 1, 88, 20),
                        "Clear Cookies",
                        Action::DevToolsAppClearCookies,
                        false,
                        dark,
                    );
                }
            }
        }
    }

    #[cfg(feature = "chrome")]
    fn paint_elements_rich(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        dark: bool,
    ) {
        let palette = self.effective_theme().palette();
        let tree_h = (height * 50 / 100).max(60);
        let rows = (tree_h / 20).max(1) as usize;
        let tree = self.dom_tree_rows();
        let search_matches = self.elements_search_matches();
        self.devtools.row = self.devtools.row.min(tree.len().saturating_sub(rows));
        let selected_id = self
            .devtools
            .selected
            .filter(|(epoch, _)| *epoch == self.dom_epoch)
            .map(|(_, id)| id);
        let tag_color = if dark { 0x8ab4f8 } else { 0x1a0dab };

        for (i, (node_id, depth, label)) in
            tree.iter().skip(self.devtools.row).take(rows).enumerate()
        {
            let ry = y + i as i32 * 20;
            let is_sel = selected_id == Some(*node_id);
            let is_match = !search_matches.is_empty() && search_matches.contains(node_id);
            if is_sel {
                canvas.rect(x, ry, width, 19, palette.selection);
                canvas.rect(x, ry, 3, 19, palette.accent);
            } else if is_match {
                canvas.rect(x, ry, width, 19, if dark { 0x3b3319 } else { 0xfef3c7 });
            } else if i % 2 == 1 {
                canvas.rect(x, ry, width, 19, palette.background);
            }
            let indent = (*depth as i32 * 10).min((width as i32) / 3);
            let has_children = self
                .document
                .nodes
                .get(*node_id)
                .is_some_and(|n| n.tag != "#text" && !n.children.is_empty());
            let color = if label.starts_with('"') {
                palette.ink
            } else {
                tag_color
            };
            self.inspector_text(
                canvas,
                x + 6 + indent,
                ry + 3,
                label,
                width.saturating_sub(10 + indent as u32),
                12.,
                color,
            );
            let toggle_w = if has_children {
                (indent as u32 + 18).min(width / 2)
            } else {
                0
            };
            if toggle_w > 0 {
                self.hits.push(Hit {
                    x,
                    y: ry,
                    w: toggle_w,
                    h: 19,
                    action: Action::DevToolsToggleExpandNode(*node_id),
                });
            }
            if width > toggle_w {
                self.hits.push(Hit {
                    x: x + toggle_w as i32,
                    y: ry,
                    w: width - toggle_w,
                    h: 19,
                    action: Action::DevToolsSelectNode(*node_id),
                });
            }
        }

        // Sub-tab bar: [Styles] [Computed] [Box Model] + H/Del shortcut hint
        let split_y = y + tree_h as i32 + 2;
        canvas.rect(x, split_y, width, 1, palette.border);
        canvas.rect(x, split_y + 1, width, 22, palette.background);
        let sub_tabs = [
            ("Styles", ElementsSubTab::Styles),
            ("Computed", ElementsSubTab::Computed),
            ("Box Model", ElementsSubTab::BoxModel),
        ];
        let mut sx = x + 4;
        for (lbl, sub) in sub_tabs {
            let sw = (self.fonts.width(lbl, 11.5).ceil() as u32 + 14).min(82);
            self.pill_button(
                canvas,
                (sx, split_y + 2, sw, 19),
                lbl,
                Action::DevToolsElementsSubTab(sub),
                self.devtools.elements_subtab == sub,
                dark,
            );
            sx += sw as i32 + 4;
        }
        if width >= 330 {
            self.inspector_text(
                canvas,
                sx + 4,
                split_y + 5,
                "H hide · Del remove",
                (x + width as i32 - sx - 8).max(0) as u32,
                10.5,
                palette.muted,
            );
        }
        canvas.rect(x, split_y + 23, width, 1, palette.border);

        let detail_y = split_y + 26;
        let detail_h = (y + height as i32 - detail_y).max(0) as u32;
        if detail_h < 18 {
            return;
        }
        let Some(sel_id) = selected_id else {
            self.inspector_text(
                canvas,
                x + 6,
                detail_y + 2,
                "Select a DOM element above or click ↖ to inspect styles.",
                width.saturating_sub(12),
                12.,
                palette.muted,
            );
            return;
        };

        match self.devtools.elements_subtab {
            ElementsSubTab::Styles => {
                // Live editable element.style { ... } box!
                self.inspector_text(
                    canvas,
                    x + 4,
                    detail_y + 3,
                    "element.style {",
                    96,
                    11.5,
                    palette.accent,
                );
                let field_x = x + 98;
                let field_w = width.saturating_sub(108).max(60);
                self.paint_field_input(
                    canvas,
                    (field_x, detail_y, field_w, 20),
                    FieldKind::InlineStyle,
                    "color: red; padding: 8px (Enter)",
                );
                self.inspector_text(
                    canvas,
                    field_x + field_w as i32 + 2,
                    detail_y + 3,
                    "}",
                    10,
                    11.5,
                    palette.accent,
                );

                let mut cy = detail_y + 24;
                let max_y = y + height as i32 - 16;
                let rules = self.matched_css_rules_for(sel_id);
                if rules.is_empty() {
                    if cy <= max_y {
                        self.inspector_text(
                            canvas,
                            x + 6,
                            cy,
                            "/* No author stylesheet rules matched; user-agent defaults apply */",
                            width.saturating_sub(12),
                            11.5,
                            palette.muted,
                        );
                        cy += 18;
                    }
                } else {
                    for (sel_txt, decls, origin) in rules {
                        if cy > max_y {
                            break;
                        }
                        let header = format!("{sel_txt} {{  /* {origin} */");
                        self.inspector_text(
                            canvas,
                            x + 6,
                            cy,
                            &header,
                            width.saturating_sub(12),
                            11.5,
                            tag_color,
                        );
                        cy += 16;
                        for decl in decls.split(';').map(str::trim).filter(|d| !d.is_empty()) {
                            if cy > max_y {
                                break;
                            }
                            let swatch = decl
                                .split_once(':')
                                .and_then(|(_, val)| parse_css_color_token(val));
                            let mut tx = x + 16;
                            if let Some(rgb) = swatch {
                                canvas.rect(tx, cy + 3, 10, 10, palette.border);
                                canvas.rect(tx + 1, cy + 4, 8, 8, rgb);
                                tx += 14;
                            }
                            self.inspector_text(
                                canvas,
                                tx,
                                cy,
                                &format!("{decl};"),
                                (x + width as i32 - tx - 6).max(20) as u32,
                                11.5,
                                palette.ink,
                            );
                            cy += 16;
                        }
                        if cy <= max_y {
                            self.inspector_text(canvas, x + 6, cy, "}", 20, 11.5, tag_color);
                            cy += 16;
                        }
                    }
                }
                if cy <= max_y
                    && let Some(node) = self.document.nodes.get(sel_id).cloned()
                    && !node.attributes.is_empty()
                {
                    let attrs: Vec<String> = node
                        .attributes
                        .iter()
                        .take(6)
                        .map(|(k, v)| format!("{}=\"{}\"", bounded(k, 24), bounded(v, 36)))
                        .collect();
                    self.inspector_text(
                        canvas,
                        x + 6,
                        cy,
                        &format!("Attributes: {}", attrs.join("  ")),
                        width.saturating_sub(12),
                        11.,
                        palette.muted,
                    );
                }
            }
            ElementsSubTab::Computed => {
                let props = self.computed_style_pairs(sel_id);
                if props.is_empty() {
                    self.inspector_text(
                        canvas,
                        x + 6,
                        detail_y + 2,
                        "Computed style unavailable for non-element node.",
                        width.saturating_sub(12),
                        12.,
                        palette.muted,
                    );
                    return;
                }
                let max_rows = (detail_h / 17).max(1) as usize;
                let prop_col_w = 122u32.min(width / 2);
                for (i, (name, val, swatch)) in props.into_iter().take(max_rows).enumerate() {
                    let ry = detail_y + i as i32 * 17;
                    if i % 2 == 1 {
                        canvas.rect(x, ry, width, 16, palette.background);
                    }
                    self.inspector_text(
                        canvas,
                        x + 6,
                        ry + 1,
                        name,
                        prop_col_w,
                        11.5,
                        palette.muted,
                    );
                    let mut vx = x + prop_col_w as i32 + 10;
                    if let Some(rgb) = swatch {
                        canvas.rect(vx, ry + 3, 10, 10, palette.border);
                        canvas.rect(vx + 1, ry + 4, 8, 8, rgb);
                        vx += 14;
                    }
                    self.inspector_text(
                        canvas,
                        vx,
                        ry + 1,
                        &val,
                        (x + width as i32 - vx - 4).max(20) as u32,
                        11.5,
                        palette.ink,
                    );
                }
            }
            ElementsSubTab::BoxModel => {
                let bounds = self.node_painted_bounds(self.dom_epoch, sel_id);
                let (bw, bh) = bounds
                    .map(|(bx, by, br, bb)| ((br - bx).max(0), (bb - by).max(0)))
                    .unwrap_or((0, 0));
                let (m, b, p) = if let Some(cs) = self.style_cache.style_for(sel_id) {
                    (
                        cs.margin.map(format_length),
                        cs.border_width.map(|v| format!("{v:.0}")),
                        cs.padding.map(format_length),
                    )
                } else {
                    (
                        ["0".into(), "0".into(), "0".into(), "0".into()],
                        ["0".into(), "0".into(), "0".into(), "0".into()],
                        ["0".into(), "0".into(), "0".into(), "0".into()],
                    )
                };
                let diag_w = width.saturating_sub(24).min(340);
                let diag_h = detail_h.saturating_sub(6).min(132);
                let dx = x + ((width - diag_w) / 2) as i32;
                let dy = detail_y + 2;
                if diag_h >= 84 && diag_w >= 200 {
                    // Concentric 4-layer Chromium Box Model diagram
                    let m_bg = if dark { 0x5c3d1e } else { 0xf9cb9c };
                    let b_bg = if dark { 0x5c4f1e } else { 0xffe599 };
                    let p_bg = if dark { 0x2d4d2f } else { 0xb6d7a8 };
                    let c_bg = if dark { 0x24425c } else { 0x9fc5e8 };
                    canvas.rect(dx, dy, diag_w, diag_h, m_bg);
                    canvas.rect(dx + 18, dy + 16, diag_w - 36, diag_h - 32, b_bg);
                    canvas.rect(dx + 36, dy + 32, diag_w - 72, diag_h - 64, p_bg);
                    canvas.rect(dx + 54, dy + 48, diag_w - 108, diag_h - 96, c_bg);

                    self.inspector_text(
                        canvas,
                        dx + 4,
                        dy + 2,
                        &format!(
                            "margin  top:{}  right:{}  bottom:{}  left:{}",
                            m[0], m[1], m[2], m[3]
                        ),
                        diag_w - 8,
                        10.5,
                        palette.ink,
                    );
                    self.inspector_text(
                        canvas,
                        dx + 22,
                        dy + 18,
                        &format!("border  {} / {} / {} / {} px", b[0], b[1], b[2], b[3]),
                        diag_w - 44,
                        10.5,
                        palette.ink,
                    );
                    self.inspector_text(
                        canvas,
                        dx + 40,
                        dy + 34,
                        &format!("padding  {} {} {} {}", p[0], p[1], p[2], p[3]),
                        diag_w - 80,
                        10.5,
                        palette.ink,
                    );
                    let content_lbl = format!("{bw} × {bh} px");
                    self.inspector_text(
                        canvas,
                        dx + 60,
                        dy + 52,
                        &content_lbl,
                        diag_w - 120,
                        11.5,
                        palette.ink,
                    );
                } else {
                    let fallback = format!(
                        "Box {bw}×{bh}px | margin: {} {} {} {} | border: {}px | pad: {} {} {} {}",
                        m[0], m[1], m[2], m[3], b[0], p[0], p[1], p[2], p[3]
                    );
                    self.inspector_text(
                        canvas,
                        x + 6,
                        detail_y + 4,
                        &fallback,
                        width.saturating_sub(12),
                        11.5,
                        palette.ink,
                    );
                }
            }
        }
    }

    #[cfg(feature = "chrome")]
    fn paint_console_rich(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        dark: bool,
    ) {
        let palette = self.effective_theme().palette();
        let line_count = self.devtools.console_input.lines().count().clamp(1, 3) as u32;
        let repl_h = (line_count * 16 + 10).min(height.saturating_sub(24).max(26));
        let log_h = height.saturating_sub(repl_h + 4);
        let repl_y = y + height as i32 - repl_h as i32;
        self.devtools.console_log_rect = Some((x, y, width, log_h));
        self.devtools.console_repl_rect = Some((x, repl_y, width, repl_h));
        let text_w = width.saturating_sub(84).max(80) as f32;
        let visual = self.build_console_visual_rows(text_w);
        let rows = (log_h / 20).max(1) as usize;
        if std::mem::take(&mut self.devtools.scroll_to_bottom) {
            self.devtools.row = visual.len().saturating_sub(rows);
        } else {
            self.devtools.row = self.devtools.row.min(visual.len().saturating_sub(rows));
        }

        if visual.is_empty() {
            let msg = match self.devtools.console_filter {
                ConsoleFilter::Errors => "No errors recorded. Type JS below to evaluate in worker.",
                ConsoleFilter::Warnings => "No warnings recorded on this page.",
                _ => "Console ready · Type JS below ($0, $$, keys, table, inspect, copy).",
            };
            self.inspector_text(
                canvas,
                x + 6,
                y + 6,
                msg,
                width.saturating_sub(12),
                12.,
                palette.muted,
            );
        } else {
            for (i, vrow) in visual.iter().skip(self.devtools.row).take(rows).enumerate() {
                let row_idx = self.devtools.row + i;
                let ry = y + i as i32 * 20;
                let (bg, stripe, ink) = match vrow.kind {
                    ConsoleEntryKind::Command => (
                        if dark { 0x1e293b } else { 0xf1f5f9 },
                        palette.accent,
                        if dark { 0x93c5fd } else { 0x1d4ed8 },
                    ),
                    ConsoleEntryKind::NavMarker => {
                        (palette.background, palette.accent, palette.muted)
                    }
                    _ => match vrow.level {
                        ConsoleLevel::Error => {
                            if dark {
                                (0x3b2326, 0xf28b82, 0xfad2cf)
                            } else {
                                (0xfff0f0, 0xd93025, 0x8c1d18)
                            }
                        }
                        ConsoleLevel::Warning => {
                            if dark {
                                (0x38301e, 0xfdd663, 0xfde293)
                            } else {
                                (0xfef7e0, 0xe37400, 0x5f370e)
                            }
                        }
                        ConsoleLevel::Verbose => (palette.panel, palette.border, palette.muted),
                        ConsoleLevel::Info => (palette.panel, palette.border, palette.ink),
                    },
                };
                canvas.rect(x, ry, width, 19, bg);
                let is_sel = self
                    .devtools
                    .console_sel
                    .is_some_and(|(a, b)| row_idx >= a.min(b) && row_idx <= a.max(b));
                if is_sel {
                    blend_rect(
                        canvas,
                        x,
                        ry,
                        width,
                        19,
                        0x2563eb,
                        if dark { 84 } else { 56 },
                    );
                    canvas.rect(x, ry, 3, 19, palette.accent);
                } else {
                    canvas.rect(x, ry, 3, 19, stripe);
                }
                canvas.rect(x, ry + 19, width, 1, palette.border);

                let indent = (vrow.group_depth as i32 * 10).min(60);
                if vrow.first_of_entry {
                    let badge = match vrow.kind {
                        ConsoleEntryKind::Command => "IN",
                        ConsoleEntryKind::Result if vrow.level != ConsoleLevel::Error => "OUT",
                        ConsoleEntryKind::Table => "TBL",
                        ConsoleEntryKind::Group { .. } => "GRP",
                        ConsoleEntryKind::NavMarker => "NAV",
                        _ => match vrow.level {
                            ConsoleLevel::Error => "ERR",
                            ConsoleLevel::Warning => "WRN",
                            ConsoleLevel::Verbose => "DBG",
                            ConsoleLevel::Info => "LOG",
                        },
                    };
                    self.inspector_text(canvas, x + 6, ry + 3, badge, 28, 10.5, stripe);
                    self.inspector_text(
                        canvas,
                        x + 34,
                        ry + 3,
                        vrow.source,
                        28,
                        10.5,
                        palette.muted,
                    );
                }

                let inspect_w = if vrow.inspect_node.is_some() {
                    54u32
                } else {
                    0
                };
                let msg_x = x + 64 + indent;
                let msg_w = (x + width as i32 - msg_x - inspect_w as i32 - 4).max(24) as u32;
                self.inspector_text(canvas, msg_x, ry + 2, &vrow.text, msg_w, 12., ink);

                if let Some(group_idx) = vrow.toggle_group {
                    self.hits.push(Hit {
                        x,
                        y: ry,
                        w: width.saturating_sub(inspect_w),
                        h: 19,
                        action: Action::DevToolsConsoleToggleGroup(group_idx),
                    });
                } else if let Some((entry_idx, object_id)) = vrow.toggle_object {
                    self.hits.push(Hit {
                        x,
                        y: ry,
                        w: width.saturating_sub(inspect_w),
                        h: 19,
                        action: Action::DevToolsConsoleToggleObject {
                            entry_idx,
                            object_id,
                        },
                    });
                } else {
                    self.hits.push(Hit {
                        x,
                        y: ry,
                        w: width.saturating_sub(inspect_w),
                        h: 19,
                        action: Action::DevToolsConsoleSelectRow(row_idx),
                    });
                }
                if let Some(nid) = vrow.inspect_node {
                    self.pill_button(
                        canvas,
                        (x + width as i32 - 52, ry + 1, 50, 17),
                        "Inspect",
                        Action::DevToolsSelectConsoleNode(nid),
                        false,
                        dark,
                    );
                }
            }
        }

        // Interactive JS REPL prompt bar at bottom of Console body
        let focused = self.focus == Focus::ConsoleInput;
        canvas.rect(
            x,
            repl_y,
            width,
            repl_h,
            if focused {
                palette.accent
            } else {
                palette.border
            },
        );
        canvas.rect(
            x + 1,
            repl_y + 1,
            width.saturating_sub(2),
            repl_h.saturating_sub(2),
            palette.field,
        );
        self.inspector_text(canvas, x + 6, repl_y + 5, ">", 14, 13., palette.accent);

        if self.devtools.console_input.is_empty() && !focused {
            self.inspector_text(
                canvas,
                x + 20,
                repl_y + 6,
                "Evaluate JS in isolated worker ($0, $$, table, inspect)…",
                width.saturating_sub(26),
                12.,
                palette.muted,
            );
        } else {
            let ghost = if focused
                && self.devtools.console_anchor.is_none()
                && self.devtools.console_cursor >= self.devtools.console_input.len()
            {
                self.console_completion_suffix().unwrap_or("").to_owned()
            } else {
                String::new()
            };
            let input_snapshot = self.devtools.console_input.clone();
            let cur_global =
                crate::clipboard::clamp_utf8(&input_snapshot, self.devtools.console_cursor);
            let sel_global = if focused {
                crate::clipboard::normalized_range(
                    &input_snapshot,
                    cur_global,
                    self.devtools.console_anchor,
                )
            } else {
                None
            };
            let avail_w = width.saturating_sub(32) as f32;
            let mut line_start = 0usize;
            for (li, raw_line) in input_snapshot.split('\n').take(3).enumerate() {
                let line_end = line_start + raw_line.len();
                let ly = repl_y + 5 + li as i32 * 16;
                let is_caret_line = (cur_global >= line_start && cur_global <= line_end)
                    || (li == 2 && cur_global > line_end);
                let cur_in_line = if is_caret_line {
                    cur_global.saturating_sub(line_start).min(raw_line.len())
                } else {
                    raw_line.len()
                };
                let line_sel = sel_global.and_then(|(s0, s1)| {
                    let l0 = s0.max(line_start).min(line_end) - line_start;
                    let l1 = s1.max(line_start).min(line_end) - line_start;
                    (l1 > l0).then_some((l0, l1))
                });
                let (shown, sx0, sx1, cx_rel) = crate::clipboard::slice_line_bytes(
                    &mut self.fonts,
                    raw_line,
                    12.5,
                    avail_w,
                    line_sel,
                    cur_in_line,
                );
                if focused && sx1 > sx0 {
                    canvas.rect(x + 20 + sx0, ly, (sx1 - sx0) as u32, 15, palette.selection);
                }
                canvas.text(&mut self.fonts, x + 20, ly, &shown, 12.5, palette.ink);
                if focused && is_caret_line {
                    let cx_pos = (x + 20 + cx_rel).min(x + width as i32 - 8);
                    canvas.rect(cx_pos, ly + 1, 2, 14, palette.accent);
                    if !ghost.is_empty() && cx_pos + 16 < x + width as i32 {
                        self.inspector_text(
                            canvas,
                            cx_pos + 3,
                            ly,
                            &ghost,
                            (x + width as i32 - cx_pos - 6) as u32,
                            12.5,
                            palette.muted,
                        );
                    }
                }
                line_start = line_end + 1;
            }
        }
        self.hits.push(Hit {
            x,
            y: repl_y,
            w: width,
            h: repl_h,
            action: Action::DevToolsConsoleInput,
        });
    }

    #[cfg(feature = "chrome")]
    fn paint_sources_rich(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        dark: bool,
    ) {
        let palette = self.effective_theme().palette();
        let sources = self.collect_sources();
        if self.devtools.selected_source >= sources.len() {
            self.devtools.selected_source = 0;
        }
        let nav_w = (width * 28 / 100).clamp(108, 176);
        canvas.rect(x, y, nav_w, height, palette.background);
        canvas.rect(x + nav_w as i32, y, 1, height, palette.border);

        let nav_rows = (height / 20).max(1) as usize;
        for (i, (kind, name, _)) in sources.iter().take(nav_rows).enumerate() {
            let ry = y + i as i32 * 20;
            let active = i == self.devtools.selected_source;
            if active {
                canvas.rect(x, ry, nav_w, 19, palette.selection);
                canvas.rect(x, ry, 3, 19, palette.accent);
            }
            let badge_col = match *kind {
                "HTML" => 0x3b82f6,
                "CSS" => 0xa855f7,
                _ => 0xf59e0b,
            };
            self.inspector_text(canvas, x + 5, ry + 4, kind, 28, 10., badge_col);
            self.inspector_text(
                canvas,
                x + 34,
                ry + 3,
                name,
                nav_w.saturating_sub(38),
                11.5,
                palette.ink,
            );
            self.hits.push(Hit {
                x,
                y: ry,
                w: nav_w,
                h: 19,
                action: Action::DevToolsSourceSelect(i),
            });
        }

        let code_x = x + nav_w as i32 + 4;
        let code_w = width.saturating_sub(nav_w + 4);
        self.devtools.sources_code_rect = Some((code_x, y, code_w, height));
        let Some((kind, _, content)) = sources.get(self.devtools.selected_source) else {
            return;
        };
        let all_lines: Vec<&str> = content.lines().take(4096).collect();
        let vis_rows = (height / 18).max(1) as usize;
        self.devtools.row = self
            .devtools
            .row
            .min(all_lines.len().saturating_sub(vis_rows));
        let query = self.devtools.sources_search.trim().to_ascii_lowercase();
        let gutter_w = 34u32;

        for (i, raw_line) in all_lines
            .iter()
            .enumerate()
            .skip(self.devtools.row)
            .take(vis_rows)
        {
            let line_no = i;
            let ry = y + (i - self.devtools.row) as i32 * 18;
            let is_sel = self
                .devtools
                .sources_sel
                .is_some_and(|(a, b)| line_no >= a.min(b) && line_no <= a.max(b));
            let is_hit = !query.is_empty() && raw_line.to_ascii_lowercase().contains(&query);
            if is_sel {
                canvas.rect(code_x, ry, code_w, 17, palette.selection);
            } else if is_hit {
                canvas.rect(
                    code_x,
                    ry,
                    code_w,
                    17,
                    if dark { 0x3b3319 } else { 0xfef3c7 },
                );
            }
            canvas.rect(code_x, ry, gutter_w, 18, palette.background);
            self.inspector_text(
                canvas,
                code_x + 3,
                ry + 2,
                &format!("{:>4}", line_no + 1),
                gutter_w - 4,
                10.5,
                palette.muted,
            );
            let trimmed = raw_line.trim_start();
            let syntax_col = if trimmed.starts_with("//")
                || trimmed.starts_with("/*")
                || trimmed.starts_with("<!--")
            {
                palette.muted
            } else if *kind == "HTML" && trimmed.starts_with('<') {
                if dark { 0x8ab4f8 } else { 0x1a0dab }
            } else if *kind == "CSS" && trimmed.contains('{') {
                if dark { 0xc084fc } else { 0x6b21a8 }
            } else {
                palette.ink
            };
            let expanded_tabs = raw_line.replace('\t', "  ");
            self.inspector_text(
                canvas,
                code_x + gutter_w as i32 + 5,
                ry + 2,
                &expanded_tabs,
                code_w.saturating_sub(gutter_w + 8),
                11.5,
                syntax_col,
            );
            self.hits.push(Hit {
                x: code_x,
                y: ry,
                w: code_w,
                h: 18,
                action: Action::DevToolsSourceSelectLine(line_no),
            });
        }
    }

    #[cfg(feature = "chrome")]
    fn paint_network_rich(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        dark: bool,
    ) {
        let palette = self.effective_theme().palette();
        canvas.rect(x, y, width, 20, palette.background);
        canvas.rect(x, y + 20, width, 1, palette.border);
        let wf_w = (width / 5).clamp(54, 108);
        let time_w = 48u32;
        let size_w = 56u32;
        let type_w = 34u32;
        let status_w = 38u32;
        let name_x = x + (status_w + type_w + 8) as i32;
        let fixed_cols = status_w + type_w + size_w + time_w + wf_w + 20;
        let name_w = width.saturating_sub(fixed_cols).max(48);
        let size_x = name_x + name_w as i32 + 4;
        let time_x = size_x + size_w as i32 + 4;
        let wf_x = x + width as i32 - wf_w as i32 - 4;

        self.inspector_text(canvas, x + 4, y + 4, "Stat", status_w, 11., palette.muted);
        self.inspector_text(
            canvas,
            x + status_w as i32 + 6,
            y + 4,
            "Type",
            type_w,
            11.,
            palette.muted,
        );
        self.inspector_text(canvas, name_x, y + 4, "Name", name_w, 11., palette.muted);
        self.inspector_text(canvas, size_x, y + 4, "Size", size_w, 11., palette.muted);
        self.inspector_text(canvas, time_x, y + 4, "Time", time_w, 11., palette.muted);
        self.inspector_text(canvas, wf_x, y + 4, "Waterfall", wf_w, 11., palette.muted);

        let indices = self.filtered_network_indices();
        if self
            .devtools
            .selected_request
            .is_some_and(|sel| sel >= self.devtools.network.len())
        {
            self.devtools.selected_request = None;
        }
        let drawer_h = if self.devtools.selected_request.is_some() && height >= 190 {
            (height * 48 / 100).clamp(112, 180)
        } else {
            0
        };
        let list_h = height.saturating_sub(22 + drawer_h);
        let rows = (list_h / 20).max(1) as usize;
        self.devtools.row = self.devtools.row.min(indices.len().saturating_sub(rows));

        let total_span = self
            .devtools
            .network
            .iter()
            .map(|e| e.start_ms + e.duration_ms)
            .fold(1.0f64, f64::max);
        let dcl_ms =
            self.devtools.performance.fetch_html_ms + self.devtools.performance.parse_html_ms;
        let load_ms = dcl_ms
            + self.devtools.performance.resources_ms
            + self.devtools.performance.external_scripts_ms
            + self.devtools.performance.script_worker_ms;

        if indices.is_empty() {
            self.inspector_text(
                canvas,
                x + 8,
                y + 30,
                "Navigate or reload to record network activity.",
                width.saturating_sub(16),
                12.,
                palette.muted,
            );
        }

        for (i, &req_idx) in indices
            .iter()
            .skip(self.devtools.row)
            .take(rows)
            .enumerate()
        {
            let ry = y + 22 + i as i32 * 20;
            let entry = self.devtools.network[req_idx].clone();
            let is_sel = self.devtools.selected_request == Some(req_idx);
            if is_sel {
                canvas.rect(x, ry, width, 19, palette.selection);
                canvas.rect(x, ry, 3, 19, palette.accent);
            } else if i % 2 == 1 {
                canvas.rect(x, ry, width, 19, palette.background);
            }
            let (status_txt, status_col) = if entry.pending {
                ("···".into(), palette.muted)
            } else if let Some(code) = entry.status {
                let col = if (200..300).contains(&code) {
                    if dark { 0x81c995 } else { 0x188038 }
                } else if (300..400).contains(&code) {
                    if dark { 0x8ab4f8 } else { 0x1967d2 }
                } else if dark {
                    0xf28b82
                } else {
                    0xd93025
                };
                (code.to_string(), col)
            } else {
                ("ERR".into(), if dark { 0xf28b82 } else { 0xd93025 })
            };
            self.inspector_text(
                canvas,
                x + 4,
                ry + 3,
                &status_txt,
                status_w,
                12.,
                status_col,
            );
            self.inspector_text(
                canvas,
                x + status_w as i32 + 6,
                ry + 3,
                entry.kind.label(),
                type_w,
                11.5,
                palette.muted,
            );
            let name = short_resource_name(&entry.url);
            self.inspector_text(canvas, name_x, ry + 3, &name, name_w, 12., palette.ink);
            let sz = if entry.pending {
                "—".into()
            } else {
                format_bytes(entry.bytes)
            };
            self.inspector_text(canvas, size_x, ry + 3, &sz, size_w, 11.5, palette.muted);
            let tm = if entry.pending {
                "…".into()
            } else {
                format!("{:.0}ms", entry.duration_ms)
            };
            self.inspector_text(canvas, time_x, ry + 3, &tm, time_w, 11.5, palette.muted);

            // Waterfall bar + DOMContentLoaded (blue) & Load (red) vertical markers
            canvas.rect(wf_x, ry + 6, wf_w, 7, palette.border);
            if !entry.pending {
                let bar_x = wf_x
                    + ((entry.start_ms / total_span) * f64::from(wf_w.saturating_sub(3))) as i32;
                let bar_w = (((entry.duration_ms / total_span) * f64::from(wf_w)).ceil() as u32)
                    .clamp(3, (wf_x + wf_w as i32 - bar_x).max(3) as u32);
                let bar_col = if entry.error.is_some() {
                    if dark { 0xf28b82 } else { 0xd93025 }
                } else {
                    match entry.kind {
                        NetKind::Doc => 0x3b82f6,
                        NetKind::Css => 0xa855f7,
                        NetKind::Js => 0xf59e0b,
                        NetKind::Font => 0x10b981,
                        NetKind::Img => 0x06b6d4,
                    }
                };
                canvas.rect(bar_x, ry + 6, bar_w, 7, bar_col);
            }
            if dcl_ms > 0.0 {
                let dx = wf_x + ((dcl_ms / total_span).min(1.0) * f64::from(wf_w - 1)) as i32;
                canvas.rect(dx, ry + 2, 1, 15, 0x3b82f6);
            }
            if load_ms > 0.0 {
                let lx = wf_x + ((load_ms / total_span).min(1.0) * f64::from(wf_w - 1)) as i32;
                canvas.rect(lx, ry + 2, 1, 15, 0xef4444);
            }
            let next_sel = if is_sel { None } else { Some(req_idx) };
            self.hits.push(Hit {
                x,
                y: ry,
                w: width,
                h: 19,
                action: Action::DevToolsNetSelect(next_sel),
            });
        }

        // 4-Tab Request Inspector Drawer: [Headers] [Response] [Preview] [Timing] [×]
        if drawer_h > 0
            && let Some(sel) = self.devtools.selected_request
            && let Some(entry) = self.devtools.network.get(sel).cloned()
        {
            let dy = y + height as i32 - drawer_h as i32;
            canvas.rect(x, dy, width, 1, palette.border);
            canvas.rect(x, dy + 1, width, 22, palette.background);
            let subtabs = [
                ("Headers", NetworkSubTab::Headers),
                ("Response", NetworkSubTab::Response),
                ("Preview", NetworkSubTab::Preview),
                ("Timing", NetworkSubTab::Timing),
            ];
            let mut sx = x + 4;
            for (lbl, st) in subtabs {
                let sw = (self.fonts.width(lbl, 11.).ceil() as u32 + 12).min(72);
                self.pill_button(
                    canvas,
                    (sx, dy + 2, sw, 19),
                    lbl,
                    Action::DevToolsNetSubTab(st),
                    self.devtools.net_subtab == st,
                    dark,
                );
                sx += sw as i32 + 3;
            }
            self.pill_button(
                canvas,
                (x + width as i32 - 26, dy + 2, 22, 19),
                "×",
                Action::DevToolsNetSelect(None),
                false,
                dark,
            );
            canvas.rect(x, dy + 23, width, 1, palette.border);

            let content_y = dy + 26;
            let max_rows = ((drawer_h.saturating_sub(28)) / 16).max(1) as usize;
            let status_desc = match entry.status {
                Some(code) => format!("HTTP {code}"),
                None if entry.pending => "Pending".into(),
                None => entry
                    .error
                    .clone()
                    .unwrap_or_else(|| "Transport error".into()),
            };
            let drawer_lines: Vec<String> = match self.devtools.net_subtab {
                NetworkSubTab::Headers => {
                    let mut out = vec![
                        format!("General: {} {} -> {}", entry.method, entry.url, status_desc),
                        format!(
                            "Content-Type: {} · Size: {}",
                            if entry.mime.is_empty() {
                                entry.kind.label()
                            } else {
                                entry.mime.as_str()
                            },
                            format_bytes(entry.bytes)
                        ),
                    ];
                    if !entry.response_headers.is_empty() {
                        out.push("Response Headers:".into());
                        for (k, v) in entry.response_headers.iter().take(8) {
                            out.push(format!("  {k}: {v}"));
                        }
                    }
                    if !entry.request_headers.is_empty() {
                        out.push("Request Headers:".into());
                        for (k, v) in entry.request_headers.iter().take(6) {
                            out.push(format!("  {k}: {v}"));
                        }
                    }
                    out
                }
                NetworkSubTab::Response => match &entry.body_preview {
                    Some(body) if !body.is_empty() => body
                        .lines()
                        .take(32)
                        .enumerate()
                        .map(|(i, l)| format!("{:>3} │ {}", i + 1, l))
                        .collect(),
                    _ if entry.kind == NetKind::Doc && !self.devtools.page_source.is_empty() => {
                        self.devtools
                            .page_source
                            .lines()
                            .take(32)
                            .enumerate()
                            .map(|(i, l)| format!("{:>3} │ {}", i + 1, l))
                            .collect()
                    }
                    _ => vec!["(Binary or unretained response payload)".into()],
                },
                NetworkSubTab::Preview => {
                    if let Some(body) = &entry.body_preview {
                        let snippet: String = body
                            .split_whitespace()
                            .take(32)
                            .collect::<Vec<_>>()
                            .join(" ");
                        vec![
                            format!(
                                "{} resource · {} · {}",
                                entry.kind.label(),
                                status_desc,
                                format_bytes(entry.bytes)
                            ),
                            snippet,
                        ]
                    } else {
                        vec![
                            format!(
                                "{} asset ({}) · {}",
                                entry.kind.label(),
                                if entry.mime.is_empty() {
                                    "binary"
                                } else {
                                    entry.mime.as_str()
                                },
                                format_bytes(entry.bytes)
                            ),
                            format!("URL: {}", entry.url),
                        ]
                    }
                }
                NetworkSubTab::Timing => vec![
                    format!("Queued / Start offset: +{:.2} ms", entry.start_ms),
                    format!("Request + Download duration: {:.2} ms", entry.duration_ms),
                    format!(
                        "Completed at: +{:.2} ms (DOMContentLoaded {:.1} ms, Load {:.1} ms)",
                        entry.start_ms + entry.duration_ms,
                        dcl_ms,
                        load_ms
                    ),
                ],
            };
            let wrapped = self.wrap_lines(&drawer_lines, width.saturating_sub(12) as f32);
            for (i, line) in wrapped.iter().take(max_rows).enumerate() {
                self.inspector_text(
                    canvas,
                    x + 6,
                    content_y + i as i32 * 16,
                    line,
                    width.saturating_sub(12),
                    11.,
                    palette.ink,
                );
            }
        }
    }

    #[cfg(feature = "chrome")]
    fn paint_performance_rich(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        dark: bool,
    ) {
        let palette = self.effective_theme().palette();
        let p = &self.devtools.performance;
        let phases = [
            ("HTML", p.fetch_html_ms, 0x3b82f6u32),
            ("Parse", p.parse_html_ms, 0x8b5cf6),
            ("Res", p.resources_ms, 0x10b981),
            ("JS", p.external_scripts_ms + p.script_worker_ms, 0xf59e0b),
            ("Paint", p.last_full_render.total_ms, 0xec4899),
        ];
        let total = phases.iter().map(|(_, ms, _)| *ms).sum::<f64>().max(0.1);
        canvas.rect(x, y + 2, width, 12, palette.background);
        let mut bx = x;
        for (_, ms, color) in phases {
            if ms <= 0.0 {
                continue;
            }
            let seg = ((ms / total) * f64::from(width)).round() as u32;
            let seg = seg.min((x + width as i32 - bx).max(0) as u32);
            if seg > 0 {
                canvas.rect(bx, y + 2, seg, 12, color);
                bx += seg as i32;
            }
        }
        // Phase legend row
        let mut lx = x;
        for (lbl, ms, color) in phases {
            if lx + 64 > x + width as i32 {
                break;
            }
            canvas.rect(lx, y + 19, 8, 8, color);
            let txt = format!("{lbl} {:.1}ms", ms);
            let tw = (self.fonts.width(&txt, 10.5).ceil() as u32 + 16).min(110);
            self.inspector_text(canvas, lx + 11, y + 16, &txt, tw, 10.5, palette.muted);
            lx += tw as i32;
        }

        // 64-Frame Render Duration Sparkline with 16.7ms (60 FPS) budget line
        let mut content_top = y + 34;
        if height >= 140 {
            let spark_h = 44u32;
            let sy = content_top;
            canvas.rect(x, sy, width, spark_h, palette.background);
            canvas.rect(x, sy, width, 1, palette.border);
            canvas.rect(x, sy + spark_h as i32 - 1, width, 1, palette.border);

            let hist = &self.devtools.frame_history_ms;
            let max_ms = hist.iter().copied().fold(20.0f64, f64::max).min(200.0);
            // 16.7ms 60 FPS target line
            let budget_y = sy + spark_h as i32
                - 2
                - ((16.67 / max_ms) * f64::from(spark_h - 6)).round() as i32;
            let budget_col = if dark { 0x166534 } else { 0x86efac };
            canvas.rect(
                x + 2,
                budget_y.clamp(sy + 2, sy + spark_h as i32 - 3),
                width.saturating_sub(4),
                1,
                budget_col,
            );

            let slots = MAX_FRAME_HISTORY.max(1) as u32;
            let bar_step = (width.saturating_sub(8) / slots).max(2);
            for (idx, &ms) in hist.iter().enumerate() {
                let bx_bar = x + 4 + idx as i32 * bar_step as i32;
                if bx_bar + 2 > x + width as i32 - 2 {
                    break;
                }
                let bh = (((ms.min(max_ms) / max_ms) * f64::from(spark_h - 6)).ceil() as u32)
                    .clamp(2, spark_h - 4);
                let col = if ms > 16.7 {
                    if dark { 0xf87171 } else { 0xdc2626 }
                } else if ms > 8.0 {
                    0xf59e0b
                } else {
                    0x3b82f6
                };
                canvas.rect(
                    bx_bar,
                    sy + spark_h as i32 - 2 - bh as i32,
                    (bar_step.saturating_sub(1)).max(2),
                    bh,
                    col,
                );
            }
            let last_ms = self.devtools.performance.last_frame.total_ms;
            let spark_lbl = format!(
                "Frame Timeline ({} samples · last {:.2}ms · green = 16.7ms / 60fps)",
                hist.len(),
                last_ms
            );
            self.inspector_text(
                canvas,
                x + 6,
                sy + 2,
                &spark_lbl,
                width.saturating_sub(12),
                10.,
                palette.muted,
            );
            content_top = sy + spark_h as i32 + 4;
        } else {
            canvas.rect(x, y + 32, width, 1, palette.border);
        }

        let rem_h = (y + height as i32 - content_top).max(16) as u32;
        self.paint_compact_lines(canvas, x + 2, content_top, width.saturating_sub(4), rem_h);
    }

    #[cfg(feature = "chrome")]
    fn paint_application_rich(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        dark: bool,
    ) {
        let palette = self.effective_theme().palette();
        match self.devtools.app_section {
            AppSection::Cookies => {
                let cookies = self.session.cookies_snapshot();
                canvas.rect(x, y, width, 20, palette.background);
                canvas.rect(x, y + 20, width, 1, palette.border);
                let name_w = (width * 24 / 100).clamp(64, 130);
                let dom_w = (width * 28 / 100).clamp(72, 150);
                let path_w = 54u32;
                let val_w = 72u32;
                let flags_x = x + (name_w + dom_w + path_w + val_w + 16) as i32;
                let flags_w = (x + width as i32 - flags_x - 4).max(36) as u32;

                self.inspector_text(canvas, x + 4, y + 4, "Name", name_w, 11., palette.muted);
                self.inspector_text(
                    canvas,
                    x + name_w as i32 + 6,
                    y + 4,
                    "Domain",
                    dom_w,
                    11.,
                    palette.muted,
                );
                self.inspector_text(
                    canvas,
                    x + (name_w + dom_w) as i32 + 8,
                    y + 4,
                    "Path",
                    path_w,
                    11.,
                    palette.muted,
                );
                self.inspector_text(
                    canvas,
                    x + (name_w + dom_w + path_w) as i32 + 10,
                    y + 4,
                    "Value",
                    val_w,
                    11.,
                    palette.muted,
                );
                self.inspector_text(canvas, flags_x, y + 4, "Attrs", flags_w, 11., palette.muted);

                if cookies.is_empty() {
                    self.inspector_text(
                        canvas,
                        x + 8,
                        y + 28,
                        "No cookies stored in session jar (values always redacted).",
                        width.saturating_sub(16),
                        12.,
                        palette.muted,
                    );
                    return;
                }
                let rows = ((height.saturating_sub(24)) / 20).max(1) as usize;
                self.devtools.row = self.devtools.row.min(cookies.len().saturating_sub(rows));
                for (i, ck) in cookies
                    .iter()
                    .skip(self.devtools.row)
                    .take(rows)
                    .enumerate()
                {
                    let ry = y + 22 + i as i32 * 20;
                    if i % 2 == 1 {
                        canvas.rect(x, ry, width, 19, palette.background);
                    }
                    self.inspector_text(canvas, x + 4, ry + 3, &ck.name, name_w, 11.5, palette.ink);
                    let dom_txt = if ck.host_only {
                        ck.domain.clone()
                    } else {
                        format!(".{}", ck.domain)
                    };
                    self.inspector_text(
                        canvas,
                        x + name_w as i32 + 6,
                        ry + 3,
                        &dom_txt,
                        dom_w,
                        11.5,
                        palette.muted,
                    );
                    self.inspector_text(
                        canvas,
                        x + (name_w + dom_w) as i32 + 8,
                        ry + 3,
                        &ck.path,
                        path_w,
                        11.5,
                        palette.muted,
                    );
                    let val_mask = format!("••• ({} B)", ck.value_bytes);
                    self.inspector_text(
                        canvas,
                        x + (name_w + dom_w + path_w) as i32 + 10,
                        ry + 3,
                        &val_mask,
                        val_w,
                        11.,
                        if dark { 0x93c5fd } else { 0x1d4ed8 },
                    );
                    let exp = match ck.ttl_secs {
                        Some(s) => format!("{s}s"),
                        None if ck.session_only => "Session".into(),
                        None => "Expired".into(),
                    };
                    let flags = format!("{}{}", if ck.secure { "Secure · " } else { "" }, exp);
                    self.inspector_text(
                        canvas,
                        flags_x,
                        ry + 3,
                        &flags,
                        flags_w,
                        11.,
                        palette.muted,
                    );
                }
            }
            AppSection::Security | AppSection::Resources => {
                self.paint_compact_lines(
                    canvas,
                    x + 4,
                    y + 2,
                    width.saturating_sub(8),
                    height.saturating_sub(4),
                );
            }
        }
    }

    #[cfg(feature = "chrome")]
    fn paint_field_input(
        &mut self,
        canvas: &mut Canvas,
        area: (i32, i32, u32, u32),
        kind: FieldKind,
        placeholder: &str,
    ) {
        let (x, y, width, height) = area;
        if width < 24 || height < 14 {
            return;
        }
        self.devtools.field_rects.push((kind, area));
        let palette = self.effective_theme().palette();
        let focused = self.focus == Focus::DevToolsField(kind);
        canvas.rect(
            x,
            y,
            width,
            height,
            if focused {
                palette.accent
            } else {
                palette.border
            },
        );
        canvas.rect(
            x + 1,
            y + 1,
            width.saturating_sub(2),
            height.saturating_sub(2),
            palette.field,
        );
        let (value, cur_b) = {
            let (buf, cur, _) = self.field_state_mut(kind);
            (buf.clone(), *cur)
        };
        let anc = self
            .devtools
            .field_anchor
            .filter(|(k, _)| *k == kind)
            .map(|(_, a)| a);
        let pad_y = (height.saturating_sub(14) / 2) as i32;
        if value.is_empty() && !focused {
            self.inspector_text(
                canvas,
                x + 5,
                y + pad_y,
                placeholder,
                width.saturating_sub(10),
                11.,
                palette.muted,
            );
        } else {
            let sel = if focused {
                crate::clipboard::normalized_range(&value, cur_b, anc)
            } else {
                None
            };
            let caret_b = if focused { cur_b } else { value.len() };
            let (shown, sx0, sx1, cx_rel) = crate::clipboard::slice_line_bytes(
                &mut self.fonts,
                &value,
                11.5,
                width.saturating_sub(12) as f32,
                sel,
                caret_b,
            );
            if focused && sx1 > sx0 {
                canvas.rect(
                    x + 5 + sx0,
                    y + 2,
                    (sx1 - sx0) as u32,
                    height.saturating_sub(4),
                    palette.selection,
                );
            }
            canvas.text(&mut self.fonts, x + 5, y + pad_y, &shown, 11.5, palette.ink);
            if focused {
                let cx_pos = (x + 5 + cx_rel).min(x + width as i32 - 4);
                canvas.rect(cx_pos, y + 3, 2, height.saturating_sub(6), palette.accent);
            }
        }
        self.hits.push(Hit {
            x,
            y,
            w: width,
            h: height,
            action: Action::DevToolsField(kind),
        });
    }

    #[cfg(feature = "chrome")]
    fn paint_compact_lines(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) {
        let palette = self.effective_theme().palette();
        let lines = self.wrap_lines(&self.inspector_lines(), width.max(80) as f32);
        let rows = (height / 20).max(1) as usize;
        self.devtools.row = self.devtools.row.min(lines.len().saturating_sub(rows));
        for (index, line) in lines.iter().skip(self.devtools.row).take(rows).enumerate() {
            self.inspector_text(
                canvas,
                x,
                y + index as i32 * 20,
                line,
                width,
                13.,
                palette.ink,
            );
        }
    }

    #[cfg(feature = "chrome")]
    fn footer_summary(&self) -> String {
        if let Some(msg) = &self.devtools.copy_feedback {
            return format!("✓ {msg}");
        }
        match self.devtools.tab {
            Tab::Elements => {
                if self.devtools.pick_mode {
                    "Picker active: hover page & click element · Esc cancel".into()
                } else {
                    "↖ pick · ▸/▾ expand · H hide · Del remove · Styles/Computed/Box".into()
                }
            }
            Tab::Console => {
                let (errs, warns) = self.devtools.console_counts();
                format!("{errs} errors · {warns} warnings · Worker JS REPL ($0, $$, table)")
            }
            Tab::Sources => {
                let count = self.collect_sources().len();
                format!("{count} sources · Ctrl+F search in file")
            }
            Tab::Network => {
                let bytes: usize = self.devtools.network.iter().map(|e| e.bytes).sum();
                let finish = self
                    .devtools
                    .network
                    .iter()
                    .map(|e| e.start_ms + e.duration_ms)
                    .fold(0.0f64, f64::max);
                format!(
                    "{} req · {} · {:.0} ms · click row for Headers/Response",
                    self.devtools.network.len(),
                    format_bytes(bytes),
                    finish
                )
            }
            Tab::Performance => format!(
                "{} frames · {}f/{}s cache hits · 60fps = 16.7ms",
                self.devtools.performance.frames_painted,
                self.devtools.performance.frame_cache_hits,
                self.devtools.performance.style_cache_hits
            ),
            Tab::Application => {
                let cookies = self.session.cookies_snapshot().len();
                format!("{cookies} session cookies (values redacted) · Rustls + Seccomp")
            }
        }
    }

    #[cfg(feature = "chrome")]
    fn inspector_text(
        &mut self,
        canvas: &mut Canvas,
        x: i32,
        y: i32,
        value: &str,
        width: u32,
        size: f32,
        color: u32,
    ) {
        let clean = bounded(value, 1024);
        let text = fit_head(&mut self.fonts, &clean, size, width as f32);
        canvas.text(&mut self.fonts, x, y, &text, size, color);
    }

    #[cfg(feature = "chrome")]
    fn inspector_button(
        &mut self,
        canvas: &mut Canvas,
        area: (i32, i32, u32, u32),
        label: &str,
        action: Action,
        focus_index: Option<usize>,
        active: bool,
    ) {
        let (x, y, width, height) = area;
        let palette = self.effective_theme().palette();
        if focus_index.is_some_and(|idx| self.modal_focus == idx) {
            canvas.rect(x - 1, y - 1, width + 2, height + 2, palette.accent);
        }
        canvas.rect(
            x,
            y,
            width,
            height,
            if active {
                palette.panel
            } else {
                palette.button
            },
        );
        let pad_y = (height.saturating_sub(16) / 2) as i32;
        self.inspector_text(
            canvas,
            x + 6,
            y + pad_y,
            label,
            width.saturating_sub(10),
            13.,
            palette.ink,
        );
        self.hits.push(Hit {
            x,
            y,
            w: width,
            h: height,
            action,
        });
    }

    #[cfg(feature = "chrome")]
    fn pill_button(
        &mut self,
        canvas: &mut Canvas,
        area: (i32, i32, u32, u32),
        label: &str,
        action: Action,
        active: bool,
        _dark: bool,
    ) {
        let (x, y, width, height) = area;
        let palette = self.effective_theme().palette();
        canvas.rect(
            x,
            y,
            width,
            height,
            if active {
                palette.selection
            } else {
                palette.button
            },
        );
        if active {
            canvas.rect(x, y + height as i32 - 2, width, 2, palette.accent);
        }
        self.inspector_text(
            canvas,
            x + 6,
            y + 4,
            label,
            width.saturating_sub(10),
            11.5,
            palette.ink,
        );
        self.hits.push(Hit {
            x,
            y,
            w: width,
            h: height,
            action,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Browser {
        let mut app = test_app();
        app.document = document::parse(
            "<style>body{margin:12px} p{display:block}</style><p id='outer'><span id='selected' class='story'>Inspect this text</span></p><form><input name='q' value='original'><button>Submit</button></form>",
            "https://example.com/",
        );
        app.page_url = "https://example.com/".into();
        app.paint();
        app
    }

    #[test]
    fn diagnostic_storage_is_bounded_deduplicated_and_reset() {
        let mut state = State::default();
        state.record("JS", "broken\n\u{1b}display");
        state.record("JS", "broken\n\u{1b}display");
        assert_eq!(state.console.len(), 1);
        assert_eq!(state.console[0].count, 2);
        assert!(!state.console[0].message.chars().any(char::is_control));
        for index in 0..200 {
            state.record("Resource", &format!("{index}{}", "💥".repeat(1000)));
        }
        assert_eq!(state.console.len(), MAX_MESSAGES);
        assert!(
            state
                .console
                .iter()
                .all(|entry| entry.message.len() <= MAX_MESSAGE_BYTES)
        );
        assert!(state.omitted > 0);
        state.new_navigation();
        assert!(state.console.is_empty() && state.rendering.entries.is_empty());
        assert_eq!(state.omitted, 0);
        assert_eq!(bounded("long", 0), "");
        assert_eq!(bounded("éé", 4), "...");
    }

    #[test]
    fn chrome_disabled_inspection_preserves_embedding() {
        let mut app = fixture();
        app.set_chrome(false);
        let before = app.paint().pixels;
        app.context_menu_at(20, 30);
        app.toggle_devtools();
        app.handle_key(Key::DeveloperTools, false, false, false);
        app.handle_key(Key::Character('I'), true, true, false);
        assert!(!app.devtools.visible());
        assert_eq!(before, app.paint().pixels);
    }

    #[test]
    fn host_input_cancel_preserves_focus_and_document() {
        let mut app = fixture();
        app.focus = Focus::Input(7);
        app.pointer_down(20, 100);
        app.content_height = 3000;
        app.wheel_scroll_by(100);
        app.cancel_pointer_input();
        assert!(app.pointer_press.is_none() && app.smooth_scroll.is_none());
        assert!(app.focus == Focus::Input(7));
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn right_click_selects_painted_element_and_docks_side_panel() {
        let mut app = fixture();
        let full_width = app.width;
        let id = app
            .document
            .nodes
            .iter()
            .position(|node| node.attr("id") == Some("selected"))
            .unwrap();
        let rect = app
            .boxes
            .iter()
            .find(|rect| rect.node == id || app.document.nodes[rect.node].parent == id)
            .cloned()
            .unwrap();
        app.context_menu_at(rect.x + 1, rect.y + 1);
        assert_eq!(app.devtools.selected, Some((app.dom_epoch, id)));
        assert!(app.popup_modal_open());
        assert!(app.hits.is_empty());
        app.handle_key(Key::DeveloperTools, false, false, false);
        assert!(app.devtools.open && app.devtools.context.is_none());
        assert_eq!(app.devtools.selected, Some((app.dom_epoch, id)));
        app.handle_key(Key::Escape, false, false, false);
        app.paint();
        app.context_menu_at(rect.x + 1, rect.y + 1);
        app.handle_key(Key::Enter, false, false, false);
        assert!(app.devtools.open && app.devtools.context.is_none());
        let side_w = app
            .devtools_side_width()
            .expect("docked on desktop surface");
        assert_eq!(app.page_viewport_width(), full_width - side_w);
        let lines = app.inspector_lines().join("\n");
        assert!(
            lines.contains("<span>")
                && lines.contains("id=\"selected\"")
                && lines.contains("class=\"story\"")
        );
        assert!(
            lines.contains("Ancestors")
                && lines.contains("Inspect this text")
                && lines.contains("Painted union")
        );
        app.paint();
        let (panel_x, _, _, _) = app.devtools_panel_rect();
        // No page hit may extend beneath the docked right-hand DevTools side panel.
        for hit in &app.hits {
            if matches!(
                hit.action,
                Action::Link { .. } | Action::Input(_) | Action::Submit(_)
            ) {
                assert!(hit.x + hit.w as i32 <= panel_x);
            }
        }
        app.handle_key(Key::Escape, false, false, false);
        assert!(!app.devtools.visible());
        assert_eq!(app.page_viewport_width(), full_width);
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn hover_link_shows_bottom_left_status_and_right_click_opens_in_new_tab() {
        let mut app = test_app();
        app.document = document::parse(
            "<style>body{margin:12px}</style><p><a id='story' href='/stories/42'>Read story</a></p>",
            "https://example.com/news/",
        );
        app.page_url = "https://example.com/news/".into();
        let baseline = app.paint();
        assert_eq!(app.hovered_link(), None);
        let link_hit = app
            .hits
            .iter()
            .find(|hit| matches!(hit.action, Action::Link { .. }))
            .cloned()
            .unwrap();
        let bottom_left = |canvas: &Canvas| -> Vec<u32> {
            let mut pixels = Vec::new();
            for y in (canvas.height - 29)..canvas.height {
                let row = y as usize * canvas.width as usize;
                pixels.extend_from_slice(&canvas.pixels[row..row + 260]);
            }
            pixels
        };
        let baseline_footer = bottom_left(&baseline);

        app.pointer_move(link_hit.x + 2, link_hit.y + 2);
        assert_eq!(app.hovered_link(), Some("https://example.com/stories/42"));
        let hovered = app.paint();
        assert_ne!(
            bottom_left(&hovered),
            baseline_footer,
            "hovering a link must render its destination URL in the bottom-left status bar"
        );

        app.pointer_move(10, 10);
        assert_eq!(app.hovered_link(), None);
        let unhovered = app.paint();
        assert_eq!(bottom_left(&unhovered), baseline_footer);

        app.pointer_move(link_hit.x + 2, link_hit.y + 2);
        app.context_menu_at(link_hit.x + 2, link_hit.y + 2);
        assert_eq!(app.hovered_link(), Some("https://example.com/stories/42"));
        let menu_canvas = app.paint();
        assert_ne!(bottom_left(&menu_canvas), baseline_footer);
        assert_eq!(app.hits.len(), 5);
        let open_new_tab_hit = app.hits[0].clone();
        let copy_link_hit = app.hits[1].clone();
        app.pointer_down(copy_link_hit.x + 4, copy_link_hit.y + 4);
        app.pointer_up(copy_link_hit.x + 4, copy_link_hit.y + 4);
        assert!(!app.popup_modal_open());
        assert_eq!(app.clipboard_text(), "https://example.com/stories/42");

        app.context_menu_at(link_hit.x + 2, link_hit.y + 2);
        app.paint();
        app.pointer_down(open_new_tab_hit.x + 4, open_new_tab_hit.y + 4);
        app.pointer_up(open_new_tab_hit.x + 4, open_new_tab_hit.y + 4);
        assert!(!app.popup_modal_open());
        assert_eq!(app.page_url, "https://example.com/news/");
        assert_eq!(
            app.take_new_tab_request().as_deref(),
            Some("https://example.com/stories/42")
        );
        assert_eq!(app.take_new_tab_request(), None);

        app.paint();
        let link_hit = app
            .hits
            .iter()
            .find(|hit| matches!(hit.action, Action::Link { .. }))
            .cloned()
            .unwrap();
        app.pointer_down_with_modifiers(link_hit.x + 2, link_hit.y + 2, true);
        app.pointer_up_with_modifiers(link_hit.x + 2, link_hit.y + 2, true);
        assert!(!app.loading);
        assert_eq!(app.page_url, "https://example.com/news/");
        assert_eq!(
            app.take_new_tab_request().as_deref(),
            Some("https://example.com/stories/42")
        );
        assert_eq!(app.take_new_tab_request(), None);
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn selection_is_invalidated_on_projection_and_side_panel_persists_across_navigation() {
        let mut app = fixture();
        app.toggle_devtools();
        assert!(app.devtools.selected.is_some());
        app.dom_epoch += 1;
        app.document = document::parse("<p>Replacement</p>", "https://example.com/next");
        assert!(app.inspector_lines()[0].contains("document changed"));
        assert_eq!(app.inspected_node_at(20, TOP + 20), None);
        app.paint();
        assert!(app.devtools.selected.is_none() && app.devtools.open);
        app.devtools.context = Some((40, 80));
        app.devtools.new_navigation();
        assert!(app.devtools.context.is_none());
        assert!(app.devtools.open);
        app.devtools.close();
        assert!(!app.devtools.visible());
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn focused_devtools_blocks_page_typing_and_routes_wheel_by_pane() {
        let mut app = fixture();
        app.document = document::parse(
            &format!(
                "<form><input name='q' value='original'></form>{}",
                "<p>Scrollable line</p>".repeat(80)
            ),
            "https://example.com/",
        );
        app.dom_epoch += 1;
        app.paint();
        let input = app
            .document
            .item_nodes
            .iter()
            .copied()
            .find(|node| app.input_item(*node).is_some())
            .unwrap();
        let old_value = app.values.get(&input).cloned();
        app.toggle_devtools();
        app.activate(Action::DevToolsTab(Tab::Console));
        for index in 0..MAX_MESSAGES {
            app.devtools.record("Resource", &format!("Message {index}"));
        }
        app.paint();
        let history = app.history.len();
        let page_scroll = app.scroll;
        app.type_text("must not enter page");
        app.handle_key(Key::Character('r'), true, false, false);
        app.handle_key(Key::Character('l'), true, false, false);
        app.wheel_scroll_by(0);
        assert_eq!(app.devtools.row, 0);
        // Pointer inside DevTools (or unset) scrolls the DevTools side panel.
        app.wheel_scroll_by(100);
        assert_eq!(app.scroll, page_scroll);
        assert_eq!(app.devtools.row, 3);
        // Pointer over the left web page scrolls the page smoothly while docked!
        app.pointer_move(40, TOP + 40);
        app.wheel_scroll_by(120);
        app.advance_scroll(Instant::now() + Duration::from_millis(200));
        assert!(app.scroll > 0);
        assert_eq!(app.devtools.row, 3);
        assert_eq!(app.values.get(&input).cloned(), old_value);
        assert_eq!(app.history.len(), history);
        app.handle_key(Key::DeveloperTools, false, false, false);
        assert!(!app.devtools.visible());
        app.handle_key(Key::Character('I'), true, true, false);
        assert!(app.devtools.open);
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn actual_render_diagnostics_survive_repaint_without_accumulating() {
        let mut app = fixture();
        app.document = document::parse(
            "<style>div{display:flow-root}</style><div>Text</div>",
            "https://example.com/",
        );
        app.paint();
        let report = app.devtools.rendering.clone();
        assert!(
            report
                .entries
                .iter()
                .any(|entry| entry.kind == "css-unsupported")
        );
        app.paint();
        assert_eq!(app.devtools.rendering, report);
        app.toggle_devtools();
        app.activate(Action::DevToolsTab(Tab::Console));
        assert!(
            app.inspector_lines()
                .iter()
                .any(|line| line.contains("css-unsupported"))
        );
        app.document = document::parse("<p>Plain document</p>", "https://example.com/");
        app.dom_epoch += 1;
        app.paint();
        assert!(app.devtools.rendering.entries.is_empty());
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn script_errors_worker_failures_and_network_timings_populate_console_and_network_tabs() {
        let mut app = fixture();
        let reply = SessionReply {
            revision: 0,
            snapshot: None,
            outcome: Default::default(),
            default_action: DefaultAction::None,
            navigation: None,
            errors: vec!["TypeError: cannot read property of undefined".into()],
            scripts_executed: 1,
            allocations: None,
            boa: None,
            state: RealmState::Closed,
            acknowledgements: vec![],
            task_wake: None,
            lifecycle: None,
            observer: None,
        };
        app.script_summary(&reply);
        app.block_session("Session expired");
        assert!(
            app.devtools
                .console
                .iter()
                .any(|entry| entry.level == ConsoleLevel::Error
                    && entry.message.contains("TypeError"))
        );
        assert!(
            app.devtools
                .console
                .iter()
                .any(|entry| entry.message.contains("Session expired"))
        );
        app.devtools.finish_network_load(
            "https://example.com/",
            Some(200),
            "text/html",
            4096,
            14.2,
            2.1,
            None,
            &[
                resources::ResourceTiming {
                    kind: "stylesheet",
                    url: "https://example.com/site.css".into(),
                    status: Some(200),
                    content_type: "text/css".into(),
                    bytes: 1024,
                    start_ms: 0.4,
                    duration_ms: 5.0,
                    error: None,
                },
                resources::ResourceTiming {
                    kind: "font",
                    url: "https://example.com/Inter.woff2".into(),
                    status: Some(200),
                    content_type: "font/woff2".into(),
                    bytes: 18432,
                    start_ms: 5.8,
                    duration_ms: 9.3,
                    error: None,
                },
            ],
            15.5,
            &[resources::ResourceTiming {
                kind: "script",
                url: "https://example.com/app.js".into(),
                status: Some(404),
                content_type: "text/plain".into(),
                bytes: 64,
                start_ms: 0.2,
                duration_ms: 4.1,
                error: Some("HTTP 404".into()),
            }],
        );
        assert_eq!(app.devtools.network.len(), 4);
        app.handle_key(Key::Character('E'), true, true, false);
        assert!(app.devtools.open && app.devtools.tab == Tab::Network);
        app.activate(Action::DevToolsNetFilter(NetFilter::Font));
        assert_eq!(app.filtered_network_indices(), vec![2]);
        app.activate(Action::DevToolsNetFilter(NetFilter::All));
        app.activate(Action::DevToolsNetSelect(Some(3)));
        assert_eq!(app.devtools.selected_request, Some(3));
        app.paint();
        app.devtools.new_navigation();
        assert!(app.devtools.console.is_empty() && app.devtools.network.is_empty());
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn compact_scaled_panels_keep_controls_inside_the_surface_and_wrap_text() {
        for scheme in [ColorScheme::Light, ColorScheme::Dark] {
            for scale in [100, 200] {
                let mut app = fixture();
                app.set_theme_preference(match scheme {
                    ColorScheme::Light => ThemePreference::Light,
                    ColorScheme::Dark => ThemePreference::Dark,
                });
                app.set_scale_preference(ScalePreference::Percent(scale));
                app.resize_surface(400 * u32::from(scale) / 100, 240 * u32::from(scale) / 100);
                app.toggle_devtools();
                app.activate(Action::DevToolsTab(Tab::Console));
                app.devtools.record(
                    "Resource",
                    &format!("{}: HTTP 403", "https://example.com/long/".repeat(18)),
                );
                let canvas = app.paint();
                assert_eq!(canvas.width, 400 * u32::from(scale) / 100);
                assert!(app.devtools_side_width().is_none());
                assert!(app.hits.iter().all(|hit| hit.x >= 0
                    && hit.y >= 0
                    && hit.x + hit.w as i32 <= app.width as i32
                    && hit.y + hit.h as i32 <= app.height as i32));
                let rows = app.inspector_display_lines();
                assert!(rows.iter().all(|row| app.fonts.width(row, 13.) <= 360.));
                assert!(rows.len() > app.inspector_lines().len());
                // Compact overlay covers the page: no page hits survive.
                assert!(app.hits.iter().all(|hit| app.modal_action(&hit.action)));
            }
        }
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn performance_dashboard_and_render_caches_track_frame_and_style_hits() {
        let mut app = test_app();
        app.document = document::parse(
            &format!(
                "<style>p{{color:#123456;margin:8px}}</style>{}",
                "<p><a href='/story'>Story link</a></p>".repeat(40)
            ),
            "https://example.com/",
        );
        app.page_url = "https://example.com/".into();
        app.dom_epoch += 1;

        app.paint();
        assert!(!app.performance_profile().last_frame.frame_cached);
        assert!(!app.performance_profile().last_frame.style_cached);
        assert_eq!(app.performance_profile().frame_cache_hits, 0);
        assert_eq!(app.performance_profile().style_cache_hits, 0);

        // UI-only redraw (such as hover or chrome update) hits the frame cache.
        app.request_redraw();
        app.paint();
        assert!(app.performance_profile().last_frame.frame_cached);
        assert_eq!(app.performance_profile().frame_cache_hits, 1);

        // Scroll redraw misses the frame cache but hits the Stylo style cache.
        app.scroll_by(80);
        app.paint();
        assert!(!app.performance_profile().last_frame.frame_cached);
        assert!(app.performance_profile().last_frame.style_cached);
        assert_eq!(app.performance_profile().style_cache_hits, 1);

        // Ctrl+Shift+P opens the Performance Dashboard tab directly.
        app.handle_key(Key::Character('P'), true, true, false);
        assert!(app.devtools.open && app.devtools.tab == Tab::Performance);
        let report = app.inspector_lines().join("\n");
        assert!(report.contains("Performance Dashboard"));
        assert!(report.contains("Page Load Breakdown"));
        assert!(report.contains("Render & Frame Cache Pipeline"));
        assert!(report.contains("Frame cache hits: 1 | Style cache hits: 1"));
        app.paint();
        app.handle_key(Key::Character('P'), true, true, false);
        assert!(!app.devtools.visible());
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn frame_cache_shares_pixels_and_matches_uncached_pixels_hits_and_geometry() {
        let mut app = fixture();
        let first = app.paint();
        let snapshot = Rc::clone(&app.frame_cache.as_ref().unwrap().1);
        let hits = |app: &Browser| {
            app.hits
                .iter()
                .map(|h| (h.x, h.y, h.w, h.h, format!("{:?}", h.action)))
                .collect::<Vec<_>>()
        };
        let expected_hits = hits(&app);
        let expected_boxes = app.boxes.clone();
        let cached = app.paint();
        assert!(app.performance_profile().last_frame.frame_cached);
        assert!(Rc::ptr_eq(&snapshot, &app.frame_cache.as_ref().unwrap().1));
        assert_eq!(
            snapshot.canvas.pixels.as_ptr(),
            app.frame_cache.as_ref().unwrap().1.canvas.pixels.as_ptr()
        );
        assert_eq!(first.pixels, cached.pixels);
        assert_eq!(hits(&app), expected_hits);
        assert_eq!(app.boxes, expected_boxes);
        let timing = &app.performance_profile().last_frame;
        assert!(timing.composition_ms > 0. && timing.chrome_ms > 0.);
        assert!(timing.total_ms >= timing.composition_ms + timing.chrome_ms);
        app.frame_cache = None;
        app.style_cache.clear();
        let fresh = app.paint();
        assert!(!app.performance_profile().last_frame.frame_cached);
        assert_eq!(fresh.pixels, cached.pixels);
        assert_eq!(hits(&app), expected_hits);
        assert_eq!(app.boxes, expected_boxes);
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn frame_cache_invalidates_for_controls_projection_resize_scale_and_short_scroll() {
        let mut app = fixture();
        let original = app.paint();
        app.scroll_by(120);
        assert_eq!(app.scroll_position(), 0, "fixture fits the viewport");
        app.paint();
        assert_eq!(
            app.performance_profile().last_frame.cache_label(),
            "frame cache hit"
        );
        let input = app.document.query_selector(0, "input").unwrap().unwrap();
        app.focus = Focus::Input(input);
        app.select_all = true;
        app.type_text("new value");
        let edited = app.paint();
        assert!(!app.performance_profile().last_frame.frame_cached);
        assert!(app.performance_profile().last_frame.style_cached);
        assert_ne!(edited.pixels, original.pixels);
        app.paint();
        assert!(app.performance_profile().last_frame.frame_cached);
        app.acknowledge_projection(
            document::parse(
                "<style>p{color:red}</style><p>Changed by event projection</p>",
                "https://example.com/",
            ),
            &[],
        );
        app.dom_epoch += 1;
        let projected = app.paint();
        assert!(!app.performance_profile().last_frame.frame_cached);
        assert!(!app.performance_profile().last_frame.style_cached);
        assert_ne!(projected.pixels, edited.pixels);
        app.resize(640, 480);
        app.paint();
        assert!(!app.performance_profile().last_frame.frame_cached);
        assert!(!app.performance_profile().last_frame.style_cached);
        app.set_scale_preference(ScalePreference::Percent(200));
        let scaled = app.paint();
        assert!(!app.performance_profile().last_frame.frame_cached);
        assert_eq!(scaled.width, 1280);
        app.set_chrome(false);
        app.paint();
        assert!(app.frame_cache.is_none());
        app.paint();
        assert!(!app.performance_profile().last_frame.frame_cached);
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn frame_cache_bypasses_oversized_surfaces_without_changing_output() {
        let mut app = test_app();
        app.document =
            document::parse("<p>Large surface still renders</p>", "https://example.com/");
        app.dom_epoch += 1;
        app.resize_surface(3000, 1800);
        let first = app.paint();
        assert!(first.pixels.len() * 4 > FRAME_CACHE_PAYLOAD_BYTES);
        assert!(app.frame_cache.is_none());
        assert_eq!(app.performance_profile().frame_cache_payload_bytes, 0);
        let second = app.paint();
        assert!(!app.performance_profile().last_frame.frame_cached);
        assert_eq!(first.pixels, second.pixels);
    }

    #[test]
    fn failed_navigation_preserves_measured_fetch_time_in_performance_report() {
        let mut app = test_app();
        app.generation = 1;
        app.loading = true;
        app.inflight = 1;
        app.tx
            .send(Loaded {
                generation: 1,
                document: None,
                result: Err("fixture connection failure".into()),
                script: None,
                fetch_html_ms: 42.5,
                parse_html_ms: 0.,
                resource_profile: Default::default(),
                script_fetch_profile: Default::default(),
                script_worker_ms: 0.,
            })
            .unwrap();
        app.poll();
        assert!(!app.last_load_succeeded());
        assert_eq!(app.performance_profile().fetch_html_ms, 42.5);
        assert!(app.performance_report().contains("HTML fetch: 42.5 ms"));
        assert!(app.performance_report().contains("load failure"));
    }

    #[test]
    fn performance_reports_requested_worker_allocation_with_explicit_availability() {
        let mut app = test_app();
        assert!(app.performance_report().contains("allocation: unavailable"));
        let mut report = mg_butane::modern::Engine::new().unwrap().stats();
        report.worker_memory = Some(mg_butane::modern::WorkerMemory {
            outstanding_bytes: 1024,
            peak_bytes: 2048,
            cumulative_bytes: 4096,
            allocations: 20,
            outstanding_limit_bytes: mg_butane::modern::WORKER_OUTSTANDING_LIMIT,
            cumulative_limit_bytes: mg_butane::modern::WORKER_CUMULATIVE_LIMIT,
        });
        let reply = SessionReply {
            revision: 0,
            snapshot: None,
            outcome: Default::default(),
            default_action: DefaultAction::None,
            navigation: None,
            errors: vec![],
            scripts_executed: 1,
            allocations: None,
            boa: Some(report),
            state: RealmState::Ready,
            acknowledgements: vec![],
            task_wake: None,
            lifecycle: None,
            observer: None,
        };
        app.script_summary(&reply);
        assert!(app.performance_profile().worker_allocation_recorded);
        assert_eq!(app.performance_profile().boa_alloc_bytes, 4096);
        assert_eq!(app.performance_profile().boa_peak_bytes, 2048);
        assert!(
            app.performance_report()
                .contains("4 KiB cumulative, 2 KiB peak outstanding (not GC heap/RSS)")
        );
    }

    #[cfg(feature = "chrome")]
    enum TestBoaMsg {
        Dispatch(SessionInput),
        Pump(TaskPump),
        Console(ConsoleCommand),
    }

    #[cfg(feature = "chrome")]
    struct TestBoaSession {
        generation: u64,
        session_id: u64,
        tx: std::sync::mpsc::Sender<TestBoaMsg>,
        rx: std::sync::mpsc::Receiver<(
            SessionReply,
            Vec<mg_sparkle::page_session::console::ConsoleWireEntry>,
            Option<ConsoleResult>,
        )>,
        next_seq: u64,
        pending: Option<scripts::SessionUpdate>,
        wire: Vec<mg_sparkle::page_session::console::ConsoleWireEntry>,
        result: Option<ConsoleResult>,
    }

    #[cfg(feature = "chrome")]
    impl scripts::ScriptSession for TestBoaSession {
        fn id(&self) -> u64 {
            self.session_id
        }
        fn try_dispatch(&mut self, input: SessionInput, _: u64) -> Result<u64, String> {
            self.next_seq += 1;
            self.tx
                .send(TestBoaMsg::Dispatch(input))
                .map_err(|e| e.to_string())?;
            let (reply, w, r) = self.rx.recv().map_err(|e| e.to_string())?;
            self.wire.extend(w);
            self.result = r;
            self.pending = Some(scripts::SessionUpdate {
                generation: self.generation,
                session_id: self.session_id,
                sequence: Some(self.next_seq),
                reply: Ok(reply),
                automation: None,
                continuation: scripts::SessionContinuation::Live,
            });
            Ok(self.next_seq)
        }
        fn try_task_pump(&mut self, pump: TaskPump, _: u64) -> Result<u64, String> {
            self.next_seq += 1;
            self.tx
                .send(TestBoaMsg::Pump(pump))
                .map_err(|e| e.to_string())?;
            let (reply, w, r) = self.rx.recv().map_err(|e| e.to_string())?;
            self.wire.extend(w);
            self.result = r;
            self.pending = Some(scripts::SessionUpdate {
                generation: self.generation,
                session_id: self.session_id,
                sequence: Some(self.next_seq),
                reply: Ok(reply),
                automation: None,
                continuation: scripts::SessionContinuation::Live,
            });
            Ok(self.next_seq)
        }
        fn try_console(&mut self, command: ConsoleCommand, _: u64) -> Result<u64, String> {
            self.next_seq += 1;
            self.tx
                .send(TestBoaMsg::Console(command))
                .map_err(|e| e.to_string())?;
            let (reply, w, r) = self.rx.recv().map_err(|e| e.to_string())?;
            self.wire.extend(w);
            self.result = r;
            self.pending = Some(scripts::SessionUpdate {
                generation: self.generation,
                session_id: self.session_id,
                sequence: Some(self.next_seq),
                reply: Ok(reply),
                automation: None,
                continuation: scripts::SessionContinuation::Live,
            });
            Ok(self.next_seq)
        }
        fn take_console(
            &mut self,
        ) -> (
            Vec<mg_sparkle::page_session::console::ConsoleWireEntry>,
            Option<ConsoleResult>,
        ) {
            (std::mem::take(&mut self.wire), self.result.take())
        }
        fn try_recv(&mut self) -> Option<scripts::SessionUpdate> {
            self.pending.take()
        }
        fn cancel(&self) {}
        fn charge_active(&mut self, _: Duration) -> Result<(), String> {
            Ok(())
        }
    }

    #[cfg(feature = "chrome")]
    struct TestBoaRuntime(std::sync::atomic::AtomicU64);

    #[cfg(feature = "chrome")]
    impl scripts::ScriptRuntime for TestBoaRuntime {
        fn set_generation(&self, _: u64) -> Result<(), String> {
            Ok(())
        }
        fn start(
            &self,
            request: mg_sparkle::js_browser::Request,
            generation: u64,
        ) -> Result<(Box<dyn scripts::ScriptSession>, SessionReply), String> {
            let (tx_cmd, rx_cmd) = std::sync::mpsc::channel::<TestBoaMsg>();
            let (tx_out, rx_out) = std::sync::mpsc::channel();
            let (tx_init, rx_init) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let (Some(mut realm), reply) =
                    mg_sparkle::js_browser::boa::BoaPageRealm::start(request)
                else {
                    let _ = tx_init.send(Err("Failed to initialize test BoaPageRealm".to_owned()));
                    return;
                };
                let wire = realm.take_console_entries();
                if tx_init.send(Ok((reply, wire))).is_err() {
                    return;
                }
                while let Ok(msg) = rx_cmd.recv() {
                    let out = match msg {
                        TestBoaMsg::Dispatch(input) => {
                            let rep = realm.dispatch(input);
                            (rep, realm.take_console_entries(), None)
                        }
                        TestBoaMsg::Pump(pump) => {
                            let rep = realm.pump_tasks(pump);
                            (rep, realm.take_console_entries(), None)
                        }
                        TestBoaMsg::Console(cmd) => {
                            let (rep, res) = realm.console_command(cmd);
                            (rep, realm.take_console_entries(), Some(res))
                        }
                    };
                    if tx_out.send(out).is_err() {
                        break;
                    }
                }
            });
            let (reply, wire) = rx_init.recv().map_err(|e| e.to_string())??;
            let session_id = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            Ok((
                Box::new(TestBoaSession {
                    generation,
                    session_id,
                    tx: tx_cmd,
                    rx: rx_out,
                    next_seq: 0,
                    pending: None,
                    wire,
                    result: None,
                }),
                reply,
            ))
        }
        fn close(&self) {}
        fn wait_idle(&self) -> Result<(), String> {
            Ok(())
        }
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn six_tabs_dock_bottom_splitter_drag_and_element_picker_work_end_to_end() {
        let mut app = fixture();
        app.resize(1280, 860);
        app.paint();

        // Ctrl+Shift+C toggles interactive Element Picker and opens Elements tab.
        app.handle_key(Key::Character('C'), true, true, false);
        assert!(app.devtools.open && app.devtools.pick_mode && app.devtools.tab == Tab::Elements);
        app.paint();

        // Hovering and clicking an element in Picker mode selects it ($0) without following links.
        let span_id = app
            .document
            .query_selector(0, "#selected")
            .unwrap()
            .unwrap();
        let (bx, by, _, _) = app.node_painted_bounds(app.dom_epoch, span_id).unwrap();
        let lx = bx as i32 + 2;
        let ly = by as i32 + TOP + 2;
        app.pointer_move(lx, ly);
        assert_eq!(
            app.devtools.hovered_inspect_node,
            Some((app.dom_epoch, span_id))
        );
        app.paint();
        app.pointer_down(lx, ly);
        app.pointer_up(lx, ly);
        assert!(!app.devtools.pick_mode);
        assert_eq!(app.devtools.selected, Some((app.dom_epoch, span_id)));
        assert_eq!(
            app.devtools
                .inspected_nodes_for(app.dom_epoch)
                .first()
                .copied(),
            Some(span_id)
        );

        // Dragging the right-dock splitter resizes the DevTools side panel width.
        app.paint();
        let initial_side = app.devtools_side_width().unwrap();
        let split_x = (app.width - initial_side) as i32;
        app.pointer_down(split_x, TOP + 120);
        assert!(app.devtools.split_dragging);
        app.pointer_move(split_x - 80, TOP + 120);
        app.pointer_up(split_x - 80, TOP + 120);
        assert!(app.devtools_side_width().unwrap() > initial_side);

        // Toggle Dock Bottom (`⬓` -> `◨`) and drag horizontal splitter.
        app.activate(Action::DevToolsToggleDock);
        assert_eq!(app.devtools.dock, DockSide::Bottom);
        app.paint();
        assert!(app.devtools_side_width().is_none());
        let initial_bottom = app.devtools_bottom_height().unwrap();
        let (_, panel_y, _, _) = app.devtools_panel_rect();
        app.pointer_down(300, panel_y);
        assert!(app.devtools.split_dragging);
        app.pointer_move(300, panel_y - 60);
        app.pointer_up(300, panel_y - 60);
        assert!(app.devtools_bottom_height().unwrap() > initial_bottom);

        // Render all 6 tabs cleanly in both Bottom and Right dock modes.
        for dock in [DockSide::Bottom, DockSide::Right] {
            app.devtools.dock = dock;
            for tab in [
                Tab::Elements,
                Tab::Console,
                Tab::Sources,
                Tab::Network,
                Tab::Performance,
                Tab::Application,
            ] {
                app.activate(Action::DevToolsTab(tab));
                let frame = app.paint();
                assert_eq!(frame.width, 1280);
                assert_eq!(frame.height, 860);
            }
        }
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn elements_inline_style_edit_hide_delete_computed_and_box_model_update_live() {
        let mut app = fixture();
        app.resize(1024, 720);
        app.toggle_devtools();
        let span_id = app
            .document
            .query_selector(0, "#selected")
            .unwrap()
            .unwrap();
        app.activate(Action::DevToolsSelectNode(span_id));
        let before = app.paint().pixels;

        // Live inline style edit mutates DOM, invalidates style & frame caches, and repaints.
        app.activate(Action::DevToolsField(FieldKind::InlineStyle));
        app.type_text("color: #ff0000; padding: 10px");
        app.handle_key(Key::Enter, false, false, false);
        assert_eq!(
            app.document.nodes[span_id].attr("style"),
            Some("color: #ff0000; padding: 10px")
        );
        let restyled = app.paint().pixels;
        assert_ne!(restyled, before);

        // Computed and BoxModel sub-tabs render Stylo computed values cleanly.
        app.activate(Action::DevToolsElementsSubTab(ElementsSubTab::Computed));
        assert!(!app.computed_style_pairs(span_id).is_empty());
        app.paint();
        app.activate(Action::DevToolsElementsSubTab(ElementsSubTab::BoxModel));
        app.paint();

        // Pressing 'H' hides and un-hides the selected node; Delete removes it.
        app.focus = Focus::DevTools;
        app.handle_key(Key::Character('h'), false, false, false);
        assert!(
            app.document.nodes[span_id]
                .attr("style")
                .unwrap_or("")
                .contains("display: none")
        );
        app.handle_key(Key::Character('h'), false, false, false);
        assert!(
            !app.document.nodes[span_id]
                .attr("style")
                .unwrap_or("")
                .contains("display: none")
        );
        let outer_id = app.document.query_selector(0, "#outer").unwrap().unwrap();
        app.activate(Action::DevToolsSelectNode(span_id));
        app.handle_key(Key::Delete, false, false, false);
        assert!(!app.document.nodes[outer_id].children.contains(&span_id));
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn console_repl_spawns_isolated_session_on_static_page_and_runs_command_line_api() {
        let path = std::env::var("MGBROWSER_FONT")
            .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
        let mut app = Browser::new(
            Fonts::from_bytes(std::fs::read(path).unwrap()).unwrap(),
            Arc::new(TestBoaRuntime(std::sync::atomic::AtomicU64::new(0))),
        );
        app.resize(1100, 760);
        app.document = document::parse(
            "<main><h1 id='hero'>Hello DevTools</h1><p class='item'>One</p><p class='item'>Two</p></main>",
            "https://example.com/",
        );
        app.page_url = "https://example.com/".into();
        app.devtools.page_source =
            "<main><h1 id='hero'>Hello DevTools</h1><p class='item'>One</p><p class='item'>Two</p></main>"
                .into();
        app.dom_epoch += 1;
        app.paint();

        let hero_id = app.document.query_selector(0, "#hero").unwrap().unwrap();
        app.toggle_devtools();
        app.activate(Action::DevToolsSelectNode(hero_id));
        app.activate(Action::DevToolsTab(Tab::Console));
        app.activate(Action::DevToolsConsoleInput);

        let pump = |app: &mut Browser| {
            for _ in 0..8 {
                app.poll();
                app.poll_task_wake(Instant::now() + Duration::from_millis(5));
            }
        };

        // Evaluate $0.textContent + $$('.item').length on a static page (spawns worker on demand!)
        app.type_text("$0.textContent + ':' + $$('.item').length");
        app.handle_key(Key::Enter, false, false, false);
        pump(&mut app);
        assert!(
            app.devtools
                .console
                .iter()
                .any(|e| e.message.contains("Hello DevTools:2")),
            "console entries: {:?}",
            app.devtools
                .console
                .iter()
                .map(|e| &e.message)
                .collect::<Vec<_>>()
        );

        // copy(...) populates console_clipboard(); inspect($('p.item')) jumps to Elements!
        app.activate(Action::DevToolsConsoleInput);
        app.type_text("copy({ok: 42}); inspect($('p.item'))");
        app.handle_key(Key::Enter, false, false, false);
        pump(&mut app);
        assert!(
            app.console_clipboard().contains("\"ok\":42"),
            "clipboard: {:?}",
            app.console_clipboard()
        );
        assert_eq!(app.devtools.tab, Tab::Elements);
        let first_item = app.document.query_selector(0, "p.item").unwrap().unwrap();
        assert_eq!(app.devtools.selected, Some((app.dom_epoch, first_item)));
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn sources_network_redaction_drawer_performance_sparkline_and_application_work() {
        let mut app = fixture();
        app.resize(1100, 760);
        app.devtools.page_source =
            "<!doctype html>\n<style>p { color: green; }</style>\n<script>console.log('hi');</script>\n<p>Body</p>"
                .into();
        app.document = document::parse(&app.devtools.page_source, "https://example.com/");
        app.dom_epoch += 1;
        app.toggle_devtools();

        // Sources navigator collects HTML + inline CSS + inline JS and searches lines.
        app.activate(Action::DevToolsTab(Tab::Sources));
        let sources = app.collect_sources();
        assert!(sources.len() >= 3);
        app.activate(Action::DevToolsField(FieldKind::SourcesSearch));
        app.type_text("console.log");
        app.handle_key(Key::Enter, false, false, false);
        app.paint();

        // Network 4-tab drawer + Preserve log + NoCache toggle.
        app.devtools.network.push(NetworkEntry {
            kind: NetKind::Doc,
            method: "GET".into(),
            url: "https://example.com/".into(),
            status: Some(200),
            mime: "text/html".into(),
            bytes: 128,
            start_ms: 0.0,
            duration_ms: 11.4,
            error: None,
            pending: false,
            request_headers: vec![("cookie".into(), "<redacted>".into())],
            response_headers: vec![("content-type".into(), "text/html".into())],
            body_preview: Some("<!doctype html><p>Body</p>".into()),
        });
        app.activate(Action::DevToolsTab(Tab::Network));
        app.activate(Action::DevToolsNetDisableCache);
        assert!(app.session.cache_disabled());
        app.activate(Action::DevToolsNetPreserve);
        assert!(app.devtools.net_preserve_log);
        app.activate(Action::DevToolsNetSelect(Some(0)));
        for sub in [
            NetworkSubTab::Headers,
            NetworkSubTab::Response,
            NetworkSubTab::Preview,
            NetworkSubTab::Timing,
        ] {
            app.activate(Action::DevToolsNetSubTab(sub));
            app.paint();
        }
        app.devtools.new_navigation_to("https://example.com/next");
        assert_eq!(app.devtools.network.len(), 1, "Preserve log retained entry");

        // Performance records frame history sparkline samples; Application renders all 3 sections.
        app.activate(Action::DevToolsTab(Tab::Performance));
        app.paint();
        assert!(!app.devtools.frame_history_ms.is_empty());
        app.activate(Action::DevToolsTab(Tab::Application));
        for sec in [
            AppSection::Cookies,
            AppSection::Security,
            AppSection::Resources,
        ] {
            app.activate(Action::DevToolsAppSection(sec));
            app.paint();
        }
    }
}
