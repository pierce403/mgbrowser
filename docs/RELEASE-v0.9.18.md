# mgbrowser v0.9.18 Experimental Preview

Bounded scripted-link correction. Per-commit delivery evidence is recorded in
the dated work log; authored controls are not upstream WPT passes.

v0.9.18 corrects scripted activation of painted empty, whitespace-only and
non-text anchors. The parent now resolves the clicked anchor's own post-handler
href against the reconstructed document base instead of requiring a rendered
text link.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

## Scripted link defaults

The same stable anchor must remain connected with a present resolvable href.
An empty href is distinct from a missing attribute. Handler text removal,
hiding, reparenting or detach/reinsert need not suppress the original anchor;
a replacement with the same HTML id does not inherit its activation identity.
Existing direct and Promise callback effects precede default selection.

The selected destination must be credential-free HTTP(S), at most 16,384
serialized UTF-8 bytes. URL validation precedes acceptance of DOM edits,
control-edit acknowledgements, revision/epoch and observer publication. A forbidden selected
target or failed active-time charge retains the previous page. Accepted
navigation consumes that checked destination without a post-commit lookup.

Cancellation, permanent detachment, href removal and terminal worker behavior
still suppress link defaults. Valid Ready replies with no default can commit
their DOM changes. Explicit script navigation retains precedence after default
identity and href eligibility checks. Forms, callback/checkpoint semantics and
the existing navigation hop limit are unchanged. No dependency, worker profile,
network capability, CDP command or resource allowance is added.
[Scoped contract](https://github.com/pierce403/mgbrowser/blob/main/docs/LINK_DEFAULTS.md).

## Validation and publication status

The recorded v0.9.17 public binary reproduces the original failure after actual
native ready/canceled paints. Focused debug/release controls and separate local
development-binary native/CDP checks at 1x and 2x subsequently pass, including
handler-selected destinations and zero trap requests. These are authored local
controls, not packaged/public v0.9.18 delivery or upstream WPT passes.

Full final-version debug/release Rust and separate JSPLAN regressions pass,
preserving all364 frozen research outcomes. Both complete, reviewed WPT lanes
remain unchanged: static338 PASS /611 FAIL /62 UNSUPPORTED of1,011; separate
Window15/16 passing files and77/79 passing final subtests. There are no
infrastructure failures or new upstream passes. Existing manual pass/runnable
protections, corpus and resource limits are unchanged.

Normal package/install/updater checks, all thirteen existing native helpers,
six additive native/CDP proofs and both frozen worker suites pass locally.
All 28 additive screenshots were inspected. Exact-source CI/site, tagged
publication and public install/reinstall/update verification remain separate
release gates. Their actual
evidence belongs in the
[dated work log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-23.md).
Historical v0.9.17 delivery receipts are not v0.9.18 release evidence.

## Install and known limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer follows the latest actually published stable release. It verifies SHA-256 and installs
without sudo. Checksums are not independent signatures. Existing browser
processes must restart to use an installed update.

Modern-web compatibility is poor. Google search to first result is not working.
Ordinary Playwright compatibility, full browser event-loop/testharness behavior
and React/Vue applications are not supported. Boa-backed inline and bounded
classic external scripting is enabled by default (`--disable-scripts` opts out);
ES modules, dynamic script loading and fetch/XHR remain unsupported. CSS, fonts,
images, tabs and resources remain bounded subsets. Full CSSOM, including indexed
enumeration, style constructors/prototypes, generic expandos, computed/rule
styles and whole-style forwarding, remains unfinished. Cookies are memory-only.

This link correction does not implement complete download, target, referrer or
disconnected-anchor behavior, synthetic click APIs or broader DOM compatibility.
It does not complete F-003/F-004/F-008/F-016/F-026 or the formal MVP. The existing
updater download ceiling remains 8 MiB.

Linux X11/XWayland is the GUI target. The restricted JavaScript worker is not
a sandbox for the browser as a whole: do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Existing worker
source/opcode/System-allocation/work/time/lifetime/wire and OS limits remain;
they are not whole-browser quotas or GC/RSS measurements. Fatal Boa roots may
require actual Context/worker teardown.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Published tags and assets
remain immutable.
