# Boa production dependency and regression boundary

Reviewed 2026-09-16 for Linux x86_64 and Rust 1.91.1. This supplements the
[pinned research audit](DEPENDENCY_AUDIT.md), not a claim that Rust dependencies
or a passing corpus make arbitrary page execution safe.

## Selected implementation and features

`mg-butane` has an optional `modern` feature using Boa 0.22.0. Sparkle selects
it for the new page integration. The original interpreter remains available to
its frozen regression tests and the separately locked research baseline. There
is no runtime retry of a failed Boa page using the original interpreter.

The seven Boa crates retain release source revision
`337a3668a0dc86dd401ea20906e782249a64a228`. `small_btree` and `tag_ptr` 0.1.0
retain their distinct reviewed revision
`ad8739f5e0b51d20faf7a2cce98afa5c40121438`. Defaults remain disabled. Production
adds only Boa's `fuzz` feature to expose the existing cumulative VM instruction
counter; this is not a fuzzer service, JIT, filesystem loader or network runtime.
The counter does not count all parser/compiler, builtin or GC work by itself.

The additional `arbitrary` and `derive_arbitrary` 1.4.2 crates declare
`MIT OR Apache-2.0`, Rust 1.63 and source revision
`dc22fdefd5456a0f4d2f190c6d3e017e3c7ddd8e`. They implement Rust data-generation
traits and Rust proc-macro derives. Neither has a build script, Cargo `links`,
native source/archive, downloaded implementation or C/C++ backend. Actual
production features, including the Boa AST/interner `arbitrary` support and GC
collection adapters, are pinned by `tools/check-components.py`.

The selected Butane modern graph contains 135 normal/build packages and 16 build
scripts. A source/archive and `links` scan found no native implementation source
or library archives. Previously reviewed engine build-script hashes are unchanged.
Shared dependencies resolve some versions differently from the separate research
lock: bitflags 2.13.1, smallvec 1.16.0, synstructure 0.13.2, thin-vec 0.2.19,
yoke-derive 0.8.2 and zerofrom-derive 0.1.7 are already in the browser's prior
graph. Their selected versions were rechecked for native sources/links and
license metadata; they add no new build scripts. This is a dependency sanity
review, not a review of every algorithm or unsafe block.

## Operating system boundary

The component guard retains its ban on direct libc dependencies in Butane and
Sparkle. It permits only these pinned transitive OS-interface paths:

- Boa 0.22.0 → rand 0.10.2 → getrandom 0.4.3 → libc 0.2.189: OS entropy.
- Boa 0.22.0 → dashmap 6.2.1 → parking_lot_core 0.9.12 → libc 0.2.189:
  synchronization.
- Boa 0.22.0 → time 0.3.55 → libc 0.2.189: time-zone/local-time interface.
- Sparkle's previously reviewed Stylo CPU-count/futex paths remain unchanged.

Each intermediate ancestor edge is checked, including the exact engine parent.
Sparkle also directly names the same pinned Boa crates for generated tracing and
host-data derives; that edge is accepted only in Sparkle's reviewed graph.
This does not permit native JS, crypto, font/image backends or arbitrary direct
platform calls in a reusable component. Worker capability denial and host hooks
are separate runtime checks; a compiled OS interface is not worker authority.

## License packaging

The release inventory now recognizes exactly the seven Boa 0.22.0 crates and
two 0.1.0 utility archives that omit the root MIT notice. It checks their
declared `Unlicense OR MIT` choice and archive VCS revision before including
`tools/licenses/boa-0.22-MIT.txt`. The notice is byte-identical to the reviewed
research copy. New versions, revisions or omitted licenses still fail packaging.
All other active notices, including Unicode data terms, are collected normally.
Project Apache-2.0 licensing does not replace upstream terms.

## Original regression provenance

Original `script_worker` and `script_session` process tests select explicit
`--legacy-script-worker` / `--legacy-script-session` entry points compiled only
with `legacy-test-engine`. Their behavior/resource assertions are unchanged.
Those tests are not evidence that Boa has the original evaluator's accounting.
The shared isolation/session-manager selftests continue to select the production
selftest commands and retain their marker assertions.

The research runner pins every original Rust implementation file, including the
AST definitions in `lib.rs`. It excludes only the new `modern.rs`/`modern/`
implementation, the exact feature-gated module declaration and Cargo metadata
that now describes both optional implementations. The new source-only SHA-256 is
`c959e57980b07a41be0ec91fd22339a2068fc4b6fc2d239109d97d0ebf440f77`, independently
computed from the same 17 files at original commit
`48cec83c0b1d41eb889373e414ded216d09cbf77`. The previous package-wide hash stays in
`inputs.json` as historical provenance. New tests verify that changing original
runtime or AST definitions changes the gate while the precise new wiring does
not. Test262, Vue and authored input hashes, denominators and every expected
outcome remain unchanged; only the input-manifest digest follows this explicit
fingerprint metadata change. The research manifest disables Butane defaults so
it does not select production Boa instruction-budget features.

## Reproduction

```sh
python3 tools/check-components.py
python3 tools/license-inventory.py tmp/boa-licenses.txt
python3 experiments/jsplan/dependency_audit.py --check
python3 -m unittest discover -s tools/jsplan -p test_runner.py
cargo test --locked --features legacy-test-engine --test script_worker --test script_session
```

The normal native-backend guard, production compilation/resource tests, actual
page interactions and release/install checks remain required separately.
