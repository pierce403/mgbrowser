# mgbrowser v0.5.0 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

Browser controls now follow your desktop's light/dark preference. Open
**Menu > Settings** for **System**, **Light** or **Dark**. Selection applies
immediately and survives restart. System follows the standard Linux desktop
settings portal while open, with a Light fallback if unavailable. Explicit
overrides do not change your desktop settings.

The toolbar, address field, status bar, menus and About/Settings dialogs share
the selected palette. Page colors remain unchanged; plain HTTP still shows the
red title/address strip and "HTTP: Not secure". Supporting window managers receive
a matching decoration hint, but own the outer frame. The host uses Rust zbus,
not GTK/Qt/libdbus bindings. No page-engine or compatibility changes are included.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies SHA-256, installs without sudo into ~/.local/bin and adds
the Mg desktop launcher/icon. MGBROWSER_INSTALL_DIR overrides the binary path;
XDG_DATA_HOME overrides desktop/icon/license data. A DejaVu/Liberation font is
required; MGBROWSER_FONT selects another TrueType/OpenType file. Settings are
saved to $XDG_CONFIG_HOME/mgbrowser/settings.json (default ~/.config/mgbrowser).
Save failures are reported; the selected palette remains active for that session.

Installed builds check for updates on startup and daily. Use Menu > Check for
updates or `mgbrowser --update` manually. Restart to run the new build; re-run the
installer to refresh desktop/icon/license material too. About shows the running
version, compile time and commit. `--no-auto-update` disables background checks.
The compressed release stays within older browsers' unchanged 8 MiB updater cap.
Checksums are not independent release signatures.

## Known limitations

- System matching means light/dark preference, not copying GTK/Qt skins, accents
  or custom desktop colors. Desktops without the settings portal use Light.
  Window managers may ignore decoration hints. Websites are not forced dark.
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
