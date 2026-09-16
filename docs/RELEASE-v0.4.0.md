# mgbrowser v0.4.0 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

This release makes Boa 0.22 the browser's opt-in page JavaScript engine. Inline
scripts share modern language state, Promise checkpoints mutate the real Rust
DOM, and retained click/submit handlers preserve controls and default actions.
Butane supplies the execution policy, Sparkle owns the DOM, and the restricted
Linux child contains execution. There is no original-engine production fallback,
JIT, Servo browser embedding or C/C++ JavaScript backend.

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
mgbrowser --enable-scripts https://example.com/
```

Scripting remains disabled by default. The installer verifies SHA-256, installs
without sudo into ~/.local/bin and adds the desktop launcher/icon. Requires a
DejaVu/Liberation font; MGBROWSER_FONT selects another TrueType/OpenType file,
with optional MGBROWSER_FONT_BOLD. MGBROWSER_INSTALL_DIR changes the binary
directory and XDG_DATA_HOME changes the desktop/icon/license data directory.

Existing installed builds check for updates on startup and daily, or use Menu >
Check for updates / `mgbrowser --update`. Restart to use the new build. Re-run the
installer to refresh desktop/icon/license files too. About shows the running
version, compile time and commit. `--no-auto-update` disables background checks.
Checksums are not independent release signatures.

## Known limitations

- Modern-web compatibility is poor. Google search to first result is not working.
- JavaScript integration is incomplete and requires --enable-scripts. External
  scripts, modules, timers, fetch/XHR and general browser event-loop behavior are
  not implemented. No React/Vue browser-application compatibility is claimed.
- Boa's explicit process profile bounds opcodes, sources, jobs and requested
  worker allocations. Native/parser/regex/GC work still relies on final OS and
  parent containment; comprehensive cooperative budgeting remains future work.
  Active detached-node listeners are retained until removal or realm teardown.
- Hacker News desktop remains a narrow visual target, not mobile fidelity,
  account actions or arbitrary linked destinations. Full CSS, flex/grid and
  general positioning are not implemented.
- Resources are bounded and same-origin only. No CSS imports, downloaded fonts
  or dynamic resource fetching. Images support PNG, first-frame GIF and a small
  static SVG shape/path subset; other images remain placeholders.
- Linux X11/XWayland is the supported GUI target. Cookies are memory-only.
- The restricted JavaScript worker is not a sandbox for the browser as a whole.
  Do not use for banking, sensitive authenticated browsing or arbitrary hostile
  websites. The experimental TLS provider is not production assurance.

See [the Boa profile and acceptance boundary](https://github.com/pierce403/mgbrowser/blob/v0.4.0/docs/BOA.md). Full JSPLAN and formal MVP
gates remain open. Project code/artwork use Apache-2.0 with LICENSE/NOTICE.
Dependencies retain their licenses: Boa's MIT option and MPL-2.0 for Stylo-related
crates are preserved in THIRD_PARTY_LICENSES.txt with source archive links.
Published v0.1.0/v0.1.1 archives retain their original MIT licensing.

Manual install: download mgbrowser-linux-x86_64.tar.gz and its .sha256 from this
release, verify with sha256sum --check, unpack and run its mgbrowser binary.
Uninstall by removing the binary, adjacent .mgbrowser-auto-update and
.mgbrowser-update.lock, and mgbrowser/, applications/mgbrowser.desktop,
icons/hicolor/scalable/apps/mgbrowser.svg and icons/hicolor/256x256/apps/mgbrowser.png
under ~/.local/share (or XDG_DATA_HOME). No background service needs removal.
