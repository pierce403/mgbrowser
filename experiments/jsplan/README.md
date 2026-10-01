# Isolated JSPLAN experiment

2026-09-16 update: the later user-authorized v0.4.0 page integration uses a
separate [Boa process-contained profile](../../docs/BOA.md). This document and
its measurements preserve the initial research increment, not the production
backend's current capabilities or resource accounting. Full P1 remains open.

This **research-only** executable compares original Butane with pinned Boa 0.22.
It is not linked into `mgbrowser`, not included in releases and not an alternate
browser backend. The separate workspace/lockfile prevents research dependencies
or feature unification from changing the production graph.

Linux x86_64, Rust **1.91.1**, Python 3 and `/usr/bin/time` (measurement only):

```sh
cargo +1.91.1 build --locked --release --manifest-path experiments/jsplan/Cargo.toml
cargo +1.91.1 test --locked --manifest-path experiments/jsplan/Cargo.toml
python3 experiments/jsplan/dependency_audit.py --check
python3 tools/jsplan/test_runner.py
python3 tools/jsplan/runner.py fetch
python3 tools/jsplan/runner.py run \
  --executable experiments/jsplan/target/release/mg-jsplan-probe \
  --build-profile release \
  --engine baseline --engine boa --output tmp/jsplan-report.json
python3 tools/jsplan/check_report.py tmp/jsplan-report.json
```

Use the Python supervisor, not a direct unbounded shell invocation. It provides
the parent wall deadline, pipe caps, per-child measurements, kill and reap. Each
fresh child requires an empty environment and installs the **same source policy**
as the browser before reading input. The shared implementation is
`src/platform/script_isolation.rs`: no syscall, process cap or executable-memory
permission changed. Unsupported targets fail to compile instead of running
without containment. Script requests contain local frozen fixtures, not live
website code or network authority.

The probe admits at most 2 MiB JSON, 1 MiB per source, 32 includes and 32 supplied
modules. Child limits remain 256 MiB address space and one CPU second; the parent
deadline is two seconds. Boa's loop count of 1,000,000, recursion 64 and stack
limit 65,536 are **not equivalent** to Butane's cumulative 1,000,000 fuel / 4 MiB
logical allocation contract. Missing comprehensive budgets are an adoption
blocker, not permission to change the production limits.

The original evaluator is an explicitly selected baseline and is never retried
after Boa fails. It retains its untyped diagnostics: negative tests cannot pass
by guessing error types from message text. Its research adapter parses before
execution to identify the failure phase, then the unchanged evaluator reparses
and charges normally. Consequently these timings are not an equal-work benchmark.
Unsupported modern syntax stays unsupported; no existing regression is weakened.

See [protocol and frozen inputs](../../tools/jsplan/README.md),
[dependency/license audit](../../docs/jsplan/DEPENDENCY_AUDIT.md) and the
[decision and results](../../docs/jsplan/RESULTS.md). Boa clocks are fixed in
the probe; Math.random is not host-injected at this pin. Supplied modules use
an in-memory map only. No Boa filesystem loader, CLI, fetch runtime or JIT is used.

At the initial tooling-only checkpoint, `mgbrowser --enable-scripts` still ran
original Butane and no new browser binary was published. The later integration
is documented separately above; its release/public-install receipts belong in
the dated work log, not these frozen experiment results.
