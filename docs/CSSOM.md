# Modern element-style declarations

2026-10-23: shipped as v0.9.14 at
`dfddb067b92bc56a06e721a99b183f56225f6177`, after cab9375 continuous Window
scoring passed exact-source CI, public-report parity and visual verification.
The behavior and authored controls below were frozen before implementation.
The implementation passes full local debug/release regressions and the frozen
JSPLAN comparison. The unchanged upstream Window selection improves from 0/16
files and 0/79 final subtests to 15/16 files and 77/79 final subtests; two real
flex-valid sign()/container-unit calc assertions remain failing. Full raw-data,
resource, HTTP, clock and cleanup review supports a manual 15-file pass ratchet.
Separate exact-source publication and installed-release acceptance now pass;
they are recorded in the [dated log](../memory/logs/2026-10-23.md), not inferred
from those local results. No v0.9.14 release gate remains.

## Behavior and implementation boundary

The scoped browser increment provides parser-backed modern `element.style`
named property reads/writes, `cssText` and callable `getPropertyValue`.
Reuse the exact locked Rust Stylo declaration grammar and serializer already
used by Sparkle, not an eight-property parser, expected-result table, computed
style, new engine or preference override. Parse on demand from the actual DOM
style attribute. Keep no second retained declaration store or new GC root.

Use the live document/base URL, author style-rule/no-quirks context and the
existing enabled property set. Declared values remain specified values, not
resolved layout lengths. Relative URL syntax does not gain fetch authority.
Raw `setAttribute('style', ...)` remains raw; a read cannot rewrite it. A
successful mutation serializes the real parsed block and atomically commits
through existing attribute admission. External attribute changes, removal and
detached elements must remain coherent without cache invalidation.

CSS-name method lookup folds non-custom ASCII case, preserves custom-name
case and does not perform IDL camelCase conversion. Named accessors support
the enabled non-custom CSS/dashed/camelCase spellings and `cssFloat`; custom
properties can be seeded through cssText and queried through getPropertyValue.
Unknown CSS property names passed to getPropertyValue return empty, not
unchecked raw text. Unknown named gets fall back to ordinary target/prototype
lookup (normally undefined), unknown named writes remain explicit unsupported
Host errors and unknown named membership is false unless ordinary lookup finds
it. No generic expando, interface/enumeration or custom named-accessor claim is
made; deferred interface methods are not callable stubs or false membership.

The style `has` trap tests structural membership only (method, cssText or
supported named accessor), not the parser/getter/serializer/result allocator.
Named/cssText get and set validate their actual Proxy receiver arguments,
not merely the trap's private branded target. Method-call receiver validation
uses the existing same-realm weak brand, before the generic convert-first path.

Single-property empty values remove the property and all shorthand longhands.
Invalid values, unsupported properties and invalid whole-value trailing tokens
leave declarations unchanged. Valid writes replace existing values AND their
importance, including a same-value write over !important. Shorthand parsing,
replacement/removal and reconstruction are generic/atomic. cssText instead
parses a complete replacement list, retaining valid siblings, so an entirely
invalid list empties it. cssText assignment updates the owner even for equal or
empty serialization, including creating an explicit empty style attribute when
previously absent. Do not use parser-cascade `push/extend` for unconditional
CSSOM replacement.

Choose CSSOMString = USVString: real ToString followed by existing UTF-8 scalar
replacement of lone surrogates. getPropertyValue requires one argument:
missing is TypeError; explicit undefined and null are strings. Validate genuine
style receiver/kind/arena before arity/conversion, including extracted/bound
method calls. Reject forged/inherited/author-Proxy/wrong-kind/foreign receivers
without invoking receiver getters or argument conversion. Ignore extra arguments.

Named setters use LegacyNullToEmptyString: null removes; undefined is converted
normally and may be invalid CSS. cssText does not use that annotation. Preserve
thrown conversion identity and completed inner effects; read the current DOM
only after conversion. Hold no State/DOM borrow across callback-capable conversion
and recheck the existing fatal policy before parsing/return/commit. Native
declaration operations must not dispatch through page-overridden JS methods.

## Resource and security boundary

Keep current 16,384-byte attribute, 4 MiB cumulative DOM, 50,000-node, depth256,
32/64 MiB requested-System, source/opcode/work/session/active/CPU/wire limits
and isolation exactly unchanged. Bound current raw attribute, incoming CSS/name
and canonical output at the existing attribute byte ceiling; oversize/storage
failure is a real ordinary Host error, not successful invalid-CSS no-op or empty
lookup. No truncation or partial replacement is permitted.

