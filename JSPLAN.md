# Butane JavaScript engine plan

Research date: 2026-09-16. Status: proposed architecture and ordered work, not
implemented compatibility. Tracks **F-016 / T-013**. Repository baseline:
[`48cec83c0b1d41eb889373e414ded216d09cbf77`][mg-baseline], v0.3.0.

Implementation update, 2026-09-16: the immediate P0/narrow P1 contribution below
is implemented as an [isolated research executable](experiments/jsplan/README.md).
[Measured results and decision](docs/jsplan/RESULTS.md) supersede the unrun status
for those named probes only. The production browser still uses original Butane;
full adoption, browser frameworks, optimization and V8 compatibility remain gated.

## 1. Recommendation

**Make Butane a useful, embeddable Rust JavaScript engine by reusing a serious
Rust implementation first, then earning its performance and V8 compatibility
claims with separate tests.** Run a bounded Boa-first integration experiment;
compare Nova where its design is relevant. Keep the existing interpreter as a
regression baseline during the experiment, not as a second permanent production
engine. Select one implementation before building a JIT or a large adapter layer.

The leading candidate is **Boa 0.22**. Its August 2026 release already includes
polymorphic inline caches and additional VM/bytecode optimizations. Boa's previous
release introduced a register VM and default NaN-boxed values. These are substantial
pieces of the architecture we would otherwise have to build. This is a stronger
starting point than attaching a JIT to our current ES5-like evaluator.
[Boa 0.22][boa-release], [VM and value representation][boa-21].

The recommended order is:

1. Establish language, embedding, resource, and framework baselines.
2. Evaluate Boa inside Butane's restricted host contract; investigate Nova's
   concrete advantages and blockers. Record an explicit adoption decision.
3. Deliver correct modern language behavior, reclaimable memory, external
   scripts/modules, scheduling, and real DOM bindings.
4. Run unmodified, pinned React and Vue application builds interactively.
5. Optimize the measured bottlenecks: representation, property access, bytecode,
   builtins, GC, startup and host calls, before speculative compilation.
6. Add a baseline JIT only when its benefit pays for compilation and its isolation
   and GC contracts are ready. Consider a small optimizing tier afterward.
7. Implement a versioned V8 embedding compatibility surface for a named consumer.

This document recommends a route; it does not install a dependency, replace the
engine, change resource caps, enable page scripting by default, or authorize an
unbounded implementation sprint. Existing releases and their evidence remain
intact. All suggested experiments below are unrun unless explicitly identified
as repository inspection or upstream-reported results.

## 2. What exists, and what must change

The current code is useful evidence and reusable integration work. It is not yet
a modern JavaScript foundation. These observations come from the baseline source
and [the current language contract](docs/JAVASCRIPT.md).

| Area | Current evidence | Consequence for this plan |
| --- | --- | --- |
| Execution | Recursive AST evaluation in `crates/mg-butane/src/runtime.rs`; shared immutable function ASTs use `Rc` | Bytecode and explicit frames are a major architectural change, not a small dispatch tweak. |
| Syntax | `lib.rs` / `syntax.rs`: classic non-strict subset; strict directives, lexical declarations, arrows, classes and modules reject | Modern bundles cannot be an acceptance target until both parsing and semantics expand. |
| Objects | `Object.properties: Vec<Property>`; environments hold `Vec<Binding>`; indexed object/function/environment stores | Interned keys, resolved local slots, shapes and bounded caches are promising measurements, not proven speedups. |
| Values | `Value` enum includes owned `Vec<u16>` strings and integer object/function identities | Preserve JavaScript code-unit semantics; investigate string sharing and compact values without exposing their layout publicly. |
| Lifetime | No GC; logical allocations accumulate against a 4 MiB budget | A long-running reactive application requires reclaiming unreachable data. GC and resource accounting need a joint design. |
| Limits | Fuel 1,000,000; objects 10,000; calls 64; independent parser/evaluator depth limits | Keep current defaults during planning. Define a separately measured application profile before changing them. |
| Builtins | Partial descriptors, symbols, coercion, regex and errors; no Promise/module system | Existing tests protect specific behavior, not complete ECMAScript correctness. |
| Browser execution | Sparkle runs classic inline scripts in a retained restricted realm; external scripts/modules, timers and fetch are unsupported | A language engine alone will not make React or Vue work in the browser. |
| Worker | Linux x86_64 child, closed descriptors, parent-validated results, seccomp | Preserve this boundary when evaluating another Rust engine. |
| Executable memory | `src/platform/script_worker.rs` rejects `PROT_EXEC` in `mmap` and `mprotect` | A JIT cannot run under the existing policy. Do not quietly relax the filter. |
| Conformance | Extensive authored regressions; architecture roadmap still calls for pinned Test262/WPT runners | Establish actual suite results before making a percentage or framework claim. |

Preserve the valuable parts: explicit host capabilities, fatal budget latching,
whole-script rejection, UTF-16 tests, observable coercion ordering, restricted
workers, retained sessions, cancellation, navigation validation and independent
allocation probes. Tests that deliberately encode an approximation must be
classified and migrated when specification behavior replaces it. For example,
unmapped non-strict `arguments` and string-valued exception approximations are
not behavior we should preserve forever merely because a test currently expects it.

## 3. Define “drop-in replacement” precisely

