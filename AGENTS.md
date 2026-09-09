# Working on mgbrowser

## Purpose and responsibilities

Current authorized task (2026-09-09): extract the original implementation into
`mg-butane`, `mg-sparkle`, `mg-chassis` and the `mg-browser` platform host; preserve
existing behavior, verify the embedding boundaries, and publish main with v0.2.0.
See `docs/ARCHITECTURE.md`. This does not reopen unrelated language, Google or
compatibility work. Prior v0.1.0/v0.1.1 tags and artifacts remain immutable.

Build a browser from the ground up in Rust and a reproducible autoresearch harness that humans and agents can contribute to. Own implementation, evidence, feature specifications, and an accurate public project page within the task requested. A native Linux HTML-flow browser now exists. The active goal is Google homepage → search → first result → destination, and it remains incomplete because the verified Google response requires JavaScript.

## Start here

- Current project-authored code, documentation and original artwork use Apache-2.0.
  Preserve third-party licenses. Published v0.1.0/v0.1.1 archives remain MIT;
  future release notes/packages must reflect Apache-2.0 and include LICENSE/NOTICE.

- Read `docs/ARCHITECTURE.md` for the component dependency contract. Run `python3 tools/check-components.py` for boundary changes and `cargo test --locked -p mg-chassis --no-default-features --test embedding` for optional-UX changes.
- Read `FEATURES.md`, `TASKS.md`, `MEMORY.md`, and `SKILLS.md`; search relevant notes before substantial work.
- Check git status and preserve unrelated changes. `AGENTS.md` is canonical; harness aliases point here.
- Read affected feature Properties, Dependencies, and Test Criteria before changes. Exact stability values are `planned`, `in-progress`, and `stable`. Only verified, complete behavior is stable.
- Follow `docs/MVP.md` for scope and `docs/AUTORESEARCH.md` for the proposed experiment contract. Do not silently substitute an existing browser engine or JavaScript runtime.
- The user chose experimental `rustls-rustcrypto` TLS and Rust-only font/image implementations. Follow `docs/DEPENDENCIES.md`; no native codec/font/crypto fallback. Unsupported images are preferable to C bindings. Keep certificate verification enabled despite research status.

## Work and closeout

- Standing user instruction (2026-09-08): every user-facing feature addition must
  finish with a new versioned GitHub Release and an updated, verified website
  installer. A source-only push is not feature completion. Bump the package/lock
  version, write matching release notes, refresh site/README, pass exact-commit
  CI/Pages, tag that commit, verify release assets/checksum and a fresh public
  install including version/worker/desktop/icons. Keep old tags immutable.
  The installer tracks GitHub latest; update its code when needed and verify it
  delivers the new feature release every time. Documentation-only changes do not
  require a new binary release. This does not authorize unrelated feature work.

- Keep changes bounded and reproducible. Update feature behavior and evidence in the same contribution.
- Append a dated entry to `memory/logs/YYYY-MM-DD.md` (America/Los_Angeles) with intent, decisions, commands/results, failures, and next steps. Never record credentials or private browsing data.
- Stage reusable observations in `memory/learnings.md`; promote verified recurring lessons into concise instructions or skills. User decisions can be recorded directly. Do not turn a single failure into a universal rule.
- Select skills automatically from `SKILLS.md` when their descriptions match; read the selected `SKILL.md`. Use `curator` when procedures change. Keep skills in tracked `skills/`, accessible to any harness through this catalog.
- Keep `index.html` accurate whenever milestones or project scope change. Run the Rust site tool below to refresh feature status and the latest log date. It must pass `--check` before publication.
- Run relevant checks, record remaining limitations, and commit finished work. Push when the task authorizes publication; public contributions use PRs. Never auto-merge research results merely because a score improved.
- During active work, revisit https://recurse.bot if the last check is over a week old; record useful differences, applying project judgment. No background agent or scheduled research run is implied.

## Verified bootstrap commands

```sh
mkdir -p tmp
rustc --edition=2024 tools/site.rs -o tmp/site
tmp/site
tmp/site --check
git diff --check
```

The site is plain HTML/CSS, without a build dependency download. `.github/workflows/pages.yml` validates and publishes a site-only artifact on pushes to `main`. Run the browser with `cargo run --locked --bin mgbrowser -- https://www.google.com/`; see `docs/RUNNING.md` for controls and local journey verification. `cargo test --locked --workspace --all-targets` covers transport, parser, paint, original JS/DOM behavior, restricted workers and UI state; local X11/CDP journeys exercise real controls. A local fixture is not evidence that Google returns search results. The general autoresearch executor is not implemented. The original JavaScript subset is experimental and opt-in with `--enable-scripts`; read docs/JAVASCRIPT.md before changing execution or its boundary.

`crates/mg-sparkle/src/document.rs` owns HTML parsing, `crates/mg-chassis/src/net.rs` owns HTTP/TLS/session cookies, `crates/mg-sparkle/src/paint.rs` owns Rust shaping/rasterization, and `crates/mg-sparkle/src/render.rs` owns page layout. Chassis owns navigation and optional chrome; `src/main.rs` composes the window/event loop. Fonts and isolated script services enter Chassis through explicit host APIs. Keep new test pages clearly identified as fixtures. Never replace Google with a fabricated page/result or count an interstitial link as a search result. Browser test screenshots contain page/query data; keep live raw responses and session details in ignored tmp/ by default.

`crates/mg-chassis/src/cdp.rs` owns loopback discovery/WebSocket transport; `crates/mg-chassis/src/cdp_browser.rs` binds the documented CDP subset to real browser behavior. Read docs/CDP.md and its schema before changing protocol commands. Use the external examples/cdp_journey.rs fixture client for CDP input/navigation verification. Protocol support is partial; never return success for an unimplemented behavior or claim general automation-client compatibility without a pinned client test.

`crates/mg-butane/src/` owns the original language implementation; `crates/mg-sparkle/src/js_browser.rs` exposes bounded DOM/navigation capabilities. `src/platform/script_worker.rs` executes page scripts in a restricted Linux x86_64 child; unsupported isolation fails closed. Do not execute live scripts in the parent, weaken TLS, impersonate another browser, or port site challenge logic. Unsupported language/platform behavior is compatibility work, not permission for a substitute engine. Preserve readable fallback on source/projection rejection and keep proposed navigation parent-validated.

## Collaboration

Lead with the result, then evidence and limitations. Favor concrete progress and repo-local records. Keep planning distinct from adopted implementation. Report local validation, GitHub deployment, HTTPS delivery, and visual inspection separately.
