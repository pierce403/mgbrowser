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

Stability: in-progress

### Dependencies

Own language/runtime, DOM integration and execution-boundary roadmap; now required for F-010 before a complete static MVP release.

### Properties

Original Rust lexer/parser/tree-walking interpreter with an explicit classic-script subset, bounded DOM capabilities and opt-in --enable-scripts. Inline scripts share one realm during document preparation; their real mutations produce native controls and CDP-visible nodes. A restricted Linux x86_64 child denies new file/network/process access and has independent logical, CPU, memory and wall limits. Unsupported platforms fail closed. No embedded existing browser/runtime shortcut or ECMAScript conformance claim.

Live Google search currently requires this work. Develop against bounded local language and DOM fixtures; retain the original Google journey as the end-to-end acceptance gate.

### Test Criteria

- [x] Language/runtime subset, authored conformance cases and active-content isolation gates are specified before implementation in docs/JAVASCRIPT.md.
- [x] Authored language and DOM tests cover evaluation order, UTF-16, closures, exceptions, mutation validity and cumulative limits.
- [x] Independent labeled-control-flow, URI and dynamic-compilation cases pass; dynamically compiled forms work through actual workers and native/CDP journeys.
- [x] Original regex literals, matcher and RegExp/String operations pass independent semantics/limit tests and actual-worker/native/CDP form journeys.
- [x] Bounded for-in enumeration and switch control flow pass independent language, actual-worker and native/CDP created-form journeys.
- [x] Explicit-state expression parsing preserves grammar/resource limits and passes independent default-stack, actual-worker and native/CDP grouped-form journeys.
- [x] Fixed-size allocation diagnostics, shared immutable function code and audited prepaid strings pass independent semantics/limit and actual-worker/native/CDP large-factory journeys.
- [x] Actual worker probes verify denied capabilities, resource termination, bounded pipe transfer and owned-child cleanup.
- [x] Native/CDP local journeys use a script-created form; script navigation and loop-error recovery are verified in a real window.
- [x] Source/projection rejection preserves original fallback and discards proposed navigation; stale completions cannot replace the active page.
- [x] GitHub CI reproduces the language/DOM/worker tests and scripted native/CDP fixture journeys for the published implementation.
- [ ] Parent-brokered external scripts, persistent realms, UI event dispatch and timers are implemented with independent fixtures.
- [ ] A pinned, licensed upstream conformance corpus and compatibility matrix complement the authored tests.
- [ ] Sufficient language and web-platform behavior passes the actual Google journey.

Evidence: 2026-09-07 original-runtime and DOM tests, real Linux worker-denial/limit probes, native /script-redirect → script-created form → local result → destination, and external CDP input/navigation/screenshot checks passed. An endless local loop exhausted fuel, retained readable content and allowed onward CDP navigation. Live Google still returns no result links with scripts enabled; partial language/browser support remains substantial work. This containment applies to the script worker, not the entire browser.

Language follow-up: labeled statements, all four URI builtins and bounded direct/indirect eval plus Function construction pass authored semantics tests. The /script-dynamic fixture creates every form control via compiled source and completes the same real native/CDP journey. Repeated valid/invalid compilation cannot reset resource budgets; cross-script var redeclaration preserves existing eval binding attributes. No dependency, worker-permission or CDP Runtime expansion was introduced.

Regex follow-up: original UTF-16 compiler/matcher, grammar-directed literals, RegExp state and String match/search/replace/split pass 18 integration groups plus parser/matcher/runtime regressions. The /script-regexp form completes both native and external CDP journeys; actual workers verify real controls and invalid-literal rejection before prefix effects. Local suite: 182 tests. Limits, ES5-shaped semantics and deliberate differences are documented in docs/JAVASCRIPT.md. No new crates or worker capabilities. Remote verification for this increment is recorded separately in the daily log.

Regex remote acceptance: 5daf77b passed Rust CI 34140478124, including the full suite and all three native/CDP journey steps. Pages 34140478195 deployed matching HTTPS content; the apex certificate is approved and HTTPS enforced. This verifies the published increment, not general JavaScript or Google compatibility.

Iteration follow-up: for-in supports own/inherited enumerable properties with duplicate suppression, nonenumerable shadowing and a bounded initial-key snapshot; switch uses strict matching, ordered selectors, fallthrough and scoped control flow. Boxed-string and builtin metadata now share own-property/enumeration rules. Independent tests cover mutation, hoisting, completion values, early errors and fatal cumulative limits. The /script-iteration fixture creates every form control using these statements, submits the real Unicode query/hidden field and reaches the local destination through both native handlers and external CDP. Local suite: 218 tests; all three exact CI journey steps pass. No new dependencies or worker/CDP capabilities. See docs/JAVASCRIPT.md for deliberate limits and the daily log for publication evidence.

Iteration remote acceptance: 82e41ff passed Rust CI 34142770048 with all 218 tests and all three native/CDP journey steps, including iteration-created forms. Pages 34142770035 deployed matching HTTPS content with an approved apex certificate and HTTPS enforcement. This closes the increment's publication gate, not the Google or full-language acceptance gates.

