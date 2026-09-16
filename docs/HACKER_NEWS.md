# Hacker News desktop rendering goal

Adopted 2026-09-09. Status: planned, not implemented or visually accepted.

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
allowed under docs/DEPENDENCIES.md, including candidate Servo-origin components.
Stylo is an option to evaluate for computed styles, not an adopted dependency or
a replacement for layout. Upstream capabilities do not expand this acceptance
scope or waive local integration/resource tests. All gates below remain unchecked.

## Acceptance

- [ ] Same captured HTML/CSS/assets rendered in Mg and a reference browser at
  1024x768 and 1280x800 content viewports, with the same available font fallback,
  zoom and device scale. Inspect both screenshots side by side. Record any
  remaining differences rather than claiming pixel identity across font engines.
- [ ] Centered 85% outer table (minimum 796px), #f6f6ef panel, #ff6600 header,
  actual Y logo, header links and right-aligned login match the reference geometry.
- [ ] All served story rows retain aligned ranks/arrows, title/domain inline flow,
  correctly grouped small metadata, row spacing and wrapping without overlap.
  Check full-page scroll, More, footer separator/links and search field as well
  as the first screen. Fix geometry differences before tuning rasterization.
- [ ] A fresh live homepage also renders correctly without --enable-scripts.
  Exercise ordinary story, comments and More links, back navigation and Ctrl+L.
  Comments/destination styling is not a new acceptance target; no voting, login,
  submission or other account-changing actions are needed.
- [ ] Small authored regression cases cover the implemented CSS/table/image
  primitives, resource bounds/failure fallback and scroll-adjusted hit geometry.
  Preserve and rerun existing CI/resource assertions and native/CDP journeys.
- [ ] Exact-commit CI/Pages, versioned binary release and fresh installation with
  the public curl command pass; packaged version, worker selftest and desktop/icon
  installation verified. Site/release notes describe the narrow compatibility gain.

## Explicit exclusions and dependencies

Desktop first: below-800px/mobile fidelity is deferred. No full CSS, flex/grid,
general event loop, external scripts, new JS builtins, Google diagnostics,
autoresearch evaluator, broad codec support or arbitrary destination compatibility.
Do not loosen TLS, resource bounds, Rust-only dependency rules or worker isolation.
The component extraction shipped in v0.2.1 on 2026-09-16, closing F-013 separately.
That release adds updates/About, not CSS: all HN acceptance gates remain unchecked.

The conversation goal tracker still holds the unfinished Google goal and rejected
creation of this replacement. The user must cancel that goal through the product
controls before this objective can become the active tracked goal. This document
records the adopted repository goal without claiming that tracker action succeeded.
