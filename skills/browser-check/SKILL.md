---
name: browser-check
description: Validate mgbrowser transport, document, paint and native interaction changes with local fixtures, then verify specifically requested live browsing journeys without substituting fixture success for website compatibility.
---

# Browser check

For every commit, follow docs/WPT.md: run the entire pinned reftest selection into a new
ignored directory with tools/wpt/run.py --update-score, review its full result,
then regenerate tmp/site and include the deterministic score and page. The
runner rebuilds the Rust adapter; do not substitute an old executable. Preserve
the exact corpus and manually reviewed pass/support ratchet when scoring engine
changes. XML, fonts, scripting and other unsupported prerequisites stay visible
in the denominator. Never count harness controls or readable fallback as WPT
passes. Pages reruns --check-score on the exact commit, including docs-only work.
This headless Browser lane does not replace native/public-release acceptance.
Every PASS page/reference must have an empty rendering-diagnostic list and zero
omissions as well as all existing replay/resource/viewport/pixel guards. A
nonempty diagnostic is a conservative FAIL, not a post-render UNSUPPORTED escape
or permission for a case-specific exemption. Opaque host/artifact failures stay
ERROR; completed=false or PENDING evidence cannot update the tracked score.
Corpus/runner expansions must precede scored engine changes. Preserve the
immutable original 24-test manifest/ratchet as well as the expanded required
pass/support sets. Freeze a source-derived roster before running the browser;
never filter by observed failures. Retain full selected/PENDING totals on an
interrupted run, but never accept PENDING as a completed score. Check the
documented process, input and evidence bounds and all runtime helper hashes.
The report stores each unique page once; inspect its lossless RGB/PNG/log and
typed failure record before accepting new passes. No source navigation or
script-proposed navigation may escape the local test-server lane.

For WPT orchestration changes, follow the separately reviewed two-lane policy
in docs/WPT_EXPANSION.md. Keep a one-lane control and compare complete runs
using the same binary/font/source: all case identities/statuses, every rendered
page's raw RGB hash and original relations, and per-page HTTP request multisets.
Aggregate counts alone cannot establish equivalence. Preserve shared run,
evidence/report/request limits, single-flight references and bounded queues.
Unknown process/fixture cleanup stops admission; it cannot free another renderer
slot. Keep partial PENDING evidence honest and verify exact-commit cold CI and
actual public report delivery after normal publication.

For JSPLAN engine research, also read `experiments/jsplan/README.md` and
`docs/jsplan/RESULTS.md`. Keep the experiment's workspace/lockfile separate from
production. Build its release executable with Rust 1.91.1, run protocol and
runner tests, verify the exact dependency audit, then run the pinned-input
comparison and `tools/jsplan/check_report.py`. The check includes known failures:
do not shrink the denominator or update expectations automatically. Use the
supervised empty-environment pipe protocol with the shared production isolation;
never initialize an experimental engine in the browser parent. Record debug and
release results separately. GNU time measurements exclude the Python launcher's
inherited memory high-water state; distinguish peak process RSS from live heap.
Research-only tooling is not a browser backend or a framework compatibility claim.
Any actual browser adoption still requires the standing release/installer gates.

Production Boa page execution uses the separate process-contained profile in
docs/BOA.md. Run modern, boa_pages, boa_page_events and boa_worker tests in debug
and release, including real restricted-child allocation/job/opcode failures.
Preserve the old interpreter's tests with --features legacy-test-engine and its
explicit --legacy-page-tests fixture lane. Never package that feature or infer
production Boa compatibility from original-engine fixtures. Packaged production
acceptance uses /script-boa and the unchanged /script-events through native input
and external CDP, plus the installed-binary worker/session smoke checks.
For typed external classic source or performance/cache changes, also build normal
journey_server/cdp_journey examples and run tools/page-services-smoke.py against
the packaged and public-installed executable. It owns its loopback/display and
checks external startup order, retained event behavior, worker accounting and
actual CLI cache/failure paths. Preserve the original inline-script journeys.
Run external_scripts regressions in debug and release; never encode trusted
source metadata in page HTML. Run inspector-smoke for Performance shortcut/tab,
compact/scaled and light/dark acceptance, and inspect its saved screenshots.
WeakRef/removed-listener tests must use actual engine checkpoint cleanup, not
test-only ClearKeptObjects. Distinguish requested System allocation counters,
GC reachability and RSS; parser/native/regex/GC work still relies on hard process
containment where cooperative hooks are absent. This is not full JSPLAN P1/P4.

