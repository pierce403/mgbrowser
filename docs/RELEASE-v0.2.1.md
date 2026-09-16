# mgbrowser v0.2.1 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

This release includes the Butane/Sparkle/Chassis component extraction previously
prepared as v0.2.0, plus automatic self-updates and Menu > About mgbrowser.
No v0.2.0 binary was published; existing v0.1.0/v0.1.1 artifacts remain unchanged.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies SHA-256, installs without sudo into ~/.local/bin and adds
the desktop launcher/icon. Requires a DejaVu/Liberation font; MGBROWSER_FONT can
select a TrueType/OpenType file. MGBROWSER_INSTALL_DIR selects another binary
directory and XDG_DATA_HOME selects the desktop/icon data directory.

Installed builds check for newer stable-channel releases in the background,
download a tag-pinned payload, verify SHA-256 and validate the candidate's version
and worker selftest before atomic replacement. Restart open windows to use it.
Menu > Check for updates or `mgbrowser --update` checks manually. Disable automatic
checks with `--no-auto-update`, MGBROWSER_NO_AUTO_UPDATE=1, or remove the adjacent
.mgbrowser-auto-update marker. No forced restart, sudo, updater daemon or telemetry.
Checksums are not independent release signatures. Self-update replaces the binary;
re-run the installer to refresh auxiliary desktop/icon/license files.

Menu > About mgbrowser and `mgbrowser --about` show the running build's version,
compile time in GMT/UTC and source commit. The original v0.1.1 browser cannot
update itself: run the website installer once to get this capability.

## Known limitations

- Modern-web compatibility is poor. Google search to first result is not working.
- Hacker News is still unstyled: external stylesheet loading, CSS cascade and
  table layout are planned, not implemented. This release does not fix CSS.
- JavaScript is an incomplete original implementation, disabled by default;
  enable only with --enable-scripts. External scripts and general browser
  event-loop behavior are incomplete.
- Full CSS is not implemented. Page images are not generally downloaded/rendered.
- Linux X11/XWayland is the supported GUI target. Cookies are memory-only.
- The restricted JavaScript worker is not a sandbox for the browser as a whole.
  Do not use for banking, sensitive authenticated browsing or arbitrary hostile
  websites. The experimental TLS provider and updater are not security audits.

Project code/artwork use Apache-2.0 with LICENSE/NOTICE and a bundled dependency
license inventory. Earlier v0.1.0/v0.1.1 archives retain MIT licensing.

Manual installation: download mgbrowser-linux-x86_64.tar.gz and its .sha256 from
this release, verify with sha256sum --check, unpack and run its mgbrowser binary.
Uninstall by removing the binary, adjacent .mgbrowser-auto-update and
.mgbrowser-update.lock, and mgbrowser/, applications/mgbrowser.desktop,
icons/hicolor/scalable/apps/mgbrowser.svg and icons/hicolor/256x256/apps/mgbrowser.png
under ~/.local/share (or XDG_DATA_HOME). No background service needs removal.
