# mgbrowser v0.9.13 Experimental Preview

Fixed trusted Window testharness output configuration for contributor hosts.
This is not a new scored WPT selection or broad web-compatibility improvement.
Local tests, exact-commit CI, tagged packaging and public install/update/native
acceptance are separate gates recorded in the
[dated log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-22.md).
The installer selects the latest actually published release.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

## Fixed contributor configuration

- A trusted source-inventory-verified host can select the single fixed
  `PinnedWindowHarnessOutputDisabledV1` contract. Native integration invokes the
  genuine pinned `setup({output:false})` before tests, without injected source,
  altered helpers, fake opener, error waiver or timeout override. This disables
  optional output and assertion-record tracking, not assertions or typed outcomes.
- Capture the real own-data ordinary registration/setup functions before any
  call. Preserve the real global receiver and an ordinary intrinsic-prototype
  object with exactly one own-data `output:false` property. Partial installation,
  capture faults, navigation, privacy loss and fatal failures stop subsequent
  sources; post-call and outer-checkpoint guards preserve the first failure.
- Original V1 observation remains registration-only; ordinary browsing remains
  Disabled. URLs, markup, page globals and CDP cannot select this trusted mode.
  It is not a general configuration API or arbitrary-page attestation.
- Strict private modern protocol v7 rejects v6 and earlier modern versions.
  Required Init/reply shapes otherwise stay unchanged. Strict legacy-v1 and
  one-shot formats remain separate. No public Runtime/CDP command or worker
  syscall is added.
- The real setup cancels and rearms the unchanged ten-second file timer.
  Cancellation releases pending capacity only; no lifetime work is refunded.
  Existing parent projection, one combined active charge and accepted-ledger
  publication remain authoritative. Already executed JavaScript is not rolled back.

The frozen [contract and controls](https://github.com/pierce403/mgbrowser/blob/v0.9.13/docs/WPT_HARNESS_OUTPUT.md)
and [implementation evidence](https://github.com/pierce403/mgbrowser/blob/main/docs/WPT_HARNESS_OUTPUT_STATUS.md)
keep the original sixteen-control suite separate from five new authored controls.
The deliberate assertion failure, real harness timeout and default-output native
failure must remain negative outcomes. A missing Completion is not a timeout
record, and a parent deadline is not proof of native execution failure.

## Unchanged limits and standards scope

All existing source, opcode, requested-System allocation, DOM, CPU, active-time,
lifetime, transaction, wire and shared-work limits remain. There is no new
execution profile, permission, hidden queue or fallback backend. Fatal VM roots
may still require actual Context/worker teardown after binding cleanup.

The pinned static selection remains 1,011 upstream flexbox reftests; its last
published result is 317 PASS / 632 FAIL / 62 UNSUPPORTED. Every commit requires
a fresh complete gate and exact-source public report. The corpus, assertions,
expectations and scorer are unchanged. Authored observer controls add zero
upstream passes; full Window readiness, WebDriver and Playwright remain unfinished.

Release compilation preserves the existing size-first optimization and runtime
limits. Archives must fit older updaters' 8 MiB compressed / 64 MiB unpacked
ceilings; no download limit is raised.

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
  ordinary React/Vue applications remain unsupported. This increment does not
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
