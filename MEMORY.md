---
summary: Compact map of mgbrowser project decisions and evidence.
---

# Project memory

- `docs/WPT.md`: pinned upstream static-reftest pilot, source-linked score,
  regression ratchet and every-commit website measurement policy. Broader WPT
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
  holds immutable inputs and reviewed outcome expectations. Browser stays v0.3.0.

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
