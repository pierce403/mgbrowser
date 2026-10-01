//! Independent X11 tab/pane acceptance using only an owned loopback fixture.
//! Run with tools/tabs-smoke.sh, never against a user's display or profile.
use std::{
    error::Error,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use x11rb::{connection::Connection, protocol::xproto::*, rust_connection::RustConnection};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const STRIP: i32 = 34;
const WIDTH: i32 = 800;
const HEIGHT: i32 = 634;
const HALF: i32 = (WIDTH - 6) / 2;
const RIGHT: i32 = HALF + 6;
const A: u32 = 0xffefd5;
const B: u32 = 0xd9edff;
const C: u32 = 0xe1f6df;

struct Fixture {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let base = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (recorded, done) = (requests.clone(), stop.clone());
        let thread = thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut line = String::new();
                        if BufReader::new(&stream).read_line(&mut line).is_err() {
                            continue;
                        }
                        let target = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                        let mut requests = recorded.lock().unwrap();
                        if requests.len() < 4096 {
                            requests.push(target.clone());
                        }
                        drop(requests);
                        let name = target
                            .trim_start_matches('/')
                            .split(['/', '?'])
                            .next()
                            .unwrap_or("a");
                        let color = match name {
                            "a" | "script-a" => A,
                            "b" | "script-b" => B,
                            _ => C,
                        };
                        let counter = if name.starts_with("script-") {
                            format!(
                                "<script>document.title='READY {name}';let count=0; document.getElementById('count').addEventListener('click',function(event){{event.preventDefault();count++;document.getElementById('query').value='{name}-'+count;document.title='COUNTER {name} '+count;}});</script>"
                            )
                        } else {
                            String::new()
                        };
                        // The counter intentionally cancels its default submit
                        // action. Use a separate button to verify the retained
                        // form value without invoking that canceled action.
                        let submit = if name.starts_with("script-") {
                            "<button>Submit</button>"
                        } else {
                            ""
                        };
                        let body = format!(
                            "<!doctype html><html><head><title>Tab fixture {name}</title><style>body{{margin:12px;background:#{color:06x};font-size:14px;line-height:24px}}input{{width:180px;height:24px;padding:0;border:0}}button{{width:100px;height:24px}}</style></head><body><form action='/{name}/submit'><input id='query' name='q'><button id='count'>Check</button>{submit}</form><p>OWNED TAB FIXTURE {name}: edit before moving.</p><p>No public website or compatibility claim.</p>{counter}</body></html>"
                        );
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            base,
            requests,
            stop,
            thread: Some(thread),
        })
    }
    fn count(&self, target: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.as_str() == target)
            .count()
    }
    fn submission_count(&self, name: &str, value: &str) -> usize {
        self.count(&format!("/{name}/submit?q={value}"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn wait(label: &str, mut ready: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        if ready()? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(60));
    }
    Err(format!("Timed out: {label}").into())
}

struct Frame {
    width: u16,
    height: u16,
    bytes: Vec<u8>,
}
impl Frame {
    fn pixel(&self, x: usize, y: usize) -> u32 {
        let offset = (y * usize::from(self.width) + x) * 4;
        u32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap()) & 0xffffff
    }
    fn save(&self, path: &Path) -> Result<()> {
        let rgb = self
            .bytes
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[2], pixel[1], pixel[0]])
            .collect();
        image::RgbImage::from_raw(self.width.into(), self.height.into(), rgb)
            .ok_or("Invalid X11 image")?
            .save(path)?;
        Ok(())
    }
}

