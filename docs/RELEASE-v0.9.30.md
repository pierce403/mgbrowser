# mgbrowser v0.9.30 Experimental Preview

Responsive CSS Grid (`[named-lines]`, `fit-content()`, `repeat(auto-fill |
auto-fit, ...)`), generalized intrinsic box sizing (`min-content`, `max-content`,
`fit-content`), CSS `box-shadow` and `text-shadow` Gaussian blur rendering,
multi-stop (`2..=16`) linear/repeating-linear/radial/repeating-radial gradients,
CSS 2D `transform` (`translate`, `scale`, `matrix`), `clip: rect(...)` /
`clip-path: inset(... round ...)` and `text-overflow: ellipsis`, pure-Rust HTML5
`<canvas>` 2D immediate-mode rasterization and PNG export, browser-hosted ES
`<script type="module">` execution, DOM Observers (`MutationObserver`,
`ResizeObserver`, `IntersectionObserver`) and `CustomElementRegistry`
(`window.customElements`), `@font-face` `unicode-range` subset filtering,
large-page (`> 1 MiB`) CDP utility-world automation (`news.google.com`), honest
Linux desktop `User-Agent` compatibility (`www.npr.org`), independent per-tab
page zoom (`25%..=500%`), persistent user-local `localStorage`, and tab session
snapshot & restore. Exact-source publication and public-install receipts are
recorded in the dated work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies, Apache-2.0 license, and pure-Rust implementation backends (zero
new crate dependencies).

## CSS Visual Parity, Responsive Grid & Intrinsic Sizing (`v0.9.30`)

- **Named Grid Lines, `fit-content()` & `repeat(auto-fill | auto-fit)` (`crates/mg-sparkle/`)**:
  - Parses explicit `[line-name]` line lists on `grid-template-columns` and
    `grid-template-rows`, synthesizes implicit `<area>-start` / `<area>-end`
    lines from `grid-template-areas`, resolves `grid-column` / `grid-row` named
    lines and `span <name>` placements, maps `fit-content(<length-percentage>)`
    tracks, and expands `repeat(auto-fill | auto-fit, ...)` track definitions
    against container dimensions while collapsing empty `auto-fit` tracks to
    `0px` (rendering MDN Web Docs `--layout-no-sidebar-extended` in full styled
    layout without readable-flow fallback).
- **Generalized Intrinsic Sizing & HTML5 `<canvas>` Replaced Layout (`crates/mg-sparkle/`)**:
  - Supports `min-content`, `max-content`, and `fit-content` keywords across
    `width`, `height`, `min-width`, `min-height`, `max-width`, `max-height`, and
    `flex-basis` across block, inline, flex, and grid formatting contexts, plus
    HTML5 `<canvas>` `300x150` intrinsic fallback and HTML `width`/`height`
    attribute sizing.
- **Shadows, Multi-Stop Gradients, 2D Transforms, Clipping & Ellipsis (`crates/mg-sparkle/`)**:
  - `box-shadow` (`0..=8` reverse-painted layers) with analytical rounded-rect
    signed-distance Gaussian blur, outer border-box cutout masking, and `inset`
    padding-box clipping; `text-shadow` (`0..=4` layers) with blurred glyph
    rasterization beneath text runs.
  - Multi-stop (`2..=16`) `linear-gradient`, `repeating-linear-gradient`,
    `radial-gradient`, and `repeating-radial-gradient` backgrounds with CSS
    Images 3 stop fixup and color-transition midpoints.
  - CSS 2D `transform` (`translate`, `translateX`, `translateY`, `translate3d`,
    `scale`, `scaleX`, `scaleY`, `matrix`) with `transform-origin`, stacking
    context creation, and `position: fixed` containing-block establishment.
  - Screen-reader `clip: rect(...)` and `clip-path: inset(... round ...)`
    zero-area visual hiding and rounded inset clipping, plus single-line
    `text-overflow: ellipsis` (`U+2026`) truncation.
- **Measured WPT Coverage**:
  - Static flexbox reftest lane advances from `541` to **558 PASS / 391 FAIL /
    62 UNSUPPORTED of 1,011** (`+17` genuine new passes over `v0.9.29`, zero
    losses; `expectations.json` manually ratcheted to protect all `558` passes
    and `949` runnable cases).
  - Separate real-Browser Window lane retains **16 / 16 files and 79 / 79 final
    subtests (100%)**.

## HTML5 `<canvas>` 2D, ES Modules, DOM Observers & Web Components (`v0.9.30`)

- **Pure-Rust HTML5 `<canvas>` 2D Engine (`crates/mg-sparkle/src/js_browser/boa/canvas.rs`, `crates/mg-chassis/`)**:
  - Implements `HTMLCanvasElement.getContext('2d')` (`<= 2048x2048`, `<= 1,048,576`
    pixels, worker-memory-accounted premultiplied RGBA buffer): `fillRect`,
    `strokeRect`, `clearRect`, subpaths (`beginPath`, `moveTo`, `lineTo`,
    `quadraticCurveTo`, `bezierCurveTo`, `arc`, `arcTo`, `ellipse`, `rect`,
    `roundRect`), `Path2D`, nonzero/evenodd scanline `fill` & dashed/joined
    `stroke`, `clip`, affine `DOMMatrix` state stack, `CanvasGradient` &
    `CanvasPattern`, shadow blur, Porter-Duff `globalCompositeOperation`,
    zero-FD embedded bitmap `fillText` / `strokeText` / `measureText`,
    `getImageData` / `putImageData` / `drawImage`, pure-Rust LZ77+Huffman PNG
    encoding (`toDataURL` / `toBlob`), and live `document.resources` projection
    into Sparkle's styled layout painter.
