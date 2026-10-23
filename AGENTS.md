# Working on mgbrowser

## Purpose and responsibilities

Completed increment (2026-10-22): fixed trusted Window harness configuration
ships as v0.9.13 at `948e86206c23c0310d17dc286ac5107a15f1f434`.
Exact-source CI/site/release, public install/reinstall, actual v0.9.12 update/no-op,
both frozen public-worker suites and all thirteen existing native helpers pass.
Receipts are in docs/WPT_HARNESS_OUTPUT_STATUS.md and the dated log. Both prior
contracts/inputs, legacy bytes and all limits remain unchanged; zero upstream
passes are added. F-016/F-026/formal MVP remain open.

Completed tooling increment (2026-10-22): the fixed real-Browser Window runner
and reviewed historical first baseline are published at
`32da1ab929f24cdb2a2081af44358878046dfd06`, with all six exact-source CI/hosting
checks and public byte/report verification passing. All sixteen original files
run: 0/16 files and 0/79 actual final subtests pass, with no infrastructure or
PENDING outcomes. Every subtest reports a caught exception at missing
style.getPropertyValue(), before CSS comparisons. See docs/WPT_WINDOW_RUNNER.md.
Preserve the unchanged pre-execution contract, original inputs, static score
and authored suites.

Completed prerequisite (2026-10-23): the genuinely cold-Cargo Window pilot at
`fec3803991ae623cac4721f8dc012fe16887db84` passes inside the existing limits.
All seven exact-source checks and public byte/report verification pass. The cold
owner takes 218.129 seconds including 213.884 seconds of provisioning/build;
all sixteen files and 79 actual final subtests fail honestly. See the dated log.
Completed tooling (2026-10-23): continuous Window score/public reports pass all
eight exact-source checks at `cab93756337623ed65a5b809401fb977c075c7a1`, twelve
public HTTPS asset/report checks, separate CI raw-evidence review and visual
acceptance. Actual coupled publication finishes within the unchanged wait.
Completed browser increment (2026-10-23): v0.9.14 parser-backed modern
element-style declarations ship at `dfddb067b92bc56a06e721a99b183f56225f6177`
under docs/CSSOM.md. Full local debug/release, frozen JSPLAN, all eight
exact-source CI/hosting checks and normal tagged release pass. Public
install/reinstall/CSSOM/worker/session/desktop/icon checks, actual v0.9.13
update/no-op, both frozen public-worker suites and all thirteen public native
helpers pass. No v0.9.14 release gate remains. Separate current-source scores
are static 317 PASS / 632 FAIL / 62 UNSUPPORTED of 1,011 and Window 15/16 files,
77/79 final subtests; two genuine flex-valid assertions still fail and the manual
ratchet protects fifteen files. Full CSSOM/F-016/F-026/formal MVP remain open.
The broader goal remains
active: review the next bounded feature before implementation, preserving its
acceptance and release gates. Never widen a resource profile, synthesize missing
rows or merge the two score lanes.

Current bounded increment (2026-10-23): physical content justification under
docs/PHYSICAL_JUSTIFY.md is implemented for v0.9.15 preparation. Full local
regressions/JSPLAN pass; independently reviewed static319/630/62 of1011 protects
two additional original column passes, with separate Window15/16 and77/79
unchanged. Normal package/install/updater, existing native gates and separate
physical native/CDP proof pass, as do both frozen worker-control suites.
Exact-source CI/public release gates remain pending.
Do not begin another engine increment before this release finishes.

Current authorized request (2026-09-23): integrate genuine pinned upstream Web
Platform Tests and maximize honest passing coverage. Follow docs/WPT.md and
T-023 / F-026. Keep corpus/runner changes separate from engine changes scored
against them; never hide failures, shrink denominators or relax security limits
to improve the score. The first static reftest pilot is not full WPT, WebDriver
or JavaScript testharness support. Browser feature fixes still require releases.

Standing user instruction (2026-09-23): show current WPT scores on the website
and update them at every commit. Run tools/wpt/run.py --update-score into a new
ignored output directory, review the results, run tmp/site and commit the score
and generated page. Pages rebuilds and reruns the selection with --check-score
on every push/PR, then publishes that exact commit's report as wpt-results.json.
Unchanged counts on a documentation commit are valid only after that fresh gate.
Do not automatically rewrite expectations.json to accept regressions.

The separate fixed Window lane uses `tools/wpt/window/run.py --update-score`
in a new ignored directory, followed by full evidence review and `tmp/site`.
Its `current-score.json` is generated; `ratchet.json` is manually reviewed and
never auto-strengthened or weakened. CI runs both full lanes independently with
--check-score; staging requires both clean exact-source reports before publishing
the separate wpt-results.json and wpt-window-results.json. Clean semantic Window
timeouts remain non-PASS under the frozen contract; they do not waive the static
lane's zero-timeout rule. No ERROR/CRASH/PENDING or uncertain cleanup is adoptable.

