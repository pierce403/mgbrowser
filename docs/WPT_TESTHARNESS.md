# Next milestone: genuine Window testharness readiness

Original plan: 2026-10-21. The 1,011-case flexbox reftest corpus remains separate.
The timer and bounded Window-error increments are shipped; see the receipts below.
No JavaScript testharness result or complete Window readiness is claimed here.
Remaining browser services require separate feature contributions and versioned
releases after the published expanded static baseline.

The [queued snapshot-lifecycle contract](DOCUMENT_LIFECYCLE.md) was frozen before
implementation against actual v0.9.10 baselines and shipped as v0.9.11 at
`ba28b9c5471382bd8b1c254c1add6f4bd8734193`. Mandatory shared admission,
strict modern v5 progress and typed sticky initial outcomes pass their separate
release/public gates; receipts are in the dated log. The measured v0.9.10
baseline used v4 and bundled startup. Static renderer
readiness integration stays independent of interpreting score gains: no arbitrary
timer wait, typed observer or new scored selection is added.

The v0.9.12 candidate implements the separately frozen
[bounded typed observer contract](WPT_OBSERVER.md). Observation remains Disabled
in normal browsing. All sixteen frozen authored controls meet their exact
positive/negative expectations through both a local restricted executable and
a separate real Browser/manager host under unchanged limits. These are local
non-scoring results, not sixteen WPT passes, hostile-page attestation or general
Window compatibility. Final release/public gates remain pending. See
[current implementation, receipts and contributor usage](WPT_OBSERVER_STATUS.md).
The frozen contract and inputs retain their original preimplementation text and
hashes; that companion records subsequent implementation evidence.

## Immutable input

The current corpus already retains the complete upstream harness at revision
`84daed4ae966d9624bed22f43587c79283ad1fea`:

- `resources/testharness.js`: 198,291 bytes, SHA-256
  `d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e`.
- `resources/testharnessreport.js`: 1,231 bytes, SHA-256
  `3777176fc736b0a78cf7617b9613b220f635daae7536ec7ebb99c61dbb525aae`.

Do not replace assertions, remove source sections, hide `document` to force a
Shell environment, or equate harness bootstrap with a WPT file pass. The upstream
report file explicitly documents the vendor `add_result_callback` and
`add_completion_callback` integration seam. Use that seam with bounded typed
records, not console text or DOM-log scraping. `setup({output:false})` may disable
the optional results table; it must not change assertions or completion rules.

## First acceptance: owned controls, no WPT score

Run the complete unchanged harness through the real restricted Boa worker with
authored positive and negative controls: synchronous pass, deliberate assertion
failure, Promise completion, load/task/microtask ordering, uncaught error,
unhandled rejection and missing completion. These controls are not upstream
tests and add nothing to the website's WPT denominator.

Required real browser services and evidence:

1. Keep the Window environment and real document. Correct document-ready and
   Window-load targeting/order, single-window identities, and listener behavior
   must replace the documented startup approximations. The harness registers
   load, message, error and unhandled-rejection listeners; accepting registrations
   alone is not proof that error/message delivery works.
2. Execute a genuine zero-delay task queued by the harness's load callback,
   after the load dispatch and its microtasks. A synchronous callback or Promise
   alias does not satisfy this ordering. Define cancellation, traced callback
   roots and finite cumulative task admission before implementing a task queue.
3. Deliver uncaught errors/rejections to the harness or retain explicit execution
   failures. They may not disappear because a normal-looking completion arrived.
4. Carry bounded typed subtest and harness records inside a reviewed session
   protocol: identity, status, name, message/stack and exactly one completion.
   Missing, extra, malformed, truncated or duplicate completion cannot pass.
   Upstream harness ERROR/TIMEOUT and runner infrastructure failure are distinct.
   Trusted reporter configuration must not be encoded in page HTML/comments.

All page execution stays in the isolated child. A future wait for a delayed task
belongs to parent-controlled idle scheduling, not worker sleeping/busy waiting
inside the two-second cumulative active allowance. No independent unbounded
queue or renewed counters are acceptable.

## Resource gate before another scored cohort

The source fits the existing 256 KiB external-source admission, and its core
listeners fit the 32-registration bound. Those numerical checks do not establish
runtime feasibility. Measure the complete harness under the unchanged 32/64 MiB
requested-System-allocation limits, one million opcodes, one CPU second, two
active seconds, existing wire/transaction/DOM bounds and normal process cleanup.
Full parser/native/GC cooperative control remains unfinished as documented in
[BOA.md](BOA.md). A source-size check is not an allocation or CPU result.

If it exhausts those limits, record the real blocker. Do not prune the harness,
renew counters or silently increase a profile to obtain a pass. Any proposed
new resource policy needs explicit review separate from scoring an engine fix.

