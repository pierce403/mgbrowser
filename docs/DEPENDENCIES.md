# Rust dependency policy

## Adopted 2026-09-07

This browser is a testing and research project, not a production browser. The user explicitly accepts building on experimental rustls-rustcrypto, including the opportunity to discover and help resolve upstream bugs. This does not authorize weakening certificate/hostname validation or silently substituting native cryptography.

| Component | Current configuration | Boundary |
| --- | --- | --- |
| TLS | rustls 0.23 with explicit rustls-rustcrypto provider; provider revision 70f76c039e587192688af18a80d5d6435dedaf22 | Defaults disabled; std, TLS 1.2 and provider zeroization selected. No ring, aws-lc-rs, OpenSSL or native-tls fallback. |
| Font parsing/rasterization | fontdue 0.9, defaults disabled; Rust ttf-parser transitively | No FreeType, CoreText, DirectWrite or other native font backend. |
| Text shaping | rustybuzz 0.20 with std and defaults disabled | Rust shaping implementation, no HarfBuzz C bindings. |
| Images | image 0.25, defaults disabled, png only | Explicit format allowlist. No automatic/default codec bundle or native fallback. |
| HTTP and URLs | Own HTTP/1.1 transport, std sockets, url 2; webpki-roots 1 for public trust anchors | No external fetch process or native TLS client. Certificate and hostname verification remain enabled. |
| HTTP compression | flate2 1, defaults disabled, rust_backend only | Rust gzip/deflate decoding with a separate decoded-body limit. |
| Session cookies | Own memory-only jar; psl 2 and httpdate 1 | Public-suffix and expiry parsing use Rust crates. No persistent or imported browser cookies. |
| Linux window | x11rb 0.13, defaults disabled, RustConnection | Rust X11 wire protocol over OS sockets, usable through X11/XWayland. No Xlib/XCB FFI or native toolkit rendering. |
| Browser automation | tungstenite 0.28 with only handshake, serde/serde_json, base64 | Loopback-only plain WebSocket CDP. No native TLS, compression or renderer fallback. Rust SHA-1 is used for the standard WebSocket handshake, not TLS certificate trust. |
| JavaScript | Own lexer, parser, evaluator and DOM bridge in Rust | No external parser, runtime, JIT, browser engine or native-language execution backend. Partial and opt-in, not ECMAScript conformance. |
| Script worker controls | libc 0.2 Rust OS declarations; std process/pipes; existing serde_json | Linux x86_64 resource limits, descriptor closure and seccomp. Not a C font/image/crypto backend or replacement JavaScript engine. |

