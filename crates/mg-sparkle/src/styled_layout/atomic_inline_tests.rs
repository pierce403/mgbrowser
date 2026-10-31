// Private authored controls for docs/ATOMIC_INLINE.md, frozen before behavior.
// Included inside styled_layout::tests::atomic_inline_tests, whose parent
// helpers and private layout types are available through `use super::*`.
// No upstream fixture bytes or production test hooks are used here.

type Geometry = (f32, f32, f32, f32);

fn geometry(r: Rect) -> Geometry {
    (r.x, r.y, r.w, r.h)
}

#[derive(Debug, PartialEq)]
enum SavedImage {
    Resource(String, usize),
    InlineSvg(usize),
}

#[derive(Debug, PartialEq)]
enum SavedPaint {
    Background(Geometry, usize, [f32; 4]),
    RoundedDecoration(Geometry, usize, [f32; 4]),
    Rect(Geometry, Color),
    Text(Geometry, usize, String, usize),
    Image(Geometry, SavedImage, Option<(usize, String, usize)>),
    Control(Geometry, usize, String, usize, bool),
}

#[derive(Debug, PartialEq)]
struct SavedScene {
    paints: Vec<SavedPaint>,
    paint_nodes: Vec<usize>,
    clips: Vec<Option<Geometry>>,
    boxes: Vec<LayoutBox>,
    box_scroll: Vec<bool>,
    hits: Vec<(Geometry, Action, Option<usize>)>,
    hit_nodes: Vec<usize>,
    borders: Vec<(usize, Geometry)>,
    static_positions: HashMap<usize, (f32, f32, bool)>,
    capacities: [usize; 9],
}

impl SavedScene {
    fn capture(scene: &Scene) -> Self {
        Self {
            paints: scene
                .paints
                .iter()
                .map(|paint| match paint {
                    Paint::Background(r, node, padding) => {
                        SavedPaint::Background(geometry(*r), *node, *padding)
                    }
                    Paint::RoundedDecoration(r, node, padding) => {
                        SavedPaint::RoundedDecoration(geometry(*r), *node, *padding)
                    }
                    Paint::Rect(r, color) => SavedPaint::Rect(geometry(*r), *color),
                    Paint::Text(r, node, text) => {
                        SavedPaint::Text(geometry(*r), *node, text.clone(), text.capacity())
                    }
                    Paint::Image(r, source, fallback) => SavedPaint::Image(
                        geometry(*r),
                        match source {
                            ImageSource::Resource(url) => {
                                SavedImage::Resource(url.clone(), url.capacity())
                            }
                            ImageSource::InlineSvg(node) => SavedImage::InlineSvg(*node),
                        },
                        fallback
                            .as_ref()
                            .map(|(node, text)| (*node, text.clone(), text.capacity())),
                    ),
                    Paint::Control(r, node, visual) => {
                        let (label, enabled) = match visual {
                            ControlVisual::Button { label } => (label.as_str(), true),
                            ControlVisual::Text { text, .. } => (text.as_str(), false),
                            ControlVisual::Select { label } => (label.as_str(), false),
                            ControlVisual::Checkbox { checked } => {
                                ("checkbox", *checked)
                            }
                            ControlVisual::Radio { checked } => ("radio", *checked),
                        };
                        SavedPaint::Control(
                            geometry(*r),
                            *node,
                            label.to_owned(),
                            label.len(),
                            enabled,
                        )
                    }
                })
                .collect(),
            paint_nodes: scene.paint_nodes.clone(),
            clips: scene.clips.iter().map(|r| r.map(geometry)).collect(),
            boxes: scene.boxes.clone(),
            box_scroll: scene.box_scroll.clone(),
            hits: scene
                .hits
                .iter()
                .map(|(r, action)| {
                    let capacity = match action {
                        Action::Link { href, .. } => Some(href.capacity()),
                        _ => None,
                    };
                    (geometry(*r), action.clone(), capacity)
                })
                .collect(),
            hit_nodes: scene.hit_nodes.clone(),
            borders: scene
                .borders
                .iter()
                .map(|(node, r)| (*node, geometry(*r)))
                .collect(),
            static_positions: scene.static_positions.clone(),
            capacities: [
                scene.paints.capacity(),
                scene.paint_nodes.capacity(),
                scene.clips.capacity(),
                scene.boxes.capacity(),
                scene.box_scroll.capacity(),
                scene.hits.capacity(),
                scene.hit_nodes.capacity(),
                scene.borders.capacity(),
                scene.static_positions.capacity(),
            ],
        }
    }
}

