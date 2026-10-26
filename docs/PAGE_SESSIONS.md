# Retained page interaction contract

Implemented bounded scope, 2026-09-07. Independent local worker, native and CDP
acceptance is recorded in the daily log. This is not general platform compatibility;
the Google result/destination goal remains open.

Shipped v0.9.11: [queued document lifecycle](DOCUMENT_LIFECYCLE.md)
adds strict modern v5 private progress, preserving explicit legacy v1 bytes.
Init no longer completes DCL/load: each mandatory task uses a later transaction
and the existing shared work ledger. Chassis pumps committed initial documents
while still loading, accepts progress only after validation plus one charge,
and retains typed initial failures across later clean work. Initial load stops
at LoadComplete, not arbitrary queue idle. Exact release/public, actual old-update
and all 13 packaged/public native receipts are in the dated log. The v4/bundled
descriptions below record the historical v0.9.10 baseline.

The v0.9.12 release adds strict modern v6 with required default-Disabled
observer configuration and replies. Its trusted test-host mode stages bounded
typed records alongside the normal arena and publishes them only after one
successful combined charge. The v5 references below remain release history;
v0.9.12 modern v6 does not accept v5 frames. Exact local controls, unchanged
limits and separate public receipts are in [WPT_OBSERVER_STATUS.md](WPT_OBSERVER_STATUS.md).

