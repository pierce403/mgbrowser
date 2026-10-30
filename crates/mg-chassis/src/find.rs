//! Find-in-page (`Ctrl+F`, `F3`, `Shift+F3`, `Ctrl+G`) state, matching, and highlights.

use crate::clipboard::{
    self, DocTextRun, clamp_utf8, edit_delete_backward, edit_delete_forward, edit_move_cursor,
    edit_replace_selection_or_insert, next_char_boundary, next_word_boundary, normalized_range,
    prev_char_boundary, prev_word_boundary,
};
use crate::{Browser, Focus};

pub const MAX_FIND_MATCHES: usize = 4096;
pub const MAX_FIND_QUERY_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FindMatch {
    pub run: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub doc_y: i32,
    pub height: u32,
}

#[derive(Default)]
pub(crate) struct FindState {
    pub open: bool,
    pub query: String,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub matches: Vec<FindMatch>,
    pub active: usize,
    pub matched_epoch: u64,
    pub matched_geometry: u64,
    pub input_rect: Option<(i32, i32, u32, u32)>,
}

/// Scan positioned `DocTextRun`s case-insensitively for `query`.
/// Preserves exact UTF-8 byte offsets in each original `run.text`.
pub(crate) fn scan_runs_case_insensitive(runs: &[DocTextRun], query: &str) -> Vec<FindMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let needle_chars: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle_chars.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for (run_idx, run) in runs.iter().enumerate() {
        if run.text.is_empty() {
            continue;
        }
        let mut lowered: Vec<(char, usize, usize)> = Vec::with_capacity(run.text.len());
        for (byte_idx, ch) in run.text.char_indices() {
            let end_byte = byte_idx + ch.len_utf8();
            for lc in ch.to_lowercase() {
                lowered.push((lc, byte_idx, end_byte));
            }
        }
        if lowered.len() < needle_chars.len() {
            continue;
        }
        let mut i = 0;
        while i + needle_chars.len() <= lowered.len() {
            let mut ok = true;
            for (k, &nc) in needle_chars.iter().enumerate() {
                if lowered[i + k].0 != nc {
                    ok = false;
                    break;
                }
            }
            if ok {
                let b0 = lowered[i].1;
                let b1 = lowered[i + needle_chars.len() - 1].2;
                let clean_boundary = (i == 0 || lowered[i - 1].1 != b0)
                    && (i + needle_chars.len() == lowered.len()
                        || lowered[i + needle_chars.len()].1
                            != lowered[i + needle_chars.len() - 1].1);
                if clean_boundary && b1 > b0 {
                    matches.push(FindMatch {
                        run: run_idx,
                        byte_start: b0,
                        byte_end: b1,
                        doc_y: run.doc_y,
                        height: run.height,
                    });
                    if matches.len() >= MAX_FIND_MATCHES {
                        return matches;
                    }
                    i += needle_chars.len();
                    continue;
                }
            }
            i += 1;
        }
    }
    matches
}

impl Browser {
    pub fn find_bar_open(&self) -> bool {
        self.find.open
    }

    pub fn find_query(&self) -> &str {
        &self.find.query
    }

    pub fn find_match_count(&self) -> (usize, usize) {
        if self.find.matches.is_empty() {
            (0, 0)
        } else {
            (
                (self.find.active % self.find.matches.len()) + 1,
                self.find.matches.len(),
            )
        }
    }

    pub(crate) fn open_find_bar(&mut self) {
        if !self.chrome {
            return;
        }
        self.omnibox.open = false;
        self.find.open = true;
        self.focus = Focus::FindInput;
        if !self.find.query.is_empty() {
            self.find.anchor = Some(0);
            self.find.cursor = self.find.query.len();
            self.select_all = true;
        } else {
            self.find.anchor = None;
            self.find.cursor = 0;
            self.select_all = false;
        }
        self.recompute_find_matches(false);
        self.dirty = true;
    }

    pub(crate) fn close_find_bar(&mut self) {
        self.find.open = false;
        self.find.input_rect = None;
        if self.focus == Focus::FindInput {
            self.focus = Focus::Page;
            self.select_all = false;
        }
        self.dirty = true;
    }