const PAIR: &str = "<a id=a href=/a>A</a><a id=n href=/n>N</a>";
const PREFIXED: &str = "<a id=p href=/p>P</a><a id=a href=/a>A</a><a id=n href=/n>N</a>";
const ASYMMETRIC: &str = "padding-left:10px;padding-right:20px;border-left:2px solid black;border-right:3px solid black;margin-left:7px;margin-right:11px";

fn fixture(width: f32, rules: &str, body: &str) -> (Document, Vec<ComputedStyle>) {
    let document = crate::document::parse(
        &format!(
            "<!doctype html><style>html,body{{margin:0}}*{{margin:0;padding:0;border:0;font-size:1px;line-height:0;font-weight:400}}#line{{width:{width}px}}a{{display:inline-block;height:10px;background:#aa0000;color:#aa0000;text-decoration:none}}#p{{width:40px;background:#0000aa;color:#0000aa}}#n{{width:10px;background:#00aa00;color:#00aa00}}{rules}</style><div id=line>{body}</div>"
        ),
        "https://fixture.example/",
    );
    // The synthetic legacy styles() helper would omit the actual inherited
    // text-node box fields which caused one of the motivating defects.
    let computed =
        crate::style::compute_styles(&document, &document.stylesheets, (800.0, 600.0)).unwrap();
    (document, computed)
}

fn node(document: &Document, selector: &str) -> usize {
    document.query_selector(0, selector).unwrap().unwrap()
}

fn row_tokens(layout: &mut Layout<'_, '_>, line: usize) -> Vec<Token> {
    let mut tokens = Vec::new();
    for child in layout.document.nodes[line].children.clone() {
        layout.tokens(child, &mut tokens, 1);
    }
    tokens
}

fn border(scene: &Scene, owner: usize) -> Geometry {
    geometry(
        scene
            .borders
            .iter()
            .rev()
            .find(|(node, _)| *node == owner)
            .expect("real emitted structural border")
            .1,
    )
}

fn owned_string(text: &str) -> String {
    let mut value = String::with_capacity(text.len() + 31);
    value.push_str(text);
    value
}

fn populate_scene(scene: &mut Scene, owner: usize) {
    let r = Rect {
        x: -7.25,
        y: 3.5,
        w: 11.5,
        h: 9.25,
    };
    scene.paint(
        owner,
        Paint::Rect(
            r,
            Color {
                rgb: 0x123456,
                alpha: 0.5,
            },
        ),
    );
    scene.paint(owner, Paint::Text(r, owner, owned_string("sentinel")));
    scene.paint(
        owner,
        Paint::Image(
            r,
            ImageSource::Resource(owned_string("https://fixture.example/sentinel")),
            Some((owner, owned_string("fallback"))),
        ),
    );
    scene.paint(owner, Paint::Image(r, ImageSource::InlineSvg(owner), None));
    scene.paint(
        owner,
        Paint::Control(
            r,
            owner,
            ControlVisual::Button {
                label: owned_string("control"),
            },
        ),
    );
    scene.box_for(owner, r);
    *scene.box_scroll.last_mut().unwrap() = false;
    scene.hit(
        owner,
        r,
        Action::Link {
            node: owner,
            href: owned_string("https://fixture.example/sentinel"),
        },
    );
    scene.border_for(owner, r);
    *scene.clips.last_mut().unwrap() = Some(r);
    scene.static_positions.insert(owner, (13.25, -4.5, false));
}

