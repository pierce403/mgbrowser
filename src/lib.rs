//! Platform host for the Mg browser components.
pub mod bookmarks;
pub mod platform;
pub mod settings;
pub mod updater;

pub const COMPILED: &str = env!("MGBROWSER_COMPILED");
pub const REVISION: &str = env!("MGBROWSER_REVISION");
