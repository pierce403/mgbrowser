//! Optional browser controls; page pixels come from Sparkle.
use super::*;
impl Browser {
    fn toolbar_button(&mut self, c: &mut Canvas, x: i32, action: Action, enabled: bool) {
        let p = self.effective_theme().palette();
        c.rect(x, 14, 36, 34, p.button);
        let color = if enabled { p.ink } else { p.muted };
        let paths: &[&[(i32, i32)]] = match action {
            Action::Menu => &[
                &[(8, 10), (27, 10)],
                &[(8, 17), (27, 17)],
                &[(8, 24), (27, 24)],
            ],
            Action::Back => &[&[(22, 9), (14, 17), (22, 25)], &[(14, 17), (28, 17)]],
            Action::Forward => &[&[(14, 9), (22, 17), (14, 25)], &[(8, 17), (22, 17)]],
            Action::Reload => &[
                &[
                    (27, 12),
                    (23, 8),
                    (14, 8),
                    (8, 14),
                    (8, 22),
                    (14, 27),
                    (23, 27),
                    (27, 23),
                ],
                &[(27, 6), (27, 14), (19, 14)],
            ],
            Action::StopLoading => &[&[(10, 9), (26, 25)], &[(26, 9), (10, 25)]],
            Action::ToggleBookmark => &[&[
                (18, 5),
                (22, 13),
                (31, 14),
                (24, 21),
                (26, 30),
                (18, 25),
                (10, 30),
                (12, 21),
                (5, 14),
                (14, 13),
                (18, 5),
            ]],
            Action::ToggleDevTools => &[
                &[(13, 11), (7, 17), (13, 23)],
                &[(20, 9), (16, 25)],
                &[(23, 11), (29, 17), (23, 23)],
            ],
            _ => &[],
        };
        let saved = matches!(action, Action::ToggleBookmark) && self.is_bookmarked();
        let active_tool = matches!(action, Action::ToggleDevTools) && self.devtools.open;
        for path in paths {
            for pair in path.windows(2) {
                icon_line(
                    c,
                    (x + pair[0].0, 14 + pair[0].1),
                    (x + pair[1].0, 14 + pair[1].1),
                    if saved || active_tool {
                        p.accent
                    } else {
                        color
                    },
                );
            }
        }
        if saved {
            c.rect(x + 15, 28, 7, 8, p.accent);
        }
        if matches!(action, Action::Menu) && self.restart_available {
            c.rect(x + 28, 17, 5, 5, p.accent);
        }
        if enabled {
            self.hits.push(Hit {
                x,
                y: 14,
                w: 36,
                h: 34,
                action,
            });
        }
    }
    fn button(&mut self, c: &mut Canvas, x: i32, y: i32, w: u32, text: &str, action: Action) {
        let palette = self.effective_theme().palette();
        c.rect(x, y, w, 34, palette.button);
        c.text(&mut self.fonts, x + 10, y + 7, text, 15., palette.ink);
        self.hits.push(Hit {
            x,
            y,
            w,
            h: 34,
            action,
        });
    }
    fn dialog_button(
        &mut self,
        c: &mut Canvas,
        position: (i32, i32),
        w: u32,
        text: &str,
        action: Action,
        index: usize,
    ) {
        let (x, y) = position;
        if self.modal_focus == index {
            c.rect(
                x - 2,
                y - 2,
                w + 4,
                38,
                self.effective_theme().palette().accent,
            );
        }
        self.button(c, x, y, w, text, action);
    }
    pub(super) fn paint_chrome(&mut self, c: &mut Canvas) {
        // Paint browser chrome last so scrolled content cannot overpaint it.
        let http = self.is_http_page();
        let palette = self.effective_theme().palette();
        c.rect(
            0,
            0,
            self.width,
            TOP as u32,
            if http {
                HTTP_CHROME
            } else {
                palette.background
            },
        );
        self.toolbar_button(c, self.width as i32 - 46, Action::Menu, true);
        self.toolbar_button(
            c,
            10,
            Action::Back,
            self.history_at > 0 && self.inflight < 2,
        );
        self.toolbar_button(
            c,
            54,
            Action::Forward,
            self.history_at + 1 < self.history.len() && self.inflight < 2,
        );
        if self.loading {
            self.toolbar_button(c, 98, Action::StopLoading, true);
        } else {
            self.toolbar_button(c, 98, Action::Reload, self.page_url != "about:blank");
        }
        self.toolbar_button(
            c,
            142,
            Action::ToggleBookmark,
            self.current_bookmark().is_some(),
        );
        if self.width >= 480 {
            self.toolbar_button(c, self.width as i32 - 90, Action::ToggleDevTools, true);
        }
        let aw = self
            .width
            .saturating_sub(if self.width >= 480 { 296 } else { 252 });
        c.rect(196, 12, aw, 39, palette.field);
        let avail_addr = aw as f32 - 20.;
        if self.focus == Focus::Address && !self.select_all {
            self.ensure_address_range_synced();
            let cur_b = clipboard::clamp_utf8(&self.address, self.selection.address_cursor);
            let sel =
                clipboard::normalized_range(&self.address, cur_b, self.selection.address_anchor);
            let (shown, sx0, sx1, cx) = clipboard::slice_line_bytes(
                &mut self.fonts,
                &self.address,
                16.,
                avail_addr,
                sel,
                cur_b,
            );
            if sx1 > sx0 {
                let sw = ((sx1 - sx0) as u32 + 2).min(aw.saturating_sub(14));
                c.rect(205 + sx0, 20, sw, 25, palette.selection);
            }
            c.text(&mut self.fonts, 206, 22, &shown, 16., palette.ink);
            let caret_x = (206 + cx).min(196 + aw as i32 - 6);
            c.rect(caret_x, 21, 2, 22, palette.accent);
        } else {
            let address = fit_tail(&mut self.fonts, &self.address, 16., avail_addr);
            if self.focus == Focus::Address && self.select_all && !address.is_empty() {
                let width =
                    (self.fonts.width(&address, 16.).ceil() as u32 + 4).min(aw.saturating_sub(14));
                c.rect(204, 20, width, 25, palette.selection);
            }
            c.text(&mut self.fonts, 206, 22, &address, 16., palette.ink);
        }
        self.hits.push(Hit {
            x: 196,
            y: 12,
            w: aw,
            h: 39,
            action: Action::Address,
        });
        c.rect(0, TOP - 1, self.width, 1, palette.border);
        if self.bookmarks_bar && self.height >= 320 {
            self.paint_bookmarks_bar(c);
        }
        c.rect(
            0,
            self.height as i32 - 29,
            self.width,
            29,
            palette.background,
        );
        let has_hovered_link = self.hovered_link().is_some();
        let raw_status = self
            .hovered_link()
            .map(|url| devtools::bounded(url, 512))
            .unwrap_or_else(|| self.status.clone());
        let status = fit_head(&mut self.fonts, &raw_status, 12., self.width as f32 - 20.);
        c.text(
            &mut self.fonts,
            10,
            self.height as i32 - 22,
            &status,
            12.,
            if has_hovered_link {
                palette.ink
            } else {
                palette.muted
            },
        );
        let ptop = self.page_top();
        let view_h = self.viewport_height() as i32;
        if self.content_height > ptop + view_h + 29 {
            let area = view_h.saturating_sub(1).max(1);
            let thumb = (area * area / (self.content_height - ptop).max(1)).max(20);
            let sy = ptop + self.scroll * area / (self.content_height - ptop).max(1);
            c.rect(
                self.page_viewport_width() as i32 - 9,
                sy.min(ptop + view_h - thumb.min(view_h)),
                6,
                (thumb as u32).min(view_h.max(1) as u32),
                palette.scrollbar,
            );
        }
        if !self.modal_open() {
            if self.find.open {
                self.paint_find_bar(c);
            } else {
                self.find.input_rect = None;
            }
            if self.omnibox.open && self.focus == Focus::Address && self.height >= 280 {
                self.paint_omnibox_dropdown(c, 196, aw);
            }
        } else {
            self.find.input_rect = None;
        }
        if self.modal_open() {
            // A popup cannot dispatch clicks into the covered page.
            self.hits.clear();
        }
        if self.menu_open {
            self.toolbar_button(c, self.width as i32 - 46, Action::Menu, true);
        }
        if self.menu_open {
            let x = self.width as i32 - 226;
            let y = 54;
            let full_menu = self.height >= 450;
            let show_devtools = self.height >= 280;
            let menu_h = if full_menu {
                352
            } else if show_devtools {
                216
            } else {
                182
            };
            c.rect(x - 8, y, 228, menu_h, palette.border);
            self.dialog_button(c, (x, y + 6), 212, "About mgbrowser", Action::About, 0);
            self.dialog_button(
                c,
                (x, y + 40),
                212,
                if self.restart_available {
                    "Update ready: restart..."
                } else {
                    "Check for updates"
                },
                Action::Update,
                1,
            );
            self.dialog_button(c, (x, y + 74), 212, "Settings", Action::Settings, 2);
            self.dialog_button(c, (x, y + 108), 212, "Bookmarks", Action::Bookmarks, 3);
            if full_menu {
                self.dialog_button(c, (x, y + 142), 212, "History", Action::History, 4);
                self.dialog_button(c, (x, y + 176), 212, "Downloads", Action::Downloads, 5);
                self.dialog_button(c, (x, y + 210), 212, "Find in page", Action::FindBar, 6);
                self.dialog_button(
                    c,
                    (x, y + 244),
                    212,
                    "Clear browsing data",
                    Action::ClearBrowsingData,
                    7,
                );
                self.dialog_button(
                    c,
                    (x, y + 278),
                    212,
                    "Developer tools",
                    Action::ToggleDevTools,
                    8,
                );
                self.dialog_button(c, (x, y + 312), 212, "Close", Action::CloseMenu, 9);
            } else if show_devtools {
                self.dialog_button(
                    c,
                    (x, y + 142),
                    212,
                    "Developer tools",
                    Action::ToggleDevTools,
                    4,
                );
                self.dialog_button(c, (x, y + 176), 212, "Close", Action::CloseMenu, 5);
            } else {
                self.dialog_button(c, (x, y + 142), 212, "Close", Action::CloseMenu, 4);
            }
        }
        if self.bookmarks_open {
            self.paint_bookmarks(c);
        }
        if self.history_open {
            self.paint_history(c);
        }
        if self.downloads_open {
            self.paint_downloads(c);
        }
        if self.about_open {
            let w = self.width.saturating_sub(40).min(700);
            let x = (self.width - w) as i32 / 2;
            let compact = self.height < 450;
            let h = if compact { 232 } else { 300 };
            let y = 125.min(self.height.saturating_sub(h + 4) as i32);
            let line_size = if compact { 14. } else { 16. };
            c.rect(x, y, w, h, palette.border);
            c.rect(x + 2, y + 2, w - 4, h - 4, palette.panel);
            for (i, line) in self.about_lines.iter().enumerate() {
                let text = fit_head(&mut self.fonts, line, line_size, w as f32 - 32.);
                c.text(
                    &mut self.fonts,
                    x + 16,
                    y + if compact {
                        12 + i as i32 * 22
                    } else {
                        20 + i as i32 * 30
                    },
                    &text,
                    line_size,
                    palette.ink,
                );
            }
            let status = fit_head(&mut self.fonts, &self.update_status, 13., w as f32 - 32.);
            c.text(
                &mut self.fonts,
                x + 16,
                y + if compact { 110 } else { 163 },
                &status,
                13.,
                palette.ink,
            );
            if let Some((received, total)) = self.update_progress {
                let width = w - 32;
                let bar_y = y + if compact { 127 } else { 184 };
                c.rect(x + 16, bar_y, width, 6, palette.border);
                if let Some(total) = total {
                    let filled = (received as u128 * u128::from(width) / total as u128) as u32;
                    c.rect(x + 16, bar_y, filled, 6, palette.accent);
                } else {
                    // Unknown content length: moving segment, not a fake percentage.
                    let segment = width / 5;
                    let offset = ((received / 8192) % (width - segment).max(1) as usize) as i32;
                    c.rect(x + 16 + offset, bar_y, segment, 6, palette.accent);
                }
            }
            let note = if self.restart_available {
                if self.restart_tab_count > 1 {
                    "Reopens all tab URLs. Layout, edits,"
                } else {
                    "Reopens page. Unsaved edits and"
                }
            } else if compact {
                "Restart after updating. Escape closes."
            } else {
                "Updates take effect after restart. Escape closes."
            };
            let note = fit_head(&mut self.fonts, note, 13., w as f32 - 32.);
            c.text(
                &mut self.fonts,
                x + 16,
                y + if compact { 134 } else { 195 },
                &note,
                13.,
                palette.muted,
            );
            if self.restart_available {
                c.text(
                    &mut self.fonts,
                    x + 16,
                    y + if compact { 153 } else { 214 },
                    if self.restart_tab_count > 1 {
                        "cookies and history will be lost."
                    } else {
                        "memory-only cookies will be lost."
                    },
                    13.,
                    palette.muted,
                );
            }
            self.dialog_button(
                c,
                (x + 16, y + if compact { 180 } else { 242 }),
                190,
                if self.restart_available {
                    "Restart now"
                } else {
                    "Check for updates"
                },
                self.update_button_action(),
                0,
            );
            self.dialog_button(
                c,
                (x + 218, y + if compact { 180 } else { 242 }),
                70,
                "Close",
                Action::CloseMenu,
                1,
            );
        }
        if self.settings_open {
            let w = self.width.saturating_sub(40).min(600);
            let x = (self.width - w) as i32 / 2;
            let compact = self.height < 320;
            let h = if compact { 232 } else { 304 };
            let y = 125.min(self.height.saturating_sub(h + 4) as i32);
            let theme_y = if compact { 60 } else { 76 };
            let size_y = if compact { 121 } else { 148 };
            let close_y = if compact { 188 } else { 254 };
            c.rect(x, y, w, h, palette.border);
            c.rect(x + 2, y + 2, w - 4, h - 4, palette.panel);
            c.text(
                &mut self.fonts,
                x + 16,
                y + if compact { 8 } else { 14 },
                "Settings: appearance",
                19.,
                palette.ink,
            );
            let current = format!(
                "Theme: {:?}  /  Using: {:?}",
                self.theme_preference,
                self.effective_theme()
            );
            let current = fit_head(&mut self.fonts, &current, 14., w as f32 - 32.);
            c.text(
                &mut self.fonts,
                x + 16,
                y + if compact { 36 } else { 46 },
                &current,
                14.,
                palette.muted,
            );
            let bw = (w - 48) / 3;
            for (index, (label, preference)) in [
                ("System", ThemePreference::System),
                ("Light", ThemePreference::Light),
                ("Dark", ThemePreference::Dark),
            ]
            .into_iter()
            .enumerate()
            {
                let bx = x + 16 + index as i32 * (bw as i32 + 8);
                self.dialog_button(
                    c,
                    (bx, y + theme_y),
                    bw,
                    label,
                    Action::Theme(preference),
                    index,
                );
                if self.theme_preference == preference {
                    c.rect(bx + 4, y + theme_y + 29, bw - 8, 3, palette.accent);
                }
            }
            c.text(
                &mut self.fonts,
                x + 16,
                y + size_y - 22,
                "Size: browser controls and pages",
                13.,
                palette.muted,
            );
            self.dialog_button(
                c,
                (x + 16, y + size_y),
                90,
                "System",
                Action::Scale(ScalePreference::System),
                3,
            );
            if self.scale_preference == ScalePreference::System {
                c.rect(x + 20, y + size_y + 29, 82, 3, palette.accent);
            }
            self.dialog_button(c, (x + 114, y + size_y), 34, "-", Action::ScaleDown, 4);
            c.rect(x + 156, y + size_y, 84, 34, palette.field);
            let size_label = format!("{}%", self.effective_scale_percent());
            c.text(
                &mut self.fonts,
                x + 169,
                y + size_y + 7,
                &size_label,
                15.,
                palette.ink,
            );
            self.dialog_button(c, (x + 248, y + size_y), 34, "+", Action::ScaleUp, 5);
            if !compact && w >= 560 {
                let search_label = format!("Search: {}", self.search_engine.label());
                self.dialog_button(
                    c,
                    (x + 296, y + size_y),
                    152,
                    &search_label,
                    Action::CycleSearchEngine,
                    7,
                );
                let bar_label = if self.bookmarks_bar {
                    "Bar: Shown"
                } else {
                    "Bar: Hidden"
                };
                self.dialog_button(
                    c,
                    (x + 456, y + size_y),
                    128,
                    bar_label,
                    Action::ToggleBookmarksBar,
                    8,
                );
            }
            if !compact {
                c.text(
                    &mut self.fonts,
                    x + 16,
                    y + 194,
                    "System follows desktop size and theme.",
                    13.,
                    palette.muted,
                );
            }
            let status = fit_head(&mut self.fonts, &self.settings_status, 13., w as f32 - 32.);
            c.text(
                &mut self.fonts,
                x + 16,
                y + if compact { 164 } else { 218 },
                &status,
                13.,
                palette.muted,
            );
            self.dialog_button(c, (x + 16, y + close_y), 80, "Close", Action::CloseMenu, 6);
            let hint = fit_head(
                &mut self.fonts,
                "Tab / arrows, Enter. Esc closes.",
                12.,
                w as f32 - 132.,
            );
            c.text(
                &mut self.fonts,
                x + 112,
                y + close_y + 11,
                &hint,
                12.,
                palette.muted,
            );
        }
    }
    fn paint_bookmarks(&mut self, c: &mut Canvas) {
        let p = self.effective_theme().palette();
        let w = self.width.saturating_sub(40).min(760);
        let h = self.height.saturating_sub(40).min(540);
        let x = (self.width - w) as i32 / 2;
        let y = (self.height - h) as i32 / 2;
        self.bookmark_page = self
            .bookmark_page
            .min(self.bookmarks.len().saturating_sub(1) / self.bookmark_rows());
        c.rect(x, y, w, h, p.border);
        c.rect(x + 2, y + 2, w - 4, h - 4, p.panel);
        c.text(
            &mut self.fonts,
            x + 14,
            y + 12,
            &format!("Bookmarks ({})", self.bookmarks.len()),
            18.,
            p.ink,
        );
        let status = fit_head(&mut self.fonts, &self.bookmark_status, 12., w as f32 - 28.);
        c.text(&mut self.fonts, x + 14, y + 38, &status, 12., p.muted);
        let start = self.bookmark_page * self.bookmark_rows();
        let mut index = 0;
        if self.bookmarks.is_empty() {
            let hint = fit_head(
                &mut self.fonts,
                "No bookmarks yet. Use the star or Ctrl+D.",
                14.,
                w as f32 - 28.,
            );
            c.text(&mut self.fonts, x + 14, y + 68, &hint, 14., p.ink);
        }
        for i in start..(start + self.bookmark_rows()).min(self.bookmarks.len()) {
            let by = y + 64 + (i - start) as i32 * 48;
            let title = fit_head(
                &mut self.fonts,
                &self.bookmarks[i].title,
                15.,
                w as f32 - 130.,
            );
            self.dialog_button(
                c,
                (x + 14, by),
                w - 112,
                &title,
                Action::OpenBookmark(i),
                index,
            );
            index += 1;
            self.dialog_button(
                c,
                (x + w as i32 - 88, by),
                74,
                "Remove",
                Action::RemoveBookmark(i),
                index,
            );
            index += 1;
            let url = fit_head(&mut self.fonts, &self.bookmarks[i].url, 10., w as f32 - 32.);
            c.text(&mut self.fonts, x + 16, by + 34, &url, 10., p.muted);
        }
        let fy = y + h as i32 - 46;
        if self.bookmark_page > 0 {
            self.dialog_button(
                c,
                (x + 14, fy),
                74,
                "Previous",
                Action::BookmarkPage(false),
                index,
            );
            index += 1;
        }
        if start + self.bookmark_rows() < self.bookmarks.len() {
            self.dialog_button(
                c,
                (x + 100, fy),
                64,
                "Next",
                Action::BookmarkPage(true),
                index,
            );
            index += 1;
        }
        self.dialog_button(
            c,
            (x + w as i32 - 88, fy),
            74,
            "Close",
            Action::CloseMenu,
            index,
        );
    }

