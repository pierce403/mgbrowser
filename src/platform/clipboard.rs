//! Pure-Rust X11 `CLIPBOARD` and `PRIMARY` selection owner and requester.
//!
//! Uses a dedicated unmapped `1x1` child window of the root screen so selection
//! ownership survives closing detached browser windows without invoking any C
//! toolkit or external helper process (`xclip`/`xsel`).

use mg_chassis::{ClipboardUpdate, PasteRequest, PasteSource};
use std::{
    error::Error,
    time::{Duration, Instant},
};
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::xproto::*,
    wrapper::ConnectionExt as _,
};

/// Maximum UTF-8 bytes transferred to or from an X11 selection.
pub const MAX_CLIPBOARD_BYTES: usize = 256 * 1024;
const PASTE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug)]
struct PendingPaste<T> {
    target: T,
    and_go: bool,
    selection: Atom,
    tried_string_fallback: bool,
    incr_active: bool,
    incr_buf: Vec<u8>,
    deadline: Instant,
}

pub struct X11Clipboard<T> {
    window: Window,
    clipboard: Atom,
    primary: Atom,
    targets: Atom,
    timestamp_atom: Atom,
    utf8_string: Atom,
    text_atom: Atom,
    mime_utf8: Atom,
    mime_plain: Atom,
    incr: Atom,
    data_prop: Atom,
    last_timestamp: Timestamp,
    clipboard_acquired: Timestamp,
    primary_acquired: Timestamp,
    clipboard_text: Option<String>,
    primary_text: Option<String>,
    owns_clipboard: bool,
    owns_primary: bool,
    pending: Option<PendingPaste<T>>,
}