// Both layouts start from the same real computed snapshot and call sequence.
// Literal expected rectangles are supplied by each authored arithmetic case.
fn assert_flow(
    width: f32,
    rules: &str,
    body: &str,
    height: f32,
    expected: &[(&str, Geometry)],
    advances: &[(&str, f32)],
    text_origins: &[(&str, f32)],
) {
    let (document, computed) = fixture(width, rules, body);
    let line = node(&document, "#line");
    let controls = Controls::default();
    let mut measured_fonts = fonts();
    let mut measured =
        measurement_layout(&document, &mut measured_fonts, &controls, computed.clone());
    let mut painted_fonts = fonts();
    let mut painted = measurement_layout(&document, &mut painted_fonts, &controls, computed);
    let cold = SavedScene::capture(&measured.scene);
    for (selector, advance) in advances {
        let id = node(&document, selector);
        assert_eq!(measured.styles[id].display, Display::InlineBlock);
        assert_eq!(
            measured.inline_box_width(id, width, None, 0),
            *advance,
            "{selector} signed advance"
        );
        assert_eq!(painted.inline_box_width(id, width, None, 0), *advance);
    }
    let tokens = row_tokens(&mut measured, line);
    let measured_height = measured.flow_tokens::<false>(line, 20.0, 10.0, width, None, tokens, 0);
    assert_eq!(measured_height, height, "measurement height");
    assert_eq!(SavedScene::capture(&measured.scene), cold);
    assert_no_scene_allocation(&measured);
    assert_eq!(measured.scene.static_positions.capacity(), 0);
    let tokens = row_tokens(&mut painted, line);
    assert_eq!(
        painted.flow_tokens::<true>(line, 20.0, 10.0, width, None, tokens, 0),
        height,
        "emitted height"
    );
    assert_eq!(
        measured.budget, painted.budget,
        "same measuring/emitting work path"
    );
    for (selector, expected) in expected {
        assert_eq!(
            border(&painted.scene, node(&document, selector)),
            *expected,
            "{selector} structural border"
        );
    }
    for (selector, x) in text_origins {
        let owner = node(&document, selector);
        let child = document.nodes[owner]
            .children
            .iter()
            .copied()
            .find(|id| document.nodes[*id].tag == "#text")
            .unwrap();
        let actual = painted
            .scene
            .paints
            .iter()
            .find_map(|paint| match paint {
                Paint::Text(r, id, _) if *id == child => Some(r.x),
                _ => None,
            })
            .expect("the real child text paint");
        assert_eq!(actual, *x, "{selector} content origin");
    }
    assert!(!measured.exhausted && !painted.exhausted);
    assert!(measured.failure.is_none() && painted.failure.is_none());
    // Seed every otherwise optional scene field and all paint payload kinds.
    // False passes may warm measurement caches, never mutate this scene.
    populate_scene(&mut painted.scene, line);
    let populated = SavedScene::capture(&painted.scene);
    for _ in 0..2 {
        let tokens = row_tokens(&mut painted, line);
        assert_eq!(
            painted.flow_tokens::<false>(line, 20.0, 10.0, width, None, tokens, 0),
            height
        );
        assert_eq!(SavedScene::capture(&painted.scene), populated);
    }
}

#[test]
fn atomic_numeric_edges_use_distinct_border_and_outer_widths() {
    let cases = [
        ("width:40px", 40.0, (20.0, 10.0, 40.0, 10.0), 60.0),
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
    ];
    for (index, (css, advance, a, next)) in cases.into_iter().enumerate() {
        let edges = if index >= 2 { ASYMMETRIC } else { "" };
        assert_flow(
            200.0,
            &format!("#a{{{edges};{css}}}"),
            PAIR,
            10.0,
            &[("#a", a), ("#n", (next, 10.0, 10.0, 10.0))],
            &[("#a", advance)],
            &[],
        );
    }
}

