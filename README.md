# mgbrowser

<img src="assets/mgbrowser.svg" width="112" alt="Burning magnesium Mg tile">

## v0.2.0 Experimental Preview

**Source available; binary release pending.** The four-component refactor is on
main. The installer below currently delivers the published v0.1.1 preview.
The v0.2.0 package and release notes are prepared; publication awaits its tag.

**Linux x86_64 / X11 or XWayland**, glibc 2.35 or newer. Install the
checksum-verified binary without sudo or Rust:

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

[Download / release notes](https://github.com/pierce403/mgbrowser/releases/latest)
· [Website](https://mgbrowser.org) · [Inspect installer](install.sh)

For an experimental **macOS source build with XQuartz**, see the
[Mac installation instructions](docs/RUNNING.md#experimental-macos-source-install).
Page-script isolation remains Linux x86_64 only.

The installer uses `~/.local/bin` (override with `MGBROWSER_INSTALL_DIR`) and adds
a user-level application launcher and Mg icon. Follow its PATH instruction if
needed. Requires a DejaVu/Liberation font, or set `MGBROWSER_FONT` to a readable
TrueType/OpenType font file. `mgbrowser --help` lists controls and options.
Current project source and original artwork use the [Apache License 2.0](LICENSE).
Dependencies retain their own licenses. Published v0.1.0/v0.1.1 archives retain
their original MIT license; v0.2.0 includes Apache-2.0 and NOTICE.

### Known limitations

- Modern-web compatibility is poor. Google search → first result is not working.
- JavaScript is an incomplete original implementation, disabled by default;
  use `--enable-scripts` to opt in. External scripts and general browser event-loop
  behavior are incomplete.
- Full CSS is not implemented. Page images are not generally downloaded/rendered yet.
- Linux X11/XWayland is the supported GUI target. Cookies are memory-only.
- The restricted JavaScript worker is **not a sandbox for the browser as a whole**.
- Do not use this release for banking, sensitive authenticated browsing, or
  arbitrary hostile websites.

The preview is separate from the formal MVP, whose stronger gates remain open.
v0.2.0 extracts the existing engines into reusable packages without expanding web compatibility.
HTTP pages have a red title/address strip and an "HTTP: Not secure" label.
Ctrl+L selects the location; type a URL and press Enter. Re-running the installer
updates to the latest release. Restart any open browser windows after updating.
See [preview details](docs/RELEASE-v0.2.0.md) for manual install and uninstall.

## Components

| Package | Role |
| --- | --- |
| `mg-browser` | Desktop executable and platform integration |
| `mg-chassis` | Browser services and optional toolbar/UX |
| `mg-butane` | Original JavaScript engine |
| `mg-sparkle` | HTML, DOM, layout and software rendering |

Butane runs independently of the browser. Sparkle renders documents to pixels
without a window, and Chassis supports embedding with browser chrome disabled.
See the [architecture, examples and compatibility roadmap](docs/ARCHITECTURE.md).
V8/JavaScriptCore and Blink/WebKit replacement APIs, Tauri integration and the
ThermiteOS port remain future work.

## Engineering background (pre-MVP)

A web browser written from the ground up in Rust, developed through reproducible experiments and open contribution.

**Status: pre-MVP research.** A native Linux browser loads HTML over verified HTTPS, draws text with Rust fonts, submits forms and follows links. Its initial CDP subset supports real automation. An opt-in original JavaScript interpreter creates usable controls and retains page state for later click/submit handlers inside a restricted worker. The live Google goal remains incomplete. There is no general autoresearch executor yet.

```sh
cargo run --locked --bin mgbrowser -- https://example.com/
```

Requires an X11/XWayland display and a DejaVu/Liberation font file, or `MGBROWSER_FONT`. See [running and testing](docs/RUNNING.md) for controls, limitations, and repeatable local interaction checks.

For automation, add `--remote-debugging-port=9222` (or `0` for an available port).
The loopback CDP subset provides discovery, navigation, DOM inspection, input and
PNG screenshots; see [the protocol contract and example client](docs/CDP.md).
Full CDP support is the long-term target, not current compatibility.

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

The latest bounded Google attempt loads the homepage and submits its actual form
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
- [Original JavaScript subset and limits](docs/JAVASCRIPT.md)
- [Retained page interaction](docs/PAGE_SESSIONS.md)
- [Contributing](CONTRIBUTING.md)
- [Agent instructions](AGENTS.md), [memory](MEMORY.md), and [skills](SKILLS.md)

## Direction

Own the browser engine: HTML parsing, DOM, CSS cascade, layout, painting, navigation and JavaScript. Build a useful document browser on Linux, expanding compatibility behind explicit acceptance gates. See the plan for the Rust dependency boundary and deferred decisions.

TLS uses the experimental rustls-rustcrypto provider. Fonts and PNG decoding use Rust implementations with native backends disabled. The current document view displays image placeholders/alt text; it does not yet download/render page images. See [dependency policy](docs/DEPENDENCIES.md); run `cargo test --locked --workspace --all-targets` for component, worker and UI/CDP tests. Full CSS and broad JavaScript compatibility remain unimplemented. Script execution requires `--enable-scripts` and supported Linux x86_64 isolation; it does not sandbox the whole browser.

## Website development

Open `index.html` directly, or serve the repository locally. After changing feature status or adding a daily log:

```sh
mkdir -p tmp
rustc --edition=2024 tools/site.rs -o tmp/site
tmp/site
tmp/site --check
```

Edit descriptive content in `index.html`; the marked status section is generated from `FEATURES.md` and log filenames. GitHub Actions checks it and deploys on every push to `main`. This keeps updates tied to recorded work rather than automatically inventing progress.
