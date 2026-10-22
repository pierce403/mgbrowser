//! Headless CLI acceptance against deterministic, owned HTTP responses.
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::{Command, Output},
    thread,
    time::Duration,
};

fn profile(status: &str, body: &str) -> Output {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let status = status.to_owned();
    let body = body.to_owned();
    let server = thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = String::new();
                    BufReader::new(&stream).read_line(&mut request).unwrap();
                    assert!(request.starts_with("GET /fixture "));
                    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("fixture accept: {error}"),
            }
        }
        panic!("profile CLI never requested its fixture");
    });
    let output = Command::new(env!("CARGO_BIN_EXE_mgbrowser"))
        .args([
            "--profile",
            &format!("http://{address}/fixture"),
            "--disable-scripts",
        ])
        .env_remove("DISPLAY")
        .output()
        .unwrap();
    server.join().unwrap();
    output
}

#[test]
fn short_profile_labels_noop_scroll_and_real_frame_hit_without_a_display() {
    let output = profile(
        "200 OK",
        "<style>p{color:#123456}</style><p>Short fixture</p>",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(
        report.contains("Scroll attempt [no movement: page fits viewport; frame cache hit]"),
        "{report}"
    );
    assert!(report.contains("UI redraw [frame cache hit]"));
    assert!(report.contains("Chrome/Inspector:"));
    assert!(report.contains("Requested worker allocation:"));
    assert!(report.contains("Shaping lookups (Fonts lifetime):"));
}

#[test]
fn tall_profile_measures_an_actual_style_cached_scroll() {
    let output = profile(
        "200 OK",
        &format!(
            "<style>p{{margin:8px}}</style>{}",
            "<p>Long scrolling fixture</p>".repeat(60)
        ),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(
        report.contains("Scroll attempt [position changed; style cache hit]"),
        "{report}"
    );
}

#[test]
fn failed_http_profile_returns_failure_and_preserves_diagnostics() {
    let output = profile("404 Not Found", "<p>Missing fixture</p>");
    assert!(!output.status.success());
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(report.contains("Performance Dashboard"));
    assert!(report.contains("unsuccessful response or load failure"));
    assert!(!report.contains("Repaint Benchmark"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Profile navigation failed"));
}
