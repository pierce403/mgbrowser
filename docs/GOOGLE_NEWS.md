# Google News: signed-out desktop reading

T-022 / F-025, authorized 2026-09-17. Status: planned, not accepted.
This records observations and a proposed implementation sequence. Taffy, JPEG
and expanded resource policies are not adopted by this document or v0.8.0.

## Goal and observed baseline

Render the actual signed-out, scripts-disabled page faithfully at 1280x800 and
1024x768: recognizable header, briefing, story groups, metadata and thumbnails,
without overlapping critical content. Then verify scrolling, one actual served
topic/story link and Back. Search, account actions and personalization come later.
Use ordinary verified HTTPS. Consent/challenges are reported boundaries, not
permission to impersonate another browser or replace the page with invented news.

Fresh Mg observation on 2026-09-17: HTTP 200 at
`https://news.google.com/home?hl=en-US&gl=US&ceid=US:en`, 1,849,110 bytes,
31 links, six admitted stylesheets and 22 resource warnings. The inspected first
screen has overlapping header/menu text, expanded weather and missing thumbnails.
Local receipts: `tmp/desktop-google-news-live.log` and the matching `.png`.
These ignored artifacts are local evidence, not files distributed with this doc.

An earlier saved response contains a 1,227,493-byte main inline stylesheet,
rejected by the existing 256 KiB per-sheet loader limit. The main HTML fits the
unchanged 8 MiB navigation limit. A sampled same-origin attachment redirects to
gstatic and is JPEG: both the current origin policy and codec allowlist reject it.
Changing news responses account for differences between capture counts.

## Structural blockers, not just missing stylesheet admission

- Header `.gb_Td`, briefing `.UJdj6` and story rows `.LU3Rqb` use flex layout.
- Main `.IKXQhd` and briefing `.XhbDsd` use 12-column
  `repeat(12,minmax(0,1fr))` grids. `.TDaRVd` spans eight columns and `.RzhCVe`
  spans four. Flex alone cannot reproduce the first-screen structure.
- Current Stylo preferences reject grid during parsing. Supporting it requires
  deliberate `layout.grid.enabled` initialization and typed computed properties,
  not just adding a layout dependency.
- Header/navigation use fixed positioning. Collapsed weather uses absolute
  positioning inside a zero-width wrapper and ancestor `overflow:hidden`.
  Painting, hit testing and scroll extent must all respect those relationships.
- Real thumbnails, branding/inline SVG, clipping and text/entity details need
  comparison against the actual assets. Do not draw site-specific replacements.

An earlier scratch render admitted all seven original inline sheets directly to
the style engine: 1,266,936 CSS bytes within its existing 2 MiB admission limit.
This bypassed the production loader and fetched no assets or scripts. The generic
percentage-height correction reduced the same input's 1280px content height from
9,522 to 4,406 pixels; the inspected output still fails the reading goal.
Receipts: `tmp/google-news-{full-css,height-fixed}-experiment.log` and screenshots.
The observed roughly 70 MiB peak process RSS is a measurement, not a memory quota.

## Candidate dependency and adapter evidence

The ignored `tmp/taffy-adapter-probe.T8K6A5/` prototype used already-downloaded
Taffy 0.14.0 with defaults disabled and `std,flexbox,grid`, plus reviewed
arrayvec 0.7.8 and smallvec 1.16.1. Offline Rust 1.91.1 debug/release runs passed.
Taffy declares MIT and Rust 1.71 minimum; preserve its license text on adoption.
The probe borrowed one source arena with cache/layout side tables, not a second
DOM. Its synthetic monospace measurement is not Mg shaping or News compatibility.

- Fixed 280x168 thumbnail plus text: at width 680, text occupies 384x80;
  shrinking to 420 makes text 124x240; wrapping at 560 places it at y184.
- Twelve-column grid at width 1040: eight-column group width 688, four-column
  group width 336, gap 16. Nested cards gain height when narrowed to width 920.
- Cold nine-node grid: 143 child-layout calls, 39 text measurements, 66 cache
  hits. Identical warm flex: one cache hit and no text measurement.
- Changing text without invalidation returns stale geometry. Returning a failure
  sentinel can leave partial cached results: reject the whole pass and discard
  its caches/scene. Callback counts do not bound every internal algorithm loop.
