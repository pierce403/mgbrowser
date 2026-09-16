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
            _ => &[],
        };
        let saved = matches!(action, Action::ToggleBookmark) && self.is_bookmarked();
        for path in paths {
            for pair in path.windows(2) {
                icon_line(
                    c,
                    (x + pair[0].0, 14 + pair[0].1),
                    (x + pair[1].0, 14 + pair[1].1),
                    if saved { p.accent } else { color },
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
        self.toolbar_button(c, 10, Action::Menu, true);
        self.toolbar_button(
            c,
            54,
            Action::Back,
            self.history_at > 0 && self.inflight < 2,
        );
        self.toolbar_button(
            c,
            98,
            Action::Forward,
            self.history_at + 1 < self.history.len() && self.inflight < 2,
        );
        self.toolbar_button(c, 142, Action::Reload, self.page_url != "about:blank");
        self.toolbar_button(
            c,
            186,
            Action::ToggleBookmark,
            self.current_bookmark().is_some(),
        );
        let aw = self.width.saturating_sub(255);
        c.rect(240, 12, aw, 39, palette.field);
        let address = fit_tail(&mut self.fonts, &self.address, 16., aw as f32 - 20.);
        if self.focus == Focus::Address && self.select_all && !address.is_empty() {
            let width =
                (self.fonts.width(&address, 16.).ceil() as u32 + 4).min(aw.saturating_sub(14));
            c.rect(248, 20, width, 25, palette.selection);
        }
        c.text(&mut self.fonts, 250, 22, &address, 16., palette.ink);
        self.hits.push(Hit {
            x: 240,
            y: 12,
            w: aw,
            h: 39,
            action: Action::Address,
        });
        c.text(
            &mut self.fonts,
            18,
            66,
            "mgbrowser",
            19.,
            if http { 0xffffff } else { palette.accent },
        );
        let visible_title = self.visible_title();
        let title = fit_head(
            &mut self.fonts,
            &visible_title,
            16.,
            self.width as f32 - 180.,
        );
        c.text(
            &mut self.fonts,
            160,
            68,
            &title,
            16.,
            if http { 0xffffff } else { palette.ink },
        );
        c.rect(0, TOP - 1, self.width, 1, palette.border);
        c.rect(
            0,
            self.height as i32 - 29,
            self.width,
            29,
            palette.background,
        );
        let status = fit_head(&mut self.fonts, &self.status, 12., self.width as f32 - 20.);
        c.text(
            &mut self.fonts,
            10,
            self.height as i32 - 22,
            &status,
            12.,
            palette.muted,
        );
        if self.content_height > self.height as i32 {
            let area = (self.height as i32 - TOP - 30).max(1);
            let thumb = (area * area / (self.content_height - TOP).max(1)).max(20);
            let sy = TOP + self.scroll * area / (self.content_height - TOP).max(1);
            c.rect(
                self.width as i32 - 9,
                sy,
                6,
                thumb as u32,
                palette.scrollbar,
            );
        }
        if self.modal_open() {
            // A popup cannot dispatch clicks into the covered page.
            self.hits.clear();
        }
        if self.menu_open {
            self.toolbar_button(c, 10, Action::Menu, true);
        }
        if self.menu_open {
            let x = 14;
            let y = 54;
            c.rect(x - 8, y, 228, 182, palette.border);
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
            self.dialog_button(c, (x, y + 142), 212, "Close", Action::CloseMenu, 4);
        }
        if self.bookmarks_open {
            self.paint_bookmarks(c);
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
            let note = if self.restart_available {
                "Reopens page. Unsaved edits and"
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
                    "memory-only cookies will be lost.",
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
