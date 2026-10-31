# mgbrowser v0.9.27 Experimental Preview

CSS layout fidelity, anti-aliased `border-radius` rendering and rounded clipping,
full CSS 2.1 Appendix E stacking contexts and `z-index` paint order, `position:
absolute` hypothetical static positioning across block/inline/flex/grid and
`position: sticky` clamping, `direction: rtl`, CSS Sizing 4 `aspect-ratio`,
linear `calc(% + px)` in flex/grid, CSS 2.1 `float`/`clear` and BFC float
avoidance, `overflow: auto | scroll` and `table-cell` clipping, general
`::before`/`::after` pseudo-elements, `visibility: collapse`, and CSSOM
container-query length units. Honest upstream WPT coverage advances from `339`
to **`528 PASS / 421 FAIL / 62 UNSUPPORTED` of `1,011`** (`+189` net new passes,
zero losses) on the static reftest lane and from `15/16` (`77/79`) to **`16/16`
files and `79/79` final subtests** (`100%`) on the Window parsing lane.
Exact-source publication and public-install receipts are recorded in the dated
work log.

Linux x86_64 / X11 or XWayland, glibc 2.35 or newer. Rust 1.91.1, locked
dependencies, Apache-2.0 license, and pure-Rust implementation backends (zero
new crate dependencies).

## CSS Layout Fidelity & WPT Coverage (`v0.9.27`)

- **Rounded Corners & Clipping (`crates/mg-sparkle/src/paint.rs`, `styled_layout.rs`)**:
  - Implements CSS Backgrounds 3 §4.5 elliptical corner resolution, overlap
    scale factor `f`, inner padding/content radii, 4×4 supersampled boundary
    anti-aliasing without outer background bleed, diagonal per-side corner
    borders, and stackable rounded `CanvasClip` masking across fills, text,
    gradients, images, and `overflow != visible`, while preserving an exact
    zero-overhead fast path when all outer radii are zero.
- **Stacking Contexts & CSS 2.1 Appendix E Painting Order (`crates/mg-sparkle/src/styled_layout/stacking.rs`)**:
  - Establishes stacking contexts for `opacity < 1.0`, `position: fixed | sticky`,
    and numeric `z-index` on positioned boxes and flex/grid items, ordering own
    backgrounds/borders, negative `z-index` child contexts, in-flow blocks,
    floats, inline/atomic-inline and `order`-sorted flex/grid items, DOM-ordered
    `z-index: auto` positioned pseudo-contexts interleaved with `z-index: 0`
    contexts, and positive `z-index` child contexts with clip-safe paint and hit
    reordering.
- **Positioned & Sticky Layout (`crates/mg-sparkle/src/styled_layout/positioned.rs`, `formatting.rs`)**:
  - Computes hypothetical static positions for `position: absolute | fixed` with
    `auto` insets across block, inline, flex, and grid formatting contexts (LTR
    and RTL), plus `position: sticky` clamping within scrollports and containing
    block content boxes.
- **RTL, `aspect-ratio`, Flex/Grid `calc()`, Floats, Overflow, Pseudo-Elements & Collapse**:
  - `direction: rtl` across block horizontal margins, bidi inline runs, table
    columns, and Taffy flex/grid axes; CSS Sizing 4 `aspect-ratio` transfer and
    automatic minimum sizing; linear `calc(% + px)` in flex/grid; CSS 2.1 §9.5
    `float` / `clear` and BFC float avoidance; `overflow: auto | scroll` and
    `table-cell` padding-box clipping; general `::before` and `::after`
    pseudo-element cascade and layout materialization; and `visibility: collapse`
    flex struts and table tracks.
- **CSSOM Container-Query Units & Upstream WPT Scores (`crates/mg-sparkle/src/style/declarations.rs`)**:
  - Supports `cqw`, `cqh`, `cqi`, `cqb`, `cqmin`, and `cqmax` in CSSOM
    `CSSStyleDeclaration` parsing and serialization without C++ bindings,
    bringing the pinned WPT Window parsing lane to **16/16 passing files and
    79/79 passing final subtests**, while lifting the pinned WPT static flexbox
    reftest lane to **528 PASS / 421 FAIL / 62 UNSUPPORTED of 1,011** (`+189`
    new passes, `0` regressions).