    fn paint_bookmarks_bar(&mut self, c: &mut Canvas) {
        let p = self.effective_theme().palette();
        c.rect(0, TOP, self.width, 28, p.background);
        c.rect(0, TOP + 27, self.width, 1, p.border);
        if self.bookmarks.is_empty() {
            let hint = fit_head(
                &mut self.fonts,
                "Bookmarks bar · Press Ctrl+D to bookmark pages, or Ctrl+Shift+B to hide",
                12.,
                self.width as f32 - 24.,
            );
            c.text(&mut self.fonts, 12, TOP + 7, &hint, 12., p.muted);
            return;
        }
        let mut bx = 10i32;
        let max_x = self.width as i32 - 12;
        for i in 0..self.bookmarks.len() {
            if bx + 72 > max_x {
                break;
            }
            let avail = (max_x - bx - 16).min(160) as f32;
            let label = fit_head(&mut self.fonts, &self.bookmarks[i].title, 12., avail);
            let bw = (self.fonts.width(&label, 12.).ceil() as u32 + 16).clamp(48, 176);
            if bx + bw as i32 > max_x {
                break;
            }
            c.rect(bx, TOP + 3, bw, 21, p.button);
            c.text(&mut self.fonts, bx + 8, TOP + 6, &label, 12., p.ink);
            self.hits.push(Hit {
                x: bx,
                y: TOP + 3,
                w: bw,
                h: 21,
                action: Action::OpenBookmark(i),
            });
            bx += bw as i32 + 6;
        }
    }

