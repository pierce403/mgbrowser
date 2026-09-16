# mgbrowser v0.6.0 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

Browser controls and websites now follow your desktop's display size. A desktop
configured for 192 DPI selects 200%, instead of drawing tiny 1:1 pixels. Text is
rasterized at physical resolution, not enlarged from a low-resolution screenshot.

Open **Menu > Settings > Size** for **System** or saved sizes from 75% to 300%.
**Ctrl+plus/minus** changes size; **Ctrl+0** restores System. Changes apply
immediately and survive restart. Explicit sizes ignore subsequent desktop DPI
changes. Existing System/Light/Dark theme choices are preserved.

System reads XSETTINGS Xft/DPI, then Xft.dpi, through the existing Rust X11
connection and rechecks while open. Missing or invalid values use 100%. No
toolkit bindings or new dependencies. Logical layout, native mouse coordinates
and CDP CSS input remain aligned; screenshots use physical display resolution.
This is whole-browser sizing, not new page-engine or JavaScript compatibility.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies SHA-256, installs without sudo into ~/.local/bin and adds
the Mg desktop launcher/icon. MGBROWSER_INSTALL_DIR overrides the binary path;
XDG_DATA_HOME overrides desktop/icon/license data. A DejaVu/Liberation font is
required; MGBROWSER_FONT selects another TrueType/OpenType file. Preferences are
saved to $XDG_CONFIG_HOME/mgbrowser/settings.json (default ~/.config/mgbrowser).
Old theme-only files default to System size. Save failures remain visible and
the selected settings still apply to that session.

Installed builds check for updates on startup and daily. Use Menu > Check for
updates or `mgbrowser --update` manually. Restart to run the new build; re-run the
installer to refresh desktop/icon/license material too. About shows the running
version, compile time and commit. `--no-auto-update` disables background checks.
The compressed release stays within older browsers' unchanged 8 MiB updater cap.
Checksums are not independent release signatures.

## Known limitations

- Size follows screen-global X11 DPI, not per-monitor Wayland scaling or
  independent page-only/per-site zoom. Images retain their bounded decoded
  resolution. Native surfaces are limited to 4800x3600 physical pixels.
- System theme follows the light/dark preference, not GTK/Qt skins or accents.
  Without the portal it uses Light; window managers may ignore decoration hints.
  Website colors are unchanged and HTTP pages retain the red warning strip.
- Modern-web compatibility is poor. Google search to first result is not working.
- Boa-backed JavaScript integration is incomplete and disabled by default:
  use --enable-scripts. External scripts, modules, timers, fetch/XHR and general
  browser event-loop behavior are not implemented. No React/Vue browser-app
  compatibility is claimed.
- Script budgets bound opcodes, source/jobs and worker allocation requests,
  not GC/RSS or comprehensive cooperative native/parser/regex/GC work. Final
  OS/parent containment remains necessary. Detached active listeners persist
  until removal or realm teardown.
- Hacker News desktop is a narrow target, not mobile fidelity, account actions
  or arbitrary destinations. Full CSS, flex/grid and general positioning are absent.
- Resources are bounded and same-origin only: no CSS imports, downloaded fonts
  or dynamic resources. Images support PNG, first-frame GIF and a small static
  SVG shape/path subset; other images remain placeholders.
- Linux X11/XWayland is the supported GUI target. Cookies are memory-only.
- The restricted JavaScript worker is not a sandbox for the browser as a whole.
  Do not use for banking, sensitive authenticated browsing or arbitrary hostile
  websites. The experimental TLS provider is not production assurance.

Full JSPLAN and formal MVP gates remain open. Project code/artwork use Apache-2.0
with LICENSE/NOTICE. Dependencies retain their licenses, including Boa's MIT option
and Stylo's MPL-2.0. The archive includes third-party texts and source links.
Published v0.1.0/v0.1.1 archives retain their original MIT licensing.

Manual install: download the tarball and its .sha256 from this release, verify
with sha256sum --check, unpack and run the included mgbrowser executable.
Uninstall: remove the binary, adjacent .mgbrowser-auto-update/.mgbrowser-update.lock,
and mgbrowser/, applications/mgbrowser.desktop, icons/hicolor/scalable/apps/mgbrowser.svg
and icons/hicolor/256x256/apps/mgbrowser.png under ~/.local/share (or XDG_DATA_HOME).
Optionally remove the appearance settings file above. No background service is installed.
