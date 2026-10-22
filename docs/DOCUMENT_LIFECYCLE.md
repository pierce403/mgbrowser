# Queued snapshot document lifecycle: implementation contract

Reviewed and frozen 2026-10-22 for F-016 / F-026 after v0.9.10, before changing
engine behavior. **Local v0.9.11 candidate implemented; release gates pending.**
Focused and full debug/release regressions, actual workers, package/install/updater
fixture, frozen JSPLAN, all 13 packaged native/CDP helpers and fresh complete WPT
review pass. Exact-commit CI, tagged packaging and public delivery remain separate
gates. Public v0.9.10 still uses modern
protocol v4 and bundled Init. This contract does not complete Window-testharness
readiness or authorize another scored corpus; the dated log records evidence.

## Measured baseline

The actual public v0.9.10 executable (release source
`6ee10072217d6ccc91a67b31486b1e10cbeb5de6`, SHA-256
`0a4b00e3ce18eae1e6b84766dc8ada6772d75738df3ea681ec0f508d52f76836`)
was exercised through five owned direct retained-worker controls, seven strict
v4 replies and ordinary EOF teardown. No display, live website, engine override
or parent JavaScript execution was involved. Each worker used its normal
isolation and allocator. Raw replies and the bounded probe are retained in
ignored `tmp/oct22-v0911-lifecycle-baseline-a/` and
`tmp/oct22-v0911-lifecycle-baseline.py`; the dated log records their hashes.

Observed, not desired behavior:

- Empty inline-script startup spends no shared work and advertises no wake
  (it still admits one empty source as one byte, not a script-disabled realm).
- Source/microtasks, interactive readiness, DCL, complete readiness and load
  all finish inside Init. A source-created timer and message run only in the
  next two transactions: work changes 7 admitted / 5 executed / 2 pending to
  7/6/1 and 7/7/0.
- Document capture `stopPropagation()` does not stop the separate noncapture
  target invocation: the title becomes `capture|target-after-stop`. Fix this
  phase-boundary bug with a regression, without dropping same-invocation
  listeners that only `stopImmediatePropagation()` should stop.
- An Init with 256 pending timers is Ready at exactly 256 admissions; an Init
  canceling 2,048 timers is Ready at exactly 2,048 admissions and zero pending.
  Mandatory lifecycle work changes these premises, not the numerical caps.

The boundary probes observe acceptance, not 257/2,049 rejection or a refund
negative. The capture-stop probe records callback order; source inspection
confirms the separate capture/noncapture invocations. It does not independently
record eventPhase fields or exercise Chassis/CDP/painting.

These authored observations are neither upstream tests nor evidence of complete
harness execution. The complete unmodified harness's earlier initialization
measurement still leaves later work pending; see [the readiness plan](WPT_TESTHARNESS.md).

Reproduce against the extracted or installed **exact public v0.9.10** executable:

```sh
python3 -B tools/lifecycle-baseline.py \
  --binary /absolute/path/to/v0.9.10/mgbrowser \
  --output tmp/lifecycle-old-baseline-UNIQUE
python3 -B -m unittest discover -s tools -p test_lifecycle_baseline.py
```

The tool freezes those five source hashes and old observations; it deliberately
rejects a different build, including a locally rebuilt same-version executable.
It never downloads/updates the browser or accepts arbitrary page code/URLs.
Use a new ignored output each time. Linux x86_64, exact binary size/SHA-256,
nonsymlink executable/output paths and normal owned-group cleanup are mandatory.
Frames are bounded to 4 MiB, input below 8 KiB, each child to four controlled-work
seconds and the probe to 25 plus bounded cleanup; its receipt is at most 64 KiB.
Those are cooperative observation deadlines, not a hard host-I/O sandbox.
Cleanup proves owned-group termination and leader reap, not individual descendant
reaping; the path/hash checks are not an adversarial execution boundary.
Negative/tool cleanup tests use synthesized data and tiny owned Python children,
not browser results. The actual public probe is a separate manual measurement.

## Dispatch and scheduling

