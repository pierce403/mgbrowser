# mgbrowser v0.9.20 Experimental Preview

Bounded worker-backed main-world Runtime evaluation and object handles.
The website installer follows the latest published release. Publication and
public-install receipts are recorded in the project's dated work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

## Runtime automation foundation

The documented CDP subset adds seven Runtime commands: enable, disable,
evaluate, callFunctionOn, getProperties, releaseObject and releaseObjectGroup.
Execution uses the existing restricted page worker's real globals, DOM and
closures, not parent-process evaluation or a second engine. Own-property
inspection returns complete admitted descriptors without calling getters.
Actual objects remain rooted in that worker until release or realm teardown;
handles are scoped to their context and connection/session owner.

Remote debugging and effective page scripting must both be enabled. Only an
already loaded usable main world is admitted. Runtime notification enablement
does not grant permission or override `--disable-scripts`. Navigation, worker
termination and target closure retire contexts and handles. Tab moves preserve
the actual realm rather than cloning or reloading it.

Automation shares the page worker's single pending slot, native control-edit
versions, parent validation and cumulative resource charges. Valid preceding
effects survive ordinary caught exceptions or unsupported result representation;
fatal or invalid completions retain the prior page. The final transaction can
return completed data but cannot publish handles to a reaped worker.
[Contract and exact admissions](https://github.com/pierce403/mgbrowser/blob/main/docs/RUNTIME_AUTOMATION.md).

This is not full Runtime or ordinary Playwright compatibility. Promise awaiting,
utility worlds, user activation, previews, Debugger and a general DOM-object
bridge are not implemented. `getProperties` requires `ownProperties:true`.
Unsupported semantic options, exotics, symbols and result representations
reject explicitly. The current CDP string codec rejects lone surrogates rather
than changing their value. Exception metadata is deliberately narrowed, without
invented source locations. Target auto-attachment, download lifecycle and stock
locator message sizes remain separate requirements.

## Validation status

Focused typed-realm, parent/broker and actual restricted-child controls pass.
The authored core native proof passes real closure/property/owner-release,
exception effects, native input, newer-edit preservation, paint and navigation
checks. The separate native move proof preserves three real worker roots across
detach/redock and rejects seven foreign/closed-handle operations. All fourteen
actual native/CDP images were inspected. Earlier test-client ordering,
tab-identity and verifier bookkeeping mistakes remain recorded as failures,
not browser passes. All nineteen local Rust/harness gates pass; frozen JSPLAN
retains all 364 outcomes. Both complete, reviewed WPT lanes retain static
339 pass / 610 fail / 62 unsupported of 1,011, and separate Window 15/16 files
with 77/79 final subtests passing. Authored controls do not add upstream WPT
passes or denominators. Normal local package/install/updater checks, thirteen
existing native helpers, eight layout proofs and both frozen worker suites pass.
Both Runtime proofs pass on the packaged executable, with all fourteen actual
images inspected. CI and public-install evidence remains distinct from these
local source/package measurements in the work log.

No existing test, source/opcode/System-allocation/work/time/lifetime/wire/OS
limit, dependency or worker profile is relaxed. Runtime adds narrower admissions
inside the existing profile, not a resource reset or new worker allowance.
The existing 8 MiB compressed updater ceiling remains enforced by packaging;
installation and actual upgrade checks are separate acceptance gates.

## Install and known limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer verifies SHA-256 and installs without sudo. Checksums are not
independent signatures. Existing processes must restart to run an installed
update.

Modern-web compatibility is poor. Google search to first result is not working.
Ordinary Playwright, full browser event-loop/testharness and React/Vue browser
applications are unsupported. Boa-backed inline and bounded classic external
scripting runs by default (`--disable-scripts` opts out); modules, dynamic
scripts, fetch/XHR and general asynchronous resource loading remain unfinished.
Full CSS/CSSOM, intrinsic sizing, baselines, vertical alignment, bidi and floats
remain incomplete. Fonts/images/tabs/resources are bounded subsets. Cookies
are memory-only. This increment does not complete F-011/F-016/F-024/F-026 or MVP.
Real Reddit and Google News loading and expected desktop appearance remain
explicit unfinished reading/visual targets, not benefits claimed for this release.

Linux X11/XWayland is the GUI target. The restricted JavaScript worker is not
a sandbox for the browser as a whole. Do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Existing guards
are not whole-browser quotas or GC/RSS measurements. Fatal Boa roots may require
actual Context/worker teardown.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Old tags/assets stay immutable.
