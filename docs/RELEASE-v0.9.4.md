# mgbrowser v0.9.4 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

Browser interaction and ECMAScript additions in v0.9.4:

- Link hover preview in the bottom-left status bar shows the resolved target URL
  whenever the pointer hovers over or right-clicks a hyperlink.
- Right-clicking a link offers **Open link in new tab** in the context menu, and
  **Ctrl+click** on a link opens its destination in a new tab directly.
- Pure-Rust Boa ECMAScript execution (`BoaPageRealm` in the restricted Linux
  `x86_64` script worker) is now enabled by default (`--disable-scripts` opts
  out; `--enable-scripts` remains accepted), with readable source fallback
  preserved if a page exceeds the 1 MiB script-worker source limit.
- Expanded DOM bindings for modern inline scripts: `Element.classList`
  (`add`, `remove`, `contains`, `toggle`, `replace`, `item`, `value`, `length`),
  `Element.dataset` (`data-*` attribute reflection), `[Symbol.iterator]` and
  array iteration methods on DOM node collections, and boolean attribute
  property reflection (`defer`, `async`, `hidden`, `disabled`, `checked`,
  `required`, `readOnly`, `multiple`, `selected`).

The initial 24-test upstream WPT flexbox reftest selection continues to pass
completely (**24 of 24 selected tests pass**, 0 failures, 0 unsupported). The
website publishes the current source's pinned Web Platform Tests selection and
its exact tested commit on every successful Pages deployment. See docs/WPT.md
for results and limitations.

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
- Boa JavaScript runs inline scripts by default (use --disable-scripts to opt
  out), within a 1 MiB page source limit. External scripts, modules, timers,
  fetch/XHR and general event-loop behavior are not implemented. No React/Vue
  browser-app compatibility is claimed.
- Script budgets do not cover every native/parser/regex/GC operation cooperatively
  or measure GC/RSS. Final OS/parent containment remains necessary. Detached
  active listeners persist until removal or realm teardown.
- Full CSS is not implemented. Flex/grid, baseline alignment, positioning and
  stacking remain bounded subsets. Transforms, rounded corners, complete line
  clamping, CSS imports and independent element scrolling remain unsupported.
  `@font-face` is limited to bounded same-origin `.ttf`/`.otf` sources.
- Resources are bounded. PNG, JPEG, first-frame GIF and restricted static SVG
  work; unsupported images remain placeholders. No dynamic loading or general
  SVG. Stylesheets and fonts remain same-origin; foreign HTTPS images load
  without cookies. No native codec/font/crypto fallback is used. TLS
  verification remains enabled.
- Linux X11/XWayland is the GUI target. Cookies are memory-only. Size is
  screen-global, not per-monitor Wayland or per-site zoom. Theme follows
  light/dark, not full toolkit skins; decorations remain window-manager owned.
- Tabs are limited to 16 across four windows with two groups per window.
  Budgets remain per tab/worker, not a whole-browser memory or CPU quota.
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
