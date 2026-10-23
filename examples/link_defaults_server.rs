//! Fixed authored loopback service for the empty-anchor native/CDP proof.
//! Separate from journey_server so every older acceptance source pin is intact.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:7878")?;
    println!("Local fixture service: http://127.0.0.1:7878");
    for stream in listener.incoming() {
        let mut stream = stream?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(3)))?;
        let mut request = String::new();
        let bytes = BufReader::new(&stream).take(8193).read_line(&mut request)?;
        let fields: Vec<_> = request.split_whitespace().take(4).collect();
        let target = fields.get(1).copied().unwrap_or("/");
        let valid = bytes <= 8192
            && request.ends_with('\n')
            && fields.len() == 3
            && fields[0] == "GET"
            && matches!(fields[2], "HTTP/1.0" | "HTTP/1.1")
            && target.starts_with('/')
            && !target.contains('?')
            && !target.contains('#');
        let (status, body) = if valid {
            match target {
                "/" => (
                    "200 OK",
                    include_str!("../tests/fixtures/journey/home.html"),
                ),
                "/link-defaults" => (
                    "200 OK",
                    include_str!("../tests/fixtures/layout/link-defaults.html"),
                ),
                "/link-default-native/hit" => (
                    "200 OK",
                    "<html><head><title>Empty native hit</title><style>html,body{margin:0}div{width:180px;height:80px;background:#123456}</style></head><body><div></div></body></html>",
                ),
                "/link-default-cdp/hit" => (
                    "200 OK",
                    "<html><head><title>Empty CDP hit</title><style>html,body{margin:0}div{width:180px;height:80px;background:#654321}</style></head><body><div></div></body></html>",
                ),
                _ => ("404 Not Found", "<h1>Unknown authored fixture route</h1>"),
            }
        } else {
            (
                "400 Bad Request",
                "<h1>Invalid authored fixture request</h1>",
            )
        };
        let headers = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes())?;
        stream.write_all(body.as_bytes())?;
        stream.flush()?;
        println!("{status} {target}");
    }
    Ok(())
}
