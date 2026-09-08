# Core constructor and primitive-prototype compatibility

Locally accepted, 2026-09-07. This addresses confirmed
gaps in our own core runtime, not an inferred origin of Google's undefined binding.
The live first-result/destination goal remains the acceptance objective.

## Coherent scope

Initialize real own constructor backlinks on Object.prototype, Array.prototype,
String.prototype, Number.prototype and Boolean.prototype. Each initially refers
to its original native constructor, is writable/configurable and nonenumerable,
and uses ordinary stored-property mutation/deletion. Changing a global constructor
must not silently rewrite an existing backlink or the runtime's intrinsic parent.
Preserve Object.prototype's null parent, Array.prototype's empty indexed storage,
all existing intrinsic identities and the constructor .prototype descriptors.

Initialize the existing own boxed-value slots of String.prototype to the empty
UTF-16 string, Number.prototype to positive zero and Boolean.prototype to false.
These are genuine branded primitive objects, not special name-based conversions.
Existing valueOf/toString, Object branding, coercion and String length reflection
must work through those payloads. Merely inheriting from a primitive prototype
does not give an ordinary object its own primitive brand/payload; incompatible
receivers still reject before coercion callbacks. Symbol.toStringTag remains the
existing presentation hook, not authority to forge a primitive payload.

Support new Number and new Boolean, directly and through existing bound functions.
Each creates a distinct object using the original intrinsic prototype. Number
defaults to positive zero when its argument is omitted; explicit undefined becomes
NaN. Otherwise perform the existing fallible ToNumber exactly once, preserving
negative zero, infinities and errors. Boolean uses ToBoolean without valueOf,
toString or Symbol.toPrimitive callbacks; omitted/undefined become false, objects
(including boxed false/zero) are true. Evaluate all supplied argument expressions
normally, then ignore extra values without additional coercion. Constructor
conversion precedes instance metadata allocation; errors return no partial object.

Bound construction preserves existing prefix/outer argument ordering, original
target delegation, ignored bound receiver, genuine instanceof behavior, limits
and rejection of nonconstructible natives. Add Number/Boolean to the supported
eligibility set, not every native. Preserve ordinary calls to the five constructors,
String construction (including its Symbol distinction), Object boxing, Array
construction and their existing real-copy policies.

