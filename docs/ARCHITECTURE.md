# Mg components

Adopted 2026-09-09. The workspace separates the original implementations into
four packages, all versioned together. The desktop executable remains
`mgbrowser`. The libraries currently have experimental Rust APIs and are consumed
from this repository; they are not published on crates.io.

| Package | Responsibility | Production Mg dependencies |
| --- | --- | --- |
| `mg-butane` | Boa execution facade/policy and preserved original evaluator baseline | None |
| `mg-sparkle` | HTML parsing, DOM, browser bindings, page protocol, flow layout and software painting | `mg-butane` |
| `mg-chassis` | Navigation, HTTP/TLS, cookies, history, page-session coordination, CDP and optional browser controls | `mg-sparkle` |
| `mg-browser` | Executable composition, X11 window/input/surface, platform font discovery and Linux script-worker isolation | All three libraries |

Butane has no DOM, graphics, networking or C/C++ engine dependency. Sparkle
provides the web-facing host bindings around Butane. Its standalone rendering
API accepts a document, font data, viewport and control state, and returns pixels,
layout boxes and hit regions in page-relative coordinates. It does not create a
window, fetch resources or start a worker. Sparkle uses standalone Rust Stylo for
CSS computation and its own block/inline/table layout. v0.9.0 adds a bounded
low-level Rust Taffy adapter for flex/grid geometry, borrowing
the same DOM and font measurements. Mg still owns scene construction, clipping,
positioned stacking, painting and input geometry. It receives stylesheet source
and bounded image bytes through Document, not a network callback. PNG/JPEG/GIF
and restricted SVG decoding remain Rust-only. Chassis owns resource fetching,
navigation-generation cancellation and download budgets. Stylesheets remain
same-origin; image chains stop sending/storing cookies at their first origin
crossing, including later redirects back. They cannot downgrade after HTTPS.
Neither resource bytes nor new fetch capabilities
are passed into the script worker.

The new layout work is not a full CSS engine or hard render-time sandbox. Limits
include 2,000,000 shared work steps, 200,000 scene operations, depth 256 and bounded
pass-local caches. Taffy contexts admit at most 512 participants with charged
16 MiB live scratch and bounded implicit grid growth. Numeric positioned stacking
contexts are bounded to 512; transform/opacity/isolation contexts are not modeled.
Simple inline SVG serialization adds at most 128 sources/2 MiB per pass, with each
source still limited to 512 KiB before the existing resolver-disabled decoder.
Image decode/cache limits and the script process boundary are unchanged. See
[News reading acceptance](GOOGLE_NEWS.md) for development and release evidence.

Chassis composes the services needed to browse. Its `chrome` Cargo feature draws
the current single-row toolbar/address field and status area. A host can disable it
with `default-features = false`, or call `Browser::set_chrome(false)` at runtime.
The same navigation and form services remain usable without that UI. Services
can become internal modules or smaller crates later without adding more public
component brands now.

The native read-only Inspector is also optional Chassis chrome. Hosts forward
right-clicks to `context_menu_at` using logical coordinates and F12 to
`Key::DeveloperTools`. Selection is tied to the painted document epoch. Sparkle
returns bounded style/layout diagnostics in `Frame`; Chassis combines those with
resource and script failures. Inspection does not enable scripting or a debug
port and is unavailable when chrome is disabled. See [Inspector](INSPECTOR.md).

Chassis's chrome accepts `ThemePreference` and a host-supplied `ColorScheme`.
Theme selection does not recolor Sparkle's page pixels. The desktop host alone
owns XDG preference persistence, Rust D-Bus portal discovery and the X11 window
decoration hint. Embedders need no session bus or configuration directory.

Display sizing likewise belongs to the host: XSETTINGS/Xresources provide the
screen-global DPI and Chassis receives System/manual size preferences. Layout,
hit regions, scrolling and CSS/CDP input stay in logical pixels; Sparkle's
`render_scaled` and `Canvas::new_scaled` rasterize into physical pixels. Existing
`render`/`Canvas::new` default to 1x. The native host converts mouse coordinates
and resizes with `Browser::resize_surface`; the original logical `resize` API
keeps its previous bounds. Native device surfaces are bounded to 4800x3600
(internal rounding may add up to three pixels per edge), independently of
unchanged worker, page-image and encoded-screenshot budgets.

## Embedding contracts

- **Butane:** `modern::Engine` wraps the pinned Boa parser/VM/GC, guarded source/
  function/global-getter execution, job checkpoints and fatal policy. It does
  not itself provide OS isolation. `runtime::Runtime` / `runtime::Host` remain
  the original interpreter's regression API, not a page fallback.
- **Sparkle:** `document::parse`, `paint::Fonts::from_bytes` and `render::render`
  provide a synchronous, headless document pipeline. Optional regular/bold face
  data is accepted by `Fonts::from_bytes_with_bold`. `js_browser::boa::BoaPageRealm`
  integrates Boa and the bounded Rust DOM; embedders own execution isolation.
  The original `js_browser::PageRealm` remains the test baseline.
- **Chassis:** `Browser::new(fonts, scripts)` accepts caller-selected font data
  and an `Arc<dyn scripts::ScriptRuntime>`. The host polls navigation, supplies
  pointer/key input, and displays the `Canvas` returned by `paint`. `BrowserCdp`
  can expose the documented loopback protocol subset for that same browser.
- **Platform:** `mg-browser::platform` supplies font discovery and `LinuxScripts`.
  Each Browser needs its own script service. The service currently re-execs the
  host executable, so that executable must dispatch `--script-worker` and
  `--script-session` before initializing fonts, networking or a window.

