# Decisions and open choices

2026-09-07: User requested a browser written 100% in Rust from the ground up; an open-contribution autoresearch harness; MVP first; Recurse-style FEATURES.md, daily logs and automatic skills; GitHub repository and regularly updated GitHub Pages site at the apex mgbrowser.org.

Proposed, not yet user-confirmed: Linux first; static-document MVP before JavaScript; own browser subsystems with audited Rust utility dependencies and OS interfaces. License is not selected. No browser implementation or autonomous experiment run was requested in the bootstrap task.

2026-09-07 follow-up: User explicitly chose rustls-rustcrypto despite its experimental status because mgbrowser is also research-only and may help uncover upstream bugs. Fonts must avoid native bindings; image features must exclude C bindings. Prefer broken/unsupported images to native fallback; a custom Rust decoder is an option when needed, not an immediate task. This settles TLS/font/image direction; Linux-first, license and OS/window interface choices remain open.

2026-09-07 active goal: User requested a runnable browser that opens Google, submits a search, and follows its first result. Implemented the initial Linux window through x11rb's Rust X11 connection and software pixels; no Xlib/XCB/native toolkit renderer. Google live evidence now makes own JavaScript/DOM integration part of the immediate goal, instead of deferring all JavaScript until after a static MVP. License remains unresolved. The goal remains active and incomplete.

2026-09-07 steering: User requested Chrome DevTools Protocol support to simplify browser automation, accepting a small initial subset but intending eventual full support. Implemented an opt-in loopback endpoint and actual browser command subset; docs/CDP.md records the staged expansion. This does not replace the Google goal or authorize another browser engine/JavaScript runtime.
