# Feature specification

Exact stability values: `planned`, `in-progress`, `stable`. Checked criteria require recorded evidence. This file drives the public status section.

Feature delivery policy: user-facing additions include a versioned GitHub Release
and a verified current website installer before handoff. The installer tracks
GitHub latest. See skills/publish-site/SKILL.md for exact-commit publication gates.

## F-025 : Google News desktop reading

Stability: planned

### Dependencies

F-005 layout/resources, F-022 diagnostics and reviewed bounded resource profiles.

### Properties

Signed-out, scripts-disabled real Google News desktop reading first.
No account/personalization or general web compatibility claim. See docs/DESKTOP_NEXT.md.

### Test criteria

- [ ] Same-input desktop comparison: recognizable header, briefing, story groups and thumbnails.
- [ ] Fresh live page, aligned scroll/click, actual served link navigation and Back.
- [ ] Reviewed Rust-only dependencies/resource policies, focused regressions and all old gates.
- [ ] Versioned release and public installer/native verification.

## F-024 : Pinned Playwright browser control

Stability: in-progress

### Dependencies

F-011 truthful CDP, F-023 stable tab targets, isolated Runtime/DOM support.

### Properties

Unmodified pinned Playwright client: attachment is not locator compatibility.
Unsupported commands and browser capabilities must fail explicitly.

### Test criteria

- [ ] Actual client connects and enumerates/creates/selects/closes stable page targets.
- [ ] Navigation/title, locator fill/click, real result, screenshot and error reporting pass.
- [ ] Target identity survives switching/detachment; stale/closed targets reject.
- [ ] Packaged/public release journey and all existing resource/isolation gates pass.

The pinned, unmodified playwright-core 1.58.2 probe is checked in under
tools/playwright. Public v0.8.0 fixes discovery but still fails auto-attachment
and download-policy initialization, before title or locators. See
docs/PLAYWRIGHT.md. A passing custom CDP client is not a substitute for this gate.

## F-023 : Detachable tabs and left/right page groups

Stability: stable

### Dependencies

F-013 host boundaries, F-018 sizing, F-011 target routing, F-015 restart/update.

### Properties

Move live page ownership across bounded tab groups/windows, not reconstructed URLs.
At most two groups per window; fresh isolated script service per tab.
See docs/DESKTOP_NEXT.md for input, restart and automation boundaries.

### Test criteria

- [x] Stable ownership, transactional moves, bounds and close/focus repair tests.
- [x] Native new/switch/close/reorder/detach/redock/left-right docking preserves live state.
- [x] Scaled focus/drag/input, independent workers and shared host preferences remain correct.
- [x] Multi-target CDP stays attached to the named page across placement changes.
- [x] Existing native/embedding/resource checks, release and public installation pass.

Shipped in v0.8.0 at 8b84604c83541beade431cfbedaa2dc449878011. Exact-commit
Rust/JSPLAN/Pages/release gates and public install pass. Public native repeats:
tmp/tabs-smoke.ZeAoeU, tmp/workspace-cdp-smoke.Fozdg6 and
tmp/workspace-restart.SToSEn. Actual v0.7.2 update/restart also passes.
See the 2026-09-17 log for hashes, run IDs and bounded scope.

## F-022 : Native inspection and compatibility diagnostics

Stability: stable

### Dependencies

F-004 painted DOM/input, F-005 CSS, F-013 optional chrome, F-017/F-018 appearance.

### Properties

Right-click Inspect element and keyboard toggle open a bounded read-only native
Elements/Diagnostics overlay. Retain real source/error details and explicit
truncation. This is not Chrome DevTools, a JS debugger/evaluator or a full CSS audit.
See docs/INSPECTOR.md for controls, diagnostic scope and native acceptance.

### Test criteria

- [x] Actual clicked node, ancestor/attribute/text/box inspection and stale selection rejection.
- [x] Bounded CSS parse/source errors, unsupported layout and resource/script diagnostics.
- [x] Panel input isolation, compact/scaled light/dark native checks and unchanged closed page.
- [x] No-chrome/worker/old tests preserved; release and public installer checks pass.

Shipped in v0.8.0 at 8b84604c83541beade431cfbedaa2dc449878011. All existing
CI/embedding/worker gates and public installation pass. Public native Inspector
light/dark 100/200% repeats pass in tmp/inspector-smoke.h6S3QS, including compact
controls, real diagnostics, modal isolation and navigation invalidation.
See the 2026-09-17 log. This does not complete Playwright or full CSS inspection.

## F-021 : Smooth wheel scrolling

Stability: stable

### Dependencies

F-004 native input/navigation, F-018 logical/physical sizing, F-013 host boundaries.

### Properties

Native wheel steps ease over 150 ms with elapsed-time cubic ease-out. Repeated
input accumulates toward a clamped target; reversal starts from visible position.
Clicks/keys, menus, navigation, resizing and scale changes cancel motion.
No idle animation redraws. Keyboard/CDP scrolling remains immediate. No new
dependency, touchpad valuators, kinetic fling, compositor or page-engine change.
Software paint cost still limits frame rate. Optional chrome remains optional.

### Test criteria

- [x] Deterministic intermediate/end positions, coalescing, reversal and bounds.
- [x] Click/keyboard/modal/navigation/geometry cancellation and painted hit alignment.
- [x] Native packaged wheel shows intermediate frames, settles and reverses at 100/200%.
- [x] Existing resource/worker/native/CDP and independent no-chrome gates pass.
- [x] Exact-commit CI/Pages, release and fresh public install/native verification.

Published v0.7.2 at 16043b103a6565d8761daf860405a0661d09a9db. Rust CI
35170104655, JSPLAN 35170104581, Pages 35170104644 and release 35302978982
pass. Public checksum/install/reinstall, worker/session/Boa, desktop/icons,
actual v0.7.1 GUI upgrade and Restart now into v0.7.2 pass. The public binary
repeats native theme/scale/toolbar/bookmark checks and intermediate wheel frames
at 100/200%. Receipts and inspected screenshots are in the 2026-09-17 log.
This adds wheel easing, not precision touchpad/fling or a frame-rate guarantee.

## F-020 : Update progress, explicit restart and compact toolbar

Stability: stable

### Dependencies

F-015 verified updates/build identity and F-013 optional host-owned browser UX.

### Properties

