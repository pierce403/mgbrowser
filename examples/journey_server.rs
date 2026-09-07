//! Deterministic local fixture service; never represents Google or public results.
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:7878")?;
    println!("Local fixture service: http://127.0.0.1:7878");
    for stream in listener.incoming() {
        let mut stream = stream?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
        let mut reader = BufReader::new(&stream);
        let mut request = String::new();
        reader.read_line(&mut request)?;
        let target = request.split_whitespace().nth(1).unwrap_or("/");
        let url = url::Url::parse(&format!("http://127.0.0.1:7878{target}"))?;
        let fields: Vec<_> = url.query_pairs().collect();
        let (status, body) = match url.path() {
            "/" => (
                "200 OK",
                include_str!("../tests/fixtures/journey/home.html"),
            ),
            "/script-home" => ("200 OK", include_str!("../tests/fixtures/script/home.html")),
            "/script-redirect" => (
                "200 OK",
                include_str!("../tests/fixtures/script/redirect.html"),
            ),
            "/script-loop" => ("200 OK", include_str!("../tests/fixtures/script/loop.html")),
            "/script-dynamic" => (
                "200 OK",
                include_str!("../tests/fixtures/script/dynamic.html"),
            ),
            "/script-regexp" => (
                "200 OK",
                include_str!("../tests/fixtures/script/regexp.html"),
            ),
            "/script-iteration" => (
                "200 OK",
                include_str!("../tests/fixtures/script/iteration.html"),
            ),
            "/script-expressions" => (
                "200 OK",
                include_str!("../tests/fixtures/script/expressions.html"),
            ),
            "/script-allocation" => (
                "200 OK",
                include_str!("../tests/fixtures/script/allocation.html"),
            ),
            "/script-arrays" => (
                "200 OK",
                include_str!("../tests/fixtures/script/arrays.html"),
            ),
            "/script-bindings" => (
                "200 OK",
                include_str!("../tests/fixtures/script/bindings.html"),
            ),
            "/script-sources" => (
                "200 OK",
                include_str!("../tests/fixtures/script/sources.html"),
            ),
            "/search"
                if fields.iter().any(|(k, v)| k == "q" && !v.is_empty())
                    && fields.iter().any(|(k, v)| k == "source" && v == "fixture") =>
            {
                (
                    "200 OK",
                    include_str!("../tests/fixtures/journey/results.html"),
                )
            }
            "/destination" => (
                "200 OK",
                include_str!("../tests/fixtures/journey/destination.html"),
            ),
            _ => (
                "400 Bad Request",
                "<h1>Fixture request failed</h1><p>Expected query and hidden field.</p>",
            ),
        };
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )?;
        eprintln!("{status} {}", url.path());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mg_deps::{document, js::syntax};

    #[test]
    fn script_fixtures_parse_without_a_static_search_form() {
        for source in [
            include_str!("../tests/fixtures/script/home.html"),
            include_str!("../tests/fixtures/script/redirect.html"),
            include_str!("../tests/fixtures/script/loop.html"),
            include_str!("../tests/fixtures/script/dynamic.html"),
            include_str!("../tests/fixtures/script/regexp.html"),
            include_str!("../tests/fixtures/script/iteration.html"),
            include_str!("../tests/fixtures/script/expressions.html"),
            include_str!("../tests/fixtures/script/allocation.html"),
            include_str!("../tests/fixtures/script/arrays.html"),
            include_str!("../tests/fixtures/script/bindings.html"),
            include_str!("../tests/fixtures/script/sources.html"),
        ] {
            let document = document::parse(source, "http://127.0.0.1:7878/script-home");
            assert!(document.forms.is_empty());
            assert!(
                !document
                    .nodes
                    .iter()
                    .any(|node| matches!(node.tag.as_str(), "form" | "input" | "button"))
            );
            let scripts: Vec<_> = document
                .nodes
                .iter()
                .filter(|node| node.tag == "script")
                .collect();
            assert_eq!(scripts.len(), 1);
            let source: String = scripts[0]
                .children
                .iter()
                .map(|id| document.nodes[*id].text.as_str())
                .collect();
            syntax::parse(&source)
                .expect("local fixture must use the implemented classic-script syntax");
        }
    }
}
