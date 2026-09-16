# JSPLAN P1 dependency and host-contract audit

2026-09-16. Scope: Linux `x86_64-unknown-linux-gnu`, Rust 1.91.1, an isolated
research executable. This is **not production engine adoption**, a security audit
of every upstream line, a Test262 score, or framework/browser compatibility.
The browser workspace `Cargo.lock` and its published v0.3.0 dependency graph stay
unchanged. No network runtime, Boa CLI or foreign JavaScript engine is imported.

## Reproducible inputs

The authoritative experiment is
[`experiments/jsplan/Cargo.toml`](../../experiments/jsplan/Cargo.toml), with its own
[lockfile](../../experiments/jsplan/Cargo.lock). `boa_engine = 0.22.0` and
`boa_gc = 0.22.0` disable default features. Both, plus `boa_ast`, `boa_interner`,
`boa_macros`, `boa_parser` and `boa_string`, have published `.cargo_vcs_info.json`
matching Boa release commit `337a3668a0dc86dd401ea20906e782249a64a228`.
The two utility dependencies, `small_btree = 0.1.0` and `tag_ptr = 0.1.0`, instead
identify `ad8739f5e0b51d20faf7a2cce98afa5c40121438`; they are not mislabeled as
v0.22 source. Registry archives are independently pinned by Cargo checksums.

[`DEPENDENCIES.json`](DEPENDENCIES.json) records the exact manifest/lock hashes,
normal/build dependency tree, selected feature union, archive checksums, source
revisions, licenses and license-text hashes, build-script hashes and declared
MSRVs. [`DEPENDENCY_LICENSES.md`](DEPENDENCY_LICENSES.md) is the compact inventory.
Host and target feature units are preserved in the tree; a package row's features
are their union, not a claim that every feature is linked into the executable.

After fetching/building the locked experiment:

```sh
cargo tree --locked --manifest-path experiments/jsplan/Cargo.toml \
  --target x86_64-unknown-linux-gnu --edges normal,build
python3 experiments/jsplan/dependency_audit.py --check
```

Use `python3 experiments/jsplan/dependency_audit.py` to regenerate after an
intentional reviewed change. The checker uses the existing native-backend denylist,
rejects new Cargo `links` declarations/native implementation source archives,
checks exact Boa source revisions and rejects unreviewed license-text omissions.
It does not substitute for reviewing new dependencies, features or build scripts.
Cargo packages must already be cached: this check runs `--locked --offline`.

## Boa: selected graph and licenses

The frozen experiment selects 133 active normal/build packages: 131 third-party
packages, the unchanged original `mg-butane`, and the research host. Sixteen
packages have build scripts. No active `ring`, OpenSSL, native engine/codec/font
backend, native allocator, `cc`, `cmake` or `pkg-config` dependency was found.
There are no Cargo `links` declarations or bundled C/C++ source/library archives
in this selected registry graph.

Boa's `temporal`, `float16`, `xsum`, `annex-b`, `intl`, `intl_bundled`, `js`,
`native-backtrace` and `fuzz` features are off. ICU4X normalization/property data
remain active: ordinary string normalization and parser character properties
need them independently of ECMA-402 `Intl`. `regress = 0.12.0` selects its Rust
implementation with UTF-16 support. Absence of optional language features means
this configuration must not inherit upstream whole-configuration conformance
claims. The graph contains async utility types, not a browser fetch runtime.

Build-script review:

| Packages | Selected build behavior |
| --- | --- |
| `crossbeam-utils`, `getrandom`, `parking_lot_core` | Rust/platform/sanitizer cfg selection. |
| `icu_normalizer_data`, `icu_properties_data` | Bundled-data cfg selection; no native ICU library. Clear inherited `ICU4X_DATA_DIR` in controlled builds if using custom build environments. |
| `libc`, `portable-atomic` | Rust compiler/target/version capability checks. Non-Linux probes are not the selected target. |
| `num-traits`, `proc-macro2`, `quote`, `rustversion`, `thiserror` | Rust compiler probes and/or generated Rust cfg/private/version material. |
| `serde`, `serde_core`, `serde_json`, `zmij` | Rust version/cfg checks and generated Rust material where applicable. |