After an installed update, About replaces its check button with Restart now.
Download progress reports actual bytes with a percentage only for known lengths;
unknown sizes show activity, and verification remains a separate stage.
One toolbar row contains Back/Forward/Refresh/Bookmark, URL, then a right-hand
hamburger menu. The redundant title row is removed; native window titles remain.
The menu opens the same warning panel. Restart requires an explicit user action,
launches the installation path rather than the running inode, then shuts down
the old browser and workers. A spawn failure retains the old window for retry.
The loaded URL and launch preferences carry over; session state does not.
An already-installed newer version is recognized before querying GitHub.
See docs/UPDATES.md. No engine or whole-browser sandbox changes.

### Test criteria

- [x] Known/unknown/truncated/redirect body progress and unchanged transport limits.
- [x] Scaled/compact progress bar, state cleanup and right-hand menu interaction.
- [x] Host readiness gates the button and request, including keyboard/compact UI.
- [x] Installing during a pointer press cannot turn the old click into a restart.
- [x] Native replaced-path launch, failed-spawn retry, reopened page and old exit.
- [x] Existing component, worker, packaged and native/CDP regression gates pass.
- [x] Exact-commit CI/Pages, versioned release and public installer/restart verified.

Published v0.7.1 at 8d8ad5654950d7f720c427aa4a396ec93927af5f. Rust CI
35164990594, JSPLAN 35164990612, Pages 35164990646 and release 35166215804 pass.
Exact-commit checks: 1,395 debug and 1,295 selected release checks, all existing
native/CDP journeys, packaged installer/updater and theme/scale checks. Public
checksum, exact installer/reinstaller, worker/session/Boa, desktop/icons and
actual v0.7.0 upgrade/no-op recheck pass. The public binary repeats native
restart/failure-retry, toolbar/bookmark and full theme/scale acceptance. Download
progress is tested with known/unknown framed bodies and rendered scaled/compact
panels; no future public release is fabricated to claim a live GUI download.
Screenshots are inspected and receipts are in the 2026-09-16 log. Scrolling
remains fixed-step; this release adds no web-engine compatibility.

## F-019 : Navigation toolbar and local bookmarks

Stability: stable

### Dependencies

F-004 native navigation, F-013 optional Chassis chrome, F-017 themes, F-018 sizing.

### Properties

Back, Forward, Refresh and Bookmark precede the URL; the hamburger menu follows
it since v0.7.1 (F-020). Selection covers only
visible URL text. Refresh and bookmark act on the loaded page, not an unsubmitted
address. A bounded local bookmark list supports add, open, remove and pagination;
Ctrl+D toggles and Ctrl+Shift+O opens it. Host-owned atomic storage preserves
malformed data and merges concurrent-window edits. No sync, folders or new web
compatibility. See docs/BOOKMARKS.md.

### Test criteria

- [x] Text-only URL selection at 100/125/200% and history availability (v0.7.0 left menu; F-020 moves it right).
- [x] Bookmark values, limits, persistence, deduplication, removal and file failures.
- [x] Modal input isolation, compact pagination and no-chrome embedding preserved.
- [x] Packaged native back/forward/refresh and bookmark add/open/remove/restart.
- [x] Exact-commit CI/Pages, new GitHub Release and fresh public installer verified.

Published v0.7.0 at 79488712b2228ba8080c53757e3e8f70a173b3fa. Rust CI
35160797175, JSPLAN 35160797233, Pages 35160797223 and release 35161878680 pass.
Local replay: 1,389 debug and 1,289 selected release checks, all existing native/
CDP journeys and packaged installer/updater/appearance acceptance. Public bytes,
checksum, exact curl install/reinstall, worker/session/Boa, desktop/icons and
actual v0.6.0 self-update/no-op recheck pass. The public-installed executable
passes native bookmark add/open/remove/restart, loaded-page refresh/history,
URL-only selection and complete scale/theme acceptance; its dialog is visually
inspected. Receipts are in the 2026-09-16 log. No broader web/MVP claim.

## F-018 : Desktop-aware browser size

Stability: stable

### Dependencies

F-013 host/render boundaries, F-017 Settings and F-012/F-015 release distribution.

### Properties

System size follows the desktop's effective X11 DPI, with 100% fallback.
Settings offers saved manual sizes from 75% to 300%. Ctrl+plus/minus steps size;
Ctrl+0 restores System. Controls and websites scale together. Text rasterizes
at physical resolution, while layout, scroll and input stay in logical pixels.
No toolkit bindings or page-engine/JavaScript compatibility additions.
Discovery is screen-global, not per-monitor Wayland scaling; this does not
implement independent page zoom. Existing image decode/cache limits remain.

### Test Criteria

- [x] Bounded desktop DPI discovery, correct priority/fallback and legacy settings migration.
- [x] Sharp physical text, scaled rectangles/images and unchanged 1x rendering.
- [x] Logical native/CDP input and physical screenshots remain aligned at fractional/2x scales.
- [x] Native System/manual controls, shortcuts, resize, scroll and restart persistence pass.
- [x] Existing component/resource/worker/native/CDP gates pass without weakened assertions.
- [x] Exact-commit CI/Pages, versioned release and fresh public install/upgrade verified.

Published v0.6.0 at 363065dc17b51d8159d3434b74e393686bdc9d99. Rust, JSPLAN,
Pages and release workflows pass on that commit. The exact public installer,
checksums, worker/session/Boa execution and desktop/icons pass. The downloaded
binary repeats native System/manual scaling, 100/125/200% navigation, 125/200%
CDP, persistence, compact dialogs, scrolling and 4K resize; frames inspected.
Actual v0.5.0 self-update installs v0.6.0 and its selftests pass. Its additional
already-current check was blocked by GitHub's exhausted anonymous API quota,
not an installation failure; see docs/UPDATES.md and the dated evidence log.

## F-017 : System, light and dark browser appearance

Stability: stable

### Dependencies

F-013 optional chrome/host boundary and F-012/F-015 release distribution.

### Properties

Menu > Settings offers System (default), Light and Dark. Selection applies
immediately and persists in the user's XDG configuration directory. System
follows the Linux desktop settings portal's color-scheme preference, with a light
fallback when unavailable. Browser controls/dialogs are themed; website colors
and HTTP's red warning are unchanged. The host hints matching window decorations
to supporting window managers. No native toolkit or page-engine changes.

### Test Criteria

- [x] Deterministic palette, modal input and unchanged page/no-chrome pixels.
- [x] Bounded preference parsing, XDG paths, restart persistence and clear save errors.
- [x] Native packaged UI follows an isolated desktop portal, with live changes,
  explicit overrides, persistence and matching window hint.
