//! Independent native-input appearance acceptance on an isolated test D-Bus.
//! Run through tools/theme-smoke.sh, never against the user's session/config.
use std::{
    error::Error,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use x11rb::{connection::Connection, protocol::xproto::*, rust_connection::RustConnection};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Portal {
    scheme: Arc<AtomicU32>,
    reads: Arc<AtomicUsize>,
    legacy: Arc<AtomicBool>,
    legacy_reads: Arc<AtomicUsize>,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Portal {
    fn read_one(
        &self,
        namespace: &str,
        key: &str,
    ) -> zbus::fdo::Result<zbus::zvariant::OwnedValue> {
        if namespace != "org.freedesktop.appearance" || key != "color-scheme" {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Unexpected appearance key".into(),
            ));
        }
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.legacy.load(Ordering::SeqCst) {
            return Err(zbus::fdo::Error::UnknownMethod(
                "Settings v1 fixture".into(),
            ));
        }
        Ok(self.scheme.load(Ordering::SeqCst).into())
    }

    fn read(&self, namespace: &str, key: &str) -> zbus::fdo::Result<zbus::zvariant::OwnedValue> {
        if namespace != "org.freedesktop.appearance" || key != "color-scheme" {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Unexpected appearance key".into(),
            ));
        }
        self.legacy_reads.fetch_add(1, Ordering::SeqCst);
        zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::Value(Box::new(
            zbus::zvariant::Value::U32(self.scheme.load(Ordering::SeqCst)),
        )))
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
    }
}