For function timers or parent-task scheduling, read the reviewed increment in
docs/WPT_TESTHARNESS.md as well as BOA.md/PAGE_SESSIONS.md. Run boa_timers and
script_tasks in debug and release, existing modern/Boa/legacy event/projection
and actual-worker tests, strict Chassis task controls and independent no-chrome
embedding. Preserve shared pending/lifetime work, CPU/active/lifetime/wire/child
bounds. Canceled work never refunds cumulative admission. Test real checkpoints
and traced callback/argument release, hostile delay conversion, direct versus
Promise-created nesting, stale/early/replayed wakes, exactly-one projection
charge, native edit preservation and owned-child cleanup. Do not repair roots
with test-only checkpoint cleanup or sleep inside active worker execution.
Run tools/task-services-smoke.py on packaged and public-installed bytes: its
authored timer-created controls must complete the unchanged native and external
CDP cancellation/form/destination assertions. This is not upstream WPT coverage.
The static adapter still does not await arbitrary timers; audit current-cohort
passing script logs for unfinished work before score adoption. A new asynchronous
scoring prerequisite/corpus requires its own frozen contribution, not a silent
change to waits or exceptions while scoring an engine fix. The original timer
increment's bundled Init was not completed Window-testharness readiness; queued
snapshot lifecycle has separate acceptance below and is not full readiness either.

For Window uncaught errors, also read docs/WINDOW_ERRORS.md and run boa_errors,
external_scripts and actual boa_worker/script_tasks controls in debug/release.
Check five-argument onerror/literal-true cancellation, capture-phase stop versus
same-invocation continuation, retained diagnostics, recursive/fatal no-JS paths,
real traced exception release and sticky foreign/unknown/dynamic-source muting.
Parent origin proof covers every redirect and survives cache/defer; DOM/base
attributes cannot supply it. Preserve the current strict modern wire version,
strict legacy v1 bytes and existing resource counters; never guess an old modern
frame's shape. Run tools/error-services-smoke.py on packaged and
public-installed bytes: require its exact expected diagnostic/PARTIAL per
journey plus native/CDP identity, checkpoint and retained form proofs. Do not
relax the old diagnostic-free page/task checkers. These are authored controls,
not rejection notification, exact source attribution or new upstream WPT passes.

For Promise rejection notification, also read docs/PROMISE_REJECTIONS.md. Run
boa_rejections, boa_rejection_resources and real script_tasks in debug/release,
plus core private fault/storage/ID/weak-map/finalizer controls and all old gates.
Check one frozen batch per parent pump, true late handling, shared work admission,
active-candidate re-entry, genuinely weak outstanding membership, and terminal
root/permit/wake cleanup. GC tests must use normal checkpoints, not test-only
kept-object repairs. Muted Reject retains no payload/permit; the conservative
observation-gap notice is sticky across replies, including loss of outstanding
handled observation. Actual notification diagnostics belong to their transaction
and cannot be retracted by cancellation, handling or later clean-looking output.
Run tools/test_rejection_services_smoke.py, then tools/rejection-services-smoke.py
on packaged and public-installed bytes. Its notification-created form requires
native/CDP payload identity, checkpoints, retained input/cancellation/navigation,
exactly one expected diagnostic and zero trap requests. Keep the old
diagnostic-free and Window-error checkers unchanged. This procedure derives from
the separately exercised authored error/rejection journeys, not WPT coverage or
completion of general Window-testharness readiness. Release/public receipts
still establish final delivery separately from local development success.

