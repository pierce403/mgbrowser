# mgbrowser v0.9.33 Experimental Preview

Fixes the oversized Google News search/sign-in backgrounds and vertical story
metadata streaks visible after scrolling, plus unintended italic text from
italic-first downloaded font stylesheets. These are generic rendering and font
selection corrections, with no Google-specific layout overrides. Navigation
labels also retain inline `white-space: nowrap` when painted inside a normal
wrapping block.

## Changes

- Rounded rectangles now translate both vertical edges by the scroll offset
  before physical-pixel conversion. Previously their height grew by that offset.
  The correction covers rounded backgrounds, borders, and overflow masks at
  ordinary, fractional, and doubled display scale.
- The resource loader and painter select the same Unicode-matching normal font
  base nearest weight 400. An earlier italic declaration cannot become the base
  used for normal text. The existing 16-candidate discovery, eight registered
  family, networking, decoding, and resource limits remain unchanged.
- One downloaded base per family remains the current model. Bold and italic
  use existing synthesis; italic-only downloaded families use fallback fonts.
  This is not full CSS font face or variable-font instance matching.

- Inline nowrap text is shaped as one collapsed phrase, matching its intrinsic
  measurement instead of summing separately shaped words. This prevents
  rounding-induced navigation wraps and preserves ordinary wrapping around the
  phrase. Grouping across nested style/text-node boundaries remains incomplete.

## Evidence and remaining scope

Pixel regressions exercise fixed rounded controls at scroll 64, 338, and 600,
scales 1, 1.35, and 2, plus a translated rounded overflow mask. Font regressions
cover italic-first declarations, normal weight selection, Unicode filtering,
candidate bounds, and rendered normal-text parity.

The actual captured Google News HTML was compared with a Chromium reference at
1280x800 and 1024x768, including scroll 600. With the same source, DejaVu Sans,
disabled scripts, and external resources blocked, search remains 48px tall and
sign-in 40px tall; metadata separators remain dots. Existing minor spacing,
image placeholders, and script-dependent content remain separate limitations.
This saved-input comparison alone does not complete F-025's live-site criteria.

Local gates, exact-source CI, publication, and public installer results are
recorded separately in the dated work log. No new dependency or resource profile
is introduced. Linux x86_64 / X11 or XWayland, Rust 1.91.1, Apache-2.0.
