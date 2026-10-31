# mgbrowser v0.9.29 Experimental Preview

Pinned unmodified Playwright `1.58.2` browser automation parity (`connectOverCDP`
over HTTP and WebSocket plus `chromium.launch` over `--remote-debugging-pipe`),
pure-Rust HTTP `gzip`/`deflate`/`br` decompression, responsive `<picture>` /
`srcset` and `data:image/*` decoding, credential-free cross-origin HTTPS
stylesheets and bounded CSS `@import`, CSS `grid-template-areas` / `grid-area`
2-column desktop layout, inline `position: relative` containing-block fallback,
flex-item BFC float isolation, `viewBox`-only and large-`viewBox` inline `<svg>`
sizing with Stylo `style="..."` and `xmlns:xlink` support, legacy vendor-prefixed
`-moz-border-radius` aliasing and Chromium-aligned DevTools Console `css-parse`
filtering, and real-site Boa DOM/JS compatibility (ECMAScript Annex B HTML-like
comments, DOM wrapper expandos, reflected HTML attributes, `document.currentScript`,
`document.write`, and blank/srcdoc `<iframe>` `contentDocument`/`contentWindow`).
Exact-source publication and public-install receipts are recorded in the dated
work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies, Apache-2.0 license, and pure-Rust implementation backends (zero
new crate dependencies).

## Pinned Playwright 1.58.2 Automation & CDP Parity (`v0.9.29`)

- **CDP Target Auto-Attachment, Isolated Utility Worlds & Pipe Transport (`src/cdp.rs`, `src/main.rs`, `crates/mg-sparkle/src/js_browser/`)**:
  - Full `Target.setAutoAttach` (`flatten: true`, `waitForDebuggerOnStart`),
    `Runtime.runIfWaitingForDebugger`, `Page.createIsolatedWorld` / utility-world
    `Runtime.evaluate` & `Runtime.callFunctionOn` (`awaitPromise`, `returnByValue`,
    element argument & return marshaling, `executionContextCreated`/`Destroyed`/
    `Cleared` even when page scripts are disabled with `--disable-scripts`),
    `Page` lifecycle events (`frameAttached`, `frameNavigated`, `domContentEventFired`,
    `loadEventFired`, `lifecycleEvent`, `javascriptDialogOpening`), `DOM` / `CSS`
    inspection, `Network` request/response events (including stylesheet HTTP 404
    reporting), `Input` mouse/keyboard/insertText, `Emulation.setDeviceMetricsOverride`,
    `Browser.setDownloadBehavior` (`DownloadProgress` / `downloadWillBegin`), and
    Chromium FD `3`/`4` NUL-delimited `--remote-debugging-pipe` + `--headless`
    virtual framebuffer mode.
  - Passes the unchanged `tools/playwright/acceptance.mjs` probe (`playwright-core`
    `1.58.2`, both HTTP and WebSocket `chromium.connectOverCDP` paths) and
    `tools/playwright/launch.test.mjs` (`chromium.launch` with locators,
    screenshots, and downloads) while rejecting unknown CDP methods explicitly.

## Real-Site CSS, SVG, HTTP & DOM Compatibility (`v0.9.29`)

- **HTTP Decompression, Cross-Origin HTTPS Stylesheets & Bounded `@import` (`crates/mg-chassis/`)**:
  - Pure-Rust `Accept-Encoding: gzip, deflate, br` response decoding (`flate2` +
    `brotli-decompressor`) with strict decoded-byte caps preventing zip/brotli
    amplification; credential-free cross-origin HTTPS `<link rel="stylesheet">`
    and `@import` loading (depth `<= 4`, cycle-checked, line/column-preserving)
    within the unchanged `16`-sheet, `32`-request, `2 MiB` resource budget.
  - Responsive `<img srcset>` / `<picture><source>` candidate selection and
    bounded `data:image/{png,jpeg,gif,svg+xml}` inline URI decoding.
- **Slashdot, Lobsters, Wikipedia & Rust-Lang Visual Layout Parity (`crates/mg-sparkle/`)**:
  - Byte-length-preserving author CSS normalization for legacy `-moz-border-radius`
    (plus corner variants, `-moz-box-sizing`, `-webkit-box-sizing`, `-moz-opacity`,
    `-moz-box-shadow`) preserving 1-based line/column diagnostic locations, paired
    with Chromium-aligned DevTools Console filtering of declarative CSS Syntax §5
    `css-parse` ignored declarations.
  - Stylo `style="..."` computed cascade on `<svg>` and SVG descendants, `xmlns:xlink`
    / `xmlns:svg` / `xml:space` / `xml:lang` attributes, inert `<defs>` / `<symbol>`
    / `<metadata>`, proportional `svg_target_size` scaling for large-coordinate
    `viewBox` icons within `MAX_SIDE = 2048` / `MAX_PIXELS = 1,048,576`, and
    containing-width sizing for `viewBox`-only inline `<svg>` elements lacking
    `width`/`height` attributes.
  - CSS Grid `grid-template-areas` explicit track sizing and `grid-area` named
    placement resolution (rendering Wikipedia's Vector 2022 2-column desktop
    layout at `>= 1120px`), CSS 2.1 §10.1 inline `position: relative` containing-block
    fragment union fallback, and isolated float state across `Taffy` flex/grid item
    measurement and BFC painting (fixing `clear: both` story lists inside flex
    containers on `lobste.rs`).
- **Real-Site Boa JS & DOM Compatibility (`crates/mg-sparkle/src/js_browser/boa/`)**:
  - Byte-length-preserving ECMAScript Annex B.1.1 `<!--` / `-->` comment
    normalization, DOM wrapper JS expando property storage with worker memory
    accounting and strict/sloppy read-only IDL semantics, reflected HTML element
    properties, `<script>`/`<style>` text serialization, `document.currentScript`,
    bounded `document.write`/`writeln`/`open`/`close`, and `subdoc:`-backed
    same-origin blank/srcdoc `<iframe>` `contentDocument` and `contentWindow`.
- **Measured WPT Coverage**:
  - Static flexbox reftest lane advances from `532` to **541 PASS / 408 FAIL /
    62 UNSUPPORTED of 1,011** (`+9` new passes over `v0.9.28` across SVG flex
    items, `data:image/*` flex-basis content sizing, and dynamic inline-size
    float/BFC measurement, zero losses; `expectations.json` ratcheted to protect
    all `541` passes).
  - Separate real-Browser Window lane retains **16 / 16 files and 79 / 79 final
    subtests (100%)**.
