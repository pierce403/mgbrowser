# mgbrowser v0.7.1 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

The toolbar is now one row: Back, Forward, Refresh and Bookmark on the left,
URL in the middle, hamburger menu on the right. The redundant browser/page title
row is removed; the native window title remains.

About shows actual download progress, including received bytes and a percentage
when the server provides the size. Unknown sizes use an activity segment, not an
invented percentage. Verification/installation is a separate stage.

After an update installs, About now offers **Restart now**. The menu's
**Update ready: restart...** item opens the same panel. One click launches the
installed executable, reopens the loaded URL and closes the old browser and
its script workers. Restart remains optional; installation never forces it.
If the executable cannot be launched, the existing window stays open for retry.

Saved bookmarks/theme/size and scripting/automatic-update preferences are kept.
Unsubmitted address edits, form/POST state, cookies, history, scroll and JavaScript
state are not restored. The panel warns about data loss before the click.
The page is reopened with GET, or example.com when no page has loaded.
Debug ports and test-journey flags are not replayed. Older releases need one
manual restart to obtain this button. A newer version installed by another
window or the installer is recognized locally before contacting GitHub.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies SHA-256, installs without sudo to ~/.local/bin and adds
the desktop launcher/icon. MGBROWSER_INSTALL_DIR overrides the binary path;
XDG_DATA_HOME overrides launcher/icon/license data. A DejaVu/Liberation font is
required; MGBROWSER_FONT can select another TrueType/OpenType file.
Settings and bookmarks remain in $XDG_CONFIG_HOME/mgbrowser (default ~/.config).
The release stays within older updaters' unchanged 8 MiB download cap.
Checksums are not independent release signatures. --no-auto-update disables
background checks; Menu > Check for updates and --update provide manual checks.

## Known limitations

- Modern-web compatibility is poor. Google search to first result is not working.
- Boa-backed JavaScript is incomplete and off by default; use --enable-scripts.
  External scripts, modules, timers, fetch/XHR and general event-loop behavior
  are not implemented. No React/Vue browser-app compatibility is claimed.
- Script budgets do not cover all native/parser/regex/GC work cooperatively or
  measure GC/RSS; final OS/parent containment remains necessary. Detached active
  listeners persist until removal or realm teardown.
- Full CSS, flex/grid and general positioning are not implemented. Hacker News
  desktop is a narrow target, not mobile fidelity or account/destination support.
- Resources are bounded and same-origin only: no CSS imports, downloaded fonts
  or dynamic loading. PNG, first-frame GIF and small static SVG images work;
  unsupported images remain placeholders.
- Linux X11/XWayland is the GUI target. Cookies are memory-only. Bookmarks are
  a local flat list of up to 128 entries, without sync/folders/import/export.
- Size is screen-global X11, not per-monitor Wayland or per-site zoom. System
  theme follows light/dark, not toolkit skins; decorations remain WM-owned.
- The restricted JavaScript worker is not a sandbox for the browser as a whole.
- Do not use this release for banking, sensitive authenticated browsing or
  arbitrary hostile websites.

Formal MVP and full JSPLAN gates remain open. Current project code/artwork uses
Apache-2.0 with LICENSE/NOTICE. Dependencies retain their licenses and the package
includes third-party texts/source links. Published v0.1.0/v0.1.1 remain MIT.

Manual install: download the archive and checksum, verify with sha256sum --check,
unpack and run mgbrowser. Uninstall by removing the executable, adjacent
.mgbrowser-auto-update/.mgbrowser-update.lock, and the mgbrowser data directory,
applications/mgbrowser.desktop and hicolor Mg icons under ~/.local/share or
XDG_DATA_HOME. Optionally remove ~/.config/mgbrowser or its XDG_CONFIG_HOME
equivalent. No background service is installed.
