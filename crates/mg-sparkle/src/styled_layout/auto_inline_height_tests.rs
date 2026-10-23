// Additive private controls for docs/AUTO_INLINE_HEIGHT.md, authored before RED.
// Nested inside atomic_inline_tests solely to reuse its unchanged real-Stylo,
// SavedScene, populate_scene and measurement helpers. No upstream fixture bytes.

const AUTO_FORMATS: [(&str, Display); 4] = [
    ("display:inline-block", Display::InlineBlock),
    (
        "display:inline-flex;flex-direction:row",
        Display::InlineFlex,
    ),
    (
        "display:inline-flex;flex-direction:column",
        Display::InlineFlex,
    ),
    ("display:inline-grid", Display::InlineGrid),
];
const AUTO_CHILD: &str = "<span id=a><span id=child></span></span>";
const AUTO_EMPTY: &str = "<span id=a></span>";
const AUTO_PAIR: &str =
    "<span id=a><span id=child></span></span><span id=b><span id=other></span></span>";

fn auto_fixture(
    format: (&str, Display),
    width: f32,
    rules: &str,
    body: &str,
) -> (Document, Vec<ComputedStyle>) {
    fixture(
        width,
        &format!(
            "#line,#a,#b,#nested,#child,#other,#percent,#grand,#after{{line-height:inherit}}\
             #a,#b,#nested{{{};width:auto;height:auto;background:#aa0000}}\
             #child,#other,#grand{{display:block;width:30px;height:12px;flex:none}}\
             #after{{display:block;width:8px;height:3px;background:#00aa00}}{rules}",
            format.0
        ),
        body,
    )
}

fn assert_auto_premise(layout: &Layout<'_, '_>, a: usize, format: (&str, Display)) {
    assert_eq!(layout.styles[a].display, format.1);
    assert_eq!(layout.styles[a].width, Length::Auto);
    assert!(!layout.styles[a].layout.width_fit_content);
    assert!(!layout.styles[a].layout.width_max_content);
    assert!(!layout.is_replaced(a));
    assert!(!layout.is_structured_button(a));
}

fn assert_auto_clean(layout: &Layout<'_, '_>, initial_budget: usize) {
    assert!(!layout.exhausted && layout.failure.is_none());
    assert!(layout.budget > 0 && layout.budget < initial_budget);
    assert_eq!(layout.format_scratch, 0);
    assert!(layout.diagnostics.entries.is_empty());
    assert_eq!(layout.diagnostics.omitted, 0);
}

