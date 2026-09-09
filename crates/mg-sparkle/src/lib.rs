//! Sparkle: an original, experimental web document and rendering engine.
//! Hosts provide fonts, networking, surfaces and process isolation.
pub mod document;
pub mod js_browser;
pub mod page_session;
pub mod paint;
pub mod render;

pub const USER_AGENT: &str =
    "mgbrowser/0.1 (experimental Rust research browser; +https://mgbrowser.org)";