For self-Window messaging, read docs/WINDOW_MESSAGES.md. Run boa_messages,
boa_message_resources, private native DOMException/clone/fault controls and
actual script_tasks in debug/release, preserving every older gate. Check genuine
later tasks, synchronous snapshot/DFS getters, cycles/aliases/sparse lengths,
trusted origins, conversion/IteratorClose fatal guards, traced event identity
and terminal root/permit cleanup. No new transfer/cross-context authority is
implied. Run tools/test_message_services_smoke.py and then
tools/message-services-smoke.py against packaged and public-installed bytes.
Require its exact queued/settled work, one actual message task plus the separate
mandatory lifecycle tasks, native/CDP clone and
checkpoint proof, retained form/cancellation/destination, zero diagnostics/trap
requests and confirmed owned cleanup. Its private display disables Mesa's disk
shader cache; artifact limits and browser settings remain unchanged. Do not
relax older diagnostic checkers. A full unchanged harness bootstrap probe is
non-scoring: successful initialization or pending work is not typed completion,
full lifecycle readiness or an upstream pass.

For queued snapshot lifecycle, read docs/DOCUMENT_LIFECYCLE.md. Its strict v5
progress, mandatory shared admission, typed sticky initial outcome and renderer
integration are one bounded contract, not general event-loop or harness support.
Run boa_lifecycle, shared/transport, actual script_tasks, Chassis and independent
embedding controls in debug/release, preserving all older tests. Keep raw
source-fatal quota negatives and explicitly separate post-load test workloads;
never blanket-add counters, subtract mandatory work or accept transient status
text in place of initial failures. A terminal reply must cancel pending work,
retain its lifecycle prefix and preserve the first actual failure; Context
teardown is still necessary for VM roots, not a test-only GC repair.
Build journey_server/cdp_journey/lifecycle_journey and run
tools/test_lifecycle_services_smoke.py, then tools/lifecycle-services-smoke.py
against packaged and public-installed bytes. The authored helper separately
checks actual native retained-form behavior, external one-per-generation
start/commit/DCL/load/stop ordering, later DOM invalidation without repeated
milestones, deferred/earlier-task checkpoints and exact HTTP/work receipts. It
leaves a long timer pending to distinguish initial load from queue idle. Its
packaged and public-installed v0.9.11 acceptance passed; subsequent changes
still require both repeats. It does not prove hidden-tab/supersession GUI behavior,
ordinary Playwright compatibility, upstream passes or typed harness completion.
Keep all older diagnostic rejection guards and native/CDP interaction proofs
when explicitly migrating their bundled-order premises.

For trusted Window testharness observation, read docs/WPT_OBSERVER.md and
docs/WPT_OBSERVER_STATUS.md. The frozen sixteen authored controls are not a
scored upstream selection. Preserve the current strict modern required Init/reply mode
(v7 for the fixed output-configuration increment; v6 is release history),
including explicit Disabled in ordinary browsing, and unchanged legacy-v1 and
one-shot formats. Run boa_observer and private native/shared-ledger/protocol
controls in debug and release, plus both observer examples' pure tests. Keep
own-data-only extraction, weak identities, lossless UTF-16 and explicit
Undefined/Null/Text, bounded copied units/capacities, final-only negative rows,
sticky faults/privacy/execution and first-Completion settlement assertions.
Test failed projection and failed one-charge acceptance without publishing rows.

Run all sixteen exact frozen inputs against the normal packaged and actual
public executable through examples/wpt_observer.rs. Its direct stdio driver
owns one cumulative two-second active allowance, a 23-second admission cutoff
and cleanup inside 25 seconds; it does not prove Chassis, HTTP or native UI.
Separately run examples/wpt_observer_browser.rs through an owned immutable
two-route fixture with exact source/provenance/HTTP receipts and the real Browser
ledger. Its owner may narrow admission to 22 seconds and process/report to 22.9
to reserve the original two-second cleanup and 0.1-second final fixture guard
inside the same 25 seconds. Record that narrower MissingCompletion deadline;
never shorten the unchanged 10,000 ms harness timer or infer a result from a
killed host. Accepted Finished cancels further pumping without draining unrelated
timers. Cases 13 and 15 must first publish Completion as Settling, then retain
actual queued error/rejection history at Finished. Verify explicit close/wait_idle
as well as owned groups/direct leaders; unknown cleanup stops new admissions.
Expected-negative verification is not browser PASS, attestation, a network
sandbox or a new WPT score. Preserve all older native/CDP/public-release gates.

