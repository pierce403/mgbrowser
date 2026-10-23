// Separate authored addendum controls: original atomic_inline_tests.rs stays frozen.
// Included as its child only to reuse unchanged real-Stylo/Scene snapshot helpers.
// Literal outer allocations are arithmetic, not candidate or upstream output.

const FORMATS: [(&str, Display); 3] = [
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
const EMPTY_PAIR: &str = "<span id=a></span><a id=n href=/n>N</a>";
const CHILD_PAIR: &str = "<a id=a href=/a><span id=child>X</span></a><a id=n href=/n>N</a>";
const CHILD_PREFIXED: &str =
    "<a id=p href=/p>P</a><a id=a href=/a><span id=child>X</span></a><a id=n href=/n>N</a>";
const CHILD_ONLY: &str = "<a id=a href=/a><span id=child>X</span></a>";

fn formatting_fixture(
    format: (&str, Display),
    width: f32,
    rules: &str,
    body: &str,
) -> (Document, Vec<ComputedStyle>) {
    fixture(
        width,
        &format!(
            "#a{{{};height:10px;background:#aa0000}}#child{{display:block;width:4px;height:6px;flex:none}}{rules}",
            format.0
        ),
        body,
    )
}

fn assert_formatting_flow(
    format: (&str, Display),
    width: f32,
    rules: &str,
    body: &str,
    height: f32,
    a_height: f32,
    advance: f32,
    expected: &[(&str, Geometry)],
) {
    let (document, computed) = formatting_fixture(format, width, rules, body);
    let line = node(&document, "#line");
    let a = node(&document, "#a");
    let controls = Controls::default();
    let mut measured_fonts = fonts();
    let mut measured =
        measurement_layout(&document, &mut measured_fonts, &controls, computed.clone());
    let mut painted_fonts = fonts();
    let mut painted = measurement_layout(&document, &mut painted_fonts, &controls, computed);
    let cold = SavedScene::capture(&measured.scene);
    for layout in [&mut measured, &mut painted] {
        assert_eq!(layout.styles[a].display, format.1);
        assert!(layout.styles[a].width.resolve(width).is_some());
        assert_eq!(
            layout.inline_box_width(a, width, None, 0),
            advance,
            "{}: signed inline admission",
            format.0
        );
        // A must supply its own height: a taller sentinel cannot hide a bad
        // auto-height metric. This calls the real inline seam, not measure_outer.
        assert_eq!(
            layout.line_metrics(line, width, None, &[Token::Box(a)], 0),
            (a_height, 0.0),
            "{}: isolated atomic line metrics",
            format.0
        );
        assert_eq!(layout.format_scratch, 0, "pass-local scratch released");
    }
    let tokens = row_tokens(&mut measured, line);
    assert_eq!(
        measured.flow_tokens::<false>(line, 20.0, 10.0, width, None, tokens, 0),
        height,
        "{}: full measurement height",
        format.0
    );
    assert_eq!(SavedScene::capture(&measured.scene), cold);
    assert_no_scene_allocation(&measured);
    assert_eq!(measured.scene.static_positions.capacity(), 0);
    let tokens = row_tokens(&mut painted, line);
    assert_eq!(
        painted.flow_tokens::<true>(line, 20.0, 10.0, width, None, tokens, 0),
        height,
        "{}: full emitted height",
        format.0
    );
    for (selector, expected) in expected {
        assert_eq!(
            border(&painted.scene, node(&document, selector)),
            *expected,
            "{}: {selector} literal border",
            format.0
        );
    }
    for layout in [&measured, &painted] {
        assert!(!layout.exhausted && layout.failure.is_none());
        assert!(layout.budget > 0 && layout.budget < 2_000_000);
        assert_eq!(layout.format_scratch, 0);
        assert!(layout.diagnostics.entries.is_empty());
        assert_eq!(layout.diagnostics.omitted, 0);
    }
    // The existing formatter intentionally uses ComputeSize versus PerformLayout
    // and charges real child emission. Do not invent equal total work counts.
    populate_scene(&mut painted.scene, line);
    let populated = SavedScene::capture(&painted.scene);
    for _ in 0..2 {
        assert_eq!(painted.inline_box_width(a, width, None, 0), advance);
        assert_eq!(
            painted.line_metrics(line, width, None, &[Token::Box(a)], 0),
            (a_height, 0.0)
        );
        let tokens = row_tokens(&mut painted, line);
        assert_eq!(
            painted.flow_tokens::<false>(line, 20.0, 10.0, width, None, tokens, 0),
            height
        );
        assert_eq!(SavedScene::capture(&painted.scene), populated);
        assert_eq!(painted.format_scratch, 0);
        assert!(!painted.exhausted && painted.failure.is_none());
    }
}

#[test]
fn formatting_inline_empty_and_nonempty_margin_boxes_are_not_forced_borders() {
    for format in FORMATS {
        for body in [EMPTY_PAIR, CHILD_PAIR] {
            assert_formatting_flow(
                format,
                200.0,
                "#a{width:24px;height:12px;border-bottom:4px solid black;margin:3px}#n{height:22px}",
                body,
                22.0,
                22.0,
                30.0,
                &[
                    ("#a", (23.0, 13.0, 24.0, 16.0)),
                    ("#n", (50.0, 10.0, 10.0, 22.0)),
                ],
            );
        }
    }
}

#[test]
fn formatting_inline_padding_and_box_sizing_count_edges_once() {
    for format in FORMATS {
        for (css, advance, a, next) in [
            (
                "width:64px;padding:0 32px",
                128.0,
                (20.0, 10.0, 128.0, 10.0),
                148.0,
            ),
            ("width:40px", 93.0, (27.0, 10.0, 75.0, 10.0), 113.0),
            (
                "width:40px;box-sizing:border-box",
                58.0,
                (27.0, 10.0, 40.0, 10.0),
                78.0,
            ),
            (
                "width:20px;box-sizing:border-box",
                53.0,
                (27.0, 10.0, 35.0, 10.0),
                73.0,
            ),
        ] {
            let edges = if advance == 128.0 { "" } else { ASYMMETRIC };
            assert_formatting_flow(
                format,
                200.0,
                &format!("#a{{{edges};{css}}}"),
                CHILD_PAIR,
                10.0,
                10.0,
                advance,
                &[("#a", a), ("#n", (next, 10.0, 10.0, 10.0))],
            );
        }
    }
}

#[test]
fn formatting_inline_percentage_basis_survives_a_nonzero_prefix() {
    let dimensions = "width:50%;padding-left:10%;padding-right:5%;border-left:2px solid black;border-right:3px solid black;margin-left:5%;margin-right:2.5%";
    for format in FORMATS {
        for (sizing, advance, border_width, next) in [
            ("content-box", 150.0, 135.0, 210.0),
            ("border-box", 115.0, 100.0, 175.0),
        ] {
            assert_formatting_flow(
                format,
                200.0,
                &format!("#a{{{dimensions};box-sizing:{sizing}}}"),
                CHILD_PREFIXED,
                10.0,
                10.0,
                advance,
                &[
                    ("#a", (70.0, 10.0, border_width, 10.0)),
                    ("#child", (92.0, 10.0, 4.0, 6.0)),
                    ("#n", (next, 10.0, 10.0, 10.0)),
                ],
            );
        }
    }
}

#[test]
fn formatting_inline_min_max_keep_the_declared_sizing_box() {
    for format in FORMATS {
        for (css, advance, border_width, next) in [
            ("width:80px;max-width:50px", 103.0, 85.0, 123.0),
            ("width:20px;min-width:50px", 103.0, 85.0, 123.0),
            (
                "width:80px;min-width:70px;max-width:50px",
                123.0,
                105.0,
                143.0,
            ),
            (
                "width:80px;min-width:70px;max-width:50px;box-sizing:border-box",
                88.0,
                70.0,
                108.0,
            ),
        ] {
            assert_formatting_flow(
                format,
                200.0,
                &format!("#a{{{ASYMMETRIC};{css}}}"),
                CHILD_PAIR,
                10.0,
                10.0,
                advance,
                &[
                    ("#a", (27.0, 10.0, border_width, 10.0)),
                    ("#n", (next, 10.0, 10.0, 10.0)),
                ],
            );
        }
        assert_formatting_flow(
            format,
            200.0,
            &format!("#a{{{ASYMMETRIC};width:25%;min-width:30%;max-width:20%}}"),
            CHILD_PREFIXED,
            10.0,
            10.0,
            113.0,
            &[
                ("#a", (67.0, 10.0, 95.0, 10.0)),
                ("#n", (173.0, 10.0, 10.0, 10.0)),
            ],
        );
    }
}

#[test]
fn formatting_inline_signed_advance_preserves_negative_totals_and_overlap() {
    for format in FORMATS {
        assert_formatting_flow(
            format,
            100.0,
            "#p{width:60px}#a{width:20px;margin-left:-30px}#n{width:50px}",
            CHILD_PREFIXED,
            10.0,
            10.0,
            -10.0,
            &[
                ("#a", (50.0, 10.0, 20.0, 10.0)),
                ("#n", (70.0, 10.0, 50.0, 10.0)),
            ],
        );
        assert_formatting_flow(
            format,
            200.0,
            "#a{width:80px;margin-left:10px;margin-right:-15px}",
            CHILD_PAIR,
            10.0,
            10.0,
            75.0,
            &[
                ("#a", (30.0, 10.0, 80.0, 10.0)),
                ("#n", (95.0, 10.0, 10.0, 10.0)),
            ],
        );
    }
}

#[test]
fn formatting_inline_auto_margins_are_zero_but_line_alignment_still_applies() {
    for format in FORMATS {
        for (margins, advance, a, next) in [
            ("margin-left:auto;margin-right:auto", 40.0, 60.0, 100.0),
            ("margin-left:auto", 40.0, 60.0, 100.0),
            ("margin-right:auto", 40.0, 60.0, 100.0),
            ("margin-left:auto;margin-right:10px", 50.0, 60.0, 110.0),
            ("margin-left:10px;margin-right:auto", 50.0, 70.0, 110.0),
        ] {
            assert_formatting_flow(
                format,
                200.0,
                &format!("#a{{width:40px;{margins}}}"),
                CHILD_PREFIXED,
                10.0,
                10.0,
                advance,
                &[
                    ("#a", (a, 10.0, 40.0, 10.0)),
                    ("#n", (next, 10.0, 10.0, 10.0)),
                ],
            );
        }
        for (align, a, next) in [("center", 115.0, 155.0), ("right", 170.0, 210.0)] {
            assert_formatting_flow(
                format,
                200.0,
                &format!("#line{{text-align:{align}}}#a{{width:40px;margin:0 auto}}"),
                CHILD_PREFIXED,
                10.0,
                10.0,
                40.0,
                &[
                    ("#a", (a, 10.0, 40.0, 10.0)),
                    ("#n", (next, 10.0, 10.0, 10.0)),
                ],
            );
        }
    }
}

#[test]
fn formatting_inline_exact_wrap_and_definite_overflow_do_not_shrink() {
    for format in FORMATS {
        for (width, next, height) in [
            (105.0, (120.0, 10.0, 5.0, 10.0), 10.0),
            (104.0, (20.0, 20.0, 5.0, 10.0), 20.0),
        ] {
            assert_formatting_flow(
                format,
                width,
                "#p{width:60px}#a{width:50px;margin-right:-10px}#n{width:5px}",
                CHILD_PREFIXED,
                height,
                10.0,
                40.0,
                &[("#a", (80.0, 10.0, 50.0, 10.0)), ("#n", next)],
            );
        }
        assert_formatting_flow(
            format,
            200.0,
            "#a{width:240px}",
            CHILD_PAIR,
            20.0,
            10.0,
            240.0,
            &[
                ("#a", (20.0, 10.0, 240.0, 10.0)),
                ("#n", (20.0, 20.0, 10.0, 10.0)),
            ],
        );
    }
}

#[test]
fn formatting_inline_auto_height_comes_from_the_same_fixed_child_in_all_seams() {
    for format in FORMATS {
        for body in [CHILD_PAIR, CHILD_ONLY] {
            let mut expected = vec![
                ("#a", (22.0, 10.0, 16.0, 12.0)),
                ("#child", (22.0, 10.0, 4.0, 12.0)),
            ];
            if body == CHILD_PAIR {
                expected.push(("#n", (40.0, 10.0, 10.0, 12.0)));
            }
            assert_formatting_flow(
                format,
                30.0,
                "#a{width:16px;height:auto;margin:0 2px}#child{height:12px}#n{height:12px}",
                body,
                12.0,
                12.0,
                20.0,
                &expected,
            );
        }
    }
}

#[test]
fn formatting_inline_keeps_existing_work_depth_and_false_scene_containment() {
    for format in FORMATS {
        let (document, computed) = formatting_fixture(format, 200.0, "#a{width:40px}", CHILD_PAIR);
        let line = node(&document, "#line");
        let a = node(&document, "#a");
        let controls = Controls::default();
        for (budget, depth) in [(0, 0), (2_000_000, MAX_DEPTH + 1)] {
            let mut font = fonts();
            let mut layout = measurement_layout(&document, &mut font, &controls, computed.clone());
            populate_scene(&mut layout.scene, line);
            let before = SavedScene::capture(&layout.scene);
            layout.budget = budget;
            let _ = layout.flow_tokens::<false>(
                line,
                20.0,
                10.0,
                200.0,
                None,
                vec![Token::Box(a)],
                depth,
            );
            assert!(layout.exhausted, "{}: existing containment", format.0);
            assert_eq!(SavedScene::capture(&layout.scene), before);
            assert!(layout.intrinsic.is_empty());
            assert!(layout.content_intrinsic_cache.is_empty());
            assert!(layout.content_height_cache.is_empty());
            assert_eq!(layout.format_scratch, 0);
        }
    }
}