#[test]
fn atomic_real_text_snapshots_cannot_enlarge_declared_width() {
    for body in [PAIR, "<a id=a href=/a></a><a id=n href=/n>N</a>"] {
        assert_flow(
            200.0,
            "#a{width:64px;padding:0 32px}",
            body,
            10.0,
            &[
                ("#a", (20.0, 10.0, 128.0, 10.0)),
                ("#n", (148.0, 10.0, 10.0, 10.0)),
            ],
            &[("#a", 128.0)],
            &[],
        );
    }
    let (document, computed) = fixture(200.0, "#a{width:64px;padding:0 32px}", PAIR);
    let a = node(&document, "#a");
    let text = document.nodes[a].children[0];
    assert_eq!(document.nodes[text].tag, "#text");
    assert_eq!(
        computed[text].width, computed[a].width,
        "preserve actual snapshot fields, not a test-only normalization"
    );
    assert_eq!(computed[text].padding, computed[a].padding);
}

#[test]
fn atomic_percentages_use_containing_width_even_after_a_prefix() {
    let dimensions = "width:50%;padding-left:10%;padding-right:5%;border-left:2px solid black;border-right:3px solid black;margin-left:5%;margin-right:2.5%";
    for (body, sizing, advance, a, next, text) in [
        (
            PAIR,
            "content-box",
            150.0,
            (30.0, 10.0, 135.0, 10.0),
            170.0,
            52.0,
        ),
        (
            PAIR,
            "border-box",
            115.0,
            (30.0, 10.0, 100.0, 10.0),
            135.0,
            52.0,
        ),
        (
            PREFIXED,
            "content-box",
            150.0,
            (70.0, 10.0, 135.0, 10.0),
            210.0,
            92.0,
        ),
        (
            PREFIXED,
            "border-box",
            115.0,
            (70.0, 10.0, 100.0, 10.0),
            175.0,
            92.0,
        ),
    ] {
        assert_flow(
            200.0,
            &format!("#a{{{dimensions};box-sizing:{sizing}}}"),
            body,
            10.0,
            &[("#a", a), ("#n", (next, 10.0, 10.0, 10.0))],
            &[("#a", advance)],
            &[("#a", text)],
        );
    }
}

#[test]
fn atomic_numeric_and_percentage_min_max_constrain_the_declared_sizing_box() {
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
            "width:80px;max-width:50px;box-sizing:border-box",
            68.0,
            50.0,
            88.0,
        ),
        (
            "width:20px;min-width:50px;box-sizing:border-box",
            68.0,
            50.0,
            88.0,
        ),
        (
            "width:80px;min-width:70px;max-width:50px;box-sizing:border-box",
            88.0,
            70.0,
            108.0,
        ),
        ("width:25%;min-width:30%;max-width:20%", 113.0, 95.0, 133.0),
    ] {
        assert_flow(
            200.0,
            &format!("#a{{{ASYMMETRIC};{css}}}"),
            PAIR,
            10.0,
            &[
                ("#a", (27.0, 10.0, border_width, 10.0)),
                ("#n", (next, 10.0, 10.0, 10.0)),
            ],
            &[("#a", advance)],
            &[],
        );
    }
    assert_flow(
        200.0,
        &format!("#a{{{ASYMMETRIC};width:25%;min-width:30%;max-width:20%}}"),
        PREFIXED,
        10.0,
        &[
            ("#a", (67.0, 10.0, 95.0, 10.0)),
            ("#n", (173.0, 10.0, 10.0, 10.0)),
        ],
        &[("#a", 113.0)],
        &[],
    );
}

#[test]
fn atomic_signed_margins_preserve_negative_advance_and_overlap() {
    for (css, a) in [
        (
            "width:80px;margin-left:-10px;margin-right:5px",
            (10.0, 10.0, 80.0, 10.0),
        ),
        (
            "width:80px;margin-left:10px;margin-right:-15px",
            (30.0, 10.0, 80.0, 10.0),
        ),
    ] {
        assert_flow(
            200.0,
            &format!("#a{{{css}}}"),
            PAIR,
            10.0,
            &[("#a", a), ("#n", (95.0, 10.0, 10.0, 10.0))],
            &[("#a", 75.0)],
            &[],
        );
    }
    assert_flow(
        100.0,
        "#p{width:60px}#a{width:20px;margin-left:-30px}#n{width:50px}",
        PREFIXED,
        10.0,
        &[
            ("#a", (50.0, 10.0, 20.0, 10.0)),
            ("#n", (70.0, 10.0, 50.0, 10.0)),
        ],
        &[("#a", -10.0)],
        &[],
    );
}

