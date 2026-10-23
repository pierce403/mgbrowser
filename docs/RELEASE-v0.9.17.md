# mgbrowser v0.9.17 Experimental Preview

v0.9.17 adds modern `element.style`
`setProperty`, `removeProperty` and `getPropertyPriority` through the existing
locked Rust Stylo declaration parser and serializer. The actual DOM style
attribute remains the only retained declaration store.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

## Declaration editing methods

CSS names and case-sensitive custom properties use the existing enabled grammar.
`setProperty` replaces both value and importance; an empty value removes even
when the supplied priority is invalid. Invalid nonempty values, unknown
properties and invalid nonempty priorities preserve the original raw attribute.
`removeProperty` returns the previous specified value and removes all shorthand
longhands atomically, including partial shorthands whose serialization is empty.
`getPropertyPriority` returns `important` only for an important declaration or
a shorthand whose complete set of constituents is important.

Genuine same-realm style receivers are checked before arity and argument
conversion. Conversion remains ordered and reentrant; completed inner effects
and thrown-value identity are preserved. All four style methods have stable
per-realm identity and correct public name/length metadata. Native declaration
operations do not call author-overridden methods. Reads and exact no-ops do not
rewrite raw attributes. No dependency, grammar preference, browser protocol,
worker profile or resource allowance is added.
[Frozen contract](https://github.com/pierce403/mgbrowser/blob/main/docs/CSSOM_METHODS.md).

The existing 16,384-byte declaration input/output limits include priority.
Fallible return-string and mutation admission precede DOM commit, including
removal. Ordinary Host errors remain catchable; existing VM fatal behavior
remains terminal. Parser and atom work still rely on unchanged process/System/
CPU containment, not comprehensive cooperative budgeting.

## Validation and delivery evidence

Focused local checks pass all 28 public binding controls, 23 private declaration/
resource controls, three fresh restricted-child cases and one retained
Init/lifecycle/click case. Actual pre-feature missing-method failures remain
recorded. The retained case completes real separate lifecycle tasks and a later
click without renewing its cumulative source/work/System budgets. Independent
source review of the three production files found no outstanding issue.
These are authored controls, not upstream WPT passes or denominator additions.

Final-version full Rust/JSPLAN regressions pass. Complete reviewed WPT selections
retain static338 pass /611 fail /62 unsupported of1,011 and separate Window15/16
files with77/79 actual final subtests passing. No infrastructure failures,
denominator changes or weakened protections. Every static outcome, page/pixel,
relation, diagnostic, request and worker counter matches the accepted prior;
Window final inventories and the two container-unit failures remain unchanged.
Normal local package/install/updater checks and all thirteen existing native/CDP
helpers pass. Separate physical/atomic1x/2x and method-native/CDP proofs pass;
all sixteen actual screenshots were inspected. Both frozen direct-worker suites
retain all21 expected outcomes, including genuine failures/timeouts, with
confirmed owned cleanup. These are authored controls, not upstream passes.
The unchanged updater download ceiling is8 MiB. Exact-source CI/site, tagged
release and public install/update acceptance are recorded separately in the
[dated work log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-23.md).
Local receipts do not establish public delivery or full feature completion.

## Install and known limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer follows the latest actually published release. It verifies
SHA-256 and installs without sudo. Checksums are not independent signatures.
Existing browser processes must restart to use an installed update.

Modern-web compatibility is poor. Google search to first result is not working.
Ordinary Playwright compatibility, full browser event-loop/testharness behavior
and React/Vue applications are not supported. Boa-backed inline and bounded
classic external scripting is enabled by default (`--disable-scripts` opts out);
ES modules, dynamic script loading and fetch/XHR remain unsupported. CSS, fonts,
images, tabs and resources remain bounded subsets. Length/item/indexed
enumeration, style constructors/prototypes, generic expandos, computed/rule
styles and `Element.style` forwarding remain deferred. These methods do not
complete CSSOM, F-003/F-005/F-016/F-026 or the formal MVP. Cookies are memory-only.
Scripted activation of empty anchors can still be rejected; the new fixture
uses ordinary text links.

Linux X11/XWayland is the GUI target. The restricted JavaScript worker is not
a sandbox for the browser as a whole: do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Existing worker
source/opcode/System-allocation/work/time/lifetime/wire and OS limits remain;
they are not whole-browser quotas or GC/RSS measurements. Fatal Boa roots may
require actual Context/worker teardown.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Published tags and assets
remain immutable.
