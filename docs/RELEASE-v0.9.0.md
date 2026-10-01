# mgbrowser v0.9.0 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

This is a Google News reading-first increment. The signed-out briefing now has
story columns, actual thumbnails and readable metadata. Bounded Rust Taffy
flex/grid sizing integrates with Mg's own document, text measurement and painter.
Relative/absolute/fixed positioning, rectangular clipping, numeric stacking,
fit-content widths, simple inline SVG and two-stop linear backgrounds support
the captured page without executing its scripts. Existing Hacker News captures
and geometry remain regression gates, not substitutes for live News checks.

JPEG decoding uses Rust-only zune/image crates. Stylesheets remain same-origin;
foreign HTTPS images may load without sending or accepting cookies. After the
first origin crossing, credentials stay suppressed even on redirects back to
the page origin. Certificate verification stays enabled. A larger per-sheet
allowance fits within the unchanged 2 MiB shared CSS/image budget. Image source,
decode/cache, request, deadline and layout limits remain bounded. Unsupported
layout/resource values produce diagnostics or readable-flow fallback.

The read-only native inspector, detachable live tabs, left/right groups,
bookmarks, theme/size settings, smooth scrolling and update progress/Restart now
remain available. Ordinary Playwright is not supported yet: its unmodified
pinned acceptance client still fails initialization. Custom CDP journeys do not
prove locator compatibility.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://news.google.com/
```

The installer verifies SHA-256, installs without sudo to ~/.local/bin and adds
the desktop launcher/icon. MGBROWSER_INSTALL_DIR overrides the binary path;
XDG_DATA_HOME overrides launcher/icon/license data. A DejaVu/Liberation font is
required; MGBROWSER_FONT can select another TrueType/OpenType file.
Settings and bookmarks remain under $XDG_CONFIG_HOME/mgbrowser (default ~/.config).
The archive stays within older updaters' unchanged 8 MiB download cap.
Checksums are not independent release signatures. --no-auto-update disables
background checks; Menu > Check for updates and --update provide manual checks.
Restart reopens committed tab URLs with fresh sessions, not unsaved edits,
cookies, history or pane/window placement.

## Known limitations

- Modern-web compatibility is poor. Google search to first result is not working.
- Google News is not pixel-perfect or generally interactive: author fonts,
  some branding/icons and rounded corners are missing. Script-dependent weather
  images/local-news content remain empty or placeholders. Topic destinations
  may use readable-flow fallback. Search, menus, account actions and personalized
  content are not acceptance claims. See docs/GOOGLE_NEWS.md for the exact scope.
- Boa JavaScript is incomplete and off by default; use --enable-scripts.
  External scripts, modules, timers, fetch/XHR and general event-loop behavior
  are not implemented. No React/Vue browser-app compatibility is claimed.
- Script budgets do not cover every native/parser/regex/GC operation cooperatively
  or measure GC/RSS. Final OS/parent containment remains necessary. Detached active
  listeners persist until removal or realm teardown.
- Full CSS is not implemented. Flex/grid, positioning and stacking are bounded
  subsets; transforms, rounded corners, complete line clamping and independently
  scrolling elements remain unsupported. No CSS imports or downloaded fonts.
- Resources are bounded; dynamic loading and general SVG are unsupported.
  PNG, JPEG, first-frame GIF and restricted static SVG images work. Unsupported
  images remain placeholders. No native codec/font/crypto fallback is used.
- Linux X11/XWayland is the GUI target. Cookies are memory-only. Size is
  screen-global X11, not per-monitor Wayland or per-site zoom. Theme follows
  light/dark, not full toolkit skins; decorations remain window-manager owned.
- Tabs are limited to 16 across four windows, with two groups per window.
  Budgets remain per tab/worker, not a whole-browser memory or CPU quota.
- The restricted JavaScript worker is not a sandbox for the browser as a whole.
- Do not use this release for banking, sensitive authenticated browsing or
  arbitrary hostile websites.

Formal MVP, full JSPLAN and Playwright gates remain open. Current project
code/artwork uses Apache-2.0 with LICENSE/NOTICE. Dependencies retain their own
licenses; third-party texts/source links include Taffy's MIT license and the
JPEG decoder's license choices. Published v0.1.0/v0.1.1 remain MIT.

Manual install: download the archive and checksum, verify with sha256sum --check,
unpack and run mgbrowser. Uninstall by removing the executable, adjacent
.mgbrowser-auto-update/.mgbrowser-update.lock, and the mgbrowser data directory,
applications/mgbrowser.desktop and hicolor Mg icons under ~/.local/share or
XDG_DATA_HOME. Optionally remove ~/.config/mgbrowser or its XDG_CONFIG_HOME
equivalent. No background service is installed.
