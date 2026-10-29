# mgbrowser v0.9.22 Experimental Preview

Google News homepage and Topic destination visual parity and styled-layout
improvements. Exact-source publication and public-install receipts are recorded
in the dated work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies and unchanged Rust-only implementation backends.

## Google News visual parity and styled Topic destination

`mg-sparkle` adds bounded CSS and layout improvements so the signed-out Google
News homepage (`https://news.google.com/home?hl=en-US&gl=US&ceid=US:en`) and its
`Top stories ›` (`Headlines`) Topic destination page render in full styled
layout without fallback across `1280x800`, default `1100x727`, and `1024x768`
viewports:

- Outer-box linear `calc(percent + px)` (`Length::Calc` with `outer_calc_only`
  gating) resolves `<main class="IKXQhd">` (`width: calc(100% - 160px)`) and
  button ripple overlays (`width: calc(100% + 2px)`) while retaining all
  existing inner/nested `calc(...)` diagnostics and WPT expectations.
- Empty elements with `:before` `background-image` and `background-size: contain`
  or `cover` cascade and resolve against final box geometry, rendering the
  4-color `Google` wordmark in the top-left `Google News` header lockup.
- Multiplied subtree `opacity` (including `Canvas::text_alpha` and skipping
  `opacity <= 0.0` paints), borderless `<input>` painting (`border-color:
  transparent`), and standard `google-material-icons` / `material-icons`
  ligature mapping (`chevron_right` to `›`) align the search bar and section
  header chevrons with Chromium.
- On the 9,997-node, 262-article `Headlines` Topic destination page, invisible
  (`display: none` or `opacity <= 0.0`) positioned subtrees and unstyled
  `inset: 0` passthrough `<div>` wrappers (`.XlKvRb` around `<a class="WwrzSb">`)
  no longer consume the 512-entry positioned/stacking admission slots while
  retaining `<a class="WwrzSb">`'s full-card link hit box; `positioned::run`
  indexes `border_boxes` in `O(1)`; and `Adapter::measure` skips redundant
  `measure_content_height` passes during horizontal-only Taffy grid track sizing.
- See [the Google News contract](https://github.com/pierce403/mgbrowser/blob/main/docs/GOOGLE_NEWS.md).

## Validation status

Focused `mg-sparkle` unit and integration tests, Hacker News 1024/1280 PNG
byte-identity, component boundary and no-chrome embedding checks, and live
`mgbrowser` Xvfb/CDP/X11 navigation across the Homepage, `Headlines` Topic
destination, and Back button pass. All 8 frozen JSPLAN gates retain all 364
research outcomes. Both complete reviewed WPT lanes are refreshed for this
commit.

## Install and known limitations

```sh
curl -fsSL https://mgbrowser.org/install.sh | bash
```

The installer follows the latest published release and verifies SHA-256 before
installing without sudo. Checksums are not independent signatures. Existing
processes must restart to use an installed update.

Modern-web compatibility is poor. The Google search to first result journey is
not working. Real Reddit loading and full interactive Google News features
remain unfinished goals. Ordinary Playwright is unsupported: autoattachment,
debugger waiting/resume, downloads, utility worlds, promise awaiting and stock
locator message-size policy remain open.

Boa JavaScript is enabled by default (`--disable-scripts` opts out); the DOM,
classic external scripts and browser event loop remain incomplete. Modules,
dynamic scripts, fetch/XHR and general asynchronous resources are not supported.
Full CSS/CSSOM, intrinsic sizing, baselines, bidi and floats remain incomplete.
Fonts/images/tabs/resources are bounded subsets; cookies are memory-only. This
increment does not complete F-011/F-016/F-024/F-025/F-026/F-027 or formal MVP.

Linux X11/XWayland is the supported GUI target. The restricted JavaScript worker
is not a sandbox for the browser as a whole. Do not use this preview for banking,
sensitive authenticated browsing or arbitrary hostile websites. Existing
limits are not whole-browser quotas or GC/RSS measurements. No dependency,
worker authority or resource profile is widened for this increment.

Project code, documentation and original artwork use Apache-2.0. Dependencies
retain their licenses in packaged THIRD_PARTY_LICENSES.txt. Old tags/assets stay
immutable.