struct Browser {
    child: Child,
    window: u32,
    conn: RustConnection,
    root: u32,
    width: u16,
    height: u16,
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Browser {
    fn launch(payload: &Path, scratch: &Path, label: &str) -> Result<Self> {
        let path = scratch.join(format!("{label}.log"));
        let log = File::create(&path)?;
        let mut child = Command::new(payload)
            .args(["http://127.0.0.1:7878/", "--no-auto-update"])
            .env("XDG_CONFIG_HOME", scratch.join("config"))
            .env("XDG_DATA_HOME", scratch.join("data"))
            .env("XDG_CACHE_HOME", scratch.join("cache"))
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()?;
        let window = (|| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if let Some(status) = child.try_wait()? {
                    return Err(format!("Browser exited {status}: {}", path.display()).into());
                }
                let contents = fs::read_to_string(&path)?;
                if contents.lines().any(|line| line.starts_with("LOADED "))
                    && let Some(window) = contents.lines().find_map(|line| {
                        line.strip_prefix("WINDOW id=")?
                            .split_whitespace()
                            .next()?
                            .parse::<u32>()
                            .ok()
                    })
                {
                    return Ok(window);
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(format!("Browser did not load fixture: {}", path.display()).into())
        })();
        let window = match window {
            Ok(window) => window,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let geometry = conn.get_geometry(window)?.reply()?;
        Ok(Self {
            child,
            window,
            conn,
            root,
            width: geometry.width,
            height: geometry.height,
        })
    }

    fn click(&self, x: i16, y: i16) -> Result<()> {
        for (response_type, mask) in [
            (BUTTON_PRESS_EVENT, EventMask::BUTTON_PRESS),
            (BUTTON_RELEASE_EVENT, EventMask::BUTTON_RELEASE),
        ] {
            self.conn.send_event(
                false,
                self.window,
                mask,
                ButtonPressEvent {
                    response_type,
                    detail: 1,
                    sequence: 0,
                    time: 0,
                    root: self.root,
                    event: self.window,
                    child: 0,
                    root_x: x,
                    root_y: y,
                    event_x: x,
                    event_y: y,
                    state: KeyButMask::default(),
                    same_screen: true,
                },
            )?;
        }
        self.conn.flush()?;
        thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    fn settings_origin(&self) -> (i16, i16, i16) {
        let width = self.width.saturating_sub(40).min(600);
        (
            ((self.width - width) / 2) as i16,
            self.height.saturating_sub(236).min(125) as i16,
            ((width - 48) / 3) as i16,
        )
    }

    fn open_settings(&self) -> Result<()> {
        self.click(self.width as i16 - 60, 75)?;
        self.click(self.width as i16 - 180, 200)
    }

    fn select(&self, index: i16) -> Result<()> {
        let (x, y, width) = self.settings_origin();
        self.click(x + 16 + index * (width + 8) + width / 2, y + 90)
    }

    fn close_settings(&self) -> Result<()> {
        let (x, y, _) = self.settings_origin();
        self.click(x + 50, y + 200)
    }

    fn capture(&self) -> Result<Vec<u8>> {
        Ok(self
            .conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.window,
                0,
                0,
                self.width,
                self.height,
                u32::MAX,
            )?
            .reply()?
            .data)
    }

    fn pixel(&self, frame: &[u8], x: usize, y: usize) -> u32 {
        let offset = (y * usize::from(self.width) + x) * 4;
        u32::from_le_bytes(frame[offset..offset + 4].try_into().unwrap()) & 0xffffff
    }

    fn appearance_matches(&self, dark: bool) -> Result<bool> {
        let frame = self.capture()?;
        let expected = if dark { 0x202622 } else { 0xeef1e9 };
        let atom = self
            .conn
            .intern_atom(false, b"_GTK_THEME_VARIANT")?
            .reply()?
            .atom;
        let utf8 = self.conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
        let hint = self
            .conn
            .get_property(false, self.window, atom, utf8, 0, 16)?
            .reply()?;
        let expected_hint: &[u8] = if dark { b"dark" } else { b"" };
        Ok(
            self.pixel(&frame, 0, usize::from(self.height) - 2) == expected
                && self.pixel(&frame, 0, 0) == 0x9f202b
                && self.pixel(&frame, 241, 13) == if dark { 0x151b17 } else { 0xffffff }
                && hint.type_ == utf8
                && hint.format == 8
                && hint.value == expected_hint,
        )
    }

    fn wait_appearance(&self, dark: bool) -> Result<()> {
        if let Err(error) = wait_until("chrome palette and native theme hint", || {
            self.appearance_matches(dark)
        }) {
            let frame = self.capture()?;
            let atom = self
                .conn
                .intern_atom(false, b"_GTK_THEME_VARIANT")?
                .reply()?
                .atom;
            let hint = self
                .conn
                .get_property(false, self.window, atom, AtomEnum::ANY, 0, 16)?
                .reply()?;
            return Err(format!(
                "{error}: expected dark={dark}, status={:06x}, HTTP={:06x}, hint={hint:?}",
                self.pixel(&frame, 0, usize::from(self.height) - 2),
                self.pixel(&frame, 0, 0)
            )
            .into());
        }
        // LOADED is logged before the first upload; let that frame finish before
        // comparing the entire page or sending the next external click.
        thread::sleep(Duration::from_millis(50));
        let frame = self.capture()?;
        assert_eq!(self.pixel(&frame, 0, 0), 0x9f202b, "HTTP warning lost");
        assert_eq!(
            self.pixel(&frame, 241, 13),
            if dark { 0x151b17 } else { 0xffffff },
            "address field did not follow appearance"
        );
        Ok(())
    }

    fn page(&self) -> Result<Vec<u8>> {
        let frame = self.capture()?;
        // Native chrome ends at 108; exclude status and the chrome scrollbar.
        Ok((108..usize::from(self.height) - 29)
            .flat_map(|y| {
                let start = y * usize::from(self.width) * 4;
                frame[start..start + (usize::from(self.width) - 9) * 4]
                    .iter()
                    .copied()
            })
            .collect())
    }

    fn screenshot(&self, path: &Path) -> Result<()> {
        let rgb: Vec<_> = self
            .capture()?
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        image::RgbImage::from_raw(self.width.into(), self.height.into(), rgb)
            .ok_or("bad native screenshot size")?
            .save(path)?;
        Ok(())
    }
}

fn wait_until(label: &str, mut condition: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if condition()? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("Timed out waiting for {label}").into())
}

fn preference(scratch: &Path, expected: &str) -> Result<()> {
    wait_until("saved appearance preference", || {
        let path = scratch.join("config/mgbrowser/settings.json");
        let Ok(bytes) = fs::read(path) else {
            return Ok(false);
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        Ok(value["theme"] == expected)
    })
}

fn set_portal(scheme: &AtomicU32, reads: &AtomicUsize, value: u32) -> Result<()> {
    scheme.store(value, Ordering::SeqCst);
    // Observe two reads to exclude a request already in flight before the store.
    let before = reads.load(Ordering::SeqCst);
    wait_until("actual portal polls", || {
        Ok(reads.load(Ordering::SeqCst) >= before + 2)
    })
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--bus-config") {
        let path = args.get(2).ok_or("private bus config path required")?;
        // No service directories: loss of the fixture portal must not activate
        // desktop portal helpers or connect this private test to real settings.
        fs::write(
            path,
            r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
  <policy context="default">
    <allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/>
  </policy>
</busconfig>
"#,
        )?;
        return Ok(());
    }
    if std::env::var("MGBROWSER_THEME_PRIVATE_SESSION").as_deref() != Ok("1") {
        return Err("Use tools/theme-smoke.sh: this fixture requires its own D-Bus session".into());
    }
    let payload = PathBuf::from(args.get(1).ok_or("browser payload required")?);
    let scratch = PathBuf::from(args.get(2).ok_or("scratch directory required")?);
    let scheme = Arc::new(AtomicU32::new(1));
    let reads = Arc::new(AtomicUsize::new(0));
    let legacy = Arc::new(AtomicBool::new(false));
    let legacy_reads = Arc::new(AtomicUsize::new(0));
    let portal = zbus::blocking::connection::Builder::session()?
        .name("org.freedesktop.portal.Desktop")?
        .serve_at(
            "/org/freedesktop/portal/desktop",
            Portal {
                scheme: scheme.clone(),
                reads: reads.clone(),
                legacy: legacy.clone(),
                legacy_reads: legacy_reads.clone(),
            },
        )?
        .build()?;

    let browser = Browser::launch(&payload, &scratch, "default-system")?;
    browser.wait_appearance(true)?;
    let page = browser.page()?;
    browser.screenshot(&scratch.join("system-dark.png"))?;
    assert!(
        !scratch.join("config/mgbrowser/settings.json").exists(),
        "startup wrote a preference"
    );
    set_portal(&scheme, &reads, 2)?;
    browser.wait_appearance(false)?;
    assert_eq!(page, browser.page()?, "System theme recolored the page");
    println!("THEME_SYSTEM_LIVE_OK");
    browser.open_settings()?;
    browser.select(2)?;
    browser.wait_appearance(true)?;
    preference(&scratch, "dark")?;
    browser.screenshot(&scratch.join("settings-dark.png"))?;
    browser.close_settings()?;
    assert_eq!(page, browser.page()?, "explicit Dark recolored the page");
    set_portal(&scheme, &reads, 1)?;
    set_portal(&scheme, &reads, 2)?;
    browser.wait_appearance(true)?;
    drop(browser);

    let browser = Browser::launch(&payload, &scratch, "persisted-dark")?;
    browser.wait_appearance(true)?;
    assert_eq!(page, browser.page()?, "restart changed page pixels");
    browser.open_settings()?;
    browser.select(1)?;
    browser.wait_appearance(false)?;
    preference(&scratch, "light")?;
    browser.screenshot(&scratch.join("settings-light.png"))?;
    browser.close_settings()?;
    assert_eq!(page, browser.page()?, "explicit Light recolored the page");
    set_portal(&scheme, &reads, 1)?;
    browser.wait_appearance(false)?;
    drop(browser);

    let browser = Browser::launch(&payload, &scratch, "persisted-light")?;
    browser.wait_appearance(false)?;
    println!("THEME_EXPLICIT_PERSISTENCE_OK");
    browser.open_settings()?;
    browser.select(0)?;
    browser.wait_appearance(true)?;
    preference(&scratch, "system")?;
    browser.close_settings()?;
    for value in [0, 99, 2] {
        set_portal(&scheme, &reads, value)?;
        browser.wait_appearance(false)?;
        assert_eq!(page, browser.page()?, "portal fallback changed page pixels");
    }
    assert_eq!(
        legacy_reads.load(Ordering::SeqCst),
        0,
        "modern portal called old Read"
    );
    legacy.store(true, Ordering::SeqCst);
    set_portal(&scheme, &reads, 1)?;
    browser.wait_appearance(true)?;
    assert!(
        legacy_reads.load(Ordering::SeqCst) > 0,
        "old portal fallback was not exercised"
    );
    set_portal(&scheme, &reads, 2)?;
    browser.wait_appearance(false)?;
    println!("THEME_LEGACY_PORTAL_OK");
    set_portal(&scheme, &reads, 1)?;
    browser.wait_appearance(true)?;
    drop(portal);
    browser.wait_appearance(false)?;
    assert_eq!(
        page,
        browser.page()?,
        "portal disappearance changed page pixels"
    );
    drop(browser);

    let browser = Browser::launch(&payload, &scratch, "missing-portal")?;
    browser.wait_appearance(false)?;
    preference(&scratch, "system")?;
    browser.screenshot(&scratch.join("system-fallback-light.png"))?;
    assert_eq!(
        page,
        browser.page()?,
        "missing portal startup changed page pixels"
    );
    println!(
        "NATIVE_THEME_SMOKE_OK {} portal_reads={} legacy_reads={}",
        scratch.display(),
        reads.load(Ordering::SeqCst),
        legacy_reads.load(Ordering::SeqCst)
    );
    Ok(())
}