The selected Linux branches do not invoke a C/C++ compiler or download an engine.
Proc macros produce Rust code. This is separate from the system linker, Rust
standard library and operating system boundary already permitted by the project.

`libc = 0.2.189` is present directly in the isolated host for the same process
controls as the browser. Engine-side paths include `getrandom = 0.4.3` for entropy,
`parking_lot_core = 0.9.12` for synchronization, and `time = 0.3.55` for system
local-time queries (`localtime_r`). These are OS-interface paths, not C/C++ JS,
crypto, image or font backends. This experiment does not alter the reusable
production component guard or approve direct platform calls inside Butane.

License inventory found declared permissive licenses: MIT/Apache choices,
Unlicense/MIT, Unicode-3.0, Zlib and the recorded alternate-license expressions.
No obvious prohibited or missing-license expression appeared in the selected
graph. Preserve upstream notices, especially Unicode data terms; Apache-2.0 for
Mg does not relicense its dependencies. The nine Boa/core utility archives omit
their root license text. The exact two source revisions above carry the same
[MIT notice](licenses/boa-MIT.txt), retained here under a narrow reviewed
exception. All other selected third-party packages contain license/notice files;
their hashes are recorded. The research executable is not a browser release
asset. Packaging it later requires collecting all those third-party texts.

An isolated engine-only source check at the exact release revision passed
`cargo check --locked --offline` with Rust 1.91.1. Boa declares MSRV 1.91.0.
The checked-in probe's execution/containment results are recorded separately:
successful dependency compilation alone does not prove safe host integration.

## Boa: explicit adoption blockers and patch surface

Pinned-source observations, distinct from dynamic probe results:

| Contract | Existing support | Work required before adoption |
| --- | --- | --- |
| Rooting and re-entry | `JsObject`/`Gc` handles, tracing derives, host callbacks, explicit realms and forced collection. | Preserve trace edges through retained DOM wrappers/listeners; prove re-entry and realm teardown. Do not hide GC-managed values in untraced closure captures. |
| Promise jobs | Caller-provided `JobExecutor` and explicit job draining. | Bound queued work and cumulative checkpoints; supply HTML task/microtask ordering separately. A successful Promise probe is not a browser event loop. |
| Modules | Caller-provided `ModuleLoader`, module linking/evaluation. | Supply only admitted source; reject unknown requests. Browser URL/origin/MIME/CSP/cancellation policy remains parent-owned. Do not use the filesystem loader. |
| Engine termination | `RuntimeLimitError` is an engine error, explicitly not catchable by JS. Loop, recursion and stack limits exist. | Latch fatal termination across later host calls/transactions. Loop counts are per frame, not an allocation/work quota over a retained realm. |
| VM work | Async execution can yield when an opcode budget is consumed. | Yielding resets the budget; it is not fatal interruption. One opcode can run a long synchronous builtin. The `instructions_remaining` API is fuzz-feature-only and is not enabled here. |
| Parsing, builtins, RegExp, GC | Rust implementations, with some local algorithmic checks. | No comprehensive host-controlled fatal budget was found across all these phases. `RegExpBuiltinExec` calls `regress` directly; parser/compiler and GC work occur outside a general host safepoint contract. Review/patch coverage, including reentrant callbacks. |
| Memory | Mark/sweep collector with ephemerons and an adaptive collection threshold. | Its private `bytes_allocated` counter counts GC boxes, not every owned vector/string/parser allocation. It is not a hard live-heap limit or a replacement for Mg's cumulative 4 MiB logical budget. Specify separate measured live/temporary/work/process limits. |
| Clocks and randomness | Context clock and HostHooks time/time-zone methods are configurable. | `Math.random` directly calls `rand::random`, not an injectable host hook in this pin. Deterministic host-provided randomness would require an explicit adapter policy or upstream patch. |

The current child address-space/CPU/deadline/seccomp boundary is final containment,
not proof that cooperative resource control passed. Rust-only source does not
make unsafe value representation, GC, regex complexity or arbitrary host callbacks
safe automatically. Nothing in this audit permits executable-memory mappings,
unrestricted filesystem/network access, or running page scripts in the parent.

