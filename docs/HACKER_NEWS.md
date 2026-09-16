# Hacker News desktop rendering goal

Adopted 2026-09-09. Implemented and accepted 2026-09-16. Status: shipped in
v0.3.0 at e3ac7a7b873eb080baf0fa9be61b343b06cbbcb9.

Goal: render the real https://news.ycombinator.com/ homepage recognizably and
faithfully at desktop content widths of 1024 and 1280 pixels, with scripting
disabled, then ship the improvement through a verified release/public installer.
This supersedes the deferred Google compatibility loop, not the formal MVP gates.

## Evidence and minimal implementation sequence

The live homepage, linked news.css, y18.svg and triangle.svg were inspected on
2026-09-09. Raw responses stay in ignored tmp/hn-baseline. Story content changes;
it is not a stable fixture. The homepage is server-rendered HTML with nested
tables, legacy presentation attributes, inline styles and one external stylesheet.
Its script is not needed to display the initial page.

1. **Stylesheet resources and computed style.** Chassis fetches linked CSS with
   bounded sizes/counts, verified TLS, relative URL resolution and navigation
   cancellation. Sparkle receives resource data without acquiring network access.
   Implement or integrate a reviewed Rust CSS parser/cascade: selector lists,
   type/class/id, compound and descendant selectors, applicable link states,
   specificity, source order, inheritance and inline styles. Parse media blocks without accidentally
   applying mobile rules to desktop. Unsupported declarations must not discard
   supported neighboring declarations. Preserve link behavior and hidden content.
2. **Tree-based boxes and tables.** Replace the flat rendering projection where
   needed with DOM-based block/inline and table layout. Support nested tables,
   shared columns, colspan, intrinsic sizing, percentage widths, min-width,
   cellspacing/cellpadding, align/valign, bgcolor and HTML width/height hints.
   Add margins, padding, borders, backgrounds and alignment with CSS overriding
   presentation hints. Honor center and the normal body margin. This is the
   largest gap: a few paint-color changes cannot reproduce the page geometry.
3. **Compact typography and inline flow.** Support inherited font families with
   available sans-serif fallbacks, normal/bold faces, px/pt sizes, line-height,
   whitespace collapsing, baseline alignment, wrapping and decorations. Match
   black story links, gray ranks/domain/subtext, small metadata and the bold
   header name. Verdana availability is not assumed on Linux.
4. **Small real image resources.** Fetch and paint the actual logo and arrow:
   img sizing/border plus CSS background URL and background-size. Both live SVGs
   use paths, fills and viewBox; use a bounded Rust-only implementation covering
   those features, not hostname-specific replacement drawings. Honor explicit
   spacer-image dimensions, including zero. Handle the transparent GIF spacer
   with a Rust-only decoder if needed; never substitute visible alt text for a
   successfully loaded transparent spacer. The transparent gradient layer must
   not prevent the arrow URL layer from painting. No general SVG or codec suite.
5. **Navigation, validation and release.** Keep hit regions tied to final layout
   and scrolling. Preserve existing link/form/browser controls; no new CDP commands
   or JavaScript capabilities should be required. Finish through the standing
   feature-release policy, not a source-only handoff.

Butane remains unchanged. Sparkle owns style/layout/image paint; Chassis owns
resource lifecycle; the platform supplies font data through the existing boundary.
Do not hard-code HN selectors, story content or coordinates in engine behavior.

2026-09-16 dependency clarification: existing Rust implementation crates are
allowed under docs/DEPENDENCIES.md. The implementation adopts standalone Stylo
0.21.0 for computed styles, not Servo's browser or layout. Rust image/resvg paths
have native/font/text/raster defaults disabled. Upstream capabilities do not
expand acceptance scope or waive local integration/resource tests.

## Implemented contract and local evidence

Chassis loads same-origin linked/inline CSS in source order and actual PNG/GIF/SVG
resources, including background-image URLs. Redirects retain same-origin checks
and use the final stylesheet URL as their base. Budgets: 16 sheets, 32 requests,
256 KiB per CSS body, 512 KiB per image, 2 MiB admitted resource data and one
10-second resource deadline. Cancellation prevents further scheduling; an active
socket/resolver operation can finish within the existing bounds. No CSS imports,
downloaded fonts, cross-origin assets or dynamic resource fetching.

Sparkle computes static styles via Stylo, then owns generic block/inline/nested
table layout, intrinsic columns, colspan, presentation hints, regular/bold text,
form controls, scroll-adjusted hits and software paint. The older flow projection
remains a readable fallback on style/layout admission failure. Fonts are supplied
by the host, and no new capabilities enter Butane or its restricted worker.

