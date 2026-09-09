//! X11 host facilities. Engines do not depend on this crate.
pub mod script_worker;

use mg_chassis::{
    Key,
    scripts::{ScriptRuntime, ScriptSession, SessionUpdate},
};
use mg_sparkle::{
    js_browser::Request,
    page_session::{SessionInput, SessionReply},
    paint::Fonts,
};
use std::time::Duration;

/// A fresh service for one Browser. Its children re-exec the current executable,
/// which must dispatch the worker CLI modes before creating any other services.
#[derive(Default)]
pub struct LinuxScripts {
    pool: script_worker::ChildPool,
}

impl ScriptRuntime for LinuxScripts {
    fn set_generation(&self, generation: u64) -> Result<(), String> {
        self.pool.set_generation(generation)
    }
    fn start(
        &self,
        request: Request,
        generation: u64,
    ) -> Result<(Box<dyn ScriptSession>, SessionReply), String> {
        script_worker::Session::start(request, generation, self.pool.clone())
            .map(|(session, reply)| (Box::new(session) as Box<dyn ScriptSession>, reply))
    }
    fn close(&self) {
        self.pool.close();
    }
    fn wait_idle(&self) -> Result<(), String> {
        self.pool.wait_idle()
    }
}
impl ScriptSession for script_worker::Session {
    fn id(&self) -> u64 {
        self.id()
    }
    fn try_dispatch(&mut self, input: SessionInput, revision: u64) -> Result<u64, String> {
        self.try_dispatch(input, revision)
    }
    fn try_recv(&mut self) -> Option<SessionUpdate> {
        self.try_recv().map(|update| SessionUpdate {
            generation: update.generation,
            session_id: update.session_id,
            sequence: update.sequence,
            reply: update.reply,
        })
    }
    fn cancel(&self) {
        self.cancel();
    }
    fn charge_active(&mut self, duration: Duration) -> Result<(), String> {
        self.charge_active(duration)
    }
}

/// Convert X11's key symbols at the platform boundary.
pub fn translate_keysym(sym: u32) -> Option<Key> {
    Some(match sym {
        0xff0d => Key::Enter,
        0xff08 => Key::Backspace,
        0xff09 => Key::Tab,
        0xff51 => Key::Left,
        0xff52 => Key::Up,
        0xff54 => Key::Down,
        0xff55 => Key::PageUp,
        0xff56 => Key::PageDown,
        0xffc2 => Key::Reload,
        _ => {
            let code = if sym & 0xff000000 == 0x01000000 {
                sym & 0xffffff
            } else {
                sym
            };
            let ch = char::from_u32(code)?;
            if ch.is_control() || (0xff00..=0xffff).contains(&code) {
                return None;
            }
            Key::Character(ch)
        }
    })
}
pub fn load_fonts() -> Result<Fonts, String> {
    if let Some(path) = std::env::var_os("MGBROWSER_FONT") {
        return load_font_path(std::path::Path::new(&path));
    }
    let paths = [
        #[cfg(target_os = "macos")]
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        #[cfg(target_os = "macos")]
        "/Library/Fonts/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
    ];
    let mut failures = Vec::new();
    for path in paths {
        match load_font_path(std::path::Path::new(path)) {
            Ok(fonts) => return Ok(fonts),
            Err(error) => failures.push(error),
        }
    }
    Err(format!(
        "No usable font found. Set MGBROWSER_FONT to a TrueType/OpenType font file. {}",
        failures.join("; ")
    ))
}

fn load_font_path(path: &std::path::Path) -> Result<Fonts, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("Cannot read font {}: {error}", path.display()))?;
    Fonts::from_bytes(bytes).map_err(|error| format!("Font {}: {error}", path.display()))
}
