# MVP and architecture plan

## Product boundary

The immediate user acceptance goal is to open our browser, navigate to google.com, submit a search, and click the first actual result link, with an attempt to load the destination. This requires evidence from the real rendered controls and live responses. A local search fixture proves the interaction mechanics only; Google homepage delivery alone does not complete this goal. Destination rendering can remain incomplete, as requested.

The broader first product is a Linux desktop browser for reading and navigating documents: enter an HTTP(S) URL, read styled text and images, scroll, follow links, and navigate back/forward without a crash on the versioned MVP corpus. It remains a testing and research browser, not initially suitable for banking, accounts, or arbitrary hostile sites.

The target is 100% Rust browser code, with our own HTML parser, DOM, CSS parser/cascade, layout, paint orchestration, navigation, and JavaScript engine. These components are not all complete; an original partial JS interpreter is now implemented. No Chromium, WebKit, Servo embedding, system webview, or existing JavaScript runtime. The user approved Rust utility crates, experimental `rustls-rustcrypto` for TLS, and Rust-only font/image processing. No C/C++ font, image or cryptographic implementations, including transitive or statically linked bindings. Unsupported images are preferable to native fallbacks; an additional decoder can be written here in Rust when needed. See `DEPENDENCIES.md` for the actual Cargo configuration and audit boundary. The initial Linux window uses x11rb's Rust X11 protocol implementation through X11/XWayland, without Xlib/XCB FFI; operating-system/window-server interfaces are recorded separately from browser implementation.

Linux is the first implemented window target, with portable core code and macOS/Windows later. No schedule promises until the first rendering slice establishes throughput.

## Current implementation and acceptance gap

The transport, own HTML parser/retained DOM, software painter and native window are integrated. Networking includes verified HTTPS, HTTP/1.1 framing, GET/POST forms, redirects, response limits, and in-memory session cookies. Painting uses fontdue/rustybuzz, explicit font files, clipped software drawing, and PNG snapshots. Native handlers and the experimental public CDP subset complete the authored local form → result → destination journey. These are separately verified from public websites; see `RUNNING.md` and the dated log.

An original Rust lexer/parser/evaluator and DOM bridge now run a limited non-strict, ES5-like subset behind `--enable-scripts`. Scripting is off by default. Linux x86_64 uses a fresh restricted process for each document, with bounded source/DOM/pipes, evaluator budgets, kernel controls and a parent deadline. Inline scripts share that document's temporary realm, can create real controls and change the title/content, and have limited startup callbacks and proposed HTTP(S) navigation. On 2026-09-07 the native script redirect → script-created form → Unicode submission → local destination path passed, as did the independent CDP journey and readable recovery after a script fuel error. Frames were inspected. This is partial implementation, not language conformance or a persistent page event loop; see `JAVASCRIPT.md`.

The real Google goal remains incomplete. Script-disabled browsing loaded and submitted its actual form but reached a JavaScript-required response. With the new opt-in interpreter, the homepage and submitted search again returned HTTP 200; scripts reported unsupported behavior and the page titled “Google Search” had no rendered result items or forms. The journey correctly exited 2. No Google result was fabricated or substituted. External scripts, many language/builtin/DOM behaviors, real events/timers and persistent realms remain missing; fixing the first reported parser error alone does not establish compatibility. Full CSS and page-image fetching are also absent.

Navigation caps outstanding workers at two and discards stale completions. HTTP requests have finite byte, redirect, and time limits, but superseded sockets keep running until completion or timeout. The caller's DNS wait is bounded; the OS resolver thread can outlive it. True cancellation, renderer/network process isolation, and clean-machine desktop testing remain work ahead. The restricted script worker does not sandbox the whole browser.

## Implementation sequence

| Stage | Deliverable | Exit gate |
| --- | --- | --- |
| M0: foundation | Public repo/site, feature contracts, corpus and Rust workspace | Reproducible checkout checks; published site; versioned initial fixtures |
| M1: vertical slice | Local HTML bytes → DOM → block layout → pixels | Desktop window and deterministic headless output render a heading, paragraph, and link from the same engine |
| M2: useful web navigation | HTTP(S), URL resolution, forms, session cookies, CSS, images, text, navigation and sufficient original scripting | Required fixtures and manual Linux scenarios pass; separately verify the requested live Google search and first-result interaction |
| M3: MVP release | Resource bounds, error recovery, packaging, contributor harness | Clean-machine install/run; 100% required corpus; documented limitations; independently reproduced contribution |
| M4: expanding web | Richer layout, accessibility, broader scripting/platform support | Each feature earns its own test gate; no claim of general compatibility |

