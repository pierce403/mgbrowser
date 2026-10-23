# mgbrowser

<img src="assets/mgbrowser.svg" width="112" alt="Burning magnesium Mg tile">

## v0.9.15 Experimental Preview

v0.9.15 adds physical `justify-content:left/right` placement to the
existing bounded Rust flex/grid adapter. Reversing flex children does not reverse
the physical edge; columns pack at the top and grid ignores irrelevant flex
direction. Safe physical alignment in reversed flex containers is explicitly
unsupported until its overflow fallback can be represented correctly. RTL,
vertical writing and full CSS Box Alignment remain unfinished.
[Scoped contract](docs/PHYSICAL_JUSTIFY.md) and [release notes](docs/RELEASE-v0.9.15.md).

Full local debug/release workspace regressions, optional-chrome embedding,
dependency/tool checks and all 364 frozen JSPLAN outcomes pass. Fourteen private
and nineteen public geometry/pixel/hit controls retain their authored pre-feature
failure evidence. The independently reviewed complete static selection improves
from 317 to **319 PASS / 630 FAIL / 62 UNSUPPORTED of 1,011**, preserving every
old pass and runnable case. Only two original column reftests gain a zero-pixel
match; five other initially targeted cases still fail. The manual ratchet now
protects those two additional passes. Corpus, scorer and resource limits are
unchanged. The separate Window lane remains 15/16 passing files and 77/79 final
subtests, with its two genuine failures retained.

