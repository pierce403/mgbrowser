# Google News: signed-out desktop reading

T-022 / F-025, authorized 2026-09-17. Status: **in-progress, not accepted**.
The implementation below shipped in v0.9.0, absent from v0.8.0. Exact-commit
remote CI, publication, public installation and fresh public-binary News
navigation pass. Full visual fidelity remains open.

## Scope and implemented contract

Render the actual signed-out, scripts-disabled page at 1280x800 and 1024x768:
recognizable header, briefing, story groups, metadata and thumbnails, without
overlapping critical content. Verify scrolling, an actual served link and Back.
Search, accounts and personalization are later work. Ordinary verified HTTPS
only: consent/challenges are boundaries, not permission to impersonate another
browser or substitute invented news.

- Standalone Stylo computes typed flex/grid properties with its grid preference
  explicitly enabled before parsing. Taffy 0.14.0 supplies flex/grid geometry
  through borrowed Mg style/document views, not another DOM or browser engine.
  Mg retains text shaping, block/table flow, painting and hit testing.
- Paint-free content/natural-size measurements share the existing wrapping and
  cumulative work budget. Pass-local caches do not survive document changes.
  Anonymous text, `display:contents`, flex order/shrink/wrap/gaps and bounded
  numeric repeat/minmax/fr grids are implemented, including 8/4-column spans.
- Relative, absolute and fixed positioning, ancestor rectangular overflow clips,
  viewport overflow propagation and numeric stacking ancestry use final geometry
  for paint, inspector boxes and hits. Fixed boxes do not grow scroll extent.
  Element overflow auto/scroll renders at its initial offset with rectangular
  clipping and an explicit unsupported-scrolling diagnostic, including flex/grid.
  There are no element scrollbars or element-scroll input/state guarantees.
- `width:fit-content` is explicit, not silently treated as auto. Block sizing
  clamps available content width between intrinsic minima/maxima; flex/grid use
  Taffy's keyword. Opposing positioned insets do not stretch it like auto width.
  Numeric constraints and box edges apply once. `fit-content()` and unrepresented
  sizing/calc values remain unsupported, with bounded diagnostics and fallback.
- `min-width:fit-content` and `max-width:fit-content` constrain ordinary,
  positioned and replaced boxes and flex items using bounded measurements.
  Definite replaced-element height transfers through the natural aspect ratio;
  box edges apply once and the minimum wins over a conflicting maximum.
  Grid items needing an unresolved
  grid-area width remain explicitly rejected, not measured against a guessed
  whole-grid width. The functional `fit-content(...)` form remains unsupported.
- PNG/GIF/JPEG and bounded external/inline SVG use the reviewed Rust decoders.
  Inline SVG preserves computed currentColor/fill/stroke and the actual CSS
  viewport's viewBox aspect behavior. Structured buttons paint their real DOM
  contents; text-only buttons retain the existing native control path.
- A bounded two-stop linear-background slice is implemented. This is not full
  CSS: rounded corners, arbitrary gradients and full SVG remain unsupported.

See [DEPENDENCIES.md](DEPENDENCIES.md) for the pinned source/features/licenses.
Taffy defaults are disabled; only std/flexbox/grid are enabled. The locked graph
uses arrayvec 0.7.8 and smallvec 1.16.0. image 0.25.10 adds only JPEG beside PNG/GIF,
using zune-jpeg 0.5.15 / zune-core 0.5.3. Rust 1.91.1 remains the toolchain.
There is no C/C++ codec/layout fallback, downloaded font backend or new JS path.

## Resource and execution bounds

The loader permits one sheet to consume the existing shared 2 MiB CSS/image
budget, instead of the old 256 KiB partition. Source order, duplicate charging,
16 sheets, 32 requests and the 10-second scheduling deadline remain unchanged.
CSS background-image discovery precedes DOM images and retains the 64-candidate
bound. Style computation separately admits at most 2 MiB combined CSS/media text
and 8 MiB owned snapshot data, including expanded grid vectors.

Stylesheets remain same-origin, including redirects. Images may cross origins
only over verified HTTPS; policy runs before each request/redirect connection.
After the first origin crossing, neither Cookie sending nor Set-Cookie storage
resumes on that image chain, even if it returns to the page origin. URL userinfo
is rejected. Same-origin HTTP images on HTTP pages are allowed, but an image
chain cannot downgrade after HTTPS. No imports, external scripts, downloaded
fonts or data-URL expansion is included. TLS verification is unchanged.

