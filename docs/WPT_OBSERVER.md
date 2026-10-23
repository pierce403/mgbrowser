# Bounded Window testharness observer contract

2026-10-22: reviewed implementation contract, **not implemented or accepted**.
This is the next non-scoring part of [Window readiness](WPT_TESTHARNESS.md),
following v0.9.11's queued snapshot lifecycle. This contract and the
[16 exact authored controls](WPT_OBSERVER_CONTROLS.json) are frozen before
implementation. They do not change the 1,011-case
static selection, runner, expectations, score, resource profile or MVP gates.

## Inputs, evidence and trust

Use the complete, unchanged upstream files at revision
`84daed4ae966d9624bed22f43587c79283ad1fea`:

- `testharness.js`: 198,291 bytes, SHA-256
  `d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e`.
- `testharnessreport.js`: 1,231 bytes, SHA-256
  `3777176fc736b0a78cf7617b9613b220f635daae7536ec7ebb99c61dbb525aae`.

The latter documents the vendor `add_result_callback` /
`add_completion_callback` seam; it is not itself an Mg reporter. No assertion
rewrites, Shell substitution, evaluated reporter glue, console/DOM scraping,
remote-context support or new scored cohort is authorized by this increment.

The verified public v0.9.11 executable ran the historical complete-harness input
through real DCL/load and one later task, followed by a fixed one-second normal
poll window. Its final snapshot recorded 10,026 opcodes, 18,664,637 cumulative
requested-System bytes, 6,972,950 peak outstanding bytes, and work
5 admitted / 3 executed / 1 pending. No diagnostic or fatal reason was emitted.
Exact inputs/requests and owned cleanup were verified. Receipt SHA-256:
`327b5f8947bbd0d4fff46d458accc87dea11d319d586fd64b12cde5430eb186c`
in `tmp/oct22-v0911-current-public-harness-a/report.json`.
This is a lifecycle/resource **prefix**, not typed completion, a WPT result,
complete feasibility, RSS/GC heap or a final post-encoding allocation peak.

Enable observation only in a trusted owned control host. Before starting a
worker, that host verifies actual HTML and every admitted source against the
frozen control inventory, including complete bootstrap bytes/hash. URLs,
comments, page globals, replies and CDP cannot grant observation authority.
Preserve ordinary fetch, provenance, source and resource admission. Use the
existing host SHA-256 implementation; no new Sparkle dependency is needed.

Callbacks are cooperative, not hostile-page attestation: pinned
`notify_result` passes the internal Tests object as its second argument, exposing
callback arrays to configured JavaScript. Typed capture does not make a
well-formed fabricated callback unforgeable. Reviewed complete source inventory
and real execution remain acceptance prerequisites. Normal browsing stays off.

## Host APIs and bootstrap boundary

Keep the shared `js_browser::Request`, unversioned one-shot and strict legacy
v1 forms unchanged. Add a default-Disabled trusted observer expectation to
`ScriptRuntime`, and a configured constructor on `LinuxScripts`. The constructor
owns immutable Init configuration; a test-host wrapper verifies the request
inventory before forwarding `start`. Existing default constructors remain off.
`BoaPageRealm::start` stays off; an explicit trusted observed-start API accepts
the typed configuration. No page selects it.

A dedicated non-scoring host composes the real Browser and configured runtime.
It must reject script navigation and projected meta refresh **before** Chassis
can schedule a fetch, preserving the existing validated-denial and exactly-one
charge pattern in `examples/wpt_render.rs`. Malformed denied projections remain
infrastructure errors, not ordinary policy failures. This is a test-host policy,
not a new production navigation restriction.

Preserve source node identity in the ordered execution list. The configured
bootstrap must be the first actually executed eligible ordinary external classic
script: not missing, duplicated, substituted, async or deferred. No earlier
source may replace registration functions. Host inventory validation proves
the bytes; the child validates node, source ordering and configuration before JS.
Compute the existing whole-realm exposure floor before any evaluation.