    pub(crate) fn recompute_find_matches(&mut self, scroll_to_active: bool) {
        self.find.matches =
            scan_runs_case_insensitive(&self.selection.page_runs, &self.find.query);
        self.find.matched_epoch = self.dom_epoch;
        self.find.matched_geometry = self.geometry_revision;
        if self.find.matches.is_empty() {
            self.find.active = 0;
        } else if self.find.active >= self.find.matches.len() {
            self.find.active = 0;
        }
        if scroll_to_active && !self.find.matches.is_empty() {
            self.scroll_active_find_into_view();
        }
        self.dirty = true;
    }

    pub(crate) fn step_find_match(&mut self, forward: bool) {
        if !self.chrome {
            return;
        }
        if !self.find.open {
            self.open_find_bar();
            return;
        }
        if self.find.matched_epoch != self.dom_epoch
            || self.find.matched_geometry != self.geometry_revision
        {
            self.recompute_find_matches(false);
        }
        let total = self.find.matches.len();
        if total == 0 {
            self.dirty = true;
            return;
        }
        self.find.active = if forward {
            (self.find.active + 1) % total
        } else {
            (self.find.active + total - 1) % total
        };
        self.scroll_active_find_into_view();
        self.dirty = true;
    }

    pub(crate) fn scroll_active_find_into_view(&mut self) {
        let Some(&m) = self.find.matches.get(self.find.active) else {
            return;
        };
        let view_h = self.viewport_height() as i32;
        if view_h <= 0 {
            return;
        }
        let top_pad = 48.min(view_h / 3);
        let bot_pad = 48.min(view_h / 3);
        let vis_top = self.scroll + top_pad;
        let vis_bot = self.scroll + view_h - bot_pad;
        let match_bot = m.doc_y + m.height as i32;
        if m.doc_y < vis_top || match_bot > vis_bot {
            let centered = m.doc_y - (view_h / 3);
            self.stop_scroll();
            self.scroll = centered.clamp(0, self.max_scroll());
        }
    }

    pub(crate) fn find_insert_text(&mut self, text: &str) {
        let clean = clipboard::sanitize_single_line(text);
        self.find.cursor = clamp_utf8(&self.find.query, self.find.cursor);
        self.find.anchor = self
            .find
            .anchor
            .map(|a| clamp_utf8(&self.find.query, a))
            .filter(|a| *a != self.find.cursor);
        if self.select_all && !self.find.query.is_empty() {
            self.find.anchor = Some(0);
            self.find.cursor = self.find.query.len();
        }
        let mut cursor = self.find.cursor;
        let mut anchor = self.find.anchor;
        let changed = if self.select_all && clean.is_empty() && !self.find.query.is_empty() {
            self.find.query.clear();
            cursor = 0;
            anchor = None;
            true
        } else {
            edit_replace_selection_or_insert(
                &mut self.find.query,
                &mut cursor,
                &mut anchor,
                &clean,
                MAX_FIND_QUERY_BYTES,
            )
        };
        if changed {
            self.find.cursor = cursor;
            self.find.anchor = anchor;
            self.select_all = false;
            self.find.active = 0;
            self.recompute_find_matches(true);
        }
    }

    pub(crate) fn handle_find_edit_key(&mut self, sym: u32, ctrl: bool, shift: bool) -> bool {
        self.find.cursor = clamp_utf8(&self.find.query, self.find.cursor);
        self.find.anchor = self
            .find
            .anchor
            .map(|a| clamp_utf8(&self.find.query, a))
            .filter(|a| *a != self.find.cursor);
        if self.select_all && !self.find.query.is_empty() {
            self.find.anchor = Some(0);
            self.find.cursor = self.find.query.len();
        }
        let mut cur = self.find.cursor;
        let mut anc = self.find.anchor;
        match sym {
            0xff08 => {
                if edit_delete_backward(&mut self.find.query, &mut cur, &mut anc, ctrl) {
                    self.find.cursor = cur;
                    self.find.anchor = anc;
                    self.select_all = false;
                    self.find.active = 0;
                    self.recompute_find_matches(true);
                }
                true
            }
            0xffff | 0xff9f => {
                if edit_delete_forward(&mut self.find.query, &mut cur, &mut anc, ctrl) {
                    self.find.cursor = cur;
                    self.find.anchor = anc;
                    self.select_all = false;
                    self.find.active = 0;
                    self.recompute_find_matches(true);
                }
                true
            }
            0xff51 => {
                let target =
                    if !shift && let Some((b0, _)) = normalized_range(&self.find.query, cur, anc) {
                        b0
                    } else if ctrl {
                        prev_word_boundary(&self.find.query, cur)
                    } else {
                        prev_char_boundary(&self.find.query, cur)
                    };
                edit_move_cursor(&self.find.query, &mut cur, &mut anc, target, shift);
                self.find.cursor = cur;
                self.find.anchor = anc;
                self.select_all = false;
                self.dirty = true;
                true
            }
            0xff53 => {
                let target =
                    if !shift && let Some((_, b1)) = normalized_range(&self.find.query, cur, anc) {
                        b1
                    } else if ctrl {
                        next_word_boundary(&self.find.query, cur)
                    } else {
                        next_char_boundary(&self.find.query, cur)
                    };
                edit_move_cursor(&self.find.query, &mut cur, &mut anc, target, shift);
                self.find.cursor = cur;
                self.find.anchor = anc;
                self.select_all = false;
                self.dirty = true;
                true
            }
            0xff50 => {
                edit_move_cursor(&self.find.query, &mut cur, &mut anc, 0, shift);
                self.find.cursor = cur;
                self.find.anchor = anc;
                self.select_all = false;
                self.dirty = true;
                true
            }
            0xff57 => {
                let end = self.find.query.len();
                edit_move_cursor(&self.find.query, &mut cur, &mut anc, end, shift);
                self.find.cursor = cur;
                self.find.anchor = anc;
                self.select_all = false;
                self.dirty = true;
                true
            }
            _ => false,
        }
    }

