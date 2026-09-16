# Working on mgbrowser

## Purpose and responsibilities

Completed request (2026-09-16): T-014 / F-017 shipped in v0.5.0 at
`8c457e04a8ce00dd8cf81802da35a054d3bad8ec`. Menu > Settings provides persistent
System/Light/Dark controls and live desktop preference following. Exact-commit
Rust/JSPLAN/Pages/release gates, public checksum/install/reinstall, worker/session,
desktop/icons, actual v0.4.1 self-update and native public-binary theme acceptance
pass. Page colors, HTTP warnings and engine behavior are unchanged. System falls
back to Light when the portal is unavailable; window decorations remain WM-owned.
No release gates remain. Further engineering requires a new request.

Completed request (2026-09-16): the bounded Boa integration shipped as v0.4.1 at
`02b413f27e8ddce9b6408887317f08cd9b9b86b7`. Exact-commit Rust CI, JSPLAN CI,
Pages and release workflow are green. Public assets/checksum, exact curl
install/reinstall, worker/session selftests, real Boa execution, desktop/icons,
two native/two external CDP journeys and fatal-loop fallback/recovery pass.
Actual v0.3.0 and v0.4.0 binaries update to v0.4.1 and then pass a no-op check.
The published archive is 7,240,140 bytes (23,746,560 unpacked), within the
unchanged 8 MiB updater limit. Size-optimized compilation and symbol stripping
preserve engine source, dependencies, resource limits and panic behavior.
v0.4.0's tag/assets remain immutable; its oversize-package failure is recorded
in the dated log. No release gates remain. Stop engineering until a new request.
The production opt-in page path uses `mg-butane::modern` and Sparkle's
`BoaPageRealm` under the
explicit `boa-page-process-v1` profile in `docs/BOA.md`. No production fallback,
native JS backend or worker syscall expansion. Original 4 MiB/fuel assertions
remain in the explicit `legacy-test-engine` test lane. Full P1 cooperative
resource control, F-016 and P2-P8 remain unfinished; Google stays deferred.
See the dated log for release/public-install receipts and remaining limitations.

Completed request (2026-09-16): T-011 / F-014 minimal Hacker News desktop rendering
shipped in v0.3.0 at e3ac7a7b873eb080baf0fa9be61b343b06cbbcb9. Standalone Rust
Stylo computes CSS; Mg owns generic table/inline layout and bounded same-origin
resources. Same-input desktop comparison, live links/hotkeys, exact-commit CI,
Pages, release assets, fresh public install and v0.2.1 self-update passed. See
`docs/HACKER_NEWS.md` and the daily log. This scoped request is complete: do not
resume Google/JavaScript optimization or broaden compatibility without a new task.

Prior completed request (2026-09-16): T-012 / F-015 self-updates and About build
identity shipped in v0.2.1, including the component extraction. That release did
not include HN styling. See docs/UPDATES.md. Do not create a separate v0.2.0 afterward.

Prior authorized task (2026-09-09): extract the original implementation into
`mg-butane`, `mg-sparkle`, `mg-chassis` and the `mg-browser` platform host; preserve
existing behavior, verify the embedding boundaries, and publish main with v0.2.0.
See `docs/ARCHITECTURE.md`. This does not reopen unrelated language, Google or
compatibility work. Prior v0.1.0/v0.1.1 tags and artifacts remain immutable.

Build a browser from the ground up in Rust and a reproducible autoresearch harness that humans and agents can contribute to. Own implementation, evidence, feature specifications, and an accurate public project page within the task requested. A native Linux HTML-flow browser now exists. The historical Google homepage → search → first result → destination goal remains incomplete and deferred.

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
- User clarification (2026-09-16): existing Rust implementation crates, including
  Servo-origin components, are welcome after dependency/features/license/test
  review. Do not limit reuse to utilities or assume upstream reputation proves
  compatibility. No C/C++ implementation backends, including transitive/static
  bindings. The scoped HN task adopted Stylo without replacing Butane or embedding
  Servo's browser. Follow the pinned review and OS-interface boundary in
  `docs/DEPENDENCIES.md`; upstream capabilities do not imply Mg layout support.

