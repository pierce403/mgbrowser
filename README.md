# mgbrowser

A web browser written from the ground up in Rust, developed through reproducible experiments and open contribution.

**Status: pre-MVP research.** A native Linux browser loads HTML over verified HTTPS, draws text with Rust fonts, submits forms and follows links. Its initial CDP subset supports real automation. An opt-in original JavaScript interpreter now creates usable page controls inside a restricted worker; local native/CDP journeys pass. Google’s homepage and form submission work, but its search scripts still exceed our supported subset and return no actionable results. The live Google goal remains incomplete. There is no general autoresearch executor yet.

```sh
cargo run --locked --bin mgbrowser -- https://www.google.com/
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
All 788 debug tests and 690 selected release checks pass locally, alongside
18 native and 18 external CDP journeys. Ingress, real-copy and resource limits
remain intact. The latest Google attempt reports property access on null or
undefined, then a later source admission exceeds the unchanged 4 MiB allocation limit.
There are still no actionable results or newly completed live stages. Next is
independent language/builtin coverage and measured storage-ownership work.
Authored script-created forms
are tested through real worker, native and CDP paths;
this remains a small, opt-in language subset, not general web compatibility.

- Website: https://mgbrowser.org
- [MVP and architecture plan](docs/MVP.md)
- [Feature specification and acceptance](FEATURES.md)
- [Next tasks](TASKS.md)
- [Autoresearch design](docs/AUTORESEARCH.md)
- [CDP automation and roadmap](docs/CDP.md)
- [Original JavaScript subset and limits](docs/JAVASCRIPT.md)
- [Contributing](CONTRIBUTING.md)
- [Agent instructions](AGENTS.md), [memory](MEMORY.md), and [skills](SKILLS.md)

## Direction

Own the browser engine: HTML parsing, DOM, CSS cascade, layout, painting, navigation and JavaScript. Build a useful document browser on Linux, expanding compatibility behind explicit acceptance gates. See the plan for the Rust dependency boundary and deferred decisions.

TLS uses the experimental rustls-rustcrypto provider. Fonts and PNG decoding use Rust implementations with native backends disabled. The current document view displays image placeholders/alt text; it does not yet download/render page images. See [dependency policy](docs/DEPENDENCIES.md); run `cargo test --locked --all-targets` for component, worker and UI/CDP tests. Full CSS and broad JavaScript compatibility remain unimplemented. Script execution requires `--enable-scripts` and supported Linux x86_64 isolation; it does not sandbox the whole browser.

## Website development

Open `index.html` directly, or serve the repository locally. After changing feature status or adding a daily log:

```sh
mkdir -p tmp
rustc --edition=2024 tools/site.rs -o tmp/site
tmp/site
tmp/site --check
```

Edit descriptive content in `index.html`; the marked status section is generated from `FEATURES.md` and log filenames. GitHub Actions checks it and deploys on every push to `main`. This keeps updates tied to recorded work rather than automatically inventing progress.
