# First upstream Window cohort: declared-value parsing

Source and runner-policy freeze: 2026-10-22, before browser execution. This is
a separate tooling contribution after v0.9.13, not a browser improvement or
an executed baseline. The runner, separate score and public result endpoint
are **not implemented**. F-016, F-026 and the formal MVP remain incomplete.
Preserve this preimplementation contract; record later implementation/results
separately. Engine fixes follow the measured baseline, in separate releases.

## Fixed source selection

At upstream `84daed4ae966d9624bed22f43587c79283ad1fea`, take **all direct-child
HTML files** in `css/css-flexbox/parsing/` whose names end in `-valid.html` or
`-invalid.html`, sorted by full path. Do not select by browser capability,
outcome, ease of implementation or an invalid-only subset.

The separate [source manifest](../tools/wpt/window/corpus.json) has SHA-256
`5223099747ed3b1669f311faa5007abb2c037bebb90aa7411b67c6470b4518d0`.
It records sixteen files, twenty unique execution/license inputs totaling
222,805 bytes, their original lengths/hashes and the complete 27-file direct
HTML domain. The excluded named protocols are eight computed-value files,
two shorthand-expansion files and one WebKit-alias file. This does **not**
exclude shorthand properties: selected `flex` and `flex-flow` remain intact.
There are no query variants, generated wrappers or long-timeout overrides.

The newline-terminated full-domain path roster hashes to
`12a14d357324d52bc83c4a313185c8bf4ebcd1cf4887f7159d8b4afb96121247`;
the selected path roster hashes to
`1d0958276e9ffddccb10561dc7611ab4d35fab02cc239737ed888ee6aed5581a`.
All inputs already exist in the parent static inventory, SHA-256
`d8c4bd6aacd93d964c10302e63b9c42576d785a055848772f9f71b2436f8075d`.
No upstream input, assertion, helper or license is rewritten.

The literal source contains **49 valid-value and 30 invalid-value calls**:
79 prospective subtests. Freeze their per-file count, creation order and exact
raw names derived from the unchanged helper. Serialization-alternative arrays
still declare one test each; commented-out calls declare none. These tests
exercise specified declaration parsing/serialization, not computed values.
Unused selector/rule/keyframe helper definitions remain in the full helper but
do not imply executed coverage. Rejection-only tests can pass vacuously when
all values are rejected; report genuine results without claiming full CSSOM.
Do not invent rows when execution never reaches registration.

Every selected original HTML has these three ordinary external classic scripts,
in order, then its original inline test body:

| Original helper | Bytes | SHA-256 |
| --- | ---: | --- |
| `resources/testharness.js` | 198291 | `d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e` |
| `resources/testharnessreport.js` | 1231 | `3777176fc736b0a78cf7617b9613b220f635daae7536ec7ebb99c61dbb525aae` |
| `css/support/parsing-testcommon.js` | 7476 | `ac482e1e1908e9b98f8f1c4b2c457ec77ae5ad2baa074205c4b0f7812f4024b5` |

Preserve exact inline bytes, including leading/trailing LF, charset/assert
metadata and inert original author/help links. Preserve `flex-valid.html`'s
107-byte inline style and container/target markup, including its container-unit,
sign and calc cases. No test-owned HTML injection or helper pruning.

`python3 tools/wpt/window/check_manifest.py` validates source only: bounded,
duplicate-rejecting closed-schema JSON; actual regular, non-symlink input bytes;
parent inventory, full domain, exact selection, original script/style framing
and literal declared names. Its narrow literal reader never evaluates JavaScript
and never assigns Mg DOM node IDs. A source-validator pass is not a Browser run,
resource-feasibility result or upstream pass. Actual Mg source-node identities
must be derived and verified by the real parser before later execution.
Validation also hashes the eleven excluded domain files: 31 original files plus
the two metadata files are read under the same shared input budget. This is not
an expansion of the twenty-file execution/license closure or selected denominator.

## Future execution boundary