impl<T: Copy + PartialEq> X11Clipboard<T> {
    pub fn new<C: Connection + RequestConnection>(
        conn: &C,
        root: Window,
    ) -> Result<Self, Box<dyn Error>> {
        let window = conn.generate_id()?;
        conn.create_window(
            x11rb::COPY_FROM_PARENT as u8,
            window,
            root,
            -10,
            -10,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;

        let clipboard_ck = conn.intern_atom(false, b"CLIPBOARD")?;
        let targets_ck = conn.intern_atom(false, b"TARGETS")?;
        let timestamp_ck = conn.intern_atom(false, b"TIMESTAMP")?;
        let utf8_ck = conn.intern_atom(false, b"UTF8_STRING")?;
        let text_ck = conn.intern_atom(false, b"TEXT")?;
        let mime_utf8_ck = conn.intern_atom(false, b"text/plain;charset=utf-8")?;
        let mime_plain_ck = conn.intern_atom(false, b"text/plain")?;
        let incr_ck = conn.intern_atom(false, b"INCR")?;
        let prop_ck = conn.intern_atom(false, b"_MGBROWSER_CLIPBOARD_DATA")?;

        Ok(Self {
            window,
            clipboard: clipboard_ck.reply()?.atom,
            primary: AtomEnum::PRIMARY.into(),
            targets: targets_ck.reply()?.atom,
            timestamp_atom: timestamp_ck.reply()?.atom,
            utf8_string: utf8_ck.reply()?.atom,
            text_atom: text_ck.reply()?.atom,
            mime_utf8: mime_utf8_ck.reply()?.atom,
            mime_plain: mime_plain_ck.reply()?.atom,
            incr: incr_ck.reply()?.atom,
            data_prop: prop_ck.reply()?.atom,
            last_timestamp: x11rb::CURRENT_TIME,
            clipboard_acquired: x11rb::CURRENT_TIME,
            primary_acquired: x11rb::CURRENT_TIME,
            clipboard_text: None,
            primary_text: None,
            owns_clipboard: false,
            owns_primary: false,
            pending: None,
        })
    }

    pub fn window(&self) -> Window {
        self.window
    }

    pub fn note_timestamp(&mut self, time: Timestamp) {
        if time != x11rb::CURRENT_TIME {
            self.last_timestamp = time;
        }
    }

    pub fn expire_stale(&mut self, now: Instant) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| now >= pending.deadline)
        {
            self.pending = None;
        }
    }

    pub fn apply_update<C: Connection + RequestConnection>(
        &mut self,
        conn: &C,
        update: ClipboardUpdate,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(text) = update.clipboard {
            let bounded = truncate_utf8(&text, MAX_CLIPBOARD_BYTES);
            let ts = self.acquire_selection(conn, self.clipboard)?;
            self.clipboard_text = Some(bounded);
            if let Some(acquired) = ts {
                self.owns_clipboard = true;
                self.clipboard_acquired = acquired;
            }
        }
        if let Some(text) = update.primary {
            let bounded = truncate_utf8(&text, MAX_CLIPBOARD_BYTES);
            let ts = self.acquire_selection(conn, self.primary)?;
            self.primary_text = Some(bounded);
            if let Some(acquired) = ts {
                self.owns_primary = true;
                self.primary_acquired = acquired;
            }
        }
        conn.flush()?;
        Ok(())
    }

    fn acquire_selection<C: Connection>(
        &self,
        conn: &C,
        selection: Atom,
    ) -> Result<Option<Timestamp>, Box<dyn Error>> {
        let ts = self.last_timestamp;
        conn.set_selection_owner(self.window, selection, ts)?;
        if conn.get_selection_owner(selection)?.reply()?.owner == self.window {
            return Ok(Some(ts));
        }
        if ts != x11rb::CURRENT_TIME {
            conn.set_selection_owner(self.window, selection, x11rb::CURRENT_TIME)?;
            if conn.get_selection_owner(selection)?.reply()?.owner == self.window {
                return Ok(Some(x11rb::CURRENT_TIME));
            }
        }
        Ok(None)
    }

    pub fn request_paste<C: Connection>(
        &mut self,
        conn: &C,
        target: T,
        req: PasteRequest,
    ) -> Result<Option<(T, String, bool)>, Box<dyn Error>> {
        let (selection, owned_text) = match req.source {
            PasteSource::Clipboard => (
                self.clipboard,
                self.owns_clipboard
                    .then_some(self.clipboard_text.as_ref())
                    .flatten(),
            ),
            PasteSource::Primary => (
                self.primary,
                self.owns_primary
                    .then_some(self.primary_text.as_ref())
                    .flatten(),
            ),
        };
        if let Some(text) = owned_text {
            return Ok(Some((target, text.clone(), req.and_go)));
        }

        let owner = conn.get_selection_owner(selection)?.reply()?.owner;
        if owner == x11rb::NONE {
            return Ok(None);
        }
        if owner == self.window {
            let fallback = match req.source {
                PasteSource::Clipboard => self.clipboard_text.clone(),
                PasteSource::Primary => self.primary_text.clone(),
            };
            return Ok(fallback.map(|text| (target, text, req.and_go)));
        }

        conn.delete_property(self.window, self.data_prop)?;
        conn.convert_selection(
            self.window,
            selection,
            self.utf8_string,
            self.data_prop,
            self.last_timestamp,
        )?;
        conn.flush()?;
        self.pending = Some(PendingPaste {
            target,
            and_go: req.and_go,
            selection,
            tried_string_fallback: false,
            incr_active: false,
            incr_buf: Vec::new(),
            deadline: Instant::now() + PASTE_TIMEOUT,
        });
        Ok(None)
    }

    pub fn handle_selection_request<C: Connection + RequestConnection>(
        &mut self,
        conn: &C,
        event: SelectionRequestEvent,
    ) -> Result<(), Box<dyn Error>> {
        self.note_timestamp(event.time);
        let payload = if event.selection == self.clipboard && self.owns_clipboard {
            self.clipboard_text
                .as_deref()
                .map(|t| (t, self.clipboard_acquired))
        } else if event.selection == self.primary && self.owns_primary {
            self.primary_text
                .as_deref()
                .map(|t| (t, self.primary_acquired))
        } else {
            None
        };

        let prop = if event.property == x11rb::NONE {
            event.target
        } else {
            event.property
        };

        let mut reply_prop = x11rb::NONE;
        if let Some((text, acquired)) = payload {
            if event.target == self.targets {
                let supported = [
                    self.targets,
                    self.timestamp_atom,
                    self.utf8_string,
                    self.mime_utf8,
                    self.mime_plain,
                    self.text_atom,
                    AtomEnum::STRING.into(),
                ];
                conn.change_property32(
                    PropMode::REPLACE,
                    event.requestor,
                    prop,
                    AtomEnum::ATOM,
                    &supported,
                )?;
                reply_prop = prop;
            } else if event.target == self.timestamp_atom {
                conn.change_property32(
                    PropMode::REPLACE,
                    event.requestor,
                    prop,
                    AtomEnum::INTEGER,
                    &[acquired],
                )?;
                reply_prop = prop;
            } else if event.target == self.utf8_string
                || event.target == self.mime_utf8
                || event.target == self.mime_plain
                || event.target == self.text_atom
            {
                let max_bytes = max_property_bytes(conn);
                let slice = truncate_utf8_slice(text, max_bytes);
                conn.change_property8(
                    PropMode::REPLACE,
                    event.requestor,
                    prop,
                    event.target,
                    slice.as_bytes(),
                )?;
                reply_prop = prop;
            } else if event.target == u32::from(AtomEnum::STRING) {
                let max_bytes = max_property_bytes(conn);
                let latin1: Vec<u8> = text
                    .chars()
                    .take(max_bytes)
                    .map(|ch| if (ch as u32) <= 0xff { ch as u8 } else { b'?' })
                    .collect();
                conn.change_property8(
                    PropMode::REPLACE,
                    event.requestor,
                    prop,
                    AtomEnum::STRING,
                    &latin1,
                )?;
                reply_prop = prop;
            }
        }

        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: event.time,
            requestor: event.requestor,
            selection: event.selection,
            target: event.target,
            property: reply_prop,
        };
        conn.send_event(false, event.requestor, EventMask::NO_EVENT, notify)?;
        conn.flush()?;
        Ok(())
    }

    pub fn handle_selection_clear(&mut self, event: SelectionClearEvent) {
        self.note_timestamp(event.time);
        if event.owner != self.window {
            return;
        }
        if event.selection == self.clipboard {
            self.owns_clipboard = false;
        } else if event.selection == self.primary {
            self.owns_primary = false;
        }
    }

    pub fn handle_selection_notify<C: Connection>(
        &mut self,
        conn: &C,
        event: SelectionNotifyEvent,
    ) -> Result<Option<(T, String, bool)>, Box<dyn Error>> {
        self.note_timestamp(event.time);
        let Some(pending) = &mut self.pending else {
            return Ok(None);
        };
        if event.requestor != self.window || event.selection != pending.selection {
            return Ok(None);
        }

        if event.property == x11rb::NONE {
            if !pending.tried_string_fallback && event.target == self.utf8_string {
                pending.tried_string_fallback = true;
                pending.deadline = Instant::now() + PASTE_TIMEOUT;
                conn.convert_selection(
                    self.window,
                    pending.selection,
                    AtomEnum::STRING.into(),
                    self.data_prop,
                    self.last_timestamp,
                )?;
                conn.flush()?;
                return Ok(None);
            }
            self.pending = None;
            return Ok(None);
        }

        let reply = conn
            .get_property(
                true,
                self.window,
                event.property,
                GetPropertyType::ANY,
                0,
                (MAX_CLIPBOARD_BYTES as u32 / 4) + 1024,
            )?
            .reply()?;

        if reply.type_ == self.incr {
            pending.incr_active = true;
            pending.incr_buf.clear();
            pending.deadline = Instant::now() + PASTE_TIMEOUT;
            conn.flush()?;
            return Ok(None);
        }

        let finished = self.pending.take().unwrap();
        let decoded = decode_selection_bytes(&reply.value, reply.type_);
        Ok(Some((finished.target, decoded, finished.and_go)))
    }

    pub fn handle_property_notify<C: Connection>(
        &mut self,
        conn: &C,
        event: PropertyNotifyEvent,
    ) -> Result<Option<(T, String, bool)>, Box<dyn Error>> {
        self.note_timestamp(event.time);
        let Some(pending) = &mut self.pending else {
            return Ok(None);
        };
        if !pending.incr_active
            || event.window != self.window
            || event.atom != self.data_prop
            || event.state != Property::NEW_VALUE
        {
            return Ok(None);
        }

        let reply = conn
            .get_property(
                true,
                self.window,
                self.data_prop,
                GetPropertyType::ANY,
                0,
                (MAX_CLIPBOARD_BYTES as u32 / 4) + 1024,
            )?
            .reply()?;
        conn.flush()?;

        if reply.value.is_empty() {
            let finished = self.pending.take().unwrap();
            let decoded = decode_selection_bytes(&finished.incr_buf, self.utf8_string);
            return Ok(Some((finished.target, decoded, finished.and_go)));
        }

        let room = MAX_CLIPBOARD_BYTES.saturating_sub(pending.incr_buf.len());
        if room > 0 {
            let take = reply.value.len().min(room);
            pending.incr_buf.extend_from_slice(&reply.value[..take]);
        }
        pending.deadline = Instant::now() + PASTE_TIMEOUT;
        Ok(None)
    }
}

