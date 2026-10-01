//! Embed browser navigation and rendering without a native window or toolbar.
use mg_chassis::{Browser, scripts::DisabledScripts};
use mg_sparkle::paint::Fonts;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !matches!(args.len(), 3 | 5) {
        return Err("Usage: headless URL FONT.ttf OUTPUT.png [WIDTH HEIGHT]".into());
    }
    let bold = std::env::var_os("MGBROWSER_FONT_BOLD").and_then(|path| std::fs::read(path).ok());
    let fonts = Fonts::from_bytes_with_bold(
        std::fs::read(&args[1]).map_err(|error| error.to_string())?,
        bold,
    )?;
    let mut browser = Browser::new(fonts, Arc::new(DisabledScripts));
    browser.set_chrome(false);
    let width = args
        .get(3)
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(800);
    let height = args
        .get(4)
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(600);
    browser.resize(width, height);
    browser.navigate(args[0].clone(), None, true);
    let deadline = Instant::now() + Duration::from_secs(35);
    while browser.is_loading() {
        if Instant::now() >= deadline {
            return Err("Navigation timed out".into());
        }
        browser.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    if !browser.last_load_succeeded() {
        return Err(browser.status().into());
    }
    browser.paint().save_png(&args[2])?;
    println!("{}: {}", browser.page_url(), browser.status());
    Ok(())
}
