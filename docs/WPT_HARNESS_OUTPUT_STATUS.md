# Fixed harness output configuration: implementation status

2026-10-22: v0.9.13 is published at `948e86206c23c0310d17dc286ac5107a15f1f434`.
Public closeout log: [dated log](../memory/logs/2026-10-22.md#v0913-public-release-closeout) below. No new WPT score.
The immutable preimplementation contract and five authored controls remain in
[WPT_HARNESS_OUTPUT.md](WPT_HARNESS_OUTPUT.md) and its companion JSON. Their
historical wording and hashes are intentionally unchanged.

## Implemented bounded scope

Trusted test hosts may select the single fixed
`PinnedWindowHarnessOutputDisabledV1` contract. After the verified complete
bootstrap, native integration captures the three real own-data ordinary
functions, registers both observers and calls the captured `setup` once with
an ordinary intrinsic-prototype object containing only own-data `output:false`.
The genuine setup disables optional output/assert-record tracking, not assertions
or the captured typed outcomes.
There is no injected source, arbitrary configuration bag, fake opener or page,
ordinary browser CLI or CDP authority to enable this facility. The trusted
contributor examples select only the fixed suites described below.

The original `PinnedWindowHarnessV1` still performs registration only; Disabled
does not read setup or invoke callbacks. Both installer paths now recheck the
existing fault/fatal/navigation/privacy guards after every call and the outer
checkpoint before committing installation. Failure closes the realm and cancels
binding work without pretending to roll back JavaScript already executed.

Strict private modern v7 intentionally rejects v6 and earlier modern frames.
Required Init/reply shapes are otherwise unchanged; strict legacy-v1 and
one-shot formats remain separate. No resource, permission or dependency profile
is expanded. The real setup cancels and rearms the normal ten-second file timer;
pending capacity can be released, but lifetime admission is never refunded.

## Focused evidence

The first integration owner accidentally selected zero transport tests through
the thin executable target. That successful process exit is not coverage.
The corrected owning-library run verifies nonempty suites and named controls.

Fresh focused debug checks pass: 62 Sparkle observer tests, 24 transport tests,
19 real restricted-worker task tests and 14/21 direct/Browser host pure tests.
The actual license/audit generators and separate JSPLAN audit check also pass;
only local package-version metadata changed. Source fingerprint:
`25886fd9af81713fcd542c0625da1d4b1cecc4e61340d8fd690d18a67d9e315c`.
The [dated log](../memory/logs/2026-10-22.md) records the receipt and corrections.

Private synthetic tests exercise genuine admission/cancellation at the 2,048
lifetime and 256 pending boundaries, exact setup descriptors and receiver,
hostile substitutions without getter calls, queued faults, no-next-source
fences, retained first failures and binding/Context teardown distinctions.
These tests are not substitutes for running the complete upstream harness.

## Complete-harness local controls

All five frozen expectations pass through the actual normal release-mode
executable and separately through the real Browser/resource-broker/manager host.
All sixteen original V1 controls also pass in both local paths. This is early
local evidence, not the eventual packaged or public binary.

| Control | Observed outcome |
| --- | --- |
| H1 | Sync Pass, harness Ok, accepted Finished; normal file timer remains. |
| H2 | Genuine assertion Fail, harness Ok, bounded Text stack; no native failure. |
| H3 | Later timer Pass, absent from Init; accepted Finished without draining the file timer. |
| H4 | Real ten-second harness Timeout, final-only Timeout row; no Result or native diagnostic. |
| H5 | Original V1 captures one Result, then the exact native output error; no Completion before MissingCompletion. |

The new-suite worker run takes 33.471 seconds; the separate Browser run takes
32.611 seconds, including the real ten-second timeout and 23/22-second H5 cutoff.
Browser receipts record fifteen exact delivered GET200 responses in total:
document, full harness and original vendor for each case, with JavaScript MIME.
Actual derived source nodes are 10, 12 and 18, not an assumed bootstrap node 1.
The direct worker lane explicitly supplies synthetic same-origin metadata and
makes no HTTP/MIME claim. Every case closes its owned runtime; all owners report
confirmed cleanup with no unknown state or unfinished cases.

Measured final admitted/executed/pending work is H1 and H2: 5/3/1; H3: 6/4/1;
H4: 5/4/0; H5: 4/4/0 in both paths. New-profile Init admits four lifetime tasks
with three still pending; original H5 admits three with three pending. This
retains the actual canceled timer admission rather than refunding it. These are
observations, not replacement quota assertions. All new-five reports fit unchanged
limits; the new-five maximum cumulative requested System allocation is 18,700,930
bytes. Across the original-sixteen and new-five local direct-worker suites the
maximum is 18,755,476 bytes. These are requested-System counters, not GC live
heap or RSS; neither number is asserted as a public-binary peak.

New-suite receipts: worker SHA-256
`514663b1780911e0b78177163e08040d9b5fbb6c09aedd268b5a366b073f43a4`;
Browser `c40f72754cfd23f5196e88bebc9fd1f4b6ce15ad3968165e8a20d308159bd43d`.
Original-sixteen receipts: worker
`e39bb6b3881015a1a3b7db16aeb3abc71fe0b7c930c3d142a467c99488babab9`;
Browser `54d05af0c1142c9185053230d769b4378179bbe6e3b795a38fecff18223b2802`.
The dated log separates those lanes and executable hashes. Expected negatives
are verified outcomes, not browser PASS or upstream score gains.

## Regression, packaged and public acceptance

The two fixed suites remain distinct: `--suite observer` selects all sixteen
original inputs; `--suite output-config` selects exactly the new five. The new
suite verifies full unchanged harness/vendor bytes, actual three-script node
order and the exact inline framing LF. Its declared assertion failure, genuine
file timeout and default-output failure must remain negative outcomes.

All fourteen local Rust gates pass, including full debug/release all-target
modern/legacy coverage, component/dependency checks and independent no-chrome
embedding. Each full Rust run reports 2,096 passing raw libtest results and the
same two explicitly ignored updater controls; these summaries can include nested
child results and are not a unique-test count. Embedding passes three tests with
none ignored. The dated log records the exact source and receipt.

The separate locked JSPLAN gate also passes all eight steps: all 364 frozen
baseline/Boa outcomes remain unchanged, including known exceptions and unsupported
inputs. It is research correctness evidence, not browser/framework compatibility.

The fresh complete static WPT gate passes in 454.296 seconds: 317 PASS /
632 FAIL / 62 UNSUPPORTED of 1,011, with zero pending or infrastructure failures.
Deep review compares every case and 1,464 page records, 1,451 exact PNGs,
1,449 raw RGB pages, fifteen typed failures, 492 original pixel relations and
1,711 HTTP requests. All original 24 cases still pass. All 37 scripted-page logs,
including the five passing pages' actual Init/DCL/load work, remain unchanged,
with no requested-System counter differences. Only the score's source fingerprint
changes; no expectation or corpus is rewritten. This is source, not public CI.

The normal packaged candidate passes installer/checksum/default-update-marker,
actual packaged updater, modern worker/session and all thirteen existing native
helpers. Its executable matches the early local normal build. The archive is
8,010,201 bytes, below the unchanged 8 MiB updater ceiling. All 39 supervised
commands complete with confirmed cleanup and no nested-cleanup uncertainty.
Root inspects its theme, scale/bookmark, inspector, restart and tab screenshots;
independent review confirms the full receipts and immutable package inputs.
Package receipt SHA-256:
`261f1bd6597413f7d1d974873432ec307700e3ce1a5d86d97f901dae99d82b20`.

Both frozen suites also pass through the exact packaged executable: new five in
33.602 seconds and original sixteen in 34.468 seconds. All cases complete with
confirmed driver/worker cleanup, no unknown state and zero retained reservations.
The genuine ten-second Timeout and original-profile native failure remain
negative outcomes, not newly passing tests. Packaged-suite receipt SHA-256:
new five `c36b5e0a959baf3153257be87e1a00f56606f00ed48347c51f13946148f4fef3`;
old sixteen `e6ac38e8f372b90b48e63afa722c258f5df85d57ba1a5a61b6d4258a52ee5336`.

Exact-source CI/site, normal tag/release assets, public install/reinstall,
actual v0.9.12 self-upgrade/no-op, both frozen public-worker suites and all
thirteen public-native helpers pass at the release source above. The exact
public evidence is separate from early-local and packaged receipts:

- Source CI and canonical/public-byte/result parity: `63ace3f18ee28b2628af26db71bb4b69478713df140f8471f06583e45ca0cc9b`.
- Tag workflow, normal release/assets and safe archive review: `b32da60bcf528af60749feeaf55104b251bb8c0befb04127c8d0603fc3d94c41`.
- Public archive: `8,003,791` bytes, SHA-256 `2fb4bfc22793e1b982bc423eff8c6bf999de7ab48da417faecdadf364829156e`.
- Actual advertised installer/reinstall: `131bbeafeb0562ef6bd437b4cb5a98742478c74082e60087be870d4e1e0eb8e4`;
  installed executable SHA-256 `fc4cf0033230885795c3d3e3e8d928a487c53119eca1ec4a4844d3bde1d9b977`.
- Actual v0.9.12 upgrade and following no-op: `e1a1ac3c826b81be859a55711fd39dce7bccc285d4ba104edb44c61be2c4445f`.
- All five new public-worker controls: `23f13e41bf823b0d0d1d78cffea70a59ba47f44082a3d5b4d2e507eaad86b287`.
- All sixteen original public-worker controls: `8cd625891104cfbc02a2f5ee0abc1924a048eab76cf4f4de10740c7d6f52b385`.
- All thirteen public-native helpers (runtime and documented cleanup): `e149a94ce4a20280bc08079bc22f2ae3878a39945b71117f93fa4fedd0aa415b`.
  Root's separate screenshot review is recorded in the dated closeout log.
- Fresh documentation-closeout WPT/full deep review: `cdaad49303795f373c75786d59fa8f2daa1240459a265fcbbbf1a5219056f182`.

Public closeout log: [dated log](../memory/logs/2026-10-22.md#v0913-public-release-closeout).
Each owner confirms its documented cleanup scope; no unknown cleanup or
unfinished selected case is accepted. Negative control outcomes remain negative.

The published static score remains 317 PASS / 632 FAIL / 62 UNSUPPORTED of 1,011.
Authored controls add zero WPT passes. No upstream Window cohort, full readiness,
WebDriver/Playwright compatibility or broader F-016/F-026/MVP gate is completed.
