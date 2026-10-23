# mgbrowser v0.9.19 Experimental Preview

Bounded auto-width atomic inline height correction. The website installer
follows the latest published release. Delivery receipts and per-commit evidence
belong in the dated work log.

Linux x86_64 / X11 or XWayland, glibc2.35 or newer. Rust1.91.1 and locked dependencies.

## Auto-width inline box heights

Ordinary auto-width inline-block, inline-flex row/column and inline-grid boxes
now contribute their actual allocated outer height to their containing line.
Previously their children could paint while the line reserved zero height.
Following blocks and enclosing borders now account for that allocation.

Measurement and painting share the existing chosen-width recipe and containing-
height Option. Padding, borders and vertical margins count once; explicit/min/
max heights, in-flow descendants, signed outer-height clamping and existing
strut/bottom alignment use the same boundary. This does not add a shrink-to-fit
or baseline algorithm, make an auto height definite for descendant percentages,
or change width admission, fit/max-content, tables, replaced elements or buttons.
[Scoped contract](https://github.com/pierce403/mgbrowser/blob/main/docs/AUTO_INLINE_HEIGHT.md).

## Validation status

Genuine unchanged-production debug/release failures are retained before the
minimal implementation. Focused candidate checks pass35 private and118 public
groups in both build modes, including all27 new authored groups. Two test-owner
bookkeeping mistakes are separately retained and explained in the log; no
browser assertion, original test, dependency, worker/profile or limit is relaxed.
Authored controls do not add upstream WPT passes or denominators.

Full final-version Rust/JSPLAN pass, preserving all364 frozen research outcomes.
Complete reviewed WPT:339 pass /610 fail /62 unsupported of1,011; Window15/16
files and77/79 final subtests. `negative-margins-001.html` improves from180
differing pixels to an exact reference match. All338 prior passes,949 runnable
cases and the original24 remain protected; the reviewed match is added manually.

Improvement is not uniform. Still-failing vertical baseline comparisons worsen
from2600 to6300 and from440 to840 pixels (two cases); `flexbox_order-box.html`
widens from41758 to76628. The still-failing horizontal justify005 reference
grows from516 to616 pixels high, exceeding the unchanged600px viewport, so its
old comparison is no longer eligible. All failures remain counted. Full CSS
baseline/intrinsic layout remains unfinished.

Normal package/install/updater checks, thirteen existing native helpers, eight
additive native/CDP proofs and both frozen worker suites pass locally. All36
additive screenshots were inspected. The new proof's secondary CDP union
expectation was corrected after retaining an actual diagnostic failure: CSS
border height24 remains independently checked, while overflowing descendants
make the documented CDP union25. Native pixel assertions are unchanged.
Exact-source CI/site, tagged-publication, public installation and old-version
upgrade receipts are recorded separately in the dated work log. Local WPT
scores are not installed-binary WPT measurements.

## Install and known limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer verifies SHA-256 and installs without sudo. Checksums are not
independent signatures. Existing processes must restart to run an installed
update. The updater's existing8MiB compressed download ceiling is unchanged.

Modern-web compatibility is poor. Google search to first result is not working.
Ordinary Playwright, full browser event-loop/testharness and React/Vue browser
applications are unsupported. Boa-backed inline and bounded classic external
scripting runs by default (`--disable-scripts` opts out); modules, dynamic
scripts, fetch/XHR and general asynchronous resource loading remain unfinished.
Full CSS/CSSOM, intrinsic sizing, baselines, vertical alignment, bidi and floats
remain incomplete. Fonts/images/tabs/resources are bounded subsets. Cookies
are memory-only. This release does not complete F-003/F-005/F-016/F-026 or MVP.

Linux X11/XWayland is the GUI target. The restricted JavaScript worker is not
a sandbox for the browser as a whole. Do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Existing
source/opcode/System-allocation/work/time/lifetime/wire/OS guards remain;
these are not whole-browser quotas or GC/RSS measurements. Fatal Boa roots
may require actual Context/worker teardown.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Old tags/assets stay immutable.
