# mgbrowser v0.9.9 Experimental Preview

This increment adds bounded queued Promise rejection notifications. Local,
exact-commit CI, packaging and public acceptance are separate required gates in the
[dated work log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-22.md).
Authored controls alone are not public-release acceptance. The installer
selects the latest actually published release.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

## Bounded Promise rejection notifications

- Window `unhandledrejection` and `rejectionhandled` use genuine later
  parent-issued worker tasks. Pending rejections freeze after the outer
  microtask checkpoint; one accepted pump runs one frozen notification batch or
  one handled-notification task, not an automatic drain of later tasks.
  Same-stack/microtask catches suppress notification, and genuinely late handling
  queues a separate task. New rejections during delivery belong to later batches.
- Native, GC-traced event wrappers expose the actual Promise and reason in
  permitted realms. Escaped event references retain identity and post-dispatch
  state. Unhandled notification is cancelable, handled notification is not;
  both are non-bubbling. Property handlers receive one event argument, and only
  literal `false` cancels through their return value. Cancellation is not
  Promise handling. Notification does not become a Window `error` event.
- The host retains the fixed `Unhandled Promise rejection` diagnostic in the
  notification transaction, regardless of cancellation or subsequent handling.
  It never extracts reason getters, coercions, message or stack. Later handling
  cannot retract an earlier reply or log; the diagnostic need not repeat in
  unrelated later transactions. Future harness observation must aggregate all
  transactions, not infer success from only the final reply.
- Foreign/unknown admitted sources and dynamic string compilation use the
  existing conservative realm-wide exposure floor. Suppressed Reject retains
  no Promise/reason, identity record or notification permit. Instead, one sticky
  scalar flag emits this fixed incomplete-observation notice in every reply:
  `Promise rejection observation suppressed by source-exposure policy; handled status unknown.`
  Even an immediately caught muted rejection conservatively remains partial
  observation. It is not proof of an uncaught rejection or a fatal resource
  failure. A new realm has its own policy; page code cannot clear the flag.
- Candidate and late-handled admissions share the existing Promise/timer work
  budget. Cancellation frees pending capacity, never lifetime admission. Pending
  payloads are traced; completed unhandled membership is weak rather than a
  hidden strong root for the Promise or reason. Teardown/fatal paths clear roots,
  tasks and wakes. No worker syscall, network or filesystem authority is added.

