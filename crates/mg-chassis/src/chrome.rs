//! Optional browser controls; page pixels come from Sparkle.
use super::*;
impl Browser {
    fn button(&mut self, c: &mut Canvas, x: i32, y: i32, w: u32, text: &str, action: Action) {
        c.rect(x, y, w, 34, 0xe3e9df);
        c.text(&mut self.fonts, x + 10, y + 7, text, 15., INK);
        self.hits.push(Hit {
            x,
            y,
            w,
            h: 34,
            action,
        });
    }
    pub(super) fn paint_chrome(&mut self, c: &mut Canvas) {
        // Paint browser chrome last so scrolled content cannot overpaint it.
        let http = self.is_http_page();
        c.rect(
            0,
            0,
            self.width,
            TOP as u32,
            if http { HTTP_CHROME } else { 0xeef1e9 },
        );
        self.button(c, 14, 14, 54, "Back", Action::Back);
        self.button(c, 76, 14, 70, "Next", Action::Forward);
        self.button(c, 154, 14, 76, "Reload", Action::Reload);
        let aw = self.width.saturating_sub(255);
        c.rect(240, 12, aw, 39, 0xffffff);
        if self.focus == Focus::Address && self.select_all {
            c.rect(247, 20, aw.saturating_sub(14), 25, 0xc6dfed);
        }
        let address = fit_tail(&mut self.fonts, &self.address, 16., aw as f32 - 20.);
        c.text(&mut self.fonts, 250, 22, &address, 16., INK);
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
            if http { 0xffffff } else { 0x345c36 },
        );
        let visible_title = self.visible_title();
        let title = fit_head(
            &mut self.fonts,
            &visible_title,
            16.,
            self.width as f32 - 185.,
        );
        c.text(
            &mut self.fonts,
            160,
            68,
            &title,
            16.,
            if http { 0xffffff } else { INK },
        );
        c.rect(0, TOP - 1, self.width, 1, 0xc4cebd);
        c.rect(0, self.height as i32 - 29, self.width, 29, 0xeef1e9);
        let status = fit_head(&mut self.fonts, &self.status, 12., self.width as f32 - 20.);
        c.text(
            &mut self.fonts,
            10,
            self.height as i32 - 22,
            &status,
            12.,
            0x53614f,
        );
        if self.content_height > self.height as i32 {
            let area = (self.height as i32 - TOP - 30).max(1);
            let thumb = (area * area / (self.content_height - TOP).max(1)).max(20);
            let sy = TOP + self.scroll * area / (self.content_height - TOP).max(1);
            c.rect(self.width as i32 - 9, sy, 6, thumb as u32, 0x8b9c83);
        }
    }
}