For fixed trusted output configuration, also read docs/WPT_HARNESS_OUTPUT.md
and docs/WPT_HARNESS_OUTPUT_STATUS.md. Keep both frozen contract/manifest pairs
unchanged. Run the original sixteen on V1, not the new contract, and all five
new controls with the fixed --suite output-config. Capture genuine own-data
ordinary functions before registration; setup uses one intrinsic ordinary
output:false properties object, never evaluated glue or a caller property bag.
Verify post-call/checkpoint guards, first-fault cancellation, no next source,
actual timer rearm without lifetime refunds and the unchanged work boundaries.

New-suite admission derives all three actual script nodes and the exact framing
LF plus authored inline body. Verify the full harness and original vendor bytes.
The real Browser owner requires exactly document+harness+vendor GET200 deliveries
with actual JavaScript MIME/provenance, while stdio sources remain explicitly
synthetic. Pin the new shared output_config.rs as a compiled input to both hosts.
H2 must retain a genuine assertion Fail and bounded Text-kind stack; H4 must use
the real ten-second timer and final-only Timeout. H5 must already have its native
failure, one Result prefix and no Completion before parent MissingCompletion;
parent Deadline cannot supply proof of native failure. Do not replace the old
matcher, hide negatives or count either suite as upstream passes. Keep the same
absolute 25-second owner and narrower 23/22-second admission cutoffs. Repeat both
suites in local Browser and packaged/public stdio lanes, alongside all older
native, full-regression, scoring and release checks.

Read affected FEATURES.md criteria and docs/RUNNING.md. Run cargo test --locked --workspace --features legacy-test-engine --all-targets and the native dependency guard for relevant changes. Keep browser code Rust-only; native display servers and test infrastructure are separate from browser dependencies.

For component or embedding changes, read docs/ARCHITECTURE.md, run python3 tools/check-components.py, and independently run cargo test --locked -p mg-chassis --no-default-features --test embedding. Workspace builds can unify the chrome feature; that independent build is required to verify its absence. Language tests live under crates/mg-butane/tests, web tests under crates/mg-sparkle/tests, and actual process tests at the repository root.

For browser appearance/settings changes, run the focused Chassis theme and host
settings/appearance tests, then `bash tools/theme-smoke.sh PATH_TO_PACKAGED_BINARY`
after building the theme_smoke and journey_server examples. This starts an owned
Xvfb, private D-Bus portal and temporary XDG configuration: never change the real
desktop preference to test following System. Verify explicit overrides, restart
persistence, live portal changes/fallback, HTTP red warning, unchanged page pixels
and native decoration hints. Inspect light/dark frames; an X11 property is only a
hint and does not prove that every window manager changes its outer decoration.

For whole-browser sizing, also run host DPI/parser/settings migration tests,
Sparkle scaled paint/layout and Chassis scale/CDP regressions. Build scale_smoke,
theme_smoke, cdp_journey and journey_server, then run
`bash tools/scale-smoke.sh PATH_TO_PACKAGED_BINARY`. Its owned 4K Xvfb and
temporary XDG roots exercise System Xresources/XSETTINGS priority and live
changes, native 100/125/200% form navigation, fractional/2x external CDP,
Settings/shortcuts, theme preservation, restart, scroll and resize. Never modify
the real desktop DPI to test this. Keep logical layout/input distinct from
physical glyph rasterization, uploads and screenshots; check compact dialogs
and stale presses after reflow. Preserve 1x and resource assertions. Native
acceptance must wait for completed paints, not just the earlier LOADED log or
window geometry. Repeated identical old frames are not proof that a requested
new paint finished. After opening a panel, changing its section or resizing,
wait for the requested visible controls/content as well as frame stability;
a background-color pixel alone can accept an empty resized surface.
For script-free modern fixtures, correlate the new navigation with actual
mandatory lifecycle completion before input; a same-title WM_NAME cannot
establish readiness, and real click/event effects still require separate proof.
Inspect actual screenshots; repeat with the public-installed
binary after release. Screen-global X11 DPI is not per-monitor Wayland scaling.

