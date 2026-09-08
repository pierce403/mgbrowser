# Bounded Array callback family

Locally accepted, 2026-09-07; publication evidence is recorded separately. This is
generic language and DOM collection compatibility work toward the Google journey,
not a diagnosis of the observed undefined binding. No website source is an
implementation input.

## Behavior

Implements the seven ES5-shaped methods forEach, map, filter, some, every, reduce
and reduceRight. The first six formerly had native properties but threw
unsupported errors; reduceRight adds a genuine nonenumerable native property.
Reference: [ES5.1 Array callback algorithms](https://262.ecma-international.org/5.1/#sec-15.4.4.16).
All seven have length 1 and use the existing native identity/metadata rules.

Reject a nullish receiver, otherwise use the existing ToObject behavior. Read
length once, perform fallible ToNumber followed by ES5 ToUint32, then validate the
callback even for an empty traversal. After those semantic steps, reject converted
length above the existing 10,000-element bound before visiting any index. Do not
clamp or use apply's existing length approximation. NaN/infinities become zero;
negative integers wrap, and values above 2^32 wrap as specified. This is an
explicit bounded traversal policy, not support for arbitrarily large array-likes.

Capture the length, not the values or presence. At each index perform current
HasProperty and, if present, current Get exactly once, including inherited data
and getters. Iterate ascending except reduceRight. Holes are skipped; an explicit
undefined is present. Mutations to later in-range indices are observed; appended
out-of-range entries are not visited. No arena borrow survives a getter/callback.

Non-reduction callbacks receive exactly value, numeric index and the original
object (the single boxed receiver for primitives). Preserve the supplied thisArg;
use undefined if omitted. Reductions receive exactly accumulator, value, index
and object, with undefined this. Dispatch through the existing call machinery,
including bound/native/Host callbacks, ordinary non-strict substitution, lexical
state, errors and cumulative recursion/fuel guards. Extra input arguments are
ignored without additional coercion. No synthetic wrapper source or replay.

forEach returns undefined. some/every use callback result truthiness without
coercion hooks and stop at the first decisive result; empty results are false/true.
map returns a fresh intrinsic array with captured length and preserved holes;
each visited position gets the callback result as a new own slot. filter returns
a dense intrinsic array containing the value read before its callback when that
callback is truthy, even if the source is changed during the call. New results do
not read constructor/species or route writes through inherited setters.
reduce/reduceRight distinguish omitted initial value from explicit undefined;
without an initial value the first present element seeds the accumulator, and no
present element throws TypeError. A callback exception stops the traversal and
propagates the original value/context. No partial result is returned.

## Indexed Host capability

Add a default-failing Host::has_indexed_property(object, index) hook used only by
this indexed traversal. Existing hosts remain source-compatible and must never
infer presence from get returning undefined. A missing/zero-length Host receiver
performs no indexed hook, matching generic empty traversal. Do not probe index
zero eagerly or add a capability read before normal semantic ordering.

BrowserHost answers this hook only for its existing collection snapshots,
validating the canonical collection handle and retained arena entry, then
checking the index against snapshot length without allocation or Get. Detached
nodes remain in their snapshot, and later new collections are distinct. Unknown,
malformed or stale handles reject rather than return fabricated absence.
The runtime still charges Host Get ingress and actual returned handle copies.
This adds no Host enumeration/prototype API, worker messages, network authority,
dynamic NodeList behavior, or synthetic collection.forEach property.

## Storage and failure rules

Keep every existing realm, parser, worker and protocol cap. Each callback argument
vector is admitted before allocation: 192 bytes for three slots, 256 for four.
Actual callee/receiver/thisArg/payload copies remain charged. Move owned accumulator
and map results where no copy occurs. filter must retain its pre-callback value
and pay for the actual independent callback copy; never reread a source getter.

map admits its full captured result slots and metadata after callback validation,
before indexed effects. filter admits its empty result metadata there, then uses
the existing bounded geometric builder for selected values after their callbacks.
No result identity is exposed during construction. Allocation failure retains
earlier effects and accepted orphan storage, returns no partial result, latches
the realm and bypasses catch/finally/later work as before. Pure traversal/results
do not add a collector, refunds or an allocation-report phase.

The new reduceRight property adds 156 measured Bootstrap bytes under existing
accounting. This was measured before updating any old exact assertion. Keep
pre-change reports intact as evidence, review every affected checkpoint, and
retain real copy/resource negative controls. The old empty-map unsupported assertion must
become an explicit successful-map capability check while preserving an independent
unsupported-operation exception check. Do not weaken unrelated assertions.

## Acceptance

Freeze independent semantic cases and an authored callback/DOM-built form against
the old library/actual restricted worker before candidate builds. Cover all seven
methods, generic receivers, UTF-16, holes/inheritance, live mutation, callbacks,
short circuits, reductions, error order and collection borrowing. Separately cover
allocation/copy/slot admission, length/fuel/depth limits, fatal latching and Host
presence/Get ordering. Keep the original test inputs and resource controls.

Require formatting, all-target debug and selected release tests, dependency guard,
actual worker/session containment checks, and all existing native/CDP journeys
plus the new real form. Inspect query/destination frames and actual requested
URLs. Only then run one bounded live Google checkpoint; the full first-result and
destination goal remains open unless actual live results and navigation prove it.

Separate unadopted findings: static AST operator tags could remove real duplicated
buffers; core primitive prototype payloads/constructor backlinks are absent;
Function.prototype callability and generic indexOf behavior need their own designs.
None is silently bundled into this family or asserted as Google's root cause.

### Verified local result : 2026-09-07

The frozen 35-group semantic suite produced 34 expected failures on c858a6f and
passes unchanged on this implementation. All 19 independent resource groups,
nine private ownership/getter/depth groups, canonical Host-presence checks and
new DOM, retained-event and actual-worker cases pass. Bootstrap is 26,155, exactly
the original 25,999 plus 128 property bytes, 11 key bytes and 17 native-name bytes.
Eleven old checkpoint files retain explicit old + 156 arithmetic; their other
phases, real-copy controls and 21,462/21,120 fuel-loop checkpoints remain unchanged.
Registration does consume
one real initialization fuel step; unchanged loop counts do not mean fuel neutrality.

The unchanged authored form previously stopped at unsupported Array.map with no
controls. It now completes one script without errors, creates its real form and
retains Verified labels at 93,783 accepted bytes: Bootstrap 26,155 + Source 1,722 +
Ast 30,368 + FunctionCode 1,152 + Runtime 34,386. No rejection or proposed navigation;
the actual restricted child exits and is reaped. All 1,041 debug tests across 43
targets and 928 selected release tests across 34 targets pass, excluding a repeated
owned-child summary from each count. Formatting, dependency guard, worker/session
selftests and all four exact CI journey blocks pass locally: 22 native and 22
external CDP journeys, with query/destination and cancellation frames inspected.
Retained-event server evidence remains two searches, two destinations, zero traps.

One subsequent Google attempt still submits its actual form but returns no
actionable results. Homepage HTTP 200 has 26 items/one form and five completed
scripts/five errors; two retained activations complete. Search HTTP 200 has zero
items/forms and two completed scripts/three errors. The first still resolves a
method call on undefined from a binding; later Source 26,636 is rejected after
4,192,169 accepted bytes against 4,194,304, repeated by the next script. Exit 2 and
the inspected blank frame leave the first-result/destination goal incomplete.
This is not a controlled live benchmark or a diagnosis of the earlier TypeError.
No live source inspection, adaptation, retry, cap increase, new dependency,
worker permission or CDP command was introduced. Raw evidence remains in ignored
tmp/; the daily log records original hashes and separate publication verification.