- [x] Existing workspace, component, dependency and native/CDP gates preserved.
- [x] Exact-commit CI/Pages, versioned release and fresh public installer verified.

Local acceptance on 2026-09-16 includes 9 focused theme checks in debug/release,
independent no-chrome embedding, and the actual packaged executable controlled
through native X11 input with a private Settings portal. Published v0.5.0 at
8c457e04a8ce00dd8cf81802da35a054d3bad8ec: all exact-commit remote gates pass.
The exact public installer and actual v0.4.1 self-update pass. The fresh public
binary repeats native System/override/restart/portal-fallback acceptance, with
unchanged page pixels and HTTP red warning. Both palettes are visually inspected;
this desktop's real Dark preference is also verified without changing it.
See the dated log for checksum, build identity and command receipts.

## F-016 : Modern Butane JavaScript and embedding compatibility

Stability: in-progress

### Dependencies

F-013 component boundaries; F-008/F-010 language and worker contracts;
F-003/F-004/F-005 browser integration; F-011 for supported automation evidence.

### Properties

[JSPLAN.md](JSPLAN.md) specifies a proposed Boa-first pure-Rust reuse evaluation,
modern language and React/Vue application gates, V8-inspired performance work,
academic experiments and a separately tested V8 embedding adapter. Preserve
explicit host capabilities and restricted execution. Language, browser behavior,
API compatibility, binary ABI and performance claims require distinct evidence.
The P0/narrow P1 comparison remains isolated and pinned. The subsequent user-
authorized v0.4.0 increment adopts Boa 0.22 for actual opt-in inline page scripts,
Promise checkpoints and retained DOM events, with no original-engine fallback.
See [the process-v1 profile](docs/BOA.md) and the preserved
[initial results](docs/jsplan/RESULTS.md). Full cooperative resource control,
JSPLAN exit gates, JIT, full Test262 and browser framework compatibility remain
open. Exact release and public-installer receipts belong in the dated work log.

### Test Criteria

- [x] Pinned selected Test262 baseline and explicit full/profile denominators,
  capability exclusions and failure classifications; not full-suite conformance.
- [x] Local Boa page tests verify modern inline scripts, Promise DOM mutation,
  retained events, weak wrapper identity, callback re-entry and teardown under
  an explicit process-contained profile; original tests and limits are preserved.
- [ ] Recorded engine choice after dependency, license, host/rooting, budget and
  restricted-worker evaluation, without a fallback engine executing page code.
- [ ] Modern language, reclaimable memory and explicit bounded application profile.
- [ ] External scripts/modules, scheduling and DOM integration pass selected WPT
  and cancellation/origin/resource tests.
- [ ] Pinned unmodified React/Vue app corpus passes real interaction and long-session
  assertions with reproducible reference comparisons.
- [ ] Performance changes show controlled compile/startup/runtime/memory/latency
  evidence; any JIT has separate executable-memory and GC/deoptimization gates.
- [ ] A named pinned external consumer passes the documented V8 source API subset;
  broad binary compatibility is not inferred from this result.
- [x] The bounded Boa increment and v0.4.1 packaging follow-up satisfy release/installer policy;
  this does not complete the wider feature or its remaining adoption gates.

Published 2026-09-16: v0.4.0 at
7fe29669f25440cf82e1218e235aa745080874ff. Exact-commit Rust, JSPLAN and Pages plus
the tagged release workflow pass. The exact public curl command installs and
reinstalls the checksum-verified binary, whose real Boa worker/session tests,
desktop/icons and two native/two external CDP journeys pass. An infinite script
stops at the opcode budget, leaves readable fallback and permits onward browsing
in the same process. These authored fixtures prove the bounded integration, not
general website or framework compatibility. See the dated log for receipts.

Packaging follow-up v0.4.1 is published at
02b413f27e8ddce9b6408887317f08cd9b9b86b7 with all exact-commit gates green.
Size-oriented compilation and stripped symbols keep the complete public archive
at 7,240,140 bytes, below existing updaters' unchanged 8 MiB download ceiling.
The exact public installer, modern worker/session and native/CDP checks pass;
actual v0.3.0 and v0.4.0 binaries both upgrade themselves and then report no newer
release. Engine source, dependency graph and resource assertions are unchanged.

## F-015 : Self-updates and build identity

Stability: stable

### Dependencies

F-012 distribution, F-013 host/chrome boundary and verified Rust HTTPS transport.

### Properties

Installed Linux browsers check for stable releases, verify tag-pinned downloads
and SHA-256, test the new executable and atomically replace it without sudo or
forced restart. Automatic checks can be disabled. Menu > About and --about show
running version, compile time and commit. See docs/UPDATES.md for trust/limits.

### Test Criteria

- [x] Reject malformed versions, downgrade/prerelease, bad hashes and unsafe archives.
- [x] Real packaged executable validates and replaces a temporary installation;
  failures preserve its original bytes and concurrent updates are excluded.
- [x] About/menu input and compile metadata verified, including a native screenshot.
- [x] Existing workspace, component, worker and native/CDP regression gates pass.
- [x] Exact-commit CI/Pages and v0.2.1 public release/installer verified.

Verified 2026-09-16: v0.2.1 at 830c6ca4ede1ccd5c23d3b73ebfd89aca06729ef.
Rust run 35095863499, Pages 35095863472 and release 35096862370 passed. Exact
public curl install/reinstall, checksum, version/build metadata, worker selftest,
desktop/icons and download links passed. The native Rust updater upgraded a
temporary verified public v0.1.1 binary using actual public v0.2.1 assets, then
performed a no-op recheck. This describes the documented experimental updater,
not independent release signing or whole-browser security.

## F-014 : Hacker News desktop rendering

Stability: stable

Verified 2026-09-16: v0.3.0 at e3ac7a7b873eb080baf0fa9be61b343b06cbbcb9.
Standalone Rust Stylo, bounded same-origin CSS/images and generic tree/table
layout passed same-input desktop/full-page comparison, fresh live navigation,
local and remote CI, release and public installation. Small raster/rounding
differences and the narrow compatibility exclusions remain in docs/HACKER_NEWS.md.

### Dependencies

F-003, F-004 and a bounded subset of F-005 using the F-013 component boundaries.

### Properties

Faithful real Hacker News homepage rendering at desktop widths with scripts off:
external CSS, cascade, nested tables, compact inline typography, colors and actual
small SVG assets. Generic engine primitives, not site-specific painting. Scope
and implementation order: docs/HACKER_NEWS.md. Google and mobile fidelity deferred.

### Test Criteria

