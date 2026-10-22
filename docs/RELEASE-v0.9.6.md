# mgbrowser v0.9.6 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

v0.9.6 corrects the bounded external-script, profiling and rendering-cache paths
introduced in v0.9.5:

- Classic external scripts use typed parent-supplied node/source entries.
  Untrusted HTML comments never supply executable transport metadata. Parent and
  worker validate source eligibility, duplicate nodes, counts and source/wire
  admission before execution. Up to 16 external sources participate within the
  existing 32 eligible script elements, with 256 KiB per source and 1 MiB for
  HTML plus admitted sources. CSS, fonts, images and scripts share the existing
  2 MiB, 32-request, absolute ten-second resource allowance. Duplicate execution
  sources consume admission again; the preload cache retains indexes rather
  than extra source copies. Resource diagnostics share one 32-message cap and
  cumulative omission count across loading phases.
- Deferred external sources execute after ordinary sources and before startup
  callbacks. Since modules remain unsupported, `nomodule` executes its classic
  fallback. Async scripts, nonempty integrity metadata and `crossorigin` are
  explicitly skipped with diagnostics. External responses require UTF-8 and a
  supported JavaScript MIME type; plain text and generic binary responses never
  become executable sources. TLS verification and foreign-source cookie and
  HTTPS-downgrade restrictions remain in force. The worker gains no network or
  filesystem capability. See [script scope and limits](BOA.md).
- The native **Performance Dashboard** (`F12` / `Ctrl+Shift+I` → **Performance**,
  or **Ctrl+Shift+P**) and headless **CLI profiler** (`mgbrowser --profile <URL>`)
  report completed browser paints, including composition and controls, and
  identify the cache path actually used. Worker counters describe System
  allocation requests or explicitly report unavailable accounting; they are not
  GC heap or RSS. Failed navigation returns a nonzero profiler exit status.
- Frame and computed-style caches share immutable snapshots, with retained
  payload limits of 16 MiB and 8 MiB per Browser. Oversized snapshots render
  normally without being retained. RustyBuzz shaping keeps its existing
  4,096-entry/1 MiB per-face cache and reports lookups, hits, misses and evictions.
  Pixel/geometry and invalidation regressions accompany these paths. Measurements
  describe the tested invocation, not a general scrolling-speed guarantee.
  These payload bounds are not allocator/RSS or whole-browser quotas. See
  [performance measurements and cache review](PERFORMANCE.md).

The pinned initial upstream WPT flexbox reftest selection remains **24 PASS,
0 FAIL, 0 UNSUPPORTED out of 24 selected tests**. This is a static reftest cohort,
not the full WPT suite or JavaScript conformance. The website publishes the
current source's score and exact tested commit after its fresh deployment gate.
See [method and limitations](WPT.md). Release, public-installation and native/CDP
receipts are recorded separately in the dated work log.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies SHA-256 and installs without sudo to ~/.local/bin, with
a desktop launcher and Mg icon. MGBROWSER_INSTALL_DIR overrides the binary path;
XDG_DATA_HOME overrides launcher/icon/license data. A DejaVu/Liberation font is
required; MGBROWSER_FONT can select another TrueType/OpenType file. Checksums are
not independent release signatures. The installer and updater retain their
existing 8 MiB compressed download cap. Settings and bookmarks stay under
$XDG_CONFIG_HOME/mgbrowser (default ~/.config/mgbrowser).

Installed builds check for updates; Menu > About > Restart now is optional.
Restart reopens committed tab URLs with fresh sessions, not unsaved edits,
cookies, history or pane/window placement. --no-auto-update disables background
checks. No background service is installed.

## Known limitations

- Modern-web compatibility is poor. Google search to first result is not working.
- Google News still has font/icon/corner differences, script-dependent
  placeholders and destination fallbacks. Search, menus, accounts and
  personalization are not acceptance claims. Ordinary Playwright is unsupported.
- Boa JavaScript runs inline and bounded classic external scripts by default
  (use --disable-scripts to opt out). This is startup preloading, not a streaming
  parser or general browser task loop. ES modules, dynamic script injection,
  timers and fetch/XHR remain unsupported. No React/Vue browser-app compatibility
  is claimed. Async/CORS script loading and integrity validation are not
  implemented; affected external sources are skipped as described above.
- Script budgets do not cover every native/parser/regex/GC operation
  cooperatively or measure GC/RSS. Final OS/parent containment remains necessary.
  Detached active listeners persist until removal or realm teardown.
- Full CSS is not implemented. Flex/grid, baseline alignment, positioning and
  stacking remain bounded subsets. Transforms, rounded corners, complete line
  clamping, CSS imports and independent element scrolling remain unsupported.
  `@font-face` is limited to bounded same-origin `.ttf`/`.otf` sources.
- PNG, JPEG, first-frame GIF and restricted static SVG work; unsupported images
  remain placeholders. No dynamic loading or general SVG. Stylesheets and fonts
  remain same-origin; foreign HTTPS images and classic scripts load without
  cookies. No native codec/font/crypto fallback is used. TLS verification remains
  enabled.
- Linux X11/XWayland is the GUI target. Cookies are memory-only. Size is
  screen-global, not per-monitor Wayland or per-site zoom. Theme follows
  light/dark, not full toolkit skins; decorations remain window-manager owned.
- Tabs are limited to 16 across four windows with two groups per window.
  Budgets remain per tab/worker, not a whole-browser memory or CPU quota.
  Frame/style cache retention alone can total 384 MiB across 16 tabs; font and
  other allocations have separate bounds.
- The restricted JavaScript worker is not a sandbox for the browser as a whole.
- Do not use this release for banking, sensitive authenticated browsing or
  arbitrary hostile websites.

Formal MVP, full JSPLAN, broader WPT and Playwright gates remain open. Project
code and artwork use Apache-2.0 with LICENSE/NOTICE; dependencies retain their
licenses. Third-party texts/source links are included. Upstream WPT fixtures
keep their own notices and are not shipped in this archive. Published
v0.1.0/v0.1.1 remain MIT.

Manual install: download the archive and checksum, verify with sha256sum --check,
unpack and run mgbrowser. Uninstall by removing the executable, adjacent
.mgbrowser-auto-update/.mgbrowser-update.lock, and the mgbrowser data directory,
applications/mgbrowser.desktop and hicolor Mg icons under ~/.local/share or
XDG_DATA_HOME. Optionally remove ~/.config/mgbrowser or its XDG_CONFIG_HOME
equivalent.