Primary pinned code: [manifest](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/engine/Cargo.toml),
[runtime limits](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/engine/src/vm/runtime_limits.rs),
[VM](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/engine/src/vm/mod.rs),
[error classification](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/engine/src/error/mod.rs),
[GC](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/gc/src/lib.rs),
[host hooks](https://github.com/boa-dev/boa/blob/337a3668a0dc86dd401ea20906e782249a64a228/core/engine/src/context/hooks.rs).

## Nova comparison: source and build gate only

Pin: `4eea7c6fae180a8a2eed45c3f6acd0d9256522b1`. An isolated path-dependent
`nova_vm = 1.0.0`, `default-features = false` manifest resolved its Linux graph,
then `cargo check --locked --offline` on Rust 1.91.1 **failed before compilation**:
14 selected Oxc 0.124.0 crates require Rust 1.92.0. No newer compiler or different
engine/frontend version was substituted. No Nova runtime, GC performance,
conformance or worker pass is claimed.

The no-default graph has 141 third-party packages and no known forbidden backend
or native compiler helper. It still includes unconditional `usdt = 0.6.0` as a
normal and build dependency. Its selected Linux build script chooses the
`stapsdt` backend: Rust proc macros emit inline assembly/ELF probe notes, not C
headers or a native JS library. The macOS branch uses `dtrace -h`; that branch is
not active here and cannot be called approved by this Linux review. Tracing,
`memmap2` and OS-call behavior still need actual worker testing if Nova is
reconsidered. Nova's build script generates Rust builtin-string tables.

Nova and its small-string/structure-of-arrays dependencies retain their MPL-2.0
identity. The source-level host API provides job and imported-module hooks, rooted
realms and `GcScope`/`NoGcScope` lifetimes. These are useful design references,
not proven drop-in compatibility with Mg. No comprehensive interruption/work
budget API was identified in the reviewed execution/VM interfaces. Its optional
RegExp implementation currently uses Rust `regex::RegexBuilder` and lossy source
conversion; that needs a dedicated ECMAScript/UTF-16 review before adopting a
RegExp-enabled profile. It is disabled in the minimal graph inspected here.
Do not carry forward an older sparse-array failure claim without a current
reproducer: this audit did not run one.

Source archive SHA-256s used for inspection:

- Boa release archive: `af7132a18b78846b23ac0b4de62251016415fefd948faacdd9d713b0e61195fd`.
- Nova pinned archive: `1930b890fc30fbaf3bb540f52062baa8a1e34cb1d4e0843e1e49d9efb309504a`.

These archive hashes identify this inspection download, not the authoritative
Cargo registry graph. Nova's scratch graph is exploratory, not an adopted or
maintained second engine configuration. The hard MSRV blocker follows the pinned
manifest's exact Oxc requirements independently of later transitive resolution.
Primary source: [Nova manifest](https://github.com/trynova/nova/blob/4eea7c6fae180a8a2eed45c3f6acd0d9256522b1/nova_vm/Cargo.toml),
[host interface](https://github.com/trynova/nova/blob/4eea7c6fae180a8a2eed45c3f6acd0d9256522b1/nova_vm/src/ecmascript/execution/agent.rs),
[RegExp source](https://github.com/trynova/nova/blob/4eea7c6fae180a8a2eed45c3f6acd0d9256522b1/nova_vm/src/ecmascript/builtins/regexp/data.rs).

## Recommendation

Continue the restricted **Boa-first P1 experiment**, keeping Boa's frontend, VM
and collector intact. The selected dependency/toolchain gate is feasible; Nova
does not improve that gate at its pin. Do not switch page execution yet. Before
P2, decide the resource profile and price the interruption/allocation/rooting
patch surface with measured tests. Production adoption remains conditional on
that evidence, retained-realm lifecycle tests and the explicit decision worksheet
in `JSPLAN.md`. No new toolchain, cap increase, default-script enablement, JIT,
browser event loop or fallback engine is authorized by passing this audit.
