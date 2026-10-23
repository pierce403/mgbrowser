# mgbrowser v0.9.16 Experimental Preview

v0.9.16 corrects definite-width atomic inline sizing in the
existing Rust layout engine. Admission, line height and painting use the same
allocation path. Padding/borders and signed margins are counted once;
percentages use the actual containing content width, not leftover line space.
Definite overflow retains its declared width, and normal-flow horizontal auto
margins resolve to zero rather than borrowing block centering.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

## Scope and evidence

This is a bounded box-model correction, not full inline formatting, vertical
alignment, bidi, floats or CSS conformance. Auto/intrinsic-width, table, replaced,
button paths retain their existing behavior. The adopted addendum extends only
normal-flow definite outer allocation to inline-flex/grid; their internal
formatting and baseline algorithms are unchanged. No dependency, worker
profile, browser protocol or resource allowance is added.
[Frozen contract](https://github.com/pierce403/mgbrowser/blob/main/docs/ATOMIC_INLINE.md).

All 59 authored groups pass focused debug/release without changed expectations:
the original twelve private/twenty-eight public controls and the separately
frozen nine private/ten public inline-flex/grid addendum controls. Actual
pre-change assertion failures remain recorded. Authored controls are not WPT
passes. Full Rust B passes all fourteen gates; JSPLAN B passes eight gates and
retains 364 frozen outcomes, not 364 passes: baseline 2 pass/117 unsupported/
63 exception, Boa 178 pass/2 unsupported/2 exception.

The separately reviewed complete static lane reports **338 PASS / 611 FAIL /
62 UNSUPPORTED of 1,011**: nineteen new exact image matches, with all 319 prior
passes and 949 runnable cases retained. The manual ratchet now protects all
338 passes. The separate Window lane remains 15/16 passing files and 77/79
passing final subtests, with two genuine flex-valid assertions still failing.
Inputs, assertions and resource limits are unchanged. The first complete
static candidate was rejected at 333/616/62 for two old-pass regressions;
Window did not run. Its evidence is preserved, not adopted. The separately
frozen addendum and fresh full B run restore both protected relations.

Improvement is not uniform: `flexbox_order-box.html` remains FAIL and its
different-pixel count increases from 40,551 to 41,758. Definite child widths
are corrected, but the reference's auto-width parent mismatch remains.
Baseline, positioned-child, float and auto/intrinsic boundaries remain open.
See the [dated evidence](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-23.md#v0916-reviewed-b-scores-and-candidate-documentation).
F-003/F-005/F-016/F-026 and the formal MVP remain unfinished.

## Delivery status

Local normal package/install/updater checks, worker/session selftests, all
thirteen existing native helpers, unchanged physical proof and atomic native/CDP
checks at 1x/2x pass. Both frozen worker suites retain their exact sixteen and
five declared outcomes through direct-worker and real Browser paths: eight
positive and thirteen expected negative controls per path, not WPT passes.
The candidate archive is 8,098,691 bytes, within the unchanged 8 MiB updater cap.
Tagged-build and public install/update acceptance are tracked separately in the
dated log; local results alone do not establish public delivery.

## Install and known limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer follows the latest actually published release, verifies SHA-256
and installs without sudo. Checksums are not independent signatures. Existing
browser processes must restart to use an installed update.

Modern-web compatibility is poor. Google search to first result is not working.
Ordinary Playwright compatibility, full browser event-loop/testharness behavior
and React/Vue applications are not supported. Boa-backed inline and bounded
classic external scripting is enabled by default (`--disable-scripts` opts out);
ES modules, dynamic script loading and fetch/XHR remain unsupported. CSS, fonts,
images, tabs and resources remain bounded subsets. The modern style-declaration
bridge is not full CSSOM. Cookies are memory-only. Scripted activation of empty
anchors can be rejected by the existing navigation validator; installed layout
checks use ordinary text links.

Linux X11/XWayland is the GUI target. The restricted JavaScript worker is not
a sandbox for the browser as a whole: do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Existing worker
source/opcode/System-allocation/work/time/lifetime/wire and OS limits remain;
they are not whole-browser quotas or GC/RSS measurements. Fatal Boa roots may
require actual Context/worker teardown.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Published tags and assets
remain immutable.
