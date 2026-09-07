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
- [x] Prepaid array construction/adoption and moved argument snapshots preserve semantics/limits and pass independent, actual-worker and native/CDP six-array form journeys.
- [x] Formal-parameter copies move into local bindings without a second payload charge; independent ingress/copy/limit tests and actual-worker/native/CDP parameter-built forms pass.
- [x] A sole Function parameter-source fragment moves without redundant joining; independent source/coercion/limit tests and actual-worker/native/CDP compiled-form journeys pass.
- [x] Compact statement layout and capacity-aware AST accounting pass independent storage/semantics/limit measurements and actual-worker/native/CDP large-function form journeys.
- [x] Genuine Symbol identities, typed keys, registry/boxing/coercion/reflection and bounded foreign admission pass independent language/DOM/limit and actual-worker/native/CDP form journeys.
- [x] Genuine function/native prototype identities and bounded string/Symbol/metadata traversal pass independent semantics/limits and frozen actual-worker/native/CDP form journeys.
- [x] Six existing Error families have genuine prototypes, instances, ordered string conversion and bounded callback-free diagnostics; independent semantics/limits and frozen actual-worker/native/CDP form journeys pass.
- [x] Bounded generic Array.concat preserves ordered one-level spreading, inherited reads, holes and identity; true Array/Arguments branding and resource admission pass independent tests and frozen worker/native/CDP form journeys.
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

Allocation remote acceptance: dfc3677 passed Rust CI 34148791646 with all 288 debug tests, 178 selected release tests and all three native/CDP journey steps, including shared-code forms. Pages 34148791643 deployed exact matching HTTPS HTML; the apex certificate is approved and HTTPS enforced. This closes the increment's publication gate, not the Google or full-language goals.

Array-ownership follow-up: all eight array-producing paths use a private consuming builder with slot prepayment, bounded geometric growth and retained unused credits. Adoption moves storage without duplicating its charge; argument snapshots move payloads after independent parameter copies. Later mutation, binding/property, ingress, real-copy and regex-reservation policies remain unchanged. The six-array fixture now creates real controls below 4 MiB and completes native/CDP result/destination journeys; a seventh maximum array still exhausts the budget and latches. All 312 debug tests, 202 selected release checks and all three exact CI journey steps pass locally, with rendered frames inspected. No dependencies, caps, worker permissions, TLS or CDP commands changed; publication evidence follows in the daily log.

Array-ownership remote acceptance: 52c9874 passed Rust CI 34150561732 with all 312 debug tests, 202 selected release checks and all three native/CDP journey steps, including the six-array form. Pages 34150561651 deployed exact matching HTTPS HTML with the approved apex certificate and HTTPS enforcement. The published increment is verified; Google and full-language acceptance remain open.

Parameter-binding follow-up: a private copy-and-bind boundary retains the charged independent formal copy and moves it into local storage without recharging its payload. Generic define, public/host/global/property ingress, caught error strings, metadata, true reads/clones and all limits stay unchanged. Independent formal/no-formal string slopes are 4/2 bytes per UTF-16 unit; a 900,000-unit formal succeeds below 4 MiB. The unchanged parameter-built fixture passes real worker/native/CDP form/result/destination checks, while a larger real-copy failure remains fatal and latched. All 327 debug tests, 217 selected release checks and all three exact CI journey steps pass locally; frames inspected. No dependencies, TLS, worker permissions or CDP commands changed. See the log for separate publication evidence.

Parameter-binding remote acceptance: 6b7f33b passed Rust CI 34151944715 with all 327 debug tests, 217 selected release checks and all three native/CDP journey steps, including parameter-built forms. Pages 34151944840 deployed exact matching HTTPS HTML with the approved apex certificate and HTTPS enforcement. This verifies the published increment, not Google or full-language acceptance.

