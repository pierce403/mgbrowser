//! App-wide text selection, cut/copy/paste editing, and host clipboard bridge.
use super::*;

/// Maximum UTF-8 payload retained or exported to the host clipboard (256 KiB).
pub const MAX_CLIPBOARD_BYTES: usize = 256 * 1024;

/// Outbound clipboard ownership changes for the platform windowing host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardUpdate {
    pub clipboard: Option<String>,
    pub primary: Option<String>,
}

/// Which platform selection buffer a paste operation reads from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteSource {
    Clipboard,
    Primary,
}

/// Inbound paste request for the platform windowing host to fulfill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasteRequest {
    pub source: PasteSource,
    pub and_go: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DocTextRun {
    pub node: usize,
    pub x: i32,
    pub doc_y: i32,
    pub width: u32,
    pub height: u32,
    pub font_size: f32,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TextPos {
    pub run: usize,
    pub byte: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageSelection {
    pub epoch: u64,
    pub anchor: TextPos,
    pub focus: TextPos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DragTarget {
    Page {
        start_x: i32,
        start_y: i32,
        moved: bool,
    },
    Address {
        start_byte: usize,
        first_focus: bool,
        moved: bool,
    },
    PageInput {
        node: usize,
        start_x: i32,
        start_byte: usize,
        moved: bool,
    },
    ConsoleLog {
        anchor_row: usize,
    },
    ConsoleRepl {
        start_byte: usize,
    },
    DevToolsField {
        kind: devtools::FieldKind,
        start_byte: usize,
    },
    SourcesCode {
        anchor_line: usize,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ContextKind {
    #[default]
    Page,
    AddressBar,
    PageInput(usize),
    ConsoleLog,
    ConsoleRepl,
    DevToolsField(devtools::FieldKind),
    Elements,
    Sources,
    Network,
    Performance,
    Application,
}

#[derive(Default)]
pub(crate) struct SelectionState {
    pub host_clipboard_enabled: bool,
    pub clipboard_buffer: String,
    pub primary_buffer: String,
    pub pending_update: ClipboardUpdate,
    pub pending_paste: Option<PasteRequest>,
    pub address_cursor: usize,
    pub address_anchor: Option<usize>,
    pub input_node: Option<usize>,
    pub input_cursor: usize,
    pub input_anchor: Option<usize>,
    pub page_runs: Vec<DocTextRun>,
    pub page_sel: Option<PageSelection>,
    pub active_drag: Option<DragTarget>,
    pub last_click: Option<(Instant, i32, i32)>,
}

pub(crate) fn clamp_utf8(s: &str, pos: usize) -> usize {
    let mut p = pos.min(s.len());
    while p > 0 && !s.is_char_boundary(p) {
        p -= 1;
    }
    p
}

pub(crate) fn bounded_clipboard(text: &str) -> String {
    if text.len() <= MAX_CLIPBOARD_BYTES {
        text.to_owned()
    } else {
        text[..clamp_utf8(text, MAX_CLIPBOARD_BYTES)].to_owned()
    }
}

pub(crate) fn prev_char_boundary(s: &str, pos: usize) -> usize {
    let p = clamp_utf8(s, pos);
    s[..p].char_indices().next_back().map_or(0, |(idx, _)| idx)
}

pub(crate) fn next_char_boundary(s: &str, pos: usize) -> usize {
    let p = clamp_utf8(s, pos);
    s[p..].chars().next().map_or(p, |ch| p + ch.len_utf8())
}

pub(crate) fn prev_word_boundary(s: &str, pos: usize) -> usize {
    let p = clamp_utf8(s, pos);
    let mut chars: Vec<(usize, char)> = s[..p].char_indices().collect();
    while let Some((_, ch)) = chars.last() {
        if ch.is_alphanumeric() || *ch == '_' {
            break;
        }
        chars.pop();
    }
    while let Some((_, ch)) = chars.last() {
        if !(ch.is_alphanumeric() || *ch == '_') {
            break;
        }
        chars.pop();
    }
    chars.last().map_or(0, |(idx, ch)| idx + ch.len_utf8())
}

pub(crate) fn next_word_boundary(s: &str, pos: usize) -> usize {
    let p = clamp_utf8(s, pos);
    let mut iter = s[p..].char_indices().peekable();
    while let Some(&(_, ch)) = iter.peek() {
        if ch.is_alphanumeric() || ch == '_' {
            break;
        }
        iter.next();
    }
    while let Some(&(_, ch)) = iter.peek() {
        if !(ch.is_alphanumeric() || ch == '_') {
            break;
        }
        iter.next();
    }
    iter.peek().map_or(s.len(), |&(idx, _)| p + idx)
}

pub(crate) fn normalized_range(
    s: &str,
    cursor: usize,
    anchor: Option<usize>,
) -> Option<(usize, usize)> {
    let c = clamp_utf8(s, cursor);
    let a = clamp_utf8(s, anchor?);
    (a != c).then_some((a.min(c), a.max(c)))
}

pub(crate) fn edit_move_cursor(
    s: &str,
    cursor: &mut usize,
    anchor: &mut Option<usize>,
    target: usize,
    shift: bool,
) {
    let cur = clamp_utf8(s, *cursor);
    let dst = clamp_utf8(s, target);
    if shift {
        let anc = anchor.map_or(cur, |a| clamp_utf8(s, a));
        *cursor = dst;
        *anchor = (anc != dst).then_some(anc);
    } else {
        *cursor = dst;
        *anchor = None;
    }
}

pub(crate) fn edit_replace_selection_or_insert(
    s: &mut String,
    cursor: &mut usize,
    anchor: &mut Option<usize>,
    insert: &str,
    max_bytes: usize,
) -> bool {
    let cur = clamp_utf8(s, *cursor);
    let (r0, r1) = normalized_range(s, cur, *anchor).unwrap_or((cur, cur));
    let retained = s.len().saturating_sub(r1 - r0);
    if retained >= max_bytes && !insert.is_empty() {
        return false;
    }
    let avail = max_bytes.saturating_sub(retained);
    let piece = if insert.len() <= avail {
        insert
    } else {
        &insert[..clamp_utf8(insert, avail)]
    };
    if piece.is_empty() && r0 == r1 {
        return false;
    }
    s.replace_range(r0..r1, piece);
    *cursor = r0 + piece.len();
    *anchor = None;
    true
}

pub(crate) fn edit_delete_backward(
    s: &mut String,
    cursor: &mut usize,
    anchor: &mut Option<usize>,
    word: bool,
) -> bool {
    let cur = clamp_utf8(s, *cursor);
    if let Some((r0, r1)) = normalized_range(s, cur, *anchor) {
        s.drain(r0..r1);
        *cursor = r0;
        *anchor = None;
        return true;
    }
    if cur == 0 {
        return false;
    }
    let prev = if word {
        prev_word_boundary(s, cur)
    } else {
        prev_char_boundary(s, cur)
    };
    s.drain(prev..cur);
    *cursor = prev;
    *anchor = None;
    true
}

pub(crate) fn edit_delete_forward(
    s: &mut String,
    cursor: &mut usize,
    anchor: &mut Option<usize>,
    word: bool,
) -> bool {
    let cur = clamp_utf8(s, *cursor);
    if let Some((r0, r1)) = normalized_range(s, cur, *anchor) {
        s.drain(r0..r1);
        *cursor = r0;
        *anchor = None;
        return true;
    }
    if cur >= s.len() {
        return false;
    }
    let next = if word {
        next_word_boundary(s, cur)
    } else {
        next_char_boundary(s, cur)
    };
    s.drain(cur..next);
    *anchor = None;
    true
}

/// Map a relative pixel X offset inside a single-line field to a UTF-8 byte index,
/// accounting for horizontal tail-scrolling when the text exceeds `avail_w`.
pub(crate) fn hit_byte_in_line(
    fonts: &mut Fonts,
    text: &str,
    font_size: f32,
    avail_w: f32,
    caret_byte: usize,
    rel_x: f32,
) -> usize {
    if text.is_empty() {
        return 0;
    }
    let chars: Vec<(usize, char)> = text.char_indices().take(4096).collect();
    let len = chars.len();
    let caret_b = clamp_utf8(text, caret_byte);
    let caret_char = chars.iter().position(|(b, _)| *b >= caret_b).unwrap_or(len);
    let mut start = 0usize;
    if fonts.width(text, font_size) > avail_w.max(4.0) {
        start = caret_char;
        while start > 0 {
            let sub_start = chars[start - 1].0;
            let sub_end = chars.get(caret_char).map_or(text.len(), |(b, _)| *b);
            if fonts.width(&text[sub_start..sub_end], font_size) <= avail_w.max(4.0) {
                start -= 1;
            } else {
                break;
            }
        }
    }
    if rel_x <= 0.0 {
        return chars.get(start).map_or(0, |(b, _)| *b);
    }
    let start_byte = chars.get(start).map_or(0, |(b, _)| *b);
    let mut prev_w = 0.0f32;
    for idx in start..len {
        let end_byte = chars.get(idx + 1).map_or(text.len(), |(b, _)| *b);
        let w = fonts.width(&text[start_byte..end_byte], font_size);
        let mid = (prev_w + w) * 0.5;
        if rel_x < mid {
            return chars[idx].0;
        }
        prev_w = w;
        if w > avail_w + 32.0 {
            return end_byte;
        }
    }
    text.len()
}

impl Browser {
    /// Enable asynchronous host clipboard integration (X11 `CLIPBOARD` and `PRIMARY`).
    /// When disabled (the default in unit/embedding tests), copy/cut/paste operate
    /// synchronously against the in-memory clipboard buffers.
    pub fn set_host_clipboard_enabled(&mut self, enabled: bool) {
        self.selection.host_clipboard_enabled = enabled;
    }

    /// Take pending outbound `CLIPBOARD` / `PRIMARY` updates for the windowing host.
    pub fn take_clipboard_update(&mut self) -> Option<ClipboardUpdate> {
        if self.selection.pending_update.clipboard.is_none()
            && self.selection.pending_update.primary.is_none()
        {
            None
        } else {
            Some(std::mem::take(&mut self.selection.pending_update))
        }
    }

    /// Take a pending inbound paste request for the windowing host to fulfill.
    pub fn take_paste_request(&mut self) -> Option<PasteRequest> {
        self.selection.pending_paste.take()
    }

    /// Most recent text written to `CLIPBOARD` by this browser instance.
    pub fn clipboard_text(&self) -> &str {
        &self.selection.clipboard_buffer
    }

    /// Most recent text written to `PRIMARY` by mouse selection in this browser instance.
    pub fn primary_text(&self) -> &str {
        &self.selection.primary_buffer
    }

    /// Seed the in-memory clipboard buffer (used by tests or host synchronization).
    pub fn set_clipboard_text(&mut self, text: impl Into<String>) {
        self.selection.clipboard_buffer = bounded_clipboard(&text.into());
    }

    /// Seed the in-memory primary selection buffer.
    pub fn set_primary_text(&mut self, text: impl Into<String>) {
        self.selection.primary_buffer = bounded_clipboard(&text.into());
    }

    pub(crate) fn write_clipboard(&mut self, text: String, also_primary: bool) {
        if text.is_empty() {
            return;
        }
        let bounded = bounded_clipboard(&text);
        self.selection.clipboard_buffer = bounded.clone();
        self.selection.pending_update.clipboard = Some(bounded.clone());
        if also_primary {
            self.selection.primary_buffer = bounded.clone();
            self.selection.pending_update.primary = Some(bounded);
        }
    }

    pub(crate) fn write_primary(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        let bounded = bounded_clipboard(&text);
        self.selection.primary_buffer = bounded.clone();
        self.selection.pending_update.primary = Some(bounded);
    }

    /// Request a paste from `CLIPBOARD` or `PRIMARY` into the currently focused editable field.
    pub fn paste_from_source(&mut self, source: PasteSource, and_go: bool) {
        if !and_go && !self.focus_is_editable() {
            return;
        }
        if self.selection.host_clipboard_enabled {
            self.selection.pending_paste = Some(PasteRequest { source, and_go });
        } else {
            let text = match source {
                PasteSource::Clipboard => self.selection.clipboard_buffer.clone(),
                PasteSource::Primary => self.selection.primary_buffer.clone(),
            };
            self.complete_paste(&text, and_go);
        }
    }

    /// Deliver pasted text from the host clipboard into the active target.
    pub fn complete_paste(&mut self, text: &str, and_go: bool) {
        let bounded = bounded_clipboard(text);
        if and_go {
            let cleaned = sanitize_single_line(&bounded);
            if cleaned.is_empty() {
                return;
            }
            self.focus = Focus::Address;
            self.address = if cleaned.len() <= MAX_EDIT_BYTES {
                cleaned
            } else {
                cleaned[..clamp_utf8(&cleaned, MAX_EDIT_BYTES)].to_owned()
            };
            self.selection.address_cursor = self.address.len();
            self.selection.address_anchor = None;
            self.select_all = false;
            self.submit_omnibox(&self.address.clone());
            return;
        }
        if bounded.is_empty() {
            return;
        }
        match self.focus {
            Focus::ConsoleInput if self.devtools.open => {
                self.console_insert_text(&bounded);
            }
            Focus::DevToolsField(kind) if self.devtools.open => {
                let single = sanitize_single_line(&bounded);
                self.devtools_field_insert_text(kind, &single);
            }
            Focus::Address if !self.modal_open() => {
                let single = sanitize_single_line(&bounded);
                self.address_insert_text(&single);
            }
            Focus::FindInput if self.find.open && !self.modal_open() => {
                self.find_insert_text(&bounded);
            }
            Focus::Input(node) if !self.modal_open() => {
                if self
                    .document
                    .nodes
                    .get(node)
                    .is_some_and(|n| n.tag == "textarea")
                {
                    self.page_input_insert_text(node, &bounded);
                } else {
                    let single = sanitize_single_line(&bounded);
                    self.page_input_insert_text(node, &single);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn focus_is_editable(&self) -> bool {
        if self.devtools.open
            && !self.menu_open
            && !self.about_open
            && !self.settings_open
            && !self.bookmarks_open
            && !self.history_open
            && !self.downloads_open
            && self.devtools.context.is_none()
            && matches!(self.focus, Focus::ConsoleInput | Focus::DevToolsField(_))
        {
            return true;
        }
        !self.modal_open()
            && matches!(
                self.focus,
                Focus::Address | Focus::Input(_) | Focus::FindInput
            )
    }

    pub(crate) fn sync_address_selection_flags(&mut self) {
        let len = self.address.len();
        self.selection.address_cursor = clamp_utf8(&self.address, self.selection.address_cursor);
        self.selection.address_anchor = self
            .selection
            .address_anchor
            .map(|a| clamp_utf8(&self.address, a))
            .filter(|a| *a != self.selection.address_cursor);
        self.select_all = len > 0
            && normalized_range(
                &self.address,
                self.selection.address_cursor,
                self.selection.address_anchor,
            ) == Some((0, len));
    }

    pub(crate) fn ensure_address_range_synced(&mut self) {
        if self.focus == Focus::Address && self.select_all && !self.address.is_empty() {
            self.selection.address_anchor = Some(0);
            self.selection.address_cursor = self.address.len();
        } else {
            self.selection.address_cursor =
                clamp_utf8(&self.address, self.selection.address_cursor);
            self.selection.address_anchor = self
                .selection
                .address_anchor
                .map(|a| clamp_utf8(&self.address, a))
                .filter(|a| *a != self.selection.address_cursor);
        }
    }

    pub(crate) fn input_default_and_kind(&self, node: usize) -> Option<(String, String)> {
        let idx = self.input_item(node)?;
        match self.document.items.get(idx)? {
            Item::Input { value, kind, .. } => Some((value.clone(), kind.clone())),
            _ => None,
        }
    }

    pub(crate) fn is_password_input(&self, node: usize) -> bool {
        self.input_default_and_kind(node)
            .is_some_and(|(_, kind)| kind.eq_ignore_ascii_case("password"))
    }

    pub(crate) fn current_input_value(&self, node: usize) -> Option<String> {
        let (default, _) = self.input_default_and_kind(node)?;
        Some(self.values.get(&node).cloned().unwrap_or(default))
    }

    pub(crate) fn ensure_input_range_synced(&mut self, node: usize) {
        let val = self.current_input_value(node).unwrap_or_default();
        if self.selection.input_node != Some(node) {
            self.selection.input_node = Some(node);
            if self.select_all && !val.is_empty() {
                self.selection.input_anchor = Some(0);
                self.selection.input_cursor = val.len();
            } else {
                self.selection.input_anchor = None;
                self.selection.input_cursor = val.len();
            }
        } else if self.select_all && !val.is_empty() {
            self.selection.input_anchor = Some(0);
            self.selection.input_cursor = val.len();
        } else {
            self.selection.input_cursor = clamp_utf8(&val, self.selection.input_cursor);
            self.selection.input_anchor = self
                .selection
                .input_anchor
                .map(|a| clamp_utf8(&val, a))
                .filter(|a| *a != self.selection.input_cursor);
        }
    }

    pub(crate) fn sync_input_selection_flags(&mut self, node: usize) {
        let len = self.current_input_value(node).map_or(0, |v| v.len());
        self.select_all = len > 0
            && normalized_range(
                &self.current_input_value(node).unwrap_or_default(),
                self.selection.input_cursor,
                self.selection.input_anchor,
            ) == Some((0, len));
    }

    /// Character-offset tuple `(sel_start_chars, sel_end_chars, caret_chars)` for Sparkle `<input>` rendering.
    pub(crate) fn spark_input_selection(&self, node: usize) -> Option<(usize, usize, usize)> {
        if self.focus != Focus::Input(node) || self.selection.input_node != Some(node) {
            return None;
        }
        let val = self.current_input_value(node)?;
        let cur_b = clamp_utf8(&val, self.selection.input_cursor);
        let caret_chars = val[..cur_b].chars().count();
        if self.select_all && !val.is_empty() {
            let total = val.chars().count();
            return Some((0, total, total));
        }
        if let Some((b0, b1)) = normalized_range(&val, cur_b, self.selection.input_anchor) {
            let c0 = val[..b0].chars().count();
            let c1 = val[..b1].chars().count();
            Some((c0, c1, caret_chars))
        } else {
            Some((caret_chars, caret_chars, caret_chars))
        }
    }

    pub(crate) fn address_insert_text(&mut self, text: &str) {
        self.ensure_address_range_synced();
        let mut cursor = self.selection.address_cursor;
        let mut anchor = self.selection.address_anchor;
        if edit_replace_selection_or_insert(
            &mut self.address,
            &mut cursor,
            &mut anchor,
            text,
            MAX_EDIT_BYTES,
        ) {
            self.selection.address_cursor = cursor;
            self.selection.address_anchor = anchor;
            self.select_all = false;
            self.refresh_omnibox_suggestions();
            self.dirty = true;
        } else if self.select_all && text.is_empty() {
            self.address.clear();
            self.selection.address_cursor = 0;
            self.selection.address_anchor = None;
            self.select_all = false;
            self.refresh_omnibox_suggestions();
            self.dirty = true;
        }
    }

    pub(crate) fn page_input_insert_text(&mut self, node: usize, text: &str) {
        if self.next_edit == u64::MAX {
            return;
        }
        let Some((default, _)) = self.input_default_and_kind(node) else {
            return;
        };
        self.ensure_input_range_synced(node);
        let mut cursor = self.selection.input_cursor;
        let mut anchor = self.selection.input_anchor;
        let was_select_all = self.select_all;
        let buf = self.values.entry(node).or_insert(default);
        let changed = if was_select_all && text.is_empty() && !buf.is_empty() {
            buf.clear();
            cursor = 0;
            anchor = None;
            true
        } else {
            edit_replace_selection_or_insert(buf, &mut cursor, &mut anchor, text, MAX_EDIT_BYTES)
        };
        if changed {
            self.selection.input_cursor = cursor;
            self.selection.input_anchor = anchor;
            self.select_all = false;
            self.next_edit = self.next_edit.saturating_add(1);
            self.edit_versions.insert(node, self.next_edit);
            self.dirty = true;
        }
    }

    /// Select all content in the currently focused surface (`Ctrl+A`).
    pub fn select_all_active(&mut self) {
        if self.devtools.open
            && !self.menu_open
            && !self.about_open
            && !self.settings_open
            && !self.bookmarks_open
            && self.devtools.context.is_none()
        {
            match self.focus {
                Focus::ConsoleInput => {
                    if !self.devtools.console_input.is_empty() {
                        self.devtools.console_anchor = Some(0);
                        self.devtools.console_cursor = self.devtools.console_input.len();
                        self.dirty = true;
                    }
                    return;
                }
                Focus::DevToolsField(kind) => {
                    let len = {
                        let (buf, cur, _) = self.field_state_mut(kind);
                        let len = buf.len();
                        *cur = len;
                        len
                    };
                    self.devtools.field_anchor = (len > 0).then_some((kind, 0));
                    self.dirty = true;
                    return;
                }
                Focus::DevTools => {
                    match self.devtools.tab {
                        devtools::Tab::Console => {
                            let total = self.console_visual_row_count();
                            if total > 0 {
                                self.devtools.console_sel = Some((0, total - 1));
                                self.dirty = true;
                            }
                        }
                        devtools::Tab::Sources => {
                            let total = self.active_source_line_count();
                            if total > 0 {
                                self.devtools.sources_sel = Some((0, total - 1));
                                self.dirty = true;
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                _ => {}
            }
        }
        if self.modal_open() {
            return;
        }
        match self.focus {
            Focus::Address => {
                if !self.address.is_empty() {
                    self.selection.address_anchor = Some(0);
                    self.selection.address_cursor = self.address.len();
                    self.select_all = true;
                } else {
                    self.select_all = true;
                }
                self.dirty = true;
            }
            Focus::FindInput => {
                if !self.find.query.is_empty() {
                    self.find.anchor = Some(0);
                    self.find.cursor = self.find.query.len();
                    self.select_all = true;
                } else {
                    self.select_all = true;
                }
                self.dirty = true;
            }
            Focus::Input(node) => {
                let len = self.current_input_value(node).map_or(0, |v| v.len());
                self.selection.input_node = Some(node);
                if len > 0 {
                    self.selection.input_anchor = Some(0);
                    self.selection.input_cursor = len;
                }
                self.select_all = true;
                self.dirty = true;
            }
            Focus::Page => {
                if let Some((idx_last, last_run)) =
                    self.selection.page_runs.iter().enumerate().next_back()
                {
                    self.selection.page_sel = Some(PageSelection {
                        epoch: self.dom_epoch,
                        anchor: TextPos { run: 0, byte: 0 },
                        focus: TextPos {
                            run: idx_last,
                            byte: last_run.text.len(),
                        },
                    });
                    if let Some(txt) = self.selected_page_text() {
                        self.write_primary(txt);
                    }
                }
                self.select_all = true;
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// Copy the active selection to `CLIPBOARD` (`Ctrl+C` / `Ctrl+Insert` / Context-Menu `Copy`).
    pub fn copy_selection(&mut self) -> Option<String> {
        if self.about_open {
            let text = self.about_lines.join("\n");
            if !text.is_empty() {
                self.write_clipboard(text.clone(), false);
                return Some(text);
            }
        }
        if self.devtools.open
            && !self.menu_open
            && !self.about_open
            && !self.settings_open
            && !self.bookmarks_open
        {
            match self.focus {
                Focus::ConsoleInput => {
                    let s = &self.devtools.console_input;
                    if let Some((b0, b1)) = normalized_range(
                        s,
                        self.devtools.console_cursor,
                        self.devtools.console_anchor,
                    ) {
                        let out = s[b0..b1].to_owned();
                        self.write_clipboard(out.clone(), false);
                        return Some(out);
                    }
                    return None;
                }
                Focus::DevToolsField(kind) => {
                    let anc = self
                        .devtools
                        .field_anchor
                        .filter(|(k, _)| *k == kind)
                        .map(|(_, a)| a);
                    let out = {
                        let (buf, cur, _) = self.field_state_mut(kind);
                        let c = *cur;
                        normalized_range(buf, c, anc).map(|(b0, b1)| buf[b0..b1].to_owned())
                    }?;
                    self.write_clipboard(out.clone(), false);
                    return Some(out);
                }
                Focus::DevTools => {
                    let out = self.devtools_copy_current_tab()?;
                    self.write_clipboard(out.clone(), false);
                    return Some(out);
                }
                _ => {}
            }
        }
        match self.focus {
            Focus::Address => {
                self.ensure_address_range_synced();
                let (b0, b1) = normalized_range(
                    &self.address,
                    self.selection.address_cursor,
                    self.selection.address_anchor,
                )?;
                let out = self.address[b0..b1].to_owned();
                self.write_clipboard(out.clone(), false);
                Some(out)
            }
            Focus::FindInput => {
                let (b0, b1) =
                    normalized_range(&self.find.query, self.find.cursor, self.find.anchor)?;
                let out = self.find.query[b0..b1].to_owned();
                self.write_clipboard(out.clone(), false);
                Some(out)
            }
            Focus::Input(node) => {
                if self.is_password_input(node) {
                    return None;
                }
                self.ensure_input_range_synced(node);
                let val = self.current_input_value(node)?;
                let (b0, b1) = normalized_range(
                    &val,
                    self.selection.input_cursor,
                    self.selection.input_anchor,
                )?;
                let out = val[b0..b1].to_owned();
                self.write_clipboard(out.clone(), false);
                Some(out)
            }
            Focus::Page => {
                let out = self.selected_page_text()?;
                self.write_clipboard(out.clone(), false);
                Some(out)
            }
            _ => None,
        }
    }

    /// Cut the active editable selection to `CLIPBOARD` (`Ctrl+X` / `Shift+Delete`).
    /// Never modifies read-only surfaces (`Page`, `Console` log, `Sources`, etc.) or password inputs.
    pub fn cut_selection(&mut self) -> Option<String> {
        if self.devtools.open
            && !self.menu_open
            && !self.about_open
            && !self.settings_open
            && !self.bookmarks_open
            && self.devtools.context.is_none()
        {
            match self.focus {
                Focus::ConsoleInput => {
                    let cur = self.devtools.console_cursor;
                    let anc = self.devtools.console_anchor;
                    let (b0, b1) = normalized_range(&self.devtools.console_input, cur, anc)?;
                    let out = self.devtools.console_input[b0..b1].to_owned();
                    self.devtools.console_input.drain(b0..b1);
                    self.devtools.console_cursor = b0;
                    self.devtools.console_anchor = None;
                    self.devtools.console_history_idx = None;
                    self.write_clipboard(out.clone(), false);
                    self.dirty = true;
                    return Some(out);
                }
                Focus::DevToolsField(kind) => {
                    let anc = self
                        .devtools
                        .field_anchor
                        .filter(|(k, _)| *k == kind)
                        .map(|(_, a)| a);
                    let out = {
                        let (buf, cur, _) = self.field_state_mut(kind);
                        let (b0, b1) = normalized_range(buf, *cur, anc)?;
                        let out = buf[b0..b1].to_owned();
                        buf.drain(b0..b1);
                        *cur = b0;
                        out
                    };
                    self.devtools.field_anchor = None;
                    if matches!(
                        kind,
                        devtools::FieldKind::ConsoleFilter | devtools::FieldKind::NetworkFilter
                    ) {
                        self.devtools.row = 0;
                    }
                    self.write_clipboard(out.clone(), false);
                    self.dirty = true;
                    return Some(out);
                }
                _ => {}
            }
        }
        if self.modal_open() {
            return None;
        }
        match self.focus {
            Focus::Address => {
                self.ensure_address_range_synced();
                let (b0, b1) = normalized_range(
                    &self.address,
                    self.selection.address_cursor,
                    self.selection.address_anchor,
                )?;
                let out = self.address[b0..b1].to_owned();
                self.address.drain(b0..b1);
                self.selection.address_cursor = b0;
                self.selection.address_anchor = None;
                self.select_all = false;
                self.refresh_omnibox_suggestions();
                self.write_clipboard(out.clone(), false);
                self.dirty = true;
                Some(out)
            }
            Focus::FindInput => {
                let (b0, b1) =
                    normalized_range(&self.find.query, self.find.cursor, self.find.anchor)?;
                let out = self.find.query[b0..b1].to_owned();
                self.find.query.drain(b0..b1);
                self.find.cursor = b0;
                self.find.anchor = None;
                self.select_all = false;
                self.find.active = 0;
                self.recompute_find_matches(true);
                self.write_clipboard(out.clone(), false);
                self.dirty = true;
                Some(out)
            }
            Focus::Input(node) => {
                if self.is_password_input(node) || self.next_edit == u64::MAX {
                    return None;
                }
                let (default, _) = self.input_default_and_kind(node)?;
                self.ensure_input_range_synced(node);
                let cur = self.selection.input_cursor;
                let anc = self.selection.input_anchor;
                let buf = self.values.entry(node).or_insert(default);
                let (b0, b1) = normalized_range(buf, cur, anc)?;
                let out = buf[b0..b1].to_owned();
                buf.drain(b0..b1);
                self.selection.input_cursor = b0;
                self.selection.input_anchor = None;
                self.select_all = false;
                self.next_edit = self.next_edit.saturating_add(1);
                self.edit_versions.insert(node, self.next_edit);
                self.write_clipboard(out.clone(), false);
                self.dirty = true;
                Some(out)
            }
            _ => None,
        }
    }

    pub(crate) fn has_page_selection(&self) -> bool {
        self.selection
            .page_sel
            .is_some_and(|sel| sel.epoch == self.dom_epoch && sel.anchor != sel.focus)
    }

    /// Extract selected page prose in reading order, preserving spaces and line breaks.
    pub fn selected_page_text(&self) -> Option<String> {
        let sel = self.selection.page_sel?;
        if sel.epoch != self.dom_epoch
            || sel.anchor == sel.focus
            || self.selection.page_runs.is_empty()
        {
            return None;
        }
        let (start, end) = if sel.anchor <= sel.focus {
            (sel.anchor, sel.focus)
        } else {
            (sel.focus, sel.anchor)
        };
        let end_run = end.run.min(self.selection.page_runs.len() - 1);
        if start.run > end_run {
            return None;
        }
        let mut out = String::new();
        let mut prev_run: Option<&DocTextRun> = None;
        for idx in start.run..=end_run {
            let run = &self.selection.page_runs[idx];
            let b0 = if idx == start.run {
                clamp_utf8(&run.text, start.byte)
            } else {
                0
            };
            let b1 = if idx == end_run {
                clamp_utf8(&run.text, end.byte)
            } else {
                run.text.len()
            };
            if b1 <= b0 {
                continue;
            }
            let slice = &run.text[b0..b1];
            if let Some(prev) = prev_run {
                let line_tol = (prev.height.max(run.height) as i32 / 2).max(6);
                if (run.doc_y - prev.doc_y).abs() > line_tol || run.x + 4 < prev.x {
                    out.push('\n');
                } else if run.x > prev.x + prev.width as i32 + 1 {
                    out.push(' ');
                }
            }
            out.push_str(slice);
            prev_run = Some(run);
            if out.len() >= MAX_CLIPBOARD_BYTES {
                break;
            }
        }
        (!out.is_empty()).then(|| bounded_clipboard(&out))
    }

    pub(crate) fn hit_page_text_pos(&mut self, x: i32, y: i32) -> Option<TextPos> {
        if self.selection.page_runs.is_empty() {
            return None;
        }
        let doc_y = y - self.page_top() + self.scroll;
        // 1. Exact or same-line match first.
        let mut best: Option<(i64, usize)> = None;
        for (idx, run) in self.selection.page_runs.iter().enumerate() {
            let ry0 = run.doc_y;
            let ry1 = run.doc_y + run.height as i32;
            let dy = if doc_y < ry0 {
                i64::from(ry0 - doc_y)
            } else if doc_y > ry1 {
                i64::from(doc_y - ry1)
            } else {
                0
            };
            let rx0 = run.x;
            let rx1 = run.x + run.width as i32;
            let dx = if x < rx0 {
                i64::from(rx0 - x)
            } else if x > rx1 {
                i64::from(x - rx1)
            } else {
                0
            };
            // Heavily weight vertical line distance so dragging horizontally stays on the line.
            let score = dy * 2000 + dx;
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, idx));
            }
        }
        let (_, idx) = best?;
        let run = self.selection.page_runs[idx].clone();
        let ry0 = run.doc_y;
        let ry1 = run.doc_y + run.height as i32;
        let byte = if doc_y < ry0 - 4 {
            0
        } else if doc_y > ry1 + 4 {
            run.text.len()
        } else {
            hit_byte_in_line(
                &mut self.fonts,
                &run.text,
                run.font_size,
                run.width as f32 + 64.0,
                run.text.len(),
                (x - run.x) as f32,
            )
        };
        Some(TextPos { run: idx, byte })
    }

    #[cfg(feature = "chrome")]
    pub(crate) fn paint_page_selection(&mut self, canvas: &mut Canvas) {
        let Some(sel) = self.selection.page_sel else {
            return;
        };
        if sel.epoch != self.dom_epoch
            || sel.anchor == sel.focus
            || self.selection.page_runs.is_empty()
        {
            return;
        }
        let (start, end) = if sel.anchor <= sel.focus {
            (sel.anchor, sel.focus)
        } else {
            (sel.focus, sel.anchor)
        };
        let end_run = end.run.min(self.selection.page_runs.len() - 1);
        if start.run > end_run {
            return;
        }
        let top = self.page_top();
        let view_h = self.viewport_height() as i32;
        let page_w = self.page_viewport_width() as i32;
        for idx in start.run..=end_run {
            let run = self.selection.page_runs[idx].clone();
            let ry = run.doc_y - self.scroll + top;
            let rh = run.height as i32;
            if ry + rh <= top || ry >= top + view_h {
                continue;
            }
            let b0 = if idx == start.run {
                clamp_utf8(&run.text, start.byte)
            } else {
                0
            };
            let b1 = if idx == end_run {
                clamp_utf8(&run.text, end.byte)
            } else {
                run.text.len()
            };
            if b1 <= b0 {
                continue;
            }
            let x0 = if b0 == 0 {
                run.x
            } else {
                run.x + self.fonts.width(&run.text[..b0], run.font_size).floor() as i32
            };
            let x1 = if b1 >= run.text.len() {
                run.x + run.width as i32
            } else {
                run.x + self.fonts.width(&run.text[..b1], run.font_size).ceil() as i32
            };
            let cx0 = x0.clamp(0, page_w);
            let cx1 = x1.clamp(0, page_w);
            let cy0 = ry.max(top);
            let cy1 = (ry + rh).min(top + view_h);
            if cx1 > cx0 && cy1 > cy0 {
                devtools::blend_rect_pub(
                    canvas,
                    cx0,
                    cy0,
                    (cx1 - cx0) as u32,
                    (cy1 - cy0) as u32,
                    0x2563eb,
                    76,
                );
            }
        }
    }

    pub(crate) fn address_bar_rect(&self) -> Option<(i32, i32, u32, u32)> {
        if !self.chrome {
            return None;
        }
        let aw = self
            .width
            .saturating_sub(if self.width >= 480 { 296 } else { 252 });
        (aw > 0).then_some((196, 12, aw, 39))
    }

    pub(crate) fn hit_address_byte(&mut self, x: i32) -> usize {
        let Some((ax, _, aw, _)) = self.address_bar_rect() else {
            return self.address.len();
        };
        let cur = clamp_utf8(&self.address, self.selection.address_cursor);
        let addr = self.address.clone();
        hit_byte_in_line(
            &mut self.fonts,
            &addr,
            16.,
            aw as f32 - 20.,
            cur,
            (x - (ax + 10)) as f32,
        )
    }

    pub(crate) fn hit_page_input_byte(&mut self, node: usize, x: i32) -> usize {
        let Some(hit) = self
            .hits
            .iter()
            .rfind(|h| matches!(h.action, Action::Input(n) if n == node))
            .cloned()
        else {
            return self.current_input_value(node).map_or(0, |v| v.len());
        };
        let raw = self.current_input_value(node).unwrap_or_default();
        if raw.is_empty() {
            return 0;
        }
        let is_pw = self.is_password_input(node);
        let font_size = self
            .style_cache
            .style_for(node)
            .map_or(17., |s| s.font_size);
        let cur_b = clamp_utf8(&raw, self.selection.input_cursor);
        if is_pw {
            let char_count = raw.chars().count();
            let masked = "•".repeat(char_count);
            let cur_chars = raw[..cur_b].chars().count();
            let cur_masked_b = cur_chars * "•".len();
            let mb = hit_byte_in_line(
                &mut self.fonts,
                &masked,
                font_size,
                hit.w as f32 - 14.,
                cur_masked_b,
                (x - (hit.x + 5)) as f32,
            );
            let target_chars = masked[..clamp_utf8(&masked, mb)].chars().count();
            raw.char_indices()
                .nth(target_chars)
                .map_or(raw.len(), |(b, _)| b)
        } else {
            hit_byte_in_line(
                &mut self.fonts,
                &raw,
                font_size,
                hit.w as f32 - 14.,
                cur_b,
                (x - (hit.x + 5)) as f32,
            )
        }
    }

    /// Middle-click (`Button 2` on X11): focus an editable control under `(x, y)`,
    /// position the caret at `(x, y)`, and paste from `PRIMARY`.
    pub fn middle_click_at(&mut self, x: i32, y: i32) {
        if !self.chrome || self.popup_modal_open() {
            return;
        }
        if let Some((ax, ay, aw, ah)) = self.address_bar_rect()
            && x >= ax
            && x < ax + aw as i32
            && y >= ay
            && y < ay + ah as i32
        {
            self.focus = Focus::Address;
            let pos = self.hit_address_byte(x);
            self.selection.address_cursor = pos;
            self.selection.address_anchor = None;
            self.select_all = false;
            self.dirty = true;
            self.paste_from_source(PasteSource::Primary, false);
            return;
        }
        if self.find.open
            && let Some((fx, fy, fw, fh)) = self.find.input_rect
            && x >= fx
            && x < fx + fw as i32
            && y >= fy
            && y < fy + fh as i32
        {
            self.focus = Focus::FindInput;
            self.find.cursor = self.find.query.len();
            self.find.anchor = None;
            self.select_all = false;
            self.dirty = true;
            self.paste_from_source(PasteSource::Primary, false);
            return;
        }
        if self.devtools.open && self.try_focus_devtools_editable_at(x, y) {
            self.paste_from_source(PasteSource::Primary, false);
            return;
        }
        if let Some(idx) = self.topmost_hit(x, y) {
            match &self.hits[idx].action {
                Action::Input(node) => {
                    let node = *node;
                    self.focus = Focus::Input(node);
                    self.ensure_input_range_synced(node);
                    let pos = self.hit_page_input_byte(node, x);
                    self.selection.input_cursor = pos;
                    self.selection.input_anchor = None;
                    self.select_all = false;
                    self.dirty = true;
                    self.paste_from_source(PasteSource::Primary, false);
                }
                Action::Link { href, .. } => {
                    let href = href.clone();
                    self.request_new_tab(&href, true);
                }
                _ => {}
            }
        }
    }

    /// Handle navigation/selection/clipboard keys in `Focus::Address` or `Focus::Input(node)`.
    /// Returns `true` when consumed.
    pub(crate) fn handle_chrome_or_page_clipboard_key(
        &mut self,
        sym: u32,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        if alt {
            return false;
        }
        // Standard + classic X11 clipboard shortcuts:
        // Copy:  Ctrl+C or Ctrl+Insert (0xff63 / 0xff9e)
        // Cut:   Ctrl+X or Shift+Delete (0xffff / 0xff9f)
        // Paste: Ctrl+V or Shift+Insert (0xff63 / 0xff9e)
        let is_copy = (ctrl && !shift && matches!(sym, 0x63 | 0x43))
            || (ctrl && !shift && matches!(sym, 0xff63 | 0xff9e));
        let is_cut = (ctrl && !shift && matches!(sym, 0x78 | 0x58))
            || (!ctrl && shift && matches!(sym, 0xffff | 0xff9f));
        let is_paste = (ctrl && !shift && matches!(sym, 0x76 | 0x56))
            || (!ctrl && shift && matches!(sym, 0xff63 | 0xff9e));
        if is_copy {
            let _ = self.copy_selection();
            return true;
        }
        if is_cut {
            let _ = self.cut_selection();
            return true;
        }
        if is_paste {
            self.paste_from_source(PasteSource::Clipboard, false);
            return true;
        }
        if ctrl && !shift && matches!(sym, 0x61 | 0x41) {
            self.select_all_active();
            return true;
        }
        if self.modal_open() {
            return false;
        }
        match self.focus {
            Focus::Address => self.handle_address_edit_key(sym, ctrl, shift),
            Focus::FindInput => self.handle_find_edit_key(sym, ctrl, shift),
            Focus::Input(node) => self.handle_page_input_edit_key(node, sym, ctrl, shift),
            _ => false,
        }
    }

    fn handle_address_edit_key(&mut self, sym: u32, ctrl: bool, shift: bool) -> bool {
        self.ensure_address_range_synced();
        let mut cur = self.selection.address_cursor;
        let mut anc = self.selection.address_anchor;
        match sym {
            0xff08 => {
                if edit_delete_backward(&mut self.address, &mut cur, &mut anc, ctrl) {
                    self.selection.address_cursor = cur;
                    self.selection.address_anchor = anc;
                    self.sync_address_selection_flags();
                    self.refresh_omnibox_suggestions();
                    self.dirty = true;
                }
                true
            }
            0xffff | 0xff9f => {
                if edit_delete_forward(&mut self.address, &mut cur, &mut anc, ctrl) {
                    self.selection.address_cursor = cur;
                    self.selection.address_anchor = anc;
                    self.sync_address_selection_flags();
                    self.refresh_omnibox_suggestions();
                    self.dirty = true;
                }
                true
            }
            0xff51 => {
                let target =
                    if !shift && let Some((b0, _)) = normalized_range(&self.address, cur, anc) {
                        b0
                    } else if ctrl {
                        prev_word_boundary(&self.address, cur)
                    } else {
                        prev_char_boundary(&self.address, cur)
                    };
                edit_move_cursor(&self.address, &mut cur, &mut anc, target, shift);
                self.selection.address_cursor = cur;
                self.selection.address_anchor = anc;
                self.sync_address_selection_flags();
                self.dirty = true;
                true
            }
            0xff53 => {
                let target =
                    if !shift && let Some((_, b1)) = normalized_range(&self.address, cur, anc) {
                        b1
                    } else if ctrl {
                        next_word_boundary(&self.address, cur)
                    } else {
                        next_char_boundary(&self.address, cur)
                    };
                edit_move_cursor(&self.address, &mut cur, &mut anc, target, shift);
                self.selection.address_cursor = cur;
                self.selection.address_anchor = anc;
                self.sync_address_selection_flags();
                self.dirty = true;
                true
            }
            0xff50 => {
                edit_move_cursor(&self.address, &mut cur, &mut anc, 0, shift);
                self.selection.address_cursor = cur;
                self.selection.address_anchor = anc;
                self.sync_address_selection_flags();
                self.dirty = true;
                true
            }
            0xff57 => {
                let end = self.address.len();
                edit_move_cursor(&self.address, &mut cur, &mut anc, end, shift);
                self.selection.address_cursor = cur;
                self.selection.address_anchor = anc;
                self.sync_address_selection_flags();
                self.dirty = true;
                true
            }
            _ => false,
        }
    }

    fn handle_page_input_edit_key(
        &mut self,
        node: usize,
        sym: u32,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        let Some((default, _)) = self.input_default_and_kind(node) else {
            return false;
        };
        self.ensure_input_range_synced(node);
        let mut cur = self.selection.input_cursor;
        let mut anc = self.selection.input_anchor;
        match sym {
            0xff08 => {
                if self.next_edit == u64::MAX {
                    return true;
                }
                let buf = self.values.entry(node).or_insert(default);
                if edit_delete_backward(buf, &mut cur, &mut anc, ctrl) {
                    self.selection.input_cursor = cur;
                    self.selection.input_anchor = anc;
                    self.sync_input_selection_flags(node);
                    self.next_edit = self.next_edit.saturating_add(1);
                    self.edit_versions.insert(node, self.next_edit);
                    self.dirty = true;
                }
                true
            }
            0xffff | 0xff9f => {
                if self.next_edit == u64::MAX {
                    return true;
                }
                let buf = self.values.entry(node).or_insert(default);
                if edit_delete_forward(buf, &mut cur, &mut anc, ctrl) {
                    self.selection.input_cursor = cur;
                    self.selection.input_anchor = anc;
                    self.sync_input_selection_flags(node);
                    self.next_edit = self.next_edit.saturating_add(1);
                    self.edit_versions.insert(node, self.next_edit);
                    self.dirty = true;
                }
                true
            }
            0xff51 => {
                let val = self.current_input_value(node).unwrap_or_default();
                let target = if !shift && let Some((b0, _)) = normalized_range(&val, cur, anc) {
                    b0
                } else if ctrl {
                    prev_word_boundary(&val, cur)
                } else {
                    prev_char_boundary(&val, cur)
                };
                edit_move_cursor(&val, &mut cur, &mut anc, target, shift);
                self.selection.input_cursor = cur;
                self.selection.input_anchor = anc;
                self.sync_input_selection_flags(node);
                self.dirty = true;
                true
            }
            0xff53 => {
                let val = self.current_input_value(node).unwrap_or_default();
                let target = if !shift && let Some((_, b1)) = normalized_range(&val, cur, anc) {
                    b1
                } else if ctrl {
                    next_word_boundary(&val, cur)
                } else {
                    next_char_boundary(&val, cur)
                };
                edit_move_cursor(&val, &mut cur, &mut anc, target, shift);
                self.selection.input_cursor = cur;
                self.selection.input_anchor = anc;
                self.sync_input_selection_flags(node);
                self.dirty = true;
                true
            }
            0xff50 => {
                let val = self.current_input_value(node).unwrap_or_default();
                edit_move_cursor(&val, &mut cur, &mut anc, 0, shift);
                self.selection.input_cursor = cur;
                self.selection.input_anchor = anc;
                self.sync_input_selection_flags(node);
                self.dirty = true;
                true
            }
            0xff57 => {
                let val = self.current_input_value(node).unwrap_or_default();
                let end = val.len();
                edit_move_cursor(&val, &mut cur, &mut anc, end, shift);
                self.selection.input_cursor = cur;
                self.selection.input_anchor = anc;
                self.sync_input_selection_flags(node);
                self.dirty = true;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn context_menu_items(&self) -> Vec<(&'static str, Action)> {
        match self.devtools.context_kind {
            ContextKind::Page => {
                let mut items = Vec::new();
                if let Some(url) = &self.devtools.context_link {
                    if self.has_page_selection() {
                        items.push(("Copy", Action::ClipboardCopy));
                    }
                    items.push((
                        "Open link in new tab",
                        Action::OpenLinkInNewTab(url.clone()),
                    ));
                    items.push(("Copy link address", Action::CopyText(url.clone())));
                    if url.starts_with("http://") || url.starts_with("https://") {
                        items.push((
                            "Save link as...",
                            Action::DownloadUrl {
                                url: url.clone(),
                                suggested: None,
                            },
                        ));
                    }
                    items.push(("Inspect element", Action::InspectElement));
                    items.push(("Close", Action::CloseMenu));
                } else if self.has_page_selection() {
                    items.push(("Copy", Action::ClipboardCopy));
                    items.push(("Select all", Action::ClipboardSelectAll));
                    items.push(("Inspect element", Action::InspectElement));
                    items.push(("Close", Action::CloseMenu));
                } else {
                    items.push(("Inspect element", Action::InspectElement));
                    items.push(("Select all", Action::ClipboardSelectAll));
                    if self.height >= 360 && self.page_url != "about:blank" {
                        items.push(("Save page as...", Action::SavePage));
                        items.push(("View page source", Action::ViewPageSource));
                    }
                    items.push(("Close", Action::CloseMenu));
                }
                items
            }
            ContextKind::AddressBar => vec![
                ("Cut", Action::ClipboardCut),
                ("Copy", Action::ClipboardCopy),
                ("Paste", Action::ClipboardPaste { and_go: false }),
                ("Paste and go", Action::ClipboardPaste { and_go: true }),
                ("Select all", Action::ClipboardSelectAll),
                ("Close", Action::CloseMenu),
            ],
            ContextKind::PageInput(node) => {
                let mut items = Vec::new();
                if !self.is_password_input(node) {
                    items.push(("Cut", Action::ClipboardCut));
                    items.push(("Copy", Action::ClipboardCopy));
                }
                items.push(("Paste", Action::ClipboardPaste { and_go: false }));
                items.push(("Select all", Action::ClipboardSelectAll));
                items.push(("Inspect element", Action::InspectElement));
                items.push(("Close", Action::CloseMenu));
                items
            }
            ContextKind::ConsoleRepl | ContextKind::DevToolsField(_) => vec![
                ("Cut", Action::ClipboardCut),
                ("Copy", Action::ClipboardCopy),
                ("Paste", Action::ClipboardPaste { and_go: false }),
                ("Select all", Action::ClipboardSelectAll),
                ("Close", Action::CloseMenu),
            ],
            ContextKind::ConsoleLog => vec![
                ("Copy message", Action::DevToolsConsoleCopySelected),
                ("Copy all visible", Action::DevToolsConsoleCopyAll),
                ("Select all", Action::ClipboardSelectAll),
                ("Clear console", Action::DevToolsConsoleClear),
                ("Close", Action::CloseMenu),
            ],
            ContextKind::Elements => vec![
                ("Copy outerHTML", Action::DevToolsElementsCopyHtml),
                ("Copy selector", Action::DevToolsElementsCopySelector),
                ("Copy styles", Action::DevToolsElementsCopyStyles),
                ("Close", Action::CloseMenu),
            ],
            ContextKind::Sources => vec![
                ("Copy source", Action::ClipboardCopy),
                ("Select all", Action::ClipboardSelectAll),
                ("Close", Action::CloseMenu),
            ],
            ContextKind::Network => vec![
                ("Copy URL", Action::DevToolsNetCopyUrl),
                ("Copy details", Action::DevToolsNetCopyDetails),
                ("Copy all URLs", Action::DevToolsNetCopyAllUrls),
                ("Close", Action::CloseMenu),
            ],
            ContextKind::Performance | ContextKind::Application => vec![
                ("Copy report", Action::ClipboardCopy),
                ("Close", Action::CloseMenu),
            ],
        }
    }
}

pub(crate) fn sanitize_single_line(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if matches!(ch, '\r' | '\n' | '\t') {
                ' '
            } else {
                ch
            }
        })
        .filter(|ch| !ch.is_control())
        .collect()
}

/// Slice `text` around `caret_byte` to fit within `avail_w`, returning
/// `(shown_str, sel_x0, sel_x1, caret_x)` in logical pixels relative to the text origin.
#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
pub(crate) fn slice_line_bytes(
    fonts: &mut Fonts,
    text: &str,
    size: f32,
    avail_w: f32,
    sel: Option<(usize, usize)>,
    caret_byte: usize,
) -> (String, i32, i32, i32) {
    let chars: Vec<(usize, char)> = text.char_indices().take(4096).collect();
    let len = chars.len();
    let caret_b = clamp_utf8(text, caret_byte);
    let caret_char = chars.iter().position(|(b, _)| *b >= caret_b).unwrap_or(len);
    let avail = avail_w.max(4.0);
    let mut start = 0usize;
    if fonts.width(text, size) > avail {
        start = caret_char;
        while start > 0 {
            let b0 = chars[start - 1].0;
            let b1 = chars.get(caret_char).map_or(text.len(), |(b, _)| *b);
            if fonts.width(&text[b0..b1], size) <= avail {
                start -= 1;
            } else {
                break;
            }
        }
    }
    let start_b = chars.get(start).map_or(0, |(b, _)| *b);
    let mut end = caret_char;
    while end < len {
        let b1 = chars.get(end + 1).map_or(text.len(), |(b, _)| *b);
        if fonts.width(&text[start_b..b1], size) <= avail {
            end += 1;
        } else {
            break;
        }
    }
    let end_b = chars.get(end).map_or(text.len(), |(b, _)| *b);
    let shown = text[start_b..end_b].to_owned();
    let px_at = |byte_pos: usize, fonts: &mut Fonts| -> i32 {
        let b = clamp_utf8(text, byte_pos).clamp(start_b, end_b);
        if b <= start_b {
            0
        } else {
            fonts.width(&text[start_b..b], size).round() as i32
        }
    };
    let (sx0, sx1) = if let Some((s0, s1)) = sel {
        (px_at(s0, fonts), px_at(s1, fonts))
    } else {
        (0, 0)
    };
    let cx = px_at(caret_b, fonts);
    (shown, sx0, sx1, cx)
}

#[cfg(all(test, feature = "chrome"))]
mod tests {
    use super::*;
    use crate::{
        Action, Browser, Focus, Key,
        devtools::{
            ConsoleEntry, ConsoleEntryKind, ConsoleLevel, FieldKind, NetKind, NetworkEntry, Tab,
        },
        scripts,
    };
    use mg_sparkle::{
        document,
        page_session::console::{ConsoleWireEntry, ConsoleWireLevel},
        paint::Fonts,
    };
    use std::sync::Arc;

    fn app() -> Browser {
        let path = std::env::var("MGBROWSER_FONT")
            .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
        let mut browser = Browser::new(
            Fonts::from_bytes(std::fs::read(path).unwrap()).unwrap(),
            Arc::new(scripts::DisabledScripts),
        );
        browser.resize(1100, 760);
        browser
    }

    #[test]
    fn console_error_rows_repl_and_devtools_fields_support_selection_copy_cut_and_paste() {
        let mut app = app();
        let long_err = "Uncaught TypeError: Cannot read properties of undefined (reading 'hydrateFeedCardComponent')";
        let stack_1 = "at hydrateFeed (https://news.example.com/bundle.js:418:27)";
        let stack_2 = "at HTMLDocument.<anonymous> (https://news.example.com/bundle.js:12:5)";
        app.devtools.console.push(ConsoleEntry {
            level: ConsoleLevel::Error,
            kind: ConsoleEntryKind::Diagnostic,
            source: "JS",
            message: long_err.into(),
            extra_lines: vec![stack_1.into(), stack_2.into()],
            count: 1,
            group_depth: 0,
            style: None,
            object_id: None,
            node_id: None,
            expanded: false,
            properties: None,
        });
        app.devtools.console.push(ConsoleEntry {
            level: ConsoleLevel::Warning,
            kind: ConsoleEntryKind::Diagnostic,
            source: "JS",
            message: "Deprecated feature flag encountered".into(),
            extra_lines: Vec::new(),
            count: 2,
            group_depth: 0,
            style: None,
            object_id: None,
            node_id: None,
            expanded: false,
            properties: None,
        });

        app.toggle_devtools();
        app.activate(Action::DevToolsTab(Tab::Console));
        let baseline = app.paint();
        let (lx, ly, _lw, _lh) = app.devtools.console_log_rect.unwrap();

        // Click first Console error row and press Ctrl+C -> copies full untruncated error + stack!
        app.pointer_down(lx + 24, ly + 8);
        app.pointer_up(lx + 24, ly + 8);
        let highlighted = app.paint();
        assert_ne!(
            highlighted.pixels, baseline.pixels,
            "clicking a Console error row must highlight it visually"
        );
        app.handle_key(Key::Character('c'), true, false, false);
        assert!(app.clipboard_text().contains(long_err));
        assert!(app.clipboard_text().contains(stack_1));
        assert!(app.clipboard_text().contains(stack_2));
        assert!(
            app.take_clipboard_update()
                .and_then(|u| u.clipboard)
                .is_some_and(|c| c.contains(stack_2))
        );

        // Ctrl+A selects all visible Console rows; subtoolbar Copy button copies both entries!
        app.handle_key(Key::Character('a'), true, false, false);
        app.activate(Action::DevToolsConsoleCopySelected);
        assert!(app.clipboard_text().contains(long_err));
        assert!(
            app.clipboard_text()
                .contains("(×2) Deprecated feature flag encountered")
        );

        // Right-click Console log area exposes Copy message / Copy all visible / Select all / Clear console.
        app.context_menu_at(lx + 30, ly + 8);
        assert!(app.popup_modal_open());
        app.paint();
        let labels: Vec<_> = app
            .context_menu_items()
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(
            labels,
            vec![
                "Copy message",
                "Copy all visible",
                "Select all",
                "Clear console",
                "Close",
            ]
        );
        app.handle_key(Key::Escape, false, false, false);
        app.paint();

        // Console `>` REPL: partial selection, Cut (Ctrl+X), Paste (Ctrl+V), Shift+Insert, middle-click PRIMARY.
        app.activate(Action::DevToolsConsoleInput);
        app.type_text("window.fetchData(123)");
        for _ in 0..5 {
            app.handle_key(Key::Left, false, true, false);
        }
        assert_eq!(app.copy_selection().as_deref(), Some("(123)"));
        app.handle_key(Key::Character('x'), true, false, false);
        assert_eq!(app.devtools.console_input, "window.fetchData");
        assert_eq!(app.clipboard_text(), "(123)");
        app.handle_key(Key::Home, false, false, false);
        app.handle_key(Key::Character('v'), true, false, false);
        assert_eq!(app.devtools.console_input, "(123)window.fetchData");
        // Shift+Insert also pastes CLIPBOARD; middle-click pastes PRIMARY!
        app.handle_key(Key::Insert, false, true, false);
        assert_eq!(app.devtools.console_input, "(123)(123)window.fetchData");
        app.write_primary("/*primary*/".into());
        app.paint();
        let (rx, ry, _, _) = app.devtools.console_repl_rect.unwrap();
        app.middle_click_at(rx + 4, ry + 8);
        assert!(app.devtools.console_input.contains("/*primary*/"));

        // DevTools filter field (FieldKind::ConsoleFilter) supports Ctrl+A, Ctrl+X, Ctrl+V.
        app.activate(Action::DevToolsField(FieldKind::ConsoleFilter));
        app.type_text("TypeError");
        app.handle_key(Key::Character('a'), true, false, false);
        app.handle_key(Key::Character('x'), true, false, false);
        assert_eq!(app.devtools.console_search, "");
        assert_eq!(app.clipboard_text(), "TypeError");
        app.handle_key(Key::Character('v'), true, false, false);
        assert_eq!(app.devtools.console_search, "TypeError");

        // Console JS `copy(...)` syncs to system clipboard queue!
        app.ingest_wire_console_and_sync(
            vec![ConsoleWireEntry {
                level: ConsoleWireLevel::Copy("{\"copiedFromConsoleApi\":true}".into()),
                text: String::new(),
                object_id: None,
                node_id: None,
                group_depth: 0,
            }],
            true,
        );
        assert_eq!(app.console_clipboard(), "{\"copiedFromConsoleApi\":true}");
        assert_eq!(app.clipboard_text(), "{\"copiedFromConsoleApi\":true}");
        assert_eq!(
            app.take_clipboard_update()
                .and_then(|u| u.clipboard)
                .as_deref(),
            Some("{\"copiedFromConsoleApi\":true}")
        );
    }

    #[test]
    fn elements_sources_network_performance_application_and_about_copy_work() {
        let mut app = app();
        let html = "<!doctype html>\n<style>#hero.banner { color: #112233; }</style>\n<div id=\"hero\" class=\"banner\"><span>Headline</span></div>\n<script>const x = 1;\nconst y = 2;\nconsole.log(x + y);</script>";
        app.devtools.page_source = html.into();
        app.document = document::parse(html, "https://example.com/");
        app.dom_epoch += 1;
        app.devtools.network.push(NetworkEntry {
            kind: NetKind::Js,
            method: "GET",
            url: "https://example.com/api/feed.json".into(),
            status: Some(200),
            mime: "application/json".into(),
            bytes: 512,
            start_ms: 2.0,
            duration_ms: 8.5,
            error: None,
            pending: false,
            request_headers: vec![("accept".into(), "application/json".into())],
            response_headers: vec![("content-type".into(), "application/json".into())],
            body_preview: Some("{\"items\":[1,2,3]}".into()),
        });

        app.toggle_devtools();
        let hero = app.document.query_selector(0, "#hero").unwrap().unwrap();
        app.select_devtools_node(hero);
        app.paint();

        // Elements: Ctrl+C copies outerHTML, actions copy selector & styles!
        app.handle_key(Key::Character('c'), true, false, false);
        assert_eq!(
            app.clipboard_text(),
            "<div id=\"hero\" class=\"banner\"><span>Headline</span></div>"
        );
        app.activate(Action::DevToolsElementsCopySelector);
        assert_eq!(app.clipboard_text(), "#hero");
        app.activate(Action::DevToolsElementsCopyStyles);
        assert!(app.clipboard_text().contains("display:"));

        // Sources: drag across code lines to copy slice, or copy full source!
        app.activate(Action::DevToolsTab(Tab::Sources));
        app.activate(Action::DevToolsSourceSelect(2)); // inline script #2
        app.paint();
        let (sx, sy, _, _) = app.devtools.sources_code_rect.unwrap();
        app.pointer_down(sx + 20, sy + 6);
        app.pointer_move(sx + 20, sy + 24);
        app.pointer_up(sx + 20, sy + 24);
        app.handle_key(Key::Character('c'), true, false, false);
        assert_eq!(app.clipboard_text(), "const x = 1;\nconst y = 2;");

        // Network: Copy URL & Copy details!
        app.activate(Action::DevToolsTab(Tab::Network));
        app.activate(Action::DevToolsNetSelect(Some(0)));
        app.activate(Action::DevToolsNetCopyUrl);
        assert_eq!(app.clipboard_text(), "https://example.com/api/feed.json");
        app.activate(Action::DevToolsNetCopyDetails);
        assert!(
            app.clipboard_text()
                .contains("GET https://example.com/api/feed.json")
        );
        assert!(app.clipboard_text().contains("{\"items\":[1,2,3]}"));

        // Performance & Application reports:
        app.activate(Action::DevToolsTab(Tab::Performance));
        app.handle_key(Key::Character('c'), true, false, false);
        assert!(
            app.clipboard_text().contains("Navigation") || app.clipboard_text().contains("Page:")
        );

        app.activate(Action::DevToolsTab(Tab::Application));
        app.handle_key(Key::Character('c'), true, false, false);
        assert!(app.clipboard_text().contains("Application"));

        // About modal Ctrl+C copies build identity:
        app.set_build_info("0.9.25", "2026-10-30T07:00:00Z", "abcdef123456");
        app.activate(Action::About);
        app.handle_key(Key::Character('c'), true, false, false);
        assert!(app.clipboard_text().contains("mgbrowser 0.9.25"));
    }

    #[test]
    fn address_bar_supports_select_all_caret_drag_cut_copy_paste_and_primary() {
        let mut app = app();
        app.focus = Focus::Page;
        app.address = "https://news.example.com/articles/top".into();
        app.paint();

        let (ax, ay, _, ah) = app.address_bar_rect().unwrap();
        // First click from page focuses address bar and selects all!
        app.pointer_down(ax + 20, ay + ah as i32 / 2);
        app.pointer_up(ax + 20, ay + ah as i32 / 2);
        assert_eq!(app.focus, Focus::Address);
        assert!(app.select_all);
        app.handle_key(Key::Character('c'), true, false, false);
        assert_eq!(
            app.clipboard_text(),
            "https://news.example.com/articles/top"
        );

        // Second click (beyond double-click radius) places caret without select-all clobbering on pointer_up!
        app.pointer_down(ax + 60, ay + ah as i32 / 2);
        app.pointer_up(ax + 60, ay + ah as i32 / 2);
        assert!(!app.select_all);
        assert_eq!(app.selection.address_anchor, None);

        // Drag selection across address bar populates PRIMARY!
        app.pointer_down(ax + 12, ay + ah as i32 / 2);
        app.pointer_move(ax + 90, ay + ah as i32 / 2);
        app.pointer_up(ax + 90, ay + ah as i32 / 2);
        assert!(app.primary_text().starts_with("http"));

        // Keyboard select last 4 chars ("/top"), Cut, Paste at end!
        app.handle_key(Key::End, false, false, false);
        for _ in 0..4 {
            app.handle_key(Key::Left, false, true, false);
        }
        app.handle_key(Key::Character('x'), true, false, false);
        assert_eq!(app.address, "https://news.example.com/articles");
        assert_eq!(app.clipboard_text(), "/top");
        app.handle_key(Key::Character('v'), true, false, false);
        assert_eq!(app.address, "https://news.example.com/articles/top");

        // Right-click address bar exposes Cut / Copy / Paste / Paste and go / Select all.
        app.context_menu_at(ax + 24, ay + ah as i32 / 2);
        let labels: Vec<_> = app
            .context_menu_items()
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert_eq!(
            labels,
            vec![
                "Cut",
                "Copy",
                "Paste",
                "Paste and go",
                "Select all",
                "Close"
            ]
        );
        app.handle_key(Key::Escape, false, false, false);

        // Async host clipboard mode queues PasteRequest for Paste and go and navigates on complete_paste!
        app.set_host_clipboard_enabled(true);
        app.activate(Action::ClipboardPaste { and_go: true });
        assert_eq!(
            app.take_paste_request(),
            Some(PasteRequest {
                source: PasteSource::Clipboard,
                and_go: true,
            })
        );
    }

    #[test]
    fn page_inputs_support_cut_copy_paste_and_password_fields_never_leak_plaintext() {
        let mut app = app();
        let html = "<form><input id=\"user\" value=\"alice_wonder\"><input id=\"pw\" type=\"password\" value=\"s3cr3t-token\"></form>";
        app.document = document::parse(html, "https://example.com/");
        app.dom_epoch += 1;
        app.paint();

        let user_node = app.document.query_selector(0, "#user").unwrap().unwrap();
        let pw_node = app.document.query_selector(0, "#pw").unwrap().unwrap();

        let user_hit = app
            .hits
            .iter()
            .find(|h| matches!(h.action, Action::Input(n) if n == user_node))
            .cloned()
            .unwrap();
        app.pointer_down(user_hit.x + 8, user_hit.y + 10);
        app.pointer_up(user_hit.x + 8, user_hit.y + 10);
        assert_eq!(app.focus, Focus::Input(user_node));

        // Select "_wonder", Cut -> bumps edit_versions and copies "_wonder"!
        app.handle_key(Key::End, false, false, false);
        for _ in 0..7 {
            app.handle_key(Key::Left, false, true, false);
        }
        let v_before = app.edit_versions.get(&user_node).copied().unwrap_or(0);
        app.handle_key(Key::Character('x'), true, false, false);
        assert_eq!(
            app.values.get(&user_node).map(String::as_str),
            Some("alice")
        );
        assert_eq!(app.clipboard_text(), "_wonder");
        let v_after_cut = app.edit_versions.get(&user_node).copied().unwrap_or(0);
        assert!(v_after_cut > v_before);

        // Paste back with Ctrl+V -> bumps edit_versions again!
        app.handle_key(Key::Character('v'), true, false, false);
        assert_eq!(
            app.values.get(&user_node).map(String::as_str),
            Some("alice_wonder")
        );
        assert!(app.edit_versions.get(&user_node).copied().unwrap_or(0) > v_after_cut);

        // Now test <input type="password">: Ctrl+A + Ctrl+C, Ctrl+X, drag PRIMARY, and context menu MUST NOT leak!
        let _ = app.take_clipboard_update();
        let pw_hit = app
            .hits
            .iter()
            .find(|h| matches!(h.action, Action::Input(n) if n == pw_node))
            .cloned()
            .unwrap();
        app.pointer_down(pw_hit.x + 4, pw_hit.y + 10);
        app.pointer_move(pw_hit.x + 120, pw_hit.y + 10);
        app.pointer_up(pw_hit.x + 120, pw_hit.y + 10);
        assert_eq!(app.focus, Focus::Input(pw_node));
        assert!(!app.primary_text().contains("s3cr3t"));

        app.handle_key(Key::Character('a'), true, false, false);
        app.handle_key(Key::Character('c'), true, false, false);
        app.handle_key(Key::Character('x'), true, false, false);
        assert_eq!(app.clipboard_text(), "_wonder");
        assert_eq!(app.take_clipboard_update(), None);
        assert_eq!(
            app.current_input_value(pw_node).as_deref(),
            Some("s3cr3t-token"),
            "Ctrl+X must not delete or copy masked password characters"
        );

        // Password context menu omits Cut and Copy, but allows Paste and Select all!
        app.context_menu_at(pw_hit.x + 10, pw_hit.y + 10);
        let pw_labels: Vec<_> = app
            .context_menu_items()
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert!(!pw_labels.contains(&"Copy"));
        assert!(!pw_labels.contains(&"Cut"));
        assert!(pw_labels.contains(&"Paste"));
        app.handle_key(Key::Escape, false, false, false);

        // Pasting into a password field DOES work!
        app.handle_key(Key::Character('a'), true, false, false);
        app.handle_key(Key::Character('v'), true, false, false);
        assert_eq!(
            app.values.get(&pw_node).map(String::as_str),
            Some("_wonder")
        );
    }

    #[test]
    fn page_prose_supports_drag_selection_double_click_word_ctrl_a_copy_and_ignores_cut_paste() {
        let mut app = app();
        let html = "<!doctype html><h1>Breaking Headline</h1><p>First paragraph with selectable prose.</p>";
        app.document = document::parse(html, "https://example.com/");
        app.dom_epoch += 1;
        let unselected = app.paint();
        assert!(
            !app.selection.page_runs.is_empty(),
            "painted page must record positioned TextRuns"
        );

        // Drag across the first TextRun -> renders selection highlight, populates PRIMARY, and Ctrl+C copies!
        let first_run = app.selection.page_runs[0].clone();
        let y_screen = first_run.doc_y - app.scroll + app.page_top() + 4;
        app.pointer_down(first_run.x + 1, y_screen);
        app.pointer_move(first_run.x + first_run.width as i32 - 1, y_screen);
        app.pointer_up(first_run.x + first_run.width as i32 - 1, y_screen);
        assert!(app.has_page_selection());
        assert!(!app.primary_text().is_empty());
        let selected_canvas = app.paint();
        assert_ne!(
            selected_canvas.pixels, unselected.pixels,
            "page text selection must render highlight pixels"
        );

        // Ctrl+A + Ctrl+C copies all visible page text in reading order; Ctrl+X / Ctrl+V are no-ops!
        app.handle_key(Key::Character('a'), true, false, false);
        app.handle_key(Key::Character('c'), true, false, false);
        assert!(app.clipboard_text().contains("Breaking Headline"));
        assert!(
            app.clipboard_text()
                .contains("First paragraph with selectable prose.")
        );
        assert_eq!(app.cut_selection(), None);
        let dom_len = app.document.nodes.len();
        app.handle_key(Key::Character('v'), true, false, false);
        assert_eq!(app.document.nodes.len(), dom_len);

        // Right-clicking page with active selection includes "Copy" above "Inspect element"!
        app.context_menu_at(first_run.x + 6, y_screen);
        let page_labels: Vec<_> = app
            .context_menu_items()
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert_eq!(
            page_labels,
            vec!["Copy", "Select all", "Inspect element", "Close"]
        );
    }
}