References: [ES5 built-in property defaults](https://262.ecma-international.org/5.1/#sec-15),
[Object prototype](https://262.ecma-international.org/5.1/#sec-15.2.4),
[Array prototype](https://262.ecma-international.org/5.1/#sec-15.4.4),
[String prototype](https://262.ecma-international.org/5.1/#sec-15.5.4),
[Boolean constructor/prototype](https://262.ecma-international.org/5.1/#sec-15.6.2),
[Number constructor/prototype](https://262.ecma-international.org/5.1/#sec-15.7.2).
The existing interpreter remains a bounded, deliberately partial ES5-shaped subset.

## Storage and failure boundary

Keep every existing cap, phase, source/parser charge, real ingress/copy cost,
worker permission and protocol message unchanged. The five new stored backlinks
are expected to add 725 Bootstrap bytes: five times property128/key11, plus native
names6/5/6/6/7. Three initial scalar/empty-string payloads use existing object slots
and require no new buffer. Verify actual layout and accounting before accepting
this estimate or changing old assertions. Five real put_own calls consume fuel;
subsequent prototype searches may inspect added properties. Do not claim identical
fuel use or mechanically adjust completed-loop counters.

Each new Number/Boolean instance must pay the existing object metadata before
allocation/publication; no refund or collector. Actual coercion callbacks, bound
argument slots, foreign values and existing boxing copies retain admission. An
ordinary conversion error remains catchable, while fuel/depth/allocation failure
latches the realm and suppresses catch/finally/later work as before. Test both
conversion-side effects before rejection and rejection before object publication.

## Independent acceptance

Pin the current b3f8fd3 library and binary. Freeze authored semantic cases and a
real form requiring recovered constructor links, primitive prototype values and
direct/bound primitive construction before any candidate build. Run old baselines
once, preserve their original errors and allocation reports, then require the
unchanged cases to pass on the candidate. No website scripts are fixture inputs.

Cover all five backlinks, own reflection/attributes/mutation, global reassignment,
intrinsic parents, branding versus inheritance/tagging, UTF-16, omitted versus
undefined, negative zero/NaN, coercion order/exception identity, extra arguments,
bound construction/instance checks, and lifetime across executions/events.
Separate resource checks cover measured Bootstrap, real copies and metadata,
near-cap conversion/instance ordering, cumulative failure and default-stack depth.

Review the old tests explicitly pinning unsupported Number/Boolean construction:
replace only that limitation with positive capability assertions, retaining a
different unsupported native and existing noncallable Function.prototype checks.
Freeze/measure any old phase or fuel checkpoint affected by genuine added storage
or prototype traversal; keep original sources and semantic/resource controls.
Require format, full debug, selected release, dependency guard, actual worker and
session checks, all existing native/CDP journeys and the new real form. Inspect
frames and actual requested URLs. Only then run one bounded live Google attempt.

Function.prototype callability requires a separate typed callable-identity design
and is not silently emulated here. Static AST operator storage, number formatting,
additional methods/descriptors, external scripts/timers and broader CDP remain
separate work. Neither fixture success nor a changed diagnostic completes Google.

## Measured component evidence

The unchanged semantic suite passes all 33 groups after showing 27 intended
failures and six passing preservation controls on b3f8fd3. Seventeen independent
resource groups and eight private groups pass. The frozen actual-worker form
changes from zero scripts/forms and Original constructor backlink required to
one script, zero errors and one real form. Accepted storage is 84,741 bytes:
Bootstrap 26,880 + Source 2,131 + Ast 35,889 + FunctionCode 448 + Runtime 19,393.

Actual Bootstrap addition is exactly 725 bytes and five property-write fuel
steps. Object remains 112 bytes, and each scalar instance pays 128 bytes after
conversion. Thirteen old checkpoint files retain historical baselines plus this
measured allowance; all non-Bootstrap phases, real-copy controls and the 21,462 /
21,120 completed-loop assertions pass unchanged. Only the explicit Number/Boolean
construction exclusions were replaced with positive capabilities; unrelated
nonconstructible natives and noncallable Function.prototype remain tested.

All 1,104 debug tests / 45 targets and 991 selected release tests / 36 targets
pass, excluding two repeated owned-child summaries per profile. Format, locked
build, dependency guard, frozen probes and actual worker/session selftests pass.
Native/CDP, live-site and publication results are recorded separately below and
in the daily log; component success is not the full Google gate.

## Browser acceptance and remaining live gate

All four corrected CI journey blocks pass locally: 23 native / 23 external CDP
journeys, with scripted batches 8/8/5 and retained requests 2 searches / 2
destinations / zero traps. The unchanged fixture's real Unicode form submits and
its first local result reaches a separate destination; frames were inspected.
The first run's final CDP launch hit a log-creation race. Five narrow readiness
guards and independent shell cases fixed the harness without changing deadlines,
browser code or assertions. Both original attempts and recoverable artifacts
remain preserved separately; see the daily log for exact failures and hashes.

One subsequent live Google attempt still exits 2: homepage and real form
submission succeed, but search HTTP 200 renders zero items/forms. Its first
reported failure is now Runtime admission of 131 bytes after 4,194,282 accepted
bytes against the unchanged 4,194,304 cap, latched into later scripts. The earlier
undefined-binding diagnostic was not reported in this response; changing served
inputs are not a controlled comparison or proof of its root cause. No actual
result/destination or new required live stage completed. No source adaptation,
impersonation, alternate engine/service or raised limit was used. Publication is
verified separately; F-008/F-010 and the persistent Google goal remain open.
