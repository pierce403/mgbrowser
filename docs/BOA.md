# Boa-backed page JavaScript

v0.4.0 introduced the Boa 0.22.0 page backend; v0.9.4 enabled it by default.
Use `--disable-scripts` to opt out. Release publication and public
installer verification are recorded separately in the dated work log.
This is a bounded JSPLAN integration increment, not completion of P1-P8 or a
claim that modern web applications generally work.

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
The DOM API remains deliberately small. Snapshot startup is still bundled in
initialization rather than a fully task-separated HTML lifecycle.

Module loading, string timers/intervals, fetch/XHR, script cookie access,
synthetic dispatch and a general browser task loop are not implemented. An idle
module loader cannot acquire filesystem/network access. Passing the separate
Vue reactivity experiment does not establish a Vue DOM or React browser app.
Google search to first result remains unfinished and deferred.

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
phases remain bundled inside Init; all timers run afterward. String compilation,
intervals, fully queued lifecycle, synthetic dispatch, general message/error/
rejection-event delivery and typed WPT testharness reporting remain unfinished.
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
