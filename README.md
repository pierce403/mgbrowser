# mgbrowser

<img src="assets/mgbrowser.svg" width="112" alt="Burning magnesium Mg tile">

## v0.9.0 Experimental Preview

Google News reading is substantially improved: bounded flex/grid layout,
positioned headers, clipping, inline SVG and JPEG thumbnails now render the
signed-out briefing and story columns. Rust Taffy supplies flex/grid sizing;
Mg still owns the DOM, measurement, painting and input. This is a reading-first
increment, not full News compatibility: author fonts, rounded corners and some
icons are missing; topic destinations can still fall back to readable flow.
Search, menus, account actions and personalization remain later work.
[Reading scope and evidence](docs/GOOGLE_NEWS.md).

Right-click a page and choose **Inspect element**, or press **F12** /
**Ctrl+Shift+I**. The native read-only inspector shows DOM attributes, text and
painted bounds, plus bounded CSS, resource and script diagnostics. This is not
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

Boa now runs opt-in page JavaScript: modern inline scripts, Promise checkpoints
and retained click/submit handlers operate on Mg's real Rust DOM. Mg keeps its
own HTML parser, layout and renderer, with Rust Stylo for CSS. No native JS
backend or full Servo browser embedding. [Scope and resource profile](docs/BOA.md).

The size-optimized, symbol-stripped release stays within older builds' unchanged
8 MiB updater limit. Layout and resource limits remain explicit; this does not
establish general CSS or arbitrary-site compatibility.

The installer selects the latest published release. Exact release and public-
installation receipts are recorded in the [dated work log](memory/logs/2026-09-18.md).

**Linux x86_64 / X11 or XWayland**, glibc 2.35 or newer. Install the
checksum-verified binary without sudo or Rust:

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

[Download / release notes](https://github.com/pierce403/mgbrowser/releases/latest)
· [Website](https://mgbrowser.org) · [Inspect installer](install.sh)

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
- Boa-backed JavaScript integration is incomplete and disabled by default;
  use `--enable-scripts` to opt in. External scripts, modules, timers, fetch/XHR
  and general browser event-loop behavior are not implemented. No React/Vue
  browser-app compatibility is claimed.
- Script budgets include cumulative opcodes, sources/jobs and 32/64 MiB
  outstanding/cumulative worker allocation requests. This is not GC/RSS
  measurement or complete cooperative budgeting; native work retains final
  OS/parent containment. Active detached listeners persist until removal/teardown.
- Full CSS is not implemented. Flex/grid, positioning and stacking are bounded
  subsets; rounded corners, transforms and independently scrolling elements are
  not implemented. Stylesheets remain same-origin with no imports or downloaded
  fonts. PNG, JPEG, first-frame GIF and restricted static SVG images are supported;
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
See [preview details](docs/RELEASE-v0.9.0.md) for manual install and uninstall.

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
the original logical allocation report. Full P1 resource control, external
loading/framework support, performance and V8 compatibility remain open.

## Engineering background (pre-MVP)

A web browser written from the ground up in Rust, developed through reproducible experiments and open contribution.

**Status: pre-MVP research.** A native Linux browser loads HTML over verified HTTPS, draws text with Rust fonts, submits forms and follows links. Its initial CDP subset supports real automation. Opt-in Boa page execution creates usable controls and retains state for later click/submit handlers inside a restricted worker. The live Google goal remains incomplete and deferred. There is no general autoresearch executor yet.

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

TLS uses the experimental rustls-rustcrypto provider. Stylo computes CSS, Taffy sizes bounded flex/grid contexts and Boa supplies page JavaScript. Fonts, PNG/JPEG/GIF and restricted SVG use Rust implementations with native backends disabled. Chassis fetches bounded same-origin CSS and policy-checked images; Sparkle has no network access. See [dependency policy](docs/DEPENDENCIES.md); run `cargo test --locked --workspace --features legacy-test-engine --all-targets` for modern and preserved-baseline tests. Full CSS and broad browser compatibility remain unimplemented. Script execution requires `--enable-scripts` and supported Linux x86_64 isolation; it does not sandbox the whole browser.

## Website development

Open `index.html` directly, or serve the repository locally. After changing feature status or adding a daily log:

```sh
mkdir -p tmp
rustc --edition=2024 tools/site.rs -o tmp/site
tmp/site
tmp/site --check
```

Edit descriptive content in `index.html`; the marked status section is generated from `FEATURES.md` and log filenames. GitHub Actions checks it and deploys on every push to `main`. This keeps updates tied to recorded work rather than automatically inventing progress.
