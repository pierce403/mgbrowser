# Mg components

Adopted 2026-09-09. The workspace separates the original implementations into
four packages, all versioned together at 0.2.0. The desktop executable remains
`mgbrowser`. The libraries currently have experimental Rust APIs and are consumed
from this repository; they are not published on crates.io.

| Package | Responsibility | Production Mg dependencies |
| --- | --- | --- |
| `mg-butane` | JavaScript syntax, values, evaluator, builtins and resource accounting | None |
| `mg-sparkle` | HTML parsing, DOM, browser bindings, page protocol, flow layout and software painting | `mg-butane` |
| `mg-chassis` | Navigation, HTTP/TLS, cookies, history, page-session coordination, CDP and optional browser controls | `mg-sparkle` |
| `mg-browser` | Executable composition, X11 window/input/surface, platform font discovery and Linux script-worker isolation | All three libraries |

Butane has no DOM, graphics, networking or native-engine dependency. Sparkle
provides the web-facing host bindings around Butane. Its standalone rendering
API accepts a document, font data, viewport and control state, and returns pixels,
layout boxes and hit regions in page-relative coordinates. It does not create a
window, fetch resources or start a worker.

Chassis composes the services needed to browse. Its `chrome` Cargo feature draws
the current toolbar, address/title strip and status area. A host can disable it
with `default-features = false`, or call `Browser::set_chrome(false)` at runtime.
The same navigation and form services remain usable without that UI. Services
can become internal modules or smaller crates later without adding more public
component brands now.

## Embedding contracts

- **Butane:** `runtime::Runtime` and `runtime::Host` evaluate the existing bounded
  language subset. Host methods explicitly supply external capabilities.
- **Sparkle:** `document::parse`, `paint::Fonts::from_bytes` and `render::render`
  provide a synchronous, headless document pipeline. `js_browser::PageRealm`
  integrates the existing interpreter and DOM; embedders own execution isolation.
- **Chassis:** `Browser::new(fonts, scripts)` accepts caller-selected font data
  and an `Arc<dyn scripts::ScriptRuntime>`. The host polls navigation, supplies
  pointer/key input, and displays the `Canvas` returned by `paint`. `BrowserCdp`
  can expose the documented loopback protocol subset for that same browser.
- **Platform:** `mg-browser::platform` supplies font discovery and `LinuxScripts`.
  Each Browser needs its own script service. The service currently re-execs the
  host executable, so that executable must dispatch `--script-worker` and
  `--script-session` before initializing fonts, networking or a window.

Chassis fences navigation generations and validates returned DOM/default actions.
The platform service owns child processes, isolation, cumulative accounting,
cancellation and bounded reaping. `DisabledScripts` returns an explicit error
when a host has no isolated worker. The desktop still requires `--enable-scripts`;
unsupported platforms retain the existing refusal path. Resource limits, protocol
validation and script language semantics are unchanged by this extraction.

The host owns render dimensions and the resulting pixel allocation. Chassis
clamps its viewport to the existing desktop bounds. Direct Sparkle callers must
choose dimensions appropriate to their own resource limits. A page surface is
software RGB (`0x00RRGGBB`); GPU/compositor integration remains future work.

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
cargo test --locked --workspace --all-targets
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
existing WebView integrations cannot select Sparkle today. The original limited
language, HTML-flow model and browser compatibility remain as documented.

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
