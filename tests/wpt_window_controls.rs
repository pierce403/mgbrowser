//! Pure controls for the separately frozen first upstream Window adapter.
//! This does not execute JavaScript, launch Browser/workers, or score WPT files.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

#[path = "../examples/wpt_window/fixture.rs"]
mod fixture;
#[path = "../examples/wpt_window/guard.rs"]
mod guard;
#[path = "../examples/wpt_window/source.rs"]
mod source;
#[path = "../examples/wpt_window/verdict.rs"]
mod verdict;