    fn paint_omnibox_dropdown(&mut self, c: &mut Canvas, x: i32, w: u32) {
        if self.omnibox.suggestions.is_empty() || w < 120 {
            return;
        }
        let p = self.effective_theme().palette();
        let row_h = 30i32;
        let max_rows = ((self.height as i32 - 90) / row_h).clamp(1, 6) as usize;
        let count = self.omnibox.suggestions.len().min(max_rows);
        if count == 0 {
            return;
        }
        let y = 52;
        let h = count as u32 * row_h as u32 + 4;
        c.rect(x, y, w, h, p.border);
        c.rect(x + 1, y + 1, w - 2, h - 2, p.panel);
        for idx in 0..count {
            let ry = y + 2 + idx as i32 * row_h;
            let selected = self.omnibox.selected == Some(idx);
            let suggestion = self.omnibox.suggestions[idx].clone();
            if selected {
                c.rect(x + 2, ry, w - 4, row_h as u32, p.selection);
            }
            let prefix = match suggestion.kind {
                omnibox::SuggestionKind::Search => "? ",
                omnibox::SuggestionKind::Navigate => "-> ",
                omnibox::SuggestionKind::Bookmark => "* ",
                omnibox::SuggestionKind::History => "H ",
                omnibox::SuggestionKind::Internal => "mg ",
            };
            let full = format!("{prefix}{}", suggestion.label);
            let shown = fit_head(&mut self.fonts, &full, 13., w as f32 - 20.);
            c.text(&mut self.fonts, x + 10, ry + 7, &shown, 13., p.ink);
            self.hits.push(Hit {
                x: x + 2,
                y: ry,
                w: w - 4,
                h: row_h as u32,
                action: Action::OmniboxSelect(suggestion.target),
            });
        }
    }

