# mgbrowser v0.9.31 Experimental Preview

W3C Beacon (`navigator.sendBeacon`), WebIDL host method-override and expando
assignment semantics across `navigator`/`console`/`history`/DOM wrappers,
Navigation Timing (`performance.timing` & `performance.navigation`), ECMA-262
Annex B string/date compatibility (`String.prototype.substr`,
`trimLeft`/`trimRight`, `Date.prototype.toGMTString`), same-origin
`iframe.contentWindow` intrinsic constructors, and `<noscript>` fallback
projection when initial page scripts hit cooperative `EngineLimit` with zero
rendered items — resolving all JavaScript console errors and blank-screen traps
on Google Search (`https://www.google.com/` -> `/search?...` ->
`/httpservice/retry/enablejs` with `0` diagnostics). Exact-source publication
and public-install receipts are recorded in the dated work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies, Apache-2.0 license, and pure-Rust implementation backends (zero
new crate dependencies).

## Beacon API, WebIDL Method Overrides & Navigation Timing (`v0.9.31`)

- **W3C `navigator.sendBeacon(url, data?)` (`crates/mg-sparkle/src/js_browser/boa/web_apis.rs`)**:
  - Brand-checks `Navigator` receivers (`TypeError` on unbound/alien `this`),
    resolves `url` against the document URL requiring `http:` or `https:`
    schemes (`TypeError` on invalid URLs or `javascript:`/`data:`/`about:`
    schemes), validates optional `USVString`, `URLSearchParams`, and
    `BufferSource` (`ArrayBuffer`, `TypedArray`, `DataView`) payloads, enforces
    the 64 KiB (`65,536`-byte) keepalive quota (`false` when exceeded, `true`
    within bounds), and exposes standard read-only `Navigator` properties
    (`cookieEnabled`, `onLine`, `webdriver`, `pdfViewerEnabled`, `vendor`,
    `languages`, `maxTouchPoints`, `hardwareConcurrency`).
- **WebIDL Host Method Overrides & Expando Properties (`crates/mg-sparkle/src/js_browser/boa.rs`)**:
  - Distinguishes read-only IDL data attributes (such as `navigator.userAgent`,
    `navigator.webdriver`, `document.nodeType`, and `event.type`, which remain
    non-writable in sloppy and strict mode) from regular WebIDL operations and
    expandos across `navigator`, `console`, `history`, and DOM/event wrappers
    (preserving `[LegacyUnforgeable]` `location` restrictions and `defineProperty`
    wrapper invariants).
- **Navigation Timing, Annex B & `iframe.contentWindow` Intrinsics (`crates/mg-sparkle/src/js_browser/boa/`)**:
  - Adds `performance.timing` (`navigationStart`, `fetchStart`, `responseStart`,
    `domInteractive`, `domComplete`, `loadEventEnd`, etc.) and
    `performance.navigation` (`type`, `redirectCount`, `TYPE_*` constants).
  - Registers ECMA-262 Annex B `String.prototype.substr` (UTF-16 code-unit
    slicing with negative start and `Infinity`/`NaN` handling),
    `String.prototype.trimLeft` / `trimRight`, and
    `Date.prototype.toGMTString`.
  - Populates same-origin `iframe.contentWindow` with standard realm
    constructors (`Object`, `Array`, `String`, `Number`, `RegExp`, `Promise`,
    `Uint8Array`, `JSON`, etc.) and platform globals (`navigator`,
    `performance`, `screen`).

## Cooperative `EngineLimit` Empty-Projection `<noscript>` Fallback (`v0.9.31`)

- **Blank-Screen Prevention on Challenge Pages (`crates/mg-chassis/src/lib.rs`)**:
  - When an initial page script realm hits cooperative `RealmState::Fatal`
    (`OPCODE_LIMIT = 1_000_000`) before revealing any body items
    (`doc.items.is_empty()`), `mg-chassis` checks whether the unscripted
    document provides `<noscript>` fallback items or a `<meta http-equiv="refresh">`
    target (`refresh_count < 4`) and adopts that fallback projection while
    preserving `InitialScriptOutcome` diagnostics, worker accounting, and
    `ScriptWorkStatus::Blocked`.
  - Note: Google's server-side SearchGuard (`SG_SS` / BotGuard) still redirects
    non-Chrome clients away from live SERP results; the full Google search
    destination goal remains incomplete and deferred as documented in
    `AGENTS.md`.

## Preserved Resource & Isolation Profile

- All `boa-page-process-v1` limits remain unchanged (`OPCODE_LIMIT = 1_000_000`,
  `32 MiB` outstanding / `64 MiB` cumulative worker allocations, `RLIMIT_NOFILE = 3`,
  `RLIMIT_FSIZE = 0`, `RLIMIT_CPU = 1`, seccomp confinement, and `< 8 MiB`
  compressed release archive ceiling).
- Static WPT reftest coverage (`558 PASS / 391 FAIL / 62 UNSUPPORTED` of
  `1,011`) and Window WPT coverage (`16/16` files, `79/79` final subtests)
  remain intact with zero regressions.
