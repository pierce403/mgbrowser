# mgbrowser v0.9.28 Experimental Preview

Interactive HTML forms and standalone controls, arbitrary element click/focus
activation, Stylo CSS3 DOM selectors (`querySelector` / `querySelectorAll` /
`matches` / `closest`), modern DOM mutation and Event APIs (`DocumentFragment`,
`DOMParser`, `outerHTML`, `insertAdjacentHTML`, `{ capture, once, passive,
signal }`, `CustomEvent`), Web Platform globals (`URL`, `URLSearchParams`,
`TextEncoder`/`TextDecoder`, `atob`/`btoa`, ChaCha20 CSPRNG
`crypto.getRandomValues`/`randomUUID`, `performance.now`, `setInterval`,
`requestAnimationFrame`, `queueMicrotask`, `structuredClone`, `matchMedia`),
live layout/style queries (`getComputedStyle`, `getBoundingClientRect`,
`scrollTo`), `localStorage`/`sessionStorage`, `document.cookie` (`HttpOnly`
isolated), `history.pushState`/`replaceState`/`popstate`, and parent-brokered
same-origin `fetch()` / `XMLHttpRequest` over the seccomp-confined `VERSION = 9`
page worker session. Exact-source publication and public-install receipts are
recorded in the dated work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies, Apache-2.0 license, and pure-Rust implementation backends (zero
new crate dependencies).

## HTML Forms, Interactive Controls & Modern Web Platform APIs (`v0.9.28`)

- **HTML Forms, Standalone Controls & Arbitrary Element Clicks (`mg-sparkle`, `mg-chassis`)**:
  - Projects standalone controls outside `<form>` alongside form-associated
    controls (`checkbox`, radio groups, `<select>`/`<option>`/`<optgroup>` with
    native dropdown popup overlay, `<button type="button"|"reset"|"submit">`,
    `<textarea>`, `<label>` `for` + descendant forwarding, `<details>`/`<summary>`,
    `<dialog>` `show`/`showModal`/`close`/`returnValue`, `form.elements`/`submit`/
    `reset`/`requestSubmit`, and `<div onclick>` arbitrary element activation).
- **Modern DOM & CSS3 Selector APIs (`crates/mg-sparkle/src/js_browser/boa/`)**:
  - Stylo CSS3 `querySelector`, `querySelectorAll`, `matches`, `closest`,
    `CSS.supports`, and `CSS.escape`; `DocumentFragment`, `DOMParser`,
    `<template>.content`, `cloneNode`, `insertBefore`/`replaceChild`/`remove`/
    `before`/`after`/`replaceWith`/`prepend`/`append`/`replaceChildren`,
    `outerHTML`, and `insertAdjacentHTML`/`Element`/`Text`; `{ capture, once,
    passive, signal }` listener options, `AbortController`/`AbortSignal`,
    `Event`/`CustomEvent`/`MouseEvent`/`KeyboardEvent`/`FocusEvent`/
    `PopStateEvent`/`HashChangeEvent`, `dispatchEvent`, `click()`/`focus()`/`blur()`,
    and `document.activeElement`.
- **Web Globals, Live Layout/Style Queries, Storage, History & `fetch`/`XHR`**:
  - `URL`, `URLSearchParams`, `TextEncoder`/`TextDecoder`, `atob`/`btoa`,
    parent-seeded ChaCha20 `crypto.getRandomValues` & `crypto.randomUUID`,
    `performance.now`/User Timing, `setInterval`, `requestAnimationFrame`,
    `queueMicrotask`, cyclic `structuredClone`, and Stylo `window.matchMedia`.
  - Live `window.getComputedStyle`, pixel-free `compute_boxes`-backed
    `getBoundingClientRect` / `getClientRects` / `offset*` / `client*` / `scroll*`,
    and `scrollTo`/`scrollBy`/`scrollIntoView`.
  - Per-origin `localStorage` and per-tab `sessionStorage`, `HttpOnly`-isolated
    `document.cookie`, same-document `history.pushState`/`replaceState`/`back`/
    `forward`/`go` (`popstate` + `hashchange`), and parent-brokered same-origin
    `Headers`/`Request`/`Response`/`fetch()` and `XMLHttpRequest` over the
    seccomp-confined `VERSION = 9` worker protocol (zero worker network or
    filesystem syscalls).
- **Measured WPT Coverage**:
  - Static flexbox reftest lane advances to **532 PASS / 417 FAIL / 62
    UNSUPPORTED of 1,011** (`+4` new scripted/form flexbox passes over `v0.9.27`,
    zero losses; `expectations.json` ratcheted to protect all `532` passes).
  - Separate real-Browser Window lane retains **16 / 16 files and 79 / 79 final
    subtests (100%)**.