Evaluate the complete bootstrap using normal Ingress/evaluation/checkpoint.
Before the next authored source, read the real global's own DATA registration
properties, require ordinary ECMAScript functions rather than callable Proxies,
capture their identities and register two
native callbacks through guarded calls/Ingress and a normal outer checkpoint.
Partial install, early completion, execution/checkpoint failure, navigation or
terminal state fails closed; a later source cannot repair it. Do not checkpoint
recursively inside a native capture hook or inject/evaluate reporter source.

## Strict modern v6 schema

Modern v6 replaces v5, without claiming backwards compatibility. Existing frame
lengths, generation/session/sequence/revision identities and one-flight rules
remain. Legacy v1 uses its exact old serializers/readers/bytes: new fields,
even null or Disabled, are forbidden. The one-shot API cannot enable observation.

The modern Init payload is a strict wrapper with two REQUIRED non-null fields:

```text
ModernInit { page: Request, observer: ObserverInit }
ObserverInit = { kind: "Disabled" }
             | { kind: "WindowHarness", contract: "PinnedWindowHarnessV1",
                 run_id: u64, bootstrap_node: u32 }
```

`run_id` is nonzero and host-chosen, not a secret. All objects reject unknown
fields, missing fields, wrong types and invalid enums. Disabled has no payload.
Prove maximum actual wrapper/envelope encoding fits the existing 1,024-byte
reserve and counted 2 MiB Init cap; metadata grants no extra bytes.

Every modern reply requires a non-null `observer`, matching trusted host mode/run:

```text
ObserverReply = { kind: "Disabled" }
 | { kind: "WindowHarness", run_id: u64,
     first_ordinal: u32, next_ordinal: u32, records: [ObserverRecord],
     progress: "Waiting" | "Settling" | "Finished", execution_failed: bool }
 | { kind: "ObservationFailed", run_id: u64, prefix_end: u32,
     fault: ObserverFault, execution_failed: bool }
ObserverRecord = { kind: "Result", test: TestRecord }
 | { kind: "Completion", harness: HarnessRecord, inventory: [TestRecord] }
TestRecord { id: u16, index: u16, name: Utf16, status: SubtestStatus,
             message: OptionalUtf16, stack: OptionalUtf16 }
HarnessRecord { status: HarnessStatus, message: OptionalUtf16, stack: OptionalUtf16 }
Utf16 = [u16]
OptionalUtf16 = { kind: "Null" } | { kind: "Undefined" }
             | { kind: "Text", units: Utf16 }
SubtestStatus = "Pass" | "Fail" | "Timeout" | "NotRun" | "PreconditionFailed"
HarnessStatus = "Ok" | "Error" | "Timeout" | "PreconditionFailed"
ObserverFault = "RejectedInit" | "InvalidConfiguration" | "MalformedCapture"
 | "DuplicateResult" | "DuplicateCompletion" | "ResultAfterCompletion"
 | "CaptureOverflow" | "StorageAdmission" | "AuthorityUnavailable"
 | "PrivacyLost" | "RealmTerminated" | "NavigationProposed" | "InvariantFailure"
```

Internal optional representations must not make absence or null valid on v6.
Use bounded sequence decoding, rejecting max+1 before growing an oversized Vec;
do not first construct an arbitrary Value tree. Reject fractional, boolean,
negative or oversized units/integers. Tagged null, undefined and empty text are
distinct. Undefined is allowed only as the actual own-data value of message or
stack, never as a missing descriptor, name, status or identity. The pinned
primitive-rejection handler genuinely assigns TestsStatus.stack undefined
(`testharness.js:5050-5063,5085-5098`); preserve it rather than inventing null or
rejecting that real harness Error.

## Capture, identity and reconciliation

Capture only required own DATA descriptors of real ordinary Test/TestsStatus
objects and a genuine dense final Array, including own data length/indices.
No generic Get, Proxy/accessor/iterator, ToString, structured_clone, toJSON or
user callback may run during extraction. Completion's third argument must be
a genuine zero-length Array for this non-debug contract; nonempty debug asserts
are explicit unsupported configuration, not silently omitted evidence.
The result callback's internal Tests argument is neither copied nor retained.

