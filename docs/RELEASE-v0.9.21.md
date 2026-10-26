# mgbrowser v0.9.21 Experimental Preview

Native-host-backed CDP target creation, activation and closure. Exact-source
publication and public-install receipts are recorded in the dated work log.

Linux x86_64 / X11 or XWayland, glibc2.35 or newer. Rust1.91.1, locked
dependencies and unchanged Rust-only implementation backends.

## Native target control

The browser-level CDP endpoint adds `Target.createTarget`, `activateTarget` and
`closeTarget`. Creation produces a real independently owned tab, not a protocol
placeholder. Empty/about:blank URLs create an empty document rather than the Mg
welcome page; HTTP(S) URLs start normal navigation only after admission. Only the
`url` parameter is supported. The new tab shares normal preferences/network
session and gets its own script service.

Activation selects the named real tab/pane/window and requests native X11
raising/input focus, subject to desktop window-manager policy. Closure removes
only that tab, retires its worker and invalidates its direct/session routes
before success. Closing the final native tab through this protocol command
explicitly rejects in this subset; ordinary native final-window closure still
exits. Old identities are never reused or silently retargeted.

Commands require the browser endpoint without an outer sessionId. Unsupported
options and stale IDs fail explicitly. Embedders must opt into the typed
single-use native host handoff; ordinary page-only ticks reject mutations.
See [the contract](https://github.com/pierce403/mgbrowser/blob/main/docs/TARGET_LIFECYCLE.md).

## Validation status

Pre-implementation assertion failures are retained. Focused controls and all20
final-version Rust/harness gates pass, including debug/release, no-chrome
embedding and actual worker regressions. All8 frozen JSPLAN gates retain all364
research outcomes, not364 passes. The first packaged native target proof passes;
all eight actual images were inspected.

Normal package/checksum/installer/updater and worker/session selftests pass,
as do all13 existing native helpers, both retained Runtime proofs and all8
layout proofs. All14 Runtime and36 layout images were inspected. The original
26 native/26 external journeys, three storage measurements and both frozen
direct-worker and separate Browser-host16+5 control suites pass. Expected
failures remain explicit; authored control success is not WPT conformance.

Both complete reviewed WPT lanes retain static339PASS/610FAIL/62UNSUPPORTED
of1,011 and Window15/16 files,77/79 final subtests. All prior protections and
the two genuine Window flex/sign-calculation failures remain. Authored tests
add no upstream WPT passes. Source CI/hosting, tagged publication and the public
installer/update are separate gates: see the dated release receipts, not an
inference from these local results.

Published at `321e1e87e47e76102d5cc3a4d010acc24d3eff72`, with all eight required
exact-source CI/hosting checks and tagged release workflow40346494949 passing.
The checksum-verified public archive is8,253,866 bytes, SHA-256
`6aacdf6e9ed4bab922610953194a90d08e8971b7d157b0e7f350290e8d2173a9`.
Public install/reinstall, actual v0.9.20 upgrade/no-op, worker/session/Boa/CSSOM,
desktop/icons, thirteen existing native helpers, both Runtime proofs, the Target
proof, eight layout proofs and both frozen direct-worker suites pass. All58
additive public images were inspected. See the
[public closeout](../memory/logs/2026-10-26.md#v0921-public-release-closeout)
for retained-data reviews and their explicit observability limits. The unrelated
high-severity undici build-tool dependency alert remains unresolved.

## Install and known limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer follows the latest published release and verifies SHA-256 before
installing without sudo. Checksums are not independent signatures. Existing
processes must restart to use an installed update.

Modern-web compatibility is poor. The Google search to first result journey is
not working. Real Reddit and Google News loading and expected desktop appearance
remain unfinished goals. Ordinary Playwright is unsupported: autoattachment,
debugger waiting/resume, downloads, utility worlds, promise awaiting and stock
locator message-size policy remain open. `Target.activateTarget` does not
implement the separate `Page.bringToFront` command.

Boa JavaScript is enabled by default (`--disable-scripts` opts out); the DOM,
classic external scripts and browser event loop remain incomplete. Modules,
dynamic scripts, fetch/XHR and general asynchronous resources are not supported.
Full CSS/CSSOM, intrinsic sizing, baselines, bidi and floats remain incomplete.
Fonts/images/tabs/resources are bounded subsets; cookies are memory-only. This
increment does not complete F-011/F-016/F-024/F-025/F-026/F-027 or formal MVP.

Linux X11/XWayland is the supported GUI target. The restricted JavaScript worker
is not a sandbox for the browser as a whole. Do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Existing
limits are not whole-browser quotas or GC/RSS measurements. No dependency,
worker authority or resource profile is widened for this increment.

Project code, documentation and original artwork use Apache-2.0. Dependencies
retain their licenses in packaged THIRD_PARTY_LICENSES.txt. Old tags/assets stay
immutable.
