# mgbrowser v0.9.7 Experimental Preview

This incremental preview adds bounded function timers, not a complete browser
event loop. Exact-commit CI, package and public-install verification receipts
are recorded in the [dated work log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-21.md).

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

## Bounded function timers

This release adds function-valued `setTimeout` and `clearTimeout` to the
Boa-backed page path, which remains enabled by default (`--disable-scripts`
opts out):

- A timeout retains its callback and extra argument values in traced,
  worker-owned state. A genuine later parent-issued task transaction runs one
  eligible callback with Window as its receiver, then drains Promise jobs.
  Timers are neither synchronous callbacks nor Promise aliases. Idle waiting
  occurs in the parent; the worker gains no network or filesystem capability.
- Delay conversion can re-enter JavaScript without holding native DOM borrows.
  Monotonic deadlines, upward-rounded wake delays, positive IDs, unique private
  handles and nesting clamping govern eligibility. The worker rechecks the
  advertised token and deadline; early, stale or replayed pumps do not invoke
  callbacks. Cancellation releases callback/argument roots and pending storage,
  not lifetime work credits.
- Window identities, document/Window startup-event targets, property-handler
  slot order and interactive readiness before deferred sources are corrected
  within **bundled snapshot startup**. Init still bundles startup callbacks and
  checkpoints. Timers, including timers created by load handlers, run only in
  later parent-issued transactions. This is not a streaming parser or fully
  task-separated HTML lifecycle.
- Tasks and click/submit activations share backpressure, revision checks and
  projection accounting. Task replies cannot acknowledge control edits or
  produce activation defaults. Newer native edits survive task projection;
  explicit script navigation retains independent parent validation. Browser
  polling advances live background tabs without requiring a paint.

