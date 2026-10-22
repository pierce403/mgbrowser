# Boa-backed page JavaScript

v0.4.0 introduced the Boa 0.22.0 page backend; v0.9.4 enabled it by default.
Use `--disable-scripts` to opt out. Release publication and public
installer verification are recorded separately in the dated work log.
This is a bounded JSPLAN integration increment, not completion of P1-P8 or a
claim that modern web applications generally work.

Shipped v0.9.11: [queued snapshot lifecycle](DOCUMENT_LIFECYCLE.md) replaces
bundled Init with two later DCL/load tasks, strict modern v5 progress and typed
sticky initial-load evidence. Exact release/public and actual old-update
receipts are in the dated log. The v0.9.7-v0.9.10 sections below retain their
historical bundled behavior.

```sh
mgbrowser --enable-scripts https://example.com/
```

## Implemented boundary

Butane's `modern::Engine` owns Boa's parser, VM, builtins and collector with Mg
policy around execution. Sparkle's `js_browser::boa::BoaPageRealm` binds it to the
existing Rust DOM and retained click/submit protocol. The Linux host starts the
actual page engine only after installing the restricted worker policy. There is
no production fallback to the original evaluator, no JIT and no executable-memory
permission change. Library callers must supply their own process isolation.

Eligible inline and parent-fetched classic scripts share a realm. Modern language fixtures exercise
lexical bindings, arrows, classes, templates, destructuring, Map/WeakMap, Proxy,
Reflect and Promise callbacks. Bounded FIFO microtask checkpoints run after
scripts and delivered callbacks. Scripts can create real controls, change text,
register later listeners and propose navigation that the parent validates.
The DOM API remains deliberately small. Ordinary and deferred classic sources
are pre-parsed/preloaded; the v0.9.11 release queues separate DCL/load tasks
after their checkpoints. This is not a streaming parser or dynamic-resource
load barrier; see [the lifecycle contract](DOCUMENT_LIFECYCLE.md).

Module loading, string timers/intervals, fetch/XHR, script cookie access,
synthetic dispatch and a general browser task loop are not implemented. An idle
module loader cannot acquire filesystem/network access. Passing the separate
Vue reactivity experiment does not establish a Vue DOM or React browser app.
Google search to first result remains unfinished and deferred.

## Bounded Window errors: v0.9.8 increment

Uncaught ordinary script, callback and native-job failures dispatch a cancelable,
non-bubbling Window `error` event. `window.onerror` receives five arguments;
only literal `true` cancels through its return value. Cancellation never removes
the bounded host diagnostic. Actual exception values live in GC-traced event
wrappers; extraction does not invoke page getters or coercions. Fatal resource
failures do not call JavaScript reporters, and recursive reporting is guarded.

Chassis supplies typed final-response/whole-redirect-chain provenance. Sparkle
validates it before JS execution and precomputes a sticky realm-wide exposure
floor: any admitted foreign/unknown source or dynamic string compilation makes
errors opaque, even later local throws. Opaque errors expose only `Script error.`,
empty filename, zero line/column and null error. Proven local realms preserve
thrown-value identity, but still report honest unknown locations. This deliberately
over-mutes rather than guessing a throw site from a callback or source path.
That v0.9.8 increment used modern v4; v0.9.11 introduces strict modern v5.
The explicit original-engine lane keeps strict v1 bytes.
See [the policy and authored controls](WINDOW_ERRORS.md).

That v0.9.8 release adds no Promise rejection notification, messaging, ErrorEvent
constructor, synthetic dispatch, fully queued lifecycle or typed WPT reporter. Listener/event,
work, wire, source, allocation and process limits remain unchanged. Authored
controls are not upstream WPT passes or a whole-browser security claim.

## Queued Promise notifications: v0.9.9

Window `unhandledrejection` and `rejectionhandled` now use real later parent
tasks. Candidates freeze after an outer checkpoint; one pump selects one batch,
one handled notice or one timer, not every later task. Real Promise/reason values
live in branded traced events. A same-stack/microtask catch suppresses delivery;
genuinely late handling has a new admission. Property handlers receive one event
argument, with literal-false cancellation for unhandled notification. Cancellation
is not handling and never retracts the transaction's host diagnostic.

Rejection candidates, jobs and timers share existing admission and lifetime
limits. Post-notification membership is native-weak, not a hidden strong root.
Foreign/unknown/dynamic-source suppression retains no rejected payload or permit;
a sticky data-only notice in every reply explicitly reports incomplete
observation, even if the muted rejection is caught. Losing future handled
observation also sets that conservative flag. No new wire fields, sources,
syscalls, network/filesystem authority or replacement engine are introduced.
See [the contract and acceptance](PROMISE_REJECTIONS.md). Exact release/public
installation, prior-binary self-upgrade and native acceptance pass at
`af5d8fc3a8f14555520c07946c0c56c355921a01`; full Window readiness stays open.

## Bounded Window messages: v0.9.10