struct Browser {
    child: Child,
    conn: RustConnection,
    root: u32,
    scale: i32,
    scratch: PathBuf,
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Browser {
    fn start(
        payload: &Path,
        scratch: &Path,
        url: &str,
        scale: i32,
        theme: &str,
        restore: &[String],
        scripts: bool,
    ) -> Result<Self> {
        // Connect before spawning so an unavailable display cannot orphan a child.
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        for name in ["home", "config/mgbrowser", "data", "cache", "runtime"] {
            fs::create_dir_all(scratch.join(name))?;
        }
        fs::write(
            scratch.join("config/mgbrowser/settings.json"),
            format!("{{\"theme\":\"{theme}\",\"scale\":{scale}}}\n"),
        )?;
        let log = File::create(scratch.join("browser.log"))?;
        let mut command = Command::new(payload);
        command.args([
            url,
            "--no-auto-update",
            if scripts {
                "--enable-scripts"
            } else {
                "--disable-scripts"
            },
        ]);
        for url in restore {
            command.args(["--restore-tab", url]);
        }
        let child = command
            .env("HOME", scratch.join("home"))
            .env("XDG_CONFIG_HOME", scratch.join("config"))
            .env("XDG_DATA_HOME", scratch.join("data"))
            .env("XDG_CACHE_HOME", scratch.join("cache"))
            .env("XDG_RUNTIME_DIR", scratch.join("runtime"))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}/absent-bus", scratch.display()),
            )
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        let mut browser = Self {
            child,
            conn,
            root,
            scale,
            scratch: scratch.to_owned(),
        };
        wait("owned browser startup", || {
            browser.alive()?;
            Ok(browser.windows()?.len() == 1
                && fs::read_to_string(browser.scratch.join("browser.log"))?.contains("LOADED "))
        })?;
        Ok(browser)
    }
    fn alive(&mut self) -> Result<()> {
        if let Some(status) = self.child.try_wait()? {
            return Err(format!(
                "Browser exited {status}; inspect {}/browser.log",
                self.scratch.display()
            )
            .into());
        }
        Ok(())
    }
    fn windows(&self) -> Result<Vec<u32>> {
        let mut windows = Vec::new();
        for window in self.conn.query_tree(self.root)?.reply()?.children {
            let Ok(attributes) = self.conn.get_window_attributes(window)?.reply() else {
                continue;
            };
            if attributes.map_state != MapState::VIEWABLE {
                continue;
            }
            let class = self
                .conn
                .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)?
                .reply()?;
            if class
                .value
                .split(|byte| *byte == 0)
                .any(|part| part == b"mgbrowser")
            {
                windows.push(window);
            }
        }
        windows.sort_unstable();
        Ok(windows)
    }
    fn physical(&self, logical: i32) -> i16 {
        ((logical * self.scale + 50) / 100) as i16
    }
    fn place(&self, window: u32, x: i32, y: i32) -> Result<()> {
        self.conn.configure_window(
            window,
            &ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .width(self.physical(WIDTH) as u32)
                .height(self.physical(HEIGHT) as u32),
        )?;
        self.conn.flush()?;
        self.settled(window)?;
        Ok(())
    }
    fn frame(&self, window: u32) -> Result<Frame> {
        let g = self.conn.get_geometry(window)?.reply()?;
        let bytes = self
            .conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                window,
                0,
                0,
                g.width,
                g.height,
                u32::MAX,
            )?
            .reply()?
            .data;
        if bytes.len() != usize::from(g.width) * usize::from(g.height) * 4 {
            return Err("Expected 32-bit Xvfb image".into());
        }
        Ok(Frame {
            width: g.width,
            height: g.height,
            bytes,
        })
    }
    fn settled(&self, window: u32) -> Result<Frame> {
        let mut previous = self.frame(window)?;
        let mut equal = 0;
        wait("completed tab/pane paint", || {
            let next = self.frame(window)?;
            equal = if previous.bytes == next.bytes {
                equal + 1
            } else {
                0
            };
            previous = next;
            Ok(equal >= 3)
        })?;
        Ok(previous)
    }
    fn expect(&self, window: u32, left: u32, right: Option<u32>, label: &str) -> Result<()> {
        let result = wait(label, || {
            let frame = self.frame(window)?;
            // Sample the authored body's empty12px margin, never text that
            // can wrap differently after a compact resize. Pane boundaries
            // are device pixels, including the fractional-scale divider.
            let inset = self.physical(5) as usize;
            let y = self.physical(230) as usize;
            let divider = self.physical(6) as usize;
            let right_x = (usize::from(frame.width) - divider) / 2 + divider + inset;
            Ok(frame.pixel(inset, y) == left
                && right.is_none_or(|color| frame.pixel(right_x, y) == color))
        });
        if result.is_err() {
            self.frame(window)?
                .save(&self.scratch.join("failed-expectation.png"))?;
        }
        result?;
        self.settled(window)?;
        Ok(())
    }
    fn key(&self, window: u32, symbol: u32, ctrl: bool, extra_shift: bool) -> Result<()> {
        let first = self.conn.setup().min_keycode;
        let map = self
            .conn
            .get_keyboard_mapping(first, self.conn.setup().max_keycode - first + 1)?
            .reply()?;
        let (code, shift) = map
            .keysyms
            .chunks(usize::from(map.keysyms_per_keycode))
            .enumerate()
            .find_map(|(i, group)| {
                group
                    .iter()
                    .take(2)
                    .position(|&s| s == symbol)
                    .map(|column| (first + i as u8, column == 1))
            })
            .ok_or("Keysym unavailable")?;
        let mut state = KeyButMask::default();
        if ctrl {
            state |= KeyButMask::CONTROL;
        }
        if shift || extra_shift {
            state |= KeyButMask::SHIFT;
        }
        self.conn
            .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)?;
        for (response_type, mask) in [
            (KEY_PRESS_EVENT, EventMask::KEY_PRESS),
            (KEY_RELEASE_EVENT, EventMask::KEY_RELEASE),
        ] {
            self.conn.send_event(
                false,
                window,
                mask,
                KeyPressEvent {
                    response_type,
                    detail: code,
                    sequence: 0,
                    time: 0,
                    root: self.root,
                    event: window,
                    child: 0,
                    root_x: 0,
                    root_y: 0,
                    event_x: 0,
                    event_y: 0,
                    state,
                    same_screen: true,
                },
            )?;
        }
        self.conn.flush()?;
        Ok(())
    }
    fn text(&self, window: u32, text: &str) -> Result<()> {
        for ch in text.chars() {
            self.key(window, ch as u32, false, false)?;
        }
        self.settled(window)?;
        Ok(())
    }
    fn root_point(&self, window: u32, x: i32, y: i32) -> Result<(i16, i16)> {
        let point = self
            .conn
            .translate_coordinates(window, self.root, self.physical(x), self.physical(y))?
            .reply()?;
        Ok((point.dst_x, point.dst_y))
    }
    fn button_root(&self, window: u32, point: (i16, i16), pressed: bool) -> Result<()> {
        let local = self
            .conn
            .translate_coordinates(self.root, window, point.0, point.1)?
            .reply()?;
        self.conn
            .warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, point.0, point.1)?;
        self.conn.send_event(
            false,
            window,
            if pressed {
                EventMask::BUTTON_PRESS
            } else {
                EventMask::BUTTON_RELEASE
            },
            ButtonPressEvent {
                response_type: if pressed {
                    BUTTON_PRESS_EVENT
                } else {
                    BUTTON_RELEASE_EVENT
                },
                detail: 1,
                sequence: 0,
                time: 0,
                root: self.root,
                event: window,
                child: 0,
                root_x: point.0,
                root_y: point.1,
                event_x: local.dst_x,
                event_y: local.dst_y,
                state: if pressed {
                    KeyButMask::default()
                } else {
                    KeyButMask::BUTTON1
                },
                same_screen: true,
            },
        )?;
        self.conn.flush()?;
        thread::sleep(Duration::from_millis(35));
        Ok(())
    }
    fn click(&self, window: u32, x: i32, y: i32) -> Result<()> {
        let point = self.root_point(window, x, y)?;
        self.button_root(window, point, true)?;
        self.button_root(window, point, false)?;
        self.settled(window)?;
        Ok(())
    }
    fn motion(&self, window: u32, point: (i16, i16)) -> Result<()> {
        let local = self
            .conn
            .translate_coordinates(self.root, window, point.0, point.1)?
            .reply()?;
        self.conn
            .warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, point.0, point.1)?;
        self.conn.send_event(
            false,
            window,
            EventMask::POINTER_MOTION,
            MotionNotifyEvent {
                response_type: MOTION_NOTIFY_EVENT,
                detail: Motion::NORMAL,
                sequence: 0,
                time: 0,
                root: self.root,
                event: window,
                child: 0,
                root_x: point.0,
                root_y: point.1,
                event_x: local.dst_x,
                event_y: local.dst_y,
                state: KeyButMask::BUTTON1,
                same_screen: true,
            },
        )?;
        self.conn.flush()?;
        thread::sleep(Duration::from_millis(70));
        Ok(())
    }
    fn drag(
        &self,
        window: u32,
        from: (i32, i32),
        destination: (i16, i16),
        cancel: bool,
        evidence: &str,
    ) -> Result<()> {
        let start = self.root_point(window, from.0, from.1)?;
        self.button_root(window, start, true)?;
        self.motion(window, (start.0 + self.physical(14), start.1))?;
        self.motion(window, destination)?;
        self.settled(window)?
            .save(&self.scratch.join(format!("{evidence}-preview.png")))?;
        if cancel {
            self.key(window, 0xff1b, false, false)?;
        }
        self.button_root(window, destination, false)?;
        Ok(())
    }
    fn edit(&self, window: u32, pane_x: i32, text: &str) -> Result<()> {
        self.click(window, pane_x + 30, STRIP + 64 + 20)?;
        self.text(window, text)
    }
    fn navigate(&self, window: u32, url: &str, color: u32) -> Result<()> {
        self.key(window, b'l'.into(), true, false)?;
        self.text(window, url)?;
        self.key(window, 0xff0d, false, false)?;
        self.expect(window, color, None, "fixture navigation")
    }
    fn close_window(&self, window: u32) -> Result<()> {
        let protocols = self.conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
        let delete = self
            .conn
            .intern_atom(false, b"WM_DELETE_WINDOW")?
            .reply()?
            .atom;
        self.conn.send_event(
            false,
            window,
            EventMask::NO_EVENT,
            ClientMessageEvent::new(
                32,
                window,
                protocols,
                [delete, x11rb::CURRENT_TIME, 0, 0, 0],
            ),
        )?;
        self.conn.flush()?;
        Ok(())
    }
    fn expect_title(&self, window: u32, expected: &str) -> Result<()> {
        wait(expected, || {
            let title = self
                .conn
                .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
                .reply()?;
            Ok(String::from_utf8_lossy(&title.value).contains(expected))
        })?;
        self.settled(window)?;
        Ok(())
    }
}

