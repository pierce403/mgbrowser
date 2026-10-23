# First Window runner implementation

First local baseline reviewed, 2026-10-22. The pre-execution contract remains
[WPT_WINDOW_PARSING.md](WPT_WINDOW_PARSING.md), unchanged. All sixteen original
Window files execute: **0 PASS / 16 FAIL**, with **0 Pass / 79 Fail** actual final
subtests and zero infrastructure errors, timeouts, crashes or PENDING files.
The static 1,011-case score, original 24-case ratchet, authored sixteen/five
suites and production browser remain separate and unchanged. F-016, F-026 and
the formal MVP stay open. Continuous Window scoring/publication is not wired yet.

## Reproduce the local measurement

With Rust 1.91.1 and the locked dependencies available in the normal Cargo cache:

```sh
python3 -B tools/wpt/window/check_manifest.py
RUSTUP_TOOLCHAIN=1.91.1 python3 -B tools/wpt/window/run.py --output tmp/wpt-window-local-unique
```

Choose a new output name for each run. The owner builds the normal release-mode
example itself, with `--locked --offline`; there is no binary override, selected-
file filter, retry or score-update mode. Explicit `--allow-locked-build-network`
permits the same locked Cargo build to provision its existing dependencies;
downloads, compilation, cleanup and verification share the same absolute build
deadline. Default local runs remain offline. See the separate
[cold-Cargo CI pilot](WPT_WINDOW_CI.md), not a current published Window score.
`--font` can select a readable regular
font; its exact size and SHA enter the shared input admission. The tool needs
Linux x86_64, normal restricted-worker support and a local loopback socket, not
an X11 window or a live website. A controlled-work run is bounded to 25 minutes.

Pure controls are separate from upstream results:

```sh
cargo +1.91.1 test --locked --example wpt_window
python3 -B -m unittest discover -s tools/wpt/window -p 'test_*.py'
```

The first run completes in 10.983 seconds including a 6.98-second build. Root
and independent data review verify all 48 case artifacts, 64 original-body GETs,
64 actual worker resource reports, exact names/IDs/optional UTF-16, clock bounds
and internal/outer cleanup. All sixteen genuine harness completions are Ok.
Every failed subtest catches `not a callable function` at the original parsing
helper's `element.style.getPropertyValue()` call: these are test-body failures,
not demonstrated CSS grammar comparison failures. The genuine harness timer
remains pending at Finished; it is not drained or shortened to obtain a result.

[first-baseline.json](../tools/wpt/window/first-baseline.json) preserves the
historical measured snapshot, including its honest pre-commit `git_dirty:true`,
binary/font/source/report pins and all sixteen outcomes. It is not a current-
commit CI score or an installed-binary measurement. Full raw local receipts and
review references are recorded in the dated log. No automatic expectation
rewrite or public Window endpoint is introduced. Wire and verify that separate
continuous gate before claiming a current published Window score; subsequent
browser fixes remain separate released contributions.

## Concrete ownership

Owner report schema 2 adds an explicit build-network policy and separate
inclusive build-phase clock, rather than calling Cargo's subprocess elapsed
time the complete build. Historical schema-1 evidence remains unchanged. This
does not change worker wire formats, host receipts, semantic verdicts or limits.

The new example composes the normal toolbar-free Browser, LinuxScripts and the
fixed source-admission guard. It dispatches the existing restricted worker modes
before creating services; page JavaScript never executes in the parent. The
example's worker re-executes that example, not the installed public executable.
Its manifest-selected original HTML, three complete helpers and actual parser
source identities enter through the existing source broker and observer profile.
No original test, helper, timer, resource limit or production API is changed.

The Rust host owns one immutable four-route loopback HTTP fixture. This lets it
retain the actual Browser ledger while closing the script pool and sealing,
draining and joining the fixture, before calling the pure semantic classifier.
No provisional PASS is sent to Python for reconstruction from raw callbacks;
there is no second result ledger or cross-process fixture-audit capability.

