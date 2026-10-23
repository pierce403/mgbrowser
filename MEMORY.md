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
  Continuous Window CI/public scoring remains pending; engine fixes are separate.

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