- [x] Same-input/font desktop screenshots compared against a reference browser.
- [x] Header, all story rows, wrapping, footer and scroll-adjusted links match.
- [x] Fresh live homepage and ordinary navigation verified with scripts disabled.
- [x] Focused primitive regressions and existing CI/native/CDP checks pass unchanged.
- [x] Versioned release and fresh public installer verified with accurate site notes.

Rust run 35105612390, Pages 35105612387 and release 35107029602 passed on the
exact tagged commit. Public checksum/install/reinstall, worker selftest,
desktop/icons, latest links and actual v0.2.1 self-update passed. The freshly
installed public binary repeated the real HN scroll/More/comments/story/back and
Ctrl+L journey. This does not complete F-005's broader corpus or F-007's MVP.

## F-013 : Reusable Mg component boundaries

Stability: stable

### Dependencies

The existing original interpreter, document pipeline, browser services and Linux host.

### Properties

Four workspace packages: mg-butane (JS), mg-sparkle (HTML/DOM/layout/paint),
mg-chassis (services and optional chrome) and mg-browser (platform executable).
Sparkle returns headless page surfaces with layout/hit geometry. Chassis accepts
host font data and an isolated script runtime, preserving existing browser behavior.
The production dependency direction is checked in CI. See docs/ARCHITECTURE.md.
V8/JSC, Blink/WebKit/Tauri drop-in APIs and ThermiteOS support remain future work.

### Test Criteria

- [x] Independent Butane evaluation and Sparkle bitmap-render examples build and run.
- [x] Chassis builds without chrome; public embedding tests compare page pixels and submit a real loopback form.
- [x] Original language, DOM, transport and actual worker regressions pass in the workspace.
- [x] Component dependency guard rejects upward edges and window dependencies in reusable libraries.
- [x] Native/CDP fixture journeys and exact-commit Rust/Pages checks pass for publication.
- [x] Component release assets and fresh public installer verified in v0.2.1,
  superseding the unpublished v0.2.0 package.

Local 2026-09-09 evidence: workspace debug and selected release suites passed;
independent no-chrome form navigation, default-disabled isolation, PNG and font
checks passed. Native static form/result navigation passed and frames were
inspected. All 26 native/26 CDP journeys passed locally and in Rust run 34352084178 for
a444cada85ec80e8ac0df6858b63f4f980e4c3b8. Pages run 34352084185 passed for that
commit, and public HTTPS index/installer bytes matched. The former publication
blocker was resolved on 2026-09-16: v0.2.1 includes this extraction and passed
the public installer gates recorded under F-015. No v0.2.0 tag was published.

## F-012 : v0.1 experimental preview

Stability: stable

### Dependencies

Existing engine at 4b9a5f74b09f4e3092f26d5c61d6b8a04e22a4da, F-001 distribution.
This preview does not require completing F-007, F-010 or the research evaluator.

### Properties

Linux x86_64 X11/XWayland technology preview, packaged with Rust 1.91.1 and the
locked graph. Checksum-verified user-level installer, Mg identity and desktop
launcher. Original preview archives use MIT; current source uses Apache-2.0 with
bundled dependency notices. Poor modern-web
compatibility and incomplete whole-browser isolation are explicit limitations.

### Test Criteria

- [x] Locked release tarball installs without Rust or sudo; packaged version and worker selftest pass.
- [x] Installer verifies checksums, rejects corruption, creates default/custom paths and desktop/icon files.
- [x] Existing Rust CI, AST-array regressions and 26 native/26 CDP journeys remain green on the release commit.
- [x] Exact green commit is tagged v0.1.0; normal GitHub Release contains binary tarball and checksum.
- [x] Public site/installer/downloads resolve; exact advertised curl command installs a working v0.1.0.

Verified 2026-09-08: v0.1.0 tags 392867f5f059cc34162360b5a63c4f16b62d6fcc.
Rust CI34227154913, Pages34227154800 and release34227857264 succeeded. Fresh
public install passed checksum/version/worker/desktop/icon checks. This stability
describes preview distribution only. Formal F-007 criteria and the unfinished
Google first-result journey are unchanged. Full evidence is in the dated log.

## F-001 : Project foundation and website

Stability: stable

### Properties

Public source, readable MVP plan, and an HTTPS project site at mgbrowser.org. Every main push validates and deploys the site; public readiness follows this file and the latest work-log date.

### Test Criteria

- [x] GitHub repository exists and matches the published commit.
- [x] Pages deploys the intended commit; live HTML matches local bytes.
- [x] Custom-domain TLS is valid and HTTPS enforcement is enabled.
- [x] Generated website status passes the Rust tool's `--check`.

Evidence: 2026-09-07 initial Pages run 34120255103 succeeded for aed2386; live HTTPS HTML matched byte-for-byte, certificate approved and HTTPS enforcement confirmed by API. See daily log for subsequent publication checks. Visual browser QA is not yet available.

## F-002 : Agent continuity and reusable skills

Stability: stable

### Properties

Canonical AGENTS.md, measurable feature contracts, bounded task queue, daily logs, compact memory/skills indexes, and reusable skill procedures. Automatic skill selection follows the catalog and does not authorize unrelated actions.

### Test Criteria

- [x] Instructions, indexes, referenced skills and first daily log exist.
- [x] Skill frontmatter validates; harness instruction aliases resolve to AGENTS.md.

Evidence: 2026-09-07 both skills passed quick_validate.py; instruction alias targets checked; website sync passed locally and in CI. Portable catalog-based selection is configured; individual agent-client autodiscovery has not been tested.

## F-003 : Local document rendering

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

## F-004 : Static web navigation

Stability: in-progress

### Dependencies

F-003.

### Properties

HTTP(S), relative links, address bar, reload, back/forward, cancellation, scroll, and visible load/errors; invalid TLS never silently succeeds.

Explicit http:// URLs are supported. Unencrypted loaded pages use a red title/address
strip with an "HTTP: Not secure" label. The indication follows the committed
response URL, including redirects, not unsubmitted location edits. Ctrl+L selects
the address from the page or a form field; typing replaces it and Enter navigates.
The desktop window manager controls the outer window-decoration color.

TLS uses rustls-rustcrypto explicitly under the research-only policy in docs/DEPENDENCIES.md. Native crypto fallback is prohibited.

### Test Criteria

- [ ] Local-server tests cover redirects, failures, request/body/time limits and cancellation.
- [ ] Manual Linux navigation and keyboard-control scenarios pass.
- [x] Local TLS handshake fixtures cover trusted/untrusted certificates and hostname mismatch with the selected provider.

