# Fixed trusted Window harness output configuration

2026-10-22: root-reviewed preimplementation contract, frozen with the five
exact authored controls in WPT_HARNESS_OUTPUT_CONTROLS.json. This is not an
implementation, runtime result, scored upstream selection or full Window-readiness
claim. The existing v0.9.12 contract, sixteen authored inputs, 1,011 static cases,
expectations, resource profile, dependencies and limits remain unchanged.

## 1. Purpose and primary evidence

Allow a trusted inventory-verified host to use genuine pinned
`setup({output:false})` before tests, without injecting/evaluating JS, modifying
HTML/helpers, replacing the vendor file, inventing an opener, setting
explicit_timeout/allow_uncaught_exception, or waiving output failures.
This also disables optional assert-output tracking via Tests.output (pinned
1502-1537), not the assertions themselves; it is not a presentation-only no-op.

Pinned revision `84daed4ae966d9624bed22f43587c79283ad1fea`, under
`tools/wpt/upstream/`:

- `resources/testharness.js`: 198291 B, SHA-256
  `d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e`.
- `resources/testharnessreport.js`: 1231 B, SHA-256
  `3777176fc736b0a78cf7617b9613b220f635daae7536ec7ebb99c61dbb525aae`.

Actual vendor lines 5-14 describe integration; 17-31 call setup only through a
real opener. Actual setup is `testharness.js:1114-1131`; Tests.setup 3729-3781
and Output.setup 4297-4306 are used unchanged. Output must still be INITIAL;
disable-before-first-Test is required, not after-the-fact failure suppression.

## 2. Adopted fixed authority and strict compatibility

Add exactly one ObserverContract value,
`PinnedWindowHarnessOutputDisabledV1`, and move modern retained transport to
strict v7. Keep the ModernInit wrapper and reply/record shapes otherwise
unchanged. New code rejects v6; old code rejects v7/new enum. No compatibility
claim. Preserve strict legacy v1 and unversioned one-shot bytes/semantics.

`PinnedWindowHarnessV1` keeps callback-registration-only configuration, with
no host setup call. Its common installer also gains the missing post-call and
post-checkpoint failure guards required by the already frozen fail-closed
contract. Existing Disabled stays off, with no setup lookup or new JS.
The old sixteen input bytes, statuses, diagnostics, quotas and expected
work remain frozen; only mechanical frame v6-to-v7 migration is distinct from
their immutable HTML/JS/manifest. Rerun V1, not the new profile; do not silently
add a call or migrate their explicit_timeout policy.

The new contract is Init-only immutable trusted configuration via current
LinuxScripts::with_observer / BoaPageRealm::start_with_observer. It is not a new
CLI browsing switch, page API, CDP setting, generic callback, property bag or
source mode. No Request field, HTML/comment metadata, fake opener or runtime
profile increase. A host verifies every actual source/provenance and bootstrap
node before start; the child still validates execution ordering and exposure.

## 3. Install state machine and exact guards

Current sites: `boa.rs:1260-1287,1331-1340`; private
`boa/observer.rs:535-631`; guarded call `boa.rs:2199-2207`, checkpoint 1725-1759.
Use existing install-in-progress/installed/fault state, not a second queue.

1. Validate strict config and bootstrap ordering before JS, preserving the exact
   reportless RejectedInit prefix-zero shape on pre-realm rejection. Compute the
   whole-source exposure floor before any source. Run the full unchanged first
   eligible ordinary external bootstrap through normal Ingress and checkpoint.
2. Before any later source, while uninstalled and at js_depth zero, verify policy,
   exposure, Ready/no-navigation/no-fault and clean bootstrap diagnostics.
   Read real-global OWN DATA values for add_result_callback,
   add_completion_callback, and (new profile only) setup. All three must have
   native OrdinaryFunction brand: not accessor, inherited-only, Proxy, bound,
   native substitute or merely callable. Do not invoke getters or coercions.
   Capture all identities before invoking any registration, retaining only local
   traced handles during install; no permanent setup-function root.
3. Allocate/admit actual native install state and callback storage first. Register
   result then completion through current guarded call/Ingress, preserving the
   actual global receiver. After EACH call recheck error/fatal/closed/navigation,
   observer fault and exposure before another call or allocation. Keep callbacks
   during partial installation invalid; do not accept early fabricated records.
4. New profile only: create one ordinary properties object using real intrinsic
   Object prototype and genuine CreateDataProperty `output:false`: own DATA,
   enumerable/writable/configurable all true (Tests.setup uses for-in). Call the
   captured setup once, actual global receiver, exactly one argument. No string evaluation
   or setup-function argument. Null-prototype is NOT equivalent: pinned setup
   calls properties.hasOwnProperty (3742,216,4304). The normal JS property reads
   inside the real setup call remain genuine guarded JS, not native extraction.