See the [reviewed contract](https://github.com/pierce403/mgbrowser/blob/v0.9.9/docs/PROMISE_REJECTIONS.md).
Constructors, synthetic dispatch, messaging, fully queued startup lifecycle and
typed WPT observation remain separate work. This is not a complete HTML event loop.

## Unchanged limits and WPT scope

Shared limits remain 256 pending / 2,048 lifetime jobs and host tasks,
32 cumulative listeners, 256 events, 1,000,000 cumulative opcodes, 1 MiB per
source / 4 MiB lifetime source admission, 32/64 MiB outstanding/cumulative
requested System allocation, a 4 MiB DOM allowance, one CPU second and 256 MiB
address space. Sessions retain two seconds total active parent time, 300 seconds
absolute lifetime, 64 transactions, 2 MiB request / 4 MiB response frames and
32 MiB aggregate wire. Two owned script children per Browser include active,
pending and retiring ownership. Idle waiting and canceled work renew no credit.
Modern v4 and explicit legacy v1 protocol shapes remain unchanged.

These are worker/session or per-Browser bounds, not whole-workspace quotas,
GC live heap or RSS. Parser/native/regex/GC work is not fully cooperatively
metered; OS, allocator and parent containment remain necessary. The original
interpreter is a separate test lane, not a production fallback.

The freshly measured pinned static selection contains 1,011 upstream flexbox
reftests: 317 PASS, 632 FAIL and 62 UNSUPPORTED, zero infrastructure failures or
pending cases and confirmed cleanup. All case outcomes, 1,464 page records
(1,449 RGB images and 15 structured failures) and request records match v0.9.8.
Selection, expectations and scoring are unchanged. Authored
rejection controls do not add WPT passes or change its denominator. Original
24-case and expanded pass/support ratchets remain intact. The website reports
its exact tested source commit, which can differ from an installed release.
This is not full WPT or JavaScript testharness support. See
[method and limitations](https://github.com/pierce403/mgbrowser/blob/v0.9.9/docs/WPT.md).

## Install and update

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer follows GitHub's latest published release, verifies SHA-256 and
installs without sudo to `~/.local/bin`, with a desktop launcher and Mg icon.
`MGBROWSER_INSTALL_DIR` overrides the binary path; `XDG_DATA_HOME` overrides
launcher/icon/license data. A DejaVu/Liberation font is required;
`MGBROWSER_FONT` can select another TrueType/OpenType file. Checksums are not
independent release signatures. Packages must fit unchanged 8 MiB compressed /
64 MiB unpacked limits. Settings/bookmarks remain under
`$XDG_CONFIG_HOME/mgbrowser` (default `~/.config/mgbrowser`).

Installed builds check for updates; **Menu > About > Restart now** is optional.
Restart reopens committed tab URLs with fresh sessions, not unsaved edits,
cookies, history or pane/window placement. `--no-auto-update` disables background
checks. No background service is installed.

## Known limitations

- Modern-web compatibility is poor. Google search to first result is not working.
  Do not use this preview for banking, sensitive authenticated browsing or
  arbitrary hostile websites. The restricted JavaScript worker is not a sandbox
  for the browser as a whole.
- Google News retains font/icon/corner differences, script-dependent placeholders
  and destination fallbacks. Search, menus, accounts and personalization are not
  acceptance claims. Ordinary Playwright is unsupported; partial CDP is not full
  automation-client compatibility. The native Inspector is read-only, not Chrome
  DevTools or a JavaScript debugger.
- Inline and bounded classic external scripts run by default (`--disable-scripts`
  opts out): at most 16 external sources within 32 eligible script elements,
  256 KiB each and 1 MiB combined HTML/source. Deferred sources follow ordinary
  sources; `nomodule` provides the classic fallback. Async, nonempty integrity
  metadata and `crossorigin` are skipped with diagnostics. Sources require UTF-8
  and an admitted JavaScript MIME type. ES modules, dynamic script injection,
  fetch/XHR and React/Vue browser-app compatibility remain unsupported.
- Function timers and rejection notification remain bounded; string timers and
  intervals are unsupported. Startup still bundles lifecycle callbacks and
  checkpoints rather than implementing a streaming parser or fully queued
  lifecycle. Messaging, typed harness observation and complete Window/testharness
  readiness remain open. Error source locations remain unknown; conservative
  source muting deliberately over-suppresses exposure.
- Pinned Boa can retain fatal callback arguments in its private VM until Context
  teardown. The browser discards terminal workers; library embedders must drop
  Fatal/Closed realms. Binding queue cleanup is not immediate whole-VM
  reclamation or a complete cooperative GC-work guarantee.
- Full CSS is not implemented. Flex/grid, positioning and stacking are bounded
  subsets. Transforms, rounded corners, complete line clamping, CSS imports and
  independent element scrolling remain unsupported. `@font-face` admits only
  bounded same-origin TTF/OTF sources.
- CSS, fonts, images and scripts share 2 MiB, 32 requests and ten seconds.
  Stylesheets/fonts remain same-origin; foreign HTTPS images and classic scripts
  load without cookies. TLS verification stays enabled. PNG, JPEG, first-frame
  GIF and restricted static SVG work; unsupported images remain placeholders.
  Dynamic resources are not fetched. No native codec, font, crypto or JavaScript
  implementation backend is introduced.
- Performance Dashboard, CLI profiler and immutable Frame/style caches remain.
  Their 16 MiB / 8 MiB retained-payload ceilings apply per Browser, with uncached
  fallback above the caps. Shaping has separate per-face bounds. These payload
  counts are not whole-browser memory limits or a scrolling-speed guarantee.
- Linux X11/XWayland is the GUI target. Cookies are memory-only. Size is
  screen-global, not per-monitor Wayland or per-site zoom. Theme follows
  light/dark preferences, not full toolkit skins; decorations remain WM-owned.
  Tabs remain limited to 16 across four windows with two groups per window.

Formal MVP, full JSPLAN, broader WPT and Playwright gates remain open. Project
code and artwork use Apache-2.0 with LICENSE/NOTICE. Third-party dependency
versions, features and licenses are unchanged; preserved texts/source links
accompany the package. Upstream WPT fixtures retain their own notices and are
not shipped in the archive. Published v0.1.0/v0.1.1 remain MIT.

Manual install: download the archive/checksum, verify with `sha256sum --check`,
unpack and run `mgbrowser`. Uninstall by removing the executable, adjacent
`.mgbrowser-auto-update` / `.mgbrowser-update.lock`, and the `mgbrowser` data
directory, `applications/mgbrowser.desktop` and hicolor Mg icons under
`~/.local/share` or `XDG_DATA_HOME`. Optionally remove `~/.config/mgbrowser` or
its `XDG_CONFIG_HOME` equivalent.