Only after this non-scoring readiness milestone should a new genuine upstream
testharness selection be declared from source, frozen before execution, and
reported separately from reftest files and subtest counts. Preserve all current
static pass/support requirements. Do not select only files already known to pass.

Before implementation read [BOA.md](BOA.md), [PAGE_SESSIONS.md](PAGE_SESSIONS.md),
the affected FEATURES.md contracts and the pinned upstream harness itself.

## Implementation review: shared work and parent tasks

2026-10-21, after the expanded baseline's exact-commit CI and public delivery at
`86ab66dfbad1798eb304eca2ae99d7a707554ed6`. This is the reviewed implementation
contract, not a claim that the following browser services are shipped or that
the complete harness fits at runtime. Keep the current 1,011-case corpus and
expectations unchanged while implementing and scoring engine changes.

Implement in dependency order: shared admission and worker-local exception
seams; parent task transactions; Window lifecycle and function timers; genuine
error/rejection delivery; then the typed vendor observer and owned controls.

### Admission and roots

Promise/native jobs and host tasks share exactly the existing 256 pending and
2,048 lifetime admissions. There is no separate timer/rejection allowance.
Admit before retaining callback, arguments, promise or reason. Beginning a task
releases its pending slot and increments executed work before invoking code;
cancellation releases only the pending slot, never a cumulative counter.
An opaque, non-cloneable permit contains only a weak Rust policy reference and
an epoch, never GC values. First fatal failure invalidates old permits and
clears pending work. Drop must not borrow policy state while a caller holds a
policy borrow. Existing report fields keep their shape but describe total shared
work, not only Promise jobs. Existing report validation remains mandatory.

Callbacks, arguments and pending rejection values belong in traced worker-owned
state. Fatal termination, cancellation, firing and realm teardown release the
appropriate roots. A worker-local execution-failure seam may retain the actual
Boa exception for ErrorEvent delivery, alongside its existing data-only safe
diagnostic. No JS exception object crosses the worker protocol. A rejection
observer is a non-capturing function pointer which invokes no JS in the Boa host
hook; it may update traced state only after all policy borrows have ended.
Fatal engine failures are never converted into dispatchable ordinary errors.

### Versioned task transaction

Modern session protocol v3 adds `TaskPump { wake_token }`; the explicit legacy
test lane retains v1. A reply may advertise `TaskWake { token, delay_ms }`, derived
from a private worker monotonic deadline with upward-rounded delay. The parent
records receipt time, validates and charges the projection once, then replaces
or disarms its wake. One pending enum distinguishes activation from task pump;
neither can bypass the other's backpressure or projection acknowledgement.

A due parent-issued pump runs at most one eligible task and its microtask
checkpoint. The worker rechecks the token and deadline before effects. Reject
early, stale and replayed tokens without callbacks. The parent consumes a wake
only after successful enqueue. Pumps carry no control edits or callback handles;
their replies require empty acknowledgements/outcomes, `DefaultAction::None`
and zero startup-script executions. Preserve newer native edits. Explicit script
navigation remains subject to the existing independent parent validation/hop
limit. Validate generation/session/sequence/revision and all resource reports on
every completion, including pumps.

Native polling already visits hidden live tabs. Schedule in Browser polling,
not paint or input callbacks. Headless acceptance needs an explicit bounded
script-work wait, not a redefinition of navigation loading or an indefinite
wait for all timers. No unsolicited child output, worker sleeping/busy waiting,
automatic restart or replay. All existing opcode/source/allocation/DOM, CPU,
two-second active time, 300-second lifetime, 64-transaction, wire and owned-child
bounds remain unchanged. Idle waits occur in the parent; execution, checkpoint,
encoding, transfer and projection remain charged.

### Acceptance before adoption

Independently test mixed Promise/host pending and cumulative exhaustion, canceled
admissions, foreign/double-used permits and fatal teardown before connecting
timers. Then test real task/microtask order, traced callback/argument lifetime,
timer cancellation/nesting, early/stale tokens, malformed pump defaults/edits,
one projection charge, native typing during a pump, hidden-tab progress and
navigation/cancellation/reaping. Existing legacy assertions stay unchanged.
Typed observer record/byte/completion bounds require their own written schema
review before implementation; this task-transaction review does not authorize
an unbounded result channel or a new scored testharness cohort.

