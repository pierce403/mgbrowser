---
name: browser-check
description: Validate mgbrowser transport, document, paint and native interaction changes with local fixtures, then verify specifically requested live browsing journeys without substituting fixture success for website compatibility.
---

# Browser check

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
WeakRef/removed-listener tests must use actual engine checkpoint cleanup, not
test-only ClearKeptObjects. Distinguish requested System allocation counters,
GC reachability and RSS; parser/native/regex/GC work still relies on hard process
containment where cooperative hooks are absent. This is not full JSPLAN P1/P4.

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

For post-update restart, run tools/chrome-update-smoke.sh with the packaged and
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

Keep live page/query artifacts in ignored tmp/ unless deliberately selected for publication without private data. Preserve deterministic local fixtures and component regressions in tests/. Update feature evidence and the daily log with both success and failure; keep the full goal open if any required live stage remains unverified.