The desktop composition lives in `src/desktop.rs`; `src/main.rs` dispatches the
command line and worker modes. `src/workspace.rs` is a display-independent owner
of live Chassis instances with stable tab/window/pane identities. Moving a tab
changes placement, not its Browser or worker. Native windows each show at most
two equal-width groups; preferences, bookmarks, network session and updater are
host-shared. The 16-tab/four-window cap is not an aggregate page-memory quota.
`BrowserCdp::tick_pages` receives every live tab, including hidden pages, under
stable target IDs. Neither switching nor detaching changes a target's identity.

Chassis fences navigation generations and validates returned DOM/default actions.
The platform service owns child processes, isolation, cumulative accounting,
cancellation and bounded reaping. `DisabledScripts` returns an explicit error
when a host has no isolated worker. The desktop still requires `--enable-scripts`;
unsupported platforms retain the existing refusal path. The extraction preserved
behavior; the later Boa adoption explicitly introduces the versioned profile in
[BOA.md](BOA.md) and protocol v2 with validated engine-specific accounting.
Original protocol/allocation assertions remain in the explicit legacy test lane.

The host owns render dimensions and the resulting pixel allocation. Chassis
emits update/restart requests only after explicit UI actions; the host authorizes
restart readiness and owns executable replacement/relaunch. Engines never launch
an updated application. Restart reopens committed tab URLs in one window with
fresh session state; it does not preserve pane placement, forms or history.

Chassis clamps its viewport to the existing desktop bounds. Direct Sparkle callers must
choose dimensions appropriate to their own resource limits. A page surface is
software RGB (`0x00RRGGBB`); GPU/compositor integration remains future work.

Wheel easing belongs to Chassis, not page engines: native hosts call
`wheel_scroll_by` for logical wheel deltas and `advance_scroll(Instant)` before
painting dirty frames. Existing `scroll_by` and CDP remain immediate. Motion
invalidates old hit geometry; hosts must present the updated paint before using
its coordinates. Clicks, keys, navigation and geometry changes cancel motion.

## Runnable examples and checks

Run from the repository root with Rust 1.91.1. Substitute a readable font path
where necessary. These examples use local source or an explicitly supplied URL.

```sh
mkdir -p tmp
cargo run --locked -p mg-butane --example eval -- '6 * 7;'
cargo run --locked -p mg-sparkle --example render -- \
  tests/fixtures/journey/home.html /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf tmp/sparkle.png
cargo run --locked -p mg-chassis --no-default-features --example headless -- \
  https://example.com/ /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf tmp/headless.png
cargo test --locked --workspace --features legacy-test-engine --all-targets
cargo test --locked -p mg-chassis --no-default-features --test embedding
python3 tools/check-components.py
```

The embedding test compares Chassis's entire toolbar-free surface with Sparkle's
output and submits a real form to an owned loopback server through the public
pointer/text/key API. Original language tests live under `crates/mg-butane/tests`,
DOM/page tests under `crates/mg-sparkle/tests`, and process tests remain at the
root with the actual executable. Shared authored fixtures remain in `tests/fixtures`.
CI checks production dependency direction and also builds Chassis independently
with default features disabled, avoiding workspace feature unification.

## Compatibility milestones

The long-term direction is Butane as a replacement for V8/JavaScriptCore and
Sparkle as a replacement for Blink/WebKit, including potential use by Tauri
embedders. **This release does not implement those drop-in APIs or ABIs.** Tauri's
existing WebView integrations cannot select Sparkle today. Modern Boa syntax does
not supply missing DOM/loading/task APIs or full CSS/layout compatibility.

[JSPLAN.md](../JSPLAN.md) develops the proposed Butane path: evaluate Boa and Nova
against the existing host/resource boundary, establish modern React/Vue tests,
then pursue measured optimization and a pinned V8 embedding adapter. The user-
authorized Boa page increment adopts only the bounded contract in BOA.md, not
the roadmap's comprehensive P1 resource gate or later framework/V8 milestones.

The first [P0/P1 research executable](../experiments/jsplan/README.md) now has its
own excluded Cargo workspace and lockfile. It depends on original Butane for an
explicit baseline and Boa for a comparison, not a production fallback. It shares
the unchanged `src/platform/script_isolation.rs` with the actual Linux workers;
engine initialization follows isolation. The v0.4.0 page path now also links Boa
through Butane's optional `modern` feature, enabled for the browser and Sparkle.
Sparkle directly names Boa types/macros for traced wrappers; execution remains
behind Butane's guarded facade. Worker syscall permissions remain unchanged.
Published release/installer acceptance is recorded separately in the dated log.

Further work needs separately scoped acceptance gates:

1. Expand and stabilize Rust embedding contracts: runtime lifecycle and handles,
   callbacks, errors, scheduling, navigation/resource services and surfaces.
2. Add a pinned Test262 runner for language behavior and pinned Web Platform Tests
   for web APIs and integrated rendering. Record unsupported tests honestly;
   existing authored regressions do not establish either suite's conformance.
3. Port host facilities to ThermiteOS: window/input, threads, clocks, randomness,
   sockets, font data and isolated processes. Current components use Rust `std`;
   the Linux build still uses its OS substrate and glibc. Direct `libc` declarations
   remain in the Linux host for process/syscall controls. There are no C-backed
   JavaScript, HTML, font, image or TLS engines in the selected dependency graph.
4. Choose a specific embedder and version, then implement and test a narrow
   compatibility adapter. V8/JSC APIs and Blink/WebKit/WebView integrations are
   distinct contracts. Any required C/C++ ABI bridge belongs at that boundary;
   broad source/binary compatibility needs dedicated tests and maintenance.

These milestones are a roadmap, not authorization for unrelated compatibility
work during this release. See [dependency policy](DEPENDENCIES.md),
[JavaScript limits](JAVASCRIPT.md) and [retained page sessions](PAGE_SESSIONS.md).
