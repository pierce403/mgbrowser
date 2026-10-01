//! Bounded wheel easing. Keyboard, CDP and embedding scrolls remain immediate.
use super::*;

const DURATION: Duration = Duration::from_millis(150);

pub(super) struct ScrollMotion {
    from: i32,
    target: i32,
    started: Instant,
}

impl Browser {
    /// Queue a wheel delta in logical pixels. Call `advance_scroll` before painting.
    pub fn wheel_scroll_by(&mut self, amount: i32) {
        if amount == 0 {
            return;
        }
        if self.devtools.open {
            self.inspector_scroll(amount.signum() * 3);
            return;
        }
        self.wheel_scroll_at(amount, Instant::now());
    }

    fn wheel_scroll_at(&mut self, amount: i32, now: Instant) {
        if self.modal_open() || amount == 0 {
            return;
        }
        self.pointer_press = None;
        let previous = self
            .smooth_scroll
            .as_ref()
            .map_or(self.scroll, |m| m.target);
        // Reverse from the visible position, not from a distant queued target.
        let base = if (previous - self.scroll).signum() == amount.signum() {
            previous
        } else {
            self.scroll
        };
        let target = base.saturating_add(amount).clamp(0, self.max_scroll());
        if target == self.scroll {
            self.stop_scroll();
        } else if target != previous || self.smooth_scroll.is_none() {
            self.smooth_scroll = Some(ScrollMotion {
                from: self.scroll,
                target,
                started: now,
            });
        }
    }

    /// Advance using elapsed time, not a frame count. No redraw once motion stops.
    /// Hosts must paint dirty frames before accepting coordinates against them.
    pub fn advance_scroll(&mut self, now: Instant) {
        if self.modal_open() {
            self.stop_scroll();
            return;
        }
        let Some(motion) = &self.smooth_scroll else {
            return;
        };
        let elapsed = now.saturating_duration_since(motion.started);
        let t = (elapsed.as_secs_f64() / DURATION.as_secs_f64()).min(1.);
        let eased = 1. - (1. - t).powi(3);
        let position = (f64::from(motion.from) + f64::from(motion.target - motion.from) * eased)
            .round() as i32;
        let next = position.clamp(0, self.max_scroll());
        if elapsed >= DURATION {
            self.stop_scroll();
        }
        if next != self.scroll {
            self.scroll = next;
            // The next paint may discover a shorter document, including on the
            // final frame after the motion itself has already been cleared.
            self.scroll_reflow_pending = true;
            self.hits.clear();
            self.boxes.clear();
            self.pointer_press = None;
            self.dirty = true;
        }
    }

    pub(super) fn stop_scroll(&mut self) {
        self.smooth_scroll = None;
    }