The release implements actual later self-Window `postMessage`, `message` and
`messageerror` callbacks, with immutable trusted sender origin and delivery-time
target checks. A partial native serializer supports primitives, ordinary objects
and arrays, preserving UTF-16, sparse length/holes, cycles and aliases. Other
builtins and valid transfers remain explicitly unsupported, never silently `{}`.
Genuine Error-backed DOMException supplies guarded branded errors. Native message
events are traced, non-bubbling and noncancelable. Existing rejection-privacy
purging preserves independent messages and their document origin.

One shared work permit precedes clone storage/getters. Native identity roots
survive getter GC, then drop before queuing the owned scalar-edge graph; no data
or authority crosses the wire. Getters and Web IDL conversions recheck fatal
state, including typed VM errors during IteratorClose. Limits, protocol and
bundled lifecycle stay unchanged. Cross-context messaging, full structured clone,
MessageEvent/Event constructors and synthetic dispatch remain unsupported.
See [the reviewed contract and separate release gates](WINDOW_MESSAGES.md).

## Bounded Window timers: v0.9.7 increment

Function-valued `setTimeout` and `clearTimeout` use real later parent-issued
transactions, not synchronous calls or Promise aliases. Callbacks and supplied
arguments are traced in the worker. Cancellation releases pending roots/storage
without refunding cumulative work. Delay conversion can re-enter JS and shares
all existing guards; deadlines use the monotonic clock, positive timer IDs and
private unique handles. Direct nested timers receive the HTML minimum-delay
clamp; timers created by Promise jobs do not inherit a timer task's nesting.

Only the parent schedules idle waits. An advertised, non-replayable wake token
authorizes at most one eligible callback and its microtask checkpoint. A pump
carries no input edits, script sources or activation/default-action authority.
The parent still validates explicit proposed navigation and every projection.
See [session accounting and scheduling](PAGE_SESSIONS.md).

Promise/native jobs and timers share the same **256 pending / 2,048 lifetime**
work admissions. There is no additional timer allowance, renewed CPU/opcode or
allocator budget, syscall expansion or automatic worker restart. Initialization
and timer/input transactions share 64 total transactions, two active seconds
and the 300-second absolute lifetime. Repeating timeout chains can exhaust these
bounds; the last accepted readable page is retained and script execution stops.
These are intentionally narrow research limits, not normal-browser longevity.

Single-window aliases include self/parent/top with null opener. Document-ready
capture/target/bubble, Window load's document target, property-handler slot order
and interactive readiness before deferred scripts are corrected. These startup
phases remained bundled inside Init in v0.9.7; all timers ran afterward. String timer compilation,
intervals, fully queued lifecycle, synthetic dispatch, cross-context messaging
and typed WPT testharness reporting remain unfinished.
The one-shot `--script-worker` path returns the initial snapshot, not a task loop.
Headless profiling measures navigation/initial paint, not completion of arbitrary
delayed tasks. Authored timer controls are not upstream WPT results.

## Classic external sources

The parent fetches parser-discovered classic sources before worker startup.
`Request.external_scripts` carries typed node/source entries; page HTML comments
are never interpreted as transport metadata. The optional field is omitted for
legacy requests, preserving their serialized representation and baseline tests.
Both parent and worker validate eligibility and source/count/wire admission.

At most 16 external sources participate within the existing 32 eligible script
elements. Each external source is at most 256 KiB; HTML plus admitted sources is
at most 1 MiB. Typed JSON fits the existing 2 MiB initial frame, reserving space
for its versioned envelope. Duplicate execution sources consume admission again.
CSS/fonts/images/scripts share 2 MiB, 32 requests and one absolute ten-second
resource deadline; this is separate from the worker's cumulative source budget.

Ordinary classic sources execute in document order. Deferred external sources
run afterward, before startup callbacks. Because modules remain unsupported,
`nomodule` executes its classic fallback. Async scripts, nonempty integrity
metadata and `crossorigin` are explicitly unsupported and skipped with bounded
diagnostics. External responses require a supported JavaScript MIME type and
UTF-8. HTTPS verification stays enabled; foreign sources cannot send/store
cookies, restore credentials after crossing origins or downgrade HTTPS.
This remains startup preloading rather than a streaming parser or general task
loop. The worker gains no network or filesystem capability.

## Explicit process profile

`boa-page-process-v1` replaces the original evaluator's logical accounting only
for the new Boa page lane. The old 4 MiB logical-allocation and fuel assertions
remain unchanged in the explicit `legacy-test-engine` test lane. These numbers
measure different things and must not be presented as equivalent budgets.