// The following block is deliberately outside the atomic line. Its origin and
// the enclosing border expose line advancement without a taller inline sentinel
// concealing a missing atomic contribution. Coordinates are literal CSS pixels.
fn assert_auto_flow(
    format: (&str, Display),
    width: f32,
    rules: &str,
    body: &str,
    metrics: (f32, f32),
    line_height: f32,
    advances: &[(&str, f32)],
    expected: &[(&str, Geometry)],
) {
    let body = format!("{body}<div id=after></div>");
    let (document, computed) = auto_fixture(format, width, rules, &body);
    let line = node(&document, "#line");
    let a = node(&document, "#a");
    let controls = Controls::default();
    let mut measured_fonts = fonts();
    let mut measured =
        measurement_layout(&document, &mut measured_fonts, &controls, computed.clone());
    let mut painted_fonts = fonts();
    let mut painted =
        measurement_layout(&document, &mut painted_fonts, &controls, computed.clone());
    let mut cold_fonts = fonts();
    let mut cold_flow = measurement_layout(&document, &mut cold_fonts, &controls, computed);
    let initial_budgets = [measured.budget, painted.budget, cold_flow.budget];
    for layout in [&measured, &painted, &cold_flow] {
        // The unchanged helper initializes2M work, then runs real replaced/
        // anonymous-table discovery. Preserve that preparation's charge and
        // compare subsequent layout work against its actual starting balance.
        // MAX_OPS remains a separate scene-operation cap.
        assert!(layout.budget > 0 && layout.budget < 2_000_000);
        assert_auto_premise(layout, a, format);
        assert_eq!(
            layout.styles[a].line_height,
            layout.styles[line].line_height
        );
        assert!(layout.intrinsic.is_empty());
    }

    let empty = SavedScene::capture(&measured.scene);
    assert_eq!(
        measured.line_metrics(line, width, None, &[Token::Box(a)], 0),
        metrics,
        "{}: isolated cold metrics",
        format.0
    );
    assert_eq!(SavedScene::capture(&measured.scene), empty);
    // This second cold layout reaches metrics through real tokenization/flow,
    // without first warming its intrinsic cache through an admission query.
    let cold_empty = SavedScene::capture(&cold_flow.scene);
    let tokens = row_tokens(&mut cold_flow, line);
    assert_eq!(
        cold_flow.flow_tokens::<false>(line, 20.0, 40.0, width, None, tokens, 0),
        line_height + 3.0
    );
    assert_eq!(SavedScene::capture(&cold_flow.scene), cold_empty);
    assert_no_scene_allocation(&cold_flow);
    assert_eq!(cold_flow.scene.static_positions.capacity(), 0);

    // Real enclosing block emission: no manually invented parent border.
    assert_eq!(
        painted.block_layout::<true>(line, 20.0, 40.0, width, None, Some(width), 0),
        (width, line_height + 3.0)
    );
    assert_eq!(
        border(&painted.scene, line),
        (20.0, 40.0, width, line_height + 3.0)
    );
    assert_eq!(
        border(&painted.scene, node(&document, "#after")),
        (20.0, 40.0 + line_height, 8.0, 3.0)
    );
    for (selector, expected) in expected {
        assert_eq!(
            border(&painted.scene, node(&document, selector)),
            *expected,
            "{}: {selector} literal border",
            format.0
        );
    }
    for (selector, advance) in advances {
        let id = node(&document, selector);
        assert_eq!(measured.inline_box_width(id, width, None, 0), *advance);
        assert_eq!(painted.inline_box_width(id, width, None, 0), *advance);
    }
    for (layout, initial) in [&measured, &painted, &cold_flow]
        .into_iter()
        .zip(initial_budgets)
    {
        assert_auto_clean(layout, initial);
    }

    // All nine Scene collections, optional metadata, clips and retained string
    // capacities must survive false passes with both warm caches and real paint.
    populate_scene(&mut painted.scene, line);
    let populated = SavedScene::capture(&painted.scene);
    for _ in 0..2 {
        assert_eq!(
            painted.line_metrics(line, width, None, &[Token::Box(a)], 0),
            metrics
        );
        let tokens = row_tokens(&mut painted, line);
        assert_eq!(
            painted.flow_tokens::<false>(line, 20.0, 40.0, width, None, tokens, 0),
            line_height + 3.0
        );
        assert_eq!(SavedScene::capture(&painted.scene), populated);
        assert_auto_clean(&painted, initial_budgets[1]);
    }
}

#[test]
fn auto_inline_fixed_child_supplies_its_own_line_height() {
    for format in AUTO_FORMATS {
        assert_auto_flow(
            format,
            100.0,
            "",
            AUTO_CHILD,
            (12.0, 0.0),
            12.0,
            &[("#a", 30.0)],
            &[
                ("#a", (20.0, 40.0, 30.0, 12.0)),
                ("#child", (20.0, 40.0, 30.0, 12.0)),
            ],
        );
    }
}

#[test]
fn auto_inline_vertical_edges_and_margins_are_counted_once() {
    for format in AUTO_FORMATS {
        assert_auto_flow(
            format,
            100.0,
            "#a{padding-top:2px;padding-bottom:4px;border-top:1px solid black;border-bottom:1px solid black;margin-top:3px;margin-bottom:5px}",
            AUTO_CHILD,
            (28.0, 0.0),
            28.0,
            &[("#a", 30.0)],
            &[
                ("#a", (20.0, 43.0, 30.0, 20.0)),
                ("#child", (20.0, 46.0, 30.0, 12.0)),
            ],
        );
    }
}

#[test]
fn auto_inline_explicit_and_min_max_heights_do_not_measure_overflow() {
    for format in AUTO_FORMATS {
        for (rules, height) in [
            ("#a{height:7px}", 7.0),
            ("#a{min-height:18px}", 18.0),
            ("#a{max-height:8px}", 8.0),
            ("#a{min-height:18px;max-height:8px}", 18.0),
        ] {
            assert_auto_flow(
                format,
                100.0,
                rules,
                AUTO_CHILD,
                (height, 0.0),
                height,
                &[("#a", 30.0)],
                &[
                    ("#a", (20.0, 40.0, 30.0, height)),
                    ("#child", (20.0, 40.0, 30.0, 12.0)),
                ],
            );
        }
    }
}

