//! Ordinary HTTP(S) interoperability check using the browser's own transport.
//! Usage: cargo run --example net_probe -- https://example.org tmp/response.html

fn main() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let url = arguments
        .next()
        .ok_or("usage: net_probe URL [output-file]")?;
    let response = mg_deps::net::fetch(&url)?;
    println!(
        "URL: {}\nStatus: {}\nType: {}\nBytes: {}",
        response.url,
        response.status,
        response.content_type,
        response.body.len()
    );
    if let Some(path) = arguments.next() {
        std::fs::write(&path, &response.body).map_err(|e| e.to_string())?;
        println!("Saved: {path}");
    } else {
        println!("{}", String::from_utf8_lossy(&response.body));
    }
    Ok(())
}
