# Working on mgbrowser

## Purpose and responsibilities

Build a browser from the ground up in Rust and a reproducible autoresearch harness that humans and agents can contribute to. Own implementation, evidence, feature specifications, and an accurate public project page within the task requested. A native Linux HTML-flow browser now exists. The active goal is Google homepage → search → first result → destination, and it remains incomplete because the verified Google response requires JavaScript.

## Start here

- Read `FEATURES.md`, `TASKS.md`, `MEMORY.md`, and `SKILLS.md`; search relevant notes before substantial work.
- Check git status and preserve unrelated changes. `AGENTS.md` is canonical; harness aliases point here.
- Read affected feature Properties, Dependencies, and Test Criteria before changes. Exact stability values are `planned`, `in-progress`, and `stable`. Only verified, complete behavior is stable.
- Follow `docs/MVP.md` for scope and `docs/AUTORESEARCH.md` for the proposed experiment contract. Do not silently substitute an existing browser engine or JavaScript runtime.
- The user chose experimental `rustls-rustcrypto` TLS and Rust-only font/image implementations. Follow `docs/DEPENDENCIES.md`; no native codec/font/crypto fallback. Unsupported images are preferable to C bindings. Keep certificate verification enabled despite research status.

## Work and closeout

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

The site is plain HTML/CSS, without a build dependency download. `.github/workflows/pages.yml` validates and publishes a site-only artifact on pushes to `main`. Run the browser with `cargo run --locked --bin mgbrowser -- https://www.google.com/`; see `docs/RUNNING.md` for controls and local journey verification. `cargo test --locked` covers transport, parser, paint and UI state; the local X11 journey exercises the actual application handlers. A local fixture is not evidence that Google returns search results. The general autoresearch executor and JavaScript engine are not implemented.

`src/document.rs` owns HTML parsing, `src/net.rs` owns HTTP/TLS/session cookies, `src/paint.rs` owns Rust shaping/rasterization, and `src/main.rs` owns layout and window/input/navigation. Keep new test pages clearly identified as fixtures. Never replace Google with a fabricated page/result or count an interstitial link as a search result. Browser test screenshots contain page/query data; keep live raw responses and session details in ignored tmp/ by default.

`src/cdp.rs` owns loopback discovery/WebSocket transport; `src/cdp_browser.rs` binds the documented CDP subset to real browser behavior. Read docs/CDP.md and its schema before changing protocol commands. Use the external examples/cdp_journey.rs fixture client for CDP input/navigation verification. Protocol support is partial; never return success for an unimplemented behavior or claim general automation-client compatibility without a pinned client test.

## Collaboration

Lead with the result, then evidence and limitations. Favor concrete progress and repo-local records. Keep planning distinct from adopted implementation. Report local validation, GitHub deployment, HTTPS delivery, and visual inspection separately.
