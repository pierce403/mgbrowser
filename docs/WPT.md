# Web Platform Tests

The goal is to increase **verified Web Platform Tests passes** while preserving
existing browser, worker, resource and security assertions. The first increment
is an honest baseline and regression gate, not a claim of broad web compatibility.

Mg runs a pinned flexbox reftest selection from the upstream
[Web Platform Tests repository](https://github.com/web-platform-tests/wpt).
This is not `wptrunner`, WebDriver, a full WPT product integration, or a
`testharness.js` executor. Test tooling does not change the shipped browser.

## Original pilot and release history

The input revision is
[`84daed4ae966d9624bed22f43587c79283ad1fea`](https://github.com/web-platform-tests/wpt/commit/84daed4ae966d9624bed22f43587c79283ad1fea).
[The original manifest](../tools/wpt/baselines/css-flexbox-first-24-v1.corpus.json) fixes the first 24 lexicographically
ordered top-level `css/css-flexbox/*.html` files having a `rel=match` or
`rel=mismatch` reference. The selection was made before rendering, with no filter
for known passes, scripts, flags or reference requirements. The cutoff is
`align-self-013.html`; 59 candidates were examined from 1,340 HTML candidates.

The initial adapter could attempt seven selected tests. Seventeen had unsupported
prerequisites, including XML/XHTML references, Ahem/webfont selection or script
setup. Those tests remain in the 24-test denominator. Their source-derived
classification is recalculated on every run; an ordinary rendering mismatch is
not relabeled unsupported to improve the score.

The initial run reported **6 PASS, 1 FAIL and 17 UNSUPPORTED out of 24**.
The v0.9.1 release reported **7 PASS, 0 FAIL and 17 UNSUPPORTED out of 24**,
with no errors, timeouts or crashes. First, `align-baseline.html`'s reference
required genuine `width: max-content` support. Removing its fallback then exposed
a separate column baseline grouping error. Both fixes preserve the upstream
files: test and reference now match exactly, without diagnostics. The ratchet
added that pass while retaining every old test and supported prerequisite.

The v0.9.2 release reports **24 PASS, 0 FAIL and 0 UNSUPPORTED out of 24**,
with no errors, timeouts or crashes. It adds bounded XML/XHTML document parsing
with CDATA stylesheet unwrapping, normal-flow block advancement by signed
vertical margin-box extent, column flex cross-axis `fit-content` sizing with
whitespace-trimmed intrinsic measurement, block static-position resolution for
`inset: auto` absolutely positioned overlays, aspect-ratio-only SVG flex item
sizing, implicit `<body>` synthesis, row flex first-baseline alignment offsets,
bounded same-origin `@font-face` `.ttf`/`.otf` loading and selection, and DOM
`element.style` camelCase property normalization with restricted-worker inline
`<script>` execution for HTML pages.
The authoritative current results are [score.json](../tools/wpt/score.json),
the website's **selected tests passing** count, and its
[full report](https://mgbrowser.org/wpt-results.json).
None is a percentage of the complete upstream suite or the installed release.

The width correction covers ordinary, atomic-inline, flex/grid, replaced and
positioned sizing while preserving forced flex widths, numeric constraints and
box edges. The column correction is deliberately bounded: horizontal/LTR,
proven single-line groups with identical participating left-margin values.
Finite-height wrapped groups and unequal left-margin group sizing produce
explicit layout diagnostics/fallback. Auto-margin and nonbaseline items do not
join the group. Other writing modes, general baseline layout and full intrinsic
sizing remain unsupported; resource limits and the restricted script path are
unchanged. These are browser changes, so public release verification is required
in addition to the source-linked WPT gate.

[v0.9.1](https://github.com/pierce403/mgbrowser/releases/tag/v0.9.1) shipped the
initial width and column-baseline fixes at `a89747f927c83e4ae9715198b5b66437b9a6f422`.
v0.9.2 ships the remaining XML/XHTML, layout, `@font-face` and DOM `style`
improvements for the 24-test cohort; see the 2026-10-13 work log for release receipts.

The unmodified upstream HTML, CSS, SVG, font and license bytes are inventoried
with lengths and SHA-256 hashes. Missing, changed or extra input files fail the
gate. Reference relations are checked against the original HTML. The runner
does not rewrite assertions, turn XHTML into HTML, remove unsupported scripts,
or substitute a different font for Ahem. Corpus updates need a separate reviewed
change and a new baseline, not an edit hidden inside a scored implementation fix.

The upstream [BSD-3-Clause license](../tools/wpt/upstream/LICENSE.md) and
per-file notices are preserved. Many test documents have their own embedded
public-domain/CC0 notices. The included unmodified Ahem font also retains its
embedded public-domain/CC0 notice; the manifest records that binary exception.
The project's Apache-2.0 license does not
replace third-party notices. These fixtures are not shipped in browser archives.

## Complete pinned flexbox selection

The separate [expansion policy](WPT_EXPANSION.md) freezes the complete recursive
`css/css-flexbox` markup reftest selection before browser execution: **1,011 tests**
at the same upstream revision. All 2,042 files in that directory and their static
dependency/license closure are imported unchanged: 2,062 files / 3,332,952 bytes.
The manifest preserves original relation/href data, explicit unsupported edges
and the source-derived roster digest. Flags, scripts and unsupported protocols
do not exclude tests from this denominator.

The original 24-case corpus and pass/support ratchet are separately retained in
`tools/wpt/baselines/`. Both their exact bytes and every old required pass/support
entry remain enforced. The larger selection is a measurement expansion, not
evidence of a browser improvement or full WPT support. The production engine is
unchanged from v0.9.6 for this baseline.

The accepted complete baseline is **317 PASS, 632 FAIL and 62 UNSUPPORTED out
of 1,011**, with **0 ERROR, TIMEOUT or CRASH**. All original 24 tests still pass.
Manual review strengthened the active ratchet to all 317 passes and all 949
runnable cases. The full report retains failure reasons and per-page evidence;
conservative diagnostic/viewport precondition failures are not all proven pixel
bugs. Broader coverage exposes existing gaps rather than reducing compatibility.

Two stylesheet references are demonstrably absent from the pinned upstream
directory. Their dependent cases remain visible FAIL outcomes, not invented
resources, dropped cases or unsupported protocols. Missing/changed inventoried
files are still infrastructure errors. See the import review for the exact
paths and source-only evidence.

## What executes

The Python standard-library runner serves only the pinned file inventory and
explicitly authored controls through private ephemeral loopback fixtures.
The reviewed two-lane orchestration in [WPT_EXPANSION.md](WPT_EXPANSION.md)
uses fresh, uniquely owned fixture contexts and renderer processes per unique
test/reference page. One-/two-lane equivalence is an adoption gate, not permission
to accept different outcomes. It invokes the Rust
[wpt_render example](../examples/wpt_render.rs) for each produced page:

1. Construct the real Chassis `Browser` without chrome. Static pages use
   `DisabledScripts`; supported HTML inline setup uses the real restricted Linux
   worker and explicit `--enable-scripts` admission.
2. Navigate normally, including the production parser and CSS/image resource
   loader, at an 800 × 600 logical-pixel viewport and scale 1.
3. Save the actual `Browser::paint` PNG and exact packed RGB bytes, then store
   RGB losslessly compressed after a verified round trip.
4. Re-render that loaded document with the same font and viewport to collect
   existing style/layout diagnostics. Require pixel equality with the actual
   browser paint before using those diagnostics.
5. Compare test and upstream reference RGB pixels, retaining the original
   match/mismatch relation. Matches require exact equality, without a custom
   tolerance. The adapter supports alternative match references and required
   mismatch references; it does not implement general chained-reference logic.

The host selects one DejaVu Sans fallback file and records its SHA-256. Supported
same-origin TTF/OTF font resources, including pinned Ahem, are fetched and selected
through the production resource/font pipeline. Each report also
records the adapter binary hash, viewport, current Git commit, dirty-tree flag,
corpus/source fingerprints, page artifacts, diagnostics and actual HTTP requests.

The default is two fixed renderer lanes (`--jobs 2`); `--jobs 1` is the
sequential equivalence control, and higher values are rejected. Assignment is
manifest index modulo lane count, not an outcome-derived partition. Shared
references render once through a bounded single-flight cache. Build and authored
controls finish before these lanes start; each lane has at most one renderer
group, conservatively up to four owned script children across both unchanged
per-Browser allowances. This is not an aggregate RSS or whole-browser sandbox.

Each produced page has a fresh fixture origin that is never reused during that
run. Its synchronous request handler drains before its immutable audit enters
the page cache. The report attributes global requests to page/lane and retains
per-page request copies, requiring a successful main-document request. Partial
or failed responses, unknown/late requests and non-200 resources invalidate the
gate. The green control also proves its external CSS request. Two producer slots
are reserved together under the single evidence allowance; they are released
only after confirmed process, fixture and lane cleanup. Unknown cleanup stops
admission and quarantines its slots, including during final-report replacement.

Two authored controls run before the scored corpus. A green square must render
using a genuinely fetched external stylesheet, a red square must differ, and
known foreground/background pixels must be correct. Equality and inequality
must behave consistently. These controls are excluded from WPT counts.
Uniform output is not accepted as a pilot pass: legitimate uniform references
need a separately reviewed precondition before entering a supported lane.
PASS additionally requires an empty rendering-diagnostic list and zero omitted
diagnostics on both the test and every reference. Any nonempty diagnostic kind
is a conservative FAIL, including unknown future kinds. This deliberately goes
beyond pixel equality: intentional-invalid-CSS cases may undercount until their
protocol is separately reviewed. No case-specific exemptions are used.
Missing resources, fallback rendering or absent/wrong-size artifacts cannot
silently pass because two pages produced the same empty or fallback image.

Reference equality still has a limitation: both sides can share an unimplemented
feature. Inspect test/reference screenshots and diagnostics when accepting new
passes. The controls are a useful guard, not an independent proof of every
reference's correctness or complete CSS support.

## Results and unsupported protocols

Every selected test has exactly one visible outcome:

| Status | Meaning |
| --- | --- |
| `PASS` | Admitted page/reference rendering and original pixel relations passed. |
| `FAIL` | A comparison or required rendering/resource precondition failed. |
| `UNSUPPORTED` | A required execution/rendering protocol is not implemented by this adapter. |
| `ERROR` | Configuration, launch, artifact or other infrastructure failure. |
| `TIMEOUT` | A page exceeded its subprocess deadline. |
| `CRASH` | The renderer process terminated by a signal. |

Missing prerequisites remain visible, with reasons, rather than disappearing
from the denominator. The current adapter supports static HTML/XHTML (including
XML markup served as XHTML), bounded
font resources and inline HTML setup. It rejects external/module/XML script
setup, event-handler attributes, `reftest-wait`, CSS imports, fuzzy or timing metadata,
animation/transitions, WPT server substitutions, responsive-image setup and
unsupported embedded-resource protocols. Top-level SVG/generic XML navigation
is rejected rather than allowing identical generic resource placeholders to pass.
Overflow/scrollbar-dependent results
are outside this viewport-only pilot. No JavaScript conformance, WebDriver,
general CSS, account workflow or live-site compatibility follows from a pass.

Page navigation has a 35-second inner deadline; the runner has a 45-second
subprocess deadline that also bounds painting. The complete run, including the
unchanged 20-minute build allowance, has a 25-minute controlled-work deadline;
a final bounded evidence flush is allowed after a failure/deadline so partial
results are not lost. This is not an OS kill at precisely 25 minutes. Inputs remain
under 10 MiB and 4,096 files; evidence is admitted under 512 MiB, reports under
64 MiB, and request audits under 20,000 entries. Per-page log/metadata/PNG caps
are 1/1/8 MiB; RGB is exactly 1,440,000 bytes before lossless compression.
Cargo stdout/stderr share a separate 32 MiB allowance. These are runner admission
and monitoring controls, not filesystem quotas or production browser limits.

Report schema 2 keeps one full record per unique page in `pages`, with thin
references from individual test results. Periodic atomic checkpoints retain
every selected test, including PENDING cases on interruption. PENDING cannot be
published as a completed score. The deterministic website summary stays schema 1.
Known admitted script/navigation failures are FAIL; malformed/opaque host or
artifact failures remain ERROR. Failed rendering geometry never becomes
UNSUPPORTED after execution. Errors, timeouts, crashes, missing requests or changed evaluated
source invalidate the overall gate. Existing production navigation, rendering,
allocation and worker limits remain unchanged. Supported script setup runs only
through the reviewed restricted child process.

This is trusted, reviewed, pinned test infrastructure, not a sandbox for arbitrary
web content. Loopback URL checks and a cleared child environment do not provide
OS network isolation. Do not point it at live sites, unreviewed user content or
access challenges. Source preflight rejects meta-refresh navigation; an
adapter-only script service rejects proposed navigation, default navigation
actions and refresh-bearing snapshots before Chassis can act on them. This
conservative protocol policy also rejects inert/detached refresh nodes; it is
not a production browser restriction or a general untrusted-source sandbox.
Owned process groups are killed on completion, timeout or output overflow; the
runner reaps its direct process, not grandchildren owned by another parent.
Future script/testharness work must use the reviewed restricted
worker path and preserve its assertions; adopting a new resource profile requires
an explicit review.

## Reproduce and publish

Run from the repository root with Rust 1.91.1, Python 3 and DejaVu Sans installed:

```sh
cargo +1.91.1 build --locked --example wpt_render
python3 -m unittest discover -s tools/wpt -p 'test_*.py'
python3 tools/wpt/run.py --update-score --output tmp/wpt-UNIQUE
rustc +1.91.1 --edition=2024 tools/site.rs -o tmp/site
tmp/site
python3 tools/wpt/run.py --check-score --output tmp/wpt-check-UNIQUE
tmp/site --check
git diff --check
```

Replace `UNIQUE` with a new run name each time. The output directory must not
already exist: stale screenshots are never reused or overwritten. Keep raw
logs, screenshots and compressed RGB evidence under ignored `tmp/`. `--font` selects an explicit
font path for diagnostics; its hash remains in the report. Every runner invocation
checks Rust 1.91.1 and builds the locked adapter from current source before
rendering, with a 20-minute build deadline. It uses the executable reported by
that Cargo invocation, including custom target directories, not a guessed old
binary path. There is no arbitrary prebuilt-binary
override that could associate stale executable bytes with a new source fingerprint.

For an orchestration change, run a fresh `--jobs 1` control and the normal
two-lane run sequentially, never concurrently. Require identical complete case
outcomes, every rendered page's raw RGB hash and original comparisons, and
per-page request multisets after normalizing only the owned ephemeral origins.
Counts alone are insufficient. Preserve the preceding baseline too; do not
accept a new result simply because parallel execution is faster.

`--update-score` updates only the deterministic summary, not regression
expectations. Review all changed outcomes before committing it. The
`expectations.json` ratchet freezes the test list and corpus hash, requires
previously passing tests to keep passing, and prevents previously runnable tests
from becoming unsupported. Newly confirmed passes and supported cases must be
added to the ratchet through an explicit reviewed change. There is no automatic
acceptance of weaker expectations.

The source fingerprint hashes evaluated browser/build/adapter/runner file
contents, not `HEAD`: including the commit containing the score would create a
circular identity. It includes runtime Python helpers as well as the runner.
The runtime report separately records the exact tested Git
commit and dirty state. Documentation-only commits can retain the same source
fingerprint while getting a fresh exact-commit report.

On every main push and pull request, Pages builds the adapter with the locked
Rust 1.91.1 graph, runs harness tests, reruns the selection and checks the committed
score and generated site. A stale or regressed score blocks publication. The
deployed `/wpt-results.json` comes from that commit's fresh run; the workflow
does not publish logs, screenshots, raw RGB data, the repository or other `tmp/`
content. Source-only test tooling needs no new binary release. Any resulting
user-visible browser change still follows the normal release and public-installer
verification policy.

## Maximize verified coverage

The next genuine JavaScript lane is planned in
[WPT_TESTHARNESS.md](WPT_TESTHARNESS.md). Its first milestone is non-scoring
full-harness readiness under unchanged restrictions, not another scored corpus
or a claim that the current browser already implements it.

1. Establish this truthful frozen baseline and keep website results current.
2. Diagnose failing supported cases using their original sources, screenshots
   and diagnostics. Implement general standards behavior, retain existing tests,
   manually tighten the ratchet and release each user-visible increment.
   The first gain is genuine `width: max-content` support and admitted synthesized
   column baselines. The subsequent v0.9.2 implementation completed all 24 cases;
   preserve that full initial cohort while expanding coverage.
3. Add broader script/testharness protocols under the existing worker and
   resource review. Implement missing prerequisites in the browser; retain
   original upstream input semantics.
4. Expand the pinned corpus in a separate reviewed change, preserving old
   coverage and reporting the changed denominator and baseline explicitly.
5. Add broader upstream runner/WebDriver integration when the actual browser
   protocols support it. Keep Test262 language results separate from integrated
   WPT web-platform behavior.

Maximizing means more genuine supported passes, not fewer tests, wider pixel
tolerances, hidden errors, weaker limits or claims based only on CSS parsing.