#[test]
fn auto_inline_parent_strut_retains_existing_bottom_alignment() {
    for format in AUTO_FORMATS {
        assert_auto_flow(
            format,
            100.0,
            "#line{line-height:20px}",
            AUTO_CHILD,
            (20.0, 1.0),
            20.0,
            &[("#a", 30.0)],
            &[
                ("#a", (20.0, 48.0, 30.0, 12.0)),
                ("#child", (20.0, 48.0, 30.0, 12.0)),
            ],
        );
    }
}

#[test]
fn auto_inline_empty_and_absolute_only_content_have_no_flow_height() {
    for format in AUTO_FORMATS {
        for (body, absolute) in [(AUTO_EMPTY, ""), (AUTO_CHILD, "#child{position:absolute}")] {
            for edges in [false, true] {
                let rules = format!(
                    "{absolute}{}",
                    if edges {
                        "#a{padding-top:2px;padding-bottom:4px;border-top:1px solid black;border-bottom:1px solid black;margin-top:3px;margin-bottom:5px}"
                    } else {
                        ""
                    }
                );
                let (outer, y, border_height) = if edges {
                    (16.0, 43.0, 8.0)
                } else {
                    (0.0, 40.0, 0.0)
                };
                assert_auto_flow(
                    format,
                    100.0,
                    &rules,
                    body,
                    (outer, 0.0),
                    outer,
                    &[("#a", 0.0)],
                    &[("#a", (20.0, y, 0.0, border_height))],
                );
            }
        }
    }
}

#[test]
fn auto_inline_signed_outer_height_is_clamped_only_for_line_contribution() {
    for format in AUTO_FORMATS {
        let (document, computed) = auto_fixture(
            format,
            100.0,
            "#a{margin-top:-9px;margin-bottom:-8px}",
            AUTO_CHILD,
        );
        let a = node(&document, "#a");
        let controls = Controls::default();
        let mut font = fonts();
        let mut layout = measurement_layout(&document, &mut font, &controls, computed);
        let empty = SavedScene::capture(&layout.scene);
        assert_eq!(
            layout.block_layout::<false>(a, 20.0, 40.0, 30.0, None, Some(30.0), 1),
            (30.0, -5.0),
            "the block boundary retains its signed outer height"
        );
        assert_eq!(SavedScene::capture(&layout.scene), empty);
        assert_auto_clean(&layout, 2_000_000);
        for (strut, height, ascent, y) in [(0.0, 0.0, 0.0, 31.0), (20.0, 20.0, 1.0, 51.0)] {
            assert_auto_flow(
                format,
                100.0,
                &format!("#line{{line-height:{strut}px}}#a{{margin-top:-9px;margin-bottom:-8px}}"),
                AUTO_CHILD,
                (height, ascent),
                height,
                &[("#a", 30.0)],
                &[
                    ("#a", (20.0, y, 30.0, 12.0)),
                    ("#child", (20.0, y, 30.0, 12.0)),
                ],
            );
        }
    }
}

#[test]
fn auto_inline_exact_fit_and_wrap_keep_existing_width_admission() {
    for format in AUTO_FORMATS {
        for (width, height, b) in [
            (60.0, 12.0, (50.0, 40.0, 30.0, 12.0)),
            (59.0, 24.0, (20.0, 52.0, 30.0, 12.0)),
        ] {
            assert_auto_flow(
                format,
                width,
                "",
                AUTO_PAIR,
                (12.0, 0.0),
                height,
                &[("#a", 30.0), ("#b", 30.0)],
                &[("#a", (20.0, 40.0, 30.0, 12.0)), ("#b", b)],
            );
        }
    }
}

#[test]
fn auto_inline_nested_atomic_boxes_share_the_real_child_height() {
    for format in AUTO_FORMATS {
        assert_auto_flow(
            format,
            100.0,
            "",
            "<span id=a><span id=nested><span id=child></span></span></span>",
            (12.0, 0.0),
            12.0,
            &[("#a", 30.0), ("#nested", 30.0)],
            &[
                ("#a", (20.0, 40.0, 30.0, 12.0)),
                ("#nested", (20.0, 40.0, 30.0, 12.0)),
                ("#child", (20.0, 40.0, 30.0, 12.0)),
            ],
        );
    }
}