#[test]
fn atomic_wrap_uses_exact_signed_margin_box_boundaries() {
    let prefix_a = "<a id=p href=/p>P</a><a id=a href=/a>A</a>";
    for (width, margin, a, height) in [
        (100.0, -10, (70.0, 10.0, 50.0, 10.0), 10.0),
        (99.0, -10, (10.0, 20.0, 50.0, 10.0), 20.0),
        (120.0, 10, (90.0, 10.0, 50.0, 10.0), 10.0),
        (119.0, 10, (30.0, 20.0, 50.0, 10.0), 20.0),
    ] {
        assert_flow(
            width,
            &format!("#p{{width:60px}}#a{{width:50px;margin-left:{margin}px}}"),
            prefix_a,
            height,
            &[("#a", a)],
            &[("#a", (50 + margin) as f32)],
            &[],
        );
    }
    for (width, next, height) in [
        (105.0, (120.0, 10.0, 5.0, 10.0), 10.0),
        (104.0, (20.0, 20.0, 5.0, 10.0), 20.0),
    ] {
        assert_flow(
            width,
            "#p{width:60px}#a{width:50px;margin-right:-10px}#n{width:5px}",
            PREFIXED,
            height,
            &[("#a", (80.0, 10.0, 50.0, 10.0)), ("#n", next)],
            &[("#a", 40.0)],
            &[],
        );
    }
}

#[test]
fn atomic_definite_overflow_does_not_shrink_to_available_or_residual_space() {
    assert_flow(
        200.0,
        "#a{width:240px}",
        PAIR,
        20.0,
        &[
            ("#a", (20.0, 10.0, 240.0, 10.0)),
            ("#n", (20.0, 20.0, 10.0, 10.0)),
        ],
        &[("#a", 240.0)],
        &[],
    );
    assert_flow(
        100.0,
        "#line{white-space:nowrap}#p{width:60px}#a{width:70px}",
        "<a id=p href=/p>P</a><a id=a href=/a>A</a>",
        10.0,
        &[("#a", (80.0, 10.0, 70.0, 10.0))],
        &[("#a", 70.0)],
        &[],
    );
    assert_flow(
        200.0,
        "#a{width:30px;overflow:hidden;font-size:12px;line-height:12px}",
        "<a id=a href=/a>MMMMMMMMMMMMMMMMMMMM</a><a id=n href=/n>N</a>",
        10.0,
        &[
            ("#a", (20.0, 10.0, 30.0, 10.0)),
            ("#n", (50.0, 10.0, 10.0, 10.0)),
        ],
        &[("#a", 30.0)],
        &[],
    );
}

#[test]
fn atomic_horizontal_auto_margins_are_zero_not_block_distribution() {
    for (margins, advance, a, next) in [
        ("margin-left:auto;margin-right:auto", 40.0, 60.0, 100.0),
        ("margin-left:auto", 40.0, 60.0, 100.0),
        ("margin-right:auto", 40.0, 60.0, 100.0),
        ("margin-left:auto;margin-right:10px", 50.0, 60.0, 110.0),
        ("margin-left:10px;margin-right:auto", 50.0, 70.0, 110.0),
    ] {
        assert_flow(
            200.0,
            &format!("#a{{width:40px;{margins}}}"),
            PREFIXED,
            10.0,
            &[
                ("#a", (a, 10.0, 40.0, 10.0)),
                ("#n", (next, 10.0, 10.0, 10.0)),
            ],
            &[("#a", advance)],
            &[],
        );
    }
    for (align, a, next) in [("center", 115.0, 155.0), ("right", 170.0, 210.0)] {
        assert_flow(
            200.0,
            &format!("#line{{text-align:{align}}}#a{{width:40px;margin:0 auto}}"),
            PREFIXED,
            10.0,
            &[
                ("#a", (a, 10.0, 40.0, 10.0)),
                ("#n", (next, 10.0, 10.0, 10.0)),
            ],
            &[("#a", 40.0)],
            &[],
        );
    }
}

