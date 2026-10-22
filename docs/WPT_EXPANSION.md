# Pinned flexbox expansion: selection before execution

2026-10-21: the user explicitly resumed the broad feature goal after v0.9.6.
This is a separate test-tooling/corpus contribution before any engine fixes.
The unchanged browser baseline is release v0.9.6 at e5c3e32; source main starts
at 3fdf70e. The existing 24-test cohort and all its passing/runnable requirements
must survive. This document fixes the next selection before browser outcomes.

## Input and selection

- Keep upstream revision `84daed4ae966d9624bed22f43587c79283ad1fea` and the
  complete `css/css-flexbox` tree `9ad051e4d7244fb68fc346148daa8e369ebb4844`.
- Inventory the whole recursive tree: 2,042 files / 3,051,436 bytes according to
  its untruncated Git tree. It contains 1,788 `.html`, 34 legacy `.htm` and
  168 XHTML/SVG candidates.
- Select every recursive `.html`/`.htm`/`.xht`/`.xhtml`/`.xml`/`.svg` document declaring an original
  `link rel=match` or `link rel=mismatch`, in lexical path order. Do not filter
  flags, script requirements, references, protocols or expected results.
- Freeze original relation/href data, the selected roster and its hash before
  rendering. Unsupported query/nonlocal/chained-reference protocols remain
  visible outcomes; they do not remove selected tests.
- Preserve all original 24 tests, reference edges and 41 file lengths/hashes.
  Freeze their prior manifest/ratchet independently of the expanded baseline.
  Include required support and license files without rewriting upstream bytes.
- Verify Git blob identities plus SHA-256/length. Reject truncated inventories,
  symlinks, altered existing bytes and missing static dependencies. Do not fetch
  arbitrary URLs found in a test. Server-generated protocols stay explicit.

This is a complete selected reftest cohort from one directory, not full WPT,
JavaScript testharness, WebDriver, manual tests or all files in the repository.
The exact selected count is determined from source, not guessed or chosen from
rendering results.

## Reviewed test-infrastructure bounds

The original pilot's 100-file inventory allowance cannot represent the complete
directory. Review and replace that inventory-count allowance with 4,096 entries;
retain the existing **10 MiB total input-byte ceiling**. Validate declared bounds
before reading file bodies. If dependency closure exceeds either bound, stop
for another explicit review instead of silently raising it or dropping cases.
This is a pinned test-input storage/metadata review, not a browser resource change.

The existing 800x600/1x viewport, exact pixel comparisons, 35-second navigation,
45-second renderer, 1,200-second build, script isolation and browser resource
limits remain unchanged. Add finite parent-run safeguards: 25 minutes overall,
512 MiB total evidence, 64 MiB report, 20,000 HTTP audit entries, 1 MiB page log,
1 MiB metadata and 8 MiB PNG. Raw RGB remains exactly 1,440,000 bytes. Store RGB
losslessly compressed and bound reads/decompression; do not widen tolerances.
Cargo stdout/stderr share a separate 32 MiB build-output allowance.

Use owned process groups on timeout/overflow, reap the direct renderer, and
verify restricted descendants terminate. Do not claim a process-group kill is
itself grandchild reaping or a whole-browser sandbox. No new worker syscalls,
network capabilities, native backends or production limits are authorized here.

## Scoring and sequence

1. Implement importer, protocol/inventory validation, bounded runner/evidence,
   structured adapter failures and independent negative tests. No engine fixes.
2. Render the frozen complete cohort against the unchanged browser. Retain full
   selected/pending totals in partial reports. Failed geometry or admitted script
   execution is FAIL, not an outcome-derived UNSUPPORTED escape hatch. Missing
   infrastructure, crashes and timeouts invalidate the gate.
3. Review new passes, failures, prerequisites, images and diagnostics. Explicitly
   migrate the ratchet while preserving every original required pass/support
   entry. Never automatically bless weaker expectations or suppress errors.
4. Publish the larger denominator and baseline with fresh exact-commit website
   verification. Existing browser releases and tags remain immutable.
5. Only then make separate general browser or execution-protocol improvements
   against that frozen corpus, with new versioned releases for browser changes.

The next genuine testharness lane needs the complete pinned upstream harness,
typed subtest/harness reporting through the restricted worker and real bounded
Window task/event behavior. A successful harness bootstrap, fake shell global,
console string or rewritten assertion is not a WPT file pass.

## Source-only import review before execution

