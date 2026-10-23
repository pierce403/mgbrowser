# Definite-width atomic inline sizing

Frozen 2026-10-23 before test execution or production implementation. This is
an adopted bounded increment under F-003/F-005/F-026 and the active broader
feature goal, not completion of those features or a prediction of WPT passes.
Baseline main: `4839542ecd8dc17dce9e367aadd524a17fea99a1`.
The preceding v0.9.15 release and documentation closeout passed all eight
exact-source checks, twelve public HTTPS/source/report checks and separate
public installation/update/native acceptance; final closeout receipt SHA-256
`2026bff246a676de3fdff9cf444aed01cc4ccf8dd7f4ad2c302a1c4521e17e5e`.
The previous goal turn was concrete progress, not a status-only turn.

## Motivation and boundaries

The complete unchanged selection currently reports319 PASS/630 FAIL/62
UNSUPPORTED of1011, with a separate Window lane of15/16 files and77/79 final
rows. Preserve every319 passing/949 runnable static case, the original24,
fifteen passing Window files and all resource/security/diagnostic assertions.
Neither scorer, corpus, original input/reference, protocol, dependency graph nor
resource profile changes in this increment. Authored controls do not enter WPT.

The source-derived review identifies two distinct ordinary inline-call errors:
outer intrinsic width is supplied as a forced border width, and inherited text
snapshots can inflate that intrinsic width before the parent's edges are added.
For example, a10px box with190px left margin paints200px; a64px content box
with32px horizontal padding on each side can paint192px instead of128px.
These are generic box-model errors in rendered references as well as pages, not
missing physical justification keywords. Do not special-case a test, selector,
reference, site, text string or output hash. Do not globally erase text snapshot
fields or change unrelated table/auto-intrinsic behavior.

Source/evidence freeze: `styled_layout.rs` SHA-256
`81ce7858235c93769d016be9e2ed56c4ed9cd851cd4d8d9e8d27417f9c037171`;
complete baseline report SHA-256
`b9dfb845265c9472ce7c68744fc2518e1225ccc16fc6becfa5e10b748e254e1b`.
The original five failures retain22080/42320/56140/24924/50688 differing pixels.
Some references also rely on float/clear: repairing this box boundary does not
promise all five will pass. Keep full CSS, floats, bidi, vertical alignment,
container units, CSSOM, empty-anchor activation and general WPT milestones honest.

For ordinary nonreplaced inline-blocks with an explicit numeric/percentage
width, resolve content/border dimensions, min/max and percentage edges using the
actual definite containing content width, not residual line space or an already
chosen outer/border width. Signed non-auto margins contribute once to wrapping
and advance; overflowing definite boxes do not shrink to the line. Use one
coherent allocation path for width admission, line metrics and emitted geometry.
Preserve existing clipping, pointer order and measurement-only Scene isolation.

Explicit policy decision before implementation: horizontal auto margins on these
ordinary atomic inline boxes resolve to zero, independently of block centering.
A new helper may not borrow block auto-margin distribution or change positioned,
flex/grid, table, replaced, structured-button or auto-width semantics incidentally.
This is a normal-flow context, not a global reinterpretation of computed display.
Test the inline route, not an assumed block-context measure_outer equivalence.
Auto width and intrinsic algorithms remain regression gates, not new conformance.