Expression follow-up: original bounded continuation parsing accepts the authored 64-group DOM factory while retaining source/token/node and structural/AST-depth limits. Shared parser work/storage and mixed evaluator recursion are bounded; default-stack regressions cover nested functions, unary expressions, switch and for-in without increasing stack sizes. The /script-expressions form completes actual-worker, native and external CDP checks with the Unicode query, hidden field and HTTP 200 local destination. Local suite: all 251 debug tests, 141 selected release tests and all three exact CI journey steps pass; native/CDP frames inspected. No dependencies, worker permissions, TLS or CDP commands changed. Publication evidence follows in the daily log; this is not a production stack-safety or full-language claim.

Expression remote acceptance: e1d28c7 passed Rust CI 34146232690 with all 251 debug tests, 141 selected release tests and all three native/CDP journey steps, including the grouped-expression form. Pages 34146232606 deployed matching HTTPS HTML; the apex certificate is approved and HTTPS enforced. This closes the increment's publication gate, not the Google or general-language goals.

Allocation follow-up: fixed-size exclusive phase totals and the first rejected charge survive fatal realm latching and bounded worker transfer; invalid reports reject the projection. Immutable function parameters/bodies are shared while closure identity/state stays distinct. Unused native argument copies and audited duplicate string wrapping charges are removed; true copies and all limits remain. The unchanged large-factory fixture now creates its controls below 4 MiB and completes actual-worker/native/CDP result/destination checks. All 288 debug tests, 178 selected release tests and all three exact CI journey steps pass locally; frames inspected. No dependency, TLS, worker permission or CDP expansion. Publication evidence follows in the daily log.

Remote evidence: implementation b9cde9e passed Rust CI 34133745718 with 88 tests and all three native/CDP journey steps. Pages 34133745636 deployed matching HTTPS content; the certificate is approved, HTTPS enforced and HTTP redirects to HTTPS. Broader JavaScript compatibility remains in-progress.

Follow-up remote evidence: 8603918 passed Rust CI 34136335877 with all 148 tests and every native/CDP journey, including dynamically compiled forms. Pages 34136335825 deployed exact matching HTTPS HTML with the approved apex certificate and HTTPS enforcement. This closes the language increment's publication gate, not F-008/F-010 compatibility.

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

Follow-ups with --enable-scripts: actual homepage and served form submission returned HTTP 200. Labels/URI support advanced the first search error to missing dynamic compilation; after Function/eval support it advanced to an identifier-escape lexer error, alongside another lexer error and missing setTimeout. Search still completed two scripts with three errors, title “Google Search”, no projected results/controls, and journey exit 2. No actual first-result destination has been verified; exact dated checkpoints are in the log.

Post-regex checkpoint: HTTP 200 homepage, three completed scripts/seven errors, 26 items and one form; actual served form submission worked. Search returned HTTP 200, two completed scripts/three errors, no items/forms, with unsupported for-in/other statements and missing setTimeout diagnostics. Journey exit 2; no result link or destination was reached. Changing diagnostics are observations, not proof that the next missing feature will complete the goal.

Post-iteration checkpoint: actual homepage HTTP 200, two completed scripts/eight errors, 26 items and one form; real served form submission worked. Search returned HTTP 200, title “Google Search”, two completed scripts/three parser-nesting-limit errors and no items/forms. Journey exit 2; inspected frame contains no results, and no destination was reached. Parser resource safety, language compatibility and missing browser APIs remain work; this observation does not justify blindly increasing execution limits.

Post-expression checkpoint: actual homepage HTTP 200, two completed scripts/eight errors, 26 items and one form; real served form submission worked with verified TLS and ordinary cookies. Search returned HTTP 200, title “Google Search”, two completed scripts/three allocation-budget errors and no items/forms. The previous parser-nesting diagnostic was absent in this response. Journey exit 2; inspected frame has no results, and no destination was reached. Next is local allocation-phase diagnosis, not blindly increasing the budget. The user goal remains incomplete.

Post-sharing checkpoint: actual homepage HTTP 200, three completed scripts/seven errors, 26 items/one form and no rejected allocation (2,468,375 accepted bytes). The served form submits through verified TLS and ordinary cookies. Search HTTP 200 still has no items/forms: its first rejected AST charge requests 2,575,110 bytes after 2,390,987 accepted bytes, and three errors repeat that same latched failure. Journey exit 2; blank search frame inspected. No result/destination was reached. Next is an independently tested audit of remaining AST/runtime storage costs, without increasing limits or adapting site challenge logic.

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

Evidence: 2026-09-07 initial cargo test --locked --all-targets passed 47 tests (35 library, 9 binary, 3 external-client regressions). External CDP journey passed under Xvfb with exact Unicode query and hidden form field, first-anchor mouse click, HTTP 200 destination and 1100×683 PNG; rendered destination inspected. The Rust command client also inspected and captured the real Google homepage through CDP. Runtime.evaluate still explicitly returns -32601: the new script worker does not yet expose persistent execution contexts or remote objects. Initial subset is working; full protocol remains in-progress.

Remote evidence: implementation commit fba063a passed Rust CI 34129329323 with all 47 tests, both native/CDP journeys and the dependency guard. Pages run 34129329247 deployed matching HTTPS HTML; HTTP redirects to HTTPS. Full protocol inventory/compatibility is the remaining feature gate.
