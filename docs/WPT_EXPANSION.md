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
  its untruncated Git tree. It contains 1,788 HTML and 168 XHTML/SVG candidates.
- Select every recursive HTML/XHTML/XML/SVG document declaring an original
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
Cargo's machine-readable build output needs its own explicit bounded allowance.

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