JavaScript source compatibility, a native embedding API, a binary ABI, and engine
performance are different contracts. Passing one does not establish the others.
[V8's embedding guide][v8-embed] describes handles, contexts, callbacks and GC
relationships well beyond `eval(string)`.

| Contract | Desired acceptance | Priority |
| --- | --- | --- |
| Ordinary JavaScript | Pinned standard-language tests plus unchanged distributed framework code produce correct observable results | First |
| Browser applications | The same production app artifacts run in Mg, with correct DOM, events, scheduling and loading | First, shared with Sparkle/Chassis |
| Native Rust embedding | An independent executable can create realms, root values, call functions, provide host objects, handle errors and drive jobs through a documented API | Early |
| `v8` Rust crate source API | A pinned Rust consumer builds with an explicit dependency/backend substitution and no engine-specific application rewrites | Later, one consumer and supported API inventory |
| `v8.h` C++ source API | A pinned embedder recompiles against a documented supported facade and runs its real workload | Separate later adapter |
| V8 binary ABI | Existing binaries link against Butane without recompilation | Open research target; no general promise |
| V8 tooling | Named inspector/CDP clients work against explicitly tested domains and versions | Independently gated |
| V8 performance | Controlled cold/warm workload, latency, memory and compilation comparisons | Independently measured |

Start the Rust compatibility investigation with the API usage of a pinned
`deno_core` embedding example, then one real consumer workload. Its source has
[moved into the Deno repository][deno-core-move]; pin the actual source revision
and crate version at the start of that investigation. This is an API inventory
and stretch acceptance target, not a claim that Deno can already switch engines.
Full Deno, Node.js, npm tooling and Electron require their own runtimes and host
facilities. A JavaScript engine does not supply Node's filesystem or networking.

The C++ route can use a thin source facade over a Rust C ABI. Any foreign-language
glue must be isolated and explicitly reviewed against the existing
[embedding boundary](docs/ARCHITECTURE.md); it cannot conceal a C/C++ engine.
If no such glue is desired, the Rust API route remains useful independently.
Do not reproduce V8's private heap layout, tagged-pointer constants or snapshots
to make a linker experiment look compatible. V8 headers contain inline/template
code coupled to implementation details; exporting similarly named functions is
not a general ABI replacement.

Plan the API in capability groups: runtime/realm lifecycle; scoped and persistent
roots; strings and values; property descriptors; functions/constructors and
re-entry; exceptions and stack locations; modules; Promise jobs; buffers and
backing-store ownership; weak references; interruption; inspector hooks. Unsupported
operations must fail explicitly. A sample shell is a useful first test, not proof
that a production embedder is supported.

## 4. Reuse decision: which Rust work should we build on?

### 4.1 Whole-engine candidates

Source observations below are dated snapshots. Upstream conformance numbers are
self-reports on upstream configurations, not comparable Mg measurements. They
must be rerun with a common pinned Test262 revision, flags and denominator.

| Candidate | Evidence checked | Proposed role | Main adoption questions |
| --- | --- | --- | --- |
| **Boa** | [0.22 release][boa-release], released 2026-08-28, reports 95.60% Test262 and polymorphic ICs. [Inspected source][boa-source] declares `Unlicense OR MIT`; core VM/parser/GC are Rust. | Leading implementation to integrate behind Butane; prefer an upstream dependency plus small patches over a permanent fork. | Fatal interruption, allocation/builtin budgets, GC rooting across DOM callbacks, worker syscall behavior, host-owned jobs/modules, long-session memory and exact active dependency graph. |
| **Nova** | [1.0 announcement][nova-release] and [source][nova-source]. Data-oriented Rust engine, MPL-2.0 manifest, Oxc frontend. Release notes explicitly flag RegExp gaps, sparse-array allocation problems and limited speed. | Comparative embedding/GC experiment; useful design reference. Promote only on measured integration advantages. | Recheck those limitations at the chosen commit; integration API, budget hooks, GC safety and mandatory `usdt` build/runtime dependency review. |
| **Evolve current Butane** | Baseline source and tests above; only serde as a direct production dependency | Fallback if neither engine can satisfy the host/resource contract at reasonable maintenance cost, or a deliberately selected engine-research objective | Own the full semantic, GC, parser, debugger and conformance burden. Reuse individual Rust components; do not mistake the current subset for a small distance from React. |

Pins for reproducibility:

- Initial Boa release candidate: `v0.22`, commit
  `337a3668a0dc86dd401ea20906e782249a64a228`.
- Boa main inspected for current manifests and limit APIs:
  `69388e59f789ed0846a8d6aad1e4dc0c91b35816`. Do not mix this tree's results
  with the release's claimed conformance.
- Nova main inspected: `4eea7c6fae180a8a2eed45c3f6acd0d9256522b1`.
- All other candidate versions must be frozen in the experiment manifest before
  execution. This planning change has not altered `Cargo.lock`.

There are already specific reasons to test adoption rather than assume it:

- Boa's inspected [RuntimeLimits][boa-limits] covers loops, stack, recursion and
  backtraces. That interface alone is not proof of comprehensive interruptibility,
  hard heap limits or bounds inside regex, sorting, parsing and GC.
- Its inspected engine manifest has an optional ICU4X-backed `Intl` path. The
  workspace also contains `ring`-enabled TLS and a native benchmark allocator in
  other configurations. Neither fact proves the selected `boa_engine` graph is
  disallowed, but it makes “the whole workspace is pure Rust” an unsafe shortcut.
  Do not import the standalone CLI, fetch runtime or benchmark configuration.
- Nova's VM manifest includes `usdt` in normal and build dependencies. Audit its
  target-dependent generated code/toolchain path; do not label it approved based
  solely on the VM's language. Disabling default features does not remove every
  unconditional dependency.

**Selection rule:** adopt the engine that passes the hard host/dependency/security
gates and reaches the application corpus with the smallest sustainable patch set.
Boa is the recommendation, not a foregone result. If a hard gate fails, document
the failure and repair cost before selecting Nova or original development. Avoid
a feature matrix that forces Mg to support two different production semantics.

### 4.2 Reusable pieces for an original or upstream-assisted engine

These are candidates, not an approved dependency list. Engine adoption should
normally retain that engine's frontend/GC rather than replace them simultaneously.

| Component | Candidate | Useful reuse and boundary |
| --- | --- | --- |
| Parser and scope tooling | [Oxc][oxc-parser]; [Boa parser/AST][boa-source]; [SWC][swc] as an alternative | Oxc is already used by Nova. Parse ordinary JS with the proper script/module goal; reject error-recovered ASTs before execution. Parser support for TS/JSX/proposals is not permission to extend browser JS. Audit scope early errors, UTF-16 literals, arena lifetimes and retained source. |
| ECMAScript regex | [regress][regress] | Rust implementation supports backreferences/lookaround and explicit UTF-16/UCS-2 entry points. Select code-unit versus Unicode matching correctly and add cancellation/resource controls. Rust's ordinary `regex` crate is not a complete JS RegExp substitute. |
| Number formatting | [ryu-js][ryu] | Reuse ECMAScript-oriented float-to-string conversion. Still implement JS coercions and API-specific formatting semantics correctly. |
| Big integers | [num-bigint][bigint] | Arithmetic substrate for BigInt; operators, conversions, mixed Number errors and denial-of-service bounds stay engine responsibilities. |
| Internationalization | [ICU4X][icu4x] | Candidate Rust data/algorithm support for ECMA-402; control bundled locales/data size without silently changing advertised behavior. |
| Date/time | [temporal_rs][temporal] | Evaluate when the selected language profile needs Temporal. Existing Date semantics and host clock/time-zone injection remain separate. |
| GC for original work | [gc-arena][gc-arena], or the chosen engine's collector | gc-arena provides exact incremental nonmoving collection, but mutation must yield before collection. A JS VM must spill roots at bounded internal safepoints; an infinite JS loop cannot postpone GC until task return. WeakMap/ephemeron support requires its own proof. |
| Code generation | [Cranelift][cranelift], [dynasm-rs][dynasm] | Cranelift supplies Rust machine-code infrastructure; dynasm offers direct templates. Neither supplies JS semantics, deoptimization, GC maps or safe executable-memory policy. Audit only the selected crate/target graph. |
| Test infrastructure | [Test262][test262], [WPT][wpt], [Fuzzilli][fuzzilli] | Reuse tests and fuzzing infrastructure; external test tools are not shipped engine backends. |

Rust wrappers over V8, SpiderMonkey, JavaScriptCore or QuickJS do not meet the
implementation policy. In particular [rquickjs][rquickjs] is a binding to QuickJS,
not a Rust implementation of it. Similarly, Oxc/SWC are compiler tooling rather
than complete runtimes, and Wasmtime/Cranelift do not turn JavaScript into a
conforming engine automatically.

For every adopted crate, record the exact version/revision, features, target,
normal/build graph, build scripts, license texts and integration tests under
[DEPENDENCIES.md](docs/DEPENDENCIES.md). Retain third-party notices, including
Nova's MPL identity if code is adopted; do not relabel upstream code Apache-2.0.
The existing OS-interface exception is unchanged.

## 5. What to borrow from V8

The enduring insight is **specialize common behavior after observing it, while
retaining a correct general path**. Object shapes and inline caches also predate
V8: the Self work below is essential background. We can reuse the techniques
without importing V8's C++ implementation.

Do not freeze the design at V8's launch. Its execution pipeline evolved toward
Ignition bytecode, Sparkplug's cheap baseline compilation, Maglev's relatively
cheap specialization, and a heavyweight optimizing tier. V8 also documented its
move from a Sea-of-Nodes representation toward CFG-based Turboshaft in 2025.
That is a reason to prefer a simple control-flow IR, not to recreate every tier.
[Pipeline evolution][v8-pipeline], [Sparkplug][v8-sparkplug], [Maglev][v8-maglev],
[Turboshaft][v8-cfg].

| Technique | Butane application | Prerequisite and priority |
| --- | --- | --- |
| Interned property names and resolved bindings | Replace repeated string comparisons/local-name searches with `AtomId` and frame/environment slots | Early for original work; preserve direct-eval and dynamic lookup semantics. |
| Hidden classes / shapes | Shared property-layout metadata plus dense slots for common ordinary objects; dictionary fallback for churn | Early after correct descriptors and key ordering. [V8 maps][v8-shapes]. |
| Inline caches | Per-access-site guarded lookup, initially monomorphic then bounded polymorphic; generic megamorphic path | Works in an interpreter. Correct invalidation precedes aggressive specialization. [Fast properties][v8-properties]. |
| Dense array storage | Packed/holey storage separate from named properties; sparse fallback before huge index/length allocations | Early memory and iteration opportunity. Holes are not `undefined`. [Elements kinds][v8-elements]. |
| Register bytecode | Resolve locals once, compact common instructions, explicit frames, shared builtin helpers and per-site feedback | Preferred original-engine baseline; do not build another interpreter if the selected engine already provides one. [Ignition][v8-ignition]. |
| Compact values and strings | Measure enum versus tagged/NaN-boxed values; shared strings, short/Latin-1 representation if worthwhile | After rooting design. Keep UTF-16 observable behavior and a portable fallback. |
| Generational allocation | Nursery for short-lived objects if measurements justify write barriers and promotions | After correct tracing and long-session reclamation. Incremental work can matter more for UI latency. |
| Lazy functions and code cache | Avoid compiling unused bodies; cache verified bytecode against exact source/engine/profile identity | After correct syntax validation and invalidation; count cache memory and retained source. |
| Builtin fast paths | Optimize strings, array operations, JSON and regex where profiles actually spend time | Often more useful than a general optimizer. JSON specialization requires proven absence of observable side effects. [V8 JSON work][v8-json]. |
| Baseline JIT | Emit cheap machine code from bytecode, initially preserving interpreter frame/slot conventions and calling shared runtime helpers | Only after JIT isolation, stack maps and amortization evidence. |
| Speculative optimizing JIT | Type/shape guards, inlining and unboxing in a small CFG/SSA IR; reconstruct interpreter state on guard failure | Late. Consider Cranelift for low-level lowering rather than writing register allocation and all targets. |
| Pointer compression | Smaller heap references when memory layout and address-range constraints warrant it | Defer. It is neither a free memory saving nor a sandbox by itself. [V8 pointer compression][v8-compression]. |

### 5.1 A concrete shape/cache design worth prototyping

For original Butane work, propose an immutable `ShapeId` describing property
keys, slot offsets, attributes and the object's prototype identity. Adding or
reconfiguring a property follows a shape transition. Values live in a compact
slot array; sparse or frequently changing objects can use dictionary storage.
Separate integer-index ordering from insertion-ordered string keys and symbols.

A property read site's initial fast case should be modest: a known ordinary
object shape with an own data property. A matching guard permits a direct slot
read; otherwise use the complete generic algorithm and update the feedback.
An own-slot cache must not accidentally intercept a Proxy or invoke an accessor
with the wrong receiver. Prototype hits and cached misses require guards covering
the relevant prototype chain, not just the receiver's shape.

Specify invalidation for `defineProperty`, property deletion, prototype changes,
accessor replacement, array representation transitions and any cached builtin
assumptions. Bound shape transitions and IC entries so hostile polymorphism cannot
turn the optimization into unbounded memory growth. Let GC trace or weakly clear
cache references so feedback does not retain every historical object graph.

Measure monomorphic records, mixed shapes, inherited properties, dictionary churn
and Vue Proxy traffic separately. A Proxy trap may call ordinary code whose own
accesses optimize well; bypassing the trap is not an optimization we may make.

### 5.2 Compiler and deoptimization requirements

If original execution survives the reuse decision, use one semantic implementation
for the slow paths across interpreter and compiled tiers. Bytecode needs explicit
source locations, exception regions, completion records, lexical environments,
resume points and roots. Async/generator suspension and `finally` must preserve
pending returns/throws. Do not retain a parser arena forever just because a closure
needs a small function; lower into owned code objects and release temporary ASTs.

A failed speculative guard must reconstruct bytecode position, receiver,
arguments, live registers, environments, exception state and inlined frames,
without replaying side effects. GC/safepoint metadata is required wherever a
runtime call can allocate or re-enter JS. Initially avoid inlining and OSR into
arbitrary loop positions; reduce the number of states that need reconstruction.

Cranelift can compile the lowered operations, but Butane still owns the meaning
of `+`, integer overflow fallback, `-0`, NaN, coercion callbacks, exception edges,
write barriers and deoptimization. Do not claim that selecting a backend solves
JavaScript optimization. A baseline compiler may be preferable to a second
optimizer if most functions execute too briefly to repay optimization costs.

## 6. Where dropping compatibility actually helps

The best savings come from choosing a small initial product surface and dropping
old implementation obligations. “Modern JavaScript” still uses decades-old
semantics. Fast paths can assume common behavior only when guarded.

| Choice | Benefit | Cost / rule |
| --- | --- | --- |
| Linux x86_64 interpreter first; architecture-neutral bytecode | Small deployment and validation surface | Keep portable core design; add AArch64/ThermiteOS with explicit gates. |
| Rust embedding first; selected V8 source API later | No need to freeze V8-like object layouts or every historical API | Preserve the long-term replacement goal with a named adapter milestone. |
| No Node/Electron compatibility in the first browser milestone | Avoid filesystem, process, native-addon and server-runtime work | npm build tools may run externally to produce fixtures; page JS still runs in Butane. |
| Production bundles and precompiled Vue templates first | Avoid requiring TS/JSX parsing, dev servers, HMR or the template compiler initially | Keep unmodified standard app builds. Development builds and compiler-in-browser are later explicit lanes. |
| Strict/module-oriented controlled-app profile | Limits the earliest corpus and some sloppy/Annex B work | Do not silently force classic scripts into strict mode. Measure actual bundle modes; add needed sloppy semantics. |
| Defer obscure web legacy features | Reduces early implementation breadth | Declare exclusions. Annex B is part of the browser compatibility contract, not freely removable while claiming full browser conformance. |
| Defer Wasm, shared-memory agents and broad inspector compatibility | Avoid multiple large subsystems before framework basics | Ordinary client React/Vue does not establish support for sites that require them. Full V8 replacement eventually needs additional scope. |
| No compatibility for old Butane bytecode, snapshots or internal Rust layouts | Freedom to change implementation cheaply | Version/reject cached artifacts explicitly; public data and JS behavior need their own stability policy. |
| Choose one production engine | Smaller correctness and maintenance surface | Old evaluator can remain a temporary test baseline, not a silent fallback for failed scripts. |

Do **not** drop these to chase speed:

- **Proxy, Reflect, getters/setters, descriptors and mutable prototypes.** Vue 3
  uses Proxies for reactive objects and accessors for refs. Transpiling syntax
  does not supply missing Proxy semantics. [Vue reactivity][vue-reactivity].
- **UTF-16 code units, lone surrogates, array holes, property ordering, coercion,
  `this`, equality distinctions, `-0` and observable exception ordering.** Rust
  strings and Rust numeric formatting alone do not implement JavaScript.
- **Promise jobs, async functions, iteration, Map/Set and genuine WeakMap
  semantics.** Replacing WeakMap with a strong Map changes lifetime behavior.
- **Correct global environments, strict mode and modules.** “Everything is
  strict” is a restricted dialect, not a drop-in execution mode.
- **Standard eval/Function behavior when enabled.** A CSP/no-dynamic-code policy
  can restrict the controlled-app lane. It must be visible and tested; JITless
  execution does not itself require disabling dynamic JavaScript compilation.

Do not freeze intrinsics, prohibit prototype mutation or secretly transpile pages
into an engine-specific dialect in ordinary browser mode. An explicitly opt-in
restricted application profile may make stronger promises; it cannot be the
evidence for general web compatibility. [ECMAScript web legacy annex][ecma-annex].

## 7. Architecture across Butane, Sparkle, Chassis and the host

The adopted [dependency direction](docs/ARCHITECTURE.md) stays intact. Butane does
not acquire DOM, network or window ownership when it acquires a better VM.

| Layer | Owns | Boundary that needs development |
| --- | --- | --- |
| Butane | Values, GC, realms, parser/compiler/VM, ECMAScript builtins, modules and Promise jobs | Scoped roots, explicit errors, interrupt hooks, host callbacks, module requests and job queue interface |
| Sparkle | DOM/Web IDL-facing objects, events, script element behavior, web-facing realm integration | Real JS object wrappers, stable node identity, synchronous DOM access, event/microtask integration |
| Chassis | Navigation, policy, resource services, cookies/storage orchestration, session generations, CDP transport | Parent-brokered script/module/fetch requests, cancellation, policy checks and task delivery |
| mg-browser/platform | OS processes, worker isolation, clocks/entropy sources, windows and eventual executable memory | Portable host hooks, child lifecycle, deadlines, fault reporting and separately reviewed JIT service |

### 7.1 Proposed Butane embedding contract

Use typed `RuntimeId`/`RealmId` and scoped or rooted value handles rather than
public `usize` heap offsets or exposed engine pointers. Identity is scoped to its
owning runtime; stale/cross-runtime handles reject. The initial runtime executes
on one owner thread. Rust `Send`/`Sync` must not be inferred from numeric handles.

The API must support creating/destroying a realm, compiling/evaluating scripts,
calling functions, defining host objects, rooting/unrooting values, inspecting
exceptions, requesting module loads and running budgeted job checkpoints. Expose
an explicit termination outcome distinct from a catchable JS exception. Host code
supplies clocks, randomness, module bytes and capabilities. No default filesystem
loader, network fetcher or unbounded background runtime enters the worker.

Keep hot internal operations concrete. A small facade at the engine/host boundary
is sufficient; do not wrap every property access in a virtual backend trait just
to keep unused engines swappable. Trace/debug APIs can be optional capabilities.

### 7.2 GC and lifetime design

On adoption, use the selected engine's collector and prove its rooting rules at
the boundary. For original work, first evaluate precise nonmoving tracing with
explicit roots and bounded collection work, then consider nursery/compaction
only when measurements show a need. Plain `Rc` cannot reclaim arbitrary JS cycles.

The root set includes VM frames, suspended async/generator state, global and module
environments, pending jobs, host-persistent roots, live callbacks, DOM listeners,
inspector handles and compiler feedback as appropriate. WeakMap needs ephemeron
processing; WeakRef/finalizers need their specified scheduling/lifetime semantics.
Do not run arbitrary script from a Rust destructor or while holding a heap/DOM
mutable borrow. Force collection at hostile re-entry points in tests.

Model DOM-wrapper cycles explicitly: JS closure → DOM wrapper → listener → JS
closure. Prefer a single traceable ownership model inside the worker, or specify
cross-heap tracing and root removal. Permanently rooting every DOM node leaks;
dropping wrappers merely because the DOM subtree was detached breaks JS identity.
Navigation must cancel jobs and release the entire old realm generation.

Resource policy needs distinct counters for **live heap**, temporary compilation
memory, code/cache memory, cumulative allocation/work, host payloads and process
RSS/CPU. GC reclaiming an object does not refund a lifetime-work budget. Conversely,
a permanently cumulative small allocation ceiling cannot support a healthy app
indefinitely. Keep current budgets unchanged until a new bounded profile is
specified with both normal-use and hostile-workload evidence.

### 7.3 Browser scheduling and loading

Promise jobs are language machinery; HTML defines when microtask checkpoints
happen relative to tasks, callbacks and rendering. A generic async executor's
ordering is not automatically browser ordering. Implement the relevant algorithms
against the [HTML event-loop contract][html-loop], then test visible behavior.

Tasks include input, timer and resource completions. Checkpoints drain queued
microtasks, including newly enqueued work, subject to an explicit fatal resource
policy. Do not silently interleave a later input task to hide an endless Promise
chain. Rendering/rAF, mutation observers and rejection reporting need their own
specified positions, cancellation and tests as they are introduced.

Script loading must distinguish classic versus module, inline versus external,
parser-blocking/defer/async, failure and navigation cancellation. ESM requires
URL resolution and module identity, live bindings, cycles, linking, dynamic import
and top-level-await completion. HTML/Fetch policy owns MIME, CORS, CSP and origin
checks; Butane does not fetch arbitrary URLs. Introduce a bounded parent/worker
resource protocol with generation IDs and validated results.

DOM operations are synchronous from JavaScript's perspective. Keep the relevant
DOM state local to script execution, or make the synchronization model explicit.
Do not make every `appendChild` a network-style asynchronous RPC. Batch visual
projection where observable behavior permits, while preserving read-after-write
DOM semantics, layout-query synchronization and cancellation. Profile IPC and
projection costs before assuming the evaluator is the main bottleneck.

## 8. What “React and Vue work” will mean

Use **React/react-dom 19.3.0** and **Vue 3.5.42** as initial corpus candidates,
verified from [React's release index][react-versions] and [Vue's release][vue-release]
on the research date. Freeze exact packages, lockfiles, build-tool versions,
commands, browser targets and emitted asset hashes before testing. A future update
is a new corpus revision. Do not rewrite framework internals or substitute Preact
to claim React compatibility.

JSX, TypeScript and Vue single-file components are normally build inputs. The
engine should execute their standard JavaScript output. Vue explicitly supplies
a [runtime-only build for precompiled templates][vue-tooling]. This is a useful
first milestone; compiler-in-browser and Vite HMR are separate, visible extensions.

| Test lane | Required behavior | Pass evidence |
| --- | --- | --- |
| Language foundation | Lexical scopes/TDZ, closures, functions/classes, destructuring/rest/spread, symbols/iterators, strict behavior, errors and standard objects | Pinned Test262 families and reduction cases; parser acceptance alone is insufficient |
| Vue reactivity without DOM | Proxies/Reflect/descriptors, ref/computed/watch, Map/Set/WeakMap behavior and Promise-based update ordering | Upstream package tests plus our host/rooting checks |
| React core without DOM | Elements, hooks through a real renderer test harness, context, state updates and exceptions | Pinned upstream tests; no claim of browser support from this lane |
| React DOM application | `createRoot`, mounting, state updates, controlled input, delegated events, effects and cleanup, keyed list reorder, unmount/remount | Real Sparkle DOM and native/CDP input; asserted DOM/state/events plus screenshots |
| Vue application | Runtime-only component mount, reactive updates, `v-model`, computed/watch, `nextTick`, keyed lists and teardown | Same interaction and lifetime evidence; preserve node identity during reorder |
| Async application | Timers, Promise ordering, fetch completion/error/abort, dynamic import and route navigation | Controlled server, reproducible event trace, rejected/stale-generation tests |
| Extended app lane | Development builds, runtime Vue templates, hydration, router/history, transitions, common UI libraries | Add explicit capability gates; one successful counter demo is not ecosystem support |
| Long session | At least 10,000 bounded update cycles and repeated mount/unmount/navigation | Post-GC retained objects/bytes stop growing from abandoned realms; stable behavior and latency report |

Framework requirements cross subsystem boundaries. Inventory each frozen bundle
and test path for DOM calls such as `createElementNS`, text/comment nodes,
`insertBefore`, property-versus-attribute writes, event listener options,
`ownerDocument`, focus/selection and document fragments. Test timers,
`performance.now`, `queueMicrotask`, rAF and MessageChannel scheduling where used.
Support the standard path the library actually takes; do not offer fake host APIs
that return success and cause it to choose a broken branch.

Basic layout/CSS and painting must also make the tested application usable. React
DOM's [root API][react-root] is only one part of that integration. Hydration,
server components, streaming and full Next/Nuxt applications are additional
application/runtime contracts, not automatic consequences of rendering a component.

Predeclare every test's assertions. A valid initial app gate includes adding,
editing, toggling and deleting items; focus/input preservation; async success and
failure; keyed reorder without recreating unaffected nodes; effect/listener cleanup;
and navigation during pending work. Use the same assets and actions in a reference
browser. Reference engines are test oracles only, never fallback page execution.

## 9. Academic work: ideas worth turning into experiments

These papers motivate mechanisms, not performance promises for Mg. Their original
languages, hardware, workloads and compiler tiers differ from ours. The experiment
column is our proposed application of the work.

| Work | Useful idea | Butane experiment and limitation |
| --- | --- | --- |
| Hölzle, Chambers and Ungar, **Polymorphic Inline Caches**, ECOOP 1991 ([paper record][paper-pic]) | Small per-site receiver caches both speed dispatch and collect feedback | Compare generic, mono and bounded-polymorphic reads/calls on fixed app traces. Self is not JS; prototype mutation and proxies need additional guards. High priority. |
| Hölzle, Chambers and Ungar, **Debugging Optimized Code with Dynamic Deoptimization**, PLDI 1992 ([paper][paper-deopt]) | Preserve a mapping back from optimized execution to inspectable baseline state | Write the frame reconstruction contract before inlining. Force exits at each guard and compare observable effects. Read before an optimizing JIT. |
| Gal et al., **Trace-based Just-in-Time Type Specialization for Dynamic Languages**, PLDI 2009 ([paper][paper-trace]) | Type-specialize observed hot paths with guarded side exits | Educational loop experiment only initially; measure branchy framework workloads and trace explosion before selecting a tracing architecture. |
| Chevalier-Boisvert and Feeley, **Simple and Effective Type Check Removal through Lazy Basic Block Versioning**, ECOOP 2015 ([paper][paper-bbv]) | Specialize blocks on known types without a large global inference pipeline | Prototype capped block versions after bytecode/GC, compare code size and compile time with a simple baseline. Generic fallback is essential. More plausible small-team experiment than reproducing TurboFan wholesale. |
| Xu and Kjolstad, **Copy-and-Patch Compilation**, OOPSLA 2021 ([paper][paper-copy]) | Stitch precompiled instruction stencils with patched operands | Compare a tiny Rust-owned stencil baseline against dynasm/Cranelift on compile-plus-run time. Original C-like/Wasm results do not establish JS performance; original build machinery is not a pure-Rust runtime solution. |
| Xu and Kjolstad, **Deegen: A JIT-Capable VM Generator for Dynamic Languages**, 2024 preprint / [2026 publication][paper-deegen-pub] ([preprint][paper-deegen]) | Generate interpreter, baseline JIT and tier transitions from shared bytecode semantics | Borrow the single semantic source idea for a future Rust opcode DSL. Its input semantics are C++; adopting the implementation conflicts with the engine policy. Do not turn generator construction into the critical path. |
| Blackburn and McKinley, **Immix**, PLDI 2008 ([paper][paper-immix]) | Region/line-based tracing with selective evacuation offers a space/time design point | Compare against a simple collector only after representative lifetime/pause data exists. It does not remove root, barrier or weak-reference obligations. |
| Barrière, Blazy, Flückiger, Pichardie and Vitek, **Formally Verified Speculation and Deoptimization in a JIT Compiler**, POPL 2021 ([paper][paper-verified]) | Make optimization/deoptimization invariants explicit and prove transitions in a model | Use a small numeric/shape IR for executable equivalence checks or proofs. The paper's proof scope is not a proof of our Rust or emitted machine code. |
| Park et al., **JEST: N+1-version Differential Testing of Both JavaScript Engines and Specification**, 2021 ([paper][paper-jest]) | Specification-derived assertions improve differential testing beyond majority voting | Build reduced semantic cases and retain the spec rationale. V8/Boa/current Butane agreement alone is not an oracle, particularly if implementations share code. |

Suggested reading order: PICs → V8 fast properties/Ignition → Nova's
[GC/rooting guide][nova-gc] → dynamic deoptimization → basic block versioning →
copy-and-patch/Deegen. JEST-style testing should influence the harness from day one.
The garbage collector guide is implementation engineering, not a peer-reviewed
paper; it is still directly relevant to Rust ownership and rooting design.

## 10. Evidence, performance and security gates

### 10.1 Conformance without misleading percentages

Build a Test262 adapter with pinned revision, harness includes, `strict`/`noStrict`
and module modes, negative parse/early/runtime expectations, `$DONE` async handling,
fresh realms and required host hooks. Treat agent/shared-memory features as an
explicit capability lane. Never count a parse error as success for a negative
test that expects a runtime error. Preserve stderr and crash classification.

Report total discovered tests/variants, passes, assertion failures, crashes,
timeouts, unsupported capabilities and exclusions separately. Publish both the
full-suite denominator and any selected-profile denominator. A skip does not
become a pass; a shrunk profile cannot manufacture a conformance improvement.
[Test262][test262] tests the language; [WPT][wpt] tests browser integration.

Use independently implemented reference engines offline/in CI for differential
testing. Normalize only specified nondeterminism, not property order or error
types. Do not normalize away real differences in signed zero, NaN comparisons,
UTF-16 or completion ordering. A serializer that loses these distinctions is an
invalid oracle. Reject nondeterministic perf comparisons before calculating gains.

### 10.2 Benchmark the whole cost

| Metric | Method |
| --- | --- |
| Startup | Separate process start, engine/realm creation, parse, compile and first execution |
| Interaction | Same frozen app/actions/viewport; measure event dispatch through visible update and p50/p95/p99 latency |
| Throughput | Fixed useful work, identical semantics and results; separate warmup from steady state |
| Memory | Allocated bytes, live bytes after GC, process peak RSS, retained source/bytecode/native code and DOM/IPC overhead |
| GC | Pause distribution, allocation rate, collection work and retained bytes after repeated navigation |
| Compilation | Compile time, code size, tier transitions, guard failures/deopts and total time including compilation |
| Resource enforcement | Fuel/work per useful operation, cancellation latency, fatal termination and bounded cache growth |

Record CPU, OS, compiler/profile, target features, engine commits, flags, thermal
conditions and test inputs. Randomize A/B order, run enough independent processes
to estimate variation, and retain raw results. Start with at least 20 runs for
short stable cases; increase only to resolve a concrete noisy decision. Report
confidence intervals and both regressions and wins, not a best run.

Use microbenchmarks to explain a result; accept changes using the app corpus.
Eventually run complete [Speedometer 3.1][speedometer] for integrated responsiveness.
Selected compatible subtests must be labeled as a subset, never an official
aggregate score. Pure-JS suites should be separately pinned and classified;
unsupported Wasm or host features cannot disappear from the report.

For a JIT, estimate break-even as compilation cost divided by saved execution
time per call, then validate actual invocation counts and startup behavior.
Select an optimization only when its measured benefit exceeds noise and does not
hide a material memory, cold-start, correctness or tail-latency regression. There
is no evidence yet for a “within N% of V8” deadline or guaranteed speedup.

### 10.3 Bound hostile execution in every tier

Rust reduces some implementation risks but does not make a parser, GC, unsafe
value representation or generated code automatically safe. Required failure cases
include allocation storms, deeply nested parsing, prototype churn, adversarial
hash keys, regex backtracking, giant BigInt operations, runaway jobs, hostile
getters/proxies and reentrant host callbacks. The process deadline is a final
containment layer, not a substitute for cooperative engine checks.

Budget checks must cover builtins, regex, parsing, module loading, GC, compilation
and native-code execution as well as JS loop backedges. Catching a JS error must
not reset a fatal limit. Optimizations must preserve equivalent resource policy;
do not remove fuel checks just to win a benchmark. Expand the budget contract
explicitly when execution changes from AST steps to bytecodes.

Before any JIT deployment, specify W^X transitions, code-memory ownership,
relocation validation, code cache limits, invalidation, safepoints, stack maps,
platform calling conventions and threat model. A reviewed platform mechanism
could provide sealed executable code or a constrained compilation service; this
is a design decision requiring tests, not permission to add generic executable
mapping access to today's worker. Preserve a fully supported JITless lane.

Typed-array optimizations must account for detached/resizable buffers and callback
side effects before removing bounds checks. Later shared memory needs its own
memory-model/agent review. Check NaN-boxing against target pointer widths and Rust
provenance assumptions; preserve number behavior and typed-buffer bit semantics.

Use Rust fuzzing for parser/bytecode/serialization boundaries and
[Fuzzilli][fuzzilli] for structured engine tests when a suitable adapter exists.
Run forced-GC and forced-deopt modes, cached versus uncached, interpreter versus
JIT and metamorphic variants. Minimize every new failure into a regression.
Sanitizers/Miri cover suitable Rust paths; neither validates arbitrary emitted
machine code. No fuzzing against unrelated third-party services is needed.

## 11. Ordered implementation milestones

Milestones are exit gates, not calendar promises. A small team should expect
browser integration and correctness to dominate early work; V8 parity across
embedders/platforms is an ongoing program. Begin with a bounded one-to-two-week
engineering evaluation for P0/P1, then re-estimate from actual blockers. This is
a proposed time box, not a claim the framework or JIT work fits that period.

| Stage | Deliverable | Exit gate / stop condition |
| --- | --- | --- |
| **P0: Baselines** | Reproducible shell runner, frozen semantic/app inputs, current-interpreter results, proposed modern profile and resource measurements | Tests classify unsupported/crash/timeout honestly. Existing regressions and worker behavior are recorded before migration. |
| **P1: Reuse decision** | Minimal Butane↔Boa adapter in the existing child; focused Nova comparison; dependency and license records | Realms, roots, callbacks/re-entry, exceptions, Promise jobs, module requests, fatal termination, long allocations and worker syscall behavior demonstrated. Produce a decision with patch burden. No silent fallback engine. |
| **P2: Modern engine foundation** | Adopt selected engine or, if justified, parser + bytecode + GC + modern language work in original Butane | Chosen language profile passes its declared Test262 gate, all unexpected failures are resolved, cycles reclaim, root/re-entry tests pass. Existing limits are preserved or replaced by an explicit reviewed profile. |
| **P3: Web execution contract** | External classic scripts/ESM, host-controlled resource loading, jobs/tasks/timers and real DOM wrappers | WPT subsets plus cancellation/origin/CSP/error-order cases pass. Resource fetches never bypass the parent. |
| **P4: Framework milestone** | Frozen React and Vue production applications and async interaction corpus | Every declared app assertion passes without framework patches; reference comparisons, long-session tests, native/CDP input and usable rendering pass. |
| **P5: Measured interpreter performance** | One-at-a-time representation/cache/builtin/GC/startup improvements in selected engine or upstream patches | Controlled before/after evidence, no conformance regression, explicit memory/latency tradeoffs. Stop optimizing a subsystem when profiles move elsewhere. |
| **P6: Optional baseline JIT** | Cranelift or template/codegen experiment with reviewed executable-memory boundary | Same semantics/resource tests as interpreter; forced exits/GC, negative isolation tests and compile-plus-run benefit on corpus. Keep JIT off if it does not pay. |
| **P7: Embedding compatibility** | A documented subset for one pinned Rust `v8` consumer, then separately a C++ facade if selected | External consumer builds with backend/dependency selection only, executes real workload and passes handle/lifecycle/error/job tests. Unsupported APIs are documented failures. |
| **P8: Selective optimization and expansion** | Small speculative tier if justified; wider apps, inspector, architectures and Wasm as separately scoped work | Deoptimization/GC invariants proven by dedicated tests; new compatibility claims have named consumers and reproducible evidence. |

Rust embedding API design begins at P1; P7 is when a V8 compatibility claim becomes
testable. P3/P4 may progress in bounded increments while P2 grows, but no overall
framework claim is made until their combined gate passes. A parser/JIT experiment
must not displace the web-execution work that actually unlocks applications.

### P1 decision worksheet

Record for Boa, Nova and the retained baseline: pinned source/build profile;
allowed dependency graph; successful and failed tests; peak memory; cold and warm
timings; GC/rooting model; job/module API fit; interruption coverage; required
patches; upstream acceptance prospects; and expected maintenance burden.

Hard blockers include a prohibited active backend, executing page code outside
the restricted child, inability to root values through host re-entry, or unbounded
untrusted work with no enforceable containment. Missing standard features are
prioritized from failing tests, not popularity scores. A custom interpreter's
small binary is not sufficient to compensate for missing language semantics.

If Boa passes, keep its normal frontend/bytecode/GC initially, place Mg policy in
the adapter, contribute broadly useful fixes upstream when separately authorized,
and carry small pinned patches only when necessary. Do not immediately fork its
GC, replace its parser and add Cranelift at once. If neither engine passes,
publish the concrete evidence and use sections 5/7 as the original-engine design.

### Repository changes to expect, not files implemented by this plan

| Location | Intended work |
| --- | --- |
| `crates/mg-butane/` | Embedding facade, selected engine or original VM, error/resource API, shell/test adapter and engine tests |
| `crates/mg-sparkle/src/js_browser.rs` and successors | Web-facing realms, wrappers/events, script and microtask integration |
| `crates/mg-chassis/` | Bounded module/script/resource services, tasks and CDP execution-context integration |
| `src/platform/script_worker.rs` and page protocol | Versioned capabilities, generation cancellation and isolation; JIT only in a separately reviewed change |
| `tools/`, `tests/fixtures/`, CI | Pinned Test262/WPT/app runners, experiment manifests and reproduction commands |
| `docs/DEPENDENCIES.md`, `docs/JAVASCRIPT.md`, `docs/ARCHITECTURE.md` | Adopted choices and precise supported behavior after each milestone |

Each implementation increment updates feature criteria and the dated log. Changes
to user-visible capabilities still require the project's versioned release and
verified installer process. This documentation-only plan does not require a new
binary release or mark F-016 stable.

## 12. Immediate next contribution

Implement **P0 plus a narrow P1 Boa probe**: use the pinned release, no browser
fetch runtime, one worker-owned realm, rooted DOM-like host object, nested callback,
Promise ordering, a supplied module, forced interruption and a bounded allocation
stress case. Preserve the old evaluator behind a test-only selection during the
comparison. Run selected Test262 families and pure Vue reactivity without claiming
browser support. Audit and publish the exact active dependency graph.

That experiment answers the highest-value question: can we import years of Rust
language work while preserving Mg's host and resource contracts? Its result decides
whether our next major effort is browser integration or original VM construction.

## Primary sources

All links were researched on 2026-09-16. Dated release claims and inspected commit
claims are distinguished above. Moving documentation is explanatory; executable
experiments must freeze revisions. Paper performance results are not Mg results.

### Engine implementations and tools

- [Boa 0.22 release][boa-release]; [Boa 0.21 register VM/NaN-boxing background][boa-21]; [inspected engine source][boa-source]; [inspected limits][boa-limits].
- [Nova 1.0 release and limitations][nova-release]; [inspected source][nova-source]; [GC/rooting design][nova-gc].
- [Oxc parser][oxc-parser], [SWC compiler tooling][swc], [regress][regress], [ryu-js][ryu], [num-bigint][bigint], [ICU4X][icu4x], [temporal_rs][temporal], [gc-arena][gc-arena].
- [Cranelift][cranelift], [dynasm-rs][dynasm], [rquickjs binding documentation][rquickjs], [Deno Core relocation][deno-core-move].

### V8 engineering

- [Ignition][v8-ignition], [2017 pipeline transition][v8-pipeline], [Sparkplug][v8-sparkplug], [Maglev][v8-maglev], [Turboshaft/CFG transition][v8-cfg].
- [Hidden classes][v8-shapes], [fast properties][v8-properties], [elements kinds][v8-elements], [pointer compression][v8-compression], [JSON.stringify specialization][v8-json], [embedding guide][v8-embed].

### Compatibility, frameworks and tests

- [ECMAScript browser legacy annex][ecma-annex], [HTML event loops][html-loop], [Test262][test262], [WPT][wpt].
- [React versions][react-versions], [React DOM root API][react-root], [Vue release][vue-release], [Vue reactivity][vue-reactivity], [Vue build variants][vue-tooling].
- [Speedometer 3.1][speedometer], [Fuzzilli][fuzzilli]. Academic sources are annotated in section 9.

[mg-baseline]: https://github.com/pierce403/mgbrowser/commit/48cec83c0b1d41eb889373e414ded216d09cbf77
[boa-release]: https://github.com/boa-dev/boa/releases/tag/v0.22
[boa-21]: https://boajs.dev/blog/2025/10/22/boa-release-21
[boa-source]: https://github.com/boa-dev/boa/tree/69388e59f789ed0846a8d6aad1e4dc0c91b35816
[boa-limits]: https://github.com/boa-dev/boa/blob/69388e59f789ed0846a8d6aad1e4dc0c91b35816/core/engine/src/vm/runtime_limits.rs
[nova-release]: https://trynova.dev/blog/nova-1.0
[nova-source]: https://github.com/trynova/nova/tree/4eea7c6fae180a8a2eed45c3f6acd0d9256522b1
[nova-gc]: https://trynova.dev/blog/guide-to-nova-gc
[oxc-parser]: https://github.com/oxc-project/oxc/blob/main/crates/oxc_parser/README.md
[swc]: https://swc.rs/docs/usage/core
[regress]: https://github.com/ridiculousfish/regress
[ryu]: https://docs.rs/ryu-js/1.0.3/ryu_js/
[bigint]: https://docs.rs/num-bigint/latest/num_bigint/
[icu4x]: https://icu4x.unicode.org/
[temporal]: https://github.com/boa-dev/temporal
[gc-arena]: https://github.com/kyren/gc-arena
[cranelift]: https://cranelift.dev/
[dynasm]: https://censoredusername.github.io/dynasm-rs/language/index.html
[rquickjs]: https://docs.rs/rquickjs/latest/rquickjs/
[deno-core-move]: https://github.com/denoland/deno_core
[v8-ignition]: https://v8.dev/docs/ignition
[v8-pipeline]: https://v8.dev/blog/launching-ignition-and-turbofan
[v8-sparkplug]: https://v8.dev/blog/sparkplug
[v8-maglev]: https://v8.dev/blog/maglev
[v8-cfg]: https://v8.dev/blog/leaving-the-sea-of-nodes
[v8-shapes]: https://v8.dev/docs/hidden-classes
[v8-properties]: https://v8.dev/blog/fast-properties
[v8-elements]: https://v8.dev/blog/elements-kinds
[v8-compression]: https://v8.dev/blog/pointer-compression
[v8-json]: https://v8.dev/blog/json-stringify
[v8-embed]: https://v8.dev/docs/embed
[ecma-annex]: https://tc39.es/ecma262/#sec-additional-ecmascript-features-for-web-browsers
[html-loop]: https://html.spec.whatwg.org/multipage/webappapis.html#event-loops
[react-versions]: https://react.dev/versions
[react-root]: https://react.dev/reference/react-dom/client/createRoot
[vue-release]: https://github.com/vuejs/core/releases/tag/v3.5.42
[vue-reactivity]: https://vuejs.org/guide/extras/reactivity-in-depth.html
[vue-tooling]: https://vuejs.org/guide/scaling-up/tooling.html
[test262]: https://github.com/tc39/test262
[wpt]: https://web-platform-tests.org/
[speedometer]: https://browserbench.org/Speedometer3.1/
[fuzzilli]: https://github.com/googleprojectzero/fuzzilli
[paper-pic]: https://bibliography.selflanguage.org/pics.html
[paper-deopt]: https://bibliography.selflanguage.org/_static/dynamic-deoptimization.pdf
[paper-trace]: https://dl.acm.org/doi/10.1145/1542476.1542528
[paper-bbv]: https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2015.101
[paper-copy]: https://arxiv.org/abs/2011.13127
[paper-deegen]: https://arxiv.org/abs/2411.11469
[paper-deegen-pub]: https://dl.acm.org/doi/10.1145/3798246
[paper-immix]: https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf
[paper-verified]: https://janvitek.org/pubs/popl21.pdf
[paper-jest]: https://arxiv.org/abs/2102.07498
