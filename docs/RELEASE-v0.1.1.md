# mgbrowser v0.1.1 Experimental Preview

<img src="https://mgbrowser.org/assets/mgbrowser.svg" width="96" alt="Burning magnesium Mg tile">

A browser written from the ground up in Rust. This downloadable preview
ships the browser we have: original HTML parsing and rendering, text, links,
forms, navigation, verified HTTPS, an experimental original JavaScript
implementation and a partial Chrome DevTools Protocol interface.

## Changes in v0.1.1

- Loaded HTTP pages have a red title/address strip and an "HTTP: Not secure" label.
  The warning follows the loaded URL, not unsubmitted address edits.
- Existing Ctrl+L location selection and Enter-to-navigate behavior now have
  focused regression coverage, including actual HTTP requests.
- Project-authored em dashes are replaced with colons; HTML entity decoding is unchanged.
- The installer replaces the binary atomically, including when it is in use.
  Restart open browser windows after updating. Re-run the same command to update;
  packaging and release checks now follow the release version.

Supported GUI: **Linux x86_64 on X11 or XWayland**. The binary is built on
Ubuntu 22.04 (glibc 2.35 or newer required), with Rust 1.91.1 and Cargo.lock.
A DejaVu/Liberation font must be installed, or set `MGBROWSER_FONT` to a readable
TrueType/OpenType font file. No Rust toolchain or sudo is needed to install.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser https://example.com/
```

The installer verifies the published SHA-256 and installs to `~/.local/bin`,
with a user-level desktop launcher and Mg icon. Set `MGBROWSER_INSTALL_DIR` to
an absolute directory to override the binary destination. Standard
`XDG_DATA_HOME` controls desktop/icon data. Add the printed directory to PATH
if necessary. You can inspect [install.sh](https://mgbrowser.org/install.sh)
before running it. Checksums detect corruption; they are not independent signatures.

For manual installation, download `mgbrowser-linux-x86_64.tar.gz` and its
`.sha256` file into the same directory, run
`sha256sum --check mgbrowser-linux-x86_64.tar.gz.sha256`, then unpack and run
`mgbrowser-linux-x86_64/mgbrowser https://example.com/`.

Ctrl+L edits the address; Enter navigates/submits; Tab moves between fields;
Alt+Left goes back; the mouse follows links and the wheel scrolls.
`--help` lists options; `--version` prints the release version;
`--script-worker-selftest` tests the restricted worker without a display.
Use `--remote-debugging-port=9222` to enable the partial loopback CDP interface.

## Known limitations

- Modern-web compatibility is poor. Google search → first result is not working.
- JavaScript is an incomplete original implementation, disabled by default;
  opt in with `--enable-scripts`. External scripts and general browser event-loop
  behavior are incomplete.
- Full CSS is not implemented. Page images are not generally downloaded or rendered.
- Linux X11/XWayland is the only supported GUI target. Cookies are memory-only.
- The restricted JavaScript worker is **not a sandbox for the browser as a whole**.
- Do not use this preview for banking, sensitive authenticated browsing, or
  arbitrary hostile websites.

These are intentional preview limitations, not a completed formal MVP. The original v0.1.0
engine cutoff is `4b9a5f74b09f4e3092f26d5c61d6b8a04e22a4da`; v0.1.1 adds
only the requested UI/distribution updates, without a compatibility iteration.
The existing local regression set includes 26 native and 26 external CDP fixture
journeys. Authored fixtures do not establish modern-site compatibility.

Project code and original Mg artwork: MIT, copyright 2026 mgbrowser contributors.
The archive includes LICENSE and THIRD_PARTY_LICENSES.txt with the locked
dependency inventory and upstream notices. Fonts are read from the host, not bundled.

To uninstall, remove the installed binary and the user data files
`applications/mgbrowser.desktop`, `icons/hicolor/scalable/apps/mgbrowser.svg`
and `icons/hicolor/256x256/apps/mgbrowser.png`,
and `mgbrowser/` under your XDG data directory (normally `~/.local/share`).