The normal package, fixture installer/updater, all thirteen existing native
helpers and the separate physical-layout native/CDP proof pass. Both frozen
worker-control suites pass through direct workers and the real Browser/HTTP host.
Published as [v0.9.15](https://github.com/pierce403/mgbrowser/releases/tag/v0.9.15)
at `7e13cca45c525921a5072a00eab658cf2782d4e9`. Exact-source CI/site and the
normal tagged release pass. Public install/reinstall, worker/session and
desktop/icon checks, actual v0.9.14 update/no-op, both frozen public-worker
suites, all thirteen existing native helpers and the separate public-binary
physical-layout native/CDP proof pass. See [the public closeout](memory/logs/2026-10-23.md#v0915-public-release-closeout)
for the separate delivery receipts. The installer selects the
[latest actually published release](https://github.com/pierce403/mgbrowser/releases/latest).
This bounded release does not complete F-005/F-016/F-026 or the formal MVP.

### Previous v0.9.14 increment

v0.9.14 adds parser-backed modern `element.style` named reads/writes,
`cssText` and callable `getPropertyValue`, using the existing locked Rust Stylo
grammar and serializer. The DOM style attribute remains authoritative; this is
declared-value support, not computed style or the full style interface.
[Scoped contract](docs/CSSOM.md) and [release notes](docs/RELEASE-v0.9.14.md).

Focused local debug checks pass nine declaration, twenty public binding, nine
private quota/fatal and three fresh restricted-worker controls, plus a real
retained-worker Init/lifecycle/click control. The separate frozen JSPLAN suite
retains all 364 outcomes. Full local debug/release all-target regressions,
optional-UX embedding and existing tooling checks pass. The independently
reviewed complete local Window measurement reports
15/16 passing files and 77/79 passing actual final subtests, with two assertion
failures retained in `flex-valid.html`. The manually strengthened Window ratchet
protects the fifteen passing files; no assertion, input or resource limit is
relaxed. Authored controls add no upstream denominator or passes.

Published as [v0.9.14](https://github.com/pierce403/mgbrowser/releases/tag/v0.9.14)
at `dfddb067b92bc56a06e721a99b183f56225f6177`. Exact-source Rust, JSPLAN,
both website score lanes and publication checks pass. The normal release,
checksum-verified public install/reinstall, delivered CSSOM/worker/session and
desktop/icon checks, actual v0.9.13 update/no-op, both frozen public-worker
suites and all thirteen existing native/CDP helpers pass. See the
[public closeout](memory/logs/2026-10-23.md#v0914-public-release-closeout)
for separate delivery evidence. The installer selects the
[latest actually published release](https://github.com/pierce403/mgbrowser/releases/latest).
Public style methods beyond `getPropertyValue`, indexed/enumerated interfaces,
constructors/prototypes, generic expandos, computed/rule styles and
`Element.style` forwarding remain deferred. F-016/F-026 and MVP remain open.

### Previous v0.9.13 increment

This release ships the fixed trusted `setup({output:false})` contributor mode
without injected JavaScript, changed harness/vendor bytes or larger resource
allowances. Five new authored controls and all sixteen original controls meet
their frozen expectations in both normal release-mode worker and separate real
Browser paths, preserving assertion failures, genuine timeout and native errors.
They add **zero upstream WPT passes**. Full local Rust and frozen JSPLAN gates
pass; the fresh complete static score remains 317/632/62 of 1,011. The packaged
installer/updater, all thirteen native helpers and both frozen worker suites
also pass. Published as [v0.9.13](https://github.com/pierce403/mgbrowser/releases/tag/v0.9.13)
at `948e86206c23c0310d17dc286ac5107a15f1f434`. Exact-source CI/site, tagged
release, public install/reinstall, actual v0.9.12 upgrade/no-op, both five-new
and sixteen-original public-worker suites and all thirteen existing public-native
helpers pass. Public closeout log: [dated log](memory/logs/2026-10-22.md#v0913-public-release-closeout). See
[implementation and evidence](docs/WPT_HARNESS_OUTPUT_STATUS.md).
The website installer delivers the latest published release. See the
[v0.9.13 release notes and limitations](docs/RELEASE-v0.9.13.md).

### Previous v0.9.12 increment

This increment adds bounded, default-off typed Window testharness observation
for contributor test hosts. Results retain raw statuses, weak identities and
lossless UTF-16; completion waits for actual queued error/rejection settlement.
Only validated, charged parent records are accepted. Ordinary browsing cannot
enable the observer through a URL, markup or CDP.
[Contract](docs/WPT_OBSERVER.md) and [implementation scope](docs/WPT_OBSERVER_STATUS.md).

All sixteen frozen authored controls pass locally through both the normal
release-mode restricted worker and a separate real Browser/resource-broker host.
They add **zero upstream WPT passes**. Final-version regressions, fresh full
scoring, the packaged installer and all thirteen native helpers pass locally.
Published as [v0.9.12](https://github.com/pierce403/mgbrowser/releases/tag/v0.9.12)
at `1d14f8b57e9a6743c32be4fc2aafa3811d121837`. Exact-commit CI/site and tagged
publication pass. Public install/reinstall, actual v0.9.11 upgrade/no-op, all
sixteen public-executable observer controls and all thirteen existing native
helpers also pass. Fresh WPT scoring reproduces 317 PASS, 632 FAIL and
62 UNSUPPORTED / 1,011. See the [dated log](memory/logs/2026-10-22.md).
The installer selects the [latest actually published release](https://github.com/pierce403/mgbrowser/releases/latest).
See the [v0.9.12 release notes and limitations](docs/RELEASE-v0.9.12.md).

### Previous v0.9.11 increment

This increment queues DOMContentLoaded and Window load as two genuine later
tasks after pre-parsed/preloaded classic sources. Readiness, event propagation,
checkpoints and mandatory admissions follow the shared existing work ledger.
Browser loading waits for actual load completion, not arbitrary queue idle;
typed bounded startup outcomes preserve initial errors across later clean work.
Strict modern v5 progress separates CDP commit, DCL, load and loading stopped.
[Lifecycle contract and limitations](docs/DOCUMENT_LIFECYCLE.md).

Published as [v0.9.11](https://github.com/pierce403/mgbrowser/releases/tag/v0.9.11)
at `ba28b9c5471382bd8b1c254c1add6f4bd8734193`. Full local regressions,
exact-source CI/site and tagged packaging, public install/reinstall, actual
v0.9.10 self-upgrade/no-op and all thirteen packaged/public native helpers pass.
Receipts are in the [dated work log](memory/logs/2026-10-22.md).
The v0.9.11 checksum-verified public installer passed. This increment is not a
streaming parser, general event loop, new upstream pass or full WPT harness.
See the [release notes and limitations](docs/RELEASE-v0.9.11.md).

### Previous v0.9.10 increment

This release adds genuine later Window self-messaging: `postMessage`,
`message`/`messageerror` listeners and handler slots, cloned data, trusted origins
and noncancelable native events. Primitives, ordinary objects, arrays, sparse
holes, cycles and aliases are supported. Other builtins and nonempty transfer
lists fail explicitly; this is not full structured clone or cross-context
messaging. Native DOMException preserves Error identity and private branding.
[Messaging contract and acceptance](docs/WINDOW_MESSAGES.md).

Published as [v0.9.10](https://github.com/pierce403/mgbrowser/releases/tag/v0.9.10)
at `6ee10072217d6ccc91a67b31486b1e10cbeb5de6`. Local full debug/release
regressions, restricted workers, fresh complete static WPT, exact-commit CI/site
and tagged packaging pass. The v0.9.10 public installer delivered that release;
fresh install,
reinstall, actual v0.9.9 self-upgrade/no-op and all twelve packaged/public native
acceptance helpers pass. Receipts are recorded in the
[dated work log](memory/logs/2026-10-22.md).
See the [release notes and limitations](docs/RELEASE-v0.9.10.md).

### Previous v0.9.9 increment

The v0.9.9 increment adds queued Window `unhandledrejection` and
`rejectionhandled` events with real Promise/reason identity, traced pending
payloads and weak post-notification membership. Frozen batches run in later
parent-issued tasks under the existing shared limits. Cancellation and later
handling never retract a notification's host diagnostic. Muted sources retain
no rejected payload; a persistent notice explicitly reports incomplete
observation. [Rejection policy and acceptance](docs/PROMISE_REJECTIONS.md).

Function timers remain real later parent-issued worker tasks, sharing the
existing cumulative limits. Startup was still **bundled** in v0.9.9, not a complete HTML
lifecycle. Cross-context messaging, string timers/intervals and the
typed testharness observer remain unfinished. Authored controls are not WPT passes.

[v0.9.9](https://github.com/pierce403/mgbrowser/releases/tag/v0.9.9)
released at `af5d8fc3a8f14555520c07946c0c56c355921a01`. Its exact-commit
Rust/JSPLAN/site checks, tagged packaging, checksum-verified public install and
reinstall, all eleven packaged/public native helpers, and actual v0.9.8
self-upgrade/no-op verification pass. See the
[dated work log](memory/logs/2026-10-22.md) and
[release notes and limitations](docs/RELEASE-v0.9.9.md).

The v0.9.8 Window-error/five-argument `onerror` service remains. Proven local
errors preserve thrown-value identity; foreign/unknown sources and dynamic
compilation conservatively mute the whole realm. Fatal resource failures never
call page reporters, and no throw location is fabricated.
[Window error policy](docs/WINDOW_ERRORS.md).

v0.9.6 corrects classic external script loading (`<script src="...">`) with
typed parent-supplied sources, shared resource admission and explicit handling
of unsupported script attributes. The native **Performance Dashboard** (`F12` /
`Ctrl+Shift+I` → **Performance**, or `Ctrl+Shift+P`) and headless **CLI profiler**
(`mgbrowser --profile <URL>`) report completed browser paints, actual cache paths
and resource timings. Frame and computed-style caches share immutable data,
retain at most 16 MiB and 8 MiB of payload per Browser, and render uncached above
those bounds. RustyBuzz shaping has separate bounded caches and measured hit
counters. These are payload limits, not RSS ceilings or a scrolling-speed
guarantee. [Performance measurements and limits](docs/PERFORMANCE.md).

Pure-Rust Boa ECMAScript execution runs inline and classic external scripts by default
(`--disable-scripts` opts out), alongside bottom-left link hover URL previews,
**Open link in new tab** from the right-click context menu and **Ctrl+click** on
links. The freshly measured WPT selection retains all 24 original passing
flexbox reftests and covers the complete pinned **1,011-case flexbox markup
reftest selection**, with failures and unsupported cases retained. That separate
selection reports **319 PASS, 630 FAIL and 62 UNSUPPORTED / 1,011**, with
no errors, timeouts or crashes. Physical column justification adds two genuine
reference matches; all prior passes and runnable cases remain protected.
Timer and installed-layout fixtures are authored controls, not additional
upstream or JavaScript testharness passes. The separate Window selection reports
15/16 passing files and 77/79 passing final subtests.
[Scope and remaining gaps](docs/WPT.md).

Google News reading is substantially improved: bounded flex/grid layout,
positioned headers, clipping, inline SVG and JPEG thumbnails now render the
signed-out briefing and story columns. Rust Taffy supplies flex/grid sizing;
Mg still owns the DOM, measurement, painting and input. This is a reading-first
increment, not full News compatibility: author fonts, rounded corners and some
icons are missing; topic destinations can still fall back to readable flow.
Search, menus, account actions and personalization remain later work.
[Reading scope and evidence](docs/GOOGLE_NEWS.md).

Right-click a page and choose **Inspect element** (or **Open link in new tab**
on a hyperlink), press **F12** / **Ctrl+Shift+I**, or press **Ctrl+Shift+P** for
the **Performance** tab. The native read-only inspector shows DOM attributes, text and
painted bounds, bounded CSS, resource and script diagnostics, and live page-load,
render-cache and subresource profiling. This is not
the Chrome DevTools frontend or a JavaScript debugger. [Inspector guide](docs/INSPECTOR.md).

**Ctrl+T** opens a blank tab; **Ctrl+W** closes the current tab, and
**Ctrl+Tab / Ctrl+Shift+Tab** switches tabs. Drag tabs to reorder, detach into
native windows or form left/right page groups. Moves preserve the live page,
including form edits and its script realm. Initial limits: 16 tabs, four windows
and two groups per window. [Desktop scope](docs/DESKTOP_NEXT.md).

**Ordinary Playwright compatibility remains unfinished.**
Stable CDP targets and a failing pinned-client acceptance test are foundations,
not a claim that ordinary Playwright locators work.
[Playwright acceptance](docs/PLAYWRIGHT.md) and the
[signed-out News reading scope](docs/GOOGLE_NEWS.md) keep their remaining gates explicit.

Wheel scrolling now eases over 150 ms. Repeated input accumulates and reversing
responds immediately; clicks, keys and navigation stop motion at the visible
position. Keyboard/CDP scrolling stays immediate. This is not high-resolution
touchpad support or a guarantee of 60 fps on the software renderer.

About shows download progress with received bytes and percentage when the server
provides a total size. After an update installs, **Menu > About > Restart now** immediately launches the
updated binary and reopens committed tab URLs in one window. Saved settings/bookmarks and the
scripting preference carry over; unsaved edits, cookies and browsing history do
not; pane/window placement is not restored. Restart is always your choice,
never forced. [Update details](docs/UPDATES.md).

The single-row navigation bar groups **Back, Forward, Refresh and Bookmark** to
the left of the URL, with a **hamburger menu** on the right. The redundant browser/
page title row is removed; the window title remains. The star or **Ctrl+D** saves/removes the loaded page.
**Menu > Bookmarks** or **Ctrl+Shift+O** opens the local list, with open/remove
controls. Bookmarks survive restart; no account or sync is involved.
URL selection highlights just the visible URL text, not the whole address field.
[Bookmark details](docs/BOOKMARKS.md).

The browser now follows your desktop's display size, so controls and websites
are readable on high-DPI screens. **Menu > Settings > Size** offers System or
saved manual sizes from 75% to 300%. **Ctrl+plus/minus** changes size;
**Ctrl+0** restores System. Text is rasterized at the correct physical size,
not enlarged from a finished screenshot. [Sizing details](docs/APPEARANCE.md).

Browser controls now follow your desktop's light/dark preference. Open
**Menu > Settings** to choose **System**, **Light** or **Dark**; changes apply
immediately and are saved for the next launch. Page colors and the red plain-HTTP
warning stay unchanged. [Appearance details](docs/APPEARANCE.md).

Boa now runs page JavaScript by default (`--disable-scripts` opts out): modern
inline and classic external `<script src>` scripts, Promise checkpoints and retained click/submit handlers operate
on Mg's real Rust DOM. Mg keeps its own HTML parser, layout and renderer, with
Rust Stylo for CSS. No native JS backend or full Servo browser embedding.
[Scope and resource profile](docs/BOA.md).

Release packages must fit older builds' unchanged 8 MiB updater limit. Layout
and resource limits remain explicit; this does not
establish general CSS or arbitrary-site compatibility.

The installer selects the latest published release. Exact release and public-
installation receipts are recorded separately in the [dated work log](memory/logs/2026-10-22.md).

**Linux x86_64 / X11 or XWayland**, glibc 2.35 or newer. Install the
checksum-verified binary without sudo or Rust:

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

[Download / release notes](https://github.com/pierce403/mgbrowser/releases/latest)
· [Website](https://mgbrowser.org) · [Inspect installer](install.sh)

### Web Platform Tests

Current-source results are recorded in [score.json](tools/wpt/score.json) and
the website's [freshly measured score](https://mgbrowser.org/#wpt).
The fixed selection contains **1,011 upstream flexbox reftests**. It is not the full WPT suite
or a JavaScript conformance score. The tests use the real Rust navigation,
resource-loading, painting and restricted script-worker code. [Method and commands](docs/WPT.md).
The website reruns the selection before every commit's Pages deployment and
publishes the [score and exact tested commit](https://mgbrowser.org/wpt-results.json).
The goal is more genuine passes, without hiding failures or losing old passes.
v0.9.2 completed all 24 tests in the initial cohort; their exact source files and
passing/runnable requirements remain separately immutable. The expanded lane
requires diagnostic-free real Browser painting and exact pixel relations.
Conservative prerequisite failures are not proof that every such image is
incorrect; both failures and unsupported protocols stay visible. Current-source website scores may
advance beyond an installed release; the report identifies the exact tested commit.
[Selection and review](docs/WPT_EXPANSION.md). General JavaScript testharness
integration remains a [separate milestone](docs/WPT_TESTHARNESS.md); the fixed
Window score lane below does not complete it.
The [bounded observer](docs/WPT_OBSERVER.md) now has a locally verified
implementation and sixteen frozen non-scoring controls; see
[scope and release evidence](docs/WPT_OBSERVER_STATUS.md). It is not a scored
JavaScript cohort, general WPT product integration or complete Window readiness.
The historical v0.9.11 public-prefix measurement remains prefix evidence only.

The [first genuine Window cohort](docs/WPT_WINDOW_PARSING.md) is separately
source-frozen: sixteen original declared-value parsing files with 79 prospective
subtests. Its source-only checker verifies selection/bytes/names, not execution.
Its [real-Browser runner](docs/WPT_WINDOW_RUNNER.md) now has a reviewed first
local baseline: **0/16 files passing; 0/79 actual final subtests passing**,
without infrastructure errors, timeouts, crashes or pending files. All fail on
the missing `style.getPropertyValue()` method, before CSS comparisons. The
[historical snapshot](tools/wpt/window/first-baseline.json) retains the exact
measured source and its pre-commit dirty status. The separate
[current Window score](tools/wpt/window/current-score.json) and
[exact-commit report](https://mgbrowser.org/wpt-window-results.json) retain the
16-file outcomes, actual final inventories and unknown partial totals separately
from 79 source-declared calls. They are not the static or installed-binary score.
Both full lanes rerun before every commit; CI checks scores and a manual Window
ratchet before staging either public report. The
[cold-Cargo measurement and continuous graph](docs/WPT_WINDOW_CI.md) preserve
all existing deadlines. Coupled CI/public verification is recorded in the log.
The historical runner contribution claimed no new upstream passes; browser-
feature gains are recorded separately above.

The installer uses `~/.local/bin` (override with `MGBROWSER_INSTALL_DIR`) and adds
a user-level application launcher and Mg icon. Follow its PATH instruction if
needed. Requires a DejaVu/Liberation font, or set `MGBROWSER_FONT` to a readable
TrueType/OpenType font file. `mgbrowser --help` lists controls and options.
Current project source and original artwork use the [Apache License 2.0](LICENSE).
Dependencies retain their own licenses. Published v0.1.0/v0.1.1 archives retain
their original MIT license. New releases include Apache-2.0, NOTICE and third-party
license texts/source links, including Boa's MIT option and MPL-2.0 for Stylo-related dependencies.

Installed builds check for newer versions on startup and daily while open, verify
checksums and test the new binary before replacement. Restart to use an update.
Menu > About shows the running build's version, compile time and source commit.
Use Menu > Check for updates or `mgbrowser --update` manually; disable background
checks with `--no-auto-update` or `MGBROWSER_NO_AUTO_UPDATE=1`.
See [updater behavior and trust](docs/UPDATES.md).

### Known limitations

- System size reads screen-global X11 desktop DPI, with 100% fallback. This is
  whole-browser sizing, not per-site page zoom or per-monitor Wayland scaling.
  Images retain their existing decoded resolution and limits.
- Hacker News and signed-out Google News desktop reading are narrow targets.
  News still has font/icon/corner differences and destination fallbacks. Mobile
  layouts, interactive News and account actions are not acceptance claims.
- Modern-web compatibility is poor. Google search → first result is not working.
- Boa-backed JavaScript integration runs inline and bounded classic external
  `<script src>` scripts by default (use `--disable-scripts` to opt out). Up to
  16 external sources participate within 32 eligible script elements, with a
  256 KiB per-source cap and a 1 MiB combined HTML/source cap. CSS, fonts, images
  and scripts share the existing 2 MiB, 32-request, ten-second loading allowance.
  Deferred sources run after ordinary sources; `nomodule` provides the classic
  fallback while modules remain unsupported. Async scripts, nonempty integrity
  metadata and `crossorigin` are skipped with diagnostics. External sources
  require UTF-8 and a supported JavaScript MIME type. The v0.9.7 release adds
  function-valued `setTimeout`/`clearTimeout`, not string timers or intervals.
  v0.9.8 adds conservatively muted Window errors; v0.9.9 adds bounded rejection
  notification. The v0.9.11 release separates DCL/load after pre-parsed/preloaded
  classic sources, not a streaming parser or dynamic-resource load barrier.
  Cross-context messaging and general JavaScript testharness execution remain
  unfinished; v0.9.12's trusted bounded observer is a separate contributor facility.
  ES modules (`type="module"`), dynamic script injection, fetch/XHR and a general
  browser event loop remain unsupported. No React/Vue browser-app compatibility
  is claimed.
- Script budgets remain cumulative: 1,000,000 opcodes, 1 MiB per source / 4 MiB
  lifetime source admission, 256 pending / 2,048 lifetime jobs and host tasks,
  and 32/64 MiB outstanding/cumulative requested worker allocation. Cancellation
  does not refund lifetime work. Sessions retain two seconds total active time,
  300 seconds absolute lifetime, 64 transactions and 32 MiB aggregate wire bytes;
  DOM and OS CPU/address-space limits also remain unchanged. These are per-worker
  limits, not GC/RSS measurement, complete cooperative budgeting or a global
  browser quota. Native work retains final OS/parent containment. Active detached
  listeners persist until removal/teardown.
- Pinned Boa may retain fatal callback arguments in its private VM until Context
  teardown. The browser discards terminal workers; library embedders must drop
  Fatal/Closed realms. Binding cleanup is not immediate whole-VM reclamation.
- Full CSS is not implemented. Flex/grid, positioning and stacking are bounded
  subsets; rounded corners, transforms and independently scrolling elements are
  not implemented. Stylesheets remain same-origin with no imports; `@font-face`
  is limited to bounded same-origin `.ttf`/`.otf` fonts. PNG, JPEG, first-frame GIF and restricted static SVG images are supported;
  foreign HTTPS images are fetched without cookies. Unsupported or blocked images
  remain placeholders. Dynamic resources are not fetched.
- Linux X11/XWayland is the supported GUI target. Cookies are memory-only.
- Navigation and script budgets apply per tab/worker, not as a whole-workspace
  memory or CPU quota. Ordinary Playwright remains unsupported.
- The restricted JavaScript worker is **not a sandbox for the browser as a whole**.
- Do not use this release for banking, sensitive authenticated browsing, or
  arbitrary hostile websites.

The preview is separate from the formal MVP, whose stronger gates remain open.
The component extraction separates the engines into reusable packages; this release
preserves the styled-document path described in [Hacker News scope](docs/HACKER_NEWS.md).
HTTP pages have a red title/address strip and an "HTTP: Not secure" label.
Ctrl+L selects the location; type a URL and press Enter. Re-running the installer
updates to the latest release. Restart any open browser windows after updating.
See [preview details](docs/RELEASE-v0.9.8.md) for manual install and uninstall.

## Components

| Package | Role |
| --- | --- |
| `mg-browser` | Desktop executable and platform integration |
| `mg-chassis` | Browser services and optional toolbar/UX |
| `mg-butane` | Boa embedding/policy and original interpreter test baseline |
| `mg-sparkle` | HTML, DOM, layout and software rendering |

Butane runs independently of the browser. Sparkle renders documents to pixels
without a window, and Chassis supports embedding with browser chrome disabled.
See the [architecture, examples and compatibility roadmap](docs/ARCHITECTURE.md).
V8/JavaScriptCore and Blink/WebKit replacement APIs, Tauri integration and the
ThermiteOS port remain future work.

See [JSPLAN.md](JSPLAN.md) for the proposed Butane roadmap: a Boa-first Rust
engine evaluation, modern React/Vue acceptance tests, V8-inspired optimizations,
academic references and separately gated embedding compatibility. The first
[isolated P0/P1 experiment](experiments/jsplan/README.md) compares original Butane
with Boa 0.22 using pinned tests and honest failure counts. The subsequent
[page integration](docs/BOA.md) uses an explicit process-contained profile, not
the original logical allocation report. Full P1 resource control, general task
and module loading, framework support and V8 compatibility remain open.

## Engineering background (pre-MVP)

A web browser written from the ground up in Rust, developed through reproducible experiments and open contribution.

**Status: pre-MVP research.** A native Linux browser loads HTML over verified HTTPS, draws text with Rust fonts, submits forms and follows links. Its initial CDP subset supports real automation. Boa page execution runs by default, creates usable controls and retains state for later click/submit handlers inside a restricted worker. The live Google goal remains incomplete and deferred. There is no general autoresearch executor yet.

```sh
cargo run --locked --bin mgbrowser -- https://example.com/
```

Requires an X11/XWayland display and a DejaVu/Liberation font file, or `MGBROWSER_FONT`. See [running and testing](docs/RUNNING.md) for controls, limitations, and repeatable local interaction checks.

For automation, add `--remote-debugging-port=9222` (or `0` for an available port).
The loopback CDP subset provides discovery, navigation, DOM inspection, input and
PNG screenshots; see [the protocol contract and example client](docs/CDP.md).
Full CDP support is the long-term target, not current compatibility.

### Historical original-engine work

The following records the preserved pre-v0.4 evaluator baseline, not the current
page backend or Boa's resource model. Original assertions still run through the
explicit `legacy-test-engine` test feature; they are not a production fallback.

The original interpreter includes bounded Function/eval and UTF-16 regular
expressions with RegExp and String matching/replacement/splitting, plus
for-in enumeration and switch control flow. Bounded explicit-state expression
parsing supports deeper grouping while mixed evaluator recursion has independent
default-stack regressions. Fixed-size allocation diagnostics and shared immutable
function code keep an authored large compiled factory within the unchanged
4 MiB budget. Prepaid array construction and moved argument snapshots now also
let six maximum-sized arrays and a real form fit below that cap. Formal parameters
keep their charged independent copies without paying again for the move into a
local binding; a separate large-parameter form also fits. A sole Function parameter
fragment now moves without redundant joining while real UTF-8 conversion stays
charged. Compact AST statements and capacity-aware storage accounting now let a
20,000-statement function fit while correctly charging holes and spare capacity.
Genuine Symbol primitives now support distinct property keys, registry lookup,
boxing, reflection and implemented coercion hooks, including fallible DOM string
conversion. Functions and native builtins now retain genuine prototype identity,
including inherited metadata and Symbol keys, construction and instanceof.
Six existing Error families now have genuine prototypes and instances, ordered
string conversion and bounded callback-free host diagnostics.
Generic Array.concat now preserves one-level spreading, inherited reads, holes
and identity, with distinct Array/Arguments branding and prepaid result storage.
Empty-only arguments snapshots now materialize on first read, preserving identity
and scope while avoiding unused objects. An authored 8,500-call page now creates
its real form under the same cap; observed snapshots still pay their full cost.
Fresh user-function defaults now also materialize only on an actual prototype
read; paid property metadata, unique identity and constructor backlinks remain
intact. An authored 4,800-function page now creates its form under the same cap.
Bound functions now preserve receiver/prefix ordering, target construction and
instance checks, with restricted metadata and prepaid bounded forwarding. An
authored bound callback creates a real form inside the restricted worker.
Nullish member failures now include bounded, redacted host-only context without
changing caught exceptions, evaluation order or frozen allocation/fuel checkpoints.
Retained page realms now dispatch real later click/submit handlers, preserving
closures, cancellation, stable node identities and versioned edits without
replaying startup. The parent validates post-handler projections and constructs
actual form/link requests; busy or failed activation is not silently accepted.
The existing cumulative fuel, 4 MiB allocation and process limits remain; retained
interaction adds explicit lifetime, transaction and wire bounds. See the
[page-session contract](docs/PAGE_SESSIONS.md).
Non-member native calls now receive undefined correctly, while shared EventTarget
listener methods apply their own nullish-to-Window rule. Host-only diagnostics
also identify the immediate producer category without tracing private source or
changing the observed expression's evaluation or resource costs.
Seven bounded Array callback methods now preserve sparse/inherited entries,
mutation, callback identity and reductions; borrowed methods work on existing
DOM collection snapshots. See the [callback contract](docs/ARRAY_CALLBACKS.md).
Five core constructor links and genuine primitive prototype payloads now work,
including direct/bound Number and Boolean construction. See the
[core intrinsic contract](docs/CORE_INTRINSICS.md).
Fixed parser operators now use static Rust strings, removing their real heap
buffers without changing grammar or limits. The [storage contract](docs/STATIC_OPERATORS.md)
and runnable `measure_ast_storage` example record controlled ownership evidence.
Bounded `Object.create` descriptors now support fresh data properties and genuine
getters/setters with original receivers, typed keys and property flags. The
[descriptor contract](docs/OBJECT_CREATE.md) and runnable `measure_object_create`
example record independent semantic and actual-allocation evidence.
The pre-extraction engine baseline passed 1,228 debug tests and 1,115 selected release checks,
along with 26 native and 26 external CDP journeys, including a handler-required interaction
sequence that cancels a link and first submit before reaching its real destination.
Authored forms
are tested through real worker, native and CDP paths;
this remains a small, opt-in language subset, not general web compatibility.

The last historical bounded Google attempt loaded the homepage and submitted its actual form
through the retained session. Search still has no actionable results: its first
failure is Ast 377,733 rejected after 4,135,770 accepted bytes against the unchanged
4 MiB cap. This response reports no unsupported descriptor error. The changed
response is not a controlled benchmark or proof of an earlier error's cause. No first
result or destination has been reached; further independently tested language,
browser API and storage-ownership work is needed.

- Website: https://mgbrowser.org
- [MVP and architecture plan](docs/MVP.md)
- [Feature specification and acceptance](FEATURES.md)
- [Next tasks](TASKS.md)
- [Autoresearch design](docs/AUTORESEARCH.md)
- [CDP automation and roadmap](docs/CDP.md)
- [Boa page execution and limits](docs/BOA.md)
- [Original JavaScript baseline and historical evidence](docs/JAVASCRIPT.md)
- [Retained page interaction](docs/PAGE_SESSIONS.md)
- [Contributing](CONTRIBUTING.md)
- [Agent instructions](AGENTS.md), [memory](MEMORY.md), and [skills](SKILLS.md)

## Direction

Own the browser's integration and original components, reusing reviewed Rust crates where useful. Build a useful document browser on Linux, expanding compatibility behind explicit acceptance gates. See the plan for the Rust dependency boundary and deferred decisions.

TLS uses the experimental rustls-rustcrypto provider. Stylo computes CSS, Taffy sizes bounded flex/grid contexts and Boa supplies page JavaScript. Fonts, PNG/JPEG/GIF and restricted SVG use Rust implementations with native backends disabled. Chassis fetches bounded same-origin CSS and policy-checked images; Sparkle has no network access. See [dependency policy](docs/DEPENDENCIES.md); run `cargo test --locked --workspace --features legacy-test-engine --all-targets` for modern and preserved-baseline tests. Full CSS and broad browser compatibility remain unimplemented. Boa scripting is enabled by default (`--disable-scripts` opts out) and requires supported Linux x86_64 isolation; it does not sandbox the whole browser.

## Website development

Open `index.html` directly, or serve the repository locally. After changing feature status or adding a daily log:

```sh
mkdir -p tmp
rustc --edition=2024 tools/site.rs -o tmp/site
tmp/site
tmp/site --check
```

Edit descriptive content in `index.html`; the marked status section is generated from `FEATURES.md` and log filenames. GitHub Actions checks it and deploys on every push to `main`. This keeps updates tied to recorded work rather than automatically inventing progress.

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

Native production remains on main. The bounded build-time artifact wait requires
a matching source revision (or the canonical FrogAlert metadata-child receipt),
then verifies all public hashes. Missing/failed canonical output stops deployment
and retains the working version. The generated branch distributes public build
input only. No GitHub lookup is added to the deployed website.