The fixture binds `127.0.0.1:0` once. The serial owner supplies a canonical,
strictly increasing list of at most sixteen earlier nonzero ports through
`--forbidden-ports` (empty for the first case). A selected duplicate is rejected
before serving, without retry. The owner independently checks actual origins.
The selected file and the three original helpers are the only served paths.
Exact GET/Host/path/body checks, JavaScript MIME, full-body write/flush evidence,
source-body verification and an immutable post-seal audit establish delivery
together. A header, request count or listener close alone is insufficient.
The audit proves its actual accepted/drained prefix and endpoint closure, not
the absence of arbitrary future packets. Join/deadline uncertainty never yields
a clean audit and stops further admission; the outer process boundary remains.

## Internal semantic evidence and final owner adoption

A live host cannot prove its own future direct-leader/group termination.
The classifier's `Cleanup::Confirmed` therefore describes the host's actual
pool close/wait_idle and fixture seal/drain/join proof. Its serialized
`semantic_decision` explicitly still requires the outer owner's acceptance.
It is not an end-to-end result or a tracked score by itself.

The owner adopts that same decision only after exact normal host exit, bounded
process completion and group cleanup, valid exclusive artifacts, identity and
resource checks. It never infers semantic results from raw callbacks or changes
the Rust decision to manufacture a pass. An outer launch/artifact/cleanup failure
is an explicit owner-infrastructure ERROR with any host decision retained only
as unadopted secondary evidence. Unknown cleanup quarantines the reservation,
stops admission, retains later PENDING slots and prohibits score adoption.
Final PASS continues to require every internal and outer proof in the frozen
contract; this decomposition removes a circular proof claim, not a requirement.

Raw initial outcomes also remain unchanged. Chassis records initial wrapper
denials as Infrastructure/parent Transport. A separate typed
`initial_transport_consequence` can explain only the actual same initial event:
matching validated first/transport cause, terminal stop, no accepted publication,
an unstarted matching ledger with parent Transport, and no contradictory
realm/lifecycle/observation evidence. Prior causes, later transport errors,
unrelated initial failures or malformed/failed acceptance remain ERROR.
Opaque strings cannot establish a budget cause or worker-specific signal.

## Existing clocks and shared accounting

One serial Python owner retains all sixteen manifest-ordered slots from the
start, initially PENDING, under the existing 25-minute controlled-work bound
including at most 1,200 seconds of build. Each case receives at most 25 seconds,
with the same absolute 22-second admission and 22.9-second host/report cutoffs,
two-second cleanup reservation and 0.1-second final guard. No stage renews these.

The host records measured cumulative active, validated idle and reserved cleanup
durations. Before adoption, the owner conservatively charges its complete case
wall interval minus only those verified exclusions against the same two-second
active allowance. It does not add host active time again, waive unmeasured
monitoring, infer idle from a timer or silently omit final serialization and
artifact validation. Unknown or inconsistent clocks are infrastructure errors.

Fresh nested case directories live under one exclusive ignored repository `tmp`
root, with canonical existing nonsymlink parents and no reuse. The shared
512 MiB evidence budget reserves 10 MiB per admitted case for two 4 MiB artifacts,
one 1 MiB log and one 1 MiB HTTP audit, plus separate atomic final-report space.
The 64 MiB report, 20,000-request and existing input/source limits still apply.
No raw-wire persistence or screenshots are needed for this semantic lane.

The font is one more input in the shared 4,096-file / 10 MiB closure, including
the fingerprint's runtime sources, original files and metadata. Admit its exact
size before reading, record the combined count/bytes, and keep that same bound
and SHA for every reread. The host also checks that admitted exact size before
its own font read. The generated executable has a separate bounded output pin;
it does not grant a second input allowance.

Fingerprint the new host, every leaf module, nested runner/checker, runtime
utilities and engine/Cargo/build inputs, as well as exact binary/font and original
input bytes. The static runner's old fingerprint does not cover these new files.
The first complete Browser baseline must retain all sixteen actual outcomes and
actual final-subtest distributions, separately from 79 source-declared calls and
unknown partial totals. No CSSOM/engine fix, new score ratchet or public Window
endpoint precedes that measured baseline and its manual review.
