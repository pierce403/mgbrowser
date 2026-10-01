# mgbrowser v0.9.1 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

This is the first standards-test-driven CSS correction after the v0.9.0 reading
increment. Preferred `width: max-content` now uses the intrinsic content width
instead of rejecting the declaration or shrinking it to available space. Numeric
constraints, box edges, inline placement, flex/grid allocation and positioned
boxes retain their own sizing rules. A wrapped page can still overflow: this
keyword is not a promise that content fits the window.

Column flex items can share a synthesized baseline in supported single-line
horizontal/LTR groups, including reverse wrapping. Participating left margins
must be identical. Finite-height wrapped groups and unequal left-margin sizing
remain explicit diagnostics, not full baseline or writing-mode support.

The website publishes the current source's pinned Web Platform Tests pilot and
its exact tested commit on every successful Pages deployment. It uses original
upstream files and a fixed denominator, not rewritten assertions. The selected
static reftests are not the entire WPT suite, a JavaScript conformance score or
proof of modern-site compatibility. See docs/WPT.md for results and limitations.
The unchanged pilot improves from 6 to 7 passes, with 0 failures and 17
unsupported tests out of 24. No test files or assertions were rewritten.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies SHA-256 and installs without sudo to ~/.local/bin, with
a desktop launcher and Mg icon. MGBROWSER_INSTALL_DIR overrides the binary path;
XDG_DATA_HOME overrides launcher/icon/license data. A DejaVu/Liberation font is
required; MGBROWSER_FONT can select another TrueType/OpenType file. Checksums are
not independent release signatures. The archive remains within existing
updaters' 8 MiB download cap. Settings and bookmarks stay under
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
- Boa JavaScript is incomplete and off by default; use --enable-scripts.
  External scripts, modules, timers, fetch/XHR and general event-loop behavior
  are not implemented. No React/Vue browser-app compatibility is claimed.
- Script budgets do not cover every native/parser/regex/GC operation cooperatively
  or measure GC/RSS. Final OS/parent containment remains necessary. Detached
  active listeners persist until removal or realm teardown.
- Full CSS is not implemented. Flex/grid, baseline alignment, positioning and
  stacking remain bounded subsets. Transforms, rounded corners, complete line
  clamping, CSS imports, downloaded fonts and independent element scrolling
  remain unsupported. No broad baseline or intrinsic-sizing conformance claim.
- Resources are bounded. PNG, JPEG, first-frame GIF and restricted static SVG
  work; unsupported images remain placeholders. No dynamic loading or general
  SVG. Stylesheets remain same-origin; foreign HTTPS images load without cookies.
  No native codec/font/crypto fallback is used. TLS verification remains enabled.
- Linux X11/XWayland is the GUI target. Cookies are memory-only. Size is
  screen-global, not per-monitor Wayland or per-site zoom. Theme follows
  light/dark, not full toolkit skins; decorations remain window-manager owned.
- Tabs are limited to 16 across four windows with two groups per window.
  Budgets remain per tab/worker, not a whole-browser memory or CPU quota.
- The restricted JavaScript worker is not a sandbox for the browser as a whole.
- Do not use this release for banking, sensitive authenticated browsing or
  arbitrary hostile websites.

Formal MVP, full JSPLAN, WPT and Playwright gates remain open. Project code and
artwork use Apache-2.0 with LICENSE/NOTICE; dependencies retain their licenses.
Third-party texts/source links are included. Upstream WPT fixtures keep their
own notices and are not shipped in this archive. Published v0.1.0/v0.1.1 remain MIT.

Manual install: download the archive and checksum, verify with sha256sum --check,
unpack and run mgbrowser. Uninstall by removing the executable, adjacent
.mgbrowser-auto-update/.mgbrowser-update.lock, and the mgbrowser data directory,
applications/mgbrowser.desktop and hicolor Mg icons under ~/.local/share or
XDG_DATA_HOME. Optionally remove ~/.config/mgbrowser or its XDG_CONFIG_HOME
equivalent.