Use a private traced native weak map from actual Test object to scalar ID,
never an untraced handle/strong-key table or hidden JS argument root. IDs 1..256
are assigned on first encounter, never reused; associate each with one immutable
creation index 0..255. Result arrival can be out of index order. A final-only
object receives its ID during ascending-index final inventory traversal.
Callbacks/local descriptor handles stay rooted while copying; end State/object
borrows before admission or latching. No JS, work scheduling or new permit is
needed for synchronous scalar capture in already admitted execution.

Copy strings losslessly as UTF-16 units, including isolated surrogates. Null and
undefined carry no text units, but their metadata still costs storage. For a
previously reported identity require the same final index/status/message/stack.
Its final name may equal the raw captured units or exactly the pinned sanitizer
output: preserve valid pairs, replace each isolated surrogate with ASCII `U+`
and its lower-case hexadecimal code unit. Retain both observations and label
reconciliation; do not infer the sanitizer branch from status/error wording.
Arbitrary renames or other mutations are observer faults in this narrow profile.
Do not require unique final names: raw lone-surrogate and literal `U+d800` names
can legally collide after upstream's pre-sanitization duplicate check.

Freeze independent vectors before code: empty/ASCII; valid D83D/DE00 pair;
isolated D800 and DC00; D800/D800/DC00 (only first replaced); DFFF/DBFF (both
replaced); literal `U+d800`; raw/sanitized-name collision. Test through the full
unchanged harness as well as the bounded native mapping. Expansion must fit
normal per-field and cumulative limits; never truncate or lossy-convert.

## Inventory and publication

The healthy cursor starts at zero. `first_ordinal` equals the prior accepted
cursor; checked `next_ordinal = first_ordinal + records.len()`, maximum 257.
Empty deltas preserve it. New IDs are consecutive, each ID/index has at most one
Result. Completion occurs once, follows all Results in that delta and forbids
any later Result/Completion. Waiting has no completion; Settling/Finished has
exactly one in this or an earlier accepted frame. The first Settling frame may
carry preceding Results and its unique Completion. After completion was emitted
in an earlier healthy frame, later Settling frames contain no new records.

Final inventory is dense creation-index order 0..N-1, N <= 256, unique ID/index,
containing every earlier Result identity with the reconciliation above. Nominal
Ok requires one earlier Result for every row. Non-Ok may contain genuine
final-only rows of ANY actual subtest status, not just NotRun: timeout/unfinished
cleanup can retain Pass, Fail or Timeout without notifying a result. Preserve
these as final-only, never synthesize callbacks or turn them into file passes.
Represent empty inventories but require the frozen nonzero counts/identities in
owned positive controls. General zero-test-file scoring is not in scope.

Transport validates strict mode/run/cursor/count/discriminant bounds; a shared
bounded pure ledger validates cumulative identities/inventory/text relationships
in Chassis. Stage it with arena projection. Publish records/cursor only after
the complete frame/accounting/lifecycle/projection checks and the existing one
combined active charge succeed. Rejected/failed-charge updates publish nothing
and admit no later command. Expose only this accepted bounded read-only Browser
observation. A direct runtime consumer must use the same ledger, not turn raw
frame decoding into a verdict. Do not clone whole document snapshots/history.

## Bounds, failure and privacy

Narrowing observer caps, within all existing combined limits:

| Quantity | Maximum |
| --- | ---: |
| Distinct identities/final rows | 256 cumulative |
| Result callbacks | 256, one per identity |
| Completion callbacks | 1 |
| Callback ordinals | 257 cumulative |
| Each name/message/stack | 4,096 UTF-16 units |
| Retained text copies, including final/reconciliation copies | 131,072 cumulative units |
| Enabled runs | One per retained realm, Init-only |

