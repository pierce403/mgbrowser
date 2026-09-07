# MVP and architecture plan

## Product boundary

The immediate user acceptance goal is to open our browser, navigate to google.com, submit a search, and click the first actual result link, with an attempt to load the destination. This requires evidence from the real rendered controls and live responses. A local search fixture proves the interaction mechanics only; Google homepage delivery alone does not complete this goal. Destination rendering can remain incomplete, as requested.

The broader first product is a Linux desktop browser for reading and navigating documents: enter an HTTP(S) URL, read styled text and images, scroll, follow links, and navigate back/forward without a crash on the versioned MVP corpus. It remains a testing and research browser, not initially suitable for banking, accounts, or arbitrary hostile sites.

The target is 100% Rust browser code, with our own HTML parser, DOM, CSS parser/cascade, layout, paint orchestration, navigation, and eventual JS engine. No Chromium, WebKit, Servo embedding, system webview, or existing JavaScript runtime. The user approved Rust utility crates, experimental `rustls-rustcrypto` for TLS, and Rust-only font/image processing. No C/C++ font, image or cryptographic implementations, including transitive or statically linked bindings. Unsupported images are preferable to native fallbacks; an additional decoder can be written here in Rust when needed. See `DEPENDENCIES.md` for the actual Cargo configuration and audit boundary. The initial Linux window uses x11rb's Rust X11 protocol implementation through X11/XWayland, without Xlib/XCB FFI; operating-system/window-server interfaces are recorded separately from browser implementation.

Linux is the first implemented window target, with portable core code and macOS/Windows later. No schedule promises until the first rendering slice establishes throughput.

## Current implementation and acceptance gap

The transport, own HTML parser, software painter and native window are integrated. Networking includes verified HTTPS, HTTP/1.1 framing, GET/POST forms, redirects, response limits, and in-memory session cookies. Painting uses fontdue/rustybuzz, explicit font files, clipped software drawing, and PNG snapshots. The local native-window journey passed form typing/submission, actual displayed result-link activation, and destination loading through application input handlers. Thirty tests pass across components and UI state. Live Google verification loaded the homepage, submitted its actual controls, and followed its no-JavaScript HTML refresh to “Enable JavaScript to use search”; no result links were returned. See `RUNNING.md` and the daily log. JavaScript execution is now on the critical path to the full requested goal.

The initial ordinary Google probe loaded the homepage and its HTML form, then received a JavaScript retry document with a no-script refresh rather than search-result links. The complete desktop/session/HTML-navigation path still needs live verification. There is no JavaScript engine, full CSS implementation, or page-image fetching yet. If the live search requires active scripting after supported HTML navigation is exercised, implementing the required own Rust engine becomes part of reaching the user's goal; the goal must remain open until actual results and the first-link interaction are verified.

Navigation currently caps outstanding workers at two and discards stale completions. HTTP requests have finite byte, redirect, and time limits, but superseded sockets keep running until they finish or time out. The caller's DNS wait is bounded; the OS resolver thread can outlive that wait. True cancellation, process isolation, and clean-machine desktop testing remain work ahead.

## Implementation sequence

| Stage | Deliverable | Exit gate |
| --- | --- | --- |
| M0: foundation | Public repo/site, feature contracts, corpus and Rust workspace | Reproducible checkout checks; published site; versioned initial fixtures |
| M1: vertical slice | Local HTML bytes → DOM → block layout → pixels | Desktop window and deterministic headless output render a heading, paragraph, and link from the same engine |
| M2: useful web navigation | HTTP(S), URL resolution, forms, session cookies, CSS, images, text, navigation | Required fixtures and manual Linux scenarios pass; separately verify the requested live Google search and first-result interaction |
| M3: MVP release | Resource bounds, error recovery, packaging, contributor harness | Clean-machine install/run; 100% required corpus; documented limitations; independently reproduced contribution |
| M4: expanding web | Richer layout, accessibility, broader scripting/platform support | Each feature earns its own test gate; no claim of general compatibility |

Finish the integrated bytes-to-pixels and navigation slice while freezing small, licensed local fixtures, including normal forms/links and unsupported-image behavior. Preserve headless rendering evidence alongside desktop checks. Introduce measurement at M1; build the general experiment executor after the first deterministic benchmark exists. The planned 20-fixture corpus and independent autoresearch evaluator are not yet completed.

## Proposed Rust workspace

The current package has transport and paint modules plus the mgbrowser executable; the following names describe future responsibility boundaries rather than existing crates. `mg-dom` owns node identity and the document tree; `mg-html` constructs it. `mg-css` parses declarations and produces computed styles. `mg-layout` creates boxes and line fragments. `mg-paint` emits a display list and rasterizes it. `mg-net` handles URLs, fetch policy, redirects, and response limits. `mg-browser` owns window/input/navigation and page lifecycle. `mg-harness` runs fixtures and records experiment evidence. Introduce crates when boundaries are real, not empty scaffolding for every future component.

Rendering flow: response bytes → tokenizer/tree builder → DOM → computed styles → layout tree → display list → surface. Input events resolve against layout and update navigation or page state. Network completion must not mutate layout from an unrelated thread.

## Static MVP scope

- HTML document/body, headings, paragraphs, links, div/span, lists, images, basic inline emphasis; specified recovery for malformed input.
- CSS tag/class/id selectors, source order/specificity/inheritance, display block/inline/none, box model, colors, font size, simple normal-flow text wrapping.
- HTTP(S) GET/POST forms, relative URLs, redirects with a cap, validated TLS, memory-only cookies, and explicit loading/error states. Fixed request/body/time budgets and cancellation when navigating away; true cancellation is still pending.
- UTF-8 text with a pinned test font, PNG images initially, scrolling/resizing, clickable links, address field, reload, back/forward, keyboard focus for browser controls.
- Deterministic headless raster output and fixture reports; a documented subset of upstream Web Platform Tests where appropriate, with pinned revision and license attribution.

JavaScript is currently unimplemented and needs a scoped own-engine roadmap if required for the immediate live-search goal. Defer flex/grid, video/audio, WebGL, extensions, sync, password storage, service workers, downloads, persistent cookies, and authenticated browsing. Broader font/script support and document accessibility need explicit follow-on milestones; the MVP must state those limits.

## Release gate

Version and freeze the required fixture manifest before scoring changes. Every required case must pass with zero crashes/hangs; optional unsupported cases remain visible in reports. Use exact DOM/layout comparisons and bounded pixel tolerances with pinned fonts/environment. Add local-server integration tests for redirects, TLS failures, cancellation, and navigation. Record startup time, render time, and peak memory on named hardware; set performance budgets after a measured baseline. A release also needs keyboard/manual desktop checks, a clean-install exercise, and a reproducible binary with dependency/license inventory.

Untrusted pages require process isolation and brokered resource access before claiming a hardened general-purpose browser. Memory safety alone does not provide that boundary. Parser fuzzing and bounded-input tests support the MVP; production sandboxing is a separate release gate.

## Decisions to resolve before relevant implementation

The initial Linux X11/XWayland interface is implemented; additional OS/window backends require review. The TLS/font/image direction is adopted; audit dependency upgrades and any new HTTP/window/raster crates against `DEPENDENCIES.md`. Choose a project license before accepting outside code; public visibility alone does not grant reuse rights. Decide process architecture before enabling active web content. JavaScript needs a separate language/VM/conformance roadmap, not an incidental library swap.
