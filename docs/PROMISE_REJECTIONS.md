# Next increment: queued Promise rejection notification

Reviewed implementation contract, 2026-10-22. The v0.9.9 implementation candidate
passes focused local controls but is not yet released. v0.9.8 at
`3e2955800de32e6442412734398d5cbdf6f884cd` supplies Window errors and source
provenance; this increment follows it toward
[Window-testharness readiness](WPT_TESTHARNESS.md). No WPT selection, observer,
scorer, expectation, resource profile or third-party dependency change is part
of this contribution. Full Window readiness and F-016/F-008 remain open.

## Genuine later notification

Implement Window `unhandledrejection` and `rejectionhandled`, not just accepted
registrations. Each native event exposes the actual Promise and reason through
GC-traced private payloads, with stable escaped-event identity. Both are
non-bubbling; only `unhandledrejection` is cancelable. Function-valued property
handlers receive one event argument; literal false cancels a cancelable event,
unlike the special five-argument Window `onerror` rule. Listener return values
do not cancel. Keep existing listener ordering/removal/propagation rules and
post-dispatch target/phase behavior. Constructors and synthetic dispatch remain
outside this increment, not falsely implemented.

Freeze candidates after a completed outer microtask checkpoint and queue one
later notification batch. Skip promises handled before delivery. Handling during
the notification or its callback checkpoints prevents outstanding membership;
genuinely late handling queues a later handled notification. Cancellation is not
handling. New rejections arising during delivery belong to later frozen batches.
These semantics follow the
[HTML notification algorithm](https://html.spec.whatwg.org/multipage/webappapis.html#notify-about-rejected-promises)
and [host hook](https://html.spec.whatwg.org/multipage/webappapis.html#host-promise-rejection-tracker).

Pinned Boa 0.22 calls Reject after releasing its Promise borrow, but calls Handle
before setting its private handled flag. Its public native Promise state access
can read a rejected reason without JavaScript; it does not expose handled state.
Maintain native candidate phases through pending, frozen, active delivery and
handling. Do not infer handling from a Promise property, thenable inspection,
an event's cancellation or a callback's return value. Keep the active candidate
observable to the hook through listeners, ordinary error reporting and their
checkpoints; do not remove its tracking record before JavaScript re-entry.

## Shared work and parent scheduling

Each retained rejection candidate must obtain one existing WorkPermit BEFORE
cloning its Promise/reason or retaining an identity record. Cancellation frees
pending admission only; lifetime work is never refunded. Freeze batches of scalar
candidate IDs, charging their actual retained capacity. A batch envelope introduces
neither another work allowance nor a second Promise/host queue budget.

One accepted parent pump executes one timer, one frozen rejection batch or one
handled-notification task, followed by the appropriate checkpoints. A batch may
consume several shared-work permits: begin each still-unhandled candidate's
permit before notification; cancel skipped entries without claiming execution.
Reported job fields count shared work units, not a newly invented count of
parent transactions. Late handled notifications require their own admission
before retaining their payloads. Duplicate/foreign permits remain fatal.

Preserve FIFO between batches and handled notifications on the DOM task source.
Select eligible work by monotonic deadline, then a common checked task sequence;
new DOM tasks receive their queue-time deadline and sequence. This defines Mg's
deterministic choice, not a universal timer-versus-DOM source order. Do not append new candidates to an
executing frozen batch or recursively drain later DOM tasks. Account for all
pending candidates, batches and handled tasks when advertising/replacing a wake;
empty canceled batches must not strand pending work or authorize a phantom pump.

Use the existing opaque v4 TaskWake/TaskPump and one in-flight transaction. No
new wire fields, unsolicited child messages, source/input/default authority,
protocol downgrade or worker waiting is added. Keep token/deadline/revision and
projection validation, stale/early/replay rejection, native-edit preservation,
hidden-tab polling and parent-validated navigation. Explicit legacy v1 stays
unchanged. Its sources, behavior and accounting are not rejection-service tests.

## Roots, storage and no-result-hook failure

Pending and active payloads belong in traced worker-owned state. Once a
notification finishes, retain only weak outstanding membership: a native traced
JsWeakMap with Promise keys and scalar values, not a Rust map or callback closing
over the Promise/reason. Escaped event wrappers may legitimately retain their
payloads. Handling, delivery, privacy suppression, cancellation, fatal failure,
close and realm teardown must release the relevant strong roots and permits.
Clear queued records, weak-map roots, active payloads and wakes on terminal
publication; invalidating permits alone does not release JavaScript values.

This binding-root rule is not immediate whole-VM reclamation. Pinned Boa 0.22's
fuzz opcode-exhaustion return bypasses its usual VM-stack truncation; a fatal
callback can leave its argument values rooted in the private VM until Context
teardown. Clearing kept objects does not repair that stack. Library callers must
discard a Fatal or Closed BoaPageRealm, never retain it as a reusable idle realm.
The production child loop returns after a terminal reply, dropping the realm;
the parent manager also stops the terminal worker before publishing completion.
Test binding cleanup immediately and finalization after actual teardown, without
test-only GC repair, new VM-stack APIs, replacement engines or renewed budgets.
The original stronger development assertion demanding unescaped VM finalization
while retaining the fatal Context failed; keep that receipt as a known upstream
terminal-retention limitation, not a passed reclamation control.

Every record, queue capacity and weak-membership entry uses existing host/System
allocation admission. There is no prepaid rejection allowance. Optional fixed
storage still requires actual admission; no uncharged reserve or renewed counter
is allowed. Preserve 256 pending / 2,048 lifetime shared work, 32 cumulative
listeners, 256 events, all source/opcode/DOM/allocation/CPU/wall/transaction/wire
and owned-child bounds. Reports keep their existing shape and validation.

The observer returns no Result and must invoke no JavaScript. Work exhaustion
already latches; storage/ID/weak-map/invariant failures must explicitly use the
existing constant fatal latch rather than ignore an error, throw a catchable
exception or return a clean Ready result. This latch is observed by guarded
execution boundaries, not a claim of synchronous VM interruption. End all
policy/State borrows before dropping permits or using another native seam.
No reporter, job or default may execute after fatal failure; nonexecuting cleanup
still runs. Ordinary listener errors use the existing Window-error path without
turning Promise rejection itself into Window `error`.

## Privacy and honest incomplete observation

Reuse the existing initial foreign/unknown and monotonic dynamic-compilation
exposure floor. Unlike muted Window errors, suppressed rejection notifications
must not expose fake null Promise/reason events. A read-only Context-level seam
for Butane's existing private dynamic scalar lets the synchronous hook decide
BEFORE retaining values; a Sparkle mirror updated after execution is too late.
Missing policy authority must fail closed. No source-path or callback registry
is introduced. Recheck before checkpoint freezing, pump delivery and outstanding
membership. Purge queued local notices if the floor fell meanwhile. Handle must
still release an earlier local entry, but must not schedule exposure while muted.
Already exposed event payloads retain their identity; do not rewrite their fields.

On suppressed Reject, retain no Promise/reason/identity record or notification
permit. Set one realm-sticky scalar observation-gap flag and emit at most one
fixed notice per reply within the existing diagnostic cap:

`Promise rejection observation suppressed by source-exposure policy; handled status unknown.`

Purging earlier queued notices sets the same flag. Repeated suppression creates
no per-rejection record or growing diagnostic collection. A later successful-
looking transaction cannot clear the flag or erase the notice. If the diagnostic
list is already full, its existing errors already preclude a clean result.
The notice is incomplete observation, NOT proof of an uncaught rejection. Even
a muted rejection caught immediately afterward conservatively produces partial
execution. This explicit limitation avoids hidden foreign roots and silent
false success; it is not a new fatal resource failure or a whole-browser sandbox.
A future typed harness observer must preserve/classify that uncertainty rather
than invent an unhandled result or accept clean completion.

Discarding an existing weak outstanding-membership map when the floor falls
also sets that flag: future handled observation is lost even when the original
unhandled reply already retained its diagnostic. A muted late Handle that finds
outstanding membership sets the same flag when deleting it. Conservatively
discarding a map may over-report uncertainty after its keys were collected or
handled; no page code or extra strong root is used to prove the map empty.

For actual local unhandled notification, retain a bounded data-only host
diagnostic independently of event cancellation or subsequent handling. Never
read reason getters, coercions, message or stack for host extraction. Page
listeners may inspect their own exposed values as ordinary guarded JavaScript.
Missing or completion-looking output cannot erase a retained execution failure.
The actual-notification diagnostic is exactly `Unhandled Promise rejection`,
recorded in that transaction like existing Window-error diagnostics. A later
handled notification does not mutate/retract the earlier reply or host log;
unrelated later replies need not repeat it. Only the observation-gap notice is
realm-sticky in every reply. Future typed observation must aggregate all
transaction failures, never infer success from the last reply alone.

## Frozen authored acceptance before implementation

Run initial owned controls against the verified public v0.9.8 binary before
engine changes; unsupported registration is an honest baseline failure, not a
WPT result. Author integration/resource controls before implementing their
behavior, preserving old assertions. Cover:

1. Real later-task timing, target/phase/receiver and exact Promise/reason identity;
   property slot order, cancelability, return values and escaped-event state.
2. Same-stack/microtask catches, handling after freeze but before pump, handling
   inside notification/checkpoints, genuinely late handling and leaf-Promise
   identity through then chains. No canceled event may masquerade as handled.
3. Frozen batch order, new rejections during delivery, FIFO DOM notifications,
   timer coexistence, one pump/one selected task and no recursive task drain.
4. Hostile reasons/Proxy/getter/coercion traps; ordinary listener exceptions,
   recursive Window errors and fatal no-JavaScript behavior.
5. Proven/foreign/unknown/dynamic floors, immediate and microtask catches while
   muted, floor changes between Reject/checkpoint/pump, bounded suppression floods,
   root release and observation-gap persistence in later replies/new realms.
6. Actual GC/checkpoint retention and release for pending, handled, escaped and
   unreachable event values; genuinely weak post-notification membership. No
   test-only kept-object cleanup may repair the active callback path.
7. Mixed shared pending/lifetime exhaustion, canceled admissions without refunds,
   event/listener/storage/ID/weak-map failures and complete terminal cleanup.
8. Real restricted-session pumps, stale/replayed input, hidden tabs, navigation,
   newer native edits and the unchanged default/projection/resource guards.
9. One authored notification-created form through native and external CDP input,
   with retained interaction, exact expected diagnostics and zero trap requests.
   Use a dedicated checker rather than relax the existing diagnostic-free checks.

The v0.9.8 boa_errors test that demands unsupported rejection registration and
no wake records that release's old boundary. Migrate only those assumptions to
real queued delivery, preserving zero Window-error dispatch and adding exact
payload/task/diagnostic checks. Do not remove its intent or alter legacy,
compatibility, AST/allocation or security assertions to obtain a pass.

Freeze production changes before final debug/release, unchanged static WPT,
packaged/public and actual old-updater acceptance. Measure the archive early:
v0.9.8 has only 4,765 bytes of headroom under the unchanged 8 MiB ceiling.
Compression-only packaging changes may preserve identical payload bytes; do not
raise the cap, drop license material or optimize unrelated JavaScript to ship.
Versioned release and verified current public installer finish this increment,
not source-only publication. Messaging, fully queued lifecycle, typed harness
observation and full-harness runtime feasibility remain separate next gates.

Local development receipt: all 292 focused tests pass in both debug and release,
including 28 private Boa binding controls, 22 semantic and 19 resource/privacy
cases. The original 18 resource controls and 22 semantic controls have recorded
old-source failures before implementation. Root's actual-worker controls and an
initial debug native/external-CDP rejection-created form pass separately. See
the dated log for exact failures, additive review controls and terminal-policy
clarification. Complete workspace debug/release each pass 1,851 tests, with no
failure and the same two manually invoked updater tests ignored. A fresh complete
WPT run retains 317 PASS / 632 FAIL / 62 UNSUPPORTED of 1,011, with unchanged
case outcomes, page pixels and requests. All eleven packaged native helpers pass.
Exact-commit CI/tag and public release/installer/old-updater acceptance remain
required; no new WPT pass or full Window readiness is claimed.
