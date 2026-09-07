# MVP and architecture plan

## Product boundary

The first product is a Linux desktop browser for reading and navigating static documents. Success means a user can enter an HTTP(S) URL, read styled text and images, scroll, follow links, and navigate back/forward without a crash on the versioned MVP corpus. It is an experimental browser, not initially suitable for banking, accounts, or arbitrary hostile sites.

The target is 100% Rust browser code, with our own HTML parser, DOM, CSS parser/cascade, layout, paint orchestration, navigation, and eventual JS engine. No Chromium, WebKit, Servo embedding, system webview, or existing JavaScript runtime. The user approved Rust utility crates, experimental `rustls-rustcrypto` for TLS, and Rust-only font/image processing. No C/C++ font, image or cryptographic implementations, including transitive or statically linked bindings. Unsupported images are preferable to native fallbacks; an additional decoder can be written here in Rust when needed. See `DEPENDENCIES.md` for the actual Cargo configuration and audit boundary. OS/window/driver interfaces remain a separate future decision, not a blanket native-library exception.

Linux first is a proposed sequencing choice, with portable core crates and macOS/Windows later. No schedule promises until the first rendering slice establishes throughput.

## Implementation sequence

| Stage | Deliverable | Exit gate |
| --- | --- | --- |
| M0: foundation | Public repo/site, feature contracts, corpus and Rust workspace | Reproducible checkout checks; published site; versioned initial fixtures |
| M1: vertical slice | Local HTML bytes → DOM → block layout → pixels | Desktop window and deterministic headless output render a heading, paragraph, and link from the same engine |
| M2: useful static web | HTTP(S), URL resolution, CSS, images, text, navigation | All required MVP fixtures pass; manual navigation scenarios pass on Linux |
| M3: MVP release | Resource bounds, error recovery, packaging, contributor harness | Clean-machine install/run; 100% required corpus; documented limitations; independently reproduced contribution |
| M4: expanding web | Forms, richer layout, accessibility, scripting foundations | Each feature earns its own test gate; no claim of general compatibility |

First engineering task: define 20 small, licensed local fixtures and implement the smallest bytes-to-pixels slice. Build a headless path early so every rendering change is testable without a desktop session. Introduce measurement at M1; build the general experiment executor after the first deterministic benchmark exists.

## Proposed Rust workspace

`mg-dom` owns node identity and the document tree; `mg-html` constructs it. `mg-css` parses declarations and produces computed styles. `mg-layout` creates boxes and line fragments. `mg-paint` emits a display list and rasterizes it. `mg-net` handles URLs, fetch policy, redirects, and response limits. `mg-browser` owns window/input/navigation and page lifecycle. `mg-harness` runs fixtures and records experiment evidence. Introduce crates when boundaries are real, not empty scaffolding for every future component.

Rendering flow: response bytes → tokenizer/tree builder → DOM → computed styles → layout tree → display list → surface. Input events resolve against layout and update navigation or page state. Network completion must not mutate layout from an unrelated thread.

## Static MVP scope

- HTML document/body, headings, paragraphs, links, div/span, lists, images, basic inline emphasis; specified recovery for malformed input.
- CSS tag/class/id selectors, source order/specificity/inheritance, display block/inline/none, box model, colors, font size, simple normal-flow text wrapping.
- HTTP(S) GET, relative URLs, redirects with a cap, validated TLS, explicit loading/error states. Fixed request/body/time budgets and cancellation when navigating away.
- UTF-8 text with a pinned test font, PNG images initially, scrolling/resizing, clickable links, address field, reload, back/forward, keyboard focus for browser controls.
- Deterministic headless raster output and fixture reports; a documented subset of upstream Web Platform Tests where appropriate, with pinned revision and license attribution.

Defer JavaScript, flex/grid, video/audio, WebGL, extensions, sync, password storage, service workers, downloads, persistent cookies, and authenticated browsing. Broader font/script support and document accessibility need explicit follow-on milestones; the MVP must state those limits.

## Release gate

Version and freeze the required fixture manifest before scoring changes. Every required case must pass with zero crashes/hangs; optional unsupported cases remain visible in reports. Use exact DOM/layout comparisons and bounded pixel tolerances with pinned fonts/environment. Add local-server integration tests for redirects, TLS failures, cancellation, and navigation. Record startup time, render time, and peak memory on named hardware; set performance budgets after a measured baseline. A release also needs keyboard/manual desktop checks, a clean-install exercise, and a reproducible binary with dependency/license inventory.

Untrusted pages require process isolation and brokered resource access before claiming a hardened general-purpose browser. Memory safety alone does not provide that boundary. Parser fuzzing and bounded-input tests support the MVP; production sandboxing is a separate release gate.

## Decisions to resolve before relevant implementation

Confirm Linux-first and the OS/window interface boundary. The TLS/font/image direction is adopted; audit dependency upgrades and any new HTTP/window/raster crates against `DEPENDENCIES.md`. Choose a project license before accepting outside code; public visibility alone does not grant reuse rights. Decide process architecture before enabling active web content. JavaScript needs a separate language/VM/conformance roadmap, not an incidental library swap.
