//! Sparkle: an original, experimental web document and rendering engine.
//! Hosts provide fonts, networking, surfaces and process isolation.
pub mod document;
mod gradient;
pub mod images;
mod inline_svg;
pub mod js_browser;
pub mod page_session;
pub mod paint;
pub mod render;
pub mod style;
mod styled_layout;
mod woff;

pub use woff::brotli_decompress_limited;

pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) mgbrowser/0.9.31";