Charge actual retained Vec capacities, owned weak-map wrapper/per-membership
metadata, fixed state and copies before retention; no free preallocated pool or
refund after send/free. Opaque upstream weak-map buckets/GC storage remain
bounded by the actual System allocator, not a claimed measured host-map capacity.
Document the concrete wrapper/membership admission policy during implementation.
The 131,072-unit cumulative copy cap applies separately to worker capture and
to the consuming parent's combined staged/accepted ledger, including actual
copies in that owner; neither is refunded. Parent copies are not measured by
worker System counters. Bound their actual capacities/metadata too and include
validation/copy in the same active charge. Worst-case JSON numeric-unit encoding
counts toward the real wire cap. Keep 4 MiB host/DOM, 32/64 MiB requested System,
1M opcodes,
256 pending/2,048 lifetime work, 32 listeners, 256 events, all source limits,
one CPU second, two active seconds, 64 transactions, 300-second lifetime,
4 MiB reply and 32 MiB aggregate wire. Combined arena/observer encoding failure
fails the transaction: no dropped rows, truncation, unsolicited chunk or retry
with renewed counters. Native scanning is bounded, not automatically VM-opcode
metered; full P1 cooperative control remains unfinished.

Latch first observer fault. Drop the entire unaccepted batch and related
temporary roots. Failed output contains only constant fault, run ID, sticky
execution flag and `prefix_end` equal to the last previously emitted healthy
cursor; no new text, status, inventory or cursor advance. Subsequent frames only
repeat that failure/prefix, allowing execution_failed false-to-true. An observer
field/record narrowing overflow can be a nonfatal sticky observation fault while
all existing execution bounds remain valid. Failed existing host/System/source/
work/opcode admission, missing internal policy authority or a native observer
map/ledger invariant instead preserves/latches the existing uncatchable fatal
policy: no further JS, first fatal cause retained, real pending-work cancellation
and terminal reply/teardown. A representable constant fault does not authorize
continuation after those failures. Nothing renews admissions/copy/ID counts.

Check trusted exposure authority before every capture and publication. Initial
foreign/unknown sources, dynamic compilation including failed compilation, or
absent authority cannot unmute results. On floor loss drop unaccepted payloads
and publish only the constant sticky failure. Earlier accepted proved-local
records remain historical evidence, never grounds for PASS after a gap. Do not
retain suppressed JS values or describe observation loss as a genuine rejection.

Set a native sticky execution-failure scalar BEFORE capped diagnostic insertion
for real source/callback/checkpoint failures, actual unhandled notification,
privacy gap and terminal/invariant failure. A canceled error event or later
catch/clean reply cannot reset it. An assertion caught/reported by the harness
does not itself set native execution failure. Keep parent broker/policy,
transport/projection/charge/artifact and cleanup failures independently sticky.
InitialScriptOutcome freezes at load and is not this post-load ledger.

An enabled pre-realm rejection has ObservationFailed/RejectedInit, prefix zero,
and the old exact reportless Closed/Initializing/initial_failure Init exception:
revision zero, no snapshot/report/effects/default/navigation/wake. It remains
admitted Script failure, not automatically infrastructure. No reportless later
reply is allowed. Legacy behavior remains exact.

## Provisional completion and finite settlement

Completion is provisional until the current source/task and normal outer
checkpoint finish. At final snapshot require js_depth zero, no checkpoint or
error dispatch active, and normal freeze/prune/privacy processing completed.
Fatal/Closed, navigation, cancellation, supersession or observer fault cannot
publish success.

Pending/Frozen/Active rejection candidates, nonempty candidate batches and
queued Handled notifications are existing unsettled observation obligations.
Inconsistent membership fails, rather than pretending none remain. Before any
completion the observer stays Waiting and ordinary wake pumping still applies.
Once completion is captured, report Settling while any such obligation remains,
with an ordinary valid wake. Parent pumps only normally eligible tasks in common
ordering. Every intervening executed task/checkpoint and its new obligations/
failures belong to this run. Persistent weak membership of already-notified
promises is NOT a pending notification.

The first accepted fully completed checkpoint with no such obligation may
publish Finished. No sentinel/free queue, special task selection, recursive
drain or worker sleep. Unrelated future timers/messages are not drained; if
ordinary ordering executes one while reaching a notice, its effects count.
Finished declares this accepted checkpoint cutoff, not absence of future errors.
Report real remaining work, then stop admission/cancel/teardown. Infinite causal
chains or exhausted budgets are explicit incomplete/failure, not a new allowance.