| Resource | Boa page bound |
| --- | --- |
| VM opcode work | 1,000,000 instructions cumulatively across scripts, jobs and callbacks |
| Recursion / VM stack | 64 / 65,536 entries |
| Source admission | 1 MiB per source, 4 MiB cumulative; includes eval/Function |
| Shared Promise/native jobs and host tasks | 256 pending, 2,048 lifetime admissions; cancellation never refunds lifetime work |
| Requested System allocation | 32 MiB outstanding, 64 MiB cumulative per worker |
| ArrayBuffer host ceiling | 4 MiB; worker-wide allocation limits still apply |
| DOM logical admission | Existing 4 MiB, 50,000 nodes, depth 256 |
| Script document / inline count | 1 MiB / 32 eligible script elements |
| OS CPU / address space | One CPU second / 256 MiB |
| Parent active time / absolute lifetime | Two seconds total / 300 seconds |
| Session transactions / aggregate wire | 64 including initialization / 32 MiB |

Static source admission counts UTF-8 bytes; dynamic compilation conservatively
counts UTF-16 storage plus parameter separators. Empty admissions cost at least
one byte. Counters and first fatal failures are never renewed by later input.

The allocator charges requests passed to Rust's `System` allocator, including
its own header/alignment, after activation in the fresh worker. It includes input,
DOM, engine and protocol allocations. Outstanding requests decrease on free;
cumulative requests never decrease. This is **not GC live-heap, allocator usable
size or RSS measurement**. A failed admission exits without unwinding or JS
finalizers, and the parent rejects the incomplete transaction.

`WorkerMemory` reports are snapshots taken before reply encoding. Limits remain
active during encoding and transmission; the snapshot is not a final process
peak or a record of every later allocation.

Opcode/source/job failures latch uncatchably, stop later execution and suppress
default actions. Parser/compiler internals, native builtins, regex and GC do not
all have cooperative work counters. Their final containment is the unchanged OS
CPU/address-space policy, allocator admission and parent deadline. **Full P1
comprehensive resource-control acceptance remains open.** This version does not
promise graceful structured failure for every resource exhaustion.

## Identity, roots and teardown

Native private brands and weak wrapper caches preserve identity while a wrapper
is live. One arena identity survives removal/reinsertion and projection; strings
or script properties cannot forge a native receiver. Cross-realm/stale receivers
reject. Native DOM borrows end before conversions or callbacks can re-enter JS.
DOM strings preserve UTF-16 conversion semantics, then replace lone surrogates
at the existing UTF-8 arena boundary.

Traced active listeners root callbacks. Removal releases the callback root, and
completed checkpoints release WeakRef kept-alive entries. Active listeners on
detached nodes remain rooted until removal or teardown, bounded by 32 cumulative
registration slots and the session lifetime. This is not full DOM ephemeron GC.
Navigation/cancellation drops the realm and reaps its owned process rather than
replaying source. Later failure retains the last accepted readable projection
and discards the unanswered activation's navigation/defaults.
Pending timers trace callbacks and arguments; firing, cancellation and terminal
publication release their roots. Scalar weak work permits cannot keep the policy
or realm alive. Fatal policy failure invalidates every outstanding permit.

Binding work/map/payload roots are cleared on terminal publication, but this is
not immediate whole-VM reclamation. Pinned Boa can retain fatal callback
arguments in its private value stack until Context teardown. Library callers
must discard Fatal/Closed BoaPageRealm values. The browser's terminal child loop
and parent manager already drop/stop those workers; no budget or Context is
reset for reuse. Tests preserve the observed upstream retention failure and
verify actual finalization at teardown, not test-only kept-object repair.

## Evidence and reproduction

Targeted local acceptance passes five GC/root/re-entry/foreign-handle/teardown
tests, 20 replays of the original retained-event contract, and ten modern-page
tests. The original DOM/event tests remain unchanged. Actual-process, packaged,
native/CDP and exact-release publication evidence is recorded separately in the
daily log, not inferred from these library tests.

```sh
cargo test --locked -p mg-butane --features modern --test modern
cargo test --locked -p mg-sparkle --lib js_browser::boa
cargo test --locked -p mg-sparkle --test boa_pages --test boa_page_events
cargo test --locked --test boa_worker
cargo test --locked --workspace --features legacy-test-engine --all-targets
```

The existing fixture server exposes `/script-boa`: two real inline scripts share
modern state and a Promise checkpoint creates the actual search form. The
`/script-events` fixture still requires later handlers, two cancellations, moved
Unicode input, retained proof and a changed link destination. See
[running](RUNNING.md) and [retained sessions](PAGE_SESSIONS.md).

Boa core crates are pinned to 0.22.0, default features disabled; the engine's
`fuzz` feature exposes its instruction counter, without selecting a fuzzer or
native backend. No upstream patch is carried. Project code remains Apache-2.0;
Boa uses its MIT license option. See [dependency policy](DEPENDENCIES.md), the
locked license inventory and the preserved [initial research results](jsplan/RESULTS.md).
The selected Test262 profile is not a full-suite or security certification.

The restricted worker is not a sandbox for the browser as a whole. Do not use
this preview for banking, sensitive authenticated browsing or arbitrary hostile
websites. Linux x86_64 X11/XWayland remains the GUI target.
