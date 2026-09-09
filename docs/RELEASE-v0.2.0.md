# mgbrowser v0.2.0 Experimental Preview

Mg now has four Rust packages: `mg-browser` for the desktop/platform host,
`mg-chassis` for browser services and optional UX, `mg-butane` for the original
JavaScript engine, and `mg-sparkle` for HTML/DOM/layout/painting.

The libraries have independent examples and tests. Chassis can browse without
its toolbar, and Sparkle can render a local document to pixels without a window.
The existing Linux script worker is supplied through an explicit host interface.
See the [architecture and embedding guide](https://github.com/pierce403/mgbrowser/blob/v0.2.0/docs/ARCHITECTURE.md).

The desktop command, controls and installer remain `mgbrowser`. This is a package
and embedding release; it does not expand web or JavaScript compatibility and
does not yet provide V8, JavaScriptCore, Blink, WebKit or Tauri drop-in adapters.

## Install or update

Supported GUI: Linux x86_64, X11/XWayland, glibc 2.35 or newer. Built with Rust
1.91.1 and the locked dependencies. A DejaVu/Liberation font is required, or set
`MGBROWSER_FONT` to a readable TrueType/OpenType file. Installation needs no Rust
toolchain or sudo.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser --version
mgbrowser https://example.com/
```

The installer verifies SHA-256 and installs to `~/.local/bin`, with a desktop
launcher and Mg icon. Re-run it to update, then restart open browser windows.
`MGBROWSER_INSTALL_DIR` can select an absolute binary directory;
`XDG_DATA_HOME` controls desktop/icon data. Follow the printed PATH instruction.

For manual installation, download `mgbrowser-linux-x86_64.tar.gz` and its
`.sha256`, run `sha256sum --check mgbrowser-linux-x86_64.tar.gz.sha256`, unpack
and run `mgbrowser-linux-x86_64/mgbrowser`. Checksums detect corruption; they are
not independent signatures. `--script-worker-selftest` checks worker isolation
without a display, and `--help` lists controls.

## Limits

- Modern-web compatibility remains poor; Google search to first result is incomplete.
- JavaScript is an incomplete original implementation and remains opt-in with
  `--enable-scripts`. External scripts and a general event loop are incomplete.
- Full CSS and general page-image fetching/rendering remain unimplemented.
- Linux X11/XWayland is the GUI target; ThermiteOS integration is future work.
- Cookies are memory-only. The script worker does not isolate the whole browser.
  Do not use this preview for sensitive browsing or arbitrary hostile websites.
- The Rust embedding APIs are experimental and have no stability guarantee.
  Libraries are available from the repository, not crates.io.

This release uses Apache-2.0 for project-authored code and original artwork,
including LICENSE and NOTICE. Dependencies retain their own licenses, preserved
in THIRD_PARTY_LICENSES.txt. The published v0.1.0/v0.1.1 MIT archives are unchanged.

To uninstall, remove the installed executable and `mgbrowser/`,
`applications/mgbrowser.desktop`, `icons/hicolor/scalable/apps/mgbrowser.svg`
and `icons/hicolor/256x256/apps/mgbrowser.png` under your XDG data directory
(normally `~/.local/share`).