Evidence: 15 transport tests passed for framing, verified TLS/rejection cases, redirects, request/body/time limits and in-memory cookie scope. A real local HTTP form submission/result click completed in the native window. Async navigation ignores stale results and caps requests at two, but lacks a transport cancellation API. Full manual keyboard/history acceptance remains open.

## F-005 : Styled text and images

Stability: in-progress

### Dependencies

F-003.

### Properties

The documented static MVP HTML/CSS subset supports cascade/inheritance, block and inline flow, wrapped UTF-8 text, box styling and PNG images.

Font parsing, shaping and rasterization use Rust implementations without native font bindings. Image codecs are explicitly enabled: PNG, first-frame GIF and a restricted static SVG shape/path subset. Unsupported/broken images retain alt text and a placeholder without preventing document rendering; no C decoder fallback.

### Test Criteria

- [ ] All required versioned DOM/layout/pixel fixtures pass.
- [ ] Unsupported syntax and malformed input produce bounded, documented behavior.
- [ ] Unsupported/corrupt image fixtures display a placeholder and preserve surrounding document layout and alt text.
- [ ] Resolved font/image features contain no native implementations or implicit codec fallback.

Current increment: rustybuzz/fontdue regular/bold faces, standalone Rust Stylo
computed CSS, bounded generic table/inline layout and same-origin PNG/GIF/SVG
resources under F-014. Focused tests do not satisfy the full planned fixture
corpus or general CSS compatibility. F-005 remains in progress.

## F-006 : Reproducible autoresearch evaluator

Stability: planned

### Dependencies

F-003 and deterministic baseline corpus.

### Properties

A Rust evaluator executes bounded experiments with immutable evaluation inputs, independent correctness/performance gates, and machine-readable evidence. Humans and agents use the same contribution contract.

### Test Criteria

- [ ] Another contributor reproduces a baseline and candidate report from a clean checkout.
- [ ] Known wrong output, timeout, crash and evaluator-tampering candidates fail gates.
- [ ] Reports contain the provenance and metrics specified in docs/AUTORESEARCH.md.

## F-007 : Linux MVP release

Stability: planned

### Dependencies

F-004, F-005, F-006.

### Properties

Installable experimental static browser with documented limitations and no claims of modern JavaScript or hardened general-purpose browsing.

### Test Criteria

- [ ] Clean-machine install, full required corpus, manual browser scenarios and resource limits pass.
- [ ] Dependency/native-code and license inventory is reviewed.
- [ ] Release artifacts, provenance and known limitations are published.

## F-008 : JavaScript and broader compatibility

Stability: in-progress

### Dependencies

Reviewed Rust language/runtime, DOM integration and execution-boundary roadmap;
required for F-010 before a complete static MVP release.

### Properties

Boa-backed Rust page execution with bounded DOM capabilities and opt-in
--enable-scripts. Inline scripts, Promise checkpoints and later click/submit
handlers share a retained realm and produce native/CDP-visible controls without
replay. A restricted Linux x86_64 child denies new file/network/process access;
docs/BOA.md specifies cumulative opcode/source/job and worker-allocation limits
plus unchanged CPU/address-space/wall/lifetime containment. Unsupported platforms
fail closed. This is not general ECMAScript/browser conformance. The original
evaluator remains an explicit test baseline, not a production fallback. Broader
events, timers, external scripts and modules remain unsupported.

Live Google search currently requires this work. Develop against bounded local language and DOM fixtures; retain the original Google journey as the end-to-end acceptance gate.

The checked criteria and engineering history below preserve the original
evaluator's verified baseline, not Boa resource/conformance claims. The explicit
`legacy-test-engine` research lane retains those assertions and limits; it is not
a production fallback. Current Boa acceptance is tracked in F-016 and docs/BOA.md.

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
- [x] Empty-only arguments snapshots materialize on first binding read with preserved identity, scope and real admission; independent semantics/resources and frozen worker/native/CDP 8,500-call form journeys pass without raising limits.
- [x] Fresh user-function default prototypes materialize on first value read with real metadata, unique identity and original constructor preserved; independent semantics/resources and frozen worker/native/CDP 4,800-function form journeys pass without raising limits.
- [x] Bounded ES5-shaped Function.bind preserves receiver/prefix ordering, construction/instance delegation and restricted metadata, with independent semantics/resources and frozen actual-worker/native/CDP bound-callback forms.
- [x] Bounded nullish member diagnostics identify the failing operation/base and allowlisted property without changing page-visible exceptions, evaluation order, resource reports or worker/native/CDP behavior.
- [x] Non-member native receivers follow the supported call semantics; immediate producer context distinguishes property presence/getter/Host/call outcomes without extra evaluations, private data or changed resource reports from instrumentation.
- [x] The seven Array callback methods preserve generic indexed traversal, callback/result semantics and cumulative bounds, including borrowed methods on DOM collection snapshots, with independent worker/native/CDP acceptance under docs/ARRAY_CALLBACKS.md.
- [x] Five core constructor backlinks, genuine String/Number/Boolean prototype payloads and direct/bound Number/Boolean construction pass independent semantic/resource and worker/native/CDP acceptance under docs/CORE_INTRINSICS.md.
- [x] Static operator tags eliminate actual per-operator buffers with unchanged grammar, layouts and limits; independent allocation/semantic/resource and frozen actual-worker/native/CDP form gates pass under docs/STATIC_OPERATORS.md.
- [x] Bounded Object.create descriptor definitions and ordinary getter/setter behavior pass the adopted independent semantic/resource, actual-worker and native/CDP local gates in docs/OBJECT_CREATE.md; exact remote publication remains pending.
- [x] Retained restricted page realms deliver real later click/submit events with cancellation, stable node identities, versioned edits, cumulative limits and native/external CDP handler-required journeys under docs/PAGE_SESSIONS.md.
- [x] Actual worker probes verify denied capabilities, resource termination, bounded pipe transfer and owned-child cleanup.
- [x] Native/CDP local journeys use a script-created form; script navigation and loop-error recovery are verified in a real window.
- [x] Source/projection rejection preserves original fallback and discards proposed navigation; stale completions cannot replace the active page.
- [x] GitHub CI reproduces the language/DOM/worker tests and scripted native/CDP fixture journeys for the published implementation.
- [ ] Parent-brokered external scripts, general UI event dispatch, timers and longer-lived platform behavior complement the bounded retained click/submit subset with independent fixtures.
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