The v0.9.13 release at `948e86206c23c0310d17dc286ac5107a15f1f434` uses strict
modern v7 for the additional fixed trusted output-disabled contract; v6 is
intentionally rejected, not translated. Its [scope and release acceptance](WPT_HARNESS_OUTPUT_STATUS.md)
keep local and public evidence separate; the public closeout log is
[dated log](../memory/logs/2026-10-22.md#v0913-public-release-closeout). The v6 statements below describe historical
v0.9.12; required progress, strict legacy-v1 and resource policy remain unchanged.

The v0.9.20 source increment adds [worker-backed synchronous Runtime](RUNTIME_AUTOMATION.md)
with strict modern-v8. A required non-null result discriminator distinguishes
ordinary Page from typed Automation completions without changing SessionReply
or granting automation to the trusted observer/legacy lanes. Old-modern versions
reject, never translate by shape. Automation shares the actual pending slot,
control-edit versions and cumulative limits. Manager-derived Live/Terminal
continuation is host metadata after terminal stop/reap, not worker wire authority.
Focused controls are not full regression/native/public release acceptance.

Since the v0.4.0 increment, one restricted child retains a Boa realm and the
existing Rust DOM after startup. The original Runtime path remains an explicit
test baseline, not a production fallback. Real
native and external CDP input can deliver typed clicks and form submission to
that same realm; scripts are never replayed to reconstruct state. The parent
alone owns network requests, window input and navigation. The existing one-shot
worker mode and its isolation selftests remain supported.

Boa adds bounded Promise checkpoints after scripts/callbacks, native branded
wrappers with weak caches, and traced listener roots. The underlying typed
activation/default-action contract below is unchanged. Active detached-node
listeners remain rooted until removal or realm teardown under the 32-slot and
lifetime bounds; this is not full DOM ephemeron GC. See [BOA.md](BOA.md).

## Behavior

- Preserve append-only worker node identities, including detached nodes. Transfer
  a bounded full node arena; validate it before projecting controls, forms and
  links. Do not preserve identity by injecting HTML attributes or trusting
  worker-supplied layout, URLs or form fields.
- Apply at most 128 versioned control edits before an activation, at most 8191
  UTF-8 bytes per value and 64 KiB for the whole encoded event request. A reply
  acknowledges only those versions, never newer typing. Defaults use the
  post-handler snapshot, not newer edits received while an event was pending.
- A click propagates through a fixed ancestor path: window, document, ancestors,
  target, then back through bubbling ancestors. Support boolean capture,
  identity/capture-based duplicate suppression and removal, function-valued
  onclick/onsubmit handlers, and correct target/currentTarget/eventPhase.
  Listener additions do not alter an already captured listener-ID list;
  removals are rechecked. Property-handler replacement preserves its slot order.
- The add/remove-listener Host entries map null/undefined receivers to this
  realm's Window, including bare aliases; explicit document/node receivers keep
  their target. This operation-specific Web IDL rule is not global normalization
  of native JavaScript or other Host calls. Other invalid receivers still reject.
- preventDefault cancels the default; stopPropagation and stopImmediatePropagation
  have distinct effects. Only a property handler returning literal false cancels
  through its return value. Ordinary callback errors are reported and other
  listeners continue; latched runtime failure stops callbacks and defaults.
- Clicking a submit button delivers click and then, if uncanceled, submit in one
  transaction. Enter activates the default submit button where available.
  Submit targets the form and exposes submitter. The parent constructs the real
  request only after both defaults permit it. Explicit script navigation wins
  over default navigation; only one request may result.
  This first slice conservatively suppresses the submit default if its handler
  detaches, disables or reassigns the submitter, while accepting successful DOM
  effects. Full HTML successful-control behavior in that case remains unsupported.
- Event records have stable identity; escaped references retain their original
  target/cancellation but currentTarget is null and eventPhase zero afterward.
  Limit registrations cumulatively to 32 (removed records do not renew slots),
  and event records to 256. Admit actual retained storage before allocation.
- Object-form listener options, synthetic dispatchEvent/element.click(), content
attribute handler compilation, string timers/intervals, keyboard/input events and general
DOM/Event conformance are outside this slice. Bounded startup external sources
now enter through the parent-owned typed request described in BOA.md; later
event requests still cannot supply sources or network authority. Unsupported
  APIs must not report fake success. v0.9.7 corrected event targeting, handler-slot
  order and readiness before deferred sources. The v0.9.11 release adds separate
  DCL/load transactions after the pre-parsed/preloaded classic sources; it does
  not implement streaming parsing or dynamic-resource readiness. Function timers
  use the separate bounded transaction below.

## Parent-issued tasks: lifecycle, timers, notifications and messages

`setTimeout(function, delay, ...arguments)` retains traced callback/argument roots
under the same shared work admission as Promise jobs; `clearTimeout` cancels
without renewing cumulative allowance. A private monotonic deadline and unique
internal handle determine eligibility. Each accepted reply replaces/disarms its
`TaskWake { token, delay_ms }`, whose relative delay is rounded upward. The parent
schedules from receipt, then validates and acknowledges projection once before
admitting new input or work. A wake can be late but must never authorize early
execution.

Modern protocol v3 introduced `TaskPump { wake_token }`, not a fabricated activation.
The v0.9.8 provenance-aware Init contract used v4 and rejected v3 frames.
The v0.9.11 release uses strict v5, rejecting earlier modern versions including
later Event/TaskPump commands; the explicit legacy lane remains v1.
It shares the activation's one pending command, sequence/revision, lifetime,
active-time, wire and child ledgers. A valid pump selects one eligible
task and its microtask checkpoints. Reject early/stale/replayed tokens without
callback effects. The reply has no control acknowledgements, activation outcome,
default action or startup-script count. Newer native form edits remain pending;
independent script navigation remains parent-validated and hop-limited.

The shipped v0.9.9 adds Window rejection notification to this same scheduler.
After an outer checkpoint, candidates freeze into a later batch. One pump
selects one timer, one frozen batch or one handled-notification task using its
deadline and common checked task sequence. A batch can begin several candidate
permits, not several parent transactions; new rejections during delivery belong
to later batches. A catch before delivery cancels pending admission without
refunding lifetime work. Genuine late handling admits a new task. Traced payloads,
weak outstanding membership and conservative sticky observation gaps follow
[the reviewed contract](PROMISE_REJECTIONS.md). No v4 wire shape changes.

The v0.9.10 release adds one self-Window message per selected pump. The worker
keeps only an owned serialized graph with scalar edges until delivery; a traced
native event then owns the cloned JS data. Clone failures cancel pending work
without refunding lifetime admission. Target mismatch still consumes the selected
task; ordinary restoration failure dispatches messageerror, while fatal resource
or invariant failure stays terminal. No serialized body or callback handle is
sent to the parent. Origin checks, getter re-entry and independent rejection
privacy follow [WINDOW_MESSAGES.md](WINDOW_MESSAGES.md); limits/wire stay fixed.

`Browser::poll` schedules tasks for every polled live page, including hidden
native tabs. Embedders must keep polling; CDP event draining and painting alone
do not drive tasks. `script_work_status` exposes explicit pending/scheduled work
for bounded host waits; `is_loading` retains its navigation meaning and does not
wait for arbitrary timers. A one-shot worker only returns Init. No child sleeps,
unsolicited callbacks/output, replay or automatic restart are introduced.

This is not a general HTML event loop. Separate DCL/load tasks operate within
the pre-parsed/preloaded classic-source domain, without a dynamic-resource
load barrier. String timers, intervals and cross-context messaging remain
unsupported. The non-scoring full-harness
readiness milestone remains open in [WPT_TESTHARNESS.md](WPT_TESTHARNESS.md).

The bounded v0.9.8 Window `error`/five-argument `onerror` path retains ordinary
host diagnostics even after cancellation. A GC-traced native event wrapper owns
the exception, with a sticky opaque policy for admitted foreign/unknown/dynamic
sources. It does not provide exact throw locations; rejection notification is
the separate v0.9.9 increment above.
See [Window errors](WINDOW_ERRORS.md) for dispatch, provenance and retention.

These semantics are a bounded subset of the [DOM event dispatch algorithm](https://dom.spec.whatwg.org/#dispatching-events)
and [HTML form submission](https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm).

## Lifecycle and resource policy

The old 2-second one-shot deadline does not become renewable. New session limits:

| Resource | Bound |
| --- | --- |
| Active parent wall time, initialization plus every transaction | 2 seconds total |
| Absolute lifetime from spawn, including idle | 300 seconds, never renewed |
| Transactions including initialization | 64 |
| Request / response frame payload | 2 MiB / 4 MiB |
| Combined lifetime wire bytes including four-byte frame headers | 32 MiB |
| Later activation or task request including envelope | 64 KiB |
| Pending commands and completions | Capacity one; one activation or task in flight |
| Owned script children, active/pending/retiring combined, per browser | 2 |

The 32 MiB wire allowance and retained lifetime are explicit new interaction
policies, not claims of unchanged protocol capacity. Encoding, startup, transfer,
decoding and projection validation count toward active time. Idle time does not.
The Boa page lane uses cumulative 1,000,000 opcodes, 4 MiB admitted source,
256 pending/2,048 lifetime shared job/task admissions and 32/64 MiB outstanding/cumulative requested
System-allocation caps. These are not the original 4 MiB logical allocation
phases, GC live heap or RSS. Original fuel/allocation assertions remain unchanged
in the legacy test lane. DOM 4 MiB, node/depth, 256 MiB address-space and one
CPU-second bounds remain. No cumulative counter resets on input. Native work
without cooperative checks retains OS/parent containment; full P1 remains open.
DOM allocation errors remain ordinary Host errors unless separately designed;
they must not be described as typed runtime-fatal unwind behavior.

Use strict versioned, length-prefixed JSON envelopes with parent-issued generation,
session identity, monotonically increasing sequence and expected revision. Reject
extra, stale, malformed, oversized, truncated or unsolicited replies. New input
never carries scripts, replacement HTML, callback handles or network authority.
Modern protocol v5, shipped in v0.9.11, requires strict lifecycle progress
and a valid Boa profile report, including platform allocator counters.
The explicit test-only legacy lane keeps v1 and the original
allocation phases. Reports must stay cumulative; neither lane can impersonate
the other's accounting or silently switch engines.

A manager owns child and pipes and handles partial duplex I/O. Cancellation is
nonblocking and wakes an idle manager promptly; Drop joins it and the child is
killed/reaped before ownership/permits are released. No child syscall expansion,
PID-only cleanup, detached manager, unbounded queue or automatic restart.
Navigation cancels the old session; a stale load cannot start a fresh worker.
Expiry, a later event's fatal error or rejected reply leaves the last accepted readable document,
but discards unanswered activation defaults and proposed navigation. Do not silently
fall through to native navigation after a handler-capable session fails. Explicit
toolbar/address navigation remains usable.
Initial valid partial-execution snapshots retain the existing startup fallback
policy; this is separate from atomic later-event publication. Parent hit testing
dispatches to rendered anchor/control identities, not arbitrary synthetic DOM
targets. The library's generic propagation tests do not claim full DOM hit testing.

Event completions are separate from navigation accounting. An accepted projection
invalidates CDP node IDs and emits DOM.documentUpdated, not synthetic page-load
events. No CDP commands are added and Runtime.evaluate remains unsupported.

## Trusted observer publication: v0.9.12

Modern Init is `ModernInit { page: Request, observer: ObserverInit }`, with both
fields required and non-null. `ObserverInit::Disabled` is the default; explicit
WindowHarness configuration carries the pinned contract, nonzero run ID and
bootstrap node. Every v6 reply requires matching non-null observer metadata.
Missing, null, extra-field, wrong-mode/run and older-modern frames reject;
unversioned one-shot and strict legacy-v1 bytes remain isolated and unchanged.
Page HTML, source comments, URLs and CDP cannot select observer authority.

`page_session::observer::ObserverLedger` validates cumulative ordinals, identities,
inventory and lossless text. A staged delta borrows the ledger and owns incoming
records without cloning accepted history. Chassis validates the outer frame,
resource report, lifecycle and projected arena before one combined active charge;
only then does infallible commit advance the accepted cursor. Projection/charge
failure publishes no new records, retains a sticky parent failure and blocks
later work. `Browser::script_observation` exposes only the accepted ledger.

Waiting has no completion. Settling preserves a captured completion while
actual pending/frozen/active rejection or handled-notification obligations remain.
The normal scheduler reaches them under the existing common ordering; no private
drain, sentinel task or extra work allowance is added. Finished marks the first
accepted completed checkpoint without those obligations, not generic queue idle
or future-error absence. Chassis stops further test commands at that cutoff;
the host explicitly closes the runtime and checks `wait_idle` before acceptance.
Real remaining timer work is reported, not erased to manufacture idle.

Observer faults and native execution failures are sticky independently of
`InitialScriptOutcome`, which freezes at load. A genuine reportless rejected Init
keeps the exact Closed/Initializing exception with RejectedInit/prefix zero;
reportless later replies remain forbidden. All existing execution, process,
wire and work bounds stay in force. The narrowing observer caps and distinct
local direct-worker/Browser receipts are recorded in
[WPT_OBSERVER_STATUS.md](WPT_OBSERVER_STATUS.md). v0.9.12 is published at
`1d14f8b57e9a6743c32be4fc2aafa3811d121837`, with all thirteen existing public-
native helpers and all sixteen public-worker controls verified separately.
This does not add a scored cohort.

## Independent acceptance

The Boa library replays all 20 original retained-event cases and adds real modern
page/Promise and GC/root/re-entry/teardown tests. Public release, actual process
and native/CDP evidence is recorded separately; a library pass does not imply it.
The historical fixture acceptance contract follows and remains in force.

Freeze a local handler-required fixture and run the old native binary before
production changes. An anchor click must cancel a trap URL and read actual Unicode
input; first submit cancels and mutates/moves controls; a second submit uses
retained closure state to add proof and permit a real request. A result-link
handler changes its destination before default navigation. The server rejects
missing proof and logs zero trap requests. Native and external CDP clients must
observe both canceled states and the final actual request/destination.

Retain all old tests and add independent event ordering/identity/removal, edit
version, stable arena validation, backpressure, stale reply, cancellation/reaping,
framing and aggregate resource tests. Review any changed exact browser-bootstrap
allocation assertion against measured added registration cost; never change
language-only checkpoints or caps just to make tests pass. Local success is not
evidence of Google compatibility. Google remains deferred for the Boa release.