Source-ownership follow-up: after all Function argument conversions, a sole parameter fragment moves its original buffer instead of allocating a redundant join. Zero/multiple fragments, actual source/UTF-8 copies, ingress, grammar and limits retain their charges and behavior. The unchanged whitespace-fragment fixture now creates a usable form at 2,935,365 accepted bytes and reaches the local destination through native/CDP input. All 344 debug tests, 234 selected release checks and all three exact CI journey steps pass locally; frames inspected. Larger multi-fragment and actual-source copies still fail fatally and latch. No existing tests, dependencies, worker permissions, TLS or CDP commands changed. Publication evidence follows in the daily log.

Source-ownership remote acceptance: 1ea6917 passed Rust CI 34153430949 with all 344 debug tests, 234 selected release checks and all three native/CDP journey steps, including the source-built form. Pages 34153430958 deployed matching HTTPS HTML; the apex certificate is approved and HTTPS enforced. This closes the increment's publication gate, not the actual Google results/first-destination goal.

AST-storage follow-up: boxing three loop fields reduces x86_64 statements from 144 to 80 bytes. A private capacity-aware visitor charges all retained container slots, holes, boxes, owned buffers and shared slices with explicit per-block overhead; inline children are not counted twice. Ten independent allocator cases match their retained requests plus the documented overhead exactly. At that increment the frozen 20,017-statement compiled form completed at 2,481,036 accepted bytes, while repeated sparse compilation still failed fatally. That increment passed all 368 debug tests, 258 selected release checks and all three exact CI journey steps locally (11 native/11 CDP destinations); frames inspected. Three old weight-dependent assertions were separately reviewed and updated while retaining semantic and fatal checks. No caps, dependencies, TLS, worker permissions or CDP commands changed. Publication evidence follows in the log.

AST-storage remote acceptance: 5361060 passed Rust CI 34155366324 with all 368 debug tests, 258 selected release checks and all three native/CDP journey steps (11 native/11 CDP destinations). Pages 34155366161 deployed matching HTTPS HTML with an approved apex certificate and HTTPS enforcement. The increment is published and verified; actual Google result links and first-destination acceptance remain incomplete.

Symbol follow-up: opaque immutable Arc handles preserve real identity and public Send/Sync; typed keys keep symbol properties separate from strings, array indices and length. Core registry, boxing, description/branding, own-symbol reflection and actual toPrimitive/toStringTag hooks are implemented, with fallible DOM string conversion and explicit host-key rejection. Destination admission charges foreign retained records even when well-known semantic identity matches. Real getter copies/hook argument slots, registry/description storage and reflection are preflighted under unchanged realm limits; the new cumulative symbol cap is 10,000. The frozen Symbol-dependent form passes actual-worker/native/CDP Unicode submission and destination checks. All 438 debug tests, 340 selected release checks and three exact CI journey steps pass locally (12 native/12 CDP destinations); frames inspected. Existing tests were not weakened. No dependencies, TLS, worker permissions or CDP commands changed; publication evidence follows in the log.

Symbol remote acceptance: 72d76d5 passed Rust CI 34158029766 with all 438 debug tests, 340 selected release checks and three native/CDP journey steps (12 native/12 CDP destinations). Pages 34158029782 deployed matching HTTPS HTML with an approved apex certificate, HTTPS enforcement and HTTP-to-HTTPS redirect. This verifies the published Symbol increment; general language and actual Google results/first-destination acceptance remain incomplete.

Typed-prototype follow-up: ordinary objects, functions and native builtins retain distinct identities separate from property storage. Shared iterative traversal preserves inherited string/Symbol/virtual metadata, original receivers, readonly and enumeration policy; new/getPrototypeOf/instanceof use the actual prototype. Native name retention and real returned/generated-name copies are preflighted, with stable reuse and measured layouts fitting existing allowances. Exact legacy traversal edges and all caps remain. The frozen prototype-dependent form passes actual-worker/native/CDP Unicode submission and destination checks. All 492 debug tests, 394 selected release checks and three exact CI journey steps pass locally (13 native/13 CDP destinations); frames inspected. No existing assertion weakened or dependency/TLS/worker/CDP expansion. Publication evidence follows in the daily log.

