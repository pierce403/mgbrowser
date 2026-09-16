//! One test-only engine request inside the exact production worker policy.
//! The Python parent owns the two-second deadline and bounded pipe exchange.
#![cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")), allow(unused))]

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!("JSPLAN worker containment currently requires Linux x86_64");

mod baseline;
mod boa;
#[path = "../../../src/platform/script_isolation.rs"]
mod isolation;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Include {
    name: String,
    source: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: u32,
    engine: String,
    goal: String,
    action: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    includes: Vec<Include>,
    #[serde(default)]
    drain_jobs: bool,
    #[serde(default, rename = "async")]
    asynchronous: bool,
    #[serde(default)]
    modules: BTreeMap<String, String>,
    #[serde(default)]
    probe: Option<String>,
}

fn failure(message: &str) -> Value {
    json!({"protocol":1,"outcome":"termination","phase":"harness","message":message})
}

fn run() -> Value {
    // The owner launches a fresh process with env_clear, as does mgbrowser.
    // Never initialize an engine or admit fixture bytes before confinement.
    if std::env::vars_os().next().is_some() {
        return failure("research worker requires an empty environment");
    }
    if let Err(error) = isolation::install_isolation() {
        return failure(&format!("worker isolation unavailable: {error}"));
    }
    let mut input = Vec::new();
    if io::stdin()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut input)
        .is_err()
        || input.len() > 2 * 1024 * 1024
    {
        return failure("request exceeds 2 MiB or cannot be read");
    }
    let request: Request = match serde_json::from_slice(&input) {
        Ok(request) => request,
        Err(_) => return failure("invalid research request schema"),
    };
    if request.protocol != 1
        || !matches!(request.engine.as_str(), "baseline" | "boa")
        || !matches!(request.goal.as_str(), "script" | "module")
        || !matches!(request.action.as_str(), "evaluate" | "parse" | "probe")
        || request.source.len() > 1024 * 1024
        || request.includes.len() > 32
        || request.modules.len() > 32
        || request
            .includes
            .iter()
            .any(|include| include.name.len() > 256 || include.source.len() > 1024 * 1024)
        || request
            .modules
            .iter()
            .any(|(name, source)| name.len() > 4096 || source.len() > 1024 * 1024)
    {
        return failure("unsupported request or source/include/module admission limit");
    }
    if request.action == "probe" && request.probe.as_deref() == Some("isolation") {
        return isolation_probe();
    }
    let request = serde_json::to_value(request).expect("validated request serializes");
    match request["engine"].as_str() {
        Some("baseline") => baseline::evaluate(&request),
        Some("boa") => boa::evaluate(&request),
        _ => unreachable!(),
    }
}

fn isolation_probe() -> Value {
    // SAFETY: all requests are local denial probes. Invalid clone flags and an
    // absent exec path prevent child/program creation even if policy regresses.
    let checks = unsafe {
        [(
            "open",
            libc::syscall(
                libc::SYS_openat,
                libc::AT_FDCWD,
                c"/proc/self/status".as_ptr(),
                libc::O_RDONLY,
                0,
            ),
        )]
    };
    if checks[0].1 != -1 || io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
        return failure("file capability was not denied with EPERM");
    }
    // Separate checks preserve each syscall's errno.
    for (name, syscall, a, b) in [
        ("socket", libc::SYS_socket, libc::AF_INET, libc::SOCK_STREAM),
        ("clone", libc::SYS_clone, libc::CLONE_THREAD, 0),
    ] {
        let result = unsafe { libc::syscall(syscall, a, b, 0, 0, 0) };
        if result != -1 || io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
            return failure(&format!("{name} capability was not denied with EPERM"));
        }
    }
    let memory = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            4096,
            libc::PROT_READ | libc::PROT_EXEC,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if memory != libc::MAP_FAILED || io::Error::last_os_error().raw_os_error() != Some(libc::EPERM)
    {
        if memory != libc::MAP_FAILED {
            unsafe {
                libc::munmap(memory, 4096);
            }
        }
        return failure("executable memory was not denied with EPERM");
    }
    json!({"protocol":1,"outcome":"ok","phase":"runtime","value":true,
        "metrics":{"policy":"production-linux-x86_64", "address_space_bytes":268435456,"cpu_seconds":1,
        "denied":["file","socket","process","executable-memory"]}})
}

fn main() {
    let response = run();
    let output = serde_json::to_vec(&response).expect("response serializes");
    let output = if output.len() > 4 * 1024 * 1024 {
        serde_json::to_vec(&failure("response exceeds 4 MiB")).unwrap()
    } else {
        output
    };
    if io::stdout().write_all(&output).is_err() {
        std::process::exit(74);
    }
}