Concat remote acceptance: bed80a1 passed Rust CI 34164731519 with all 604 debug tests, 506 selected release checks and all three native/CDP journey steps (15 native/15 CDP destinations). Pages 34164731520 deployed exact matching HTTPS HTML with an approved apex certificate, HTTPS enforcement and HTTP-to-HTTPS redirect. This verifies the published concat increment; actual Google results/first-destination acceptance remains incomplete.

Empty-arguments follow-up: a real nondeletable binding retains a private callee identity for empty actual lists; first read creates the existing branded snapshot once, while successful replacement cancels pending storage. Nonempty lists and real payload copies are unchanged. Unread calls pay 265 Runtime bytes; first read pays the remaining 262 and the existing property-write fuel step. Failed admission remains fatal, keeps prior effects/admitted orphan storage and publishes no partial value. Binding is 80 bytes, Environment 48 and Object 112, covered by existing allowances. All 29 independent semantics pass on old and new runtimes, with 14 resource and seven private groups. The frozen 8,500-call page now creates its actual controls at 2,317,555 accepted bytes; observed snapshots still exhaust the unchanged cap. All 659 debug tests, 561 selected release checks and three exact CI journey steps pass locally (16 native/16 external CDP destinations); query/destination frames inspected. No old assertion, dependency, TLS, worker, CDP or limit expansion. Exact publication evidence follows in the daily log; this is not Google acceptance.

Empty-arguments remote acceptance: 849b0fc passed Rust CI 34166732640 with all 659 debug tests, 561 selected release checks and all three native/CDP journey steps (16 native/16 CDP destinations). Pages 34166732594 deployed exact matching HTTPS HTML with an approved apex certificate, HTTPS enforcement and HTTP-to-HTTPS redirect. This verifies the published storage increment, not Google's results/first-destination acceptance.

Default-prototype follow-up: fresh user functions keep their paid property bag/prototype metadata (265 Runtime bytes), while only the unused unique default object/constructor backlink (267 bytes) waits for an actual value read. Metadata-only operations do not force it; successful paid own replacement cancels pending storage. Inherited reads resolve the owner, old defaults retain identity, real copies and existing construction/instanceof semantics remain intact. Original fuel steps stay with actual work; failed admission retains charged orphan storage without a partial published value. Function32/Code56/Object112/Property64 fit existing allowances, Bootstrap25,854 unchanged. All 30 independent semantic groups pass on old and new runtimes, with 17 resource and ten private groups. Three initial new-test assumptions about absent Object/Array native backlinks were corrected before candidate testing; no native-backlink feature or old assertion was changed. The frozen 4,800-function page now creates real controls at 3,232,783 accepted bytes; observed defaults still exhaust the unchanged cap. All 721 debug tests, 623 selected release checks and three exact CI journey steps pass locally (17 native/17 external CDP destinations); query/destination frames inspected. No dependency, TLS, worker, CDP or limit expansion. Exact publication evidence follows in the daily log; this is not Google acceptance.

Default-prototype remote acceptance: 1f1cdb0 passed Rust CI 34173224049 with 721 debug tests, 623 selected release checks and 17 native/17 external CDP destinations. Pages 34173224055 deployed matching HTTPS HTML with an approved apex certificate, HTTPS enforcement and HTTP-to-HTTPS redirect. This verifies the published storage increment, not actual Google results or first-destination acceptance.

Bound-callable follow-up: original Function.bind retains receiver and ordered prefixes behind existing function identities, including rebinding, target construction/instance delegation and restricted caller/arguments metadata. A bounded iterative chain prepays one output vector, charges actual forwarded copies and unwinds conceptual entries on every outcome. Bound metadata 160, property bag 128 and prefix 64/slot cover real storage; ordinary 128 remains sufficient. The real builtin adds 145 Bootstrap bytes to 25,999; only the three old Bootstrap pins changed, not the 4 MiB cap or other assertions. Independent 36 semantic/16 resource/ten private groups and 32 DOM/46 worker groups pass. The frozen previously failing page now creates its actual form at 104,479 accepted bytes with no error/rejection. All 788 debug tests, 690 selected release checks and three exact CI journey steps pass locally, with 18 native/18 external CDP destinations; query/destination frames inspected. No dependency, TLS, worker, CDP command or execution-limit expansion. Exact publication evidence follows in the daily log; this is not Google acceptance.

Bound-callable remote acceptance: 0c48eeb passed Rust CI 34176183048 with 788 debug tests, 690 selected release checks and 18 native/18 external CDP destinations. Pages 34176182971 deployed matching HTTPS HTML with an approved apex certificate, HTTPS enforcement and HTTP-to-HTTPS redirect. This verifies the published bind increment, not actual Google results or first-destination acceptance.

Remote evidence: implementation b9cde9e passed Rust CI 34133745718 with 88 tests and all three native/CDP journey steps. Pages 34133745636 deployed matching HTTPS content; the certificate is approved, HTTPS enforced and HTTP redirects to HTTPS. Broader JavaScript compatibility remains in-progress.

Follow-up remote evidence: 8603918 passed Rust CI 34136335877 with all 148 tests and every native/CDP journey, including dynamically compiled forms. Pages 34136335825 deployed exact matching HTTPS HTML with the approved apex certificate and HTTPS enforcement. This closes the language increment's publication gate, not F-008/F-010 compatibility.

Nullish-diagnostic follow-up: fixed four-byte fault context records the originating reference operation, null/undefined base and standard key or redacted category. Catch-visible strings, key evaluation/coercion order, catch/finally/fatal propagation and frozen allocation/fuel checkpoints stay unchanged. Independent 32 semantic/15 resource/seven private groups, 34 DOM/49 worker groups, all 848 debug tests and 749 selected release checks pass. The unchanged two-script fixture still creates its form at exactly 57,479 accepted bytes and completes actual-worker/native/CDP checks; all 19 native/19 external CDP destinations pass, with query/destination frames inspected. The first aggregate run found a two-script fixture mistakenly added to a one-script test list; a dedicated exact-two-script check corrected only that new integration assertion. No production, fixture, old assertion, dependency, permission or cap change was needed. The live search failure now identifies resolve-call-target on undefined with a redacted string key; it does not identify the producer or missing capability. Google still has no actionable results. Publication evidence is recorded separately in the daily log.