#[test]
fn auto_inline_percentage_vertical_edges_keep_the_old_chosen_width_basis() {
    // Old emission chooses w30, then available=w30 and forced=Some(w30).
    // Padding-top10% is3, not10 from the original100px containing line.
    assert_auto_flow(
        AUTO_FORMATS[0],
        100.0,
        "#a{padding-top:10%}",
        AUTO_CHILD,
        (15.0, 0.0),
        15.0,
        &[("#a", 30.0)],
        &[
            ("#a", (20.0, 40.0, 30.0, 15.0)),
            ("#child", (20.0, 43.0, 30.0, 12.0)),
        ],
    );
}

#[test]
fn auto_inline_percentage_height_preserves_the_exact_optional_basis() {
    let (document, computed) = auto_fixture(AUTO_FORMATS[0], 100.0, "#a{height:50%}", AUTO_CHILD);
    let line = node(&document, "#line");
    let a = node(&document, "#a");
    let controls = Controls::default();
    for (basis, height) in [(None, 12.0), (Some(40.0), 20.0)] {
        let mut measured_fonts = fonts();
        let mut measured =
            measurement_layout(&document, &mut measured_fonts, &controls, computed.clone());
        let mut painted_fonts = fonts();
        let mut painted =
            measurement_layout(&document, &mut painted_fonts, &controls, computed.clone());
        assert_auto_premise(&measured, a, AUTO_FORMATS[0]);
        let cold = SavedScene::capture(&measured.scene);
        assert_eq!(
            measured.line_metrics(line, 100.0, basis, &[Token::Box(a)], 0),
            (height, 0.0)
        );
        let tokens = row_tokens(&mut measured, line);
        assert_eq!(
            measured.flow_tokens::<false>(line, 20.0, 40.0, 100.0, basis, tokens, 0),
            height
        );
        assert_eq!(SavedScene::capture(&measured.scene), cold);
        let tokens = row_tokens(&mut painted, line);
        assert_eq!(
            painted.flow_tokens::<true>(line, 20.0, 40.0, 100.0, basis, tokens, 0),
            height
        );
        assert_eq!(border(&painted.scene, a), (20.0, 40.0, 30.0, height));
        assert_eq!(
            border(&painted.scene, node(&document, "#child")),
            (20.0, 40.0, 30.0, 12.0)
        );
        assert_auto_clean(&measured, 2_000_000);
        assert_auto_clean(&painted, 2_000_000);
    }
}

#[test]
fn auto_inline_auto_height_and_minimum_do_not_make_a_child_percentage_definite() {
    for (minimum, height) in [(0.0, 12.0), (18.0, 18.0)] {
        let (document, computed) = auto_fixture(
            AUTO_FORMATS[0],
            100.0,
            &format!("#a{{min-height:{minimum}px}}#percent{{display:block;width:30px;height:50%}}"),
            "<span id=a><span id=percent><span id=grand></span></span></span>",
        );
        let line = node(&document, "#line");
        let a = node(&document, "#a");
        let controls = Controls::default();
        for basis in [None, Some(40.0)] {
            let mut measured_fonts = fonts();
            let mut measured =
                measurement_layout(&document, &mut measured_fonts, &controls, computed.clone());
            let mut painted_fonts = fonts();
            let mut painted =
                measurement_layout(&document, &mut painted_fonts, &controls, computed.clone());
            let empty = SavedScene::capture(&measured.scene);
            assert_eq!(measured.styles[a].height, Length::Auto);
            assert_eq!(
                measured.line_metrics(line, 100.0, basis, &[Token::Box(a)], 0),
                (height, 0.0)
            );
            assert_eq!(SavedScene::capture(&measured.scene), empty);
            let tokens = row_tokens(&mut painted, line);
            assert_eq!(
                painted.flow_tokens::<true>(line, 20.0, 40.0, 100.0, basis, tokens, 0),
                height
            );
            assert_eq!(border(&painted.scene, a), (20.0, 40.0, 30.0, height));
            assert_eq!(
                border(&painted.scene, node(&document, "#percent")),
                (20.0, 40.0, 30.0, 12.0)
            );
            assert_eq!(
                border(&painted.scene, node(&document, "#grand")),
                (20.0, 40.0, 30.0, 12.0)
            );
            assert_auto_clean(&measured, 2_000_000);
            assert_auto_clean(&painted, 2_000_000);
        }
    }
}