Typed-prototype remote acceptance: a3295d5 passed Rust CI 34160242750 with all 492 debug tests, 394 selected release checks and all three native/CDP journey steps (13 native/13 CDP destinations). Pages 34160242756 deployed matching HTTPS HTML with an approved apex certificate, HTTPS enforcement and HTTP-to-HTTPS redirect. This verifies the published generic prototype increment; the latest actual Google results/first-destination journey still fails.

Error-family follow-up: the six already-exposed constructors now have ES5 intrinsic prototype chains, defaults, attributes and correctly linked instances. Generic Error.prototype.toString preserves ordered real property reads and fallible UTF-16 conversion; actual copies and joined buffers stay charged. Genuine Error host diagnostics use bounded data-only inspection without page callbacks, Host access or post-failure realm charges. Ordinary diagnostic strings and fatal latching remain unchanged. Measured Object is 112 bytes within its existing 128-byte allowance; Bootstrap is 25,854. All 547 debug tests, 449 selected release checks and three exact CI journey steps pass locally (14 native/14 external CDP destinations); new form/destination frames inspected. Independent coverage includes 27 semantic, 15 resource and seven private groups. No existing assertion weakened or dependency/TLS/worker/CDP expansion. Exact publication evidence follows in the daily log.

Error-family remote acceptance: 343de7c passed Rust CI 34162343272 with all 547 debug tests, 449 selected release checks and all three native/CDP journey steps (14 native/14 CDP destinations). Pages 34162343107 deployed matching HTTPS HTML with an approved apex certificate, HTTPS enforcement and HTTP-to-HTTPS redirect. This verifies the published Error increment; actual Google search results and first destination remain unverified.

Concat follow-up: generic receivers are boxed, true arrays spread one level through ordered inherited HasProperty/Get, and nonarrays including opaque Host handles append once without callbacks or coercion. Holes, trailing length, identity and UTF-16 are preserved. Array.prototype is a genuine empty array; arguments snapshots have their own brand and Object.prototype parent, with the remaining indexed-length approximation documented. Metadata and geometric slots are prepaid before getters, actual copies stay charged and failures expose no partial result. Object remains 112 bytes and Bootstrap 25,854. All 604 debug tests, 506 selected release checks and three exact CI journey steps pass locally (15 native/15 external CDP destinations); new query/destination frames inspected. Independent coverage includes 26 semantic, 15 resource and 10 private groups. No existing assertion weakened or dependency/TLS/worker/CDP/limit expansion. Exact publication evidence follows in the daily log.

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

Post-array checkpoint: homepage HTTP 200, three completed scripts/seven errors, 26 items/one form and no rejected allocation (2,446,746 accepted bytes). Real form submission works with verified TLS and ordinary cookies. Search HTTP 200 still has zero items/forms, two completed scripts and three errors repeating one AST rejection: accepted 1,822,051/requested 2,575,110/limit 4,194,304 bytes. The remaining admission gap is 202,857 bytes; Runtime charges are lower, but no result/destination was reached. Journey exit 2; blank frame inspected. Continue independent ownership/AST accounting work without fabricating results or claiming that one more optimization completes the goal.

Post-parameter checkpoint: real homepage/form submission still works with verified TLS and ordinary cookies; homepage has 26 items/one form, three completed scripts/seven errors and no allocation rejection (2,448,406 accepted bytes). Search HTTP 200 still has no items/forms, two completed scripts and three errors repeating a single AST rejection: accepted 1,684,603/requested 2,575,110/limit 4,194,304. Runtime charges are 1,379,528; remaining admission gap 65,409 bytes. Exit 2/JOURNEY_INCOMPLETE; blank frame inspected with no result/destination. This is an observed response, not proof that one more change will finish compatibility.