Prior authorized request (2026-09-17): native right-click element inspection
and useful CSS/resource/JavaScript diagnostics; live detachable tabs and left/right
page groups; pinned Playwright control; and signed-out Google News desktop reading.
User confirmed faithful reading first, with interactions as a later milestone.
Track the separate acceptance gates in docs/DESKTOP_NEXT.md and FEATURES.md.
Do not equate CDP attachment with locator support or static news rendering with
working account/personalization features. Finish each user-visible increment
through the standing release policy. Existing worker/security/resource assertions
remain in force; any new resource profile requires an explicit documented review.

Completed request (2026-09-16): T-015 / F-018 shipped as v0.6.0 at
`363065dc17b51d8159d3434b74e393686bdc9d99`. System follows effective X11 DPI;
saved 75..300% sizing and Ctrl+plus/minus/0 scale controls and pages together,
with physical glyph rasterization and logical input/CDP coordinates. Exact-
commit CI/Pages/release, public install, actual v0.5.0 upgrade and public native
scale acceptance pass. An extra no-op update recheck hit GitHub's anonymous API
quota; do not call that check passed or bypass the limit. See docs/UPDATES.md.
Scope is screen-global X11 sizing, not per-monitor Wayland or per-site zoom.
No feature/release work remains. Further engineering requires a new request.

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

`crates/mg-sparkle/src/document.rs` owns HTML parsing, `crates/mg-chassis/src/net.rs` owns HTTP/TLS/session cookies, `crates/mg-sparkle/src/paint.rs` owns Rust shaping/rasterization, and `crates/mg-sparkle/src/render.rs` owns page layout. Chassis owns navigation and optional chrome; `src/main.rs` dispatches arguments/worker modes and `src/desktop.rs` composes native windows/input. `src/workspace.rs` owns live tab placement without cloning/reloading pages. Fonts and isolated script services enter Chassis through explicit host APIs. Keep new test pages clearly identified as fixtures. Never replace Google with a fabricated page/result or count an interstitial link as a search result. Browser test screenshots contain page/query data; keep live raw responses and session details in ignored tmp/ by default.

`crates/mg-chassis/src/cdp.rs` owns loopback discovery/WebSocket transport; `crates/mg-chassis/src/cdp_browser.rs` binds the documented CDP subset to real browser behavior. Read docs/CDP.md and its schema before changing protocol commands. Use the external examples/cdp_journey.rs fixture client for CDP input/navigation verification. Protocol support is partial; never return success for an unimplemented behavior or claim general automation-client compatibility without a pinned client test.

`crates/mg-butane/src/modern.rs` owns the Boa execution facade; the original language files remain a test baseline. `crates/mg-sparkle/src/js_browser/boa.rs` binds the shared bounded DOM/navigation operations to modern page execution. `src/platform/script_worker.rs` executes page scripts in a restricted Linux x86_64 child; unsupported isolation fails closed. The worker allocator counts outstanding/cumulative System requests, not GC live heap or RSS. Do not execute live scripts in the parent, weaken TLS, impersonate another browser, or port site challenge logic. Preserve readable fallback on source/projection rejection and keep proposed navigation parent-validated. Browser-visible changes still require a new release, not just a source push.

## Collaboration

Lead with the result, then evidence and limitations. Favor concrete progress and repo-local records. Keep planning distinct from adopted implementation. Report local validation, GitHub deployment, HTTPS delivery, and visual inspection separately.

## Native Cloudflare website publishing

The canonical public build keeps its existing validation and Pages recovery.
Only after that artifact is validated (and releases published where required),
a repository-only CI token mirrors its public bytes into codex/cloudflare-static.
Cloudflare native Git builds publish main from /cloudflare after waiting for the exact canonical source artifact; credentials
stay in Cloudflare and previews are off. Source main and generated publication
commits are separate identities. A stale-main check and non-forced branch update
prevent older CI output from replacing newer source. No runtime, benchmark,
firmware, APK, or signing behavior is changed by this hosting-only migration.
Usage uses zone request metrics with no browser analytics.

Keep code changes on main. The generated branch is public output only, not a
source development branch. Never bypass canonical release or score gates.

Native production remains on main. The bounded build-time artifact wait requires
a matching source revision (or the canonical FrogAlert metadata-child receipt),
then verifies all public hashes. Missing/failed canonical output stops deployment
and retains the working version. The generated branch distributes public build
input only. No GitHub lookup is added to the deployed website.
