# Feature specification

Exact stability values: `planned`, `in-progress`, `stable`. Checked criteria require recorded evidence. This file drives the public status section.

## F-001 — Project foundation and website

Stability: stable

### Properties

Public source, readable MVP plan, and an HTTPS project site at mgbrowser.org. Every main push validates and deploys the site; public readiness follows this file and the latest work-log date.

### Test Criteria

- [x] GitHub repository exists and matches the published commit.
- [x] Pages deploys the intended commit; live HTML matches local bytes.
- [x] Custom-domain TLS is valid and HTTPS enforcement is enabled.
- [x] Generated website status passes the Rust tool's `--check`.

Evidence: 2026-09-07 initial Pages run 34120255103 succeeded for aed2386; live HTTPS HTML matched byte-for-byte, certificate approved and HTTPS enforcement confirmed by API. See daily log for subsequent publication checks. Visual browser QA is not yet available.

## F-002 — Agent continuity and reusable skills

Stability: stable

### Properties

Canonical AGENTS.md, measurable feature contracts, bounded task queue, daily logs, compact memory/skills indexes, and reusable skill procedures. Automatic skill selection follows the catalog and does not authorize unrelated actions.

### Test Criteria

- [x] Instructions, indexes, referenced skills and first daily log exist.
- [x] Skill frontmatter validates; harness instruction aliases resolve to AGENTS.md.

Evidence: 2026-09-07 both skills passed quick_validate.py; instruction alias targets checked; website sync passed locally and in CI. Portable catalog-based selection is configured; individual agent-client autodiscovery has not been tested.

## F-003 — Local document rendering

Stability: in-progress

### Dependencies

MVP scope and Rust dependency boundary; initial versioned fixtures.

### Properties

Our Rust HTML/DOM/style/layout/paint pipeline draws a local heading, paragraph, and link in both a desktop window and headless output.

### Test Criteria

- [ ] DOM and layout snapshots match required vertical-slice fixtures.
- [ ] Repeated headless renders match under a pinned environment.
- [ ] Linux desktop renders the same document and responds to resize.

Evidence: 2026-09-07 native X11 window displayed the local journey and Google homepage using own HTML flow and Rust text paint. Screenshot frames were inspected. Broader DOM/layout snapshots and resize acceptance remain open.

## F-004 — Static web navigation

Stability: in-progress

### Dependencies

F-003.

### Properties

HTTP(S), relative links, address bar, reload, back/forward, cancellation, scroll, and visible load/errors; invalid TLS never silently succeeds.

TLS uses rustls-rustcrypto explicitly under the research-only policy in docs/DEPENDENCIES.md. Native crypto fallback is prohibited.

### Test Criteria

- [ ] Local-server tests cover redirects, failures, request/body/time limits and cancellation.
- [ ] Manual Linux navigation and keyboard-control scenarios pass.
- [x] Local TLS handshake fixtures cover trusted/untrusted certificates and hostname mismatch with the selected provider.

Evidence: 15 transport tests passed for framing, verified TLS/rejection cases, redirects, request/body/time limits and in-memory cookie scope. A real local HTTP form submission/result click completed in the native window. Async navigation ignores stale results and caps requests at two, but lacks a transport cancellation API. Full manual keyboard/history acceptance remains open.

## F-005 — Styled text and images

Stability: in-progress

### Dependencies

F-003.

### Properties

The documented static MVP HTML/CSS subset supports cascade/inheritance, block and inline flow, wrapped UTF-8 text, box styling and PNG images.

Font parsing, shaping and rasterization use Rust implementations without native font bindings. Image codecs are explicitly enabled, initially PNG only. Unsupported/broken images retain alt text and a placeholder without preventing document rendering; no C decoder fallback.

### Test Criteria

- [ ] All required versioned DOM/layout/pixel fixtures pass.
- [ ] Unsupported syntax and malformed input produce bounded, documented behavior.
- [ ] Unsupported/corrupt image fixtures display a placeholder and preserve surrounding document layout and alt text.
- [ ] Resolved font/image features contain no native implementations or implicit codec fallback.

Evidence: native frames use rustybuzz/fontdue and show image placeholders/alt text. No page-image downloading or full CSS cascade yet. Current dependency guard passes; this does not satisfy the full fixture corpus.

## F-006 — Reproducible autoresearch evaluator

Stability: planned

### Dependencies

F-003 and deterministic baseline corpus.

### Properties

A Rust evaluator executes bounded experiments with immutable evaluation inputs, independent correctness/performance gates, and machine-readable evidence. Humans and agents use the same contribution contract.

### Test Criteria

- [ ] Another contributor reproduces a baseline and candidate report from a clean checkout.
- [ ] Known wrong output, timeout, crash and evaluator-tampering candidates fail gates.
- [ ] Reports contain the provenance and metrics specified in docs/AUTORESEARCH.md.

## F-007 — Linux MVP release

Stability: planned

### Dependencies

F-004, F-005, F-006.

### Properties

Installable experimental static browser with documented limitations and no claims of modern JavaScript or hardened general-purpose browsing.

### Test Criteria

- [ ] Clean-machine install, full required corpus, manual browser scenarios and resource limits pass.
- [ ] Dependency/native-code and license inventory is reviewed.
- [ ] Release artifacts, provenance and known limitations are published.

