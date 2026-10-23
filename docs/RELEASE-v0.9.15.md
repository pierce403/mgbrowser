# mgbrowser v0.9.15 Experimental Preview

v0.9.15 adds physical `justify-content:left/right` placement to the existing
bounded flex/grid layout adapter. This increment uses the already locked Rust
Stylo and Taffy crates; it adds no dependency, worker profile, browser protocol
or resource allowance. The full browser, CSS and WPT milestones remain open.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

## Physical content justification

- Horizontal LTR flex rows keep the group against its physical left/right edge,
  including `row-reverse`; reversing children does not reverse those edges.
- In columns, both physical keywords pack at the top. Grid uses its inline axis
  and ignores an irrelevant authored `flex-direction`.
- Normal ordering, auto margins, wrapping, gaps, box edges and overflow clipping
  keep their existing behavior. Non-formatting block/table properties remain
  inert. The same resolver serves validation and actual layout conversion.
- Forward flex and grid preserve explicit `safe` alignment. Physical `safe`
  values in reversed flex contexts produce a specific unsupported diagnostic
  until their overflow fallback can be represented correctly. RTL, vertical
  writing and unrelated unsupported alignment remain outside this increment.

The [frozen contract](https://github.com/pierce403/mgbrowser/blob/v0.9.15/docs/PHYSICAL_JUSTIFY.md)
records the source/specification review, original failures and precise boundary.
This is not full CSS Box Alignment or bidi support.

## Evidence and release status

The fourteen private adapter and nineteen public geometry/pixel/hit controls
were frozen before implementation. Actual old-source runs recorded nine and
fifteen assertion failures respectively; focused debug/release runs now pass
all controls and selected existing layout regressions. They are original
authored controls, not upstream WPT results.

Final-version full debug/release regressions, optional-chrome embedding,
dependency/tooling checks and all364 frozen JSPLAN outcomes pass. The complete
independently reviewed static selection reports319PASS/630FAIL/62UNSUPPORTED
of1011. Only the original left/right-002 column cases gain PASS, each with zero
different pixels against its original reference. All old317 passing and949
runnable cases and the first24 passes are preserved; the manually strengthened
ratchet protects319 passes. Five other initially targeted cases still fail real
pixel comparisons. Corpus, scorer, viewport and resource limits are unchanged.

The separate complete Window lane remains15/16 passing files and77/79 actual
final subtests; its two genuine flex-valid assertions remain failing. Neither
denominator includes authored controls. Normal packaged installation/updater,
all thirteen existing native helpers, the separate physical-layout native/CDP
proof and both frozen worker-control suites pass locally. Exact-source CI/site
and public delivery require separate verification receipts.
Consult the [dated log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-23.md)
and published release receipts for their actual results. Local measurements alone
do not establish binary delivery.

## Install and limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer selects the latest actually published release, verifies SHA-256
and installs without sudo. Checksums are not independent signatures. Existing
browser processes must restart to use an installed update.

Modern-web compatibility is poor. Google search to first result is not working.
Ordinary Playwright compatibility, full browser event-loop/testharness behavior
and React/Vue applications are not supported. Boa-backed inline and bounded
classic external scripting is enabled by default (`--disable-scripts` opts out);
ES modules, dynamic script loading and fetch/XHR remain unsupported. CSS, fonts,
images, tabs and resources remain bounded subsets. The modern style-declaration
bridge is not full CSSOM. Cookies are memory-only.
Scripted activation of empty anchors can be rejected by the existing default
navigation validator; the installed layout check uses ordinary text links.

Linux X11/XWayland is the GUI target. The restricted JavaScript worker is not
a sandbox for the browser as a whole: do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Existing worker
source/opcode/System-allocation/work/time/lifetime/wire and OS limits remain;
they are not whole-browser quotas or GC/RSS measurements. Fatal Boa roots may
require actual Context/worker teardown.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Previously published tags
and assets remain immutable.