Primary references: [CSS2.2 width and inline-block rules](https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width)
and [CSS Box Sizing3, 4 September2026 draft §3.3](https://www.w3.org/TR/2026/WD-css-sizing-3-20260904/#box-sizing).
CSS2.2 §10.3.9 was read during the preceding review; this turn's refresh returned
an unspecified fetch error, not new source evidence. The dated sizing draft was
refetched. This limited implementation is not full standards conformance.

## Acceptance sequence

1. Add independent public geometry/pixel/hit and nested private measurement
   controls against real Stylo snapshots. Freeze their bytes, then run them on
   the unchanged production code and retain genuine assertion failures. Do not
   repair expected values from candidate output. Separate infrastructure errors.
2. Implement only this shared inline sizing boundary; preserve both original
   failure records and old tests. No test-only production API or alternate engine.
3. Run focused and full debug/release Rust, no-chrome embedding, dependency/
   component/tooling and all frozen JSPLAN checks. Run both complete unchanged
   WPT selections, inspect every changed page/outcome/resource record and only
   manually ratchet independently verified new passes. No weakened expectations.
4. Ship v0.9.16 with normal Rust1.91.1/locked/default binary, exact-commit CI/site,
   <=8MiB archive/checksum, exact public install/reinstall and actual v0.9.15
   update/no-op. Repeat thirteen native helpers, the unchanged physical proof and
   both frozen worker suites, plus an additive installed native/CDP sizing proof
   at1x/2x with literal geometry/pixels, ordinary text-link navigation, distinct
   destination paints, actual request/lifecycle/cleanup evidence and screenshots.
   Keep user profiles/desktop untouched. A source push alone is not delivery.

## Frozen authored matrix

The matrix below was designed from CSS box arithmetic before execution. Its
source-only precursor SHA-256 was
`0fdd7203e02bc2ba6cceaf3ee6b80f61adfcfc755dec42e048e175db79b44432`.
It does not prescribe production implementation. Root review also freezes:

- Auto margin controls: C200/P40/A40/N10 with both auto margins, left-only auto,
  right-only auto and auto paired with a10px fixed opposite margin. Auto resolves
  to0, not the leftover block-space distribution; retain text-align positioning.
- Actual auto-height text flow under an explicit width must use the same width
  for measurement and emission; test deterministic pinned-font line splitting
  without introducing a new vertical-align or glyph rasterization oracle.
- Private controls use real compute_styles, not the synthetic legacy styles()
  helper that omits inherited text-box fields. Exercise whole flow_tokens in
  both EMIT modes and the actual following sentinel border, not just a correctly
  functioning unforced block measurement. Repeat false measurement on cold and
  populated scenes and verify every Scene vector, parallel metadata, clip and
  static-position map/capacity remains unchanged. Preserve work/depth exhaustion.

### Matrix scope and oracle

Ordinary nonreplaced display:inline-block boxes whose width is explicit px or a
percentage resolved against a definite containing content width. Horizontal
padding, borders and signed non-auto margins must be counted exactly once.
Ordinary inline whitespace and text-align continue to control line placement.
Do not change Stylo parsing, computed snapshots, the general table/intrinsic
algorithm, vertical alignment, bidi, floats, limits or dependency profiles.

Use independent authored HTML, not upstream bodies/expected values. Default
each fixture to html/body margin0, all margins/padding/borders0, font-size1px,
line-height0. A fixed wrapper padding-top10px/padding-left20px places the
containing block's content origin at (20,10). Its width C is stated below.
Adjacent markup must have NO whitespace between atomic siblings, except the
explicit whitespace control. All atomic boxes have content height10px and no
vertical edges, so their border height and each line's height are exactly10px.
Use ordinary literal-text anchors, text color matching their solid background,
so both paint and existing link hit paths can be checked without empty-link
default-action assumptions. A is red#aa0000; prefix P is blue#0000aa; next
sentinel N is green#00aa00. Borders, when present, are solid black.

Notation: W=declared width; E=horizontal padding+borders; Cw=used content width;
Bw=border width; O=SIGNED margin-box advance. Unless wrapping occurs,
border-left = line cursor + ML and next cursor = old cursor + O.
No margin collapse; no zero clamp on O. Public rectangles are border boxes,
not margin boxes. Every interval below is half-open, at y10 with height10.
Private measured sizes remain floats; public integer checks use integral cases.

### A. Numeric width/edge allocation (containing width200)

N is width10 and follows immediately unless the row says otherwise.

| ID | A CSS (all omitted edges0) | Cw/Bw/O | A border x interval | N x |
| --- | --- | --- | --- | ---: |
| A1 | width40px; content-box | 40/40/40 | [20,60) | 60 |
| A2 | width64px; padding-left/right32px | 64/128/128 | [20,148) | 148 |
| A3 | width40px; PL10,PR20,BL2,BR3; ML7,MR11 | 40/75/93 | [27,102) | 113 |
| A4 | A3 plus box-sizing:border-box | 5/40/58 | [27,67) | 78 |
| A5 | A4 with width20px (edge floor) | 0/35/53 | [27,62) | 73 |
| A6 | width80px; ML-10,MR5 | 80/80/75 | [10,90) | 95 |
| A7 | width80px; ML10,MR-15 | 80/80/75 | [30,110) | 95 |

A1/A2: repeat empty and with ordinary text. The border width cannot grow merely
because text snapshots retain owner width/padding. A3/A4 demonstrate that the
same declared width changes sizing box, not edge multiplicity. A5 asserts a
nonnegative content box, not a35px new declared-width minimum.

Literal paint/hit probes: A1 red+link A at(25,15), N green+link N at(65,15);
A2 red/A at(140,15), N green/N at(150,15). A3 black/A at(27,15), red/A at(40,15),
black/A at(101,15), WHITE/no hit at(108,15), green/N at(115,15).
A4/A5 red/A at(40,15); N at(80,15)/(75,15). A6 red/A at(15,15), WHITE/no hit at
(92,15), green/N at(97,15). A7 red/A at(90,15), green/N at(100,15): the later N
must win the overlap rather than the earlier A retaining the topmost hit.

### B. Real containing-block percentage basis

C=200. A has W50%, PL10%, PR5%, BL2px, BR3px, ML5%, MR2.5%.
The resolved numbers are W100, PL20, PR10, BL2, BR3, ML10, MR5; E35.
All percentages retain basis200 in measurement, wrapping, min/max and emission.

| ID | Setup/sizing | Cw/Bw/O | A border | A content starts | N x |
| --- | --- | --- | --- | ---: | ---: |
| B1 | no prefix, content-box | 100/135/150 | [30,165) | 52 | 170 |
| B2 | no prefix, border-box | 65/100/115 | [30,130) | 52 | 135 |
| B3 | preceding P width40, content-box | 100/135/150 | [70,205) | 92 | 210 |
| B4 | preceding P width40, border-box | 65/100/115 | [70,170) | 92 | 175 |

N width10 makes B3 an EXACT200px line fit: P40+O150+N10=200, ending x220.
Wrong remaining-line basis160, the135/100px chosen border width, or the150/115px
outer width must not become the percentage basis. Probe red/A at(60,15) B1/B2,
(100,15) B3/B4; black/A at each stated border-left; green/N at N x+2.
Also place the same width200 block inside a wider padded ancestor: ancestor
padding/viewport width do not replace the nearest containing content width200.

### C. Numeric min/max precedence (C200; A3 edges/margins)

| ID | A width/min/max and sizing | Cw/Bw/O | A border | N x |
| --- | --- | --- | --- | ---: |
| C1 | W80,max50; content-box | 50/85/103 | [27,112) | 123 |
| C2 | W20,min50; content-box | 50/85/103 | [27,112) | 123 |
| C3 | W80,min70,max50; content-box | 70/105/123 | [27,132) | 143 |
| C4 | W80,max50; border-box | 15/50/68 | [27,77) | 88 |
| C5 | W20,min50; border-box | 15/50/68 | [27,77) | 88 |
| C6 | W80,min70,max50; border-box | 35/70/88 | [27,97) | 108 |
| C7 | W25%,min30%,max20%; content-box | 60/95/113 | [27,122) | 133 |

Each row: red/A at(40,15), black/A at border-left and border-right-1;
WHITE/no hit at N x-3; green/N at N x+2. Min wins contradictory min/max; do not
apply padding before numeric content-box min/max or twice for border-box.
Repeat C7 after P40 to prove min/max still use200: add40 to border/N coordinates.

### D. Overflow, signed advance and exact line boundaries

No edges except stated margins. All y values below are border tops.

| ID | C and adjacent sequence | Exact A / following rectangle |
| --- | --- | --- |
| D1 | C100; P60,A50 ML-10 | A=(70,10,50,10), same line, total advance100 |
| D2 | C99; P60,A50 ML-10 | A=(10,20,50,10), wrapped; no leftover-space basis |
| D3 | C105; P60,A50 MR-10,N5 | A=(80,10,50,10), N=(120,10,5,10) overlap |
| D4 | C104; same D3 sequence | A=(80,10,50,10), N=(20,20,5,10) |
| D5 | C100; P60,A20 ML-30,N50 | A=(50,10,20,10), N=(70,10,50,10) |
| D6 | C120; P60,A50 ML10 | A=(90,10,50,10), exact fit |
| D7 | C119; same D6 sequence | A=(30,20,50,10), wrapped |
| D8 | C200; A240,N10 | A=(20,10,240,10), N=(20,20,10,10) |
| D9 | C100; white-space:nowrap; P60,A70 | A=(80,10,70,10), no wrap |

D5 requires O=-10 for A; clamping its advance to0 is wrong. D3/D4 do not clamp
the50px border to residual45/44px. Probe A red/link within its nonoverlapped
region; D3 N green/link at(122,15), D4 N at(22,25); D8 red/A at(250,15), white/
no hit at(265,15). No overflow:hidden or viewport clipping in these controls.
Separately repeat D8 with overflow:hidden on C200: preserve its raw border
allocation in private checks, but public visible boxes/paint/hits follow the
existing clip to[20,220); do not demand unclipped public Inspector geometry.

Content overflow control: A width30/height10, text of twenty M glyphs at font12,
line-height12, overflow:hidden; N width10. A=(20,10,30,10), N=(50,10,10,10).
The text must not enlarge the declared box or subsequent advance; red/A(25,15),
green/N(55,15), white/no hit(65,15). No glyph-width or baseline golden required.

### E. Shared sizing, alignment and regression gates

- text-align:center/right, C200, adjacent A40/N10: borders A x95/N135 and
  A x170/N210 respectively, both y10. Alignment uses total outer50, not each
  border independently. Test positive margin gaps separately from text spaces;
  explicit source whitespace must use the pinned font's actual space advance,
  not a guessed pixel constant or an upstream expectation copied into a helper.
- Test integer geometry/pixel/hits at scales1 and2. A fractional private case
  W20.5, PL1.25,PR2.25,BL1,BR2,ML3.5,MR-1.5 has Cw20.5/Bw27/O29,
  border x23.5; next cursor49. Do not round contributions before layout.
  Public checks use existing physical_edge/scene rounding, not relaxed tolerance.
- Reuse public formatting_contexts.rs:1-96 and physical_justify.rs:1-124 helpers
  (render_scaled/Fonts, last node LayoutBox, exact solid pixel/topmost link hit).
  Pure private seams: styled_layout.rs::tests measurement_layout,
  assert_no_scene_allocation, measure_outer, existing paint-free cache/work tests.
  Exercise inline_box_width and actual flow_tokens/line with EMIT=false and true;
  testing measure_outer alone would miss the current wrong inline caller.
- Require measured outer O/line height equal emitted advance/height for every
  matrix row, same containing basis and no Scene allocation on measurement.
  Repeated measuring must not append/shift boxes, paint or hits. Keep cache-key,
  depth, work-exhaustion and no-partial-pass behavior; no new counters/allowance.
- Auto-width including min/max shrink-to-fit, tables/inline-table, replaced
  img/svg/controls, structured buttons and inline-flex/grid are REGRESSIONS,
  not new sizing algorithms. Inline auto margins in the newly reviewed definite
  route follow the explicit zero policy above; other auto-margin contexts must
  not change incidentally. Block distribution is not inline-auto conformance.
- Preserve the separate float/clear confounds in horiz-001b/005, all319 static
  passes,15 Window passes, genuine diagnostics and unchanged limits.
  Run no producer for this design. Actual TDD/full scores/package/public release
  follow the acceptance sequence above. No five-pass promise.

## Adopted addendum: definite inline-flex/grid outer allocation

2026-10-23, after the first complete static candidate gate and before new
authored controls or this extension's production change. The original ordinary
inline-block matrix and forty controls above remain frozen, including their
earlier narrower exclusions. This addendum explicitly supersedes only the
inline-flex/grid outer-allocation exclusion, not their inner layout algorithms.

The first candidate gate was rejected, not adopted:333PASS/616FAIL/62UNSUPPORTED
of1011, with two old-pass regressions. Static report SHA-256
`4b28d8bd46c7713fa1c08efab7ddf015281985799faa49f6073655eb64958f0e`;
owner `787aa3a59dfcfb0af639d436e5d2b468ff62ee44035f5aa717dbf9b3c5432527`.
Window never ran. In both empty flex baseline relations only the shared ordinary
inline-block reference changed. Each complete80-pixel mismatch is the rightmost
four columns of a20px-high final border: the test remains20px wide while the
corrected reference is16px. There is no baseline displacement. Source review
shows width16 plus side margins2 each becomes intrinsic outer20, incorrectly
forced into the remaining inline-flex border-allocation path. Do not restore
that bug in references, add empty-only exceptions or waive old passing cases.

Adopt the same definite numeric/percentage normal-flow outer-allocation rule for
nonreplaced `inline-flex` and `inline-grid`: real containing content width,
unforced declared sizing box, min/max and edges counted once, signed margins
counted once in admission/advance, horizontal auto margins zero. Admission,
metrics and emitted geometry must share that context. Existing Taffy receives
the resulting definite CONTENT width as before. No flex/grid child algorithm,
intrinsic/auto-width route, baseline, vertical-align, formatting-context policy,
resource limit, corpus, scorer, diagnostic or ratchet changes are adopted.

Primary-source review this turn: [CSS Display3 atomic inline definition](https://www.w3.org/TR/2026/CRD-css-display-3-20260605/#atomic-inline),
[Flexbox1 §3 inline-level flex containers](https://www.w3.org/TR/2025/CRD-css-flexbox-1-20251014/#flex-containers)
and [Grid1 §5.2 container sizing](https://www.w3.org/TR/2025/CRD-css-grid-1-20250326/#grid-container-sizing).
The outer inline role is separate from the internal formatting context; Grid
explicitly uses atomic inline sizing in an inline formatting context. These
references justify this narrow shared boundary, not general conformance.

Freeze separate new public and private authored controls before execution;
retain the original public/private control files byte-for-byte. Repeat literal
cases for inline-flex row, inline-flex column and inline-grid, including empty
and nonempty boxes. Use the original origin(20,10), real containing width200,
no inter-box whitespace, deterministic fixed child sizes and diagnostic-free
public1x/2x rendering. These are new authored fixtures, not copied upstream.

- Empty width24/height12/bottom border4/margin3: border(23,13,24,16), signed
  outer advance30, outer height22. A following height22 sentinel starts x50.
- A2 padding once: content64 plus side padding32 gives border128/advance128.
  A3 asymmetric content-box gives border75/advance93; A4 border-box gives40/58.
  Include fixed-size children without glyph/baseline oracles.
- B3 percentage edges after prefix40: real basis200 gives border135 at x70,
  advance150 and next sentinel x210. Repeat border-box and numeric min/max
  precedence with literal expectations from the original arithmetic.
- D5 negative advance: prefix60, width20/ML-30, next50 gives A x50 and N x70.
  An explicit-width auto-margin box after prefix40 must not center in block
  free space. Include auto+fixed opposite margin.
- Overwide width240 in C200 remains240; next10 wraps. Auto-height fixed-size
  children verify measurement/emission height parity without promising new
  intrinsic sizing or baseline behavior.
- Private controls exercise actual inline admission, line metrics and complete
  flow in both EMIT modes, signed advance, cold/populated Scene isolation and
  existing work/depth exhaustion. Preserve all old assertions and budgets.

Record genuine assertion RED on the current ordinary-fixed production before
extending the predicate. Then rerun focused/full gates and BOTH fresh complete
WPT lanes, retaining the rejected run separately. The original319 static passes,
949 runnable cases,24 pilot controls and15 passing Window files remain binding.
No new public score or successful release is claimed by this addendum.
