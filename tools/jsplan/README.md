# JSPLAN P0/P1 research runner

This is a bounded baseline and engine-integration experiment, not a new browser
execution path, a complete autoresearch executor, or a Test262 conformance claim.
The production browser and its existing resource limits are unchanged. The
experimental executable installs the existing worker confinement before reading
the request. Python additionally starts each case in a fresh child with an empty
environment, a two-second wall deadline, bounded pipes and forced child reaping.
Cleanup identifies the supervised child with a Linux pidfd and rechecks ownership
before signaling. A 100 ms reaping grace escalates to the still-owned supervisor
process group; nonblocking cleanup stops after one second and reports a failure
if reaping did not complete. A reaped numeric process identity is never signaled.

## Reproduce

Use Linux x86_64, Python 3, GNU `/usr/bin/time` and the experiment's pinned Rust toolchain. Build the
probe using the instructions in `experiments/jsplan/README.md`, then run from the
repository root:

```sh
python3 tools/jsplan/test_runner.py
python3 tools/jsplan/runner.py fetch
python3 tools/jsplan/runner.py run \
  --executable experiments/jsplan/target/release/mg-jsplan-probe \
  --engine baseline --engine boa --build-profile release --output tmp/jsplan-report.json
python3 tools/jsplan/check_report.py tmp/jsplan-report.json
```

`fetch` is the only network operation. It downloads the exact official Test262
revision and Vue package archives in `inputs.json`, verifies their SHA-256 hashes,
and rejects links, unsafe paths and oversized archives. Downloads and extracted
third-party sources/licenses remain under ignored `tmp/jsplan-inputs`. The runner
verifies the extracted Test262 tree, Vue bundle and authored fixture hashes before
execution. Changed inputs need a reviewed manifest change and a new baseline.
The original Butane `Cargo.toml` and `src/**/*.rs` fingerprint matches the pinned
v0.3.0 baseline; the runner checks it before each comparison. `--build-profile`
records the caller's declaration, not an inferred claim from a pathname; the
executable's content hash is the exact measured build identity.
No reference engine executes page code, and no fallback engine is selected.

`--lane probe`, `--lane authored`, `--lane vue-no-dom` and `--lane test262` select
explicit report lanes; repeat a flag to combine lanes. Even a lane-filtered report
retains the full pinned corpus and selected-profile denominators. Engine failures
are report data, not infrastructure failures: run exits 2 for invalid setup or
protocol, but a successful report can contain semantic failures. CI must check
the declared expectations in the report rather than assuming exit 0 means all
tests passed. `check_report.py` requires the exact reviewed per-case outcome
inventory, including known failures and unsupported capabilities. Missing cases,
changed outcomes and partial/lane-filtered reports fail that gate. Expectations
are never automatically regenerated in CI. Results are written after every case so interrupted runs retain
partial evidence; partial results must not be claimed as the full profile.

## Frozen scope and denominators

Test262 revision `07eded464b6ce232331835198efccddc6e26eb08` has 53,582 runnable
JavaScript files and 102,926 strict/sloppy/module/raw variants under `test/`;
294 `_FIXTURE` JavaScript support files are not standalone tests. The count
includes staging and Intl tests even though they are outside this selected lane.
Four complete families are selected: Proxy `get`, Map `set`, WeakMap `set`, and
Promise `resolve`. Five named lexical/arrow/module files cover early/runtime
negative and module-goal handling. This is 88 files / 173 variants, excluding
53,494 full-suite files. Cases using unavailable `$262` host hooks are reported
unsupported, not removed from the selected denominator. No percentage is claimed.

Each variant gets a new process and realm. `assert.js` and `sta.js` are executed
separately before the main source; async tests also load `doneprintHandle.js`,
then ordered requested harness includes. Strict variants prepend the required
directive to the test only. Raw sources receive no modifications or harness.
Module sources retain their module parse goal. Parse/early, resolution and
runtime negative phases and error constructor names must match: a parse failure
cannot pass a runtime-negative test. Narrow probes exercise a supplied in-memory
module; this selected Test262 profile does not claim general module-fixture or
agent-host support. Async success requires the actual completion signal after
job draining; missing completion is a timeout, not success.

Authored fixtures assert coercion order, array holes, signed zero, NaN, UTF-16
lone surrogates and closures in JavaScript, avoiding a lossy JSON value oracle.
The modern fixture adds language/Proxy/collection checks. The unmodified
`@vue/reactivity` 3.5.42 global production bundle runs reactive/ref/computed,
effect/watch cleanup, Map/WeakMap identity, 1,000 disposable effect scopes and a
Promise completion check. This is not a Vue DOM application or a framework
compatibility claim, and effect-scope disposal alone does not prove heap recovery.
Named native probes separately check confinement, roots/re-entry, jobs, modules,
fatal interruption and cyclic allocations.

## Protocol and evidence

One JSON request goes to stdin:

```json
{"protocol":1,"engine":"boa","goal":"script","action":"evaluate","source":"true;","includes":[],"modules":{},"drain_jobs":true,"async":false}
```

Goals: `script` / `module`; actions: `evaluate`, `parse`, `probe`. Includes contain
`name` and `source`; modules map host-approved specifiers to source strings.
Named probes add `probe`. The response has `protocol:1`, `outcome` (`ok`,
`exception`, `unsupported`, `termination`), optional `phase` (`harness`, `parse`,
`early`, `resolution`, `runtime`), `error_type`, `message`, `value`, `done`
(`ok`, `error`, `missing`) and engine-specific `metrics`. Positive authored cases
require boolean `true`; negative Test262 cases require a typed matching phase.
Original Butane's untyped approximations do not become typed successes.

Distinct result categories: pass, assertion-failure, unsupported, exception,
crash (raw signal), timeout, termination, harness-failure and infrastructure-error.
The report preserves bounded stdout/stderr, exit status, exact request hashes,
input pins, executable hash, source commit/worktree state, CPU/OS/Python/compiler,
wall time and child CPU/peak RSS from GNU time's Linux `wait4`, not the cumulative
`RUSAGE_CHILDREN` high-water mark. Peak RSS is process memory, not GC live bytes.
The trusted GNU time wrapper starts after an exec boundary so the Python corpus
inventory does not inflate the measured Rust child's pre-exec RSS. The report
keeps the inflated supervisor high-water mark separately. Child CPU values have
GNU time's 0.01-second precision; wall time covers wrapper startup too.
The timing includes startup, compilation and execution and is one correctness
sample, not an A/B performance result. Thermal conditions are uncontrolled;
20-run randomized comparisons and confidence intervals remain a later experiment.

The parent request/stdout/stderr caps are 2 MiB / 4 MiB / 64 KiB. The child retains
the existing 256 MiB address-space and one CPU-second process caps. These do not
prove Boa has a complete cooperative parser/builtin/regex/GC work budget. Browser
adoption remains gated on the P1 decision and later application-profile work.

Third-party tests and framework code retain the licenses in their downloaded
archives (Test262 BSD and Vue MIT); they are not relicensed as project code.
Primary pinned interpretation rules:
[Test262 INTERPRETING.md](https://github.com/tc39/test262/blob/07eded464b6ce232331835198efccddc6e26eb08/INTERPRETING.md).
