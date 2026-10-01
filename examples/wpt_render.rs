//! Static WPT adapter over the real Chassis navigation, resources and paint path.
//!
//! This is not WebDriver or a script/testharness executor. The caller serves a
//! reviewed pinned static selection on loopback and enforces the outer timeout.
//! Run: wpt_render http://127.0.0.1:PORT/test.html FONT.ttf OUTPUT_PREFIX
use mg_chassis::{Browser, scripts::DisabledScripts};
use mg_sparkle::{
    paint::{Canvas, Fonts},
    render::{self, Controls, Viewport},
};
use serde_json::json;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use url::Url;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const LOAD_TIMEOUT: Duration = Duration::from_secs(35);

fn local_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|error| format!("Invalid test URL: {error}"))?;
    let authority = value
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .ok_or("Test URL must use http://127.0.0.1 with an explicit port")?;
    let port = authority
        .strip_prefix("127.0.0.1:")
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or("Test URL must use http://127.0.0.1 with an explicit nonzero port")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(port)
    {
        return Err("Test URL must be loopback HTTP without credentials, query or fragment".into());
    }
    Ok(url)
}

fn artifact_path(prefix: &Path, suffix: &str) -> PathBuf {
    let mut path: OsString = prefix.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

fn check_final_url(requested: &Url, value: &str) -> Result<(), String> {
    let final_url = Url::parse(value).map_err(|error| format!("Invalid final URL: {error}"))?;
    // URL serialization removes an explicit default :80 port. Compare the
    // parsed origin rather than rejecting that legitimate normalization.
    if requested.origin() != final_url.origin()
        || !final_url.username().is_empty()
        || final_url.password().is_some()
        || final_url.query().is_some()
        || final_url.fragment().is_some()
    {
        return Err("Test navigation changed origin or added credentials/query/fragment".into());
    }
    Ok(())
}

fn rgb_bytes(canvas: &Canvas) -> Result<Vec<u8>, String> {
    if canvas.width != WIDTH
        || canvas.height != HEIGHT
        || canvas.pixels.len() != WIDTH as usize * HEIGHT as usize
        || canvas.scale() != 1.0
    {
        return Err("Expected an exact 800x600 viewport at scale 1".into());
    }
    let mut rgb = Vec::with_capacity(canvas.pixels.len() * 3);
    for &pixel in &canvas.pixels {
        rgb.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
    }
    Ok(rgb)
}

fn render_page(url: &str, font_path: &Path, prefix: &Path) -> Result<(), String> {
    let requested = local_url(url)?;
    let font = fs::read(font_path).map_err(|error| format!("Cannot read font: {error}"))?;
    let mut browser = Browser::new(Fonts::from_bytes(font.clone())?, Arc::new(DisabledScripts));
    browser.set_chrome(false);
    browser.resize(WIDTH, HEIGHT);
    browser.navigate(requested.to_string(), None, false);
    let deadline = Instant::now() + LOAD_TIMEOUT;
    while browser.is_loading() {
        if Instant::now() >= deadline {
            return Err(format!("Navigation timed out: {}", browser.status()));
        }
        browser.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    if !browser.last_load_succeeded() {
        return Err(format!("Navigation failed: {}", browser.status()));
    }
    check_final_url(&requested, browser.page_url())?;
    let canvas = browser.paint();
    let rgb = rgb_bytes(&canvas)?;

    // Browser keeps paint diagnostics private. Render its actual loaded DOM
    // again with identical inputs, then require byte-identical output before
    // associating the second pass's diagnostics with Browser::paint's pixels.
    let frame = render::render(
        browser.document(),
        &mut Fonts::from_bytes(font)?,
        Viewport {
            width: WIDTH,
            height: HEIGHT,
            scroll: 0,
        },
        &Controls::default(),
    );
    if canvas.width != frame.canvas.width
        || canvas.height != frame.canvas.height
        || canvas.pixels != frame.canvas.pixels
    {
        return Err("Diagnostic replay differs from the actual Browser::paint output".into());
    }
    let diagnostics: Vec<_> = frame
        .diagnostics
        .entries
        .iter()
        .map(|diagnostic| {
            json!({
                "kind": diagnostic.kind,
                "message": diagnostic.message,
                "source": diagnostic.source,
                "node": diagnostic.node,
                "line": diagnostic.line,
                "column": diagnostic.column,
                "truncated": diagnostic.truncated,
            })
        })
        .collect();
    let report = json!({
        "schema_version": 1,
        "url": url,
        "final_url": browser.page_url(),
        "width": WIDTH,
        "height": HEIGHT,
        "scale": 1,
        "non_uniform": canvas.pixels.iter().any(|pixel| *pixel != canvas.pixels[0]),
        "content_height": frame.content_height,
        "diagnostics": diagnostics,
        "diagnostics_omitted": frame.diagnostics.omitted,
        "resource_warnings": browser.document().resource_warnings,
        "stylesheets": browser.document().stylesheets.len(),
        "resources": browser.document().resources.len(),
        "status": browser.status(),
        "scripts": false,
        "browser_replay_equal": true,
    });
    let png_path = artifact_path(prefix, ".png");
    canvas.save_png(png_path.to_str().ok_or("PNG path is not UTF-8")?)?;
    fs::write(artifact_path(prefix, ".rgb"), rgb)
        .map_err(|error| format!("Cannot write RGB artifact: {error}"))?;
    fs::write(
        artifact_path(prefix, ".json"),
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Cannot write report: {error}"))?;
    println!("WPT_RENDER_OK {} {}x{}", browser.page_url(), WIDTH, HEIGHT);
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 || args[2].is_empty() {
        return Err(
            "Usage: wpt_render http://127.0.0.1:PORT/test.html FONT.ttf OUTPUT_PREFIX".into(),
        );
    }
    render_page(&args[0], Path::new(&args[1]), Path::new(&args[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_only_explicit_port_loopback_http() {
        for url in [
            "http://127.0.0.1:8000/test.html",
            "http://127.0.0.1:80/test.html",
            "http://127.0.0.1:65535/",
        ] {
            assert!(local_url(url).is_ok(), "{url}");
        }
        for url in [
            "https://127.0.0.1:8000/test.html",
            "http://localhost:8000/test.html",
            "http://127.0.0.2:8000/test.html",
            "http://[::1]:8000/test.html",
            "http://127.0.0.1/test.html",
            "http://127.0.0.1:0/test.html",
            "http://127.0.0.1:65536/test.html",
            "http://user@127.0.0.1:8000/test.html",
            "http://user:password@127.0.0.1:8000/test.html",
            "http://127.0.0.1:8000/test.html?variant=1",
            "http://127.0.0.1:8000/test.html#fragment",
            "http://2130706433:8000/test.html",
            "file:///tmp/test.html",
        ] {
            assert!(local_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn artifact_suffix_does_not_replace_prefix_extension() {
        assert_eq!(
            artifact_path(Path::new("results/test.ref"), ".json"),
            PathBuf::from("results/test.ref.json")
        );
    }

    #[test]
    fn final_url_allows_default_port_normalization_but_not_origin_changes() {
        let requested = local_url("http://127.0.0.1:80/test.html").unwrap();
        assert!(check_final_url(&requested, "http://127.0.0.1/test.html").is_ok());
        for value in [
            "http://127.0.0.1:8000/test.html",
            "http://example.com/test.html",
            "http://user@127.0.0.1/test.html",
            "http://127.0.0.1/test.html?variant=1",
            "http://127.0.0.1/test.html#fragment",
        ] {
            assert!(check_final_url(&requested, value).is_err(), "{value}");
        }
    }

    #[test]
    fn rgb_conversion_preserves_channels_and_exact_length() {
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 0x12_34_56);
        canvas.pixels[1] = 0xfe_dc_ba;
        let rgb = rgb_bytes(&canvas).unwrap();
        assert_eq!(rgb.len(), WIDTH as usize * HEIGHT as usize * 3);
        assert_eq!(&rgb[..6], &[0x12, 0x34, 0x56, 0xfe, 0xdc, 0xba]);
    }

    #[test]
    fn rejects_wrong_surface_size_and_truncated_pixels() {
        assert!(rgb_bytes(&Canvas::new(WIDTH - 1, HEIGHT, 0)).is_err());
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 0);
        canvas.pixels.pop();
        assert!(rgb_bytes(&canvas).is_err());
        assert!(rgb_bytes(&Canvas::new_scaled(WIDTH, HEIGHT, 0, 1.25)).is_err());
    }
}