fn exercise(payload: &Path, scratch: &Path, fixture: &Fixture, scale: i32) -> Result<()> {
    let dir = scratch.join(format!("scale-{scale}"));
    let theme = if scale == 100 { "dark" } else { "light" };
    let initial_requests = [
        fixture.count("/a"),
        fixture.count("/b"),
        fixture.count("/c"),
    ];
    let initial_submissions = [
        fixture.submission_count("a", "alphaleftsecond"),
        fixture.submission_count("b", "betarightseconddetacheddocked"),
    ];
    let mut browser = Browser::start(
        payload,
        &dir,
        &format!("{}/a", fixture.base),
        scale,
        theme,
        &[],
        false,
    )?;
    let window = browser.windows()?[0];
    browser.place(window, 40, 40)?;
    browser.expect(window, A, None, "first fixture")?;
    // Creating a blank second tab enables the strip without loading a public URL.
    browser.key(window, b't'.into(), true, false)?;
    browser.navigate(window, &format!("{}/b", fixture.base), B)?;
    browser.edit(window, 0, "beta")?;
    // X11 exposes Shift+Tab as ISO_Left_Tab, not a second plain Tab keysym.
    browser.key(window, 0xfe20, true, false)?;
    browser.expect(window, A, None, "Ctrl+Shift+Tab selects previous")?;
    browser.edit(window, 0, "alpha")?;
    browser.key(window, 0xff09, true, false)?;
    browser.expect(window, B, None, "Ctrl+Tab selects next")?;
    browser.key(window, b't'.into(), true, false)?;
    browser.navigate(window, &format!("{}/c", fixture.base), C)?;
    let themed = browser.settled(window)?;
    assert_eq!(
        themed.pixel(
            browser.physical(700) as usize,
            browser.physical(16) as usize
        ),
        if theme == "dark" { 0x202622 } else { 0xeef1e9 },
        "Actual native tab strip must follow saved theme"
    );
    assert_eq!(
        themed.pixel(browser.physical(5) as usize, browser.physical(230) as usize),
        C,
        "Browser theme must not replace the page's authored background"
    );
    themed.save(&dir.join("tab-strip-theme.png"))?;
    let before = [
        fixture.count("/a"),
        fixture.count("/b"),
        fixture.count("/c"),
    ];
    assert_eq!(
        before,
        initial_requests.map(|count| count + 1),
        "Each tab must actually request its exact fixture URL once"
    );
    browser.drag(
        window,
        (450, 16),
        browser.root_point(window, 5, 16)?,
        false,
        "reorder",
    )?;
    browser.expect(window, C, None, "reordered tab stays selected")?;
    browser.key(window, 0xff09, true, false)?;
    browser.expect(window, A, None, "reordered C precedes A")?;
    browser.key(window, 0xfe20, true, false)?;
    browser.expect(window, C, None, "reverse cycle reaches reordered C")?;
    // Closing a tab during its drag must release ownership and pointer grab.
    // A subsequent queued motion/release must not use the removed tab ID.
    let closing_drag = browser.root_point(window, 70, 16)?;
    browser.button_root(window, closing_drag, true)?;
    browser.motion(
        window,
        (closing_drag.0 + browser.physical(20), closing_drag.1),
    )?;
    browser.key(window, b'w'.into(), true, false)?;
    let stale_motion = browser.root_point(window, 120, 16)?;
    browser.motion(window, stale_motion)?;
    browser.button_root(window, stale_motion, false)?;
    browser.expect(window, A, None, "closing C leaves A alive")?;
    browser.alive()?;
    browser.drag(
        window,
        (250, 16),
        browser.root_point(window, WIDTH - 5, 200)?,
        false,
        "split-right",
    )?;
    browser.expect(window, A, Some(B), "B docks to right of A")?;
    // Direct X11 resize bypasses a window manager's minimum-size hints. The
    // host must still retain both complete360px logical Browser surfaces.
    browser.conn.configure_window(
        window,
        &ConfigureWindowAux::new()
            .width(browser.physical(400) as u32)
            .height(browser.physical(300) as u32),
    )?;
    browser.conn.flush()?;
    wait("compact split enforces complete pane widths", || {
        let geometry = browser.conn.get_geometry(window)?.reply()?;
        Ok(
            i32::from(geometry.width) >= i32::from(browser.physical(726))
                && i32::from(geometry.height) >= i32::from(browser.physical(274)),
        )
    })?;
    browser.expect(
        window,
        A,
        Some(B),
        "both compact split panes remain visible",
    )?;
    browser
        .settled(window)?
        .save(&dir.join("compact-split.png"))?;
    browser.place(window, 40, 40)?;
    browser.edit(window, 0, "left")?;
    browser.edit(window, RIGHT, "right")?;
    browser
        .settled(window)?
        .save(&dir.join("right-panes.png"))?;
    // A window has at most two groups. Merge before requesting a fresh left
    // split; this does not require an unimplemented direct pane swap.
    browser.drag(
        window,
        (RIGHT + 70, 16),
        browser.root_point(window, 230, 16)?,
        false,
        "merge-before-left",
    )?;
    browser.expect(window, B, None, "merge collapses empty right group")?;
    browser.drag(
        window,
        (250, 16),
        browser.root_point(window, 5, 200)?,
        false,
        "split-left",
    )?;
    browser.expect(window, B, Some(A), "B moves to left of A")?;
    browser.edit(window, 0, "second")?;
    browser.edit(window, RIGHT, "second")?;
    let detached_point = browser.root_point(window, WIDTH + 100, 100)?;
    browser.drag(window, (70, 16), detached_point, true, "cancel-detach")?;
    browser.expect(window, B, Some(A), "Escape preserves split placement")?;
    assert_eq!(
        browser.windows()?.len(),
        1,
        "Canceled drag created a window"
    );
    browser.drag(window, (70, 16), detached_point, false, "detach")?;
    wait("native detached window", || {
        Ok(browser.windows()?.len() == 2)
    })?;
    let detached = browser
        .windows()?
        .into_iter()
        .find(|id| *id != window)
        .ok_or("Missing detached window")?;
    browser.place(detached, 40 + i32::from(browser.physical(WIDTH + 60)), 40)?;
    browser.expect(window, A, None, "A survives detach")?;
    browser.expect(detached, B, None, "B appears in detached window")?;
    browser.edit(detached, 0, "detached")?;
    browser.settled(detached)?.save(&dir.join("detached.png"))?;
    browser.drag(
        detached,
        (70, 16),
        browser.root_point(window, 230, 16)?,
        false,
        "redock",
    )?;
    wait("redock removes emptied source window", || {
        Ok(browser.windows()? == [window])
    })?;
    browser.expect(window, B, None, "redocked B selected")?;
    browser.edit(window, 0, "docked")?;
    assert_eq!(
        [
            fixture.count("/a"),
            fixture.count("/b"),
            fixture.count("/c")
        ],
        before,
        "Moving/switching tabs reloaded live documents"
    );
    browser.key(window, 0xff0d, false, false)?;
    wait("B edited state survives all moves", || {
        Ok(
            fixture.submission_count("b", "betarightseconddetacheddocked")
                == initial_submissions[1] + 1,
        )
    })?;
    browser.key(window, 0xff09, true, false)?;
    browser.expect(window, A, None, "A after redock submission")?;
    browser.click(window, 30, STRIP + 64 + 20)?;
    browser.key(window, 0xff0d, false, false)?;
    wait("A retained state and focused-pane input", || {
        Ok(fixture.submission_count("a", "alphaleftsecond") == initial_submissions[0] + 1)
    })?;
    browser
        .settled(window)?
        .save(&dir.join("state-preserved.png"))?;
    // Detach a second time, close only that native window, then close the final one.
    browser.drag(window, (250, 16), detached_point, false, "final-detach")?;
    wait("second detach", || Ok(browser.windows()?.len() == 2))?;
    let detached = browser
        .windows()?
        .into_iter()
        .find(|id| *id != window)
        .ok_or("Missing second window")?;
    browser.close_window(detached)?;
    wait("closing extra window preserves original", || {
        Ok(browser.windows()? == [window])
    })?;
    browser.alive()?;
    browser.close_window(window)?;
    wait("only final-window close exits", || {
        Ok(browser
            .child
            .try_wait()?
            .is_some_and(|status| status.success()))
    })?;
    assert!(
        browser.windows()?.is_empty(),
        "Owned windows remain after exit"
    );
    println!("NATIVE_TABS_OK {scale}% {}", dir.display());
    Ok(())
}

