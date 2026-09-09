//! Embed browser navigation and rendering without a native window or toolbar.
use mg_chassis::{Browser, scripts::DisabledScripts};
use mg_sparkle::paint::Fonts;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("Usage: headless URL FONT.ttf OUTPUT.png".into());
    }
    let fonts = Fonts::from_bytes(std::fs::read(&args[1]).map_err(|error| error.to_string())?)?;
    let mut browser = Browser::new(fonts, Arc::new(DisabledScripts));
    browser.set_chrome(false);
    browser.resize(800, 600);
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
    println!("{}", browser.page_url());
    Ok(())
}
