//! Read-only native inspection of the page that Chassis actually renders.
//! This is not a Chrome DevTools frontend, JavaScript evaluator or debugger.
use super::*;

const MAX_MESSAGES: usize = 96;
const MAX_MESSAGE_BYTES: usize = 512;
const MAX_LINES: usize = 128;

#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    pub context: Option<(i32, i32)>,
    pub diagnostics: bool,
    pub row: usize,
    pub painted_epoch: u64,
    selected: Option<(u64, usize)>,
    messages: Vec<String>,
    omitted: usize,
    pub rendering: mg_sparkle::style::StyleDiagnostics,
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
        assert_eq!(state.messages.len(), 1);
        assert!(!state.messages[0].chars().any(char::is_control));
        for index in 0..200 {
            state.record("Resource", &format!("{index}{}", "💥".repeat(1000)));
        }
        assert_eq!(state.messages.len(), MAX_MESSAGES);
        assert!(
            state
                .messages
                .iter()
                .all(|text| text.len() <= MAX_MESSAGE_BYTES)
        );
        assert!(state.omitted > 0);
        state.new_navigation();
        assert!(state.messages.is_empty() && state.rendering.entries.is_empty());
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
    fn right_click_selects_the_painted_noninteractive_element() {
        let mut app = fixture();
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
        assert!(app.modal_open());
        // Context opening discards old hits before the menu's first paint.
        assert!(app.hits.is_empty());
        app.handle_key(Key::DeveloperTools, false, false, false);
        assert!(app.devtools.open && app.devtools.context.is_none());
        assert_eq!(app.devtools.selected, Some((app.dom_epoch, id)));
        app.handle_key(Key::Escape, false, false, false);
        app.paint();
        app.context_menu_at(rect.x + 1, rect.y + 1);
        app.handle_key(Key::Enter, false, false, false);
        assert!(app.devtools.open && app.devtools.context.is_none());
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
        assert!(app.hits.iter().all(|hit| app.modal_action(&hit.action)));
        app.handle_key(Key::Escape, false, false, false);
        assert!(!app.modal_open());
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn selection_is_invalidated_on_projection_and_stale_paint_cannot_retarget() {
        let mut app = fixture();
        app.toggle_devtools();
        assert!(app.devtools.selected.is_some());
        app.dom_epoch += 1;
        app.document = document::parse("<p>Replacement</p>", "https://example.com/next");
        assert!(app.inspector_lines()[0].contains("document changed"));
        assert_eq!(app.inspected_node_at(20, TOP + 20), None);
        app.paint();
        assert!(app.devtools.selected.is_none() && app.devtools.open);
        app.devtools.new_navigation();
        assert!(!app.devtools.visible());
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn overlay_blocks_page_input_and_wheel_scrolls_its_own_content() {
        let mut app = fixture();
        let input = app
            .document
            .item_nodes
            .iter()
            .copied()
            .find(|node| app.input_item(*node).is_some())
            .unwrap();
        app.focus = Focus::Input(input);
        let old_value = app.values.get(&input).cloned();
        app.toggle_devtools();
        app.activate(Action::InspectorSection(true));
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
        app.wheel_scroll_by(100);
        assert_eq!(app.scroll, page_scroll);
        assert_eq!(app.devtools.row, 3);
        assert_eq!(app.values.get(&input).cloned(), old_value);
        assert_eq!(app.history.len(), history);
        assert!(app.smooth_scroll.is_none());
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
            "<style>div{display:flex;position:absolute}</style><div>Text</div>",
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
        app.activate(Action::InspectorSection(true));
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
    fn script_errors_and_worker_failures_are_retained_as_bounded_diagnostics() {
        let mut app = fixture();
        let reply = SessionReply {
            revision: 0,
            snapshot: None,
            outcome: Default::default(),
            default_action: DefaultAction::None,
            navigation: None,
            errors: vec!["External script loading is not implemented".into()],
            scripts_executed: 0,
            allocations: None,
            boa: None,
            state: RealmState::Closed,
            acknowledgements: vec![],
        };
        app.script_summary(&reply);
        app.block_session("Session expired");
        assert!(
            app.devtools
                .messages
                .iter()
                .any(|line| line.contains("External script"))
        );
        assert!(
            app.devtools
                .messages
                .iter()
                .any(|line| line.contains("Session expired"))
        );
        app.devtools.new_navigation();
        assert!(app.devtools.messages.is_empty());
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
                app.activate(Action::InspectorSection(true));
                app.devtools.record(
                    "Resource",
                    &format!("{}: HTTP 403", "https://example.com/long/".repeat(18)),
                );
                let canvas = app.paint();
                assert_eq!(canvas.width, 400 * u32::from(scale) / 100);
                assert!(app.hits.iter().all(|hit| hit.x >= 0
                    && hit.y >= 0
                    && hit.x + hit.w as i32 <= app.width as i32
                    && hit.y + hit.h as i32 <= app.height as i32));
                let rows = app.inspector_display_lines();
                assert!(rows.iter().all(|row| app.fonts.width(row, 13.) <= 360.));
                assert!(rows.len() > app.inspector_lines().len());
                // No page hit survives, including after geometry/style changes.
                assert!(app.hits.iter().all(|hit| app.modal_action(&hit.action)));
            }
        }
    }
}

