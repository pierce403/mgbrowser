# Bounded Object.create descriptors

Adopted implementation contract, 2026-09-07; local acceptance completed 2026-09-08. This is an
independently specified language increment, not a site-specific workaround or
a claim that Google will produce results. The published baseline is
c2f07236940e7f1d7b05d6bdc6d50ce50bb102db. No live script is an implementation input.

## Public behavior

Extend only `Object.create(proto, properties)`. Preserve the existing omitted/
undefined second-argument path and typed ordinary/function/native prototype
identities. Validate the prototype, create a fresh ordinary object, then process
a supplied map. Null maps reject; other primitives box. Host prototypes, Host
maps and Host descriptor records remain unsupported, without new Host authority.

Snapshot all typed own keys, including Symbols and nonenumerable keys; exclude
inherited entries. Recheck each current own property's enumerability, then Get
its value once if selected. Convert every selected descriptor before installing
any result properties. Ordinary callback failures retain earlier effects but
return no partial result. This chooses the Symbol-capable collection algorithm,
not ES5's earlier enumerable-string snapshot.
[Object.create and ObjectDefineProperties](https://tc39.es/ecma262/multipage/fundamental-objects.html#sec-objectdefineproperties)

Each descriptor must be a supported object identity. Check inherited presence
and conditionally read fields in order: enumerable, configurable, value,
writable, get, set. Boolean flags use truthiness without coercion callbacks.
Accessors must be callable or undefined; invalid get rejects before reading set.
After the reads, mixed data/accessor field presence rejects, including explicit
undefined. Missing flags default false; missing values/accessors default
undefined. An empty descriptor defines a readonly, nonenumerable,
nonconfigurable data property.
[ToPropertyDescriptor and completion](https://tc39.es/ecma262/multipage/ecmascript-data-types-and-values.html#sec-topropertydescriptor)

The fresh object's own definitions bypass inherited setters/readonly fields.
Subsequent reads/writes preserve the original receiver. Absent getters return
undefined; absent setters ignore non-strict writes. Setters receive exactly one
uncoerced argument and their return is ignored. Writable inherited data shadows;
nonwritable data does not. Configurability controls deletion. Descriptor-map key
order is explicitly local to this algorithm: canonical uint32 indices except
4294967295 ascending, then other strings in creation order, then Symbols in
creation order. Existing for-in/keys ordering approximations do not change.
[Ordinary get, set and own-key operations](https://tc39.es/ecma262/multipage/ordinary-and-exotic-objects-behaviours.html#sec-ordinarysetwithowndescriptor)

Existing names/keys/symbols/hasOwn/in inspection must not execute accessors.
An absent getter is present-property undefined, not a claimed callback result.
Actual getters retain the existing immediate-producer observation. Error
diagnostic formatting treats accessor fields as inaccessible values without
invoking callbacks or changing the first resource failure.

## Deliberate exclusions

No public defineProperty, defineProperties, getOwnPropertyDescriptor or plural
descriptor reflection, accessor literal syntax, Proxy, mutable prototypes,
extensibility APIs or exotic-array descriptor redefinition. Source inspection
corrected a carried-forward note: singular descriptor reflection does not exist
in the baseline and is not a preservation requirement. Function.prototype
callability, general events/timers and external scripts remain separate work.
The result is always ordinary, even with array/function/boxed-string ancestors.
Special property names, including __proto__, do not change its internal prototype.

## Storage and admission contract

Keep public Value, the AST, dependencies, worker permissions and all existing
resource caps unchanged. Existing traversal edge distinctions and fuel ticks on
unaffected paths are regression gates. A new descriptor snapshot is limited to
10,000 own keys, using the existing array-size constant as this explicit new
collection bound; it is not permission to raise any cap.

The selected private representation has one Inline(Value) or boxed accessor-pair
payload and an accessor flag. Canonical states are inline data; inline getter
(callable or undefined) with absent setter; or pair accessor with callable setter
and callable-or-undefined getter. Pair-with-data-flag is invalid. Constructors
and access methods must enforce these states. The existing Host-backed global
getter/paired Host setter is a documented adapter exception, not a writable JS
accessor. Preserve that adapter and Symbol.description without extra allocation.

Independent x86_64 mirror measurements found Value32, key24, two-variant payload32,
pair64 and property64. A three-variant Data/Getter/Pair payload instead measured40
and property72; a direct second Value measured property96. The production types
must be measured independently before acceptance. A setter-bearing pair prepays
its actual 64-byte allocation plus the existing 16-byte allocation allowance.
No pair is allocated for absent setters; no Rc or side-table identity is needed.

New work prepays actual typed-key vector capacity and every copied key buffer,
then an exact-capacity staged property vector sized to the key-count upper bound
before the first map Get. Check the key bound before allocating. Metadata scans,
duplicate handling and index sorting spend shared fuel; do not hold arena borrows
across callbacks. Recheck field presence live rather than copying descriptor
objects. Existing Get/native-name/Host/Symbol admission and real payload-copy
charges remain. Move the already-admitted returned field values into staged
properties; moving them creates no second payload buffer. Prepay a pair before
allocating it. After all conversion succeeds, move the whole staged vector into
the still-unpublished fresh object; do not allocate/copy another result vector.

All accepted charges remain cumulative after failure or deletion. Temporary key
and staging vectors include a 16-byte allowance per allocated backing block;
zero capacity creates no block. Explicit capacity growth prepays its full new
backing allocation, not an unmeasured post-allocation guess. Preserve ordinary
legacy insertion accounting outside this new path; the layout probe separately
found that its Vec minimum capacity can exceed a single 128-byte row credit.
That pre-existing logical-accounting approximation is not proof of RSS coverage
and must not be silently fixed by broadly changing this increment's charges.

Accessor invocation retains charged true callable/receiver copies and prepays
the existing one-argument 64-byte slot before entering a setter. Fatal allocation,
fuel, call/evaluation/prototype depth or worker failure remains latched, bypasses
catch/finally/later scripts, and grants no result/default navigation. No cap,
active-time deadline, stack size or isolation rule is relaxed to pass a test.

## Acceptance gates

Freeze independent semantic/resource sources and a descriptor-required authored
form against the pinned old library/browser before production changes. Include
legacy controls, field order and mutation, all flag combinations, data identity/
UTF-16, typed key ordering, missing halves, bound/native callbacks, receiver and
compound/update behavior, ordinary exceptions and copied versus moved storage.

Private gates cover canonical payload/layout, native callable copies, key/staging/
pair/argument admission before callbacks, Symbol ingress, callback-free diagnostics,
pending function prototypes, all historical traversal edges and first-failure
latching. Public default-stack and real restricted-worker tests must cover both
successful descriptor-built controls and fatal readable fallback without later
effects. Existing tests and the fixed 23-case AST allocator guard stay required.

Run full debug and selected release tests, actual worker/session selftests,
native dependency review, all existing native/external CDP journeys plus the new
form, and retained cancellation checks. Inspect fresh rendered query/destination
frames and reap owned services. Only then make one bounded real Google attempt.
Fixture success is not Google result/destination acceptance. Record local,
published-CI, Pages/HTTPS and visual evidence separately; retain raw live data
only in ignored tmp. Local and live outcomes follow; publication is a separate gate.

## Frozen baseline and component evidence

The independent semantic corpus froze 28 groups before implementation: two legacy
controls pass on c2f0723 and 26 descriptor-dependent groups fail. The resource
corpus froze 10 groups: two legacy controls pass and eight descriptor-dependent groups
fail. Raw old-library results and source hashes are retained in the daily log.
One new recursion assertion was corrected after the first candidate run: the
pinned old runtime, existing tests and an executed old-library control all prove
the existing error is "JavaScript call depth exhausted". Only its mistaken test
literal changed; both the original failure and correction are recorded.

The frozen 1,917-byte form has no static controls. The old restricted worker
reports the explicit unsupported descriptor error, executes no complete script
and creates no form at 69,723 accepted bytes. The candidate completes one script,
creates the actual form and reports no error or rejection at 75,451 bytes:
Bootstrap 26,880 + Source 1,861 + Ast 33,702 + FunctionCode 256 + Runtime 12,752.
Old rejection and new completion perform different work; their totals are not
an equal-work performance comparison. Both fatal accessor-worker variants retain
readable fallback without catch/finally/later effects or navigation.

All 1,193 full-debug tests/51 targets and 1,080 selected-release tests/40 targets
pass, including 28 semantic, 10 resource, 42 DOM, 58 actual-worker and seven new
private storage groups. Production sizes are Value32/Stored32/Property64/
AccessorPair64; Bootstrap26,880 and old create
fuel/cost remain fixed. Private checks isolate snapshot/staging/pair/argument
admission, true callable/receiver copies, moved UTF-16 pointers, exact result
capacity, fallible in-place sorting, invalid internal states and callback-free
diagnostics. The legacy writable Host adapter remains distinct from absent JS
getters without an additional table scan.

The unchanged eight-case actual allocator probe passes every correctness and
whole-realm drop check. Matched candidate operations show a setter adds exactly
64 requested/live bytes and one allocation/block, with an 80-byte logical charge.
Increasing an ASCII key by 448 bytes adds exactly 448 actual/logical bytes;
increasing a UTF-16 Get result by 512 units adds exactly 1,024 bytes. No second
payload copy is observed. These are requested sizes, not RSS or proof of complete
heap coverage. Empty/hidden maps remain observational controls; prebuilt arguments
and native names are consumed inside the window, so live deltas can be negative.

The public [allocation example](../examples/measure_object_create.rs) preserves
the frozen measurement code, adding only a mandatory support assertion and
updated comments/formatting. Its complete candidate output matches the frozen
probe. Compiled against the old library, it exits 101 at that new assertion,
preventing CI from silently accepting unsupported behavior. CI runs it under a
five-second timeout; this is not the general autoresearch executor. The unchanged
23-case AST guard also passes with output identical to the previous checkpoint.

## Browser and live acceptance

All four local CI blocks pass: 25 native and 25 external-CDP destinations,
scripted batches 8/8/7, and retained requests two searches/two destinations/zero
traps. The frozen form submits its real Unicode query and hidden source field,
follows the first authored result, and reaches a separate destination. Its submit
button is unnamed. Fresh native/CDP frames and both canceled retained states were
independently inspected. Actual worker/session selftests, formatting, locked
builds and dependency checks pass; owned processes/listeners are reaped.

One subsequent Google attempt remains incomplete. Homepage HTTP 200 exposes
26 items/one form, five completed scripts/five errors and 2,383,102 accepted
startup bytes without rejection. Two later activations complete and the actual
form submits. Search HTTP 200 has zero items/forms, three completed scripts and
two errors: Ast 377,733 is rejected after 4,135,770 accepted against 4,194,304,
then the next script repeats that first failure. This response has no unsupported
descriptor error, but changing live inputs are not a controlled causal benchmark.
Exit 2 and an inspected blank frame leave the actual first result/destination
unverified. No live source inspection/adaptation, retry, impersonation, raised
cap, new dependency, worker permission or CDP command was introduced. Exact-SHA
remote CI, Pages and HTTPS evidence is recorded separately in the daily log.
