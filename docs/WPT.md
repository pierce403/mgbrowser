# Web Platform Tests

The goal is to increase **verified Web Platform Tests passes** while preserving
existing browser, worker, resource and security assertions. The first increment
is an honest baseline and regression gate, not a claim of broad web compatibility.

Mg runs a pinned static-reftest pilot from the upstream
[Web Platform Tests repository](https://github.com/web-platform-tests/wpt).
This is not `wptrunner`, WebDriver, a full WPT product integration, or a
`testharness.js` executor. Test tooling does not change the shipped browser.

## Fixed selection and baseline

The input revision is
[`84daed4ae966d9624bed22f43587c79283ad1fea`](https://github.com/web-platform-tests/wpt/commit/84daed4ae966d9624bed22f43587c79283ad1fea).
[The manifest](../tools/wpt/corpus.json) fixes the first 24 lexicographically
ordered top-level `css/css-flexbox/*.html` files having a `rel=match` or
`rel=mismatch` reference. The selection was made before rendering, with no filter
for known passes, scripts, flags or reference requirements. The cutoff is
`align-self-013.html`; 59 candidates were examined from 1,340 HTML candidates.

The initial adapter can attempt seven selected tests. Seventeen have unsupported
prerequisites, including XML/XHTML references, Ahem/webfont selection or script
setup. Those tests remain in the 24-test denominator. Their source-derived
classification is recalculated on every run; an ordinary rendering mismatch is
not relabeled unsupported to improve the score.

The first completed run reports **6 PASS, 1 FAIL and 17 UNSUPPORTED out of 24**,
with no errors, timeouts or crashes. `align-baseline.html` fails because its
reference uses unsupported `width: max-content`, causing the real layout path
to fall back. This is not proof of an isolated flex-baseline bug;
matching fallback pixels are not accepted as a
pass. The authoritative current results are [score.json](../tools/wpt/score.json),
the website's **selected tests passing** count, and its
[full report](https://mgbrowser.org/wpt-results.json).
None is a percentage of the complete upstream suite or the installed release.

The unmodified upstream HTML, CSS, SVG, font and license bytes are inventoried
with lengths and SHA-256 hashes. Missing, changed or extra input files fail the
gate. Reference relations are checked against the original HTML. The runner
does not rewrite assertions, turn XHTML into HTML, remove unsupported scripts,
or substitute a different font for Ahem. Corpus updates need a separate reviewed
change and a new baseline, not an edit hidden inside a scored implementation fix.

Upstream tests retain their [BSD-3-Clause license](../tools/wpt/upstream/LICENSE.md).
The included unmodified Ahem font retains its embedded public-domain/CC0 notice;
the manifest records that exception. The project's Apache-2.0 license does not
replace third-party notices. These fixtures are not shipped in browser archives.

## What executes

The Python standard-library runner starts a private, ephemeral loopback HTTP
server serving only the pinned file inventory and explicitly authored controls.
It invokes the Rust [wpt_render example](../examples/wpt_render.rs) in a fresh
process for each unique test or reference page:

1. Construct the real Chassis `Browser` with `DisabledScripts` and no chrome.
2. Navigate normally, including the production parser and CSS/image resource
   loader, at an 800 × 600 logical-pixel viewport and scale 1.
3. Save the actual `Browser::paint` PNG and exact packed RGB bytes.
4. Re-render that loaded document with the same font and viewport to collect
   existing style/layout diagnostics. Require pixel equality with the actual
   browser paint before using those diagnostics.
5. Compare test and upstream reference RGB pixels, retaining the original
   match/mismatch relation. Matches require exact equality, without a custom
   tolerance. The adapter supports alternative match references and required
   mismatch references; it does not implement general chained-reference logic.

The host selects one DejaVu Sans font file and records its SHA-256. This does
not provide CSS font-family selection or downloaded fonts. Each report also
records the adapter binary hash, viewport, current Git commit, dirty-tree flag,
corpus/source fingerprints, page artifacts, diagnostics and actual HTTP requests.

Two authored controls run before the scored corpus. A green square must render
using a genuinely fetched external stylesheet, a red square must differ, and
known foreground/background pixels must be correct. Equality and inequality
must behave consistently. These controls are excluded from WPT counts.
Uniform output is not accepted as a pilot pass: legitimate uniform references
need a separately reviewed precondition before entering a supported lane.
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
from the denominator. The current adapter rejects script/event-handler setup,
`reftest-wait`, XML/XHTML, Ahem/webfonts, CSS imports, fuzzy or timing metadata,
animation/transitions, WPT server substitutions, responsive-image setup and
unsupported embedded-resource protocols. Overflow/scrollbar-dependent results
are outside this viewport-only pilot. No JavaScript conformance, WebDriver,
general CSS, account workflow or live-site compatibility follows from a pass.

Page navigation has a 35-second inner deadline; the runner has a 45-second
subprocess deadline that also bounds painting. Partial reports are written as
tests finish. Errors, timeouts, crashes, missing requests or changed evaluated
source invalidate the overall gate. Existing production navigation, rendering,
allocation and worker limits remain unchanged. The adapter never enables scripts
or executes a page in an unrestricted parent JavaScript runtime.

This is trusted, reviewed, pinned test infrastructure, not a sandbox for arbitrary
web content. Loopback URL checks and a cleared child environment do not provide
OS network isolation. Do not point it at live sites, unreviewed user content or
access challenges. Future script/testharness work must use the reviewed restricted
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
logs, screenshots and RGB bytes under ignored `tmp/`. `--font` selects an explicit
font path for diagnostics; its hash remains in the report. Every runner invocation
checks Rust 1.91.1 and builds the locked adapter from current source before
rendering, with a 20-minute build deadline. It uses the executable reported by
that Cargo invocation, including custom target directories, not a guessed old
binary path. There is no arbitrary prebuilt-binary
override that could associate stale executable bytes with a new source fingerprint.

`--update-score` updates only the deterministic summary, not regression
expectations. Review all changed outcomes before committing it. The
`expectations.json` ratchet freezes the test list and corpus hash, requires
previously passing tests to keep passing, and prevents previously runnable tests
from becoming unsupported. Newly confirmed passes and supported cases must be
added to the ratchet through an explicit reviewed change. There is no automatic
acceptance of weaker expectations.

The source fingerprint hashes evaluated browser/build/adapter/runner file
contents, not `HEAD`: including the commit containing the score would create a
circular identity. The runtime report separately records the exact tested Git
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

1. Establish this truthful frozen baseline and keep website results current.
2. Diagnose failing supported cases using their original sources, screenshots
   and diagnostics. Implement general standards behavior, retain existing tests,
   manually tighten the ratchet and release each user-visible increment.
3. Reduce unsupported prerequisites deliberately: genuine XML/XHTML semantics,
   appropriate Ahem/font selection, then general script/testharness protocols
   under the existing worker and resource review. Do not rewrite tests to avoid
   their prerequisites.
4. Expand the pinned corpus in a separate reviewed change, preserving old
   coverage and reporting the changed denominator and baseline explicitly.
5. Add broader upstream runner/WebDriver integration when the actual browser
   protocols support it. Keep Test262 language results separate from integrated
   WPT web-platform behavior.

Maximizing means more genuine supported passes, not fewer tests, wider pixel
tolerances, hidden errors, weaker limits or claims based only on CSS parsing.