impl State {
    pub fn visible(&self) -> bool {
        self.open || self.context.is_some()
    }
    pub fn close(&mut self) {
        self.open = false;
        self.context = None;
        self.row = 0;
    }
    pub fn new_navigation(&mut self) {
        self.close();
        self.selected = None;
        self.painted_epoch = 0;
        self.messages.clear();
        self.omitted = 0;
        self.rendering = Default::default();
    }
    pub fn invalidate_selection(&mut self, epoch: u64) {
        if self
            .selected
            .is_some_and(|(selected_epoch, _)| selected_epoch != epoch)
        {
            self.selected = None;
            self.context = None;
            self.row = 0;
        }
    }
    pub fn record(&mut self, source: &str, message: &str) {
        let entry = bounded(
            &format!("{source}: {}", bounded(message, MAX_MESSAGE_BYTES)),
            MAX_MESSAGE_BYTES,
        );
        if self.messages.contains(&entry) {
            return;
        }
        if self.messages.len() == MAX_MESSAGES {
            self.omitted = self.omitted.saturating_add(1);
        } else {
            self.messages.push(entry);
        }
    }
}

/// Plain text only: page strings cannot insert new rows or terminal controls.
fn bounded(value: &str, limit: usize) -> String {
    if limit < 3 {
        return String::new();
    }
    let mut result = String::new();
    for ch in value.chars() {
        let ch = if ch.is_control()
            || matches!(ch, '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
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

impl Browser {
    /// Open the native page context menu at logical browser-surface coordinates.
    /// Only geometry from the currently painted document may identify a node.
    /// Hosts must convert physical input using the current display scale.
    pub fn context_menu_at(&mut self, x: i32, y: i32) {
        if !self.chrome
            || self.modal_open()
            || self.loading
            || x < 0
            || x >= self.width as i32
            || y < self.page_top()
            || y >= self.page_top() + self.viewport_height() as i32
        {
            return;
        }
        self.cancel_pointer_input();
        self.select_all = false;
        self.focus = Focus::Page;
        self.devtools.selected = self
            .inspected_node_at(x, y)
            .map(|node| (self.dom_epoch, node));
        self.devtools.context = Some((x, y));
        self.modal_focus = 0;
        self.hits.clear();
        self.dirty = true;
    }

    /// Toggle the bounded native inspector without enabling remote debugging.
    pub fn toggle_devtools(&mut self) {
        if !self.chrome {
            return;
        }
        self.cancel_pointer_input();
        if self.devtools.open {
            self.devtools.close();
        } else {
            if self.devtools.context.is_none() {
                self.devtools.selected =
                    (!self.document.nodes.is_empty()).then_some((self.dom_epoch, 0));
            }
            self.open_inspector();
        }
        self.hits.clear();
        self.dirty = true;
    }

    /// Cancel transient input when a host switches or moves the owning pane.
    /// The focused control, page session and native dialogs are preserved.
    pub fn cancel_pointer_input(&mut self) {
        self.stop_scroll();
        self.pointer_press = None;
    }

    pub(super) fn open_inspector(&mut self) {
        self.devtools.invalidate_selection(self.dom_epoch);
        self.devtools.open = true;
        self.devtools.context = None;
        self.devtools.diagnostics = false;
        self.devtools.row = 0;
        self.menu_open = false;
        self.about_open = false;
        self.settings_open = false;
        self.bookmarks_open = false;
        self.modal_focus = 0;
        self.focus = Focus::Page;
        self.select_all = false;
        self.hits.clear();
        self.pointer_press = None;
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

    pub(super) fn inspector_scroll(&mut self, rows: i32) {
        if !self.devtools.open {
            return;
        }
        let count = self.inspector_display_lines().len();
        self.devtools.row = self
            .devtools
            .row
            .saturating_add_signed(rows as isize)
            .min(count.saturating_sub(self.inspector_rows()));
        self.pointer_press = None;
        self.dirty = true;
    }

    fn inspector_rows(&self) -> usize {
        (self.height.saturating_sub(184) / 20).clamp(1, 120) as usize
    }

    // Wrap bounded source text using the same logical font metrics as painting.
    // Long resource URLs must not hide the error at the end of a row.
    fn inspector_display_lines(&mut self) -> Vec<String> {
        let width = self.width.saturating_sub(16).min(860).saturating_sub(24) as f32;
        let mut lines = Vec::new();
        for raw in self.inspector_lines() {
            let clean = bounded(&raw, 2048);
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

    fn inspector_lines(&self) -> Vec<String> {
        if self.devtools.diagnostics {
            let mut lines = vec![
                format!(
                    "JavaScript: {}",
                    if self.scripts_enabled {
                        "enabled (bounded Boa)"
                    } else {
                        "disabled; use --enable-scripts"
                    }
                ),
                format!(
                    "{} loaded stylesheets; {} fetched image resources",
                    self.document.stylesheets.len(),
                    self.document.resources.len()
                ),
                "Partial diagnostics only: absence of errors is not compatibility.".into(),
                "No JS evaluator/debugger, network timeline or console capture yet.".into(),
            ];
            lines.extend(self.devtools.messages.iter().cloned());
            for entry in &self.devtools.rendering.entries {
                let location = match (entry.line, entry.column) {
                    (Some(line), Some(column)) => format!(":{line}:{column}"),
                    _ => String::new(),
                };
                let node = entry
                    .node
                    .map(|id| format!(" node {id}"))
                    .unwrap_or_default();
                lines.push(bounded(
                    &format!(
                        "{}{} {}{}: {}",
                        entry.kind, node, entry.source, location, entry.message
                    ),
                    1024,
                ));
            }
            let omitted = self
                .devtools
                .omitted
                .saturating_add(self.devtools.rendering.omitted);
            if omitted > 0 {
                lines.push(format!(
                    "{omitted} additional diagnostics omitted by bounds"
                ));
            }
            return lines;
        }
        let Some((epoch, id)) = self
            .devtools
            .selected
            .filter(|(epoch, _)| *epoch == self.dom_epoch)
        else {
            return vec![
                "Selection unavailable or document changed.".into(),
                "Close and right-click an element to inspect the current page.".into(),
            ];
        };
        let Some(node) = self.document.nodes.get(id) else {
            return vec!["Selected node is unavailable.".into()];
        };
        let mut lines = vec![
            format!("Node {id} <{}> : document {epoch}", bounded(&node.tag, 80)),
            format!("Page: {}", bounded(&self.page_url, 512)),
            "Read-only parser DOM and painted geometry, not full Chrome DevTools.".into(),
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
        lines.push(format!("Ancestors (nearest 16): {}", ancestors.join(" > ")));
        let mut bounds: Option<(i64, i64, i64, i64)> = None;
        let mut work = 0;
        for rect in &self.boxes {
            let mut source = rect.node;
            for _ in 0..=256 {
                work += 1;
                if work > 4_000_000 {
                    break;
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
            if work > 4_000_000 {
                break;
            }
        }
        if self.devtools.painted_epoch != epoch || work > 4_000_000 || self.boxes.len() >= 100_000 {
            lines.push("Painted bounds unavailable (stale geometry or query limit).".into());
        } else if let Some((x, y, right, bottom)) = bounds {
            lines.push(format!(
                "Painted union: x={x} y={y} width={} height={} CSS px",
                right - x,
                bottom - y
            ));
            lines.push("Coordinates are viewport-relative; not a full CSS box model.".into());
        } else {
            lines.push("No painted box for this node.".into());
        }
        lines.push("Attributes (first 32):".into());
        for (key, value) in node.attributes.iter().take(32) {
            lines.push(format!(
                "  {}=\"{}\"",
                bounded(key, 80),
                bounded(value, 512)
            ));
        }
        if node.attributes.len() > 32 {
            lines.push("  Additional attributes omitted.".into());
        }
        if node.attributes.is_empty() {
            lines.push("  (none)".into());
        }
        lines.push("Text sample (bounded descendant text, not rendered innerText):".into());
        let mut text = String::new();
        let mut pending = vec![id];
        let mut steps = 0;
        while let Some(current) = pending.pop() {
            steps += 1;
            if steps > 4096 || text.len() >= 512 {
                break;
            }
            let Some(value) = self.document.nodes.get(current) else {
                continue;
            };
            if value.tag == "#text" {
                text.push_str(&bounded(&value.text, 512usize.saturating_sub(text.len())));
            }
            let available = 4096usize.saturating_sub(pending.len());
            pending.extend(value.children.iter().take(available).rev().copied());
        }
        lines.push(if text.is_empty() {
            "  (empty)".into()
        } else {
            text
        });
        lines.push(
            "Source style attributes are above. Computed-style inspection/editing is not implemented."
                .into(),
        );
        lines.truncate(MAX_LINES);
        lines
    }

    #[cfg(feature = "chrome")]
    pub(super) fn paint_devtools(&mut self, canvas: &mut Canvas) {
        if !self.devtools.visible() {
            return;
        }
        let palette = self.effective_theme().palette();
        self.hits.clear();
        if let Some((x, y)) = self.devtools.context {
            let width = self.width.saturating_sub(12).min(226);
            let x = x.clamp(6, self.width.saturating_sub(width + 6) as i32);
            let y = y.clamp(6, self.height.saturating_sub(86) as i32);
            canvas.rect(x, y, width, 80, palette.border);
            self.inspector_button(
                canvas,
                (x + 4, y + 4, width - 8),
                "Inspect element",
                Action::InspectElement,
                0,
            );
            self.inspector_button(
                canvas,
                (x + 4, y + 42, width - 8),
                "Close",
                Action::CloseMenu,
                1,
            );
            return;
        }
        let width = self.width.saturating_sub(16).min(860);
        let height = self.height.saturating_sub(16);
        let x = (self.width - width) as i32 / 2;
        let y = 8;
        canvas.rect(x, y, width, height, palette.border);
        canvas.rect(x + 2, y + 2, width - 4, height - 4, palette.panel);
        self.inspector_text(
            canvas,
            x + 12,
            y + 10,
            "Developer tools : read-only preview",
            width - 24,
            17.,
            palette.ink,
        );
        let button_width = (width - 40) / 3;
        for (index, (label, action)) in [
            ("Elements", Action::InspectorSection(false)),
            ("Diagnostics", Action::InspectorSection(true)),
            ("Close", Action::CloseMenu),
        ]
        .into_iter()
        .enumerate()
        {
            self.inspector_button(
                canvas,
                (
                    x + 12 + index as i32 * (button_width as i32 + 8),
                    y + 40,
                    button_width,
                ),
                label,
                action,
                index,
            );
        }
        let selected = usize::from(self.devtools.diagnostics);
        canvas.rect(
            x + 16 + selected as i32 * (button_width as i32 + 8),
            y + 70,
            button_width - 8,
            3,
            palette.accent,
        );
        let lines = self.inspector_display_lines();
        let rows = self.inspector_rows();
        self.devtools.row = self.devtools.row.min(lines.len().saturating_sub(rows));
        for (index, line) in lines.iter().skip(self.devtools.row).take(rows).enumerate() {
            self.inspector_text(
                canvas,
                x + 12,
                y + 84 + index as i32 * 20,
                line,
                width - 24,
                13.,
                palette.ink,
            );
        }
        let footer = y + height as i32 - 72;
        self.inspector_button(
            canvas,
            (x + 12, footer, 92),
            "Previous",
            Action::InspectorScroll(false),
            3,
        );
        self.inspector_button(
            canvas,
            (x + 112, footer, 72),
            "Next",
            Action::InspectorScroll(true),
            4,
        );
        let label = format!(
            "{}-{} of {}",
            self.devtools.row + 1,
            (self.devtools.row + rows).min(lines.len()),
            lines.len()
        );
        self.inspector_text(
            canvas,
            x + 196,
            footer + 8,
            &label,
            width.saturating_sub(210),
            12.,
            palette.muted,
        );
        self.inspector_text(
            canvas,
            x + 12,
            y + height as i32 - 26,
            "Wheel/arrows: scroll. Tab/Enter: controls. Esc/F12: close.",
            width - 24,
            12.,
            palette.muted,
        );
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
        area: (i32, i32, u32),
        label: &str,
        action: Action,
        index: usize,
    ) {
        let (x, y, width) = area;
        let palette = self.effective_theme().palette();
        if self.modal_focus == index {
            canvas.rect(x - 1, y - 1, width + 2, 36, palette.accent);
        }
        canvas.rect(x, y, width, 34, palette.button);
        self.inspector_text(
            canvas,
            x + 8,
            y + 8,
            label,
            width.saturating_sub(16),
            14.,
            palette.ink,
        );
        self.hits.push(Hit {
            x,
            y,
            w: width,
            h: 34,
            action,
        });
    }
}
