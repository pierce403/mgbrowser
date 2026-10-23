---
summary: Compact map of mgbrowser project decisions and evidence.
---

# Project memory

- `docs/WPT.md` and `docs/WPT_EXPANSION.md`: complete pinned 1,011-case flexbox
  reftest selection, immutable original 24-case ratchet, source-linked score,
  diagnostic-free pass prerequisites and every-commit website measurement policy. Broader WPT
  support remains open; selected static pixels are not JavaScript conformance.

- `docs/APPEARANCE.md`: System/Light/Dark browser controls, persistent XDG
  preferences, desktop DPI sizing with manual overrides, Rust-only desktop
  discovery and isolated native acceptance.

- `docs/ARCHITECTURE.md`: adopted Butane/Sparkle/Chassis/platform boundaries, optional chrome, embedding examples and future compatibility/ThermiteOS gates.
- `JSPLAN.md`: proposed Butane engine strategy, Rust reuse evaluation, V8 techniques,
  academic sources and modern framework/embedding gates (F-016 / T-013); no engine
  adoption or new compatibility is implied by the plan.
- `docs/jsplan/RESULTS.md`: implemented P0/narrow P1 comparison, exact dependency
  review, selected Test262/Vue results and remaining Boa adoption gates.
  `experiments/jsplan` is excluded from the production workspace; `tools/jsplan`
  holds immutable inputs and reviewed outcome expectations. Production Boa page
  execution has its separate contract in docs/BOA.md.

- `docs/PERFORMANCE.md`: Browser paint/CLI measurements, cache identity and
  explicit retained-payload ceilings; unavailable worker accounting stays visible.

- `docs/PROMISE_REJECTIONS.md`: shipped v0.9.9's queued rejection notification,
  shared work, traced/weak ownership and persistent privacy-observation gaps.
  Pinned Boa fatal VM roots require actual terminal teardown, not an extra
  kept-object clear. Release/public acceptance is separate from WPT coverage.
- `docs/WINDOW_MESSAGES.md`: v0.9.10 bounded self-messaging, partial clone
  domain, measured unchanged-harness initialization and separate lifecycle review.
- `docs/DOCUMENT_LIFECYCLE.md`: shipped v0.9.11 queued snapshot lifecycle,
  strict v5 progress and sticky initial outcomes, after the measured public
  v0.9.10 baseline. Exact release/public receipts are in the dated log; full
  Window readiness and broader WPT integration remain
  open.
- `docs/WPT_OBSERVER.md`: immutable frozen bounded typed observer contract.
  `docs/WPT_OBSERVER_STATUS.md`: released v0.9.12 implementation and separate
  local/public receipts at source `9f7b953a`. Full Window readiness and broader
  WPT integration remain open; zero added passes, static corpus/limits unchanged.
- `docs/WPT_HARNESS_OUTPUT.md` and its STATUS companion: v0.9.13 fixed trusted
  output:false configuration, separate frozen five-new/sixteen-original controls
  and completed public release acceptance. Ordinary browsing is unchanged.
- `docs/WPT_WINDOW_PARSING.md` and `tools/wpt/window/corpus.json`: separate
  pre-execution freeze of sixteen genuine upstream files / 79 source calls;
  source-only checker, fixed report/resource policy and future baseline gates.
  `docs/WPT_WINDOW_RUNNER.md` and first-baseline.json record the reviewed first
  local real-Browser run: 0/16 passing files, 0/79 actual final subtests passing,
  all test-body getPropertyValue failures, zero infrastructure/PENDING outcomes.
  Separate current-score.json/manual ratchet and parallel CI/public-report
  staging keep the 16-file Window lane distinct from static scoring. Engine
  fixes are separate releases. `docs/WPT_WINDOW_CI.md`: genuinely cold pilot
  accepted at fec3803, unchanged deadlines and actual critical-path evidence;
  coupled publication requires its own exact-source verification.
  That coupled gate now passes at cab9375: all eight exact-source checks,
  separate current reports/public12 parity, retained CI data and visual checks.
- `docs/CSSOM.md`: shipped v0.9.14 modern element-style declared-value bridge at
  `dfddb067b92bc56a06e721a99b183f56225f6177`,
  using existing pinned Stylo with no new worker profile/dependencies. Frozen
  authored API/quota/fresh/retained-worker and full local debug/release checks
  pass. Reviewed Window coverage improves to 15/16 files and 77/79 final subtests;
  two genuine flex-valid assertions remain failing, fifteen pass files are
  manually protected. Exact-source CI/site/release, public install/reinstall,
  actual v0.9.13 update/no-op, both frozen public-worker suites and all thirteen
  public native helpers pass; no v0.9.14 release gate remains. Separate static
  measurement at that release was 317/632/62 of 1,011. Full CSSOM and F-016/F-026/MVP remain incomplete;
  the active broader goal next requires a separately reviewed feature increment.
- `docs/PHYSICAL_JUSTIFY.md`: shipped v0.9.15 layout contract at
  `7e13cca45c525921a5072a00eab658cf2782d4e9` for
  physical left/right content justification. Existing static failures motivate
  axis-aware flex/grid mapping, with explicit reversed-flex Safe refusal until
  correct overflow fallback exists. Authored old-behavior failures and focused
  debug/release controls and full final-version regressions/JSPLAN pass after
  the minimal adapter implementation. Reviewed static coverage is319/630/62
  of1011, with two genuine column reftest passes manually protected; the Window
  lane remains15/16 files and77/79 final rows. Normal package/native/CDP and frozen
  worker-control checks pass. Exact-source CI/site, release/public install,
  actual v0.9.14 update/no-op, both public-worker suites, thirteen public native
  helpers and the separate public physical proof pass; no v0.9.15 release gate
  remains. See [the public closeout](memory/logs/2026-10-23.md#v0915-public-release-closeout);
  the separate container-unit dependency/renderer change remains deferred.

- `docs/MVP.md`: proposed product boundary, architecture and release gates.
- `docs/DEPENDENCIES.md`: adopted Rust-only TLS/font/image policy and initial Cargo configuration.
- `docs/HACKER_NEWS.md`: v0.3.0's accepted desktop scope, standalone Rust Stylo,
  bounded resources, visual/live evidence and explicit compatibility exclusions.
- `docs/AUTORESEARCH.md`: proposed reproducible experiment contract.
- `docs/CDP.md`: implemented automation subset, local client checks and full-protocol roadmap.
- `docs/JAVASCRIPT.md`: original language subset, restricted worker boundary, limits and local-versus-live evidence.
- `docs/OBJECT_CREATE.md`: adopted bounded descriptor/accessor contract; acceptance and old-baseline evidence in the latest daily log.
- `memory/logs/`: dated work evidence; start with the latest relevant entry.
- `memory/learnings.md`: observations awaiting promotion into guidance.
- `memory/notes/decisions.md`: explicit user direction and unresolved choices.

Search these files with `rg` before substantial work. Keep private data and secrets out of this public repository.
