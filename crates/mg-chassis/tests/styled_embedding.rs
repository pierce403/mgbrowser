//! One generic styled-page journey through public host APIs and real HTTP.
use mg_chassis::{Browser, scripts::DisabledScripts};
use mg_sparkle::{
    paint::Fonts,
    render::{self, Action, Controls, Viewport},
};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::Arc,
    time::{Duration, Instant},
};

fn fonts() -> Fonts {
    Fonts::from_bytes(std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").unwrap())
        .unwrap()
}
fn finish(browser: &mut Browser) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while browser.is_loading() {
        assert!(Instant::now() < deadline, "{}", browser.status());
        browser.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(browser.last_load_succeeded(), "{}", browser.status());
}

#[test]
fn linked_css_svg_table_and_scrolled_link_use_real_resource_and_input_paths() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut paths = Vec::new();
        while paths.len() < 4 {
            assert!(
                Instant::now() < deadline,
                "missing fixture request: {paths:?}"
            );
            let (mut socket, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(socket.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let path = line.split_whitespace().nth(1).unwrap().to_owned();
            let (mime, body) = match path.as_str() {
                "/" => (
                    "text/html",
                    "<html><head><title>Styled fixture</title><link rel='stylesheet' href='/style.css'></head><body><table><tr><td><img src='/icon.svg' width='12' height='12'></td><td>Styled cells</td></tr></table><div class='space'></div><a id='destination' href='/destination'>Continue to destination</a><div class='space'></div></body></html>",
                ),
                "/style.css" => (
                    "text/css",
                    "body{margin:8px;background:#f6f6ef;color:#222;font-size:14px} table{width:80%;background:#ff6600;border-spacing:0} .space{height:350px} a{color:#000;text-decoration:none}",
                ),
                "/icon.svg" => (
                    "image/svg+xml",
                    "<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12'><rect width='12' height='12' fill='#00ff00'/></svg>",
                ),
                "/destination" => (
                    "text/html",
                    "<title>Destination</title><p>Reached through the styled link.</p>",
                ),
                _ => panic!("Unexpected request {path}"),
            };
            paths.push(path);
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        paths
    });
    let mut browser = Browser::new(fonts(), Arc::new(DisabledScripts));
    browser.set_chrome(false);
    browser.resize(640, 480);
    browser.navigate(format!("{base}/"), None, true);
    finish(&mut browser);
    assert_eq!(browser.document().stylesheets.len(), 1);
    assert_eq!(browser.document().resources.len(), 1);
    assert!(
        browser.document().resource_warnings.is_empty(),
        "{:?}",
        browser.document().resource_warnings
    );
    let canvas = browser.paint();
    assert!(
        canvas.pixels.contains(&0xff6600),
        "external CSS background must be painted"
    );
    assert!(
        canvas.pixels.contains(&0x00ff00),
        "downloaded SVG must be painted"
    );
    browser.scroll_by(200);
    browser.paint();
    let frame = render::render(
        browser.document(),
        &mut fonts(),
        Viewport {
            width: 640,
            height: 480,
            scroll: 200,
        },
        &Controls::default(),
    );
    let hit = frame
        .hits
        .iter()
        .find(|hit| matches!(&hit.action,Action::Link{href,..} if href.ends_with("/destination")))
        .unwrap();
    browser.pointer_down(hit.x + 2, hit.y + 2);
    browser.pointer_up(hit.x + 2, hit.y + 2);
    finish(&mut browser);
    assert_eq!(browser.document().title, "Destination");
    let requests = server.join().unwrap();
    assert_eq!(requests, ["/", "/style.css", "/icon.svg", "/destination"]);
}