For input/navigation changes, build the binary/examples, start the loopback journey_server, and run the browser with its local URL, --smoke-search, --exit-after-smoke and an ignored tmp/ evidence directory. Inspect rendered frames, actual requested URLs and final exit status. Stop only the fixture service you started. CI uses Xvfb to reproduce this path.

For toolbar/bookmark changes, the scale smoke also tests packaged native
Back/Forward/Refresh, loaded-versus-edited URL selection, local save/open/remove,
restart and Ctrl+D. Preserve those assertions when changing toolbar coordinates.
Run bookmark store/value/modal tests and the independent no-chrome embedding
gate. Use private XDG roots only: never read or modify real user bookmarks.
Inspect the saved bookmark-dialog screenshot, then repeat the same acceptance
with the public-installed release. Storage belongs to the host, not Chassis.

For post-update restart, first run tools/test_chrome_update_smoke.py, then
tools/chrome-update-smoke.sh with the packaged and
public-installed executable. It retains the running-inode worker regression,
then uses an explicitly synthetic newer-version wrapper to activate local
update readiness without GitHub API access. Native clicks must preserve the old
window after a missing-path spawn failure, then launch the replacement path,
reopen the committed URL with preserved preferences and exit the old process.
This fixture is not proof of a real future version; public update/install and
checksum checks remain separate. Inspect the ready/retry dialog screenshots.
For tab-workspace changes, also run tools/workspace-restart-smoke.sh against
packaged/public bytes: native Check/Restart must relaunch all committed URLs
with literal arguments in one fresh window and exit the old process. This uses
the same synthetic local installed-newer approach, not a release-server proof.
For download progress, preserve transport tests for exact body bytes, unknown
lengths, chunk framing, redirects, limits and truncation. Inspect the About bar
at compact and scaled sizes in both themes; a received-byte count is not an
installation-success signal. After toolbar height changes, update native/CDP
geometry expectations while retaining pixel, input and resource assertions.

For wheel easing, run Chassis's deterministic scroll tests with controlled
Instants: intermediate/final positions, reversal, bounds, cancellation and
painted hit alignment. Retain the native scale-smoke scrollbar traces at
100/200%: several monotonic intermediate frames, settling and return to origin
must occur on the packaged and public-installed binary. Final position alone
does not prove smooth motion. Preserve immediate keyboard/CDP behavior and
optional-chrome embedding. This does not establish touchpad/kinetic support or
a general frame-rate guarantee on software-rendered pages.

For styled rendering, include the Sparkle styles/image/layout tests and Chassis
styled_embedding journey (real linked CSS, downloaded SVG and a scrolled click).
Read docs/HACKER_NEWS.md for the bounded desktop contract. Compare identical
captured HTML/CSS/assets in Mg and a reference browser with matching viewport,
scale, regular/bold font files and response charset; record scrollbar policy.
Inspect both the first screen and the full-page/footer geometry, then separately
verify fresh live navigation. Do not replace actual assets with site-specific
drawings or treat Stylo's computed properties as implemented layout features.
New dependency configurations require the source/feature/license review in
docs/DEPENDENCIES.md; the component guard's exact OS-only libc exceptions are
not permission for direct platform APIs or native browser backends in Sparkle.

The driver exercises application input handlers in a real window; state that distinction when independent desktop input was not performed. A successful local fixture proves that path only. For an authorized live-site journey, use the site's actual form fields/links and ordinary cookies/redirects; record the exact failing stage and response. Never count a placeholder, interstitial link, fabricated result, or another service as completing the requested site journey.

