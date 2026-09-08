# Retained page interaction contract

Implemented bounded scope, 2026-09-07. Independent local worker, native and CDP
acceptance is recorded in the daily log. This is not general platform compatibility;
the Google result/destination goal remains open.

One restricted child retains the original Runtime and DOM after startup. Real
native and external CDP input can deliver typed clicks and form submission to
that same realm; scripts are never replayed to reconstruct state. The parent
alone owns network requests, window input and navigation. The existing one-shot
worker mode and its isolation selftests remain supported.

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
  attribute handler compilation, timers, keyboard/input events, external script
  loading and general DOM/Event conformance are outside this slice. Unsupported
  APIs must not report fake success. Startup approximations remain documented.

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
| Later event request including envelope | 64 KiB |
| Pending commands and completions | Capacity one; one event in flight |
| Owned script children, active/pending/retiring combined, per browser | 2 |

The 32 MiB wire allowance and retained lifetime are explicit new interaction
policies, not claims of unchanged protocol capacity. Encoding, startup, transfer,
decoding and projection validation count toward active time. Idle time does not.
Keep existing cumulative runtime fuel/4 MiB allocation, DOM 4 MiB, node/depth,
256 MiB address-space and one CPU-second limits. Do not reset any on input.
DOM allocation errors remain ordinary Host errors unless separately designed;
they must not be described as typed runtime-fatal unwind behavior.

Use strict versioned, length-prefixed JSON envelopes with parent-issued generation,
session identity, monotonically increasing sequence and expected revision. Reject
extra, stale, malformed, oversized, truncated or unsolicited replies. New input
never carries scripts, replacement HTML, callback handles or network authority.

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

## Independent acceptance

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
evidence of Google compatibility; only then run one bounded live checkpoint.
