//! Platform host for the Mg browser components.
pub mod bookmarks;
pub mod desktop;
pub mod downloads;
pub mod history;
pub mod local_storage;
pub mod platform;
pub mod restart;
pub mod session_store;
pub mod settings;
pub mod updater;
pub mod workspace;

pub const COMPILED: &str = env!("MGBROWSER_COMPILED");
pub const REVISION: &str = env!("MGBROWSER_REVISION");