5. Do not hold State/object borrows across allocation or callbacks, and do not
   checkpoint recursively inside an active JS callback. After all guarded calls,
   run one normal outer checkpoint, freeze/prune normal work, then recheck all
   guards, including post-checkpoint observer.fault, before allocation-free
   installed commit. A queued capture fault must not mark installed or run the
   next source. No later source runs after
   partial install failure. Page-initiated later setup calls remain ordinary JS;
   only this host install call is exactly once, not an overridden setup API.

On ordinary setup throw/checkpoint error retain the existing safe diagnostic,
sticky execution failure and first observer fault, close installation and cancel
work through the existing terminal path. Existing Window.error reporting may run
only under its normal guards; no JS after actual fatal. Preserve first fatal
cause and terminal lifecycle prefix; never retry with fresh counters or complete
from a later clean-looking reply. Actual Context teardown remains necessary for
pinned upstream fatal VM roots. Native install cannot authenticate a malicious
ordinary replacement function; reviewed exact source inventory is the authority.

## 4. Timer, storage, wire and apply accounting

This call is real JS inside Init, not a free host-only toggle or scheduled task.
Use existing opcode/System/host/CPU/active accounting; do not add a WorkPermit
for a synchronous call or an independent configuration allowance. Charge actual
retained native metadata/capacities before retention; ordinary VM object/storage
uses real System accounting, not an invented large or free native byte charge.

Tests constructor 3726 created the original real timer. Tests.setup ends with
set_timeout (3781): 3801-3810 cancels old timeout then admits a new normal timer.
Cancellation releases pending only, never lifetime admission. Keep its real
10000 ms duration and original parent absolute deadline. The new setup adds one
lifetime admission when the normal timer exists; explicit_timeout is never sent.
Do not mask a resulting limit failure or subtract mandatory two lifecycle tasks.

Freeze inclusive boundary controls from actual steps: before-call N -> N+1 on
rearm, then N+3 only if both lifecycle permits can be atomically admitted.
Private state seams can isolate N=2045 inclusive versus N=2046 lifecycle failure
without claiming those are ordinary full-harness startup goldens. At pending
capacity, canceling the original timer really frees one slot for rearm; measure
that order instead of falsely expecting every full-256 configuration to fail.
Rearm-only can succeed at 256 pending; the two mandatory lifecycle slots then
fail if it stays full, unless genuine intervening work released pending capacity.

Recount maximum actual v7 Init wrapper with u64/u32 maxima and longest enum;
retain the existing 1024 B envelope reserve/2 MiB Init cap. Bounded decoding,
unknown/duplicate/null rejection and actual retained capacity guards remain.
Pre-call metadata/storage admission failure prevents setup and any later source.
Later encoding failures retain the true executed prefix without healthy
publication or rollback claims. Host/System/work/opcode failures still latch
existing fatal policy; no catchable fallback.

Init snapshots/progress/observer delta are provisional. Keep current prepare
before lifecycle publication, then terminal/privacy/fault revalidation and
allocation-free commit; never advance a healthy observer cursor after failed
setup/publication. Parent stages the SAME ledger/projection/accounting and
publishes only after one successful combined active charge. Failed projection,
wire encoding or charge publishes no new rows/progress/cursor and cancels session.
No new reply chunk, omission, retry, capacity refund or last-reply verdict.
Atomicity means accepted publication, not rollback of JS/timer effects already
executed in the child. Retain their real diagnostic/counter/lifecycle prefix.

## 5. Frozen controls and additive host reuse

Full-harness authored controls use exact full bootstrap FIRST, exact original
vendor SECOND, then ordinary authored test source; no setup in authored HTML.
The companion freezes complete HTML/source bytes and hashes. Derive actual
bootstrap/source IDs at admission and verify their entire ordered roster.
Do not reuse authored node1/two-script assumptions for real upstream documents.

- H1 normal new-profile sync assert_equals Pass: one real Result/Completion,
  accepted LoadComplete+Finished, no diagnostics/fault, actual default timer
  still pending at cutoff; exact UI disabling is configuration, not DOM scraping.
- H2 deliberate assertion Fail: harness Ok/subtest Fail, not browser PASS; proves
  disabling optional output did not bypass assertions or typed negative records.
- H3 async completion before real 10 s timer; H4 genuine default harness Timeout
  with actual final statuses. No explicit_timeout or shortened timers/deadlines.
- H5 old V1/default-output sibling: any actual unsupported output exception must
  remain a failure, never special-cased as success/UNSUPPORTED after execution.
- H6 preserved sixteen old controls on V1, byte-for-byte input/expectation and
  substantive work/diagnostic/settlement/resource guards; distinct from H1-H5.

Isolated native/transport/parent negatives are NOT fake harness-readiness proofs:

