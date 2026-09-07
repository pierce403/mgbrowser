# Rust dependency policy

## Adopted 2026-09-07

This browser is a testing and research project, not a production browser. The user explicitly accepts building on experimental rustls-rustcrypto, including the opportunity to discover and help resolve upstream bugs. This does not authorize weakening certificate/hostname validation or silently substituting native cryptography.

| Component | Initial configuration | Boundary |
| --- | --- | --- |
| TLS | rustls 0.23 with explicit rustls-rustcrypto provider; provider revision 70f76c039e587192688af18a80d5d6435dedaf22 | Defaults disabled; std, TLS 1.2 and provider zeroization selected. No ring, aws-lc-rs, OpenSSL or native-tls fallback. |
| Font parsing/rasterization | fontdue 0.9, defaults disabled; Rust ttf-parser transitively | No FreeType, CoreText, DirectWrite or other native font backend. |
| Text shaping | rustybuzz 0.20 with std and defaults disabled | Rust shaping implementation, no HarfBuzz C bindings. |
| Images | image 0.25, defaults disabled, png only | Explicit format allowlist. No automatic/default codec bundle or native fallback. |

The manifest sets allowed features; Cargo.lock pins the resolved versions. The Git revision intentionally uses maintained upstream source rather than the old crates.io alpha. This is not a claim that the provider is audited, production-ready, or bug-free. Primary references: [provider manifest](https://github.com/RustCrypto/rustls-rustcrypto/blob/70f76c039e587192688af18a80d5d6435dedaf22/Cargo.toml), [release discussion](https://github.com/RustCrypto/rustls-rustcrypto/issues/107), [fontdue](https://github.com/mooman219/fontdue), [rustybuzz](https://github.com/harfbuzz/rustybuzz), [image](https://github.com/image-rs/image).

## Integration status

The root mg-deps library is a small dependency foundation. It constructs a rustls client configuration with an explicit provider and caller-supplied roots, retaining normal certificate validation. It does not yet fetch pages, choose a browser root store, or implement navigation. Smoke tests cover client construction, PNG round-trip, rejection of a disabled codec, and invalid font input. These do not establish TLS handshake interoperability, successful font rendering, parser robustness or browser readiness.

## Audit upgrades before accepting them

Review the actual target-specific normal/build dependency graph and enabled features, not just crate descriptions. Check build.rs and Cargo `links` declarations, native source files and compiler/FFI dependencies. Absence of a `-sys` name alone proves nothing. Maintain Cargo.lock and use `cargo test --locked`; review its diff on each upgrade. Ensure HTTP clients do not re-enable default rustls crypto features through Cargo feature unification.

The Rust CI workflow runs `tools/check-dependencies.rs`, a regression guard rejecting known native backends and native build helpers in the active normal/build graph. It is a denylist, not proof about arbitrary new dependencies; source review remains necessary. Compile it with `rustc --edition=2024 tools/check-dependencies.rs -o tmp/check-dependencies` and run `tmp/check-dependencies`.

Initial Linux check: the active build uses Rust PNG/deflate, fontdue/rustybuzz/ttf-parser and RustCrypto. No ring, cc or other prohibited backend was built. Cargo metadata/lock resolution also includes inactive packages such as ring; `cargo tree --locked -e normal,build -i ring` has no path, including with `--target all`. Do not confuse resolver inventory with enabled build dependencies. Rust `libc` declarations are present for OS primitives. This is a dependency-feature/build review, not a cryptographic audit or whole-program proof of source language.

OS primitives such as entropy and sockets may use Rust declarations of platform APIs; they do not constitute a bundled C codec or crypto implementation. Record those interfaces separately. This policy does not claim the Rust standard library, operating system, compiler or drivers are themselves entirely Rust. Windowing and platform font discovery are not yet selected; introducing native font processing through them is prohibited.

## Unsupported or broken images

The browser must eventually show a broken-image placeholder and available alt text when a format is unsupported or decoding fails, while continuing to render the document. There is no decoder fallback through a native library, external process, or remote service. Add a format only after a pure Rust implementation and its transitive features pass review. If no suitable implementation exists, leave it unsupported or implement a bounded Rust decoder with format fixtures and malformed-input tests. Writing a new decoder is a separate feature task, not required now.

## Upstream research evidence

Record provider revision, Rust version, OS/architecture and a minimal local reproducer for failures. Distinguish provider failures from our integration mistakes; preserve failing fixtures when lawful and free of secrets. Report normal interoperability and correctness bugs through a separately authorized upstream contribution. Handle potential security findings responsibly; do not automatically publish exploit details or run offensive testing against third parties. No external issue submission or autonomous testing is configured by this policy.
