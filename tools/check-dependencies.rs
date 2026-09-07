//! A regression guard for known native backends, not a complete source audit.
use std::process::{self, Command};

const FORBIDDEN: &[&str] = &[
    "ring",
    "aws-lc-rs",
    "aws-lc-sys",
    "aws-lc-fips-sys",
    "native-tls",
    "openssl",
    "openssl-sys",
    "freetype",
    "freetype-rs",
    "freetype-sys",
    "harfbuzz_rs",
    "harfbuzz-sys",
    "core-text",
    "dwrote",
    "libz-sys",
    "libz-ng-sys",
    "zlib-ng-sys",
    "libwebp-sys",
    "libwebp-sys2",
    "dav1d-sys",
    "libaom-sys",
    "mozjpeg-sys",
    "turbojpeg-sys",
    "cc",
    "cmake",
    "pkg-config",
];

fn main() {
    let output = Command::new("cargo")
        .args([
            "tree",
            "--locked",
            "--edges",
            "normal,build",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()
        .expect("cargo must be available");
    if !output.status.success() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        process::exit(1);
    }
    let tree = String::from_utf8(output.stdout).expect("Cargo output must be UTF-8");
    let mut failed = false;
    for line in tree.lines() {
        if let Some(name) = line.split_whitespace().next() {
            if FORBIDDEN.contains(&name) {
                eprintln!("Prohibited active dependency: {line}");
                failed = true;
            }
        }
    }
    if failed {
        process::exit(1);
    }
    println!(
        "No known prohibited native backends in the active normal/build graph. Source review is still required for dependency changes."
    );
}
