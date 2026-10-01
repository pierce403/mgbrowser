# JSPLAN P0 / narrow P1 results

2026-09-16 later update: the user subsequently authorized actual page execution.
The v0.4.0 [Boa integration](../BOA.md) has a distinct process-contained profile;
full P1 resource control remains open. The original decision and measurements
below are preserved as historical evidence, not rewritten as adoption acceptance.

Initial 2026-09-16 decision: **continue with Boa as the preferred research candidate;
do not adopt it for browser pages yet.** The first contribution specified in
JSPLAN section 12 is implemented. This does not complete the full P1 adoption
gate, P2 modern engine foundation, or any browser framework/V8 milestone.

Pulled `main` to `2f7d6f2ca63de0d83cd056574bc20b957e13dfc3` before work.
The original evaluator matches plan baseline
`48cec83c0b1d41eb889373e414ded216d09cbf77`: no Butane language source or existing
assertion changed. At this checkpoint, production `Cargo.lock`, version 0.3.0 and
page execution remained unchanged. Boa existed only in the excluded workspace.

## Implemented and measured

[Reproduction commands](../../experiments/jsplan/README.md) build a real Rust
executable, then launch one isolated child per case. It requires an empty
environment and installs the shared production policy before reading source:
closed descriptors, seccomp, non-executable anonymous memory only, 256 MiB address
space and one CPU second. The external supervisor adds a two-second deadline,
bounded request/output/error pipes, kill/reap and per-process measurements.

The isolation implementation was moved unchanged into a shared source file,
not copied into a divergent permissive research sandbox. Existing BPF/denial
regressions and actual browser-worker tests pass. The experimental CLI is not
installed or packaged. No source can select Boa from a browser page.

| Probe | Observed result |
| --- | --- |
| Host roots and nested callback re-entry | Same rooted node identity survives forced GC inside nested callbacks; value changes from 41 to 42; zero finalizations while rooted. |
| Promise checkpoint | Before draining: `sync`. After draining: `sync,first,second,nested`. Jobs are driven explicitly by the host. |
| Supplied module | In-memory dependency live binding increments to 42. Unknown modules do not acquire file or network authority. |
| Fatal loop interruption | Uncatchable engine limit leaves marker zero; the adapter refuses a later script in that realm. This is not comprehensive work accounting. |
| Cyclic allocations | Ten batches of 1,000 native-backed JS self-cycles finalize exactly at each forced-GC checkpoint; a separate rooted node remains usable. Not a live-heap or DOM-cycle measurement. |
| Worker capabilities | File open, socket/process creation and executable mapping return EPERM inside the research child. Existing broader worker selftests are preserved. |

Boa's fixed clock and supplied-only module loader demonstrate host control of
those facilities. `Math.random` is not host-injected in the pinned engine.
Callback-safe diagnostics inspect raw data/prototype identities: hostile error
getters and replaced constructors cannot run during reporting. Test262 assertion
failures are distinguished from ordinary exceptions without guessing from strings.

## Frozen language and reactivity profile

[Input manifest](../../tools/jsplan/inputs.json): Test262
`07eded464b6ce232331835198efccddc6e26eb08`, exact official archive and extracted-tree
hashes; Vue `@vue/reactivity` 3.5.42, exact unmodified global production bundle
hash; and hashed authored inputs. Dependencies pin Boa 0.22.0 release source
`337a3668a0dc86dd401ea20906e782249a64a228`. Default features are disabled.

Full Test262 inventory: **53,582 test files / 102,926 variants**, plus 294 support
fixtures excluded from standalone execution. Selected profile: **88 files /
173 variants**. It includes complete Proxy `get`, Map `set`, WeakMap `set` and
Promise `resolve` families, plus five named lexical/arrow/module files.
The other 53,494 files are not executed. No full-suite percentage is claimed.

| Release-build result | Original Butane | Boa 0.22 |
| --- | ---: | ---: |
| Selected Test262 passes | 0 | 169 |
| Selected Test262 unsupported | 110 | 2 |
| Selected Test262 exceptions | 63 | 2 |
| Six host/containment probes | 1 pass, 5 unsupported | 6 pass |
| Two authored semantic cases | 1 pass, 1 unsupported | 2 pass |
| Vue reactivity without DOM | Unsupported | Pass |
| Total, 182 cases per engine | 2 pass, 117 unsupported, 63 exceptions | 178 pass, 2 unsupported, 2 exceptions |

Both Boa exceptions are `WeakMap/prototype/set/adds-symbol-element.js`, in sloppy
and strict modes: a symbol key throws TypeError. Both unsupported variants are
`Proxy/get/trap-is-not-callable-realm.js`, requiring the absent `$262` realm hook.
Neither gap is removed from the denominator. Current Butane also fails the
standard harness in some variants; its authored regression suite remains separate
and passes. These counts do not mean that it has zero useful JS behavior.