- **Browser-Hosted ES `<script type="module">`, Observers & Custom Elements (`crates/mg-sparkle/src/js_browser/boa/`, `crates/mg-chassis/`)**:
  - Preloads bounded inline and external `<script type="module">` graphs (`<= 16`
    modules, `<script type="importmap">` specifier resolution, live namespace
    bindings, `import.meta.url`, and parent-brokered same-origin dynamic
    `import()`, with `nomodule` suppression in browser sessions while keeping
    standalone `BoaPageRealm::start` / `boa_worker` assertions untouched).
  - Implements microtask-batched `MutationObserver` (`childList`, `attributes`,
    `characterData`, `subtree`), layout-synced `ResizeObserver` and
    `IntersectionObserver`, and `CustomElementRegistry` (`customElements.define`,
    `get`, `whenDefined`, `upgrade`, `connectedCallback`, `disconnectedCallback`,
    `attributeChangedCallback`), plus `Element.animate` / `getAnimations` and
    `window.visualViewport`.

## Real-Site Compatibility, Large-Page CDP & Desktop Session UX (`v0.9.30`)

- **`@font-face` `unicode-range` Filtering, Wikipedia Sprite/SVG/JS Compatibility & Large-Page CDP (`crates/mg-sparkle/`, `crates/mg-chassis/`)**:
  - Filters `@font-face` rules against document text codepoints before fetching
    font binaries so multi-subset stylesheets (such as MDN's Cyrillic/Greek/Latin
    font sheets) download matching subsets instead of exhausting `MAX_FONTS` on
    unused scripts.
  - Updates `USER_AGENT` to `"Mozilla/5.0 (X11; Linux x86_64) mgbrowser/0.9.30"`,
    resolving CDN crawler-signature TLS tar-pitting on sites such as `www.npr.org`.
  - Adds `window.Image` (`new Image(width?, height?)`), raises the browser-session
    `addEventListener` cap to `2,048` memory-accounted listeners (while keeping
    the standalone `BoaPageRealm::start` contract cap at `32`), supports bounded
    external/inline SVG `<style>` blocks and acyclic internal `<use href="#id">`
    sprite references, resolves intrinsic `background-size: auto` and
    `background-position` offsets with element-box clipping for CSS sprites,
    multiplies `color` alpha (`color: transparent`) during text painting,
    suppresses spurious gradient diagnostics for MediaWiki's transparent
    `linear-gradient(transparent, transparent), url(...)` fallback idiom, and
    replaces per-reply full-HTML string materialization with zero-allocation
    serialized-byte validation (`www.wikipedia.org` executes all page scripts
    and renders its SVG wordmark sprite with `0` JS/CSS/SVG errors or warnings).
  - Synthesizes a bounded (`<= 128 KiB`, `<= 2,500` nodes) `<script>`-free DOM
    snapshot when opening an on-demand CDP automation session on oversize (`> 1
    MiB`) HTML documents or after page-script budget exhaustion so Playwright
    `page.title()` and utility-world queries succeed well within the 2-second
    active wall budget on pages like `en.wikipedia.org`, `news.google.com`, and
    `developer.mozilla.org`.
- **Per-Tab Page Zoom, Persistent `localStorage` & Session Restore (`crates/mg-chassis/`, `src/`)**:
  - Independent per-tab page content zoom (`25%..=500%`, `Alt+=` / `Alt+-` /
    `Alt+0`, `Ctrl+MouseWheel`, address-bar zoom badge, and Hamburger Menu zoom
    row) separate from whole-browser UI scale.
  - Atomic `0o600` (`O_NOFOLLOW`, `flock`, `<= 512 KiB`) user-local persistence
    for `window.localStorage` (`local-storage.json`, cleared on `Ctrl+Shift+Delete`)
    and open-tab session snapshots (`session.json`, restorable via Menu ->
    `Restore session`, `chrome://newtab`, or `--restore-session`).

## Remaining Limitations

- Modern-web compatibility remains bounded: cross-origin dynamic `import()`,
  WebGL/WebGPU, HTML5 `<video>`/`<audio>` codecs, Service Workers, cross-origin
  CORS preflights, CSS 3D transforms/keyframe animations, and full Chromium
  multi-process site isolation are not implemented.
- Resource budgets (`16` stylesheets, `32` requests, `2 MiB` total resources,
  `1 MiB` page script source, `2048px` / `1,048,576`-pixel image/canvas cap) and
  the seccomp-confined script worker (`RLIMIT_NOFILE = 3`, `RLIMIT_FSIZE = 0`,
  not a whole-browser sandbox) remain unchanged. Do not use this preview for
  banking, sensitive authenticated browsing, or arbitrary hostile websites.