For CDP changes, read docs/CDP.md and docs/cdp-protocol.json. Run the external Rust examples/cdp_journey.rs client against an owned native browser with --remote-debugging-port=0 and the loopback fixture server; docs/CDP.md gives commands and CI reproduces them under Xvfb. This proves public WebSocket behavior independently of App hooks. Verify schema/discovery, session and stale-node errors, actual form query/result destination, viewport PNG dimensions and rendered output. Keep local endpoint ports/process identities explicit and stop only test processes you started. Use the implemented protocol subset; an unsupported Runtime command is not authorization to substitute another browser engine.

For native inspection, build inspector_smoke and run tools/inspector-smoke.sh
against the packaged and public-installed binary. Check actual right-click
selection, input isolation, fresh navigation and real CSS/resource/script
diagnostics in compact/scaled light/dark views. For tabs, build tabs_smoke and
run tools/tabs-smoke.sh with its owned display/profile/server. Preserve actual
form edits, history and script realms across detach/redock; verify focused-pane
input and last-window-only exit. Model tests alone do not prove native routing.
Multi-target CDP must retain the named page across moves and reject closed
targets; it must never follow whichever tab is active. Build workspace_cdp_smoke
and run tools/workspace-cdp-smoke.sh on the supplied packaged/public binary to
verify native creation/detachment/closure through actual protocol routes.
Run the unmodified pinned
client in tools/playwright before claiming Playwright compatibility: a passing
Rust CDP journey or successful attachment is insufficient.

For script changes, read docs/JAVASCRIPT.md. Keep live execution in the restricted worker; run the language/DOM tests and actual worker isolation selftest before an authorized live page. Use --enable-scripts with /script-redirect and /script-home on the local fixture server: the form must be created by real script execution, then usable through normal input and the external CDP client. Exercise /script-loop, verify a bounded error with readable content, and navigate onward in the same browser over CDP. CI reproduces these local checks. Source/projection rejection must retain the original no-script fallback and discard proposed navigation. Passing authored fixtures or syscall-denial probes is not full ECMAScript conformance or whole-browser sandbox assurance.

For retained events, input/default actions or worker lifecycle changes, also read docs/PAGE_SESSIONS.md. Run the page_events, page_projection and script_session tests, then the /script-events fixture with --enable-scripts --smoke-events and the external cdp_journey client. Require both canceled states, the exact handler-created proof/query/submit fields, the handler-updated destination, and zero /event-trap server requests; docs/RUNNING.md and the CI retained-event step provide the commands. Check stale CDP IDs without fabricated page-load events. Keep parent and child lifetime/active-time/aggregate budgets cumulative; navigation and shutdown must reap owned children without replaying scripts. Do not replace these later-interaction checks with a startup-only created-form test.

For the first scored Window lane, follow the pre-execution source/report freeze
in docs/WPT_WINDOW_PARSING.md. Run its fixed source-only checker and separate
unit discovery in tools/wpt/window; the existing top-level discovery does not
recurse into that un-packaged directory. Source calls are not delivered rows.
Use docs/WPT_WINDOW_RUNNER.md and the fixed tools/wpt/window/run.py owner to
reproduce its real-Browser run into a new ignored output directory. It builds
the normal locked/offline release example, retains all sixteen slots, original
helpers, exact source/font/binary pins and the existing shared limits. Review
accepted records, raw resource/diagnostic evidence, original-body HTTP audit,
clock accounting and separate internal/outer cleanup before adopting a result.
No executable override, filtering, retry or automatic score rewrite is offered.
The first local historical baseline is 0/16 files and 0/79 final subtests passing;
all fail through caught getPropertyValue test-body exceptions, not proven CSS
comparison mismatches. Continuous Window CI/publication is a separate next gate.
Keep the Browser baseline/score separate from static WPT and both authored
control suites. No CSSOM fix before all sixteen baseline outcomes; no fabricated
missing rows, outcome-based exclusions, widened profile or combined headline.
The source checker alone proves neither execution nor resource feasibility.

Keep live page/query artifacts in ignored tmp/ unless deliberately selected for publication without private data. Preserve deterministic local fixtures and component regressions in tests/. Update feature evidence and the daily log with both success and failure; keep the full goal open if any required live stage remains unverified.
