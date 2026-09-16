# mgbrowser v0.7.0 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

Menu, Back, Forward, Refresh and Bookmark now sit together to the left of the
URL. Selected URL text is highlighted without coloring the unused address field.
Alt+Left/Right navigates history; Ctrl+R/F5 refreshes the loaded page, ignoring
an unsubmitted address edit.

Use the star or Ctrl+D to save/remove the loaded page. Menu > Bookmarks or
Ctrl+Shift+O opens a local list with open/remove controls and pagination.
Bookmarks survive restart. Storage is bounded to 128 HTTP(S) URLs, rejects URL
credentials, merges edits from multiple windows and preserves malformed files.
There is no sync, account, folder, rename or import/export support yet.
URLs may contain private query data: bookmarks are local files, not encrypted
secrets. Do not bookmark sensitive token-bearing URLs.

Desktop-aware sizing, saved System/Light/Dark choices and whole-browser size
shortcuts remain unchanged. This release adds controls, not web compatibility.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies SHA-256 and installs without sudo into ~/.local/bin,
including the desktop launcher/icon. MGBROWSER_INSTALL_DIR overrides the binary
path; XDG_DATA_HOME overrides desktop/icon/license data. A DejaVu/Liberation font
is required; MGBROWSER_FONT selects another TrueType/OpenType file.
Bookmarks live in $XDG_CONFIG_HOME/mgbrowser/bookmarks.json (default
~/.config/mgbrowser/bookmarks.json), alongside appearance settings.json.
Save failures appear in the status bar and bookmark dialog. Fix the file or
directory and retry; invalid existing data is not silently overwritten.

Installed builds check for updates at startup and daily. Menu > Check for updates
or mgbrowser --update checks manually. Restart to run the new build. About shows
version, compile time and commit. --no-auto-update disables background checks.
Re-running the installer also refreshes desktop/icon/license material.
The archive remains within older browsers' unchanged 8 MiB updater limit.
Checksums are not independent release signatures.

## Known limitations

- Modern-web compatibility is poor. Google search to first result is not working.
- Boa-backed JavaScript integration is incomplete and disabled by default:
  use --enable-scripts. External scripts, modules, timers, fetch/XHR and general
  browser event-loop behavior are not implemented. No React/Vue compatibility claim.
- Script budgets bound opcodes, source/jobs and worker allocation requests,
  not GC/RSS or complete cooperative native/parser/regex/GC work. OS/parent
  containment remains necessary. Detached active listeners persist until removal
  or realm teardown.
- Hacker News desktop is a narrow target, not mobile fidelity, account actions
  or arbitrary destinations. Full CSS, flex/grid and general positioning are absent.
- Resources are bounded and same-origin only: no CSS imports, downloaded fonts
  or dynamic resources. Images support PNG, first-frame GIF and a small static
  SVG subset; unsupported images remain placeholders.
- Linux X11/XWayland is the supported GUI target. Cookies are memory-only.
- Size uses screen-global X11 DPI, not per-monitor Wayland or per-site page zoom.
  System theme follows light/dark, not toolkit skins; window managers may ignore
  decoration hints. HTTP pages retain their red warning strip.
- The restricted JavaScript worker is not a sandbox for the browser as a whole.
- Do not use this release for banking, sensitive authenticated browsing or
  arbitrary hostile websites.

Formal MVP and full JSPLAN gates remain open. Project code/artwork use Apache-2.0
with LICENSE/NOTICE. Dependencies retain their licenses, including Boa's MIT
option and Stylo's MPL-2.0. The archive includes third-party texts/source links.
Published v0.1.0/v0.1.1 archives retain their original MIT licensing.

Manual install: download the archive and .sha256 from this release, verify with
sha256sum --check, unpack and run mgbrowser. Uninstall: remove the binary,
adjacent .mgbrowser-auto-update/.mgbrowser-update.lock, and mgbrowser/,
applications/mgbrowser.desktop, icons/hicolor/scalable/apps/mgbrowser.svg and
icons/hicolor/256x256/apps/mgbrowser.png under ~/.local/share (or XDG_DATA_HOME).
Optionally remove settings.json, bookmarks.json and .bookmarks.lock in the
configuration directory above. No background service is installed.
