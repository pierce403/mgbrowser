# Running the research browser

The current target is Linux with an X11 or XWayland display. The application uses
Rust X11 protocol code and software-rendered pixels; it does not embed another
browser, a native UI toolkit, or a native font renderer.

```sh
cargo run --locked --bin mgbrowser -- https://www.google.com/
```

A readable font file is required. The browser tries DejaVu Sans and Liberation
Sans at common Linux locations. Set `MGBROWSER_FONT=/absolute/path/font.ttf` to
choose another file; this reads font data and does not call a platform font API.

Use Ctrl+L to edit the URL, Enter to navigate, Tab to move between document input
fields, Enter in a field to submit its form, and the mouse to activate links and
buttons. Scroll with the wheel/Page Up/Page Down; Back/Next/Reload controls are in
the toolbar. Text editing initially supports typing, select-all, and Backspace;
there is no clipboard or full cursor/selection editor yet.

## What it renders

HTML is parsed by our own bounded tokenizer/tree builder, then flattened into a
simple flowing document with headings, text, links, and form controls. Rustybuzz
shapes text and fontdue rasterizes it. This is an early document view: full CSS,
JavaScript, font fallback/bidi layout, and downloaded image rendering are not yet
implemented. Images show a placeholder and alt text. Those limits are visible
in the browser and must not be mistaken for compatibility with the modern web.

HTTP(S) uses our own HTTP/1.1 transport and the selected experimental RustCrypto
TLS provider, with public trust roots and certificate verification. Cookies are
memory-only. HTML zero-delay refreshes are bounded. There is no browser sandbox,
credential store, HTTP/2, proxy configuration, or persistent browsing profile.

## Repeatable local interaction check

In one terminal:

```sh
cargo run --locked --example journey_server
```

In another:

```sh
cargo run --locked --bin mgbrowser -- http://127.0.0.1:7878/ \
  --smoke-search 'Rust & café' --exit-after-smoke --evidence-dir tmp/local-journey
```

The browser opens a real window and drives the same click, text, and Enter
handlers used by desktop input. It captures its rendered frames while opening
the fixture form, typing a query, submitting it to the local server, and clicking
the first result heading. It exits nonzero if any required stage fails. These
are scripted application-handler checks, not independent physical keyboard input
or proof about Google's server behavior. The local pages are explicitly labeled
fixtures and contain no fabricated Google results.

For the live target, use the same command with `https://www.google.com/` and
`--evidence-dir tmp/google-journey`. It uses the actual returned form controls,
actual heading links, and ordinary session behavior. A JavaScript/interstitial
response without results is a failed journey, not a pass. Keep public-network
checks manual and bounded; ordinary CI uses only the local fixture.

## Browser automation

Enable the experimental Chrome DevTools Protocol subset explicitly:

```sh
cargo run --locked --bin mgbrowser -- https://www.google.com/ --remote-debugging-port=9222
```

Discovery is at `http://127.0.0.1:9222/json/list`; the page WebSocket is
`ws://127.0.0.1:9222/devtools/page/page-1`. Port zero chooses an available port
and prints its address. Debugging is disabled by default and grants local clients
control of the page. See [CDP.md](CDP.md) for commands, limits, and the external
Rust fixture client. This is not yet full DevTools/Playwright compatibility.

## Validation

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
mkdir -p tmp
rustc --edition=2024 tools/check-dependencies.rs -o tmp/check-dependencies
tmp/check-dependencies
```

The unit/integration checks cover parser behavior, Rust font painting, verified
local TLS handshakes and rejection cases, HTTP framing, redirects, cookie scope,
and UI state transitions. Consult FEATURES.md and the daily log for which end-to-end
checks have actually run and what remains incomplete.