    fn paint_find_bar(&mut self, c: &mut Canvas) {
        let p = self.effective_theme().palette();
        let page_w = self.page_viewport_width();
        let w = page_w.saturating_sub(20).min(356);
        if w < 160 {
            self.find.input_rect = None;
            return;
        }
        let h = 36u32;
        let x = (page_w as i32 - w as i32 - 14).max(6);
        let y = self.page_top() + 8;
        c.rect(x, y, w, h, p.border);
        c.rect(x + 1, y + 1, w - 2, h - 2, p.panel);

        let btn_area = 94u32;
        let count_str = if self.find.query.is_empty() {
            String::new()
        } else {
            let (cur, total) = self.find_match_count();
            format!("{cur}/{total}")
        };
        let count_w = if count_str.is_empty() {
            0u32
        } else {
            (self.fonts.width(&count_str, 12.).ceil() as u32 + 10).min(64)
        };
        let input_w = w.saturating_sub(btn_area + count_w + 14).max(52);
        let ix = x + 6;
        let iy = y + 5;
        let ih = 26u32;
        c.rect(ix, iy, input_w, ih, p.field);
        self.find.input_rect = Some((ix, iy, input_w, ih));

        let avail = input_w as f32 - 12.;
        if self.focus == Focus::FindInput && !self.select_all {
            let cur_b = clipboard::clamp_utf8(&self.find.query, self.find.cursor);
            let sel = clipboard::normalized_range(&self.find.query, cur_b, self.find.anchor);
            let (shown, sx0, sx1, cx) = clipboard::slice_line_bytes(
                &mut self.fonts,
                &self.find.query,
                13.,
                avail,
                sel,
                cur_b,
            );
            if sx1 > sx0 {
                let sw = ((sx1 - sx0) as u32 + 2).min(input_w.saturating_sub(8));
                c.rect(ix + 5 + sx0, iy + 3, sw, 20, p.selection);
            }
            c.text(&mut self.fonts, ix + 6, iy + 5, &shown, 13., p.ink);
            let caret_x = (ix + 6 + cx).min(ix + input_w as i32 - 4);
            c.rect(caret_x, iy + 4, 2, 18, p.accent);
        } else if self.find.query.is_empty() {
            let ph = fit_head(&mut self.fonts, "Find in page...", 13., avail);
            c.text(&mut self.fonts, ix + 6, iy + 5, &ph, 13., p.muted);
            if self.focus == Focus::FindInput {
                c.rect(ix + 6, iy + 4, 2, 18, p.accent);
            }
        } else {
            let shown = fit_tail(&mut self.fonts, &self.find.query, 13., avail);
            if self.focus == Focus::FindInput && self.select_all {
                let sw =
                    (self.fonts.width(&shown, 13.).ceil() as u32 + 4).min(input_w.saturating_sub(8));
                c.rect(ix + 4, iy + 3, sw, 20, p.selection);
            }
            c.text(&mut self.fonts, ix + 6, iy + 5, &shown, 13., p.ink);
        }
        self.hits.push(Hit {
            x: ix,
            y: iy,
            w: input_w,
            h: ih,
            action: Action::FindBar,
        });

        if !count_str.is_empty() {
            let count_color = if self.find.matches.is_empty() {
                HTTP_CHROME
            } else {
                p.muted
            };
            c.text(
                &mut self.fonts,
                ix + input_w as i32 + 4,
                y + 11,
                &count_str,
                12.,
                count_color,
            );
        }

        let bx = x + w as i32 - 90;
        for (idx, (lbl, act)) in [
            ("<", Action::FindStep(false)),
            (">", Action::FindStep(true)),
            ("x", Action::FindClose),
        ]
        .into_iter()
        .enumerate()
        {
            let rx = bx + idx as i32 * 29;
            c.rect(rx, iy, 26, ih, p.button);
            c.text(&mut self.fonts, rx + 9, iy + 5, lbl, 13., p.ink);
            self.hits.push(Hit {
                x: rx,
                y: iy,
                w: 26,
                h: ih,
                action: act,
            });
        }
    }

