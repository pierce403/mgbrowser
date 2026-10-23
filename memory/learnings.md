# Candidate learnings

- 2026-10-23: The fixed Window host sampled a raw asynchronous Init after a
  Browser poll but before main-thread charge/ledger publication. A fresh
  check-score run retained a Correlation error and unknown initial quiescence;
  it was not adopted. The test-host correction defers only that unacknowledged
  loading/initial state without consuming a cursor, preserving strict checks
  after acknowledgement and all cleanup/deadline limits. Two deterministic
  pure families cover both interleavings and acknowledged negatives. This is a
  host sampling lesson, not permission to trust raw callbacks or waive failures.

- 2026-10-22: Authored Window-error and Promise-rejection native/CDP fixtures
  intentionally retain execution diagnostics after cancellation. Dedicated
  checkers require the exact expected diagnostic and partial transaction while
  preserving the older diagnostic-free checkers. Their negative controls reject
  missing/extra/erased errors, phantom tasks and invalid/renewed counters. Both
  native paths now also pass in the public v0.9.12 helper acceptance, recorded
  in the dated log. Curator extends browser-check for this recurring
  acceptance pattern, not to count these authored controls as WPT passes.

- 2026-10-21: Expanded WPT validation took over 20 minutes to publish its
  canonical artifact, exposing a race against the native publisher's 18-minute
  wait and documented 20-minute build maximum. The old native failure's exact
  cause remains unconfirmed because log access was denied. A separately reviewed
  two-lane runner reproduces all case/pixel/request evidence locally in 471.514
  seconds versus 1,029.827 sequentially, without changing engine or shared limits.
  Actual cold CI and public delivery, not that local speedup alone, establish
  publication success. Do not treat a denied log as permission to change authority.

- 2026-10-21: Expanded-source WPT review found exact pixel matches where both
  sides ignored writing modes, element scrolling or static-position behavior.
  Empty-image controls alone did not catch these nonuniform shared failures.
  The reviewed generic diagnostic-free pass prerequisite keeps these failures
  visible without modifying upstream bytes or selecting cases by outcome.
  Reference equality still does not independently prove every feature works.
- 2026-10-21: A test-host ScriptRuntime wrapper initially acknowledged validation
  before Chassis, consuming the session's exactly-one acknowledgement slot.
  Review caught the bug; strict pending-state mocks now enforce the contract
  that permissive mocks missed:
  defer wrapper duration into Chassis's combined charge; rejected replies charge
  once before cancellation. Real Boa scripts and the original 24 cases pass
  after correction. This did not change worker accounting limits.

- 2026-09-23: Release version bumps must synchronize the excluded JSPLAN
  workspace's local mg-butane lock entry and exact dependency-audit records.
  Previous v0.7.1/v0.7.2/v0.8.0/v0.9.0 changes did this; the initial v0.9.1
  candidate omitted it and failed --locked before research execution. Curator
  added the recurring check to publish-site, without changing third-party pins
  or weakening the research gate. See the dated log for correction evidence.

- 2026-09-17: Inspector context-menu, compact resize and three-tab About tests
  independently accepted an unchanged previous framebuffer while a requested
  new paint was still computing. Requiring actual requested control/text pixels
  plus stability fixed the native waits without relaxing page equality, query,
  worker or resource assertions. Curator promoted this repeated observation to
  browser-check. Exact artifacts/failures are in the dated log.
- 2026-09-17: Sequential tab-smoke cases lost their owned Xvfb connection when
  the final client disconnected between cases. Its private display now uses
  `-noreset`; the full three-scale/script/restore journey passes. This is a
  test-display lifetime observation, not a browser or global display workaround.

- 2026-09-07: One manually invoked script-worker baseline redirected the restricted child's stdout to a regular file and correctly hit its zero file-size limit. The normal pipe-based protocol returned a valid bounded reply with the same source and unchanged restrictions. Preserve the failed harness output separately; this is a local harness observation, not permission to weaken worker limits.

- 2026-09-07: Feature status and work-log date drive a generated region of the project page. Initial local and GitHub CI validation passed; Pages served matching bytes. The rule is recorded in AGENTS.md and publish-site.
- 2026-09-07: Certificate approval and HTTPS enforcement settings propagated before an already cached HTTP response changed. Check live HTTPS independently and avoid treating a successful settings mutation as proof of redirect behavior.
- 2026-09-07: One documentation-only CI run hit the shared 50-second scripted-CDP deadline on its final fixture after all tests/native journeys and 16 total CDP journeys passed. The same implementation previously passed. Two eight-fixture browser batches pass the exact local replay with17 native/17 CDP destinations; remote verification follows. Every check is retained and harness scheduling remains distinct from browser/worker resource limits. This is a single observed timeout, not general guidance to relax limits.
- 2026-09-07: One core-intrinsic local CI replay exited when a CDP readiness loop read the child launch log before background redirection created it. Missing log is a pending readiness state, not browser failure. Guarded the five identical reads while preserving process checks, deadlines, HTTP readiness, read errors and final assertions. Independent shell cases and corrected replay are recorded in the daily log. Curator leaves this single observation here; no broader skill or limit change.
- 2026-10-21: Timer eligibility belongs to the currently running HTML task, not
  every callback's ancestry. A Promise checkpoint is not a timer task; its newly
  created timer must not inherit direct timer nesting. The v0.9.7 controls pair
  direct nesting with Promise-created timers. Shared pending/lifetime admission
  and traced callback/argument roots are independent of that eligibility rule.
