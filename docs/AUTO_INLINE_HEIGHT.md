# Auto-width atomic inline height parity

Frozen 2026-10-23 before new test execution or implementation. Baseline main:
`be71e0c735e64ac5f430fd77a0d1e3f60828ac4e`, following the fully delivered
v0.9.18 and its documentation closeout. All eight exact-source CI/hosting checks
and twelve-path public byte/report verification pass; closeout receipt SHA256
`53b487684b19a7ae733107be587e6f7ace212af5e24d7e16fa5ac196b42233e7`.
The preceding goal turn published and verified that release, not merely a plan.
The broader feature goal remains active and its original criteria stay open.

## Evidence and scope

The complete retained static report has SHA256
`b7590c1f7fe8aacceb0fb9f21fa82721972c6515117877070baf701343aa53a1`.
Its `negative-margins-001.html` case remains FAIL with180 differing pixels:
test and reference have the same400 green pixels, but the enclosing parent
border differs. Root separately viewed both actual retained PNGs. That alone
does not guarantee a new pass or prove general negative-margin correctness.

Source review finds ordinary auto-width atomic line metrics falling back to
`replaced_size`, while actual emission calls real `block_layout`. Under inherited
line-height0, measurement can report0 while a descendant paints10px high.
Baseline styled_layout.rs SHA256:
`4d39099c7c83dc309e52086bd0dd518353d290bd35f69c7496d9e1074a08421b`.

Restrict this correction to nonreplaced, non-structured-button boxes with
computed display InlineBlock/InlineFlex/InlineGrid, actual width Auto, and neither
width_fit_content nor width_max_content set. Inline tables, other width keywords,
definite-width boxes and replaced/button sizing keep their existing routes.
Intrinsic min/max-width flags remain existing behavior, not new exclusions.

Share the existing final auto-width layout boundary between measurement and
emission. Preserve the old emission recipe: `w=intrinsic.max.min(width)`, then
`block_layout(available=w, forced=Some(w))` with the same containing-height Option.
Do not replace available=w with the original containing width or use this
helper's returned outer width to change inline_box_width/wrapping admission.
Keep the effective recursion depth: line depth L enters block_layout at L+1.
Pure measurement uses EMIT=false. Its nonnegative outer height contributes once
to the line, subject to the existing parent strut and bottom-alignment behavior.
Do not turn measured auto height into a definite percentage-height basis.

No new shrink-to-fit, baseline, vertical alignment, float, table, box-sizing,
raster, clipping, hit-rounding or CSS grammar algorithm. No test/site/reference
special case, dependency change, worker capability or resource allowance.
Preserve the2,000,000 work-step allowance, separate200,000 scene-operation cap,
depth256 and all scratch/cache/input/process guards. These are not a whole-browser
memory or CPU quota. Extra measurement spends real existing work; failures and
exhaustion cannot masquerade as successful zero-height layout.

Primary sources reviewed2026-10-23:
[CSS2.2 auto height](https://www.w3.org/TR/CSS22/visudet.html#complicated-cases),
[line-box height](https://www.w3.org/TR/CSS22/visudet.html#line-height), and
[flex containers](https://www.w3.org/TR/css-flexbox-1/#flex-containers).
Ordinary inline-block line contribution includes its margin box, auto height
depends on in-flow descendants, and inline-flex participates as an inline-level
box with its own formatting context. Full baseline/vertical alignment is separate.

## Authored matrix before execution

Use actual computed Stylo snapshots and public render_scaled geometry/pixel/hit
routes, not synthetic styles or copied upstream bodies. Default html/body
margins0,font-size1,line-height0, no inter-atomic markup whitespace. Private
tests reuse unchanged SavedScene/populate_scene/measurement helpers, not old
assertion wrappers that assume definite width, height10 or equal work counts.
The normal measurement helper and production renderer both initialize2,000,000
work steps; MAX_OPS is a scene cap. Selected lower-budget controls are supplemental.

Run the ordinary inline-block, inline-flex row, inline-flex column and inline-grid
contexts, with these independent arithmetic expectations:

- In-flow child width30,height12 gives a12px line contribution. Enclosing borders
  and following block sentinels expose the real parent advance.
- Vertical padding2/4,borders1/1,margins3/5 give28; count every edge once.
  Horizontal edges remain0 in this arithmetic control.
- Explicit height7 with an overflowing12px child contributes7, not its painted
  overflow envelope. Separate min-height18 gives18, max-height8 gives8; combined
  min18/max8 gives18.
- Parent strut20 keeps metrics at(20,1) for font-size1. Existing bottom alignment
  shifts a12px outer box by8; do not assert a new top/baseline alignment.
- Empty and absolute-child-only boxes have0 content height; their specified
  margins/borders/padding still contribute normally.
- Content12 with margins-top/bottom -9/-8 returns signed outer height-5; line
  contribution0, border origin9 above the line. Use a positive line origin to
  distinguish geometry from viewport clipping. Strut20 preserves the existing
 20px alignment shift after nonnegative outer-height clamping.
- Ordinary inline-block A with width:auto,height50% and a12px child gives12
  with containing_height=None versus20 with Some40. Separately auto-height A,
  including min-height, must not make a percentage descendant definite. Do not
  invent new flex/grid percentage-resolution expectations.
- Two30px siblings at containing width60 exact-fit; width59 wraps into two12px
  lines. Preserve chosen widths, horizontal positions and cursor advance.
- Nested boxes; cold and populated caches; isolated line_metrics and full flow.
  Compare measured height with emitted allocations, without demanding equal
  Taffy operation counts. Measurement must preserve all nine Scene collections,
  metadata/maps, string capacities, clips and capacities with no leaked paint.
- Keep near-boundary recursion, exhausted work/depth and truthful failure controls.
  Existing definite/fit/max-content, structured-button, image/SVG and table
  assertions remain unchanged regression gates.

Public controls use literal geometry, solid pixels and topmost hit behavior at
scales1/2, with empty diagnostics and zero omissions. Empty render-only boxes
may be spans; use ordinary text-bearing links where link activation is required.
No baseline, float or general intrinsic-width conformance is inferred.

Read and freeze new test literals before running the unchanged engine. Retain
actual assertion failures separately from infrastructure failures. If a literal
premise exposes another width/baseline/positioning dependency, review it before
implementation rather than updating expected values from candidate output.

## Acceptance and delivery

Preserve all338 passing and949 runnable static cases, original24 controls and
fifteen passing Window files. Corpus/runner/policy/manual protections stay fixed.
Focused/full debug and release Rust, independent no-chrome embedding, dependency/
component/tooling and all364 frozen JSPLAN outcomes remain required. Run both
complete unchanged WPT lanes before each commit, inspect retained full results,
resources/diagnostics/pixels/cleanup, and manually ratchet only proven new passes.
Authored controls add no upstream numerator or denominator.

Repeat normal package/install/updater checks, thirteen existing native helpers,
all six current additive native/CDP proofs and both frozen worker suites. Add a
separate owned installed native/CDP proof of the new visible parent/sentinel
behavior, preserving older helper/source pins. Inspect its actual screenshots,
requested URLs, input/lifecycle/accounting and cleanup; native handler events
are not independent physical input or ordinary Playwright compatibility.

Ship the next normal versioned release with locked Rust1.91.1/default binary,
exact-commit CI/site, <=8MiB archive/checksum, exact public curl install/reinstall,
actual old v0.9.18 update/no-op and public repeats. Keep old tags immutable and
the website scores current. Full F-003/F-005/F-016/F-026, general inline formatting,
WebDriver/Playwright and the formal MVP remain unfinished. No guaranteed WPT gain.