    fn paint_history(&mut self, c: &mut Canvas) {
        let p = self.effective_theme().palette();
        let w = self.width.saturating_sub(40).min(760);
        let h = self.height.saturating_sub(40).min(540);
        let x = (self.width - w) as i32 / 2;
        let y = (self.height - h) as i32 / 2;
        let rows = self.bookmark_rows();
        self.history_page = self
            .history_page
            .min(self.browsing_history.len().saturating_sub(1) / rows);
        c.rect(x, y, w, h, p.border);
        c.rect(x + 2, y + 2, w - 4, h - 4, p.panel);
        c.text(
            &mut self.fonts,
            x + 14,
            y + 12,
            &format!("History ({})", self.browsing_history.len()),
            18.,
            p.ink,
        );
        let sub = fit_head(
            &mut self.fonts,
            "Recent HTTP(S) visits · Ctrl+H toggles · Clear browsing data wipes all",
            12.,
            w as f32 - 28.,
        );
        c.text(&mut self.fonts, x + 14, y + 38, &sub, 12., p.muted);
        let start = self.history_page * rows;
        let mut index = 0;
        if self.browsing_history.is_empty() {
            let hint = fit_head(
                &mut self.fonts,
                "No browsing history recorded yet.",
                14.,
                w as f32 - 28.,
            );
            c.text(&mut self.fonts, x + 14, y + 68, &hint, 14., p.ink);
        }
        for i in start..(start + rows).min(self.browsing_history.len()) {
            let by = y + 64 + (i - start) as i32 * 48;
            let title = fit_head(
                &mut self.fonts,
                &self.browsing_history[i].title,
                15.,
                w as f32 - 130.,
            );
            self.dialog_button(
                c,
                (x + 14, by),
                w - 112,
                &title,
                Action::OpenHistory(i),
                index,
            );
            index += 1;
            self.dialog_button(
                c,
                (x + w as i32 - 88, by),
                74,
                "Remove",
                Action::RemoveHistory(i),
                index,
            );
            index += 1;
            let detail = format!(
                "{} · {} visit{}",
                self.browsing_history[i].url,
                self.browsing_history[i].visits,
                if self.browsing_history[i].visits == 1 {
                    ""
                } else {
                    "s"
                }
            );
            let url = fit_head(&mut self.fonts, &detail, 10., w as f32 - 32.);
            c.text(&mut self.fonts, x + 16, by + 34, &url, 10., p.muted);
        }
        let fy = y + h as i32 - 46;
        if self.history_page > 0 {
            self.dialog_button(
                c,
                (x + 14, fy),
                74,
                "Previous",
                Action::HistoryPage(false),
                index,
            );
            index += 1;
        }
        if start + rows < self.browsing_history.len() {
            self.dialog_button(
                c,
                (x + 100, fy),
                64,
                "Next",
                Action::HistoryPage(true),
                index,
            );
            index += 1;
        }
        if !self.browsing_history.is_empty() && w >= 360 {
            self.dialog_button(
                c,
                (x + 176, fy),
                96,
                "Clear all",
                Action::ClearBrowsingData,
                index,
            );
            index += 1;
        }
        self.dialog_button(
            c,
            (x + w as i32 - 88, fy),
            74,
            "Close",
            Action::CloseMenu,
            index,
        );
    }