#[test]
fn auto_inline_cold_and_warm_depth_boundary_keeps_the_existing_call_depth() {
    // Direct line depth L reaches A's block at L+1, its child token/block at
    // L+3. The leaf has no children. L253 therefore reaches, but does not exceed,
    // depth256; L254 must fail. No Taffy recursion threshold is inferred here.
    let (document, computed) = auto_fixture(AUTO_FORMATS[0], 100.0, "", AUTO_CHILD);
    let line = node(&document, "#line");
    let a = node(&document, "#a");
    let controls = Controls::default();
    assert_eq!(MAX_DEPTH, 256);
    for warm in [false, true] {
        for (depth, accepted) in [(253, true), (254, false)] {
            for emit in [false, true] {
                let mut font = fonts();
                let mut layout =
                    measurement_layout(&document, &mut font, &controls, computed.clone());
                if warm {
                    assert_eq!(layout.inline_box_width(a, 100.0, None, 0), 30.0);
                    assert!(!layout.intrinsic.is_empty());
                }
                populate_scene(&mut layout.scene, line);
                let saved = SavedScene::capture(&layout.scene);
                let height = if emit {
                    layout.line::<true>(
                        line,
                        20.0,
                        40.0,
                        100.0,
                        None,
                        &[Token::Box(a)],
                        30.0,
                        depth,
                    )
                } else {
                    let (height, ascent) =
                        layout.line_metrics(line, 100.0, None, &[Token::Box(a)], depth);
                    assert_eq!(ascent, 0.0);
                    height
                };
                if accepted {
                    assert_eq!(height, 12.0);
                    assert_auto_clean(&layout, 2_000_000);
                    if emit {
                        assert_eq!(border(&layout.scene, a), (20.0, 40.0, 30.0, 12.0));
                    }
                } else {
                    assert!(
                        layout.exhausted,
                        "depth exhaustion must invalidate the entire pass"
                    );
                    assert!(
                        layout
                            .measure_outer(a, 30.0, None, Some(30.0), depth)
                            .is_none()
                    );
                }
                if !emit {
                    assert_eq!(SavedScene::capture(&layout.scene), saved);
                }
                assert_eq!(layout.format_scratch, 0);
            }
        }
    }
}

#[test]
fn auto_inline_exhausted_measurement_is_not_a_successful_zero_height() {
    for format in AUTO_FORMATS {
        let (document, computed) = auto_fixture(format, 100.0, "", AUTO_CHILD);
        let line = node(&document, "#line");
        let a = node(&document, "#a");
        let controls = Controls::default();
        for (budget, depth) in [(0, 0), (2_000_000, MAX_DEPTH + 1)] {
            let mut font = fonts();
            let mut layout = measurement_layout(&document, &mut font, &controls, computed.clone());
            layout.budget = budget;
            populate_scene(&mut layout.scene, line);
            let saved = SavedScene::capture(&layout.scene);
            let _ = layout.line_metrics(line, 100.0, None, &[Token::Box(a)], depth);
            assert!(
                layout.exhausted,
                "zero-height output cannot erase a failed measurement"
            );
            assert_eq!(SavedScene::capture(&layout.scene), saved);
            assert_eq!(layout.format_scratch, 0);
            assert!(layout.content_height_cache.is_empty());
            assert!(layout.content_intrinsic_cache.is_empty());
            assert!(
                layout
                    .measure_outer(a, 30.0, None, Some(30.0), depth)
                    .is_none()
            );
        }
    }
}

#[test]
fn auto_inline_small_fixture_also_fits_a_supplemental_lower_work_allowance() {
    // Supplemental only: production and measurement_layout initialize2M work.
    // This200k budget is not a renamed scene cap or a new production allowance.
    for format in AUTO_FORMATS {
        let (document, computed) = auto_fixture(format, 100.0, "", AUTO_CHILD);
        let line = node(&document, "#line");
        let a = node(&document, "#a");
        let controls = Controls::default();
        let mut font = fonts();
        let mut layout = measurement_layout(&document, &mut font, &controls, computed);
        layout.budget = 200_000;
        let saved = SavedScene::capture(&layout.scene);
        assert_eq!(
            layout.line_metrics(line, 100.0, None, &[Token::Box(a)], 0),
            (12.0, 0.0)
        );
        assert_eq!(SavedScene::capture(&layout.scene), saved);
        assert_auto_clean(&layout, 200_000);
    }
}