Use a separate fixed-inventory real-Browser test adapter, not a general runner,
production CLI/CDP mode or altered closed matcher for either authored suite.
Navigate to the original HTML using the normal toolbar-free Browser, configured
LinuxScripts and an owned immutable loopback fixture. Keep ordinary browsing's
observer Disabled. Use the existing trusted
`PinnedWindowHarnessOutputDisabledV1` contract from
[WPT_HARNESS_OUTPUT.md](WPT_HARNESS_OUTPUT.md), strict modern v7 and the full
unchanged vendor. Real guarded `setup({output:false})` only disables optional
output/tracking; keep assertions and the normal 10,000 ms harness timer/rearm.
No `explicit_timeout`, fake opener, replacement vendor or injected setup script.

The modern-v7 reference above records the original boundary. The v0.9.20
transport uses strict modern v8 and required Page results, without
changing this fixed observer configuration, upstream inputs or resource policy.
Automation remains forbidden in the trusted observer lane.

Verify the actual request URL/HTML, three external bodies, script eligibility,
real Mg node identities/order and final-response SameOrigin provenance before
inner start. No earlier eligible source may precede bootstrap. Meta/default or
script-proposed navigation must not grant another fetch. The fixed authored
source plans cannot be reused unchanged: these original files have four scripts
and original metadata/style absent from those controls.

Read only `Browser::script_observation()` after ordinary poll publication. The
normal Chassis path validates identity/progress/lifecycle/projection, stages
the same pure ObserverLedger and charges once before allocation-free commit.
A wrapper must forward TaskPump and the one combined charge, not acknowledge
twice or accept a callback before parent validation. Raw transport, page globals,
DOM status text and console messages are diagnostic evidence, not accepted rows.

PASS requires four successful complete immutable GET deliveries: main HTML and
three JavaScript bodies with correct MIME, exact bytes and the owned origin.
Author/help links are inert, not requests. Reject/record extra, missing, unknown,
partial, late or misattributed deliveries; seal/drain the never-reused origin
before final audit. A header alone is not full-body proof. Keep actual negative
request prefixes on typed production rejection; do not fabricate deliveries.
Wrong inventory/fixture bytes or opaque ownership are infrastructure errors,
not browser incompatibility. Do not contact foreign services to satisfy routes.

## Separate evidence and disposition

Keep a new schema/report/score/expectation lane separate from static score.json,
its 1,011 files and its original 24/317-pass/949-runnable ratchet. One manifest-
ordered record per selected Window file starts PENDING and is never dropped.
Retain source/closure/selection hashes, tested Git dirty state, exact binary,
font, engine/build/adapter/runner/helper fingerprints, run identity, accepted
cursor/records/inventory/reconciliation, raw harness status/message/stack,
lossless UTF-16 and Undefined/Null/Text distinctions, sticky native/parent/fault
history, lifecycle/initial outcome, cumulative accounting, diagnostic prefix,
typed broker/policy evidence, clocks, artifacts, request audit and cleanup.

Count each unique final dense creation-index row once, never both Result and
Completion copies. Never deduplicate by names. Keep arrival order, IDs, indices,
Result-present/final-only links and every genuine final-only status. A partial
prefix is partial evidence; unregistered/unobserved tests remain unknown, not
fabricated NotRun rows. Compare the completed genuine inventory against the
nonzero source count and exact expected names/order without rewriting either.

The following is a local reporting contract, not a wptrunner-equivalence claim:

