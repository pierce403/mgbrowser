//! Public embedding contract, exercised with and without the optional UX feature.
use mg_chassis::{
    Browser, Key,
    scripts::{DisabledScripts, ScriptRuntime},
};
use mg_sparkle::{
    js_browser::Request,
    paint::Fonts,
    render::{self, Controls, Viewport},
};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::Arc,
    time::{Duration, Instant},
};

fn fonts() -> Fonts {
    let path = std::env::var("MGBROWSER_FONT")
        .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
    Fonts::from_bytes(std::fs::read(path).unwrap()).unwrap()
}
fn loaded(browser: &mut Browser) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while browser.is_loading() {
        assert!(Instant::now() < deadline, "{}", browser.status());
        browser.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(browser.last_load_succeeded(), "{}", browser.status());
}

#[test]
fn headless_host_uses_sparkle_pixels_and_real_form_navigation() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut requests = Vec::new();
        while requests.len() < 2 {
            assert!(Instant::now() < deadline, "Missing embedded navigation");
            let (mut socket, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = String::new();
            BufReader::new(socket.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            requests.push(request);
            let html = if requests.len() == 1 {
                "<title>Embedding</title><h1>Mg components</h1><form action='/search'><input name=q><button>Search</button></form>"
            } else {
                "<title>Destination</title><p>Submitted through Chassis</p>"
            };
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len()).unwrap();
        }
        requests
    });
    let mut browser = Browser::new(fonts(), Arc::new(DisabledScripts));
    browser.set_chrome(false);
    browser.resize(640, 480);
    browser.navigate(format!("{base}/"), None, true);
    loaded(&mut browser);
    let frame = render::render(
        browser.document(),
        &mut fonts(),
        Viewport {
            width: 640,
            height: 480,
            scroll: 0,
        },
        &Controls::default(),
    );
    let canvas = browser.paint();
    assert_eq!((canvas.width, canvas.height), (640, 480));
    assert_eq!(
        canvas.pixels, frame.canvas.pixels,
        "Headless embedding must use the unmodified Sparkle page surface"
    );
    let input = frame
        .hits
        .iter()
        .find(|hit| matches!(hit.action, render::Action::Input(_)))
        .unwrap();
    browser.pointer_down(input.x + 3, input.y + 3);
    browser.pointer_up(input.x + 3, input.y + 3);
    browser.type_text("Rust & café");
    browser.handle_key(Key::Enter, false, false, false);
    loaded(&mut browser);
    assert_eq!(browser.document().title, "Destination");
    let requests = server.join().unwrap();
    assert_eq!(requests[0], "GET / HTTP/1.1\r\n");
    assert_eq!(requests[1], "GET /search?q=Rust+%26+caf%C3%A9 HTTP/1.1\r\n");
}

#[test]
fn absent_isolation_is_an_explicit_error() {
    assert!(
        DisabledScripts
            .start(
                Request {
                    url: "https://example.test/".into(),
                    html: "<script>1+1</script>".into()
                },
                1
            )
            .is_err()
    );
}

#[test]
fn selected_provider_builds_a_client() {
    let config = mg_chassis::tls_client_config(rustls::RootCertStore::empty()).unwrap();
    let name = "localhost".try_into().unwrap();
    let client = rustls::ClientConnection::new(config.into(), name).unwrap();
    assert!(client.wants_write());
}