Semantic references: [HTML timers](https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#timers),
[document lifecycle](https://html.spec.whatwg.org/multipage/document-lifecycle.html)
and [runtime errors/rejections](https://html.spec.whatwg.org/multipage/webappapis.html#runtime-script-errors).

### First browser increment: timers, not complete lifecycle readiness

The first implementation keeps snapshot startup callbacks bundled inside Init.
Correct their document/Window targets, handler-slot ordering and interactive
readiness before deferred sources, but do not describe that as fully queued HTML
lifecycle or a streaming parser. Timers, including ones scheduled by load
listeners, cannot run until a later parent-issued task transaction after Init
and its checkpoints. Fully task-separated lifecycle, error/rejection delivery
and typed harness completion remain required later work; this partial increment
does not complete the readiness milestone above.

Support function-valued `setTimeout` and `clearTimeout` with genuine argument
roots, Web IDL delay conversion, monotonic eligibility, nested-timer clamping,
positive IDs and unique handles rechecked at firing. String timers, intervals
and unrelated event families remain explicitly unsupported in this increment.
Do not accept message/error registrations as proof of message/error delivery.

The modern regression which expects an arbitrary replacement `onload` getter
to be read during dispatch encodes the old approximation, not HTML handler-slot
semantics. Correct that expectation explicitly: add negative getter-not-dispatched
coverage and preserve actual-global identity, callback re-entry and fatal guard
coverage through standards-valid handler assignment/callbacks. Do not change
legacy behavior or any existing allocation/resource/security assertion.

The static adapter's session wrapper must forward the new host transaction
through its existing navigation/default guard and single combined projection
charge. This is host-API integration only: no corpus, runner, wait, status,
diagnostic, pixel or expectation policy change. The static adapter does not
currently await arbitrary timers. Audit the fresh current-cohort evidence for
retained pending work before adopting its score; do not bless a nominal PASS
whose required task is unfinished. Before introducing any timer-dependent
selection, separately freeze and test a finite readiness protocol or conservative
unfinished-task rejection. Do not silently expand scoring in this engine change.

### Implementation receipt

The first timer increment shipped in
[v0.9.7](https://github.com/pierce403/mgbrowser/releases/tag/v0.9.7) at
`3afaabdfacb6305d7c8637072fef60bd77d692ee`. Exact-commit CI, packaged/public
installation, actual v0.9.6 self-upgrade and public native/CDP controls pass,
including a timer completing while its tab is hidden. The fresh static cohort
remains 317 PASS / 632 FAIL / 62 UNSUPPORTED of 1,011, with no retained task in
passing scripted cases. See the [dated receipts](../memory/logs/2026-10-21.md).
This is only the first increment: full Window readiness still requires the
remaining services and independently reviewed typed observation above.

The next bounded increment shipped as v0.9.8 at
`3e2955800de32e6442412734398d5cbdf6f884cd`. It implements genuine Window
uncaught-error dispatch with parent-owned provenance, conservative whole-realm muting, traced
exception retention and five-argument `onerror`. It deliberately does not guess
source locations, notify Promise rejections, provide messaging, separate the
bundled lifecycle into tasks or add a typed testharness observer. See
[the reviewed error contract](WINDOW_ERRORS.md) and the 2026-10-22 work log for
authored controls and separate release gates. The pinned scored selection and
observer/scoring protocol remain unchanged; full readiness is still open.
Exact-source CI, normal release/public installation and actual v0.9.7
self-upgrade pass; the public native/CDP error fixture retains its expected
diagnostic and proves actual callback/checkpoint/exception identity. Queued
Promise rejection notification is the next separately reviewed increment, not
implemented or accepted by this release.

The [queued rejection contract](PROMISE_REJECTIONS.md) records shipped v0.9.9's
shared admissions, frozen batches, weak membership, no-result-hook failures and
conservative observation-gap policy. Full debug/release regressions, exact-source
CI, tagged package, public installation, actual v0.9.8 self-upgrade and public
native acceptance pass at `af5d8fc3a8f14555520c07946c0c56c355921a01`.
Pinned Boa's fatal VM argument roots require terminal Context teardown,
which the actual browser worker already performs. This is not full-harness
acceptance. Keep its authored controls separate from the scored
upstream corpus and future typed observation.

The complete unchanged harness was subsequently measured once through the
verified public v0.9.9 browser: the first bootstrap blocker is unsupported Window
`message` registration. Initial parsing/admission fit, but execution stopped
early with a pending task, so complete feasibility and typed completion remain
unknown. [The messaging contract](WINDOW_MESSAGES.md) records the exact
diagnostic, deliberately limited serialization scope and unchanged resource
policy. Genuine self-messaging comes before separately reviewed queued lifecycle
and trusted observation; none of these authored controls adds an upstream pass.

The v0.9.10 candidate's packaged executable subsequently initializes the exact
same full harness without that registration diagnostic. It consumes 9,007
opcodes and 18,603,428 cumulative requested-System bytes, but two jobs remain
pending when the navigation-only profile returns. This clears an initial
bootstrap blocker, not the queued lifecycle/typed observer gates or complete
runtime feasibility. The exact-input/cleanup receipt is recorded in the
messaging contract; no scored waits, inputs or expectations change.