fn restored_urls(payload: &Path, scratch: &Path, fixture: &Fixture) -> Result<()> {
    let dir = scratch.join("restored-urls");
    let restore = [format!("{}/b", fixture.base), format!("{}/c", fixture.base)];
    let mut browser = Browser::start(
        payload,
        &dir,
        &format!("{}/a", fixture.base),
        100,
        "light",
        &restore,
        false,
    )?;
    let window = browser.windows()?[0];
    browser.place(window, 40, 40)?;
    // Reopen contract is tested independently of the updater's synthetic version fixture.
    for (x, color) in [(70, A), (270, B), (470, C)] {
        browser.click(window, x, 16)?;
        browser.expect(window, color, None, "restored URL tab")?;
    }
    browser
        .settled(window)?
        .save(&dir.join("three-restored-tabs.png"))?;
    browser.close_window(window)?;
    wait("restored-window close", || {
        Ok(browser
            .child
            .try_wait()?
            .is_some_and(|status| status.success()))
    })?;
    println!("NATIVE_RESTORE_URLS_OK (startup restore, not updater-triggered restart)");
    Ok(())
}

fn scripted_realms(payload: &Path, scratch: &Path, fixture: &Fixture) -> Result<()> {
    let dir = scratch.join("scripted-realms");
    let mut browser = Browser::start(
        payload,
        &dir,
        &format!("{}/script-a", fixture.base),
        100,
        "light",
        &[],
        true,
    )?;
    let window = browser.windows()?[0];
    browser.place(window, 40, 40)?;
    browser.key(window, b't'.into(), true, false)?;
    browser.navigate(window, &format!("{}/script-b", fixture.base), B)?;
    browser.expect_title(window, "READY script-b")?;
    browser.click(window, 235, STRIP + 64 + 20)?;
    browser.expect_title(window, "COUNTER script-b 1")?;
    browser.key(window, 0xfe20, true, false)?;
    browser.expect(window, A, None, "first scripted tab selected")?;
    browser.expect_title(window, "READY script-a")?;
    browser.click(window, 235, STRIP + 64 + 20)?;
    browser.expect_title(window, "COUNTER script-a 1")?;
    let before = [fixture.count("/script-a"), fixture.count("/script-b")];
    browser.drag(
        window,
        (70, 16),
        browser.root_point(window, WIDTH + 100, 100)?,
        false,
        "script-detach",
    )?;
    wait(
        "scripted tab detaches",
        || Ok(browser.windows()?.len() == 2),
    )?;
    let detached = browser
        .windows()?
        .into_iter()
        .find(|id| *id != window)
        .ok_or("Missing scripted detached window")?;
    browser.place(detached, 950, 40)?;
    browser.expect(detached, A, None, "scripted A in new window")?;
    browser.click(detached, 235, STRIP + 64 + 20)?;
    browser.expect_title(detached, "COUNTER script-a 2")?;
    browser.drag(
        detached,
        (70, 16),
        browser.root_point(window, 230, 16)?,
        false,
        "script-redock",
    )?;
    wait("scripted redock closes empty window", || {
        Ok(browser.windows()? == [window])
    })?;
    browser.expect(window, A, None, "redocked scripted A selected")?;
    browser.click(window, 235, STRIP + 64 + 20)?;
    browser.expect_title(window, "COUNTER script-a 3")?;
    browser
        .settled(window)?
        .save(&dir.join("retained-counter-three.png"))?;
    assert_eq!(
        [fixture.count("/script-a"), fixture.count("/script-b")],
        before,
        "Scripted tab movement replayed its document"
    );
    // Close the still-live realm, not a tab that already navigated away from it.
    browser.key(window, b'w'.into(), true, false)?;
    browser.expect(window, B, None, "other scripted tab survives close")?;
    browser.expect_title(window, "COUNTER script-b 1")?;
    browser.click(window, 235, STRIP + 64 + 20)?;
    browser.expect_title(window, "COUNTER script-b 2")?;
    browser.click(window, 335, STRIP + 64 + 20)?;
    wait("independent surviving realm submits real value", || {
        Ok(fixture.submission_count("script-b", "script-b-2") == 1)
    })?;
    browser
        .settled(window)?
        .save(&dir.join("surviving-realm-submission.png"))?;
    browser.close_window(window)?;
    wait("scripted final-window close", || {
        Ok(browser
            .child
            .try_wait()?
            .is_some_and(|status| status.success()))
    })?;
    println!("NATIVE_TAB_REALMS_OK (owned inline scripts, retained and independent)");
    Ok(())
}

fn main() -> Result<()> {
    if std::env::var("MGBROWSER_TABS_PRIVATE_DISPLAY").as_deref() != Ok("1") {
        return Err("Run via tools/tabs-smoke.sh on its owned display".into());
    }
    let mut args = std::env::args_os().skip(1);
    let payload = PathBuf::from(args.next().ok_or("Expected packaged executable")?);
    let scratch = PathBuf::from(args.next().ok_or("Expected scratch directory")?);
    let fixture = Fixture::start()?;
    let result = (|| {
        for scale in [100, 125, 200] {
            exercise(&payload, &scratch, &fixture, scale)?;
        }
        scripted_realms(&payload, &scratch, &fixture)?;
        restored_urls(&payload, &scratch, &fixture)
    })();
    fs::write(
        scratch.join("requests.txt"),
        fixture.requests.lock().unwrap().join("\n"),
    )?;
    result
}