#[test]
fn atomic_fractional_border_and_signed_outer_advance_are_not_rounded_early() {
    assert_flow(
        200.0,
        "#a{width:20.5px;padding-left:1.25px;padding-right:2.25px;border-left:1px solid black;border-right:2px solid black;margin-left:3.5px;margin-right:-1.5px}",
        PAIR,
        10.0,
        &[
            ("#a", (23.5, 10.0, 27.0, 10.0)),
            ("#n", (49.0, 10.0, 10.0, 10.0)),
        ],
        &[("#a", 29.0)],
        &[],
    );
}

#[test]
fn atomic_auto_height_measures_the_same_actual_content_width_as_emission() {
    let mut font = fonts();
    let m = font.width_weight("M", 12.0, false);
    let space = font.width_weight(" ", 12.0, false);
    assert!(
        m <= 14.0 && m * 2.0 + space > 14.0,
        "authored pinned-font premise: one M fits14px but M-space-M does not"
    );
    // Two12px lines; N is24px too so no new baseline/vertical-align oracle.
    assert_flow(
        200.0,
        "#a{width:14px;height:auto;padding-left:3px;padding-right:5px;border-left:1px solid black;border-right:2px solid black;margin-left:7px;margin-right:11px;font-size:12px;line-height:12px}#n{height:24px}",
        "<a id=a href=/a>M M</a><a id=n href=/n>N</a>",
        24.0,
        &[
            ("#a", (27.0, 10.0, 25.0, 24.0)),
            ("#n", (63.0, 10.0, 10.0, 24.0)),
        ],
        &[("#a", 43.0)],
        &[],
    );
    // Without N, A itself must supply the measured24px line height. The paired
    // control alone could hide bad auto-height metrics behind N's fixed24px.
    assert_flow(
        200.0,
        "#a{width:14px;height:auto;padding-left:3px;padding-right:5px;border-left:1px solid black;border-right:2px solid black;margin-left:7px;margin-right:11px;font-size:12px;line-height:12px}",
        "<a id=a href=/a>M M</a>",
        24.0,
        &[("#a", (27.0, 10.0, 25.0, 24.0))],
        &[("#a", 43.0)],
        &[],
    );
}

#[test]
fn atomic_false_pass_preserves_cold_and_every_populated_scene_field() {
    assert_flow(
        200.0,
        "#a{width:40px}",
        PAIR,
        10.0,
        &[
            ("#a", (20.0, 10.0, 40.0, 10.0)),
            ("#n", (60.0, 10.0, 10.0, 10.0)),
        ],
        &[("#a", 40.0)],
        &[],
    );
}

#[test]
fn atomic_measurement_retains_existing_depth_and_work_exhaustion() {
    let (document, computed) = fixture(200.0, "#a{width:40px}", PAIR);
    let line = node(&document, "#line");
    let a = node(&document, "#a");
    let controls = Controls::default();
    for (budget, depth) in [(0, 0), (2_000_000, MAX_DEPTH + 1)] {
        let mut font = fonts();
        let mut layout = measurement_layout(&document, &mut font, &controls, computed.clone());
        populate_scene(&mut layout.scene, line);
        let before = SavedScene::capture(&layout.scene);
        layout.budget = budget;
        let _ =
            layout.flow_tokens::<false>(line, 20.0, 10.0, 200.0, None, vec![Token::Box(a)], depth);
        assert!(layout.exhausted, "entry must retain existing containment");
        assert_eq!(SavedScene::capture(&layout.scene), before);
        assert!(layout.intrinsic.is_empty());
        assert!(layout.content_intrinsic_cache.is_empty());
        assert!(layout.content_height_cache.is_empty());
    }
}
