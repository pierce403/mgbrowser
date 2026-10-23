# mgbrowser v0.9.12 Experimental Preview

Bounded, default-off Window testharness observation for contributor test hosts.
This is not a new scored WPT selection or a claim of broad web compatibility.
Local tests, exact-commit CI, tagged packaging, public installation and actual
update/native acceptance are separate gates recorded in the
[dated log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-22.md).
The installer selects the latest actually published release.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

## Contributor test observation

- Explicit trusted host configuration can observe the pinned complete upstream
  Window harness through the normal restricted worker. Ordinary browsing leaves
  observation disabled; page markup, URLs and CDP cannot enable it.
- Native callbacks capture bounded typed results and completion, separate harness
  and subtest statuses, stable weak Test identities and lossless UTF-16. Own
  Undefined, Null and Text remain distinct. Final-name reconciliation preserves
  the pinned harness's surrogate sanitization without merging distinct tests.
- Completion remains provisional while actual ordinary error/rejection
  notifications are unsettled. An assertion failure, uncaught error, unhandled
  rejection, missing completion and genuine harness timeout remain different
  outcomes. Expected-negative controls are not browser passes.
- Parent observations publish only after existing projection/lifecycle/accounting
  validation and one combined active-time charge. Rejected projections or failed
  charges cannot publish result records. Accepted Finished stops further test
  pumping without draining an unrelated timer.
- Strict private modern protocol v6 requires explicit Init configuration and
  reply observation, including Disabled. Earlier modern frames are rejected;
  strict legacy-v1 and one-shot test lanes retain their existing formats. No
  public Runtime/CDP command, worker syscall or executable-override API is added.

The contract and sixteen frozen authored inputs are in
[docs/WPT_OBSERVER.md](https://github.com/pierce403/mgbrowser/blob/v0.9.12/docs/WPT_OBSERVER.md)
and its companion manifest. The direct-worker driver and separate real
Browser/resource-broker test host are contributor tools, not a general WPT runner,
WebDriver integration or an arbitrary-server network sandbox.

## Unchanged limits and standards scope

Observation adds narrowing caps inside the existing execution and storage
limits: 256 Test identities, 257 callback ordinals, 4,096 UTF-16 units per field
and 131,072 cumulative copied units in each worker/consuming parent. It does not
increase the shared 256 pending / 2,048 lifetime work, one million opcodes,
32/64 MiB requested-System allocation, 4 MiB DOM storage, one CPU second,
two cumulative active seconds, 64 transactions or 32 MiB wire allowance.
Faults and execution failures stay sticky. Context/worker teardown is still
required for terminal VM roots.

The pinned selection remains 1,011 upstream static flexbox reftests; its last
published result is 317 PASS / 632 FAIL / 62 UNSUPPORTED. Every commit requires
a fresh complete gate and exact-source public report. The corpus, assertions,
expectations and scorer are unchanged. These authored observer controls add
zero upstream passes; broader Window readiness and automation remain unfinished.

Release compilation preserves the existing size-first optimization and runtime
limits. Archives must still fit older updaters' 8 MiB compressed / 64 MiB unpacked
ceilings; no download limit is raised for this release.

## Install and update

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

SHA-256 is verified before user-level installation without sudo, with a desktop
launcher and Mg icon. `MGBROWSER_INSTALL_DIR` overrides the executable destination;
`XDG_DATA_HOME` overrides launcher/icon/license data. DejaVu/Liberation or an
explicit `MGBROWSER_FONT` is required. Checksums are not independent signatures.
Installed builds check for updates; **Menu > About > Restart now** reopens committed
tab URLs, not unsaved edits, cookies, history or pane layout. `--no-auto-update`
disables background checks. No background service is installed.

## Known limitations

- Modern-web compatibility is poor; Google search to first result is not working.
  Do not use this preview for banking, sensitive authenticated browsing or
  arbitrary hostile websites. The restricted JavaScript worker is not a sandbox
  for the browser as a whole.
- Linux x86_64 on X11/XWayland is the GUI target. Cookies are memory-only. There
  is no credential store, HTTP/2, proxy configuration or persistent session.
- Pure-Rust Boa scripting runs by default (`--disable-scripts` opts out), but its
  browser bindings are incomplete. Bounded startup classic external scripts work;
  async, integrity and crossorigin attributes are skipped with diagnostics.
  Modules, dynamic script injection, fetch/XHR, string timers, intervals and
  ordinary React/Vue applications remain unsupported. This observer does not
  complete general browser event-loop or WPT testharness support.
- Full CSS is not implemented. Flex/grid/positioning/stacking, images and fonts
  remain bounded subsets. Imports, transforms, rounded corners, full line
  clamping and independent element scrolling remain incomplete. Google News has
  fidelity gaps; account actions and personalization are not acceptance claims.
- Ordinary Playwright compatibility remains unfinished. Partial CDP and the
  read-only Inspector are not full Chrome DevTools or a JavaScript debugger.
- Source-error muting conservatively suppresses a realm after foreign, unknown
  or dynamically compiled sources. Error locations remain unknown; rejection
  observation gaps are explicit and cancellation cannot erase diagnostics.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Old v0.1.0/v0.1.1 artifacts
remain MIT; published tags and assets remain immutable.