    fn paint_downloads(&mut self, c: &mut Canvas) {
        let p = self.effective_theme().palette();
        let w = self.width.saturating_sub(40).min(760);
        let h = self.height.saturating_sub(40).min(540);
        let x = (self.width - w) as i32 / 2;
        let y = (self.height - h) as i32 / 2;
        let rows = self.bookmark_rows();
        self.download_page = self
            .download_page
            .min(self.downloads.len().saturating_sub(1) / rows);
        c.rect(x, y, w, h, p.border);
        c.rect(x + 2, y + 2, w - 4, h - 4, p.panel);
        c.text(
            &mut self.fonts,
            x + 14,
            y + 12,
            &format!("Downloads ({})", self.downloads.len()),
            18.,
            p.ink,
        );
        let sub = fit_head(
            &mut self.fonts,
            "Saved to Downloads directory · Ctrl+J toggles · Ctrl+S saves current page",
            12.,
            w as f32 - 28.,
        );
        c.text(&mut self.fonts, x + 14, y + 38, &sub, 12., p.muted);
        let start = self.download_page * rows;
        let total = self.downloads.len();
        if total == 0 {
            let hint = fit_head(
                &mut self.fonts,
                "No downloads yet in this session.",
                14.,
                w as f32 - 28.,
            );
            c.text(&mut self.fonts, x + 14, y + 68, &hint, 14., p.ink);
        }
        for offset in 0..rows.min(total.saturating_sub(start)) {
            let dl_idx = total - 1 - (start + offset);
            let dl = &self.downloads[dl_idx];
            let by = y + 64 + offset as i32 * 48;
            c.rect(x + 14, by, w - 28, 40, p.field);
            let state_tag = match dl.state {
                downloads::DownloadState::InProgress => "Downloading...".to_owned(),
                downloads::DownloadState::Completed => format!(
                    "Saved ({} B) -> {}",
                    dl.bytes,
                    dl.saved_path.as_deref().unwrap_or("Downloads")
                ),
                downloads::DownloadState::Failed => format!(
                    "Failed: {}",
                    dl.error.as_deref().unwrap_or("error")
                ),
            };
            let title = fit_head(&mut self.fonts, &dl.filename, 14., w as f32 - 44.);
            c.text(&mut self.fonts, x + 22, by + 5, &title, 14., p.ink);
            let detail = fit_head(&mut self.fonts, &state_tag, 11., w as f32 - 44.);
            c.text(&mut self.fonts, x + 22, by + 23, &detail, 11., p.muted);
        }
        let fy = y + h as i32 - 46;
        let mut index = 0;
        if self.download_page > 0 {
            self.dialog_button(
                c,
                (x + 14, fy),
                74,
                "Previous",
                Action::DownloadPage(false),
                index,
            );
            index += 1;
        }
        if start + rows < total {
            self.dialog_button(
                c,
                (x + 100, fy),
                64,
                "Next",
                Action::DownloadPage(true),
                index,
            );
            index += 1;
        }
        if !self.downloads.is_empty() && w >= 360 {
            self.dialog_button(
                c,
                (x + 176, fy),
                96,
                "Clear list",
                Action::ClearDownloads,
                index,
            );
            index += 1;
        }
        self.dialog_button(
            c,
            (x + w as i32 - 88, fy),
            74,
            "Close",
            Action::CloseMenu,
            index,
        );
    }
}

fn icon_line(c: &mut Canvas, a: (i32, i32), b: (i32, i32), color: u32) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
    for step in 0..=steps {
        c.rect(
            a.0 + (b.0 - a.0) * step / steps,
            a.1 + (b.1 - a.1) * step / steps,
            2,
            2,
            color,
        );
    }
}