- 2026-10-21: The new hidden-tab timer fixture initially navigated between two
  documents with identical title/input markup before caching CDP node IDs.
  Review required one full snapshot proving the exact destination URL and
  disappearance of the old document's unique marker, followed by fresh IDs.
  A matching title alone was not proof that asynchronous navigation completed.
- 2026-10-21: Sequential packaged page/task acceptance found no live fixture
  listener but two server-side TIME_WAIT sockets. Its non-reusable Python bind
  incorrectly called the port occupied. A reusable-address probe plus unchanged
  exact child/listener ownership fixes this particular harness failure; real
  ephemeral controls distinguish TIME_WAIT reuse from a still-active listener.
  This is not permission to reuse an unrelated service or enable SO_REUSEPORT.

- 2026-10-22: A five-case probe against the exact public v0.9.10 retained worker
  confirms bundled lifecycle, zero empty-start work and Ready at exactly 256
  pending / 2,048 canceled lifetime admissions. A queued-lifecycle increment must
  explicitly migrate these premises while retaining the same inclusive limits;
  adding two to every expected counter is not valid when source execution was
  already fatal. The probe also reproduces a document capture-stop phase bug.
  These are recorded baseline observations, not completed queued lifecycle or
  full-harness readiness. docs/DOCUMENT_LIFECYCLE.md fixes the next contract.
- 2026-10-22: A new queued-DCL fault control exposes one specific Closed cleanup
  edge: at 255 retained events, the DCL callback queues a Promise then throws;
  its Window error reporter exhausts record 257 and closes the host before the
  normal checkpoint. Dropping only host tasks leaves one VM job pending. The
  existing engine cancellation latch clears that residual queue, conditionally
  after host drops and without replacing an earlier fatal reason; outward Closed
  and the uncompleted lifecycle prefix remain. This does not prove all VM roots
  release without Context teardown or justify a general test-only GC repair.
- 2026-10-22: Native navigation readiness has two separate proofs. For the
  owned script-free scale fixture, new LOADED plus a matching old WM_NAME can
  precede mandatory lifecycle completion. The helper now requires that new
  navigation's real Init/DCL/load accounting and acknowledgements before its
  unchanged single click and PAGE_EVENT_APPLIED check. Three pure controls and
  a fresh full thirteen-helper run on public v0.9.11 pass. The first public
  timeout's exact click-admission timing was not logged: do not generalize that
  inference into a proven browser defect or call title equality a paint barrier.
  Together with the prior same-title/old-frame observations, curator records
  this readiness-versus-input-proof distinction in browser-check.
- 2026-10-22: The typed observer's sixteen frozen authored controls pass through
  both actual normal release-mode workers and a separate real Browser/resource
  broker host. A captured harness Ok can precede queued unhandled/Handled errors:
  cases 13/15 actually publish Settling before Finished with sticky native failure.
  MissingCompletion and the genuine unchanged 10-second file Timeout remain
  different outcomes. Curator records the two acceptance paths and same-deadline
  cleanup reservation in browser-check; none of these authored controls adds
  upstream passes or proves public/native delivery by itself.
- 2026-10-22: Package A's installer fixture correctly rejected an inherited
  MGBROWSER_NO_AUTO_UPDATE=1: that intentional product opt-out omits the marker
  the default-install test requires. The corrected owner removes only that
  variable for default installer checks; native/worker opt-outs stay intact.
  Fresh package/public installation passes without changing installer/assertions.
  Curator records the environment distinction in publish-site; keep the failure.
- 2026-10-22: For the pinned complete Window harness, genuine setup({output:false})
  cancels and rearms its normal file timer: actual new-profile Init has four
  lifetime admissions and three pending, versus original V1's three/three in the
  frozen sibling. Cancellation is not lifetime credit. Optional assert-record
  tracking is disabled, but the deliberate assertion still produces typed Fail.
  Derive source nodes from actual documents (10/12/18 in the frozen controls),
  not the original authored fixtures' node 1. These local controls do not add
  upstream passes or prove public delivery. Curator preserves the separate
  original/new suites, exact source/MIME proof and native-before-parent failure
  requirement in browser-check.
