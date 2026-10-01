# mgbrowser v0.3.0 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

This release brings desktop Hacker News rendering: its real linked stylesheet,
nested tables, inherited compact typography, orange header, actual logo and vote
arrows. CSS computation uses the standalone Rust Stylo crate from Servo. Mg keeps
its own HTML parser, layout, software painting, transport and original JavaScript
implementation. No Servo browser embedding or C/C++ rendering/font/codec backend.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://news.ycombinator.com/
```

The installer verifies SHA-256, installs without sudo into ~/.local/bin and adds
the desktop launcher/icon. Requires a DejaVu/Liberation font; MGBROWSER_FONT can
select a TrueType/OpenType file, with optional MGBROWSER_FONT_BOLD. The known
DejaVu/Liberation bold companion is selected automatically when available.
MGBROWSER_INSTALL_DIR selects another binary directory and XDG_DATA_HOME selects
the desktop/icon/license data directory.

Existing v0.2.1 installs check for updates on startup and daily, or use Menu >
Check for updates / `mgbrowser --update`. Restart to use the new build. Re-run the
installer to refresh auxiliary desktop/icon/license files too. About shows the
running version, compile time and commit. `--no-auto-update` disables background
checks. Checksums are not independent release signatures.

## Known limitations

- Modern-web compatibility is poor. Google search to first result is not working.
- Hacker News desktop is the narrow visual target, not mobile fidelity, account
  actions, comments-page fidelity or arbitrary linked destinations.
- JavaScript remains an incomplete original implementation, disabled by default;
  use --enable-scripts to opt in. External scripts and general browser event-loop
  behavior are incomplete. This release adds no language features.
- Full CSS is not implemented. Stylo computes more properties than our renderer
  supports; flex/grid, general positioning and broad layout compatibility remain
  absent. Transparent gradients can coexist with URL layers, but general gradient
  painting is not implemented.
- Resources are same-origin only, with size/count/time limits. No CSS imports,
  downloaded fonts or dynamic resource fetching. PNG, first-frame GIF and a small
  static SVG path/shape subset work; other images remain placeholders.
- Linux X11/XWayland is the supported GUI target. Cookies are memory-only.
- The restricted JavaScript worker is not a sandbox for the browser as a whole.
  Do not use for banking, sensitive authenticated browsing or arbitrary hostile
  websites. The experimental TLS provider has not become production assurance.

Project code/artwork use Apache-2.0 with LICENSE/NOTICE. Dependencies retain their
licenses, including MPL-2.0 for Stylo-related crates. THIRD_PARTY_LICENSES.txt
preserves texts and links to each unmodified registry source archive. Earlier
v0.1.0/v0.1.1 archives retain MIT licensing. Formal MVP gates remain open.

Manual install: download mgbrowser-linux-x86_64.tar.gz and its .sha256 from this
release, verify with sha256sum --check, unpack and run its mgbrowser binary.
Uninstall by removing the binary, adjacent .mgbrowser-auto-update and
.mgbrowser-update.lock, and mgbrowser/, applications/mgbrowser.desktop,
icons/hicolor/scalable/apps/mgbrowser.svg and icons/hicolor/256x256/apps/mgbrowser.png
under ~/.local/share (or XDG_DATA_HOME). No background service needs removal.