| Accepted evidence | File disposition / basis |
| --- | --- |
| Finished, harness Ok, all rows Pass, exact declared nonzero inventory, clean LoadComplete/Ready/initial/native/privacy/parent/broker history, full resource proof and confirmed cleanup | PASS |
| Harness Ok with Fail/Timeout/NotRun rows | FAIL / Assertions; retain raw distribution |
| Harness Ok with missing, zero, extra or differently named/ordered declared rows | FAIL / DeclaredInventoryIncomplete or DeclaredInventoryMismatch; never synthesize rows |
| Genuine harness Error | FAIL / HarnessError, distinct from infrastructure ERROR |
| Genuine harness Timeout | TIMEOUT / HarnessTimeout, with actual final rows |
| Genuine harness/subtest PreconditionFailed without stronger failure | PRECONDITION_FAILED, still selected and non-PASS |
| Sticky native execution_failed, even after cancellation/handling or Ok/all-Pass callbacks | FAIL / NativeExecution |
| Known typed fatal resource limit or CaptureOverflow | FAIL / ResourceLimit or ObservationLimit, preserving actual prefix |
| PrivacyLost | FAIL / ObservationLost; accepted earlier rows cannot enable PASS |
| Valid host navigation/default/meta policy denial or typed production resource rejection | FAIL / PolicyDenied or ResourceAdmission |
| Healthy Waiting/Settling at the frozen admission cutoff, with confirmed teardown | TIMEOUT / MissingCompletion or UnsettledCompletion; not invented HarnessTimeout |
| Source-declared unimplemented protocol rejected before JavaScript | UNSUPPORTED / exact prerequisite, retaining its slot |
| Inventory/authority/configuration/projection/charge/wire/launch/artifact/opaque transport failure, malformed or inconsistent capture | ERROR / exact infrastructure layer |
| Actual worker signal with authoritative ownership/signal evidence | CRASH / actual signal; generic Transport is not such proof |
| Unknown cleanup, interruption or supersession | ERROR / uncertainty, retaining later PENDING and prohibiting adoption |

Integrity/ownership uncertainty takes precedence. Preserve every secondary fact.
An authoritative crash remains explicit. Otherwise an established native,
resource, privacy or policy failure before a deadline remains the primary cause;
the deadline must not erase it. Distinguish raw harness Timeout from a missing
completion cutoff. Prefer the earliest validated concrete resource/observation/
policy cause over the generic NativeExecution flag it also sets; preserve the
sticky first fault and all secondary causes. For otherwise simultaneous concrete
causes use ResourceLimit, ObservationLimit, ObservationLost, PolicyDenied,
ResourceAdmission order, never diagnostic-string order. StorageAdmission/
RealmTerminated alone do not identify a
budget cause; do not classify from warning strings or execute getters to guess.
RejectedInit is script failure only for the exact validated pre-realm exception;
broken source/configuration premises are ERROR. Duplicate/inconsistent rows,
MalformedCapture or violated first-fault rules are observation-integrity ERROR,
not repairable result data. Genuine completed zero-row Ok is semantic FAIL here,
not an infrastructure error, because the fixed source declares nonzero tests.
Expected-failure/optional metadata never converts non-PASS into PASS. An exception
caught by a genuine assertion can be Pass without itself setting native failure.

No missing CSSOM API or grammar after execution becomes adapter UNSUPPORTED.
The sixteen selected files use ordinary Window/classic protocols; no unsupported
outcome is predicted. Preserve bounded style/layout diagnostics, but do not copy
static RGB/nonuniform/pixel prerequisites into semantic verdicts or treat CSS
rejection diagnostics as assertion results. Native, privacy, parent and required-
resource failures independently block PASS; no blanket JavaScript-error waiver.

## Fixed ownership and resource envelope

Finished is an accepted completed checkpoint with causal error/rejection
obligations settled through normal task order, not pending-work zero or future
error absence. The genuine harness timer may remain pending at Finished. Pump
only advertised eligible tasks through Browser::poll; no special callback drain,
arbitrary-timer wait, shortened timer, retry or worker restart. Stop commands at
the accepted cutoff, then close/wait_idle and drain/join the fixture explicitly.

Use one serial adapter group with a fresh Browser/runtime/origin per file, no
new aggregate concurrency. Existing at-most-two active/pending/retiring children
remain an ownership allowance, not a new memory quota. One absolute **25 s** file
owner includes setup, load, pumps, evidence and cleanup: admission stops at 22 s,
host/report at 22.9 s, preserving 2 s cleanup and 0.1 s final fixture guard. No renewal.
The same cumulative **2 s active** allowance counts launch/encode/transfer/decode/
validate/project/bookkeeping; exclude only validated idle and reserved cleanup.

