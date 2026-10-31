//! End-to-end Milestone 2 integration tests over real isolated `VERSION = 9`
//! worker processes (`LinuxScripts`), `mg-chassis::Browser`, and a local HTTP server.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[global_allocator]
static ALLOCATOR: mg_browser::platform::worker_memory::WorkerAllocator =
    mg_browser::platform::worker_memory::WorkerAllocator;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    use mg_browser::platform::LinuxScripts;
    use mg_chassis::{Browser, ScriptWorkStatus};
    use mg_sparkle::{document::Document, paint::Fonts};
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader, Read, Write},
        net::{TcpListener, TcpStream},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    fn fonts() -> Fonts {
        let path = std::env::var("MGBROWSER_FONT")
            .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
        Fonts::from_bytes(std::fs::read(path).expect("read font")).expect("parse font")
    }

    fn text_of(doc: &Document, selector: &str) -> String {
        let Some(root) = doc.query_selector(0, selector).ok().flatten() else {
            return String::new();
        };
        let mut stack = vec![root];
        let mut out = String::new();
        while let Some(id) = stack.pop() {
            if doc.nodes[id].tag == "#text" {
                out.push_str(&doc.nodes[id].text);
            }
            stack.extend(doc.nodes[id].children.iter().rev().copied());
        }
        out
    }

    fn settle(browser: &mut Browser) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            assert!(
                Instant::now() < deadline,
                "timed out settling browser: status={}",
                browser.status()
            );
            browser.poll();
            if !browser.is_loading() {
                match browser.script_work_status() {
                    ScriptWorkStatus::Idle
                    | ScriptWorkStatus::NoSession
                    | ScriptWorkStatus::Blocked => break,
                    ScriptWorkStatus::Pending => {}
                    ScriptWorkStatus::Scheduled(at) => {
                        if Instant::now() < at {
                            let wait = at
                                .saturating_duration_since(Instant::now())
                                .min(Duration::from_millis(10));
                            thread::sleep(wait);
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    struct LocalServer {
        origin: String,
        stop: Arc<AtomicBool>,
        join: Option<thread::JoinHandle<()>>,
    }

    impl Drop for LocalServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(addr) = self.origin.strip_prefix("http://") {
                let _ = TcpStream::connect(addr);
            }
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    fn spawn_server<F>(handler: F) -> LocalServer
    where
        F: Fn(&str, &str, &HashMap<String, String>, &[u8]) -> (u16, Vec<(String, String)>, Vec<u8>)
            + Send
            + Sync
            + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let handler = Arc::new(handler);
        let join = thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(mut stream) = stream else {
                    continue;
                };
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                let h = handler.clone();
                thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut req_line = String::new();
                    if reader.read_line(&mut req_line).is_err() || req_line.trim().is_empty() {
                        return;
                    }
                    let mut parts = req_line.split_whitespace();
                    let method = parts.next().unwrap_or("GET").to_owned();
                    let path = parts.next().unwrap_or("/").to_owned();
                    let mut headers = HashMap::new();
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty()
                        {
                            break;
                        }
                        if let Some((k, v)) = line.split_once(':') {
                            let key = k.trim().to_ascii_lowercase();
                            let val = v.trim().to_owned();
                            headers
                                .entry(key)
                                .and_modify(|prev: &mut String| {
                                    prev.push_str("; ");
                                    prev.push_str(&val);
                                })
                                .or_insert(val);
                        }
                    }
                    let content_len = headers
                        .get("content-length")
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0);
                    let mut body = vec![0u8; content_len];
                    if content_len > 0 {
                        let _ = reader.read_exact(&mut body);
                    }
                    let (status, resp_headers, resp_body) = h(&method, &path, &headers, &body);
                    let reason = match status {
                        200 => "OK",
                        201 => "Created",
                        404 => "Not Found",
                        _ => "Status",
                    };
                    let mut head = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
                        resp_body.len()
                    );
                    for (k, v) in resp_headers {
                        head.push_str(&format!("{k}: {v}\r\n"));
                    }
                    head.push_str("\r\n");
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&resp_body);
                    let _ = stream.flush();
                });
            }
        });
        LocalServer {
            origin: format!("http://127.0.0.1:{port}"),
            stop,
            join: Some(join),
        }
    }

    fn new_browser() -> Browser {
        let scripts = Arc::new(LinuxScripts::default());
        let mut b = Browser::new(fonts(), scripts);
        b.set_scripts_enabled(true);
        b
    }

    fn test_crypto_storage_and_clear_browsing_data() {
        let srv = spawn_server(|_method, path, _headers, _body| match path {
            "/step1" => (
                200,
                vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
                br#"<!doctype html><html><body><p id="out"></p><script>
                  const bytes = new Uint8Array(32);
                  crypto.getRandomValues(bytes);
                  const nonZero = bytes.some(b => b !== 0);
                  const uuid = crypto.randomUUID();
                  localStorage.setItem('persistKey', 'persisted-value');
                  sessionStorage.setItem('tabKey', 'session-value');
                  document.getElementById('out').textContent = nonZero + ':' + uuid;
                </script></body></html>"#
                    .to_vec(),
            ),
            "/step2" => (
                200,
                vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
                br#"<!doctype html><html><body><p id="out"></p><script>
                  const uuid2 = crypto.randomUUID();
                  document.getElementById('out').textContent = [
                    String(localStorage.getItem('persistKey')),
                    String(sessionStorage.getItem('tabKey')),
                    uuid2
                  ].join('|');
                </script></body></html>"#
                    .to_vec(),
            ),
            _ => (404, vec![], b"missing".to_vec()),
        });

        let mut browser = new_browser();
        browser.navigate(format!("{}/step1", srv.origin), None, true);
        settle(&mut browser);
        let out1 = text_of(browser.document(), "#out");
        let (nz, uuid1) = out1.split_once(':').expect("step1 output");
        assert_eq!(nz, "true", "worker crypto must have real random seed");
        assert_eq!(uuid1.len(), 36);

        // Navigate to /step2 -> both localStorage and sessionStorage persist across navigations,
        // and the new worker gets a fresh independent random seed!
        browser.navigate(format!("{}/step2", srv.origin), None, true);
        settle(&mut browser);
        let out2 = text_of(browser.document(), "#out");
        let parts: Vec<&str> = out2.split('|').collect();
        assert_eq!(parts[0], "persisted-value");
        assert_eq!(parts[1], "session-value");
        assert_ne!(
            parts[2], uuid1,
            "distinct worker sessions must have distinct seeds"
        );

        // Clear browsing data -> wipes localStorage and sessionStorage!
        browser.clear_browsing_data();
        browser.navigate(format!("{}/step2", srv.origin), None, true);
        settle(&mut browser);
        let out3 = text_of(browser.document(), "#out");
        assert!(
            out3.starts_with("null|null|"),
            "expected cleared storage, got {out3}"
        );
    }

    fn test_fetch_xhr_cookies_and_httponly_isolation() {
        let srv = spawn_server(|method, path, headers, body| {
            match path {
            "/app" => (
                200,
                vec![
                    ("Content-Type".into(), "text/html; charset=utf-8".into()),
                    (
                        "Set-Cookie".into(),
                        "secret=http-only-val; HttpOnly; Path=/".into(),
                    ),
                    ("Set-Cookie".into(), "visible=pub-1; Path=/".into()),
                ],
                br#"<!doctype html><html><body><p id="out">pending</p><script>
                  (async () => {
                    const beforeCookie = document.cookie;
                    // Attempt to overwrite HttpOnly cookie from JS (must be rejected)
                    document.cookie = 'secret=spoofed; Path=/';
                    document.cookie = 'client=added-by-js; Path=/';

                    // 1. Same-origin POST fetch with JSON + custom header
                    const r = await fetch('/api/echo', {
                      method: 'POST',
                      headers: { 'Content-Type': 'application/json', 'X-Custom': 'm2-ok' },
                      body: JSON.stringify({ hello: 'world' })
                    });
                    const leakedSetCookie = r.headers.get('set-cookie');
                    const data = await r.json();

                    // 2. Cross-origin fetch must reject with TypeError
                    let crossRejected = false;
                    try {
                      await fetch('http://127.0.0.1:1/forbidden');
                    } catch (e) {
                      crossRejected = e instanceof TypeError;
                    }

                    // 3. AbortController pre-aborted fetch rejects with AbortError
                    const ac = new AbortController();
                    ac.abort();
                    let abortName = '';
                    try {
                      await fetch('/api/echo', { signal: ac.signal });
                    } catch (e) {
                      abortName = e.name;
                    }

                    // 4. Async XMLHttpRequest
                    const xhrText = await new Promise((resolve, reject) => {
                      const xhr = new XMLHttpRequest();
                      xhr.open('GET', '/api/xhr-ping');
                      xhr.setRequestHeader('X-XHR', 'yes');
                      xhr.onload = () => resolve(xhr.status + ':' + xhr.responseText + ':' + xhr.getResponseHeader('x-pong'));
                      xhr.onerror = () => reject(new Error('xhr network error'));
                      xhr.send();
                    });

                    document.getElementById('out').textContent = [
                      'initCookie=' + beforeCookie,
                      'wireCookie=' + data.cookie,
                      'custom=' + data.custom,
                      'body=' + data.body,
                      'leaked=' + leakedSetCookie,
                      'afterCookie=' + document.cookie,
                      'cross=' + crossRejected,
                      'abort=' + abortName,
                      'xhr=' + xhrText
                    ].join(' | ');
                  })();
                </script></body></html>"#
                    .to_vec(),
            ),
            "/api/echo" => {
                let cookie = headers.get("cookie").cloned().unwrap_or_default();
                let custom = headers.get("x-custom").cloned().unwrap_or_default();
                let body_str = String::from_utf8_lossy(body);
                let json = serde_json::json!({
                    "method": method,
                    "cookie": cookie,
                    "custom": custom,
                    "body": body_str,
                })
                .to_string();
                (
                    200,
                    vec![
                        ("Content-Type".into(), "application/json".into()),
                        ("Set-Cookie".into(), "from_fetch=yes; Path=/".into()),
                        (
                            "Set-Cookie".into(),
                            "hidden_fetch=topsecret; HttpOnly; Path=/".into(),
                        ),
                    ],
                    json.into_bytes(),
                )
            }
            "/api/xhr-ping" => {
                let x = headers.get("x-xhr").cloned().unwrap_or_default();
                (
                    200,
                    vec![
                        ("Content-Type".into(), "text/plain".into()),
                        ("X-Pong".into(), format!("ack-{x}")),
                    ],
                    b"pong-body".to_vec(),
                )
            }
            _ => (404, vec![], b"missing".to_vec()),
        }
        });

        let mut browser = new_browser();
        browser.navigate(format!("{}/app", srv.origin), None, true);
        settle(&mut browser);
        let out = text_of(browser.document(), "#out");
        assert!(
            out.contains("initCookie=visible=pub-1"),
            "HttpOnly cookie must not appear in initial document.cookie; got: {out}"
        );
        assert!(
            out.contains("secret=http-only-val") && out.contains("client=added-by-js"),
            "Wire Cookie header must carry both unmodified HttpOnly cookie and JS cookie; got: {out}"
        );
        assert!(
            !out.contains("secret=spoofed"),
            "JS must not overwrite HttpOnly cookie; got: {out}"
        );
        assert!(out.contains("custom=m2-ok"), "got: {out}");
        assert!(out.contains("body={\"hello\":\"world\"}"), "got: {out}");
        assert!(
            out.contains("leaked=null"),
            "Set-Cookie must be stripped from fetch Headers; got: {out}"
        );
        assert!(
            out.contains("from_fetch=yes") && !out.contains("hidden_fetch"),
            "Updated non-HttpOnly Set-Cookie must sync to document.cookie without leaking HttpOnly; got: {out}"
        );
        assert!(
            out.contains("cross=true"),
            "Cross-origin fetch must reject; got: {out}"
        );
        assert!(
            out.contains("abort=AbortError"),
            "Aborted fetch must reject with AbortError; got: {out}"
        );
        assert!(
            out.contains("xhr=200:pong-body:ack-yes"),
            "Async XHR failed; got: {out}"
        );
    }

    fn test_same_document_history_push_replace_popstate_and_hashchange() {
        let srv = spawn_server(|_method, _path, _headers, _body| {
            (
                200,
                vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
                br#"<!doctype html><html><body>
                  <button id="step" type="button">Push</button>
                  <p id="out"></p>
                  <script>
                    window.__aliveToken = 'same-worker-' + crypto.randomUUID();
                    const events = [];
                    window.addEventListener('popstate', (e) => {
                      events.push('pop:' + location.pathname + location.hash + ':' + JSON.stringify(e.state));
                      render();
                    });
                    window.addEventListener('hashchange', (e) => {
                      events.push('hash:' + new URL(e.oldURL).hash + '->' + new URL(e.newURL).hash);
                      render();
                    });
                    function render() {
                      document.getElementById('out').textContent = [
                        window.__aliveToken,
                        location.pathname + location.hash,
                        history.length,
                        JSON.stringify(history.state),
                        events.join(';')
                      ].join(' | ');
                    }
                    history.replaceState({ page: 'home' }, '', '/start');
                    history.pushState({ page: 'items' }, '', '/items');
                    history.pushState({ page: 'details' }, '', '/items#sec1');
                    render();
                  </script>
                </body></html>"#
                    .to_vec(),
            )
        });

        let mut browser = new_browser();
        browser.navigate(format!("{}/initial", srv.origin), None, true);
        settle(&mut browser);

        let initial_out = text_of(browser.document(), "#out");
        let token = initial_out.split(" | ").next().unwrap().to_owned();
        assert!(token.starts_with("same-worker-"));
        assert!(
            browser.page_url().ends_with("/items#sec1"),
            "page_url after pushState: {}",
            browser.page_url()
        );
        assert!(initial_out.contains(" | /items#sec1 | 3 | {\"page\":\"details\"} | "));

        // 1. Traverse back from /items#sec1 -> /items (only hash differs -> popstate AND hashchange!)
        browser.back();
        settle(&mut browser);
        let back1 = text_of(browser.document(), "#out");
        assert!(
            back1.starts_with(&token),
            "Same-document back() must preserve the live worker without reloading! got: {back1}"
        );
        assert!(browser.page_url().ends_with("/items"));
        assert!(
            back1.contains("pop:/items:{\"page\":\"items\"}") && back1.contains("hash:#sec1->"),
            "got: {back1}"
        );

        // 2. Traverse back again -> /start (path differs -> popstate only, with { page: "home" })
        browser.back();
        settle(&mut browser);
        let back2 = text_of(browser.document(), "#out");
        assert!(back2.starts_with(&token));
        assert!(browser.page_url().ends_with("/start"));
        assert!(
            back2.contains("pop:/start:{\"page\":\"home\"}"),
            "got: {back2}"
        );

        // 3. Traverse forward -> /items
        browser.forward();
        settle(&mut browser);
        let fwd = text_of(browser.document(), "#out");
        assert!(fwd.starts_with(&token));
        assert!(browser.page_url().ends_with("/items"));
        assert!(
            fwd.contains(" | /items | 3 | {\"page\":\"items\"} | "),
            "got: {fwd}"
        );
    }

    fn test_select_dropdown_popup_interaction_dispatches_to_worker() {
        let srv = spawn_server(|_method, _path, _headers, _body| {
            (
                200,
                vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
                br#"<!doctype html><html><body style="margin: 20px">
                  <select id="color" name="color">
                    <option value="red">Red</option>
                    <option value="green">Green</option>
                    <option value="blue">Blue</option>
                  </select>
                  <p id="out">none</p>
                  <script>
                    const sel = document.getElementById('color');
                    const log = [];
                    sel.addEventListener('input', () => log.push('input:' + sel.value));
                    sel.addEventListener('change', () => {
                      log.push('change:' + sel.value + ':' + sel.selectedIndex);
                      document.getElementById('out').textContent = log.join(',');
                    });
                  </script>
                </body></html>"#
                    .to_vec(),
            )
        });

        let mut browser = new_browser();
        browser.resize(800, 600);
        browser.navigate(format!("{}/select", srv.origin), None, true);
        settle(&mut browser);
        let _ = browser.paint();

        // Click on the <select> control at (30, 64 + 28) to open the dropdown popup!
        let click_x = 40;
        let click_y = 64 + 26;
        browser.pointer_down(click_x, click_y);
        browser.pointer_up(click_x, click_y);
        let _ = browser.paint();

        // Click the 3rd option ("Blue", index 2) in the open dropdown popup below the <select>!
        let option_y = click_y + 28 + 2 * 26 + 10;
        browser.pointer_down(click_x, option_y);
        browser.pointer_up(click_x, option_y);
        settle(&mut browser);

        assert_eq!(
            text_of(browser.document(), "#out"),
            "input:blue,change:blue:2"
        );
    }

    pub fn run_all() {
        test_crypto_storage_and_clear_browsing_data();
        test_fetch_xhr_cookies_and_httponly_isolation();
        test_same_document_history_push_replace_popstate_and_hashchange();
        test_select_dropdown_popup_interaction_dispatches_to_worker();
        println!("m2_platform_session: all 4 end-to-end suites passed");
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--script-worker") => mg_browser::platform::script_worker::worker_entry(),
        Some("--script-session") => mg_browser::platform::script_worker::session_entry(),
        _ => {}
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    linux::run_all();
}