The manifest sets allowed features; Cargo.lock pins the resolved versions. The Git revision intentionally uses maintained upstream source rather than the old crates.io alpha. This is not a claim that the provider is audited, production-ready, or bug-free. Primary references: [provider manifest](https://github.com/RustCrypto/rustls-rustcrypto/blob/70f76c039e587192688af18a80d5d6435dedaf22/Cargo.toml), [release discussion](https://github.com/RustCrypto/rustls-rustcrypto/issues/107), [fontdue](https://github.com/mooman219/fontdue), [rustybuzz](https://github.com/harfbuzz/rustybuzz), [image](https://github.com/image-rs/image).

## Integration status

The mg-deps library now contains the transport and software painter used by the mgbrowser executable. `src/net.rs` implements HTTP(S) GET and URL-encoded POST, relative redirects, chunked responses, gzip/deflate decoding, and shared in-memory sessions. The public root store comes from webpki-roots; the explicit RustCrypto provider handles every TLS connection. `src/paint.rs` shapes and rasterizes a selected font file with Rust code, clips text/rectangles, caches glyphs, and writes PNG snapshots. Font loading checks explicit file paths or `MGBROWSER_FONT`; it does not invoke platform font-processing APIs.

On 2026-09-07, transport tests passed, including local trusted, unknown-issuer, and hostname-mismatch TLS handshakes, redirect/form behavior, response limits, and cookie scoping. Both sides of the local TLS tests use RustCrypto. The public DER fixtures were generated using OpenSSL's command-line tooling; neither the browser nor its tests call or link OpenSSL. Painter tests cover clipping, coverage blending, and text measurement/painting; the dependency smoke test separately covers PNG round-trip. These component checks are distinct from the dated native/CDP journey evidence.

Ordinary Google homepage delivery and real form submission succeeded through the native window with verified TLS and session cookies. The initial script-disabled journey reached a JavaScript-required page. The later opt-in scripting attempt still failed: the search response returned HTTP 200 but unsupported script behavior left no rendered result links. The actual first-result/destination goal remains unmet. Separately, the authored local script-built form and destination journey passed through both native handlers and external CDP. See `JAVASCRIPT.md` and the dated log; fixture success is not Google compatibility. Full CSS and page-image fetching remain unimplemented; image elements show a text/alt placeholder.

The original interpreter runs eligible inline scripts, startup callbacks and bounded later click/submit handlers in one retained realm per document. Browser use requires `--enable-scripts` and a fresh restricted Linux x86_64 worker. The worker has an empty environment, closes inherited descriptors, installs seccomp/resource limits before reading page input, and returns bounded DOM/navigation data to the parent. Its direct libc dependency supplies Rust declarations for those operating-system calls; no native parser, interpreter, font, image or crypto implementation was added. External script loading and a general event loop are absent. Unsupported isolation fails closed; the renderer and network parent remain outside this boundary. Retention and cumulative lifetime/transaction/wire admission follow `PAGE_SESSIONS.md`.

Requests have a 30-second deadline across HTTP redirects, an 8-second socket/DNS-wait budget per operation, eight HTTP redirects, 64 KiB request/header bounds, and separate 8 MiB encoded/decoded body bounds. The window currently permits two outstanding navigation workers and ignores stale completions. Superseded network operations continue until completion or timeout; cancellation does not interrupt sockets or the platform resolver. A resolver thread can outlive the caller's bounded wait. These transport limits do not constitute network-process isolation or a fully cancellable resource budget. Script workers separately have two cumulative active wall seconds, one CPU second and a 256 MiB address-space cap. Retained sessions also have a 300-second absolute lifetime, 64 transactions and 32 MiB aggregate wire admission; evaluator/DOM/protocol bounds are documented in `JAVASCRIPT.md` and `PAGE_SESSIONS.md`.

Sessions retain at most 128 cookies, 32 per registrable site, 4 KiB per cookie, and 16 KiB per request cookie header. Tests cover host-only/domain/path/Secure matching, public-suffix rejection, secure-name prefixes, replacement, and expiry/deletion. Cookies disappear when the session is dropped. SameSite context enforcement and partitioned cookies are not implemented; Partitioned cookies are rejected.

## Audit upgrades before accepting them

Review the actual target-specific normal/build dependency graph and enabled features, not just crate descriptions. Check build.rs and Cargo `links` declarations, native source files and compiler/FFI dependencies. Absence of a `-sys` name alone proves nothing. Maintain Cargo.lock and use `cargo test --locked`; review its diff on each upgrade. Ensure HTTP clients do not re-enable default rustls crypto features through Cargo feature unification.

The Rust CI workflow runs `tools/check-dependencies.rs`, a regression guard rejecting known native backends and native build helpers in the active normal/build graph. It is a denylist, not proof about arbitrary new dependencies; source review remains necessary. Compile it with `rustc --edition=2024 tools/check-dependencies.rs -o tmp/check-dependencies` and run `tmp/check-dependencies`.

Initial Linux check: the active build uses Rust PNG/deflate, fontdue/rustybuzz/ttf-parser and RustCrypto. No ring, cc or other prohibited backend was built. Cargo metadata/lock resolution also includes inactive packages such as ring; `cargo tree --locked -e normal,build -i ring` has no path, including with `--target all`. Do not confuse resolver inventory with enabled build dependencies. Rust `libc` declarations are present for OS primitives. This is a dependency-feature/build review, not a cryptographic audit or whole-program proof of source language.

OS primitives such as entropy and sockets may use Rust declarations of platform APIs; they do not constitute a bundled C codec or crypto implementation. The initial window uses x11rb's RustConnection and uploads our software-painted pixels to an existing X11/XWayland server. The current surface requires 32-bit little-endian pixels. This policy does not claim the Rust standard library, operating system, window server, compiler or drivers are themselves entirely Rust. Native font processing remains prohibited; additional platform backends need their own interface/dependency review.

## Unsupported or broken images

The browser must eventually show a broken-image placeholder and available alt text when a format is unsupported or decoding fails, while continuing to render the document. There is no decoder fallback through a native library, external process, or remote service. Add a format only after a pure Rust implementation and its transitive features pass review. If no suitable implementation exists, leave it unsupported or implement a bounded Rust decoder with format fixtures and malformed-input tests. Writing a new decoder is a separate feature task, not required now.

## Upstream research evidence

Record provider revision, Rust version, OS/architecture and a minimal local reproducer for failures. Distinguish provider failures from our integration mistakes; preserve failing fixtures when lawful and free of secrets. Report normal interoperability and correctness bugs through a separately authorized upstream contribution. Handle potential security findings responsibly; do not automatically publish exploit details or run offensive testing against third parties. No external issue submission or autonomous testing is configured by this policy.
