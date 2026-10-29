# mgbrowser v0.9.23 Experimental Preview

Google News default-mode browsing and pure-Rust WOFF1/WOFF2 `@font-face` web
fonts: clean oversize-document script skipping, working scroll and link
activation, styled `Full coverage` (`/stories/...`) layout, and WOFF1/WOFF2
SFNT reconstruction with synthetic bold/oblique styling for downloaded faces.
Exact-source publication and public-install receipts are recorded in the dated
work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies and pure-Rust implementation backends (`brotli-decompressor = 5.0.3`
with `default-features = false`, `#![forbid(unsafe_code)]`).

## Clean Google News default-mode loading, scrolling, and link navigation

`mg-chassis` and `mg-sparkle` resolve default-mode (`--enable-scripts`) script
errors and link-activation blocking on `news.google.com` and extend styled
layout to `Full coverage` (`/stories/...`) destinations without widening any
worker, memory, or layout budget:

- Before preparing external scripts or spawning an isolated script worker,
  `resources::script_document_admissible` checks whether a page's raw HTML, URL,
  and encoded initial request fit within the existing 1 MiB source, 16 KiB URL,
  and 2 MiB JSON wire caps. Ordinary browsing documents above those caps (such
  as Google News's 1.86..4.40 MB `/home`, `/topics/...`, `/stories/...`, and
  `/showcase` SSR payloads) skip worker startup cleanly (`JavaScript skipped`),
  parse once with `<noscript>` preserved, emit zero `SCRIPT_*` or DevTools JS
  rejection errors, and leave `script_blocked = false` so scrolling and link
  clicks (`Top stories ›`, category tabs, `Full coverage`, Ctrl+click new tab,
  and Back/Forward) work normally.
- Admitted `< 1 MiB` scripted pages that actually start a script realm and fail
  or close still enforce `closed_script_realm_never_falls_through_to_native_activation`,
  and trusted `WindowHarness` runs retain full worker observation.
- Outer-box `Length::Calc` supports bounded `min(...)` and `max(...)`
  comparisons between a pixel bound and a linear `<length-percentage>` term
  (`CalcOp::{Linear, MinPx, MaxPx}`, keeping `size_of::<Length>() == 16`), and
  CSS grid placement preflight charges linear `O(cells * count + count^2)` work
  within `MAX_GRID_WORK = 1_000_000`, allowing 51-item 12-column `Full coverage`
  (`/stories/...`) pages and Material 3 `max(48px, 100%)` touch-target overlays
  to render in full centered styled layout without fallback.
- See [the Google News contract](https://github.com/pierce403/mgbrowser/blob/main/docs/GOOGLE_NEWS.md).

## Pure-Rust WOFF1 and WOFF2 `@font-face` web fonts

`mg-sparkle` and `mg-chassis` load and render same-origin `.woff2` and `.woff`
`@font-face` stylesheets (such as `M Plus 1M` / `mplus-1m` on `badge.nyx.ms` and
`sl.ush.fund`) alongside `.ttf` and `.otf`:

- `@font-face` `src:` scanning inspects all `url(...)` candidates in declaration
  order and admits `.woff2`, `.woff`, `.ttf`, and `.otf` URLs and `font/woff2` /
  `font/woff` MIME types within the unchanged same-origin resource ledger.
- `mg-sparkle::woff` reconstructs uncompressed OpenType/TrueType SFNT tables
  from `wOFF` (zlib via `flate2` `rust_backend`) and `wOF2` (RFC 7932 Brotli via
  `brotli-decompressor = 5.0.3`, `default-features = false`,
  `#![forbid(unsafe_code)]`, including W3C WOFF2 transformed `glyf`/`loca` and
  `hmtx` reconstruction) within bounded 8 MiB scratch and 4 MiB SFNT caps.
- Downloaded `@font-face` families without separate bold/italic faces synthesize
  horizontal stem emboldening (`font-weight >= 600`) and subpixel-interpolated
  oblique shear (`font-style: italic | oblique` and HTML `<i>`/`<em>` UA rules)
  without altering system-font glyph metrics or WPT baselines.

## Validation status

Workspace debug and release unit/integration tests (including `legacy-test-engine`),
component boundary and no-chrome embedding checks, Hacker News 1024/1280 PNG
byte-identity, and live default-flags `mgbrowser` Xvfb/CDP/X11 scrolling and
link navigation across Google News Homepage, `Top stories ›`, `Full coverage`,
and Back pass with zero script errors. All 8 frozen JSPLAN gates retain all 364
research outcomes. Both complete reviewed WPT lanes are refreshed for this
commit.

## Install and known limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer follows the latest published release and verifies SHA-256 before
installing without sudo. Checksums are not independent signatures. Existing
processes must restart to use an installed update.

Modern-web compatibility is poor. The Google search to first result journey is
not working. Real Reddit loading and full interactive Google News features
remain unfinished goals. Ordinary Playwright is unsupported: autoattachment,
debugger waiting/resume, downloads, utility worlds, promise awaiting and stock
locator message-size policy remain open.

Boa JavaScript is enabled by default (`--disable-scripts` opts out); documents
exceeding the 1 MiB source or 2 MiB wire admission cap skip script execution and
render statically. The DOM, classic external scripts and browser event loop
remain incomplete. Modules, dynamic scripts, fetch/XHR and general asynchronous
resources are not supported. Full CSS/CSSOM, intrinsic sizing, baselines, bidi
and floats remain incomplete. Fonts/images/tabs/resources are bounded subsets;
cookies are memory-only. This increment does not complete F-011/F-016/F-024/
F-025/F-026/F-027 or formal MVP.

Linux X11/XWayland is the supported GUI target. The restricted JavaScript worker
is not a sandbox for the browser as a whole. Do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Existing
limits are not whole-browser quotas or GC/RSS measurements. No worker authority
or resource profile is widened for this increment; `brotli-decompressor 5.0.3`
(`default-features = false`) is reviewed in `docs/DEPENDENCIES.md`.

Project code, documentation and original artwork use Apache-2.0. Dependencies
retain their licenses in packaged THIRD_PARTY_LICENSES.txt. Old tags/assets stay
immutable.