The initial source inventory selected 980 reftests from 1,956 markup candidates.
Its static closure is 2,062 files / 3,332,952 bytes, within the reviewed bounds.
The selected-path digest is
`fbcac1eb0546bb45b4c7c4fa024d50ade145d7e002b55b8f93e962b3bfe62b4c`.
No browser outcomes were used to choose this initial source-only roster.
Before any rendering, inventory audit found 34 additional `.htm` files. Amend
the extension rule to include that ordinary HTML spelling as well, retaining
all initial 980 cases. Record the resulting final roster/digest before execution;
do not use browser results to decide which legacy HTML files to include.

The final source-only roster is **1,011 reftests / 1,990 markup candidates**:
31 `.htm` reftests are added, with all 34 `.htm` files inventoried. Independent
comparison retains every initial 980 test/reference record and all original 41
file length/SHA-256 records. Freeze the following before the first browser run:

- Selected-path SHA-256:
  `1e092c4bb5606d35411b115aebca19bc438ae023051253a14b6f3f8931409ea0`.
- Final corpus SHA-256:
  `d8c4bd6aacd93d964c10302e63b9c42576d785a055848772f9f71b2436f8075d`.
- Complete Git-tree reconstruction matches the pinned flexbox tree. The importer
  visits 2,660 dependency edges; 19 nonlocal/embedded references stay deferred.
  Source-only namespace/escaped-CSS audit found no such contexts in these inputs;
  independent parser controls cover those forms without executing a page.

Two stylesheets referenced by upstream are absent from the complete pinned
flexbox tree: `percentage-descendant-of-anonymous-flex-item.html` names
`resources/flexbox.css`, and
`reference/flexbox-with-multi-column-property-ref.html` names its own relative
`resources/flexbox.css`. Import review explicitly permits recording these
verified upstream absences in the manifest. This is a narrow exception to the
missing-dependency import rejection above, not permission to ignore missing
downloads. Keep the source and every selected dependent case unchanged, report
those source prerequisites as FAIL before navigation, and do not fabricate a
stylesheet or call them UNSUPPORTED. Missing or changed inventoried bytes remain
an infrastructure ERROR that invalidates the gate.

## Diagnostic precondition review

The first unchanged-engine run exposed pixel-equal pairs carrying explicit
unsupported-CSS diagnostics (including writing modes and scrolling). The old
pilot rejected layout/style fallback but did not reject every diagnostic kind.
Do not adopt such matches automatically. Review a uniform, stronger empty-
diagnostics prerequisite before publishing the expanded baseline, keeping all
1,011 cases and original assertions. Re-run the complete cohort after a runner
correction; do not relabel only selected observed failures or alter the engine.
Approved policy: PASS requires `diagnostics == []` and
`diagnostics_omitted == 0`, in addition to all old resource/replay/viewport/pixel
guards. Any nonempty diagnostic kind, including an unknown future kind, is FAIL;
malformed metadata is ERROR. No observed-case exemptions are allowed. The five
current diagnostic kinds all describe parse errors or renderer gaps, not
informational telemetry. An unmet conservative prerequisite is not proof that
every such image is incorrect: intentional invalid-CSS tests can require a later
reviewed protocol. The first exploratory run was stopped with its full partial
denominator intact; no score from that run was accepted.

## Accepted unchanged-engine baseline

The complete strict rerun reports **317 PASS, 632 FAIL and 62 UNSUPPORTED /
1,011**, with no ERROR, TIMEOUT, CRASH, PENDING or infrastructure errors. All
original 24 tests pass. The run took 933.9 seconds; its 1,711 HTTP requests all
returned 200. Evidence occupies 35,618,167 bytes and the report 3,282,110 bytes,
within the reviewed bounds. Local evidence is retained under ignored
`tmp/oct21-wpt-flexbox-baseline-b/`.

Reviewed screenshots include positioned green-square, intrinsic/flex sizing,
wrap/reverse, clipping, image and table cases. An independent validation re-read
RGB evidence and rechecked original relations for every accepted pass (503
unique pages), with empty diagnostics and all old guards. Pixel equality remains
a narrow 800x600 assertion, not proof of full feature support.

After that review, explicitly strengthen the active expectations to **317
required passes and 949 required runnable cases**, retaining every original
24-case requirement. This is a manual baseline adoption, not an automatic
expectation rewrite by `--update-score`. Production engine, dependencies,
worker permissions and resource caps remain byte-for-byte those of v0.9.6.
The source-only import/runner contribution does not require a new binary release;
exact-commit CI and public website/report delivery remain publication gates.
