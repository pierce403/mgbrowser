# Physical content justification: next bounded increment

2026-10-23: pre-implementation contract. This is planned behavior, not a shipped
feature or a measured WPT gain. v0.9.14 delivery is complete; retain that release,
both fixed score lanes and every existing resource/security assertion.

## Existing failure and source evidence

The complete accepted static report at v0.9.14 retains seven FAIL cases with
only the content-alignment fallback recorded on their test pages:

- `flexbox-justify-content-horiz-001a.xhtml`
- `flexbox-justify-content-horiz-001b.xhtml`
- `flexbox-justify-content-horiz-005.xhtml`
- `flexbox_justifycontent-left-001.html` and `-left-002.html`
- `flexbox_justifycontent-right-001.html` and `-right-002.html`

All are under the unchanged `css/css-flexbox/` selection. Their original
references have no diagnostics/omissions and content heights of 340, 516, 297 or
489 px within the existing 600 px viewport. Readable-flow fallback is not correct
alignment. Removing that fallback alone does not establish pixel equality or
promise seven passes: other geometry differences can remain.
Source-bound local evidence: `tmp/oct23-v0914-copy-static-a/report.json`, SHA-256
`f320628606e3c547469f44c85737cf963beec8c8ce902a499a88ff283dfe5b14`.
This ignored artifact is a receipt identifier, not a public download URL.

Stylo already parses and computes these keywords. Mg preserves `Left`/`Right`
in `style.rs::alignment`, but `styled_layout/formatting_style.rs` rejects them
in the context-free content-alignment mapper. Both validation and actual trait
conversion must use the same context-aware resolution. No parser, dependency,
CSSOM, worker, protocol, input selection or scoring change is needed.

## Behavior boundary

For the existing represented horizontal-tb/LTR layout profile, use these
logical Taffy keywords while retaining normal item order and reversal:

| Actual container | `left` | `right` |
| --- | --- | --- |
| Flex row or row-reverse | Start | End |
| Flex column or column-reverse | Start | Start |
| Grid inline axis | Start | End |

Physical positioning must not become FlexStart/FlexEnd merely because the
children reverse. Grid must ignore an irrelevant authored `flex-direction`:
its current trait route delegates to the flex getter. Do not remap computed
CSS or introduce a second child reorder. Block/table properties with no
implemented content-justification context must remain inert, not acquire a new
formatting algorithm. Anonymous item views remain non-formatting.

Preserve Default/Unsafe translation and existing auto-margin, shrinking,
wrapping, gap, padding/border and clipping behavior. Test negative free space;
do not clamp it to manufacture alignment. Preserve Safe on ordinary row/column
and grid. For new physical keywords in reversed flex contexts, explicitly
reject Safe before layout until its overflow fallback can be represented
correctly: the locked Taffy fallback selects logical Start, whereas the current
specification requires flex-start. Do not silently claim those are equivalent.
Keep existing nonphysical alignment behavior unchanged in this increment.

Retain current RTL/writing-mode diagnostics, unsupported/legacy/wrong-property
keyword rejection, all layout-work/scratch/scene/viewport caps and no-chrome
embedding. This is not full bidi, writing modes, self-alignment, element scrolling
or general Box Alignment conformance. Do not weaken an unrelated diagnostic.

## Frozen acceptance before changing behavior

1. Add authored adapter and rendered-geometry controls; record their actual
   pre-feature failure. Cover both keywords in all four flex directions,
   inline-flex, grid and inline-grid, including grid with irrelevant column/
   column-reverse declarations. Preserve source order and distinguish group
   placement from reversed child order using unequal, separately colored items.
2. Cover positive/zero/negative free space, one/multiple items, explicit Safe/
   Unsafe and Default, nonshrinking overflow, auto-margin precedence, wrapped
   lines/wrap-reverse and bounded padding/border/gaps. Safe reverse refusal must
   retain a diagnostic, not silently use a different keyword or pass fallback.
   Keep existing Start/End/FlexStart/FlexEnd, item/cross-axis and RTL/writing-mode
   negatives. Replace only an obsolete Left/Right refusal assertion with its
   real new positive behavior; preserve all unrelated negative controls.
3. Run focused and full debug/release Rust, independent embedding, dependency/
   component and frozen JSPLAN gates. Measure both complete unchanged WPT lanes,
   review actual changed pixels/relations/diagnostics/requests/resources and
   strengthen the manual ratchet only for independently verified new passes.
   Preserve all 317 static passes/949 runnable cases, original 24 and fifteen
   Window passes; keep the two genuine container-unit assertions visible.
4. Finish the next versioned release: normal locked binary, exact-commit CI/
   site, <=8 MiB archive, published checksum, exact public install/reinstall,
   actual previous-version update/no-op, existing worker/native/CDP acceptance
   and an installed-binary proof of this alignment behavior. A source push or
   headless score alone is not delivery. F-005/F-016/F-026/formal MVP remain open.

Primary specification review: CSS Box Alignment
[8 October 2026 draft, positional alignment](https://www.w3.org/TR/2026/WD-css-align-3-20261008/#positional-values)
and [overflow alignment](https://www.w3.org/TR/2026/WD-css-align-3-20261008/#overflow-values).
The [21 October editor's draft](https://drafts.csswg.org/css-align-3/#overflow-values)
uses flow-start for the same flex overflow fallback. These are drafts, not a
claim of full conformance. Locked Taffy 0.14.0 `compute/common/alignment.rs`
supplies the actual Start/End/reverse/safety behavior under review.

The separate container-unit candidate is deferred, not solved: the existing
Stylo Servo grammar rejects all six units; verified upstream commit
[`bd4e31763a4991a7b11768650cca4b8febb389bb`](https://github.com/servo/stylo/blob/bd4e31763a4991a7b11768650cca4b8febb389bb/style/values/specified/length.rs#L189-L196)
retains that gate. Published-crate
metadata was unavailable (HTTP403), and Mg lacks actual container-size lookup.
Do not enable Gecko, mutate the Cargo registry, replace units with pixels or
adopt an unreviewed dependency source to bypass those gaps.