## F-008 — JavaScript and broader compatibility

Stability: planned

### Dependencies

Own language/runtime, DOM integration and execution-boundary roadmap; now required for F-010 before a complete static MVP release.

### Properties

Future own Rust JavaScript engine and expanded platform support; no embedded existing browser/runtime shortcut.

Live Google search currently requires this work. Develop against bounded local language and DOM fixtures; retain the original Google journey as the end-to-end acceptance gate.

### Test Criteria

- [ ] Language/runtime subset, conformance suite and active-content isolation gates are specified before implementation.

## F-009 — Experimental Rust dependency foundation

Stability: stable

### Properties

Cargo configuration pins rustls-rustcrypto and explicitly selects Rust font/image implementations. image defaults are disabled with PNG as the only codec. The library constructs TLS configuration with an explicit provider and caller-supplied roots. A CI denylist guards known native backends; dependency source review remains required. This is not yet an HTTPS or document-rendering implementation.

### Test Criteria

- [x] Locked dependencies compile and TLS client construction succeeds with the explicit provider.
- [x] PNG round-trip succeeds, a disabled codec is rejected, and Rust font APIs reject invalid font input.
- [x] Active Linux normal/build dependency graph passes the native-backend regression guard.
- [x] GitHub CI reproduces the locked build and dependency checks.

Evidence: local cargo test --locked passed all three initial smoke tests on 2026-09-07; GitHub Rust run 34121634459 reproduced formatting, dependency guard and smoke tests for 645d30b. docs/DEPENDENCIES.md records the build/dependency review. Stable refers to this dependency configuration contract, not production TLS readiness. Later handshake and font-rendering evidence is recorded under F-003/F-004 and the daily log.

## F-010 — Google search to first destination

Stability: in-progress

### Dependencies

F-003, F-004, and sufficient F-008 JavaScript/DOM support for the actual served pages.

### Properties

Open our native Rust browser, navigate to google.com, enter and submit a search, display Google's actual results, click the first result and attempt to load its destination. Preserve real TLS validation and the Rust-only implementation policy. A local fixture, different search engine, fabricated result, or troubleshooting link cannot substitute for Google's result.

### Test Criteria

- [x] Native browser window opens and renders the actual Google homepage.
- [x] The real q field accepts input and submits the served form controls through our HTTP/TLS stack.
- [x] The browser carries ordinary in-memory cookies and follows bounded standard HTML refreshes.
- [ ] The actual Google search results render as actionable links.
- [ ] Clicking the first result navigates to its actual destination, with an observed response or clear load failure.

Evidence: 2026-09-07 native Google journey returned HTTP 200 homepage, submitted “Rust programming language”, and followed the served no-JavaScript refresh to a page titled “Enable JavaScript to use search”. The journey correctly exited 2 because there were no result links. Separately, the explicitly labeled localhost fixture journey completed all stages and exited 0. The user goal is not achieved.

## F-011 — Chrome DevTools Protocol automation

Stability: in-progress

### Dependencies

F-003 and F-004 initially; broader browser capabilities for eventual full protocol coverage.

### Properties

Opt-in, loopback-only CDP controls the actual native browser. The first subset covers discovery, one page/target, flattened sessions, navigation, actual DOM inspection/selectors, editable controls, mouse/keyboard input, and PNG viewport screenshots. Unsupported commands and parameters fail explicitly. docs/CDP.md and docs/cdp-protocol.json describe the implemented contract, limitations and roadmap toward full protocol support. No production, full Chrome, DevTools frontend, Playwright or Puppeteer compatibility claim.

### Test Criteria

- [x] Discovery and real WebSocket tests cover endpoint identity, Host/Origin restrictions, bounded messages, connection limits and cleanup.
- [x] DOM commands inspect retained source nodes; stale node/session handles and unsupported selectors/methods are rejected.
- [x] Session enablement and navigation lifecycle events distinguish load failures from successful delivery.
- [x] Page coordinates and PNG dimensions exclude native toolbar/status pixels, including after scrolling.
- [x] An external Rust CDP client completes the actual local fixture form → result → destination journey, captures PNG and verifies flattened sessions.
- [x] GitHub CI independently reproduces the CDP fixture journey for the published revision.
- [ ] A pinned upstream schema inventory and client-version compatibility matrix cover the entire protocol as underlying capabilities become available.

Evidence: 2026-09-07 cargo test --locked --all-targets passed 47 tests (35 library, 9 binary, 3 external-client regressions). External CDP journey passed under Xvfb with exact Unicode query and hidden form field, first-anchor mouse click, HTTP 200 destination and 1100×683 PNG; rendered destination inspected. The Rust command client also inspected and captured the real Google homepage through CDP. Runtime.evaluate explicitly returns -32601 because no JavaScript engine exists. Initial subset is working; full protocol remains in-progress.

Remote evidence: implementation commit fba063a passed Rust CI 34129329323 with all 47 tests, both native/CDP journeys and the dependency guard. Pages run 34129329247 deployed matching HTTPS HTML; HTTP redirects to HTTPS. Full protocol inventory/compatibility is the remaining feature gate.