The Vue exercise runs reactive/ref/computed, effects/watch cleanup, Map/WeakMap
object identity, 1,000 disposable effect scopes and a Promise checkpoint using
the unchanged package. **No Vue DOM application, React renderer, HTML task loop,
network fetch, external browser script or full framework compatibility is tested.**

Every case has a fresh process/realm. Strict/raw/module goals, ordered harness
includes, typed parse/early/resolution/runtime negatives and actual async
completion are handled separately. Wrong-phase or untyped errors never pass a
negative test. [Reviewed outcomes](../../tools/jsplan/expectations.json) gate all
364 results, including the known failures. CI rejects missing/duplicate cases,
changed denominators, altered inputs and any changed classification. Expectation
updates require review, not automatic score acceptance.

## Resources and limits of the evidence

Rust 1.91.1 release build on Linux x86_64, Intel Core Ultra 7 165H. The supervisor
records executable/source/input hashes, compiler, CPU/OS and raw bounded output.
The initial direct wait4 measurement inherited the large Python launcher's memory
high-water state: those RSS numbers are discarded as engine measurements. The
final runner uses a freshly exec'd GNU time supervisor with a private measurement
file, recording engine and supervisor peaks separately. Fixture stderr cannot
forge that file after isolation. Test tools are not shipped browser dependencies.

One controlled-input correctness run measured approximately 6-8 MB peak process
RSS for the original baseline, 6-16 MB for Boa, and about 56 ms end-to-end for the
Vue case. These are coarse process measurements, not GC live bytes or a speed
comparison. Baseline preflight parsing duplicates its existing evaluator parse;
adapter work is asymmetric. Thermal state was uncontrolled and no randomized
20-run performance study was performed. Exact run-specific values remain in the
machine-readable report and CI artifact. The final local executable SHA-256 was
`e49b8f86e3db65f711f2c1b5bc2eda8ffc81d39bb2209555d0be44b9e972edeb`;
the baseline source fingerprint was
`ed89c0f7c2b40458b2b26f77e5bfb47648b1bdb7d3e303dcbab51d7d3ca948d1`.

The debug Vue run exited on signal 9 near one CPU second; the release run passed
without changing input or caps. A separate endless-Promise-chain probe also exited
on signal 9 at roughly one CPU second, without a structured engine termination.
This is consistent with the final OS CPU cap, not a cooperative job-work limit
or the parent's two-second timeout. Raw signals remain classified as crashes;
they do not become successful negative tests. Neither failure was hidden by
raising limits or patching framework code.

## Adoption decision and next gate

The [exact dependency/license audit](DEPENDENCY_AUDIT.md) covers 133 active
packages and 16 build scripts. It found no selected prohibited C/C++ backend.
Boa builds on Rust 1.91.1. The pinned Nova alternative fails that toolchain gate:
14 Oxc 0.124.0 crates require Rust 1.92. No Nova execution/performance pass is
claimed. Nova's Linux USDT path is Rust/ELF tracing machinery, not automatically
a forbidden C engine; its worker behavior remains untested.

Continue Boa's normal frontend, VM and collector. No engine fork or upstream
patch has been adopted; the current patch burden is confined to this Mg probe.
The unpriced future maintenance surface is significant:

1. Specify and implement fatal cumulative work control across VM jobs, parsing,
   compilation, builtins, regex, GC and callback re-entry. Loop counters alone
   are insufficient, and process termination is only final containment.
2. Specify an explicit application profile separating live heap, temporary/code
   memory, host payload, cumulative work and process caps. Measure honest normal
   and hostile workloads before assigning/replacing production limits. Current
   4 MiB cumulative logical accounting is not equivalent to a GC-box threshold.
3. Prove retained realm cancellation, stale/cross-runtime handles, real DOM
   wrapper/listener cycles, teardown and host roots. Current host-like objects
   are not the production DOM bridge or a stable embedding API.
4. Resolve/classify language gaps against an expanded, predeclared Test262
   profile. Price host-randomness and budget hooks before choosing patches.

No evidence here justifies replacing the original interpreter today, silently
raising its limits, keeping dual production engines or starting JIT work. Full
P1 remains open. Browser adoption, scripts/tasks/DOM, React/Vue applications and
V8 embedding are later gates in order, not implied by this experiment.

## Regression and publication policy

All existing CI steps pass locally: 1,276 debug tests, 1,177 selected release
tests, independent no-chrome embedding, allocation probes, packaged installer /
updater, worker denial tests, and 26 native / 26 external CDP fixture journeys.
The research executable adds seven protocol and 18 harness tests plus the frozen comparison.
No old language, resource, compatibility or AST-array assertion was weakened.

This increment adds developer research tooling, not a browser-visible capability:
the public release remains **v0.3.0** and old tags remain immutable. The website
describes the experiment without advertising Boa-backed browsing. Any later
browser feature still requires a new versioned release and verified public install.
Exact-commit remote CI/Pages evidence is recorded in the dated project log.