- Layout: 2,000,000 shared work steps, 200,000 scene operations/boxes, depth 256,
  16,384 characters per text node, finite extents bounded to 1,000,000px.
- Formatting: 512 participants per context, 128 tracks per axis, 16 MiB total
  live scratch. Grid placement work is prepaid against both its 1,000,000-step
  preflight bound and the remaining shared budget. There are at most 512
  positioned nodes. Rejected passes discard their partial scene/cache results.
- Content measurements: at most 8,192 entries in each pass-local intrinsic/height
  cache; hits still consume work. No failed measurement is cached as a size.
- Images: 512 KiB source, 2048px maximum side, 1,048,576 decoded/target pixels,
  8 MiB raster-decoder allocation allowance; paint cache 128 entries and 16 MiB.
- Inline SVG: depth 32, 2,048 nodes, 32 KiB per attribute name/value, 512 KiB serialized
  source; pass-local source cache 128 entries and 2 MiB. Only admitted simple
  shapes/metadata are serialized. Embedded images, references, event/style
  attributes, SVG text, file/network resolvers and foreign content are rejected.
- Style diagnostics: 64 entries, 512-byte messages and 256-byte sources, with
  truncation/omission reported. Resource warnings retain 32 entries of 1,024 bytes.

These are admission/cache/work bounds, not hard CPU containment or a sandbox for
the browser. The restricted script-worker boundary and all prior limits remain.

## Current evidence: 2026-09-18

A fresh ordinary HTTP 200 capture, `tmp/news-live.qhKyqo`, contains 1,852,460 HTML
bytes, 942 DOM nodes, seven admitted sheets (1,305,432 CSS bytes), 14 DOM images
and 20 resources (59,187 bytes). Three resource warnings remain: the foreign font
sheet, a data-URL background SVG and one image exceeding 512 KiB. No live scripts
were executed. The capture records resource/source/artifact identities.

Its saved production-resource replay after the fit-content correction,
`tmp/news-replay.55ltWA`, has no layout fallback at 1280x800 or 1024x768, at scroll 0
and 600. All four frames contain 939 boxes; heights are 1838 and 1946 respectively.
Measured render time is 294..329ms, not a guaranteed performance bound. The source
identity remained unchanged during that replay. The fresh response's ten visible
`.vr1PYe` wrappers use `width:fit-content`; that exact conversion loss is fixed.
An unrelated hidden `calc(100% + 4px)` value remains unsupported.

Same-input [HN](HACKER_NEWS.md) 1024/1280 PNGs, complete box/hit logs and 1,211px
content height remain byte-identical to the pre-migration baseline, including
the fit-content slice. Its five public-render tests and adapter test pass;
the focused release run passes 179 tests including existing layout/height/image
assertions. These are local regression results, not final CI or release evidence.

The separate matched-input reference checkpoint uses saved capture
`tmp/news-live.w7wMzV` and `tmp/news-reference.cH2GEM`: captured resources only,
disabled scripts/network, recorded URL transport rewrites and the same DejaVu
regular/bold fonts. Reference height is 1826 versus Mg 1838 at 1280; both are 1946
at 1024. A 15px reference scrollbar gutter explains 7.5px of horizontal centering.
Dimensions alone do not establish fidelity. That pre-correction frame stacked
header cells incorrectly. The final slice adds one anonymous row for pure-cell
containers using the existing table measurement/painter; mixed ordinary/table
content remains explicitly unsupported. Final replay evidence follows separately.
Rounded corners, embedded SVG images, downloaded fonts and literal icon-font
names such as `chevron_right` remain visual gaps. No text-to-icon substitutions
are used. Missing weather pictures and Local News placeholders also occur in
the scripts-disabled reference, and are not invented browser content.

An owned native Xvfb/private-profile checkpoint followed the actual served Top
stories link and pressed Back: all three responses were HTTP 200. The topic and
changed homepage triggered readable-flow fallback for unrepresented layout
values. Fresh homepage replay now passes, but this does not identify every value
in the earlier topic/Back responses. That checkpoint did not satisfy the final
native link/Back gate; the later packaged result is recorded below.
Ignored receipts are local evidence, not distributed fixtures or release assets.

