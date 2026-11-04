# mgbrowser v0.9.32 Experimental Preview

End-to-end live Google Search compatibility (`https://www.google.com/` -> search
query -> SearchGuard `knitsail` challenge & `SG_SS` cookie -> `HTTP 200` SERP
with inline `data:image/` maps/thumbnails -> click first organic result ->
destination), strict `<noscript>` suppression whenever `scripts_enabled == true`,
sloppy direct-`eval` dynamic-binding slot isolation in `boa_engine 0.22.0`,
WebIDL prototype/brand/event/image/window surface completion, `256 KiB`
`data:image/` attribute materialization, styled `Display::Inline` descendant
geometry & `2,000,000`-step worker `compute_boxes` layout parity, and the
explicit `boa-interactive-page-v1` desktop script profile. Exact-source
publication and public-install receipts are recorded in the dated work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies, Apache-2.0 license, and pure-Rust implementation backends (zero
C/C++ bindings or new crates).

## Root-Cause Fixes & Live Google Search Parity (`v0.9.32`)

- **Never Activate `<noscript>` When `scripts_enabled == true` (`crates/mg-chassis/src/lib.rs`)**:
  - Removed the `v0.9.31` fallback that projected `<noscript>` elements and
    followed `<noscript><meta http-equiv="refresh">` to
    `/httpservice/retry/enablejs` ("Turn on JavaScript to keep searching") when
    initial scripts hit `RealmState::Fatal`. Per HTML §4.12.2, `<noscript>` is
    rendered strictly and exclusively when scripting is disabled
    (`--disable-scripts` / `scripts_enabled == false`).
- **Sloppy Direct-`eval` Dynamic `var` Slot Isolation (`vendor/boa_engine-0.22.0/`)**:
  - Fixed an upstream `boa_engine 0.22.0` bug where sloppy direct `eval("var x = ...")`
    inside a repeatedly invoked function mutated the shared `CodeBlock`
    compile-time `Scope` so subsequent calls created `FunctionEnvironment` slots
    treating earlier `eval` `var` declarations as undeleted static bindings and
    mis-evaluating `typeof` in `GetNameOrUndefined` — causing SearchGuard's
    `knitsail` VM to silently abort ~50% of cold sessions at ~143 admissions
    with `HTTP 429` instead of completing ~222 admissions with `HTTP 200`.
- **Complete WebIDL Prototype, Event, Image & Window Surface (`crates/mg-sparkle/src/js_browser/boa/`)**:
  - Installed branded `Window`, `Navigator` (`PluginArray`, `MimeTypeArray`),
    `HTMLIFrameElement` (`contentWindow`, `contentDocument`), `HTMLMediaElement` /
    `HTMLVideoElement` / `HTMLAudioElement`, `HTMLImageElement` (`naturalWidth`,
    `naturalHeight`, `complete`, `currentSrc`, `decode`, legacy layout
    reflections), `Element`/`Node` namespace/attribute/visibility methods, 42
    audited `Window` properties/methods (`[Global]`/`[Replaceable]` descriptors,
    `getSelection`, `requestIdleCallback`), and full `MouseEvent`/`UIEvent`
    properties (`screenX`/`screenY`, `x`/`y`, `offsetX`/`offsetY`,
    `layerX`/`layerY`, `movementX`/`movementY`, `view`, `which`,
    `getModifierState`), while preventing modern handles (`navigator`, `history`,
    `mevent:*`, `subdoc:*`, `abortsignal:*`, `xhr:*`) from leaking through
    `host.get`.
- **Inline `data:image/` (`<= 256 KiB`) & Worker Layout Geometry (`crates/mg-sparkle/`)**:
  - Raised `data:image/` `src` attribute and inline image materialization limits
    to `256 KiB` (`<= 8` large `> 4 KiB` inline images, `<= 1 MiB` aggregate) so
    Google SERP inline base64 maps and thumbnails (`19` inline images up to
    `77 KiB`) render cleanly.
  - Updated `geometry_style::node_rect`, `collect_live_stylesheets`,
    `ensure_layout_cached`, and `styled_layout::compute_boxes` (`2,000,000`
    layout budget + descendant box unions for `Display::Inline` anchors) so
    `getBoundingClientRect()` and Playwright `locator.click()` hit-test organic
    SERP links accurately.
- **Interactive Desktop Script Profile (`boa-interactive-page-v1`)**:
  - Interactive desktop browsing (`mgbrowser` default GUI launch or
    `--interactive-script-profile`) uses `ResourceProfile::InteractivePageV1`
    (`32M` opcodes, `16 MiB` cumulative source, `64 MiB` outstanding / `512 MiB`
    cumulative worker allocations, `5 CPU s`, `768 MiB` `RLIMIT_AS`, `10 s`
    active time) with identical seccomp, `RLIMIT_NOFILE = 3`, and
    `RLIMIT_FSIZE = 0` confinement, while preserving `boa-page-process-v1`
    unchanged across all library defaults, worker selftests, WPT runners, and CI
    gates.
- **Preserved WPT Coverage**:
  - Static WPT reftest coverage (`558 PASS / 391 FAIL / 62 UNSUPPORTED` of
    `1,011`) and Window WPT coverage (`16/16` files, `79/79` final subtests)
    remain intact with zero regressions, within the unchanged `< 8 MiB` release
    archive limit.
