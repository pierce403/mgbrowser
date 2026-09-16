//! Optional browser controls; page pixels come from Sparkle.
use super::*;
impl Browser {
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
        self.button(c, 14, 14, 54, "Back", Action::Back);
        self.button(c, 76, 14, 70, "Next", Action::Forward);
        self.button(c, 154, 14, 76, "Reload", Action::Reload);
        let aw = self.width.saturating_sub(255);
        c.rect(240, 12, aw, 39, palette.field);
        if self.focus == Focus::Address && self.select_all {
            c.rect(247, 20, aw.saturating_sub(14), 25, palette.selection);
        }
        let address = fit_tail(&mut self.fonts, &self.address, 16., aw as f32 - 20.);
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
            self.width as f32 - 285.,
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
        let menu = if self.update_status.starts_with("Installed ") {
            "Menu *"
        } else {
            "Menu"
        };
        self.button(c, self.width as i32 - 100, 62, 86, menu, Action::Menu);
        if self.menu_open {
            let x = self.width as i32 - 234;
            let y = 102.min(self.height.saturating_sub(170) as i32);
            c.rect(x - 8, y, 228, 166, palette.border);
            self.dialog_button(c, (x, y + 6), 212, "About mgbrowser", Action::About, 0);
            self.dialog_button(c, (x, y + 46), 212, "Check for updates", Action::Update, 1);
            self.dialog_button(c, (x, y + 86), 212, "Settings", Action::Settings, 2);
            self.dialog_button(c, (x, y + 126), 212, "Close", Action::CloseMenu, 3);
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
            let note = if compact {
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
            self.dialog_button(
                c,
                (x + 16, y + if compact { 180 } else { 242 }),
                190,
                "Check for updates",
                Action::Update,
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
}