Extend the integrated bytes-to-pixels and navigation slice while freezing small, licensed local fixtures, including normal forms/links, script-built controls and unsupported-image behavior. Preserve headless rendering evidence alongside desktop checks. Introduce measurement at M1; build the general experiment executor after the first deterministic benchmark exists. The planned 20-fixture corpus and independent autoresearch evaluator are not yet completed.

## Proposed Rust workspace

The current package contains document, transport, paint, CDP and JS modules plus the mgbrowser executable and its script worker; the following names describe future responsibility boundaries rather than existing crates. `mg-dom` owns node identity and the document tree; `mg-html` constructs it. `mg-css` parses declarations and produces computed styles. `mg-layout` creates boxes and line fragments. `mg-paint` emits a display list and rasterizes it. `mg-net` handles URLs, fetch policy, redirects, and response limits. `mg-js` owns language/runtime semantics; an explicit host bridge exposes bounded browser capabilities. `mg-browser` owns window/input/navigation and page lifecycle. `mg-harness` runs fixtures and records experiment evidence. Introduce crates when boundaries are real, not empty scaffolding for every future component.

Rendering flow: response bytes → tokenizer/tree builder → DOM → computed styles → layout tree → display list → surface. Input events resolve against layout and update navigation or page state. Network completion must not mutate layout from an unrelated thread.

## Static MVP scope

- HTML document/body, headings, paragraphs, links, div/span, lists, images, basic inline emphasis; specified recovery for malformed input.
- CSS tag/class/id selectors, source order/specificity/inheritance, display block/inline/none, box model, colors, font size, simple normal-flow text wrapping.
- HTTP(S) GET/POST forms, relative URLs, redirects with a cap, validated TLS, memory-only cookies, and explicit loading/error states. Fixed request/body/time budgets and cancellation when navigating away; true cancellation is still pending.
- UTF-8 text with a pinned test font, PNG images initially, scrolling/resizing, clickable links, address field, reload, back/forward, keyboard focus for browser controls.
- Deterministic headless raster output and fixture reports; a documented subset of upstream Web Platform Tests where appropriate, with pinned revision and license attribution.

The static subset remains a useful acceptance layer, not a replacement for the immediate live-search goal. The implemented partial interpreter follows `JAVASCRIPT.md`; expand general language/DOM correctness, external script loading and lifecycle support with independent fixtures before claiming broader compatibility. Defer flex/grid, video/audio, WebGL, extensions, sync, password storage, service workers, downloads, persistent cookies, and authenticated browsing. Broader font/script support and document accessibility need explicit follow-on milestones; the MVP must state those limits.

## Release gate

Version and freeze the required fixture manifest before scoring changes. Every required case must pass with zero crashes/hangs; optional unsupported cases remain visible in reports. Use exact DOM/layout comparisons and bounded pixel tolerances with pinned fonts/environment. Add local-server integration tests for redirects, TLS failures, cancellation, and navigation. Record startup time, render time, and peak memory on named hardware; set performance budgets after a measured baseline. A release also needs keyboard/manual desktop checks, a clean-install exercise, and a reproducible binary with dependency/license inventory.

Untrusted pages require process isolation and brokered resource access before claiming a hardened general-purpose browser. The initial Linux x86_64 script boundary is implemented and tested with owned child processes, but does not isolate the renderer/network or establish production hardening. Memory safety alone is insufficient. Parser fuzzing, a pinned conformance corpus and bounded-input tests support the MVP; a whole-browser security review remains a separate release gate.

## Decisions to resolve before relevant implementation

The initial Linux X11/XWayland interface and Linux x86_64 script worker are implemented; additional OS/window/isolation backends require review and must fail closed when unavailable. The TLS/font/image direction is adopted; audit dependency upgrades and any new HTTP/window/raster crates against `DEPENDENCIES.md`. Choose a project license before accepting outside code; public visibility alone does not grant reuse rights. Design persistent realm ownership and parent-brokered external script fetching before adding them. JavaScript needs continuing language/runtime/conformance gates, not an incidental library swap.
