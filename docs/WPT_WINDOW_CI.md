# Window cold-Cargo CI and continuous publication

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

## Accepted cold prerequisite

The genuinely cold pilot passes at clean source
`fec3803991ae623cac4721f8dc012fe16887db84`, website run39922557433,
job120311021542, attempt1. Artifact12488460763 retains the actual bounded
76-file evidence closure. Its complete report SHA-256 is
`d83c0d5a7d1d1a8076d62380864b8725db32106e335a3d08ec330c9471f23ffc`.
All sixteen files are FAIL/Assertions; all 79 actual final subtests Fail.
All 64 original-body GETs, resource/clock and internal/outer cleanup proofs pass;
no infrastructure/PENDING outcome or producer reservation remains.

Toolchain installation takes 8 seconds, cold preflight 1 second, inclusive build
213.884 seconds, whole owner 218.129 seconds and complete pilot job 234 seconds.
All 443 compiler artifacts report fresh:false. Dependency downloads occur inside
the measured build, not a preceding fetch. Actual CI binary/font hashes differ
from the local measurement and are retained, not normalized. All seven exact-
source checks and public byte/report verification pass; the dated log records
independent artifact/resource review and immutable public receipts.

Static validation takes 741 seconds. Actual push-to-native completion is
776 seconds, leaving 304 seconds against the unchanged 1,080-second native wait.
Window is not the critical path in this run. This is a measured prerequisite,
not a guarantee about queue/network/build variance in the new coupled graph.

## Continuous graph and acceptance

Inspect the exact-commit CI artifact, all sixteen outcomes, actual resource and
cleanup evidence. Record toolchain/setup/job/build/run timings separately.
Native publication currently waits 1,080 seconds for its canonical artifact;
the 1,200-second build ceiling alone cannot prove a future mandatory gate fits.
Measure the complete cold critical path with headroom; do not lengthen a timeout
or assume a cache hit to claim feasibility.

The independent `window-validate` and static `validate` jobs each run their full
--check-score measurement. A new `stage` job needs both, downloads only their
separate single-report artifacts and verifies clean exact-SHA/source/score
binding before staging. Only stage uploads the Pages artifact; recovery Pages
and canonical mirroring depend on it. Raw case/build evidence remains a separate
seven-day artifact, never part of the public output. Each public report stays
under the existing 25 MiB hosting ceiling; no pruning or larger limit is used.
The exact separate `/wpt-window-results.json` endpoint and generated Window card
must be verified on the actual contribution. Never merge the static
1,011-file and Window 16-file denominators. Safely classified genuine Window
timeouts are not automatically infrastructure failures; preserve the original
[disposition contract](WPT_WINDOW_PARSING.md).

Measure the new coupled critical path and public immutable byte/report parity;
do not infer acceptance from the preceding independent pilot. Only after those
gates may CSSOM changes be scored against the frozen cohort.
Browser changes still require ordinary release, installer/update and native
acceptance. F-016, F-026 and the formal MVP remain open.
