# Decisions and open choices

2026-09-08: User explicitly selects Apache License 2.0, superseding the earlier
MIT fallback for current project-authored source/docs/artwork and future releases.
Preserve dependency licenses and immutable v0.1.0/v0.1.1 MIT archives.

2026-09-08 standing user policy: whenever features are added, roll a new release
and update/verify the installer on the public site. Apply now as v0.1.1 for the
HTTP warning update. Source-only publication is no longer sufficient for feature
handoff. Keep old release tags immutable; this does not revive deferred engine work.

2026-09-08: User explicitly freezes engine/compatibility at
4b9a5f74b09f4e3092f26d5c61d6b8a04e22a4da for v0.1.0 Experimental Preview.
Ship Linux x86_64 X11/XWayland with a binary tarball, checksum installer, Mg
identity and desktop entry. Google, JavaScript/storage iteration, CSS/images,
CDP expansion and evaluator work are deferred. F-007 remains separate and open.
History and decision notes contained no adopted license. Following the user's
explicit fallback, adopt MIT for project code and original artwork in this release.

2026-09-07: User requested a browser written 100% in Rust from the ground up; an open-contribution autoresearch harness; MVP first; Recurse-style FEATURES.md, daily logs and automatic skills; GitHub repository and regularly updated GitHub Pages site at the apex mgbrowser.org.

Proposed, not yet user-confirmed: Linux first; static-document MVP before JavaScript; own browser subsystems with audited Rust utility dependencies and OS interfaces. License is not selected. No browser implementation or autonomous experiment run was requested in the bootstrap task.

2026-09-07 follow-up: User explicitly chose rustls-rustcrypto despite its experimental status because mgbrowser is also research-only and may help uncover upstream bugs. Fonts must avoid native bindings; image features must exclude C bindings. Prefer broken/unsupported images to native fallback; a custom Rust decoder is an option when needed, not an immediate task. This settles TLS/font/image direction; Linux-first, license and OS/window interface choices remain open.

2026-09-07 active goal: User requested a runnable browser that opens Google, submits a search, and follows its first result. Implemented the initial Linux window through x11rb's Rust X11 connection and software pixels; no Xlib/XCB/native toolkit renderer. Google live evidence now makes own JavaScript/DOM integration part of the immediate goal, instead of deferring all JavaScript until after a static MVP. License remains unresolved. The goal remains active and incomplete.

2026-09-07 steering: User requested Chrome DevTools Protocol support to simplify browser automation, accepting a small initial subset but intending eventual full support. Implemented an opt-in loopback endpoint and actual browser command subset; docs/CDP.md records the staged expansion. This does not replace the Google goal or authorize another browser engine/JavaScript runtime.


## 2026-09-09: reusable Mg components

User selected mg-butane for the original JavaScript engine, mg-sparkle for HTML,
DOM and rendering, mg-chassis for browser services plus optional UX, and mg-browser
for platform integration/composition. Long-term replacement targets are V8/JSC
and Blink/WebKit, with ThermiteOS as the intended Rust OS host. User authorized
implementing the split and pushing main. Rust embedding comes first; drop-in
adapters and ThermiteOS integration need later scoped work. No additional public
service-component names are required now.
