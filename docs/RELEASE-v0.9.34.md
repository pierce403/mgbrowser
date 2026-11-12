# mgbrowser v0.9.34 Experimental Preview

Signed-out desktop visual and JavaScript UI parity across modern large-bundle
websites (`google.com`, `youtube.com`, `wikipedia.org`, `yahoo.com`, `bing.com`,
`github.com`, `duckduckgo.com`, and `news.google.com`, plus Shreddit and Amazon
storefront fixtures where live automation encounters anti-bot challenges),
pure-Rust WebP image decoding (`image-webp = 0.2.4`), CSS `mask-image` /
`-webkit-mask` icon tinting, credential-free CORS `fetch()` / `XMLHttpRequest`
and cross-origin HTTPS ES `<script type="module">` loading, `DocumentType` and
`Intl` globals, and expanded `boa-interactive-page-v1` desktop resource/JS
budgets while preserving `boa-page-process-v1` and both upstream WPT score lanes
unchanged.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies, Apache-2.0 license, and pure-Rust implementation backends (zero
C/C++ bindings).

## Changes

- **Pure-Rust WebP Decoding & Interactive Oversize Downscaling (`crates/mg-sparkle/src/images.rs`)**:
  - Enabled the pure-Rust `webp` feature on `image = 0.25.10` (`image-webp = 0.2.4`,
    `byteorder-lite = 0.1.0`, `quick-error = 2.0.1`; zero `libwebp` or C/C++ FFI
    dependencies).
  - Added `RIFF....WEBP` magic-header sniffing across generic
    `application/binary` and `application/octet-stream` CDN responses (decoding
    live Google News story thumbnails and favicons) and proportional interactive
    downscaling into `<= 2048` px per side / `<= 1,048,576` pixels while keeping
    non-interactive rejection unchanged.
- **CSS `mask-image` / `-webkit-mask`, Dropdowns & Form Controls (`crates/mg-sparkle/`)**:
  - Added CSS `mask-image` / `-webkit-mask` (`url(...)`, position, size, repeat)
    alpha-channel tinting for monochrome SVG/raster UI icons (Wikipedia Vector
    2022, GitHub Primer, Bing), nested `<li>` dropdown subtree containment,
    `<input placeholder>` text painting, and author-styled `<button>` backgrounds
    and borders.
  - Raised interactive `<link rel="stylesheet">` discovery across `<head>` and
    `<body>` to `64` stylesheets with an `8 MiB` Stylo CSS input budget (`16`
    sheets and `2 MiB` unchanged in non-interactive mode) and strict HTML/XML
    attribute-key validation matching arena projection.
- **CORS `fetch`/XHR, Cross-Origin HTTPS ES Modules & Subresource Policy (`crates/mg-chassis/`, `crates/mg-sparkle/`)**:
  - Added `Policy::for_mode(true)` (`64` sheets, `192` requests, `4 MiB`
    per-image/font, `8 MiB` per-CSS, `16 MiB` decoded script response body,
    `48 MiB` aggregate, `20 s` deadline) and Subresource Integrity
    (`sha256`/`sha384`/`sha512`) verification.
  - Enabled credential-free cross-origin HTTPS `<script type="module">`
    resolution under `InteractivePageV1`, unified static `import` specifier
    discovery with regex-literal-aware `esm::extract_module_dependencies` (fixing
    minified regex `/\{/` masking on GitHub's `primer-react` bundle), and added
    credential-free CORS `Access-Control-Allow-Origin` `fetch()` and
    `XMLHttpRequest` enforcement in the parent network broker.
- **Interactive Boa Worker Profile, WebIDL Globals & CDP Stability (`crates/mg-butane/`, `crates/mg-sparkle/`, `crates/mg-chassis/`, `src/platform/`)**:
  - Scaled `boa-interactive-page-v1` (`16 MiB` per script / `32 MiB` cumulative
    source, `384` recursion / `262,144` VM stack, `16 MiB` `ArrayBuffer`,
    `48 MiB` DOM logical admission in both script execution and
    `BrowserHost::validate_input`, `384 MiB` outstanding / `4 GiB` cumulative
    worker allocation, `2,560 MiB` `RLIMIT_AS`, `30 CPU s`, `30 s` active wall,
    `256` transactions / `128 MiB` session wire) with identical seccomp,
    `RLIMIT_NOFILE = 3`, and `RLIMIT_FSIZE = 0` confinement; `boa-page-process-v1`
    remains 100% unchanged.
  - Installed `window.DocumentType` and ECMA-402 `window.Intl` (`DateTimeFormat`,
    `NumberFormat`, `Collator`, `PluralRules`, `RelativeTimeFormat`,
    `ListFormat`, `DisplayNames`, `Segmenter`, `Locale`), synthetic
    `keydown`/`keypress`/`keyup` + single-text-input `<form>` `Enter` submission,
    and a `4,096`-slot (`<= 16 MiB`) outbound CDP WebSocket event queue.

## Evidence and remaining scope

- `tests/real_site_compat.rs` (`27` integration tests) and live Chrome vs.
  `mgbrowser` Playwright audits verify rendering and UI interaction across
  `google.com`, `youtube.com`, `wikipedia.org`, `yahoo.com`, `bing.com`,
  `github.com`, `duckduckgo.com`, and `news.google.com`. Live `reddit.com`
  (reCAPTCHA) and `amazon.com` (AWS WAF proof-of-work) serve anti-bot challenges
  to automated clients in both Chrome and `mgbrowser` and are verified via local
  Shreddit and Amazon storefront fixtures without solving or bypassing anti-bot
  gates.
- `<video>`/audio media streaming, account sign-in, full ECMA-402 locale tables,
  and F-016 / F-025 / F-026 / F-027 / formal MVP completion remain open.
- Pinned WPT static reftests (`558 PASS / 391 FAIL / 62 UNSUPPORTED` of `1,011`)
  and Window declared-value tests (`16/16` files, `79/79` final subtests) remain
  intact. Exact-source CI, release publication, and public installer/updater
  receipts are recorded in `memory/logs/2026-11-11.md`.