New canonical retained String capacity excess must be charged in addition to
the existing attr len charge, before replacement. New serializer/read-result
and UTF-16 result capacities need actual cumulative admission before transfer.
This does not retroactively claim exact capacity accounting for the old generic
DOM arena. Stylo's temporary parser/atoms/concrete String writer remain native
work under actual System/CPU/address-space/parent-active containment; they are
not cooperatively opcode-metered or protected by a new preallocation CSS quota.
Do not prewarm atoms outside the restricted child or touch StyleThreadPool,
Device/cascade/font services or add syscalls, dependencies, tasks or allowances.

Keep ordinary Host errors catchable and existing attempted DOM charges
unrefunded. Do not introduce a blanket CSSOM fatal latch. Existing VM fatals
stay terminal/unmaskable; allocator exit remains process failure, not a CSS
exception. All fallible admission must precede semantic DOM mutation.

## Explicitly deferred

Public setProperty/removeProperty/getPropertyPriority, length/item/indexes,
ownKeys/define/delete/enumeration, CSSStyleDeclaration/CSSStyleProperties
constructors/prototypes, generic expandos, computed/rule styles and Element.style
PutForwards are not implemented or claimed by this increment. Preserve existing
define/delete refusal and the explicit unsupported style assignment. Standard
style assignment forwards to cssText; calling that behavior simply readonly
would be inaccurate. Legacy raw serialization and all original assertions stay.

## Frozen authored controls and delivery gates

The pre-implementation twenty public controls are in
[boa_cssom.rs](../crates/mg-sparkle/tests/boa_cssom.rs). They cover callable/arity,
receiver/coercion order, null/undefined/scalar conversion, CSS names versus named
accessors, custom case, atomic invalid/empty/shorthand/importance behavior,
attribute coherence (including explicit empty replacement on an absent owner),
reentry and weak/detached identity. Nine private controls in
`src/js_browser/boa/cssom_resource_tests.rs` use the existing host-quota seam;
three `tests/boa_worker.rs` controls start fresh restricted children for first
parse, nonstatic custom atoms and URL syntax. No parent prewarming or new
inspection capability is introduced. Record pre-feature missing-API failures
before implementation. Preserve old observer16+output5, all static1011/Window16+79,
all resource/security/legacy/default/navigation assertions. Authored controls
add no upstream result denominator or pass.

After real restricted-child first parse/custom atoms/URL/resource and debug/
release regressions pass, run both full unchanged WPT lanes, review every changed
row/pixel/resource/diagnostic and manually strengthen only proven ratchets.
Then ordinary versioned release, exact CI/site, <=8MiB package, public installation/
reinstall, actual prior-version update/no-op and all existing native/CDP gates.
No predicted upstream pass count. Full CSSOM, F-016/F-026 and MVP stay open.

Initial focused debug validation passes9 declaration,20 public binding,
9 private quota/fatal and3 fresh restricted-worker controls. Genuine first
parses, nonstatic atom initialization and URL serialization use the unchanged
worker profile. Fresh requested-System peaks are347644/413867/351954 bytes,
with cumulative423273/508714/441265 bytes respectively, not GC/RSS values.
The owned process receipts confirm normal exits/cleanup. A retained-session
click control also passes: real Init/load/click retain declaration state and
change width12px to27px, with cumulative work2/0/2→2/2/0→2/2/0 and monotonic
actual System counts. Full debug/release workspace, no-chrome embedding and
frozen364-outcome JSPLAN gates pass separately. One earlier existing fatal-
rejection control hit EOF; the unchanged isolated and fresh full runs pass.
Its first failure remains recorded without claiming a proven cause or raising
limits. Authored results add no WPT passes.

Completed delivery: all eight exact-source CI/hosting checks and the tagged
release workflow pass. The 8,098,810-byte public archive fits the unchanged
8 MiB ceiling, with verified SHA-256
`72044de1e34ab3bcd41a4669bdbcc02bdc7351e5efb8428a72ff35672cf45c42`.
The exact public curl install/reinstall, installed CSSOM behavior, version/About,
worker/session selftests, desktop/icons, actual v0.9.13 update/no-op, both frozen
public-worker suites and all thirteen public native/CDP helpers pass. Separate
current-source static scoring remains 317/632/62 of 1,011; Window scoring remains
15/16 files and 77/79 actual final subtests. These are not installed-binary or
full-WPT scores. Full CSSOM and F-016/F-026/MVP remain open; the next feature
requires separate review before implementation.

Primary review: CSSOM draft 2026-10-13 sections3/6.6.1/7.1,
https://drafts.csswg.org/cssom/ ; WebIDL living standard 2026-10-19 sections
3.2.10/3.2.12/3.7.6/3.7.7, https://webidl.spec.whatwg.org/ .
