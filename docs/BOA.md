# Boa-backed page JavaScript

v0.4.0 implements an opt-in Boa 0.22.0 page backend. Release publication and public
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

Eligible inline classic scripts share a realm. Modern language fixtures exercise
lexical bindings, arrows, classes, templates, destructuring, Map/WeakMap, Proxy,
Reflect and Promise callbacks. Bounded FIFO microtask checkpoints run after
scripts and delivered callbacks. Scripts can create real controls, change text,
register later listeners and propose navigation that the parent validates.
The existing DOM API and startup-event approximations remain deliberately small.

External scripts, module loading, timers, fetch/XHR, script cookie access,
synthetic dispatch and a general browser task loop are not implemented. An idle
module loader cannot acquire filesystem/network access. Passing the separate
Vue reactivity experiment does not establish a Vue DOM or React browser app.
Google search to first result remains unfinished and deferred.

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
| Promise queue | 256 pending, 2,048 lifetime admitted jobs |
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