    pub(super) fn max_scroll(&self) -> i32 {
        self.content_height
            .saturating_sub(self.height as i32)
            .saturating_add(40)
            .max(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Browser {
        let mut app = test_app();
        app.content_height = 3000;
        app
    }

    #[test]
    fn wheel_eases_then_finishes_exactly_without_idle_redraws() {
        let mut app = page();
        let now = Instant::now();
        app.wheel_scroll_at(100, now);
        assert_eq!(app.scroll, 0);
        app.advance_scroll(now + Duration::from_millis(30));
        assert_eq!(app.scroll, 49);
        app.advance_scroll(now + Duration::from_millis(75));
        assert_eq!(app.scroll, 88);
        app.advance_scroll(now + DURATION);
        assert_eq!(app.scroll, 100);
        assert!(app.smooth_scroll.is_none());
        app.dirty = false;
        app.advance_scroll(now + Duration::from_secs(10));
        assert!(!app.dirty);
    }

    #[test]
    fn repeated_wheel_accumulates_but_reverses_from_visible_position() {
        let mut app = page();
        let now = Instant::now();
        app.wheel_scroll_at(100, now);
        app.wheel_scroll_at(100, now);
        assert_eq!(app.smooth_scroll.as_ref().unwrap().target, 200);
        app.advance_scroll(now + Duration::from_millis(30));
        let visible = app.scroll;
        app.wheel_scroll_at(-50, now + Duration::from_millis(30));
        assert_eq!(app.scroll, visible);
        app.advance_scroll(now + Duration::from_millis(180));
        assert_eq!(app.scroll, visible - 50);
    }

    #[test]
    fn bounds_large_deltas_stalls_and_direct_scroll_are_safe() {
        let mut app = page();
        let now = Instant::now();
        app.wheel_scroll_at(i32::MAX, now);
        app.wheel_scroll_at(i32::MAX, now + Duration::from_millis(10));
        assert_eq!(app.smooth_scroll.as_ref().unwrap().started, now);
        app.advance_scroll(now + Duration::from_secs(1));
        assert_eq!(app.scroll, app.max_scroll());
        app.wheel_scroll_at(i32::MIN, now);
        app.advance_scroll(now + DURATION);
        assert_eq!(app.scroll, 0);
        app.wheel_scroll_at(100, now);
        app.scroll_by(70);
        assert_eq!(app.scroll, 70);
        assert!(app.smooth_scroll.is_none());
        app.content_height = 0;
        app.wheel_scroll_at(100, now);
        app.advance_scroll(now + DURATION);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn clicks_keys_resize_and_navigation_cancel_at_the_visible_position() {
        let mut app = page();
        let now = Instant::now();
        app.wheel_scroll_at(100, now);
        app.advance_scroll(now + Duration::from_millis(30));
        let visible = app.scroll;
        app.pointer_down(20, 100);
        app.advance_scroll(now + DURATION);
        assert_eq!(app.scroll, visible);
        assert!(app.smooth_scroll.is_none());
        app.wheel_scroll_at(100, now);
        assert!(app.pointer_press.is_none());
        app.key(0xff1b, false, false, false);
        assert!(app.smooth_scroll.is_none());
        app.wheel_scroll_at(100, now);
        app.resize(800, 600);
        assert!(app.smooth_scroll.is_none());
        app.wheel_scroll_at(100, now);
        app.navigate("invalid://example".into(), None, true);
        assert!(app.smooth_scroll.is_none());
    }

    #[test]
    fn painted_input_remains_clickable_when_motion_is_interrupted() {
        let mut app = page();
        app.resize(900, 240);
        app.document = document::parse(
            &format!(
                "<p>Heading</p><form><input name=q></form>{}",
                "<p>More content</p>".repeat(30)
            ),
            "https://example.test/",
        );
        app.paint();
        let now = Instant::now();
        app.wheel_scroll_at(100, now);
        app.advance_scroll(now + Duration::from_millis(20));
        assert!(
            app.hits.is_empty(),
            "Unpainted geometry must not accept input"
        );
        app.paint();
        let hit = app
            .hits
            .iter()
            .find(|h| matches!(h.action, Action::Input(_)))
            .unwrap()
            .clone();
        let position = app.scroll;
        app.pointer_down(hit.x + 2, hit.y + 2);
        app.advance_scroll(now + DURATION);
        assert_eq!(app.scroll, position);
        app.pointer_up(hit.x + 2, hit.y + 2);
        assert!(matches!(app.focus, Focus::Input(_)));
        app.type_text("still aligned");
        assert!(app.values.values().any(|v| v == "still aligned"));
        app.wheel_scroll_at(100, now);
        app.document = document::parse("<p>Short page</p>", "https://example.test/");
        app.advance_scroll(now + DURATION);
        assert!(app.smooth_scroll.is_none());
        app.paint();
        assert_eq!(app.scroll, 0);
        assert!(app.smooth_scroll.is_none());
    }

    #[cfg(feature = "chrome")]
    #[test]
    fn modals_block_wheel_and_embedding_can_opt_in_without_chrome() {
        let mut app = page();
        let now = Instant::now();
        app.wheel_scroll_at(100, now);
        app.activate(Action::Menu);
        app.wheel_scroll_at(100, now);
        app.advance_scroll(now + DURATION);
        assert_eq!(app.scroll, 0);
        app.set_chrome(false);
        app.wheel_scroll_at(100, now);
        app.advance_scroll(now + DURATION);
        assert_eq!(app.scroll, 100);
    }
}
