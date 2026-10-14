# mgbrowser v0.9.3 Experimental Preview

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked dependencies.

This security maintenance release updates the locked `rustls` package from
0.23.44 to 0.23.45 for [GHSA-2mjx-qc3c-rqvc](https://github.com/advisories/GHSA-2mjx-qc3c-rqvc).
The browser's TLS verification and RustCrypto provider configuration remain
enabled. The pinned initial WPT flexbox cohort remains 24 PASS, 0 FAIL, 0 ERROR,
and 0 UNSUPPORTED. No browser feature or website installer behavior is added.

The scope and limitations of the [v0.9.2 preview](RELEASE-v0.9.2.md) still
apply: this is an experimental reading-first browser, not a general-purpose
replacement for a mature browser. Do not use it for banking, sensitive
authenticated browsing, or arbitrary hostile websites. Boa scripting remains
experimental and off by default.

Manual install: download the Linux archive and checksum, verify with
`sha256sum --check`, unpack, and run `mgbrowser`. The [website installer](https://mgbrowser.org/)
tracks the latest GitHub release. Uninstall by removing the executable, adjacent
`.mgbrowser-auto-update`/`.mgbrowser-update.lock`, the `mgbrowser` data directory,
desktop launcher and Mg icons under `~/.local/share` or `XDG_DATA_HOME`, and
optional configuration under `~/.config/mgbrowser` or `XDG_CONFIG_HOME`.

Project-authored code and artwork use Apache-2.0 with `LICENSE` and `NOTICE`;
dependencies retain their licenses. Third-party license texts and source links
are included in the archive.