fn max_property_bytes<C: RequestConnection>(conn: &C) -> usize {
    conn.maximum_request_bytes()
        .saturating_sub(64)
        .clamp(4096, MAX_CLIPBOARD_BYTES)
}

fn truncate_utf8_slice(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    truncate_utf8_slice(text, max_bytes).to_owned()
}

fn decode_selection_bytes(bytes: &[u8], prop_type: Atom) -> String {
    let raw = if prop_type == u32::from(AtomEnum::STRING) {
        match std::str::from_utf8(bytes) {
            Ok(utf8) => utf8.to_owned(),
            Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
        }
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    truncate_utf8(&raw, MAX_CLIPBOARD_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::{protocol::Event, rust_connection::RustConnection};

    #[test]
    fn utf8_and_latin1_decoding_respects_boundaries() {
        let utf8 = "Console error — ligne 42 🚀";
        assert_eq!(decode_selection_bytes(utf8.as_bytes(), 999), utf8);
        let latin1 = &[b'c', b'a', b'f', 0xe9];
        assert_eq!(
            decode_selection_bytes(latin1, AtomEnum::STRING.into()),
            "café"
        );
        let truncated = truncate_utf8("ab🚀cd", 4);
        assert_eq!(truncated, "ab");
    }

    #[test]
    fn x11_clipboard_and_primary_roundtrip_between_two_clients() {
        let Ok((conn_a, screen_a)) = x11rb::connect(None) else {
            // Headless build environments without DISPLAY skip live X11 socket test;
            // CI and local Xvfb gates run with DISPLAY set.
            return;
        };
        let (conn_b, screen_b) = x11rb::connect(None).unwrap();
        let root_a = conn_a.setup().roots[screen_a].root;
        let root_b = conn_b.setup().roots[screen_b].root;

        let mut clip_a = X11Clipboard::<u32>::new(&conn_a, root_a).unwrap();
        let mut clip_b = X11Clipboard::<u32>::new(&conn_b, root_b).unwrap();

        // 1. Client A publishes CLIPBOARD and PRIMARY; Client A local short-circuit is 0-latency.
        clip_a
            .apply_update(
                &conn_a,
                ClipboardUpdate {
                    clipboard: Some(
                        "[ERR JS] Uncaught TypeError: Cannot read properties of null\n  at app.js:14:9"
                            .into(),
                    ),
                    primary: Some("https://news.google.com/home".into()),
                },
            )
            .unwrap();

        let fast = clip_a
            .request_paste(
                &conn_a,
                7,
                PasteRequest {
                    source: PasteSource::Clipboard,
                    and_go: false,
                },
            )
            .unwrap();
        assert_eq!(
            fast,
            Some((
                7,
                "[ERR JS] Uncaught TypeError: Cannot read properties of null\n  at app.js:14:9"
                    .into(),
                false
            ))
        );

        // 2. Client B requests CLIPBOARD and PRIMARY from Client A over the real X11 server!
        assert_eq!(
            clip_b
                .request_paste(
                    &conn_b,
                    42,
                    PasteRequest {
                        source: PasteSource::Clipboard,
                        and_go: false,
                    },
                )
                .unwrap(),
            None
        );
        let received_clip = pump_transfer(&conn_a, &mut clip_a, &conn_b, &mut clip_b);
        assert_eq!(
            received_clip,
            (
                42,
                "[ERR JS] Uncaught TypeError: Cannot read properties of null\n  at app.js:14:9"
                    .into(),
                false
            )
        );

        assert_eq!(
            clip_b
                .request_paste(
                    &conn_b,
                    99,
                    PasteRequest {
                        source: PasteSource::Primary,
                        and_go: true,
                    },
                )
                .unwrap(),
            None
        );
        let received_primary = pump_transfer(&conn_a, &mut clip_a, &conn_b, &mut clip_b);
        assert_eq!(
            received_primary,
            (99, "https://news.google.com/home".into(), true)
        );

        // 3. Client B takes ownership of CLIPBOARD; Client A receives SelectionClear and fetches from B!
        clip_b
            .apply_update(
                &conn_b,
                ClipboardUpdate {
                    clipboard: Some("pasted from external X11 client ✨".into()),
                    primary: None,
                },
            )
            .unwrap();
        // Pump SelectionClear on A.
        let clear_deadline = Instant::now() + Duration::from_secs(2);
        while clip_a.owns_clipboard && Instant::now() < clear_deadline {
            while let Some(ev) = conn_a.poll_for_event().unwrap() {
                if let Event::SelectionClear(sc) = ev {
                    clip_a.handle_selection_clear(sc);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!clip_a.owns_clipboard);

        assert_eq!(
            clip_a
                .request_paste(
                    &conn_a,
                    1,
                    PasteRequest {
                        source: PasteSource::Clipboard,
                        and_go: false,
                    },
                )
                .unwrap(),
            None
        );
        let received_from_b = pump_transfer(&conn_b, &mut clip_b, &conn_a, &mut clip_a);
        assert_eq!(
            received_from_b,
            (1, "pasted from external X11 client ✨".into(), false)
        );
    }

    fn pump_transfer(
        owner_conn: &RustConnection,
        owner: &mut X11Clipboard<u32>,
        req_conn: &RustConnection,
        requester: &mut X11Clipboard<u32>,
    ) -> (u32, String, bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            while let Some(ev) = owner_conn.poll_for_event().unwrap() {
                match ev {
                    Event::SelectionRequest(sr) => {
                        owner.handle_selection_request(owner_conn, sr).unwrap();
                    }
                    Event::SelectionClear(sc) => owner.handle_selection_clear(sc),
                    _ => {}
                }
            }
            while let Some(ev) = req_conn.poll_for_event().unwrap() {
                match ev {
                    Event::SelectionNotify(sn) => {
                        if let Some(done) = requester.handle_selection_notify(req_conn, sn).unwrap()
                        {
                            return done;
                        }
                    }
                    Event::PropertyNotify(pn) => {
                        if let Some(done) = requester.handle_property_notify(req_conn, pn).unwrap()
                        {
                            return done;
                        }
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("X11 selection transfer timed out");
    }
}
