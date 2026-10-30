# mgbrowser v0.9.24 Experimental Preview

Full 6-panel Chromium-style DevTools workspace (`Elements`, `Console`, `Sources`,
`Network`, `Performance`, `Application`) with Right/Bottom docking, draggable
splitter resize, interactive Element Picker (`↖` / `Ctrl+Shift+C`) with 4-layer
Stylo box-model overlays, live `element.style` DOM/CSS editing, isolated Boa
worker JS Console REPL + Command Line API, and 4-tab Network request inspection.
Exact-source publication and public-install receipts are recorded in the dated
work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies and pure-Rust implementation backends.

## Six-Panel Chromium-Style DevTools Workspace

`mg-chassis`, `mg-sparkle` and `mg-browser` expand the built-in DevTools panel
(`F12` / `Ctrl+Shift+I` / right-click `Inspect`) across six panels with
Right/Bottom docking (`⬓` / `◨`), a draggable 4 px splitter, and compact-overlay
fallback on narrow viewports:

- **Interactive Element Picker (`↖` / `Ctrl+Shift+C`) & 4-Layer Box Model
  Overlay**: hovering the page highlights any DOM element with concentric Stylo
  Margin (orange), Border (yellow), Padding (green), and Content (blue) layers
  plus a floating `tag#id.class  W×H` tooltip; clicking selects the element as
  `$0` in `Elements` without activating page links or forms.
- **`Elements` (`Styles` | `Computed` | `Box Model`)**: collapsible `▸`/`▾` DOM
  tree with `== $0` badge, CSS selector & text search (`Ctrl+F`), `H` visibility
  toggle, `Delete` node removal, live editable `element.style { ... }` inline
  declarations with instant Stylo restyle/reflow and color swatches, real Stylo
  `ComputedStyle` properties, and a concentric 4-layer Box Model diagram.
- **`Console` (`Ctrl+Shift+J`)**: 19-method WHATWG/Chromium `console.*` API
  (`log`, `info`, `warn`, `error`, `debug`, `dir`, `dirxml`, `table`, `trace`,
  `assert`, `clear`, `count`, `countReset`, `group`, `groupCollapsed`,
  `groupEnd`, `time`, `timeLog`, `timeEnd`) with `%s/%d/%i/%f/%o/%O/%j/%c/%%`
  formatting inside the isolated Boa worker without invoking user JS getters or
  `Proxy` traps; interactive `>` JS REPL with Command Line API (`$_`, `$0..$4`,
  `$()`, `$$()`, `keys()`, `values()`, `dir()`, `dirxml()`, `table()`, `clear()`,
  `copy()`, `inspect()`), on-demand isolated worker startup on static/oversize
  pages, inline `▶`/`▼` object & DOM property tree expansion, `[Inspect]`
  jump-to-Elements badges, `Tab` ghost autocomplete, 64-entry `Up`/`Down`
  history, `Shift+Enter` multiline input, `-term` negative text filtering,
  severity pills, and `Preserve log`.
- **`Sources`**: navigator for `[HTML]` document source, `[CSS]` inline and
  same-origin stylesheets, and `[JS]` inline and external scripts with
  line-numbered syntax-colored viewing and `Ctrl+F` line search.
- **`Network` (`Ctrl+Shift+E`)**: `All`/`Doc`/`CSS`/`JS`/`Font`/`Img` filters,
  URL substring filter, `Preserve log`, `Disable cache`, Waterfall timeline with
  `DOMContentLoaded` (blue) and `Load` (red) vertical markers, and a 4-tab
  Request Inspector drawer (`Headers` | `Response` | `Preview` | `Timing`) that
  redacts sensitive `cookie`, `set-cookie`, and `authorization` header values.
- **`Performance` (`Ctrl+Shift+P`)**: `● Reload & Profile`, Load Phase Flame Bar,
  64-frame render-duration sparkline with `16.7 ms` (60 FPS) budget line, Boa
  worker opcode/allocation metrics, and frame/style/shaping cache hit telemetry.
- **`Application`**: live session `Cookies` table (with redacted byte lengths and
  `Clear Cookies`), `Security` origin/TLS/worker-sandbox summary, and `Resources`
  inventory.

## Validation status

Workspace debug and release unit/integration tests (including `legacy-test-engine`
and protocol v8 worker session selftests), component boundary and no-chrome
embedding checks, packaged native X11 `inspector-smoke` across light/dark and
100%/200% scale, all 13 native/CDP smoke helpers, all 8 frozen JSPLAN gates, and
both complete reviewed WPT lanes pass.

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
locator message-size policy remain open. JavaScript breakpoint stepping, heap
snapshots, flamechart CPU sampling, and persistent Web Storage (`localStorage` /
`IndexedDB`) remain outside this bounded DevTools increment.

Boa JavaScript is enabled by default (`--disable-scripts` opts out); documents
exceeding the 1 MiB source or 2 MiB wire admission cap skip page-load script
execution and spawn a bounded on-demand worker only when interactive Console
evaluation is explicitly requested. Full CSS/CSSOM, intrinsic sizing, baselines,
bidi and floats remain incomplete. This increment does not complete F-011/F-016/
F-024/F-025/F-026/F-027 or formal MVP.

Linux X11/XWayland is the supported GUI target. The restricted JavaScript worker
is not a sandbox for the browser as a whole. Do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Project code,
documentation and original artwork use Apache-2.0. Dependencies retain their
licenses in packaged THIRD_PARTY_LICENSES.txt. Old tags/assets stay immutable.