- N1 missing/inherited/accessor/getter/Proxy/bound/nonordinary setup: zero trap,
  getter/coercion/call effects; invalid install with no next-source marker.
- N2 ordinary setup throws; partial registration throws; actual native checkpoint
  failure, navigation, dynamic compilation/privacy loss, or infinite/fatal work:
  retain first diagnostic/fault, no installed commit/next source/healthy cursor.
- N3 reentry invokes result/completion while installing, re-invokes install, or
  mutates setup global after identity capture: fail closed where required, do not
  re-resolve/call an attacker replacement; no recursive checkpoint/free work.
- N4 exact/+1 actual metadata/envelope/retained-capacity/host/work boundaries,
  policy missing and first-fatal paths: no setup effects after denied admission,
  no pending/root leak from binding; Context teardown separately proven.
- N5 page HTML/comment/global/fake base cannot select profile or upgrade source
  exposure; forged wire mode/run/bootstrap/version/null/unknown fields rejected.
- N6 failed Init projection/one-charge/encoding and supersession/cancel: no
  published new cursor/records, no later command, confirmed owned cleanup.
- N7 setup creates a Promise rejection: preserve normal candidate/Batch handling
  and later diagnostic/settlement, not a fabricated synchronous checkpoint error.
  Same-checkpoint catch suppresses notification; actual unhandled delivery cannot
  be erased by a later catch or normal-looking Completion. No error waiver.

## 6. Exact implementation decisions

Keep the profile out of the installed browser CLI, page globals and CDP. The
reply/record shapes stay unchanged; strict v7 and trusted immutable Init select
the configuration, not a new echoed page assertion or hostile attestation.

Capture all three own-data ordinary functions before any registration. Use the
existing InvalidConfiguration fault and bounded "Observer configuration" error
prefix for a genuine setup throw; checkpoint errors retain their existing
"Promise checkpoint" prefix. Preserve any earlier fault or fatal cause.
Invalid descriptors/brands retain existing MalformedCapture;
navigation, privacy and missing-authority paths retain their specific faults.
After each guarded call and the final outer checkpoint, revalidate actual
fatal/closed/navigation/error/fault/exposure/depth state before another call,
allocation or source. Ordinary JS within a call is not rolled back, and a
nonfatal capture fault does not pretend to synchronously abort its JS stack.

Use private unit seams for synthetic registration/setup, quota and failure
inputs, never a public generic execution/configuration API. Resource-boundary
tests perform actual counted admissions/cancellations, not counter assignment.
At lifetime N=2046, setup may admit 2047 and the first lifecycle permit 2048
before the second fails: retained queue publication is atomic, cumulative
admissions are not refunded. Record that failure prefix rather than claiming
no work occurred.

Reuse the two existing observer examples with fixed --suite values:
"observer" (also the omitted default, original sixteen) and "output-config"
(exactly the five companion controls). Reject duplicate/unknown suite values,
cross-suite IDs and arbitrary manifest paths/properties/source/profile overrides.
Use one small shared pure new-suite parser/source-plan/matcher module, not new
duplicated hosts or a permissive rewrite of the original matcher. Preserve all
old assertions; only their intentional v6-to-v7 frame migration is mechanical.

New-suite preflight derives all three actual script indices, verifies both exact
external sources and provenance, permits only the exact benign charset meta,
and still rejects meta refresh or any extra resource/navigation source. Browser
acceptance requires exactly document+harness+vendor real GET200 records and real
JS-MIME admission; stdio labels its two synthetic sources and makes no HTTP claim.
Keep the existing navigation fence, malformed-denial distinctions, one combined
charge, ledger and cleanup protocols. The standard host cutoffs remain 23-second
stdio and 22-second Browser admission within the same absolute 25-second owner;
Browser process/report cutoff stays 22.9 seconds with the final 0.1-second guard.

Require a typed bounded Text constraint for H2's unmeasured stack, never a literal
unknown marker or a wildcard allowing Null/Undefined. All declared semantic
constraints, current resource validation and the full selected5 denominator
remain mandatory. H5 retains its one Result prefix and exact native failure to
the normal deadline, never old08's empty missing-completion shortcut.

Measure new full-harness work/resource counts; do not guess resource goldens.
After implementation, preserve all old16 controls in local Browser and packaged/
public stdio lanes, and run all new5 in each corresponding lane. Existing full
regressions, fresh1011 scoring, native/public release and real-upgrade gates
remain required before shipping the next browser version.

This is a separately versioned readiness service, not CSS parsing or new WPT scores.
The proposed sixteen upstream parsing files require their own source-frozen
cohort/runner contribution and honest baseline before CSS fixes. Missing declared
prerequisites retain denominator; actual runtime/output/assertion failures cannot
be reclassified away. No broader Window readiness or output DOM support follows.
