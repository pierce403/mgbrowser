//! Platform host for the Mg browser components.
pub mod bookmarks;
pub mod desktop;
pub mod platform;
pub mod restart;
pub mod settings;
pub mod updater;
pub mod workspace;

pub const COMPILED: &str = env!("MGBROWSER_COMPILED");
pub const REVISION: &str = env!("MGBROWSER_REVISION");