The later native homepage in `tmp/news-native-final.eGUsLQ` has 970 nodes, not the
942-node saved response above. Its additional topic-chip strip uses
`max-width:fit-content`, an independently confirmed conversion loss at node807.
The new constraint implementation covers that case; final fresh verification
must still pass. This illustrates why identical-input replay is not a substitute
for current native behavior. The scratch driver also hit the topic DOM's 4 MiB
CDP response bound; requesting depth3 fixes evidence collection without changing
the browser's limit. The same chip strip's flex child uses overflow-x:scroll;
static initial-offset clipping is now consistent with ordinary block flow, while
the unsupported-scrolling diagnostic remains. This does not enable chip-strip
scroll controls or other page interactions. The only remaining visible snapshot
loss was its child node813's `min-width:fit-content`, now implemented too. The
exact 970-node native DOM then renders without fallback in a 227ms release
diagnostic replay (933 boxes, 114 hits, height1766), without fetched assets. This
is not a replacement for final fresh native screenshots. Hidden node866's mixed
calculation remains unsupported. HN release/debug PNGs and complete geometry/hit
logs still match byte-for-byte. Fourteen fit-sizing and eight boundary tests
pass release, including the independently reproduced SVG ratio correction.

Final packaged v0.9.0 native checkpoint: `tmp/news-native-candidate.mk3Nzp`.
The fresh signed-out homepage, its actual Top stories destination and Back all
returned HTTP200. Home and Back rendered styled without fallback; page scrolling
and the actual anchor's CDP mouse coordinates aligned, and Back used native X11
toolbar input. Root inspected home, offset600 and Back screenshots. The topic
destination still falls back at node335 for an unrepresented positioned value;
that is a documented destination limit, not a styled-topic success. The complete
archive is8,275,877 bytes, below the unchanged8MiB updater ceiling. Final public
tagged bytes and installer still require separate verification.

Public v0.9.0 receipt: tag commit `7b5fecac59c167442a330e2a4822b153073fc69b`,
Rust35326867514, JSPLAN35326867508, Pages35326867490 and release35328880780
all pass. The public archive is8,271,015 bytes; SHA-256 is
`7769562d7f0943ccf7beec0deea647f2a02ac45e9412998260f949f8dc33962f`.
The exact website command installs/reinstalls that release, including verified
version/commit, worker/session/Boa execution, desktop/icons and Apache NOTICE.

The fresh public-installed native journey `tmp/news-public-native.wjEpO0`
passes home, its actual Top stories link and native toolbar Back, all HTTP200.
Home and Back stay styled without fallback; root inspected home, scroll600 and
Back frames. Actual current stories/photos differ from the saved captures.
The topic still falls back at positioned node335: this is not styled-topic or
full News acceptance. Scripts remain disabled, with a private profile and owned
Xvfb. CDP captures show page pixels; link/scroll use real public CDP input and
Back uses native X11 input. This is not an ordinary Playwright journey.

## Historical baseline and migration evidence

The 2026-09-17 live baseline was HTTP 200, 1,849,110 bytes, 31 links, six admitted
sheets and 22 warnings (`tmp/desktop-google-news-live.log`/`.png`). Its first
screen overlapped and lacked thumbnails. A 1,227,493-byte main sheet exceeded the
old 256 KiB per-sheet allowance; a same-origin image redirected to foreign JPEG,
which the old policy/codec set rejected. These blockers are now implemented,
not outstanding dependency proposals. Response counts vary with real news.

An earlier scratch-only CSS admission bypass, without fetched assets/scripts,
reduced height from 9,522 to 4,406px after the percentage-height fix but still failed
reading acceptance. The isolated `tmp/taffy-adapter-probe.T8K6A5/` validated
measured flex and 12-column grid mechanics, cache invalidation and allocation
costs. Its synthetic text and smallvec 1.16.1 were prototype inputs, not production
compatibility evidence. Production uses real Mg measurement and the locked graph.

## Remaining acceptance and stop condition

Matched-input first-screen/scrolled comparison and fresh packaged home/link/Back
checks now pass their narrow reading/navigation scope. Preserve HN identity and
existing style/image/table/height tests, dependency/component and embedding
guards, normal CI and packaged/public installer acceptance. F-025 stays
in-progress: fonts/icons/corners, live response variation, topic fallback and
interactive behavior remain open. Publication does not remove those limits.

Static reading does not complete Playwright, JSPLAN, Google search or the formal
Linux MVP. External scripts, fetch/XHR, timers, accounts and full interactive News
remain later gates. Stop this milestone at faithful signed-out reading.
