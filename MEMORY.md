---
summary: Compact map of mgbrowser project decisions and evidence.
---

# Project memory

- `docs/ARCHITECTURE.md`: adopted Butane/Sparkle/Chassis/platform boundaries, optional chrome, embedding examples and future compatibility/ThermiteOS gates.

- `docs/MVP.md`: proposed product boundary, architecture and release gates.
- `docs/DEPENDENCIES.md`: adopted Rust-only TLS/font/image policy and initial Cargo configuration.
- `docs/AUTORESEARCH.md`: proposed reproducible experiment contract.
- `docs/CDP.md`: implemented automation subset, local client checks and full-protocol roadmap.
- `docs/JAVASCRIPT.md`: original language subset, restricted worker boundary, limits and local-versus-live evidence.
- `docs/OBJECT_CREATE.md`: adopted bounded descriptor/accessor contract; acceptance and old-baseline evidence in the latest daily log.
- `memory/logs/`: dated work evidence; start with the latest relevant entry.
- `memory/learnings.md`: observations awaiting promotion into guidance.
- `memory/notes/decisions.md`: explicit user direction and unresolved choices.

Search these files with `rg` before substantial work. Keep private data and secrets out of this public repository.
