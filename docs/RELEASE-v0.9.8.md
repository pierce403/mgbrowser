# mgbrowser v0.9.8 Experimental Preview

This increment adds bounded uncaught Window error reporting. Exact-commit CI,
fresh WPT measurement, packaging and public-install/native verification receipts
are recorded separately in the
[dated work log](https://github.com/pierce403/mgbrowser/blob/main/memory/logs/2026-10-22.md).
Authored controls alone are not public-release acceptance.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

## Bounded Window errors

- Ordinary uncaught script, lifecycle, retained-handler and timer failures can
  deliver a genuine cancelable, non-bubbling Window `error` event. A traced
  worker-owned wrapper preserves the actual thrown value where exposure is
  permitted; escaped events retain it, while unreachable events can release it.
  Host extraction does not invoke thrown-object getters or coercions.
- Function-valued `window.onerror` receives message, filename, line, column and
  error. Only literal `true` cancels through its return value; ordinary listener
  returns do not cancel. Cancellation retains the bounded host diagnostic and
  does not turn failed execution into success. Recursive reporter failures keep
  diagnostics without recursively dispatching forever. Fatal policy/engine
  failure invokes no reporter JavaScript or later jobs/callbacks.
- The parent supplies typed final-response and complete-redirect-chain
  provenance for admitted classic external sources. Any foreign hop remains
  muted even after a same-origin return. Cache hits and deferred sources preserve
  that proof; HTML, `src` changes and listener locations cannot grant it.
- Exposure uses a deliberately conservative **sticky realm-wide floor**.
  Foreign or unknown admitted code mutes all errors in that realm, including an
  earlier inline failure before later deferred code executes. Successful or
  failed dynamic compilation attempts also make later errors opaque. A new
  realm has its own policy; cancellation or a navigation proposal does not reset
  the old one. Actual exception identity is exposed only in a proven
  same-origin-only realm, not from an inferred throw site.
- Unknown filename and line/column remain empty string and zero, including for
  unmuted errors. Muted events contain `Script error.`, empty filename, zero
  coordinates and null error; they do not secretly retain the foreign exception.
  This is not complete interoperable source-location reporting.

Modern retained sessions use protocol v4 and reject v3 frames before execution.
The explicit legacy v1 test lane retains its old wire shape and strict reader;
new provenance metadata, including null, is rejected there. This is not bidirectional
mixed-version compatibility. Metadata consumes existing wire/host-storage
admission; source counters still measure JavaScript text. See the
[reviewed contract](https://github.com/pierce403/mgbrowser/blob/v0.9.8/docs/WINDOW_ERRORS.md).

## Existing limits and WPT scope

The shared bounds remain 256 pending / 2,048 lifetime jobs and host tasks,
32 cumulative listeners, 256 events, 1,000,000 cumulative opcodes, 1 MiB per
source / 4 MiB lifetime source admission, 32/64 MiB outstanding/cumulative
requested System allocation, a 4 MiB DOM allowance, one CPU second and 256 MiB
address space. Sessions retain two seconds total active parent time, 300 seconds
absolute lifetime, 64 transactions, 2 MiB request / 4 MiB response frames and
32 MiB aggregate wire. Two owned script children per Browser include active,
pending and retiring ownership. Cancellation and idle waiting renew no credit;
no worker syscall, network/filesystem authority or automatic restart is added.

These are per-worker/session or per-Browser bounds, not whole-workspace quotas,
GC live heap or RSS. Parser/native/regex/GC work is not fully cooperatively
metered; OS, allocator and parent containment remain necessary. The original
interpreter is a separate test lane, not a production fallback.

The freshly measured pinned 1,011-case static flexbox reftest selection reports
317 PASS, 632 FAIL and 62 UNSUPPORTED, zero infrastructure failures or pending
cases and confirmed cleanup. Selection, expectations and scoring are unchanged;
all case outcomes, 1,464 page pixel hashes and request records match v0.9.7. Authored error
controls are not new WPT passes or additions to its denominator. Original
24-case and expanded pass/support ratchets remain intact. The website reports
the exact tested source commit, which can differ from an installed release.
This is not full WPT or JavaScript testharness support. See
[method and limitations](https://github.com/pierce403/mgbrowser/blob/v0.9.8/docs/WPT.md).

## Install and update

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer follows GitHub's latest published release. It verifies SHA-256 and installs without sudo to
`~/.local/bin`, with a desktop launcher and Mg icon. `MGBROWSER_INSTALL_DIR`
overrides the binary path; `XDG_DATA_HOME` overrides launcher/icon/license data.
A DejaVu/Liberation font is required; `MGBROWSER_FONT` can select another
TrueType/OpenType file. Checksums are not independent release signatures.
Packages must fit the unchanged 8 MiB compressed / 64 MiB unpacked limits.
Settings/bookmarks remain under `$XDG_CONFIG_HOME/mgbrowser` (default
`~/.config/mgbrowser`).

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
- Function timers remain bounded; string timers and intervals are unsupported.
  Startup still bundles lifecycle callbacks/checkpoints rather than implementing
  a streaming parser or fully task-separated lifecycle. Promise rejection
  delivery, messaging, typed harness observation and complete Window/testharness
  readiness remain open. `Promise.reject` alone does not become Window `error`.
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
