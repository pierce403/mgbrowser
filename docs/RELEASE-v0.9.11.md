# mgbrowser v0.9.11 Experimental Preview

Queued snapshot document lifecycle on the existing Rust browser engine.
Local tests, exact-commit CI, tagged packaging and public install/update/native
acceptance are separate gates recorded in the
[dated log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-22.md).
The installer selects the latest actually published release.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1 and locked dependencies.

Release compilation uses size-first optimization (`z`, previously `s`) to keep
the complete licensed payload below existing updaters' 8 MiB download ceiling.
Default panic/unwind, LTO/codegen policies and runtime limits are unchanged.
This is a size tradeoff, not a throughput improvement claim.

## Document loading

- After ordinary sources and their checkpoints, readiness becomes interactive
  before deferred sources. DOMContentLoaded and Window load run as two separate
  genuine parent-pumped tasks after the final source checkpoint. Earlier eligible
  source-created timers/messages can run first; lifecycle has no special priority.
- DCL preserves document targeting, capture/target/bubble identity and real
  checkpoints. Window load follows complete readiness, with its legacy document
  target and nonbubbling/noncancelable semantics. Capture stopPropagation stops
  the next invocation without suppressing remaining same-invocation listeners.
- Both mandatory tasks consume the existing shared work ledger, even without
  listeners: empty Init is 2 admitted / 0 executed / 2 pending, then 2/1/1 and
  2/2/0. Admission/storage failure is terminal, not a free or synchronous fallback.
  Cancellation releases pending work, never lifetime credit.
- Strict modern retained protocol v5 carries private, monotonic progress and a
  sticky initial-failure bit. Old modern frames are rejected; the explicit
  original-engine test lane preserves strict v1 bytes. No remote Runtime API is
  added. Rejection privacy purging preserves independent lifecycle/message tasks.
- Browser loading now waits for actual load completion, not an empty queue.
  Later long timers or load-created tasks do not extend initial-load waiting.
  Typed bounded startup results preserve broker/script errors across clean later
  projections; opaque host failures remain distinguishable from admitted errors.
  CDP reports document commit, DCL, load and loading stopped separately, once per
  generation. Later projections invalidate DOM IDs without repeating load events.

This is the existing pre-parsed/preloaded classic-source domain, not a streaming
parser, dynamic-resource load barrier, general event loop, typed WPT observer or
complete Window-testharness readiness. See
[the contract](https://github.com/pierce403/mgbrowser/blob/v0.9.11/docs/DOCUMENT_LIFECYCLE.md).

## Unchanged limits and standards scope

Lifecycle tasks share 256 pending / 2,048 lifetime work admissions with Promise
jobs, timers, messages and rejection notices. Other bounds remain 32 cumulative
listeners, 256 events, 1,000,000 opcodes, 1 MiB source / 4 MiB lifetime source,
32/64 MiB outstanding/cumulative requested System allocation, 4 MiB DOM storage,
one CPU second and 256 MiB address space. Sessions retain two total active
seconds, 300 seconds absolute lifetime, 64 transactions, 2 MiB request / 4 MiB
response frames and 32 MiB aggregate wire. No dependency, syscall or filesystem/
network authority is added. Context teardown remains necessary for fatal VM
roots; embedders must discard Fatal/Closed realms.

The pinned selection remains 1,011 upstream static flexbox reftests. Its last
published result is 317 PASS / 632 FAIL / 62 UNSUPPORTED; every commit requires
a fresh complete gate and exact-source public report. Corpus, assertions,
expectations, scorer and deadlines are unchanged. Authored lifecycle acceptance
is not an upstream pass or complete JavaScript harness execution.

## Install and update

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

SHA-256 is checked before installation without sudo to `~/.local/bin`, with a
desktop launcher and Mg icon. `MGBROWSER_INSTALL_DIR` overrides the executable
destination; `XDG_DATA_HOME` overrides launcher/icon/license data. A normal
DejaVu/Liberation font or `MGBROWSER_FONT` is required. Checksums are not
independent release signatures. Older updaters retain their 8 MiB compressed /
64 MiB unpacked limits.

Installed builds check for updates. Optional **Menu > About > Restart now**
reopens committed tab URLs, not unsaved edits, cookies, history or pane layout.
`--no-auto-update` disables background checks. No background service is installed.

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
  ordinary React/Vue browser applications remain unsupported. Queued snapshot
  lifecycle does not provide full browser event-loop or WPT harness behavior.
- Full CSS is not implemented. Flex/grid/positioning/stacking, images and fonts
  are bounded subsets. CSS imports, transforms, rounded corners, full line
  clamping and independent element scrolling remain incomplete. Google News
  has fidelity gaps; account actions/personalization are not acceptance claims.
- Ordinary Playwright compatibility remains unfinished. Partial CDP and the
  read-only Inspector are not full Chrome DevTools or a JavaScript debugger.
- Source-error muting conservatively suppresses a realm after foreign, unknown
  or dynamically compiled sources. Error locations remain unknown; rejection
  observation gaps are explicit and cancellation cannot erase diagnostics.

Project code, documentation and original artwork: Apache-2.0. Dependencies retain
their licenses in packaged THIRD_PARTY_LICENSES.txt. Old v0.1.0/v0.1.1 artifacts
remain MIT; published tags and assets remain immutable.