Within the existing pre-parsed, preloaded classic-source domain, execute ordinary
sources/checkpoints in Init, update readiness to interactive before deferred
sources, then enqueue separate DCL and load tasks after the final source
checkpoint. DCL bubbles from document through Window and is noncancelable. The
later load task first updates complete readiness, then dispatches nonbubbling,
noncancelable Window load with its legacy document target. Readiness changes
dispatch document `readystatechange`. Preserve real callback/checkpoint and
handler-slot behavior. These semantics follow [HTML end-of-parsing](https://html.spec.whatwg.org/multipage/parsing.html#the-end)
and [document readiness](https://html.spec.whatwg.org/multipage/dom.html#current-document-readiness).

Use existing deadline/checked-handle order across timers, messages and DOM tasks;
do not prioritize lifecycle to preserve old bundled ordering. Earlier eligible
source-created tasks can precede DCL. Both lifecycle tasks enter the queue before
DCL callbacks can enqueue more work: there is no lazy successor admission or
new load barrier. This deterministic choice applies to the current preloaded
domain, not general cross-task-source ordering or dynamic resource loading.

Each parent pump executes at most one selected task with its checkpoints. Never
run a task recursively, sleep in the worker, or drain the queue to infer load.
Publish a milestone only after its dispatch/checkpoints finish successfully;
ordinary callback diagnostics do not erase the milestone or the failure record.
Fatal/Closed execution or proposed navigation cannot manufacture completion.

Fix the measured capture-stop boundary while preserving targeting, event-phase
identity, listener snapshot/removal, property-slot order, noncancelability and
escaped events' final null currentTarget/zero eventPhase. Unobserved events
need not allocate invisible event records. Distinguish the stop flag checked
between invocations from immediate-stop within an invocation, as specified by
[DOM event invocation](https://dom.spec.whatwg.org/#concept-event-listener-invoke).

## Mandatory admission, ownership and termination

Admit both mandatory tasks after deferred execution, before queue storage or
publication, with no page code between their admissions. Each gets a genuine
existing WorkPermit, even without listeners. Use the existing DOM-task storage
ledger and checked handles; no separate/free queue, quota or reservation grants
extra execution. Both entries publish together or execution terminates. If the
second admission fails, cancel/drop the first outside State/policy borrows;
lifetime admissions never roll back. Storage/ID/invariant failures are terminal,
never synchronous startup fallback. Existing event-record failures close the
realm and do not advance its milestone.

For an otherwise empty successful startup, the required work accounting is:

| Reply | Admitted | Executed | Pending |
| --- | ---: | ---: | ---: |
| Init | 2 | 0 | 2 |
| DCL pump | 2 | 1 | 1 |
| Load pump | 2 | 2 | 0 |

Beginning each task releases its pending slot and counts execution before any
callback. Cancellation, navigation, fatal failure and teardown disarm wakes and
release pending roots/permits, never cumulative credit. Rejection privacy purging
must select rejection notices specifically: independent lifecycle and message
tasks survive a foreign/unknown/dynamic-source exposure floor without creating
a spurious rejection-observation gap. Preserve fatal Context-teardown behavior.

The inclusive ceilings remain 256 pending / 2,048 lifetime shared work, 32
listener slots, 256 retained events, 1,000,000 opcodes, existing source/DOM and
32/64 MiB requested-System limits. Init plus lifecycle pumps still share 64
transactions, two active seconds, 300-second absolute lifetime, 32 MiB aggregate
wire and the same frame/child/process bounds. No syscall or capability expansion.

Boundary controls must distinguish lifecycle admission from earlier failures:
254 pending + two lifecycle permits fits; 255 + two fails at second admission.
2,046 canceled lifetime admissions + two fits; 2,047 + two fails at the second;
2,048 fails at the first. A source already fatal never receives two extra work
admissions. Do not blindly add two to every assertion, refund cancellation,
subtract lifecycle work from reports or give lifecycle extra transactions.

## Strict modern v5 progress

Add one required modern-only reply field, not a new command or task authority:

```json
"lifecycle": {"phase":"Interactive", "initial_failure":false}
```

Phases are `Initializing`, `Interactive`, `DomContentLoaded`, `LoadComplete`.
Use a strict typed object/enum and boolean; reject null, absent modern fields,
unknown fields/values and coercions. Reject old modern v4 frames rather than
guessing or falling back. The explicit original-engine v1 serializer/decoder
keeps its exact historical bytes and rejects even a null lifecycle field; an
Option/default alone does not establish strict legacy rejection.

Required validation at worker transport and Chassis boundaries:

- Init cannot claim DCL/load. A normal Ready/no-navigation Init is Interactive
  with pending work and a wake. Preserve genuine early script navigation:
  Ready plus validated navigation may still be Initializing or Interactive;
  cancel that generation without dispatching a fake lifecycle.
- Preserve the existing report-less rejected pre-realm Closed Init exception,
  represented as Initializing with initial_failure=true, not successful loading.
  It cannot open a retained session or bypass ordinary report validation later.
- Later phases never regress/skip; only an accepted Ready TaskPump can advance
  Interactive to DCL or DCL to LoadComplete, once per transition. Init/Event
  advancement is independently rejectable by the parent. The worker additionally
  enforces that only its selected lifecycle task advances the phase: an opaque
  wake token does not let the parent independently identify a selected timer or
  message. Do not claim that stronger wire proof. Incomplete Ready/no-navigation
  replies require a wake; terminal state is not completion.
- initial_failure latches ordinary errors or fatal/Closed failure before load
  completion, including errors in the reply transitioning to LoadComplete.
  Check against the previous phase; later clean replies cannot reset it. Freeze
  it after LoadComplete rather than rewriting history with unrelated later work.
  This bit covers worker observations. The parent separately merges its typed
  broker/transport/validation failures; a false child bit cannot erase a cause
  the child could not observe.
- Validate identity, revision, single-flight, pump's empty edits/outcomes/defaults,
  cumulative accounting and monotonic wake on every reply. Publish phase only
  after snapshot validation and exactly one combined active/projection charge.

Worker progress is private host data, not derived from mutable readyState,
title, status text, DOM attributes or page reporter functions. It says nothing
about full testharness completion or success.

## Parent, headless and CDP behavior

Separate network-fetch admission from waiting for initial lifecycle. Once the
new document/session is committed, allow due tasks for that same generation
while initial loading remains true. Never pump an old document during a newer
fetch. Keep one pending command and existing native-edit versioning/default
validation. Preserve explicit loading-time activation rejection rather than
silently inventing successful input; toolbar navigation can cancel the load.
Hidden live tabs continue to progress through normal host polling.

Expose a typed parent initial-script outcome: disabled/not requested, pending,
complete, partial or failed, with bounded cumulative startup-script/error data.
Keep it independent of transient task status and RealmState::Ready. Preload,
worker, validation, ordinary script and lifecycle errors cannot disappear after
a clean later projection. Transport/opaque host failures remain distinguishable
from admitted script errors; neither can be accepted as a clean script result.
Script-preload failure evidence must come from typed parent broker metadata,
not parsing mixed resource warnings or status text. Existing timing/count-only
ScriptFetchProfile fields do not yet provide that evidence.

For modern v5, `is_loading` settles on actual LoadComplete or explicit terminal/
abort/fetch failure, not task-wake absence. Disabled-script documents settle at
parent parse/resource completion; the explicit legacy v1 lane settles through
its bundled Init/failure, without inventing v5 progress. Existing finite caller
deadlines remain. Stop
initial-load waiting immediately at that milestone even if a long timer or a
load-created zero-delay task remains. The latter requires a separate explicit
bounded script-work wait for future harness acceptance. CLI profiling measures
initial load/paint, not arbitrary timer or full harness completion.

For CDP, separate document commit (`frameNavigated`, initial DOM invalidation),
accepted DCL and load milestones, and stopped/failed loading. Emit each actual
milestone once for its generation, with monotonic receipt timestamps. Settle
pending Page.navigate on success or explicit failure/abort, including superseded
or closed targets; no fabricated content/load events on a terminal prefix.
Later projections still invalidate DOM IDs without repeating page-load events.
Disabled-script documents retain normal parent-owned parsing/resource completion;
do not fabricate worker progress for them. No new Runtime/locator compatibility
or CDP command is implied.

## Renderer integration and immutable scoring policy

The existing static adapter waits on is_loading and then inspects a `JS: ` status
string. Later task completion overwrites that string. Before scoring the engine
change, replace this incidental string dependency with the reviewed typed initial
outcome above; accepting another status string would hide earlier failures.
Freeze and test that host integration independently of interpreting score gains.
Keep the classification boundary explicit: valid admitted partial/rejected
script or lifecycle outcomes are FAIL; opaque start/transport, report-accounting,
validation or projection failures are ERROR; the existing deadline is TIMEOUT.
Do not downgrade infrastructure failures into an ordinary script failure or
count a terminal prefix as successful initial loading.

Keep its 35-second inner/45-second outer bounds, source/navigation restrictions,
error classifications, diagnostics, pixels and input/expectation roster unchanged.
Do not add general timer waits, new supported prerequisites or observer records.
All current passing scripted consumers must reach completed initial lifecycle,
retain zero pending work and clean diagnostics. Their measured shared work will
include the mandatory tasks; preserve full raw reports rather than hiding them.
Compare all case outcomes, requests, raw RGB/PNG and diagnostics, not just counts.
Future testharness/scorer/corpus expansion remains a separate frozen contribution.

## Acceptance and delivery

Before adoption, add independent library and actual restricted-worker controls
for separate Init/DCL/load replies, readiness/deferred order, earlier tasks,
real checkpoints, phase-stop semantics, ordinary errors followed by clean work,
fatal/navigation/teardown prefixes, storage/ID/second-permit failures, exact
pending/lifetime/transaction limits, strict modern/legacy bytes and stale,
duplicate, skipping/regressing or Event-advanced progress. Test a pending long
timer after load and a genuine load-created zero-delay task afterward.

Test Chassis single-flight/one charge, edits during pumps, superseding navigation,
hidden tabs, typed sticky failure, finite headless waits and external CDP event
ordering. Explicitly migrate authored message/timer/rejection fixtures' bundled
ordering premises, retaining every clone/identity/origin/cancellation/navigation,
diagnostic/trap and resource assertion. Historical exact zero/2,048 Init tests
need explicit new boundary tests or post-load workloads, never blanket weakening.

Run all existing debug/release/legacy/embedding/JSPLAN gates and a fresh full
static WPT measurement. A browser-visible implementation finishes with a new
versioned release, exact-source CI/site, public checksum/install, actual old
updater and packaged/public native/CDP acceptance. Until then v0.9.10 remains the
installed release, this milestone unfinished, and F-016/F-026/MVP remain open.
