# Worker-backed Runtime foundation

Implementation contract, 2026-10-23. Shipped as v0.9.20 at
`28413873dc1c19cae537af9158899760c40dc7f7`. Focused typed-realm, parent/broker,
actual restricted-child and full local Rust/JSPLAN checks pass. Both native/CDP
proofs pass on packaged and public-installed executables, including newer page
form edits and retained roots across detach/redock/closure; all fourteen actual
images were inspected. Both complete score lanes remain unchanged. Exact-source
required CI/hosting, normal publication, public install/update and frozen controls
pass separately; see the [public closeout](../memory/logs/2026-10-25.md#v0920-public-release-closeout).
This advances TASKS item3/F-011/F-024, not their full requirements or the broader
goal. The latest ordinary-client baseline is the failed public v0.9.21
unmodified Playwright1.58.2 run in [PLAYWRIGHT.md](PLAYWRIGHT.md). It still fails
connection before title/locators; the synchronous Runtime contract below is
unchanged. Future startup work is scoped in [AUTOMATION_STARTUP.md](AUTOMATION_STARTUP.md).

## Required result

Provide real main-world execution contexts, generic evaluation/function calls,
worker-owned object handles, own-property inspection and release. Supplied code
must run in the existing restricted page worker, sharing its actual DOM, globals
and cumulative budgets. No parent evaluation, expression recognition, fabricated
title or second engine. End-to-end native/CDP and normal packaged/public release
acceptance are required before calling this increment shipped.

This first synchronous subset does not implement promise awaiting, utility
worlds or user activation. Reject semantic options that are not implemented;
do not silently acknowledge them. Their genuine implementations, real target/
download/lifecycle integration and the unchanged full Playwright journey remain
subsequent requirements. Do not call this complete Runtime or Playwright support.

## Permission, identity and publication

- Require remote debugging and effective page scripting enabled; never override
  `--disable-scripts`. Use only an already accepted usable realm after its initial
  load completes. Loading, busy, absent, expired, terminal or closed sessions reject.
  The trusted Window-observer lane cannot accept automation.
- Bind context identity to target, navigation generation and actual worker session,
  not DOM projection revision. Contexts survive accepted projections and native
  tab moves, but not navigation, terminal failure or closure. Route authority is
  checked independently; no request can select an unrelated target/owner.
- Enabling Runtime reports actual contexts; disabling its notifications is not
  a scripting permission grant. Freeze exact event/reply ordering and context-ID
  bounds in the CDP adapter tests before exposing its schema.
- Automation shares the existing one pending/validation slot with Event/TaskPump.
  Evaluate/call/inspection carry real bounded control edits and their versions;
  release/owner cleanup carry none and leave native edits pending. Preserve newer typing,
  and never fabricate an activation/default/lifecycle milestone. Source and
  snapshot errors remain distinguishable from unsupported result representation.
- Parent identity/resource/arena/navigation/edit validation and exactly one
  active-time charge precede publication of any result, handle or DOM effect.
  Ordinary preceding effects survive caught exceptions/representation rejection;
  fatal execution, malformed replies or rejected charge retain the previous page.
  A still-valid completion after client revocation still takes that charge and
  commits its real DOM/edit effects; suppress the revoked client's result/handles
  and schedule ordinary root cleanup. Never drop a completion while retaining a
  now-desynchronized mutable worker realm.
- Sequence63 is the final allowed transaction: its Ready reply does not mean the
  manager kept the child alive. Publish completed data only with explicit trusted
  terminal status; destroy the context and never advertise surviving handles.
  `SessionUpdate` carries manager-derived `SessionContinuation::{Live,Terminal}`
  alongside the owned page/result, after actual terminal stop/reap and before
  sending. It is not worker-supplied wire authority or a sampled closed flag.
  A Terminal completion still takes its one charge and commits a valid Ready
  projection. Any handle-bearing value/property/exception result rejects as a
  whole before handle publication; completed by-value data remains returnable.
  Emit that result/rejection before context destruction, suppress further wakes,
  and admit no further realm work. Unknown reaping remains a cleanup failure.
  Navigation or blockage before a cached completion is delivered also prevents
  dead handle publication. Preserve the manager's original continuation evidence
  and completed nonhandle data; never cancel a replacement worker to clean an old
  context's cached result.

## Typed operation/result seam

Use a new strict modern envelope version. Existing Event/TaskPump/Init payloads,
legacy-v1 bytes, one-shot formats and observer authority stay separate. Missing,
extra, wrong-version or command-mismatched result fields reject before publication.

```text
AutomationRequest { owner: u64, operation: Operation, edits: Vec<ControlEdit> }
Operation = Evaluate { source, return_by_value, group }
          | CallFunction { source, receiver: Option<u64>, arguments,
                           return_by_value, group }
          | GetOwnProperties { object: u64 }
          | ReleaseObject { object: u64 }
          | ReleaseGroup { group }
          | ReleaseOwners { owners: Vec<u64> }
Argument = Undefined | Value(SerializedValue) | Number | BigInt | Object(u64)
AutomationResult = Value(RemoteValue)
                 | Exception { diagnostic, exception: Option<RemoteValue> }
                 | Properties(Vec<OwnProperty>) | Released
                 | Rejected { diagnostic }
RemoteValue = Undefined | Null | Boolean | Number | String(Utf16Units) | BigInt
            | Object { object: u64, kind: ObjectKind }
            | ByValue(SerializedValue)
SerializedValue = Null | Boolean | FiniteNumber | String(Utf16Units)
                | Array(Vec<SerializedValue>)
                | Object(Vec<(Utf16Units, SerializedValue)>)
```

These shapes name responsibilities; the Rust representations must retain strict
decoding, bounded allocation and finite-number validation. Numeric source limits
below apply before cloning/admitting, and the exact encoded-frame limit still wins.
Remote object numbers are worker-local opaque IDs; the CDP adapter must bind them
to the actual context/route rather than accept an owner supplied by a client.
ReleaseOwners is parent-only cleanup: owner0 can select only that operation;
ordinary operations require a nonzero broker-issued owner. No CDP command exposes
the cleanup owner/list directly. Receiver-less calls use the actual realm global;
missing call group inherits the receiver's group, where present.
Property/accessor results inherit the inspected object's group. `OwnProperty`
contains lossless name plus enumerable/configurable flags and either a Data
descriptor (value,writable) or Accessor descriptor (optional get/set handles).
Every lookup checks the broker-issued owner as well as its object ID; IDs from
another owner/context cannot select objects even when their numeric value exists.

The modern-v8 frame keeps `reply: SessionReply` and adds a required non-null
`result: ModernResult::{Page,Automation(AutomationResult)}` discriminator. Page
is required for Init/Event/TaskPump; Automation only for the new typed command.
Separate strict legacy request/reply enums/views preserve v1 bytes. One-shot and
shared SessionReply shapes do not acquire automation authority. Explicitly
migrate raw modern consumers/tests with their old semantic assertions intact;
do not accept old-modern frames by shape or alter the frozen WPT inputs/mapper.

## Narrowing admissions within the unchanged resource profile

| Item | Ceiling |
| --- | --- |
| Evaluation/function source | 16 KiB UTF-8; actual compile wrapper also charged |
| Encoded operation / encoded edit list | 32 KiB / 24 KiB; complete later frame remains64 KiB |
| Control edits | Existing128 edits,8191 UTF-8 bytes/value, existing version checks |
| Call arguments | 32;16 KiB encoded argument list |
| Live roots / cumulative issued object IDs | 256 /512, never reused or refunded |
| Live route owners / lifetime owner identities | 32 /64 |
| Live named groups / group name | 32 /64 UTF-8 bytes |
| Complete own-property descriptors | 64; reject overflow, never truncate |
| Newly issued roots in one result | 32, including exception/property/accessor roots |
| Individual string / BigInt | 4096 UTF-16 units /4096 magnitude bits before formatting |
| By-value depth / visited entries | 16 /512 |
| Aggregate by-value key/string storage | 16,384 UTF-16 units |
| Native diagnostic | 512 UTF-8 bytes; no script-controlled extraction |
| Encoded automation-result section | 128 KiB; complete reply remains4 MiB |
| Revoked owners in one cleanup operation | 32 distinct nonzero IDs |

Keep64 total transactions including Init,32 MiB lifetime wire,2s cumulative active/
300s absolute lifetime, source/opcode/System/DOM/work/CPU/address-space/child bounds.
No counter reset, reserved hidden allowance, process restart or syscall expansion.
Existing System/native containment still applies to upstream enumeration scratch
allocation; a64-descriptor output cap is not a claim of cooperative enumeration.
Measure actual costs and archive size; do not adjust old assertions/caps to fit.

## Values, roots and exceptions

Preserve exact undefined/null/boolean, finite/nonfinite/negative-zero and bounded
BigInt values. Worker strings/keys use lossless UTF-16. The current CDP String
codec cannot represent lone surrogates: reject such external results explicitly,
not lossy replacement or literal backslash escapes. A new codec needs separate
review. Symbol representation remains explicitly unsupported in this subset.

Root actual objects in lazy, GC-traced worker-owned storage tied to BoaPageRealm.
Do not use the weak DOM-wrapper cache or retain JS values in Chassis. Preserve
receiver/argument identity across aliases, cycles and real GC. Use guarded native
call invocation, not a page-overridable `.call`. Registry construction is lazy;
ordinary page bootstrap must not gain a prepaid remote-root reserve.

Own-property inspection requires supported ordinary objects/arrays and complete
own descriptors in native key order, including nonenumerables. Getters/setters
may become actual handles but are not called. Reject proxies, DOM-wrapper proxies,
other exotics and unsupported symbol keys before inspection, never empty success.
By-value trees admit only ordinary own enumerable data and dense arrays, without
getters, toJSON or coercion. Reject cycles, holes and unsupported nested values;
acyclic aliases may duplicate only within the same aggregate admission.
Compound negative zero is explicitly unsupported, not normalized to positive
zero. Top-level values and call arguments preserve it through the typed variant.

Evaluate/CallFunction perform one ordinary outer guarded microtask checkpoint
after success or an ordinary caught exception, before serializing the retained
result or page snapshot. Preserve existing nested real error-reporter handling;
never run further JavaScript after fatal failure. GetOwnProperties and all release/
cleanup operations run no JavaScript or checkpoint and advance no lifecycle.
Stage multi-handle results atomically: rejected representation/admission releases
new temporary roots without refunding cumulative issuance or other accounting.

Native exception extraction must not invoke `.name`, `.message`, `.stack`, getters
or coercions. A caught Runtime exception has separate typed transport and does not
dispatch page `window.onerror`. Genuine checkpoint/job failures retain their
existing page behavior and diagnostics. Do not fabricate source locations.
The advertised narrowed ExceptionDetails omits unknown line/column/source/stack
fields. If the thrown value is unrepresentable, omit both its exception member
and the command's result, rather than fabricate thrown undefined. This is not the
full upstream required shape; ordinary representable throws retain their value.

After valid worker admission but before source parsing/execution/checkpoint,
lower Sparkle's sticky exposure floor and purge earlier private rejection notices
under the existing privacy/no-refund rules. A destination context is not proof of
same-origin fetched source. Do not relabel Butane's dynamic-compilation hook flag;
actual eval/Function still sets it normally. Rejected preflight does not mutate
the realm. See [WINDOW_ERRORS.md](WINDOW_ERRORS.md) and
[PROMISE_REJECTIONS.md](PROMISE_REJECTIONS.md).

## Release and revocation

Release drops actual roots, never cumulative issuance/accounting. Distinguish
forged, foreign, stale and already-released IDs with bounded scalar history;
same-owner duplicate release is idempotent, not permission to accept arbitrary IDs.
Group release affects only that owner's matching roots. No live-handle eviction.

Disconnect/detach immediately revokes parent authority. Track revoked owners in a
bounded set, then submit one ordinary single-flight cleanup transaction when idle.
Pending-revoked owners continue occupying live-owner capacity until accepted
cleanup. Churn cannot exceed the bounded pending set; reject further admission
or terminate on an invariant failure, never silently discard revocations.
It consumes existing transaction/wire/active allowances and calls no JS/checkpoint.
If terminal/expired/exhausted admission prevents cleanup, cancel/reap that same
worker through existing terminal policy. Its visible loss of further page-script
execution is explicit; no transaction65, root leak, extra queue or fresh worker.

Disabling debugging or scripting retires the exact worker that successfully
accepted automation, including pending work and cached handles. Track its actual
generation/session identity, not just a boolean or the filtered permission gate.
Ordinary pages, notification-only use, rejected local preflight and replacement
workers must remain untouched. Re-enabling a flag cannot revive the canceled
realm. Embedders retaining Browsers after dropping BrowserCdp must explicitly
disable debugging first; the native error path already does so. Browser teardown
retains its existing worker cancellation/reaping ownership.

## Required authored acceptance

Before implementation, add the new controls without weakening existing tests:

1. Generic retained globals/closures, actual DOM effects, receiver/argument alias
   identity, callbacks, primitives/trees and every unsupported representation.
2. Real GC root survival/release, groups/owners/tombstones, hostile descriptors/
   proxies/getters and atomic admission of multi-handle results.
3. Caught exceptions with preceding effects but no Window onerror, genuine queued
   errors, sticky source privacy, rejection purging and observer-lane rejection.
4. Exact source/result/root/property/count/encoding bounds, fatal no-later-JS,
   unchanged cumulative accounting and complete terminal root teardown.
5. Actual restricted-child framing/sequence/version/malformed/oversize cases,
   real cancellation/reaping, final-transaction status and legacy preservation.
6. Parent single-flight, stale completion, control edits/newer typing, failed
   validation/charge and no early handle/result publication.
7. Direct/flattened multi-client/multi-target context lifetimes, move survival,
   navigation/destruction, disconnect during execution and exhausted cleanup.
8. Owned native/CDP reading/mutation/paint/input proof using arbitrary source,
   followed by all existing local gates, both full WPT lanes, packaged/public
   repeats and a versioned release. Authored controls add no WPT passes.

Full Playwright, utility-world/awaiting/activation, the locator wire-size review,
all remaining CDP domains, F-016, F-026 and formal MVP requirements stay open.