## Work and closeout

- Standing user instruction (2026-09-16): publish user-directed project work,
  including plans, documentation and routine changes, directly to `main`. Do not
  open a PR or draft PR for this work unless the user explicitly requests one.
  Preserve unrelated changes and use ordinary fast-forward publication; this does
  not authorize force pushes, history replacement or unrequested implementation.

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
- Run relevant checks, record remaining limitations, and commit finished work.
  Publish user-directed work directly to main under the standing instruction
  above. Outside contributors may still use PRs. Never adopt research results
  merely because a score improved.
- During active work, revisit https://recurse.bot if the last check is over a week old; record useful differences, applying project judgment. No background agent or scheduled research run is implied.

## Verified bootstrap commands

```sh
mkdir -p tmp
rustc --edition=2024 tools/site.rs -o tmp/site
tmp/site
tmp/site --check
git diff --check
```

The site is plain HTML/CSS, without a build dependency download. `.github/workflows/pages.yml` validates and publishes a site-only artifact on pushes to `main`. Run the browser with `cargo run --locked --bin mgbrowser -- https://example.com/`; see `docs/RUNNING.md` for controls and local journey verification. `cargo test --locked --workspace --features legacy-test-engine --all-targets` covers modern and original-baseline JS/DOM, transport, parser, paint, restricted workers and UI state; local X11/CDP journeys exercise real controls. A local fixture is not evidence that Google returns search results. The general autoresearch executor is not implemented. Boa scripting is experimental and opt-in with `--enable-scripts`; read docs/BOA.md and docs/PAGE_SESSIONS.md before changing execution or its boundary. docs/JAVASCRIPT.md preserves the original evaluator's historical contract.

`crates/mg-sparkle/src/document.rs` owns HTML parsing, `crates/mg-chassis/src/net.rs` owns HTTP/TLS/session cookies, `crates/mg-sparkle/src/paint.rs` owns Rust shaping/rasterization, and `crates/mg-sparkle/src/render.rs` owns page layout. Chassis owns navigation and optional chrome; `src/main.rs` composes the window/event loop. Fonts and isolated script services enter Chassis through explicit host APIs. Keep new test pages clearly identified as fixtures. Never replace Google with a fabricated page/result or count an interstitial link as a search result. Browser test screenshots contain page/query data; keep live raw responses and session details in ignored tmp/ by default.

`crates/mg-chassis/src/cdp.rs` owns loopback discovery/WebSocket transport; `crates/mg-chassis/src/cdp_browser.rs` binds the documented CDP subset to real browser behavior. Read docs/CDP.md and its schema before changing protocol commands. Use the external examples/cdp_journey.rs fixture client for CDP input/navigation verification. Protocol support is partial; never return success for an unimplemented behavior or claim general automation-client compatibility without a pinned client test.

`crates/mg-butane/src/modern.rs` owns the Boa execution facade; the original language files remain a test baseline. `crates/mg-sparkle/src/js_browser/boa.rs` binds the shared bounded DOM/navigation operations to modern page execution. `src/platform/script_worker.rs` executes page scripts in a restricted Linux x86_64 child; unsupported isolation fails closed. The worker allocator counts outstanding/cumulative System requests, not GC live heap or RSS. Do not execute live scripts in the parent, weaken TLS, impersonate another browser, or port site challenge logic. Preserve readable fallback on source/projection rejection and keep proposed navigation parent-validated. Browser-visible changes still require a new release, not just a source push.

## Collaboration

Lead with the result, then evidence and limitations. Favor concrete progress and repo-local records. Keep planning distinct from adopted implementation. Report local validation, GitHub deployment, HTTPS delivery, and visual inspection separately.