    #[cfg(feature = "chrome")]
    pub(crate) fn paint_find_highlights(&mut self, canvas: &mut mg_sparkle::paint::Canvas) {
        if !self.find.open || self.find.query.is_empty() {
            return;
        }
        if self.find.matched_epoch != self.dom_epoch
            || self.find.matched_geometry != self.geometry_revision
        {
            self.find.matches =
                scan_runs_case_insensitive(&self.selection.page_runs, &self.find.query);
            self.find.matched_epoch = self.dom_epoch;
            self.find.matched_geometry = self.geometry_revision;
            if self.find.active >= self.find.matches.len() {
                self.find.active = 0;
            }
        }
        if self.find.matches.is_empty() {
            return;
        }
        let top = self.page_top();
        let view_h = self.viewport_height() as i32;
        let page_w = self.page_viewport_width() as i32;
        let active_idx = self.find.active.min(self.find.matches.len() - 1);
        for (idx, m) in self.find.matches.clone().into_iter().enumerate() {
            let Some(run) = self.selection.page_runs.get(m.run).cloned() else {
                continue;
            };
            let ry = run.doc_y - self.scroll + top;
            let rh = run.height as i32;
            if ry + rh <= top || ry >= top + view_h {
                continue;
            }
            let b0 = clamp_utf8(&run.text, m.byte_start);
            let b1 = clamp_utf8(&run.text, m.byte_end);
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
                let (color, alpha) = if idx == active_idx {
                    (0xf97316, 155)
                } else {
                    (0xfacc15, 110)
                };
                crate::devtools::blend_rect_pub(
                    canvas,
                    cx0,
                    cy0,
                    (cx1 - cx0) as u32,
                    (cy1 - cy0) as u32,
                    color,
                    alpha,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_insensitive_unicode_find_returns_valid_utf8_slices() {
        let runs = vec![
            DocTextRun {
                node: 1,
                x: 10,
                doc_y: 20,
                width: 200,
                height: 18,
                font_size: 16.0,
                text: "Hello Rust & CAFÉ rust!".into(),
            },
            DocTextRun {
                node: 2,
                x: 10,
                doc_y: 44,
                width: 120,
                height: 18,
                font_size: 16.0,
                text: "More café notes".into(),
            },
        ];
        let rust_hits = scan_runs_case_insensitive(&runs, "RUST");
        assert_eq!(rust_hits.len(), 2);
        assert_eq!(
            &runs[rust_hits[0].run].text[rust_hits[0].byte_start..rust_hits[0].byte_end],
            "Rust"
        );
        assert_eq!(
            &runs[rust_hits[1].run].text[rust_hits[1].byte_start..rust_hits[1].byte_end],
            "rust"
        );

        let cafe_hits = scan_runs_case_insensitive(&runs, "café");
        assert_eq!(cafe_hits.len(), 2);
        assert_eq!(
            &runs[cafe_hits[0].run].text[cafe_hits[0].byte_start..cafe_hits[0].byte_end],
            "CAFÉ"
        );
        assert_eq!(
            &runs[cafe_hits[1].run].text[cafe_hits[1].byte_start..cafe_hits[1].byte_end],
            "café"
        );
    }
}