String timers, intervals, full task-separated lifecycle, error/rejection event
delivery and the typed testharness result observer remain unfinished. These
changes do not establish complete upstream testharness readiness or general
browser event-loop conformance. See [the reviewed increment](https://github.com/pierce403/mgbrowser/blob/v0.9.7/docs/WPT_TESTHARNESS.md).

## Existing limits are retained

Promise/native jobs and host tasks share **256 pending / 2,048 lifetime
admissions per worker**, with no independent timer allowance. Canceled work does
not renew cumulative admission. First fatal failure invalidates permits,
releases pending timer roots and prevents later execution.

The existing profile also retains 1,000,000 cumulative opcodes, 1 MiB per source /
4 MiB lifetime source admission, 32/64 MiB outstanding/cumulative requested
System allocation, the 4 MiB DOM allowance, one CPU second and 256 MiB address
space. Sessions retain two seconds total active parent time, 300 seconds
absolute lifetime, 64 transactions, 2 MiB request / 4 MiB response frames and
32 MiB aggregate lifetime wire bytes. Encoding, transfer and projection remain
charged; idle waiting does not renew these counters. Owned script children
remain limited to two per Browser across active, pending and retiring ownership.

These are per-worker/session or per-Browser limits, not a whole-workspace memory
or CPU quota, GC live heap or RSS measurement. Parser/native/regex/GC work is not
fully cooperatively metered; OS, allocator and parent containment remain
necessary. The original interpreter remains a separate test lane, not a
production fallback. See [Boa scope](https://github.com/pierce403/mgbrowser/blob/v0.9.7/docs/BOA.md) and [retained sessions](https://github.com/pierce403/mgbrowser/blob/v0.9.7/docs/PAGE_SESSIONS.md).

## WPT and verification scope

The freshly measured pinned **1,011-case flexbox reftest selection** is
**317 PASS, 632 FAIL and 62 UNSUPPORTED**, with no errors, timeouts or crashes,
measured against the v0.9.7 source. All 24 original passing/runnable requirements
remain immutable. Those are existing static-cohort results, not new timer or
JavaScript testharness passes. Authored timer controls add nothing to the WPT
denominator. All case outcomes, rendered-page RGB hashes and per-page request
records match the prior static-cohort baseline. Passing scripted pages have no
unfinished jobs/tasks; arbitrary timer waiting is not a new scoring policy.
The website reports current-source results and the exact tested commit, which
can differ from an installed release. See [method and limitations](https://github.com/pierce403/mgbrowser/blob/v0.9.7/docs/WPT.md).

## Install and update

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies SHA-256 and installs without sudo to `~/.local/bin`, with
a desktop launcher and Mg icon. `MGBROWSER_INSTALL_DIR` overrides the binary path;
`XDG_DATA_HOME` overrides launcher/icon/license data. A DejaVu/Liberation font is
required; `MGBROWSER_FONT` can select another TrueType/OpenType file. Checksums
are not independent release signatures. Packages must fit the unchanged 8 MiB
compressed download and 64 MiB unpacked limits. Settings and bookmarks remain
under `$XDG_CONFIG_HOME/mgbrowser` (default `~/.config/mgbrowser`).

Installed builds check for updates; **Menu > About > Restart now** is optional.
Restart reopens committed tab URLs with fresh sessions, not unsaved edits,
cookies, history or pane/window placement. `--no-auto-update` disables background
checks. No background service is installed.

## Known limitations

- Modern-web compatibility is poor. Google search to first result is not working.
  Do not use this preview for banking, sensitive authenticated browsing or
  arbitrary hostile websites. The restricted JavaScript worker is **not a
  sandbox for the browser as a whole**.
- Google News still has font/icon/corner differences, script-dependent
  placeholders and destination fallbacks. Search, menus, accounts and
  personalization are not acceptance claims. Ordinary Playwright is unsupported;
  partial CDP is not full automation-client compatibility. The native Inspector
  is read-only, not Chrome DevTools or a JavaScript debugger.
- Inline and bounded classic external sources execute by default. Up to 16
  external sources participate within 32 eligible script elements, at most
  256 KiB each and 1 MiB combined HTML/source. Deferred sources follow ordinary
  sources; `nomodule` provides the classic fallback. Async scripts, nonempty
  integrity metadata and `crossorigin` are skipped with diagnostics. Sources
  require UTF-8 and an admitted JavaScript MIME type. ES modules, dynamic script
  injection, fetch/XHR and React/Vue browser-app compatibility remain unsupported.
  The limited timer increment does not complete lifecycle, error/rejection or
  upstream testharness support.
- Full CSS is not implemented. Flex/grid, positioning and stacking are bounded
  subsets. Transforms, rounded corners, complete line clamping, CSS imports and
  independent element scrolling remain unsupported. `@font-face` admits only
  bounded same-origin TTF/OTF sources.
- CSS, fonts, images and scripts share 2 MiB, 32 requests and one ten-second
  loading allowance. Stylesheets/fonts remain same-origin; foreign HTTPS images
  and classic scripts load without cookies. TLS verification stays enabled.
  PNG, JPEG, first-frame GIF and restricted static SVG work; unsupported images
  remain placeholders. Dynamic resources are not fetched. No native codec,
  font, crypto or JavaScript implementation backend is introduced.
- Performance Dashboard, CLI profiler and immutable Frame/style caches remain.
  Their 16 MiB / 8 MiB retained-payload ceilings apply per Browser, with uncached
  fallback above the caps. Shaping has separate per-face bounds. Cache payload
  counts are not whole-browser memory limits or a scrolling-speed guarantee.
- Linux X11/XWayland is the GUI target. Cookies are memory-only. Size is
  screen-global, not per-monitor Wayland or per-site zoom. Theme follows
  light/dark preferences, not full toolkit skins; decorations remain WM-owned.
  Tabs remain limited to 16 across four windows with two groups per window.

Formal MVP, full JSPLAN, broader WPT and Playwright gates remain open. Project
code and artwork use Apache-2.0 with LICENSE/NOTICE. Third-party versions,
features and licenses are unchanged by this increment; preserved dependency
texts/source links accompany the package. Upstream WPT fixtures retain their
own notices and are not shipped in the archive. Published v0.1.0/v0.1.1 remain MIT.

Manual install: download the archive and checksum, verify with
`sha256sum --check`, unpack and run `mgbrowser`. Uninstall by removing the
executable, adjacent `.mgbrowser-auto-update` / `.mgbrowser-update.lock`, and the
`mgbrowser` data directory, `applications/mgbrowser.desktop` and hicolor Mg icons
under `~/.local/share` or `XDG_DATA_HOME`. Optionally remove `~/.config/mgbrowser`
or its `XDG_CONFIG_HOME` equivalent.
