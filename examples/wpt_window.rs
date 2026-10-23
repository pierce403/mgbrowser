//! Fixed upstream Window cohort adapter. Not the static WPT lane or a public CLI.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "wpt_window/fixture.rs"]
mod fixture;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "wpt_window/guard.rs"]
mod guard;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "wpt_window/host.rs"]
mod host;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "wpt_window/source.rs"]
mod source;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "wpt_window/verdict.rs"]
mod verdict;

#[global_allocator]
static ALLOCATOR: mg_browser::platform::worker_memory::WorkerAllocator =
    mg_browser::platform::worker_memory::WorkerAllocator;

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--script-worker") => mg_browser::platform::script_worker::worker_entry(),
        Some("--script-session") => mg_browser::platform::script_worker::session_entry(),
        _ => {}
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    match host::run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("WINDOW_HOST_ERROR {error}");
            std::process::exit(1);
        }
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        eprintln!("Fixed Window adapter requires Linux x86_64");
        std::process::exit(1);
    }
}