Retained-event follow-up: the original restricted realm and append-only DOM arena survive startup. Real click/submit callbacks preserve closure state, capture/bubble order, cancellation and identity; versioned edits cannot erase newer typing. Parent-validated post-handler snapshots determine actual requests. Typed immediate admission failures reach CDP as -32000; successful event projection invalidates node IDs without fake page-load events. Cumulative active/runtime/process limits remain, with explicitly new 300-second lifetime, 64-transaction and 32 MiB wire bounds. Browser-host setup adds a measured 794 Runtime bytes; the legacy diagnostic checkpoint is now 58,273, without changed raw Runtime checkpoints or a raised cap. All 905 debug tests, 792 selected release checks and four exact CI journey steps pass locally (20 native/20 external CDP). The frozen handler-required journey observes two cancellations, moved Unicode input, retained proof and the handler-updated destination, with zero trap requests. Native/CDP frames inspected; actual worker ownership/framing/cleanup tests pass. See docs/PAGE_SESSIONS.md and the daily log for exact scope, failures and publication evidence. This is not Google or full CDP compatibility.

Receiver/producer follow-up: non-member native calls receive undefined; ordinary non-strict user functions retain their global substitution, and shared EventTarget listener methods apply their own nullish-to-Window rule. Immediate producer context records only the existing evaluation/traversal, with binding/expression boundaries and fixed redacted keys. MemberContext is seven bytes; result layouts and frozen instrumentation-only allocation/fuel reports remain unchanged. All 971 debug tests, 858 selected release checks and four exact CI journey steps pass locally (21 native/21 external CDP). The frozen old-build failure now creates its real form at 56,597 accepted bytes and reaches actual worker/native/CDP destinations; frames inspected. Existing startup/event/diagnostic assertions remain, with two new invalid test expectations corrected using independent baseline evidence. One live attempt still has no results; binding attribution is not a root-cause diagnosis. No dependency, TLS, worker permission, CDP command or execution-cap expansion. Exact failures, evidence and publication status are recorded in the daily log.

Array-callback follow-up: forEach/map/filter/some/every/reduce/reduceRight preserve current indexed presence/Get, holes/inherited values, captured length, live in-range mutation, receiver/argument identity, short circuits and reductions. Borrowed methods use a narrow, default-failing Host presence hook for existing canonical DOM collection snapshots, without new collection methods or live NodeLists. Independent 35 semantic/19 resource/nine private groups and actual DOM/session/worker checks pass. The frozen old-build failure now creates its real form at 93,783 accepted bytes and completes native/CDP journeys. Real reduceRight metadata adds exactly 156 Bootstrap bytes; eleven old checkpoint files retain their original baselines plus that allowance, with other phases, copy controls and fuel-loop counts unchanged. All 1,041 debug tests, 928 selected release checks and four exact CI journey steps pass locally (22 native/22 external CDP), excluding repeated child summaries. Frames inspected; retained requests are 2/2 with zero traps; processes reaped. One live attempt still yields no Google results. Scope, failures and separate publication receipts are in docs/ARRAY_CALLBACKS.md and the daily log; no caps, dependencies, permissions or protocol commands changed.

Core-intrinsic follow-up: five real constructor backlinks and genuine String/Number/Boolean prototype payloads preserve intrinsic identities, reflection and branded rejection. Direct/bound Number/Boolean construction preserves conversion order, omitted/undefined distinctions, truthiness and fatal limits. Independent 33 semantic/17 resource/eight private groups plus DOM/later-event/actual-worker checks pass; the frozen old-build failure now creates its real form at 84,741 accepted bytes. Measured backlinks add 725 Bootstrap bytes; thirteen old checkpoint files retain historical allowances, all other phases/copies/fuel counts and caps. All 1,104 debug/991 selected release tests and 23 native/23 CDP journeys pass, excluding two repeated child summaries per profile. The first journey attempt exposed a log-creation race; separately tested readiness guards fix the harness without deadline or browser changes. Frames inspected, retained requests 2/2 with zero traps, owned processes reaped. One live Google attempt still renders no results, first rejecting Runtime 131 after 4,194,282 accepted bytes. No root-cause claim about the prior undefined binding or new live stage is made. See docs/CORE_INTRINSICS.md and the daily log for both original attempts, limits and separate publication evidence.

Object.create descriptor follow-up: ordered descriptor conversion, typed-key snapshots, data flags and genuine getter/setter storage preserve original receivers, ordinary errors and cumulative limits. All 1,193 full-debug tests across 51 targets and 1,080 selected release tests across 40 targets pass locally, including 28 semantic, 10 resource and seven private storage groups. The public eight-case measure_object_create example requires descriptor support and checks actual requested allocation/copy/drop behavior; it is not an RSS measurement or general research executor. The unchanged authored worker completes one script with no errors or rejection at 75,451 accepted bytes: Bootstrap 26,880 + Source 1,861 + Ast 33,702 + FunctionCode 256 + Runtime 12,752. All 25 native/25 external CDP journeys pass locally; actual query, hidden source, unnamed submit, local result and destination frames were inspected. Retained checks record two searches, two destinations and zero traps. Existing caps, dependencies and worker authority remain unchanged. The subsequent Google attempt still has no results; exact remote publication is pending. See docs/OBJECT_CREATE.md and the daily log for scope and evidence.

## F-009 : Experimental Rust dependency foundation

Stability: stable

### Properties

Cargo configuration pins rustls-rustcrypto and explicitly selects Rust font/image implementations. image defaults are disabled with PNG as the only codec. The library constructs TLS configuration with an explicit provider and caller-supplied roots. A CI denylist guards known native backends; dependency source review remains required. This is not yet an HTTPS or document-rendering implementation.

### Test Criteria

- [x] Locked dependencies compile and TLS client construction succeeds with the explicit provider.
- [x] PNG round-trip succeeds, a disabled codec is rejected, and Rust font APIs reject invalid font input.
- [x] Active Linux normal/build dependency graph passes the native-backend regression guard.
- [x] GitHub CI reproduces the locked build and dependency checks.

Evidence: local cargo test --locked passed all three initial smoke tests on 2026-09-07; GitHub Rust run 34121634459 reproduced formatting, dependency guard and smoke tests for 645d30b. docs/DEPENDENCIES.md records the build/dependency review. Stable refers to this dependency configuration contract, not production TLS readiness. Later handshake and font-rendering evidence is recorded under F-003/F-004 and the daily log.

## F-010 : Google search to first destination

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

