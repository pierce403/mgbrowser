# Window cold-Cargo CI pilot

2026-10-23: tooling only, before mandatory continuous Window scoring. The
reviewed first local baseline and runner were published at
`32da1ab929f24cdb2a2081af44358878046dfd06`; all six exact-source checks, public
bytes/report and website visual acceptance passed. The historical measurement
remains 0/16 passing files and 0/79 passing actual final subtests. No browser
feature, new upstream pass or installed-release score is claimed here.

The local baseline used a warm Cargo cache. It does not prove that a fresh CI
machine can provision dependencies and build within the existing deadlines.
The independent `window-cold-pilot` Pages job measures that prerequisite before
the site artifact is made dependent on the Window lane.

## Fixed measurement

The job installs Rust 1.91.1 as a recorded prerequisite, then requires a new
empty task-owned Cargo home and an absent target directory. It restores no
cache and does no Cargo fetch or compile before the owner starts. The normal
report owner gets explicit `--allow-locked-build-network` permission to download
only the existing Cargo.lock graph through ordinary configured Cargo sources.
Default local runs remain offline. This option does not enable browser or
worker networking, select a different executable, add features or change inputs.

Provisioning, the normal default-feature release example build, cleanup and
binary/evidence verification share one absolute 1,200-second build allowance
inside the existing 1,500-second run. No separate fetch allowance or retry is
introduced. All sixteen manifest slots remain present; late/failed builds
cannot admit cases. Unknown cleanup stops admission and retains its reservation.
Every normal per-case resource and ownership assertion remains in force.

Only the bounded report, process receipts/logs and case accepted/receipt/HTTP
audits are uploaded as a seven-day GitHub artifact. Profiles, Cargo caches,
target output and repository contents are not uploaded or publicly staged.
An eligible all-failing measurement is a valid baseline, not a passing browser.

## Acceptance still required

Inspect the exact-commit CI artifact, all sixteen outcomes, actual resource and
cleanup evidence. Record toolchain/setup/job/build/run timings separately.
Native publication currently waits 1,080 seconds for its canonical artifact;
the 1,200-second build ceiling alone cannot prove a future mandatory gate fits.
Measure the complete cold critical path with headroom; do not lengthen a timeout
or assume a cache hit to claim feasibility.

The pilot is deliberately not a dependency of static validation, staging or
canonical publication. A subsequent reviewed increment must wire both lanes
into staging, add deterministic current Window metadata and a manual ratchet,
and publish/verify the exact separate `/wpt-window-results.json` report. That
endpoint and current Window score do not exist yet. Never merge the static
1,011-file and Window 16-file denominators. Safely classified genuine Window
timeouts are not automatically infrastructure failures; preserve the original
[disposition contract](WPT_WINDOW_PARSING.md).

Only after those gates may CSSOM changes be scored against the frozen cohort.
Browser changes still require ordinary release, installer/update and native
acceptance. F-016, F-026 and the formal MVP remain open.