PNG and first-frame GIF use Rust image; SVG accepts only bounded static shapes,
paths and groups. DTDs, text, filters, use references, embedded images, active
content and file/data resolvers are rejected. Raster limits: 2048 per side and
1,048,576 pixels; decoded image cache: 128 entries/16 MiB. Natural image sizing
is cached by resource URL. Layout admits 2 million work steps, 200,000 scene
operations/boxes, depth 256 and 16,384 characters per text node. Failure falls
back instead of silently truncating the page. These bounds do not sandbox Stylo
or the browser parent. See DEPENDENCIES.md for style snapshot limits.

Captured comparison (2026-09-16): saved real homepage and original assets, UTF-8
response encoding, DejaVu Sans regular/bold, device scale one, no reference
scrollbar gutter. At 1024x768 and 1280x800, inspected Mg/Chrome 146 screenshots
and full-page/footer captures. All 30 story/metadata row integer positions match
the reference: first story y42, last y1053, More y1098, separator y1123, search
field y1168/h21. Panel bounds are reference subpixel values rounded to software
pixels. Total height differs by one rounding pixel (1211 vs 1210). Text
rasterization/baselines and input borders differ slightly; no missing rows,
overlap or lost footer. Raw page content and screenshots remain in ignored tmp/.

## Acceptance

- [x] Same captured HTML/CSS/assets rendered in Mg and a reference browser at
  1024x768 and 1280x800 content viewports, with the same available font fallback,
  zoom and device scale. Inspect both screenshots side by side. Record any
  remaining differences rather than claiming pixel identity across font engines.
- [x] Centered 85% outer table (minimum 796px), #f6f6ef panel, #ff6600 header,
  actual Y logo, header links and right-aligned login match the reference geometry.
- [x] All served story rows retain aligned ranks/arrows, title/domain inline flow,
  correctly grouped small metadata, row spacing and wrapping without overlap.
  Check full-page scroll, More, footer separator/links and search field as well
  as the first screen. Fix geometry differences before tuning rasterization.
- [x] A fresh live homepage also renders correctly without --enable-scripts.
  Exercise ordinary story, comments and More links, back navigation and Ctrl+L.
  Comments/destination styling is not a new acceptance target; no voting, login,
  submission or other account-changing actions are needed.
- [x] Small authored regression cases cover the implemented CSS/table/image
  primitives, resource bounds/failure fallback and scroll-adjusted hit geometry.
  Preserve and rerun existing CI/resource assertions and native/CDP journeys.
- [x] Exact-commit CI/Pages, versioned binary release and fresh installation with
  the public curl command pass; packaged version, worker selftest and desktop/icon
  installation verified. Site/release notes describe the narrow compatibility gain.

Release receipts: [v0.3.0](https://github.com/pierce403/mgbrowser/releases/tag/v0.3.0),
[Rust CI](https://github.com/pierce403/mgbrowser/actions/runs/35105612390),
[Pages](https://github.com/pierce403/mgbrowser/actions/runs/35105612387) and
[tagged release workflow](https://github.com/pierce403/mgbrowser/actions/runs/35107029602).
Published archive SHA-256:
`c31e669a84deb566ab7df520f26d1c528117031fe82161b508f421e76c84f97b`.
The exact public curl command installed/reinstalled 0.3.0, passed the worker
selftest, installed desktop/icons/licenses and resolved current download links.
A verified public v0.2.1 executable updated itself to 0.3.0 and passed a no-op
recheck. The fresh public binary repeated the complete live HN journey below.
Build identity: commit e3ac7a7b873e, Wed, 16 Sep 2026 14:15:09 GMT.

Live acceptance on 2026-09-16: release-built native Mg under Xvfb, scripts off,
1024x768 content. External X11 events exercised Ctrl+L and Alt+Left; the existing
external CDP subset clicked the actual More, comments and first-story links and
scrolled to the footer. All delivered HTTP 200 and returned to the homepage.
The first story led to a Mastodon page that requires JavaScript: delivery is not
destination compatibility. Inspected fresh homepage and bottom screenshots;
no login, vote, submission or account-changing action was attempted.

## Explicit exclusions and dependencies

Desktop first: below-800px/mobile fidelity is deferred. No full CSS, flex/grid,
general event loop, external scripts, new JS builtins, Google diagnostics,
autoresearch evaluator, broad codec support or arbitrary destination compatibility.
Do not loosen TLS, resource bounds, Rust-only dependency rules or worker isolation.
The component extraction shipped in v0.2.1 on 2026-09-16, closing F-013 separately.
That release adds updates/About, not CSS; HN styling belongs to the new v0.3.0
increment. Formal MVP and broader F-005 fixture gates remain open.

The conversation goal tracker still holds the unfinished Google goal and rejected
creation of this replacement. The user must cancel that goal through the product
controls before this objective can become the active tracked goal. This document
records the adopted repository goal without claiming that tracker action succeeded.