Post-empty-arguments checkpoint: homepage HTTP 200 retains 26 items/one form, three completed scripts/seven errors and no allocation rejection (2,505,994 accepted bytes). Actual served-form submission works with verified TLS and ordinary cookies. Search HTTP 200 remains blank with zero items/forms and two completed scripts/three errors repeating a Runtime rejection: 4,194,254 accepted, 128 requested, 4,194,304 limit. Search Ast is 2,438,297, FunctionCode 22,016 and Runtime 1,575,703. Exit 2/JOURNEY_INCOMPLETE; blank frame inspected, no result/destination. The authored storage improvement has not completed any new live journey stage. Changing responses are not a controlled benchmark. Continue independently measured storage/ownership work and separately designed browser capabilities, without raised limits or live-source adaptation.

Post-default-prototype checkpoint: homepage HTTP 200 retains 26 items/one form, three completed scripts/seven errors and no allocation rejection (2,428,284 accepted bytes). Actual served-form submission works with verified TLS and ordinary cookies. Search HTTP 200 remains blank with zero items/forms and two completed scripts/three errors: first a non-callable-value TypeError, then Source 26,999 rejected after 4,187,629 accepted bytes, repeated by the next script. Search Ast is 2,438,297, FunctionCode25,216, Runtime1,564,706 and regex1,172 combined. Exit 2/JOURNEY_INCOMPLETE; blank frame inspected, no result/destination or newly completed live stage. The generic error does not identify the missing callable, and changing responses are not a controlled benchmark. Next is independent language/builtin coverage and measured storage ownership, with no raised limits or live-source adaptation.

Post-bind checkpoint: homepage HTTP 200 retains 26 items/one form, three completed scripts/seven errors and no allocation rejection (2,429,821 accepted bytes). The actual served form submits through verified TLS and ordinary cookies. Search HTTP 200 remains blank with zero items/forms and two completed scripts/three errors: first property access on null or undefined, then Source 26,794 rejected after 4,191,219 accepted bytes, repeated by the next script. Search phases are Bootstrap 25,999, Source 132,384, Ast 2,438,297, FunctionCode 25,376, Runtime 1,567,991 and regex 1,172 combined. Exit 2/JOURNEY_INCOMPLETE; blank frame inspected, no result/destination or new completed live stage. A changed diagnostic is not proof of a newly completed stage or a diagnosis of missing behavior; no live-source inspection/adaptation/retry or increased limit.

Post-retained-event checkpoint: homepage HTTP 200 has 26 items/one form, five completed scripts/five errors and no allocation rejection (2,429,689 accepted at startup). Two actual later activations complete in the same realm, reaching 2,432,977 accepted bytes, and the real form submits with verified TLS and ordinary cookies. Search HTTP 200 remains blank with zero items/forms and two completed scripts/three errors: the same undefined method-call target, then Source 27,142 rejected after 4,192,013 accepted bytes against 4,194,304, repeated. Search Runtime is 1,568,785, exactly the previous diagnostic checkpoint plus the measured 794-byte host setup. Exit 2/JOURNEY_INCOMPLETE; blank frame inspected, no actual result/destination or new required live stage. The fixed local handler-dependent journey passes; changing live responses are not a controlled comparison or evidence that a particular missing API is the cause.

Post-receiver/producer checkpoint: homepage HTTP 200 retains 26 items/one form, five completed scripts/five errors and no allocation rejection at 2,429,450 startup bytes. Two retained activations complete and the actual form submits. Search HTTP 200 remains blank with zero items/forms, two completed scripts/three errors. The first is resolve-call-target on undefined with a redacted string key and producer kind=binding; that identifies the immediate binding read, not its underlying cause. Later Source 26,963 is rejected after 4,192,013 accepted bytes against 4,194,304, repeated by the next script. Exit 2; blank frame inspected, no actual result/destination or new required live stage. No live-source inspection, adaptation or retry; limits, TLS and isolation remain unchanged.

Post-array-callback checkpoint: homepage HTTP 200 retains 26 items/one form, five completed scripts/five errors and 2,429,617 accepted startup bytes. Two retained activations complete and the real form submits. Search HTTP 200 remains blank with zero items/forms, two completed scripts/three errors: the same undefined method-call target read from a binding, then Source 26,636 rejected after 4,192,169 accepted bytes against 4,194,304, repeated. Exit 2; blank frame inspected, no first result/destination or newly completed live stage. The later admission failure does not diagnose the earlier TypeError; changing responses are not a controlled benchmark. No live-source inspection, adaptation, retry or raised limits.

Post-core-intrinsic checkpoint: homepage HTTP 200 retains 26 items/one form, five completed scripts/five errors and 2,430,051 startup bytes. Two retained activations complete and the actual form submits. Search HTTP 200 still renders zero items/forms with two completed scripts/three errors, now first rejecting Runtime 131 bytes after 4,194,282 accepted against 4,194,304 and latching that failure into later scripts. The earlier undefined-binding error is absent from this response; changing served inputs are not a controlled comparison or proof of cause. Exit 2, blank frame inspected, no actual result/destination or new required live stage. One bounded attempt only; no source inspection/adaptation, impersonation, alternate service or raised limit.

Post-static-operator F-010 checkpoint: homepage HTTP 200 retains 26 items/one form, five completed scripts/five errors and 2,382,576 startup bytes without rejection. Two retained activations complete and the actual form submits. Search HTTP 200 remains blank with zero items/forms, two completed scripts/three errors: first unsupported Object.create property descriptors, then Ast 378,301 rejected after 4,128,478 accepted against 4,194,304, repeated by the next script. Exit 2; blank frame inspected, no first result/destination or new required stage. One bounded attempt, no source inspection/adaptation, retry or raised cap. Controlled local storage evidence is separate: 23 allocator cases, 1,145 debug/1,032 selected release tests and 24 native/24 external CDP journeys pass. The frozen operator-rich form completes at 4,053,180 accepted bytes. See docs/STATIC_OPERATORS.md and the daily log.

Post-Object.create F-010 checkpoint: homepage HTTP 200 retains 26 items/one form, five completed scripts/five errors and 2,383,102 accepted startup bytes without rejection. Two retained activations complete and the actual served form submits. Search HTTP 200 remains blank with zero items/forms, three completed scripts/two errors: first Ast 377,733 rejected after 4,135,770 accepted bytes against 4,194,304, then the repeated latch. The unsupported descriptor error is absent from this response; changing served inputs are not a controlled comparison or proof of cause. This single bounded attempt exits 2; its blank frame was inspected, with no result, destination or newly completed required stage. No live-source inspection, adaptation, retry or cap change occurred. Local descriptor acceptance is separate from the still-incomplete Google goal and pending remote publication.

## F-011 : Chrome DevTools Protocol automation

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