- Measured per-node types: Cache 368 bytes, paired Layout records 152 bytes,
  full Style 552 bytes. Do not blindly duplicate them across the whole DOM.

Existing image 0.25.10's opt-in JPEG feature was separately reviewed with
zune-jpeg 0.5.15 / zune-core 0.5.3, Rust 1.75 minimum and MIT/Apache-2.0/Zlib
license options. A captured 19,622-byte JPEG decoded as 280x168 in the isolated
probe. That does not enable JPEG in the browser. Follow [DEPENDENCIES.md](DEPENDENCIES.md).

## Smallest production migration

1. Extract paint-free measurement without changing layout behavior. Reuse
   `Fonts::width_weight` and `css_line_height` in `paint.rs`. In
   `styled_layout.rs`, share `tokens`/`flow`/`line` wrapping and line metrics;
   preserve existing whitespace, baseline approximation and rounding.
   `intrinsic` already includes CSS sizing and box edges: expose content-only
   measurements so Taffy does not count those edges twice. Separate natural
   replaced sizes and table height measurement from final scene emission.
2. Add typed flex properties in `style.rs` and a private low-level adapter.
   Borrow Mg identities; represent anonymous inline runs and `display:contents`
   deliberately. Dispatch new flex contexts only: retain old block/table paths.
   Use bounded, pass-local measurement caches first, not persistent invalidation.
3. Add bounded grid tracks, repeat/minmax/fr, spans and intrinsic auto rows, with
   the explicit Stylo preference. Charge owned track vectors in style accounting.
4. Add relative/absolute/fixed containing blocks, insets, ordered paint and
   rectangular ancestor clips. Taffy does not supply Mg's clipping or painter.
   Emit paint, inspector boxes and hits once from final geometry. Fixed boxes
   must not receive ordinary scroll subtraction or inflate document height.
   Press/release and activation must resolve the same topmost target.
5. Integrate reviewed stylesheet/image policy and JPEG separately, then compare
   the real page. Unsupported sizing, calc or layout must remain diagnostic,
   not silently become zero or claim successful support.

## Resource and regression gates

Keep 2,000,000 layout work steps, 200,000 scene operations/boxes, depth 256,
16,384 characters per text node, finite extents and all existing assertions.
Charge repeated measurement cumulatively; review a bounded scratch-memory
profile and expanded grid-track admission before dependency adoption. A callback
budget is not hard CPU containment. TLS and worker isolation stay unchanged.

Review a larger per-sheet CSS allowance against the existing 2 MiB aggregate,
not an unbounded cap increase. Preserve source ordering, duplicate charging,
request counts, deadlines and visible rejection. Cross-origin image requests
need an explicit HTTPS/redirect/credential policy: do not reuse page cookies or
accept third-party cookies incidentally. Validate policy before each redirect's
socket is opened, and test both initial-request and redirect rejection.
Retain 512 KiB image source, 2048-side/1,048,576-pixel and decoded-cache bounds.
JPEG MIME/decoder admission, malformed input and dimension rejection need tests.
No external scripts, downloaded fonts or general resource expansion is implied.

## Acceptance and stop condition

- Preserve same-input [HN](HACKER_NEWS.md) screenshots, box/hit lists and content
  height at 1024/1280 after measurement extraction and each layout increment.
  The prior percentage-height change preserved those outputs byte-for-byte;
  this is not evidence that the proposed Taffy integration will do so.
- Test flex shrink/wrap, mixed text/table/replaced leaves, 8/4-span grids,
  nested auto-height cards, clipping, fixed-header scroll and overlapping links.
  Check fractional widths and 100/125/200% input/paint alignment.
- Compare identical real News HTML/CSS/assets in Mg and a reference browser with
  matching viewport/fonts/scale, including the first screen and scrolled content.
  Then separately repeat fresh live load, actual served link navigation and Back.
- Retain style/image/table/percentage-height tests, dependency/component guards,
  styled/no-chrome embedding, normal CI, and packaged/public installer acceptance.
  User-visible delivery requires a new release, not merely a source push.

Static acceptance does not complete Playwright, JSPLAN, Google search or the
formal Linux MVP. External scripts, fetch/XHR, timers, account actions and full
interactive News remain later gates. Stop this milestone at faithful reading.
