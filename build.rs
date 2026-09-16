use std::{
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn main() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/main");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=crates");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=build.rs");
    let time = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => {
            UNIX_EPOCH + Duration::from_secs(value.parse().expect("invalid SOURCE_DATE_EPOCH"))
        }
        Err(_) => SystemTime::now(),
    };
    let mut revision = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into());
    if Command::new("git")
        .args([
            "diff",
            "--quiet",
            "HEAD",
            "--",
            "src",
            "crates",
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
        ])
        .status()
        .is_ok_and(|status| status.code() == Some(1))
    {
        revision.push_str("-dirty");
    }
    println!(
        "cargo:rustc-env=MGBROWSER_COMPILED={}",
        httpdate::fmt_http_date(time)
    );
    println!("cargo:rustc-env=MGBROWSER_REVISION={revision}");
}