An owned positive additionally requires actual accepted LoadComplete,
initial_failure false, Ready realm, matching run, exactly one reconciled Finished
completion, harness Ok, its expected nonzero subtests Pass, no sticky failure and
confirmed cleanup. Non-Ok completion preserves its actual lifecycle prefix and
raw harness/subtest statuses; it cannot manufacture load or a pass.

## Controls, outcomes and public verification

NEW authored controls explicitly use ordinary
`setup({output:false, explicit_timeout:true})` with a parent absolute 25-second
deadline including fetch/bootstrap/tasks and cleanup. Stop execution/observation
admission by 23 seconds, reserving the existing two seconds for owned cleanup
inside that same absolute 25-second bound; no renewed cleanup deadline.
Idle waiting remains parent-owned; count all active work/validation/encoding/
transfer/projection under the unchanged two-second budget. Do not change the
historical public-probe inputs. Also freeze default-timeout controls: successful
completion with the original file timer still pending, and genuine harness
Timeout. Parent deadline/missing completion is NOT fabricated harness Timeout.

The frozen control inventory records exact HTML, the complete content-addressed
bootstrap and source-derived outcomes before code; no case has been executed or
adopted as a browser result. Resource counters and unknown stack values are not
guessed goldens. Its fixed roster covers synchronous Pass; assertion
Fail with harness Ok; Promise pass/fail; source/DCL/load/later-task/microtask order;
uncaught error/unhandled/caught rejection; missing/duplicate completion; default
timer retention/Timeout and cleanup Error/final-only statuses. Add focused native
controls for getter/Proxy/coercion non-effects; malformed/null/status/identity/
inventory/ordinal inputs; surrogate vectors/collisions; completion followed by
same-task/microtask error, unresolved rejection or throwing Handled callback;
diagnostic saturation; initial/later privacy loss; exact/+1 caps, actual capacity,
GC/weak roots, fatal/navigation/cancel/supersession, legacy/version/null/wire/
transaction failures, projection/one-charge failure and actual child cleanup.
Preserve every old assertion. These authored controls add ZERO upstream passes.

Keep layers separate in receipts: raw harness/subtest status, Finished or observer
fault, sticky execution/broker failure, deadline/missing completion, unexpected
worker crash, infrastructure and cleanup uncertainty. A verifier can validate an
expected negative control without labeling its browser outcome PASS. Zero-row
Ok does not satisfy an owned positive. No new general WPT file/status mapping or
denominator follows from this contract. Unknown cleanup prevents acceptance.

Candidate Browser controls verify post-projection accepted records, single charge,
broker guards and real normal task scheduling. For actual packaged/public bytes,
reuse existing early `mgbrowser --script-session` restricted stdio entry, not a
new CLI browsing mode or embedded harness. A bounded external owned driver sends
strict v6 Init and ordinary TaskPump frames with the exact complete harness,
applies the same pure ledger and existing resource/active/lifetime/wire/identity
bounds, retains typed raw frames and verifies cleanup. Direct stdio bypasses the
manager's parent active ledger: the driver must measure launch, encoding, duplex
transfer, decoding and validation cumulatively against ONE two-second allowance,
excluding only validated parent idle time between due pumps. The child's OS/
allocator/VM guards alone do not establish that parent active bound.
This directly tests the
shipped restricted worker, NOT Chassis/network/desktop acceptance; retain the
separate real Browser and all existing native/CDP/public release gates.

After the accepted cutoff close/cancel the runtime and check wait_idle explicitly;
Browser Drop's logged wait error is insufficient proof. Actual Context/worker
teardown releases pinned fatal VM roots. Claim owned groups/direct leaders only,
not individual descendant reaping or whole-browser sandboxing. Ship this browser
increment through the normal versioned release/public-installer policy. Full
Window readiness and another independently selected/scored cohort remain open
until complete unchanged-harness controls actually pass under unchanged limits.