Post-source checkpoint: homepage HTTP 200 and real form submission still work with verified TLS and ordinary cookies. Search HTTP 200 still has no items/forms; the two completed scripts, three latched errors and all accepted phase totals match the prior checkpoint, including an AST rejection after 1,684,603 total accepted bytes/requested 2,575,110/limit 4,194,304. Gap remains 65,409 bytes. Exit 2; blank frame inspected, no result/destination. The sole-fragment optimization has no observed benefit on this served search response. Next is measured AST representation and container-aware storage accounting, not another assumption about source-copy savings.

Post-AST checkpoint: homepage HTTP 200 retains 26 items/one form, three completed scripts/seven errors and no rejected allocation (2,255,667 accepted bytes). The actual served form submits through verified TLS and ordinary cookies. Search HTTP 200 still has zero items/forms and two completed scripts/three errors, but its first error is now `ReferenceError: Symbol is not defined`. A later script rejects an AST request of 387,844 after 4,078,595 accepted bytes; the third error repeats that latched failure. Search accepted Ast is 2,438,297 and Runtime 1,451,075. Exit 2/JOURNEY_INCOMPLETE; blank frame inspected with no result/destination. This response progressed beyond the prior admission error, not to Google compatibility. Next is independently specified Symbol/property-key support and separate cumulative-storage diagnosis.

Post-Symbol checkpoint: homepage HTTP 200 retains 26 items/one form, three completed scripts/seven errors and no allocation rejection (2,506,006 accepted bytes). The actual form submits through verified TLS and ordinary cookies. Search HTTP 200 still has no items/forms and two completed scripts/three errors: first `TypeError: prototype must be an object or null`, then Ast 387,964 rejected after 4,107,538 accepted bytes, repeated by the next script. Search accepted Ast is 2,438,297 and Runtime 1,474,970. Exit 2; blank frame inspected, no actual result or destination. Missing Symbol is no longer reported, but the new message alone does not identify the supplied prototype or cause. Next is independent object/prototype correctness and cumulative-storage work, not site-source adaptation or raised limits.

Post-typed-prototype checkpoint: homepage HTTP 200 and actual form submission still work through verified TLS and ordinary cookies, with 26 items/one form, three completed scripts/seven errors and no allocation rejection (2,506,470 accepted bytes). Search HTTP 200 remains blank with zero items/forms, two completed scripts/three errors: the same prototype TypeError, then Ast 387,500 rejected after 4,107,727 accepted bytes, repeated by the next script. Exit 2, no result or destination. The independently verified generic correction did not resolve that leading live diagnostic. Further builtin/prototype coverage and storage work must use authored cases without assuming the live argument or adapting site source.

Post-Error checkpoint: homepage HTTP 200 and actual form submission work through verified TLS and ordinary cookies, with 26 items/one form, three completed scripts/seven errors and no allocation rejection (2,512,389 accepted bytes). Search HTTP 200 still has zero items/forms, two completed scripts/three errors. First is now `Unsupported JavaScript behavior: Array.concat`; later Ast 387,620 is rejected after 4,132,042 accepted bytes, then repeated. Exit 2/JOURNEY_INCOMPLETE; blank frame inspected, no result or destination. The prior prototype error is absent in this response, not proof of its original argument or a controlled site benchmark. Next is independently specified bounded Array.concat support and separate cumulative-storage diagnosis; no live-source adaptation or raised limits.

Post-concat checkpoint: homepage HTTP 200 retains 26 items/one form, three completed scripts/seven errors and no allocation rejection (2,511,733 accepted bytes). Actual form submission works through verified TLS and ordinary cookies. Search HTTP 200 is still blank with zero items/forms and two completed scripts/three errors, now repeating a FunctionCode allocation rejection: 4,194,294 accepted, 128 requested, 4,194,304 limit. Search Ast is 2,438,297 and Runtime 1,575,999. Exit 2/JOURNEY_INCOMPLETE; blank frame inspected, no result or destination. Unsupported concat is absent in this response, not a controlled benchmark or proof that one more allocation change will complete compatibility. Next is independently measured cumulative-storage ownership work without raised limits or live-source adaptation.

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