Keep [BOA.md](BOA.md), [PAGE_SESSIONS.md](PAGE_SESSIONS.md) and observer/output
limits unchanged: 300 s lifetime, 64 transactions/one flight, 32 MiB aggregate wire,
2 MiB Init/4 MiB reply/64 KiB later frames, 1 CPU s/256 MiB address space,
1M opcodes, 4 MiB cumulative source, 32/64 MiB requested-System outstanding/
cumulative, 4 MiB host/DOM, 50,000 nodes/depth 256, 32 listener slots, 256 events,
shared 256 pending/2,048 lifetime work. Preserve 1 MiB document/32 eligible scripts,
16 external sources/256 KiB each, 1 MiB HTML+admitted-source admission and 1 MiB
individual engine-source limit, counted encodings and dynamic-compilation
privacy loss. Observer 256 identities/257 ordinals/4,096 units per field/131,072
cumulative units apply separately to worker and combined parent with actual
capacities, no refunds/truncation/chunking. System counters are not RSS/GC heap
or a whole-process peak. No new profile, syscall or native implementation.

Keep the shared production CSS/font/image/script 2 MiB/32-request/absolute 10 s
ledger. Tooling upper bounds remain 4,096 files/10 MiB input before reads,
512 MiB shared evidence, 64 MiB report and 20,000 HTTP records, not per-file
multiplications. Source metadata is bounded 4 MiB and the cohort manifest 128 KiB.
Reserve 10 MiB per admitted case for two 4 MiB report/receipt artifacts, 1 MiB log
and 1 MiB request audit; reserve atomic final-report space separately within the
shared ceiling. No unbounded DOM history. Persisting raw wire requires reserving
its additional full 32 MiB worst case explicitly. Semantic acceptance needs no
screenshots/RGB; optional diagnostics require charged existing bounds.

One **25-minute** controlled-work run includes at most 1,200 s build. Each file
gets min(25 s, remaining reserved run time), including cleanup/evidence reserves.
Insufficient time retains PENDING and rejects adoption; never extend the limit.
Kill only exact owned groups before leader reap, and separately prove worker-
pool close/wait_idle plus fixture join. Killing the host does not prove separate
workers are gone. Unknown ownership stops admission and quarantines the slot;
no kill sweeps, artifact trust or producer reuse while mutation may continue.

## Implementation and baseline gates still ahead

Before execution, implement pure mapping negatives for every disposition and
precedence above: missing/zero/mismatched inventory; final-only/out-of-order/
colliding names; partial unknown totals; genuine versus missing timeout; sticky
native/privacy/fatal outcomes; body/MIME/provenance failure; one combined charge;
projection refusal; Finished with pending timer; cleanup uncertainty and PENDING.
Reuse the unchanged 16+5 authored suites, never as upstream numerator entries.

The first Browser run records **all sixteen** actual outcomes before CSSOM or
engine fixes. A fully measured baseline may contain genuine FAIL,
PRECONDITION_FAILED or safely closed TIMEOUT: all remain non-PASS. ERROR,
opaque crash, unknown cleanup, incomplete evidence or PENDING prohibit tracked
score adoption. Keep the static zero-infrastructure/timeout/crash gate unchanged.
Only manually reviewed source-bound expectations can later strengthen; no
automatic regression acceptance or failed-file removal.

Publish separate 16-file and actual final-subtest distributions, with 79 source-
declared calls and unknown partial totals explicitly distinguished. Do not merge
them into a 1,027-file or 1,090-test headline. A new report endpoint/every-commit
gate needs separate reviewed wiring after a genuine complete baseline; the
current website score and `/wpt-results.json` remain the static lane. New runner/
adapter/helpers must enter the new lane's source fingerprint. Re-run the complete
existing 1,011-case gate for every contribution. Subsequent browser fixes still
require ordinary feature release, public installer/update and native acceptance.
