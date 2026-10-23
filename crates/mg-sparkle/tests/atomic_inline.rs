//! Independent authored controls for docs/ATOMIC_INLINE.md, frozen before execution.
//! The A-E rectangles/probes are literal box arithmetic, not candidate output or
//! upstream fixtures. These render-only tests do not exercise link activation.
use mg_sparkle::{
    document::{Document, parse},
    paint::Fonts,
    render::{Action, Controls, Frame, Hit, LayoutBox, Viewport, render_scaled},
};

type Rect = (i32, i32, u32, u32);
type Probe = (i32, i32, u32, Option<&'static str>);

const RED: u32 = 0xaa0000;
const GREEN: u32 = 0x00aa00;
const BLUE: u32 = 0x0000aa;
const BLACK: u32 = 0x000000;
const WHITE: u32 = 0xffffff;

fn fonts() -> Fonts {
    Fonts::from_bytes(
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap(),
    )
    .unwrap()
}

fn document(width: u32, container_css: &str, body: &str) -> Document {
    // The nearest containing width is deliberately different from the wider,
    // padded ancestor and viewport. There is no source whitespace between boxes.
    parse(
        &format!(
            "<html><head><style>html,body{{margin:0;background:#ffffff}} *{{margin:0;padding:0;border:0;font-size:1px;line-height:0}} .atomic{{display:inline-block;height:10px;text-decoration:none}} #a{{background:#aa0000;color:#aa0000}} #p{{background:#0000aa;color:#0000aa}} #n{{background:#00aa00;color:#00aa00}}</style></head><body><div style='width:320px;padding-top:10px;padding-left:20px'><div id=container style='width:{width}px;{container_css}'>{body}</div></div></body></html>"
        ),
        "https://fixture.example/",
    )
}

fn anchor(id: &str, css: &str, text: &str) -> String {
    format!("<a class=atomic id={id} href=/{id} style='{css}'>{text}</a>")
}

fn sequence(
    width: u32,
    container_css: &str,
    prefix: Option<u32>,
    a_css: &str,
    a_text: &str,
    next: Option<u32>,
) -> Document {
    let p = prefix.map_or_else(String::new, |width| {
        anchor("p", &format!("width:{width}px"), "P")
    });
    // Empty A1/A2 controls use non-link spans. All interactive boxes elsewhere
    // have ordinary literal text: no invisible shim or empty-link default claim.
    let a = if a_text.is_empty() {
        format!("<span class=atomic id=a style='{a_css}'></span>")
    } else {
        anchor("a", a_css, a_text)
    };
    let n = next.map_or_else(String::new, |width| {
        anchor("n", &format!("width:{width}px"), "N")
    });
    document(width, container_css, &format!("{p}{a}{n}"))
}

fn render(document: &Document, scale: f32) -> Frame {
    let frame = render_scaled(
        document,
        &mut fonts(),
        Viewport {
            width: 400,
            height: 160,
            scroll: 0,
        },
        &Controls::default(),
        scale,
    );
    assert_eq!(frame.diagnostics.omitted, 0);
    assert!(
        frame.diagnostics.entries.is_empty(),
        "authored sizing requires diagnostic-free styled layout: {:?}",
        frame.diagnostics
    );
    assert_eq!(frame.canvas.width, (400.0 * scale) as u32);
    assert_eq!(frame.canvas.height, (160.0 * scale) as u32);
    frame
}

fn rect<'a>(document: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let node = document.query_selector(0, selector).unwrap().unwrap();
    frame
        .boxes
        .iter()
        .rev()
        .find(|item| item.node == node)
        .unwrap_or_else(|| panic!("missing box for {selector}"))
}

fn geometry(rect: &LayoutBox) -> Rect {
    (rect.x, rect.y, rect.width, rect.height)
}

fn contains(hit: &Hit, x: i32, y: i32) -> bool {
    x >= hit.x
        && y >= hit.y
        && i64::from(x) < i64::from(hit.x) + i64::from(hit.w)
        && i64::from(y) < i64::from(hit.y) + i64::from(hit.h)
}

fn assert_probe(document: &Document, frame: &Frame, probe: Probe) {
    let (x, y, color, path) = probe;
    assert!(x >= 0 && y >= 0);
    let px = frame.canvas.physical_edge(i64::from(x)) as usize;
    let py = frame.canvas.physical_edge(i64::from(y)) as usize;
    assert!(px < frame.canvas.width as usize && py < frame.canvas.height as usize);
    assert_eq!(
        frame.canvas.pixels[py * frame.canvas.width as usize + px],
        color,
        "pixel ({x}, {y}) at scale {}",
        frame.canvas.scale()
    );
    let hit = frame.hits.iter().rev().find(|hit| contains(hit, x, y));
    if let Some(path) = path {
        let hit = hit.unwrap_or_else(|| panic!("no topmost link at ({x}, {y})"));
        let expected_node = document
            .query_selector(0, &format!("#{}", path.trim_start_matches('/')))
            .unwrap()
            .unwrap();
        assert!(
            matches!(&hit.action, Action::Link { node, href }
                if *node == expected_node && href == &format!("https://fixture.example{path}")),
            "wrong topmost hit at ({x}, {y}): {:?}",
            hit.action
        );
    } else {
        assert!(
            hit.is_none(),
            "unexpected topmost hit at ({x}, {y}): {hit:?}"
        );
    }
}

fn check(document: &Document, expected: &[(&str, Rect)], probes: &[Probe]) {
    for scale in [1.0, 2.0] {
        let frame = render(document, scale);
        for (selector, expected) in expected {
            assert_eq!(
                geometry(rect(document, &frame, selector)),
                *expected,
                "{selector} at scale {scale}"
            );
        }
        for probe in probes {
            assert_probe(document, &frame, *probe);
        }
    }
}

macro_rules! case {
    ($name:ident, $width:expr, $container:expr, $prefix:expr, $css:expr,
     $next:expr, $expected:expr, $probes:expr) => {
        #[test]
        fn $name() {
            let doc = sequence($width, $container, $prefix, $css, "A", $next);
            check(&doc, $expected, $probes);
        }
    };
}

case!(
    a1_content_width,
    200,
    "",
    None,
    "width:40px",
    Some(10),
    &[("#a", (20, 10, 40, 10)), ("#n", (60, 10, 10, 10))],
    &[(25, 15, RED, Some("/a")), (65, 15, GREEN, Some("/n"))]
);
case!(
    a2_padding_is_added_once_even_with_inherited_text_snapshots,
    200,
    "",
    None,
    "width:64px;padding-left:32px;padding-right:32px",
    Some(10),
    &[("#a", (20, 10, 128, 10)), ("#n", (148, 10, 10, 10))],
    &[(140, 15, RED, Some("/a")), (150, 15, GREEN, Some("/n"))]
);
case!(
    a3_content_box_counts_each_edge_and_margin_once,
    200,
    "",
    None,
    "width:40px;padding-left:10px;padding-right:20px;border-left:2px solid black;border-right:3px solid black;margin-left:7px;margin-right:11px",
    Some(10),
    &[("#a", (27, 10, 75, 10)), ("#n", (113, 10, 10, 10))],
    &[
        (27, 15, BLACK, Some("/a")),
        (40, 15, RED, Some("/a")),
        (101, 15, BLACK, Some("/a")),
        (108, 15, WHITE, None),
        (115, 15, GREEN, Some("/n"))
    ]
);
case!(
    a4_border_box_changes_sizing_box_not_edge_count,
    200,
    "",
    None,
    "box-sizing:border-box;width:40px;padding-left:10px;padding-right:20px;border-left:2px solid black;border-right:3px solid black;margin-left:7px;margin-right:11px",
    Some(10),
    &[("#a", (27, 10, 40, 10)), ("#n", (78, 10, 10, 10))],
    &[
        (27, 15, BLACK, Some("/a")),
        (40, 15, RED, Some("/a")),
        (66, 15, BLACK, Some("/a")),
        (75, 15, WHITE, None),
        (80, 15, GREEN, Some("/n"))
    ]
);
case!(
    a5_border_box_has_nonnegative_content_and_edge_floor,
    200,
    "",
    None,
    "box-sizing:border-box;width:20px;padding-left:10px;padding-right:20px;border-left:2px solid black;border-right:3px solid black;margin-left:7px;margin-right:11px",
    Some(10),
    &[("#a", (27, 10, 35, 10)), ("#n", (73, 10, 10, 10))],
    &[
        (27, 15, BLACK, Some("/a")),
        (40, 15, RED, Some("/a")),
        (61, 15, BLACK, Some("/a")),
        (70, 15, WHITE, None),
        (75, 15, GREEN, Some("/n"))
    ]
);
case!(
    a6_negative_left_margin_moves_border_without_inflating_width,
    200,
    "",
    None,
    "width:80px;margin-left:-10px;margin-right:5px",
    Some(10),
    &[("#a", (10, 10, 80, 10)), ("#n", (95, 10, 10, 10))],
    &[
        (15, 15, RED, Some("/a")),
        (92, 15, WHITE, None),
        (97, 15, GREEN, Some("/n"))
    ]
);
case!(
    a7_later_sentinel_wins_negative_right_margin_overlap,
    200,
    "",
    None,
    "width:80px;margin-left:10px;margin-right:-15px",
    Some(10),
    &[("#a", (30, 10, 80, 10)), ("#n", (95, 10, 10, 10))],
    &[
        (90, 15, RED, Some("/a")),
        (100, 15, GREEN, Some("/n")),
        (107, 15, RED, Some("/a"))
    ]
);

#[test]
fn a1_a2_empty_nonlink_boxes_have_the_same_literal_border_allocation() {
    for (css, a, n, probes) in [
        (
            "width:40px",
            (20, 10, 40, 10),
            (60, 10, 10, 10),
            [(25, 15, RED, None), (65, 15, GREEN, Some("/n"))],
        ),
        (
            "width:64px;padding-left:32px;padding-right:32px",
            (20, 10, 128, 10),
            (148, 10, 10, 10),
            [(140, 15, RED, None), (150, 15, GREEN, Some("/n"))],
        ),
    ] {
        let doc = sequence(200, "", None, css, "", Some(10));
        check(&doc, &[("#a", a), ("#n", n)], &probes);
    }
}

const PERCENT_EDGES: &str = "width:50%;padding-left:10%;padding-right:5%;border-left:2px solid black;border-right:3px solid black;margin-left:5%;margin-right:2.5%";

#[test]
fn b1_b4_percentages_keep_the_nearest_content_width_with_and_without_prefix() {
    // All four expectations use C=200, not viewport400, padded ancestor320,
    // residual160, chosen border135/100 or margin-box150/115.
    for (prefix, sizing, a, n, content_x, probes) in [
        (
            None,
            "content-box",
            (30, 10, 135, 10),
            (170, 10, 10, 10),
            52,
            [
                (30, 15, BLACK, Some("/a")),
                (60, 15, RED, Some("/a")),
                (172, 15, GREEN, Some("/n")),
            ],
        ),
        (
            None,
            "border-box",
            (30, 10, 100, 10),
            (135, 10, 10, 10),
            52,
            [
                (30, 15, BLACK, Some("/a")),
                (60, 15, RED, Some("/a")),
                (137, 15, GREEN, Some("/n")),
            ],
        ),
        (
            Some(40),
            "content-box",
            (70, 10, 135, 10),
            (210, 10, 10, 10),
            92,
            [
                (70, 15, BLACK, Some("/a")),
                (100, 15, RED, Some("/a")),
                (212, 15, GREEN, Some("/n")),
            ],
        ),
        (
            Some(40),
            "border-box",
            (70, 10, 100, 10),
            (175, 10, 10, 10),
            92,
            [
                (70, 15, BLACK, Some("/a")),
                (100, 15, RED, Some("/a")),
                (177, 15, GREEN, Some("/n")),
            ],
        ),
    ] {
        let doc = sequence(
            200,
            "",
            prefix,
            &format!("{PERCENT_EDGES};box-sizing:{sizing}"),
            "A",
            Some(10),
        );
        check(
            &doc,
            &[("#container", (20, 10, 200, 10)), ("#a", a), ("#n", n)],
            &probes,
        );
        if prefix.is_some() {
            check(
                &doc,
                &[("#p", (20, 10, 40, 10))],
                &[(25, 15, BLUE, Some("/p"))],
            );
        }
        let a = doc.query_selector(0, "#a").unwrap().unwrap();
        let text = doc.nodes[a].children[0];
        assert_eq!(doc.nodes[text].text, "A");
        for scale in [1.0, 2.0] {
            let frame = render(&doc, scale);
            let starts: Vec<_> = frame
                .boxes
                .iter()
                .filter(|item| item.node == text)
                .map(|item| item.x)
                .collect();
            assert_eq!(starts, [content_x]);
        }
    }
}

const FIXED_EDGES: &str = "padding-left:10px;padding-right:20px;border-left:2px solid black;border-right:3px solid black;margin-left:7px;margin-right:11px";

#[test]
fn c1_c7_min_max_precedence_uses_the_declared_sizing_box_and_containing_basis() {
    for (id, sizes, sizing, prefix, a, n, right_edge, gap, next) in [
        (
            "C1",
            "width:80px;max-width:50px",
            "content-box",
            None,
            (27, 10, 85, 10),
            (123, 10, 10, 10),
            111,
            120,
            125,
        ),
        (
            "C2",
            "width:20px;min-width:50px",
            "content-box",
            None,
            (27, 10, 85, 10),
            (123, 10, 10, 10),
            111,
            120,
            125,
        ),
        (
            "C3",
            "width:80px;min-width:70px;max-width:50px",
            "content-box",
            None,
            (27, 10, 105, 10),
            (143, 10, 10, 10),
            131,
            140,
            145,
        ),
        (
            "C4",
            "width:80px;max-width:50px",
            "border-box",
            None,
            (27, 10, 50, 10),
            (88, 10, 10, 10),
            76,
            85,
            90,
        ),
        (
            "C5",
            "width:20px;min-width:50px",
            "border-box",
            None,
            (27, 10, 50, 10),
            (88, 10, 10, 10),
            76,
            85,
            90,
        ),
        (
            "C6",
            "width:80px;min-width:70px;max-width:50px",
            "border-box",
            None,
            (27, 10, 70, 10),
            (108, 10, 10, 10),
            96,
            105,
            110,
        ),
        (
            "C7",
            "width:25%;min-width:30%;max-width:20%",
            "content-box",
            None,
            (27, 10, 95, 10),
            (133, 10, 10, 10),
            121,
            130,
            135,
        ),
        (
            "C7 after P40",
            "width:25%;min-width:30%;max-width:20%",
            "content-box",
            Some(40),
            (67, 10, 95, 10),
            (173, 10, 10, 10),
            161,
            170,
            175,
        ),
    ] {
        let doc = sequence(
            200,
            "",
            prefix,
            &format!("{FIXED_EDGES};{sizes};box-sizing:{sizing}"),
            "A",
            Some(10),
        );
        // These two shifted probes are explicit literals for the prefixed row.
        let (left, red) = if id == "C7 after P40" {
            (67, 80)
        } else {
            (27, 40)
        };
        check(
            &doc,
            &[("#a", a), ("#n", n)],
            &[
                (left, 15, BLACK, Some("/a")),
                (red, 15, RED, Some("/a")),
                (right_edge, 15, BLACK, Some("/a")),
                (gap, 15, WHITE, None),
                (next, 15, GREEN, Some("/n")),
            ],
        );
    }
}

case!(
    d1_signed_left_margin_makes_an_exact_line_fit,
    100,
    "",
    Some(60),
    "width:50px;margin-left:-10px",
    None,
    &[("#p", (20, 10, 60, 10)), ("#a", (70, 10, 50, 10))],
    &[
        (25, 15, BLUE, Some("/p")),
        (75, 15, RED, Some("/a")),
        (115, 15, RED, Some("/a")),
        (125, 15, WHITE, None)
    ]
);
case!(
    d2_one_pixel_less_wraps_the_same_signed_box,
    99,
    "",
    Some(60),
    "width:50px;margin-left:-10px",
    None,
    &[("#p", (20, 10, 60, 10)), ("#a", (10, 20, 50, 10))],
    &[
        (25, 15, BLUE, Some("/p")),
        (15, 25, RED, Some("/a")),
        (65, 25, WHITE, None)
    ]
);
case!(
    d3_negative_right_margin_preserves_border_overflow_and_overlap_order,
    105,
    "",
    Some(60),
    "width:50px;margin-right:-10px",
    Some(5),
    &[
        ("#p", (20, 10, 60, 10)),
        ("#a", (80, 10, 50, 10)),
        ("#n", (120, 10, 5, 10))
    ],
    &[
        (100, 15, RED, Some("/a")),
        (122, 15, GREEN, Some("/n")),
        (127, 15, RED, Some("/a"))
    ]
);
case!(
    d4_next_sentinel_wraps_without_shrinking_the_previous_border,
    104,
    "",
    Some(60),
    "width:50px;margin-right:-10px",
    Some(5),
    &[
        ("#p", (20, 10, 60, 10)),
        ("#a", (80, 10, 50, 10)),
        ("#n", (20, 20, 5, 10))
    ],
    &[
        (100, 15, RED, Some("/a")),
        (127, 15, RED, Some("/a")),
        (22, 25, GREEN, Some("/n")),
        (30, 25, WHITE, None)
    ]
);
case!(
    d5_negative_total_advance_is_not_clamped_to_zero,
    100,
    "",
    Some(60),
    "width:20px;margin-left:-30px",
    Some(50),
    &[
        ("#p", (20, 10, 60, 10)),
        ("#a", (50, 10, 20, 10)),
        ("#n", (70, 10, 50, 10))
    ],
    &[
        (25, 15, BLUE, Some("/p")),
        (55, 15, RED, Some("/a")),
        (75, 15, GREEN, Some("/n")),
        (115, 15, GREEN, Some("/n"))
    ]
);
case!(
    d6_positive_margin_exact_fit,
    120,
    "",
    Some(60),
    "width:50px;margin-left:10px",
    None,
    &[("#p", (20, 10, 60, 10)), ("#a", (90, 10, 50, 10))],
    &[
        (85, 15, WHITE, None),
        (95, 15, RED, Some("/a")),
        (135, 15, RED, Some("/a"))
    ]
);
case!(
    d7_positive_margin_one_pixel_over_wraps,
    119,
    "",
    Some(60),
    "width:50px;margin-left:10px",
    None,
    &[("#p", (20, 10, 60, 10)), ("#a", (30, 20, 50, 10))],
    &[
        (25, 25, WHITE, None),
        (35, 25, RED, Some("/a")),
        (75, 25, RED, Some("/a"))
    ]
);
case!(
    d8_definite_overwide_box_overflows_and_next_box_wraps,
    200,
    "",
    None,
    "width:240px",
    Some(10),
    &[("#a", (20, 10, 240, 10)), ("#n", (20, 20, 10, 10))],
    &[
        (250, 15, RED, Some("/a")),
        (265, 15, WHITE, None),
        (25, 25, GREEN, Some("/n"))
    ]
);
case!(
    d9_nowrap_keeps_definite_overflow_on_the_same_line,
    100,
    "white-space:nowrap",
    Some(60),
    "width:70px",
    None,
    &[("#p", (20, 10, 60, 10)), ("#a", (80, 10, 70, 10))],
    &[
        (125, 15, RED, Some("/a")),
        (145, 15, RED, Some("/a")),
        (155, 15, WHITE, None)
    ]
);
case!(
    d8_hidden_overflow_clips_public_geometry_pixels_and_hits_not_line_advance,
    200,
    "overflow:hidden",
    None,
    "width:240px",
    Some(10),
    &[("#a", (20, 10, 200, 10)), ("#n", (20, 20, 10, 10))],
    &[
        (215, 15, RED, Some("/a")),
        (225, 15, WHITE, None),
        (250, 15, WHITE, None),
        (25, 25, GREEN, Some("/n"))
    ]
);

#[test]
fn d_content_overflow_cannot_enlarge_the_declared_box_or_following_advance() {
    let doc = sequence(
        200,
        "",
        None,
        "width:30px;font-size:12px;line-height:12px;overflow:hidden",
        "MMMMMMMMMMMMMMMMMMMM",
        Some(10),
    );
    check(
        &doc,
        &[("#a", (20, 10, 30, 10)), ("#n", (50, 10, 10, 10))],
        &[
            (25, 15, RED, Some("/a")),
            (55, 15, GREEN, Some("/n")),
            (65, 15, WHITE, None),
        ],
    );
}

case!(
    e_center_aligns_total_line_advance,
    200,
    "text-align:center",
    None,
    "width:40px",
    Some(10),
    &[("#a", (95, 10, 40, 10)), ("#n", (135, 10, 10, 10))],
    &[
        (90, 15, WHITE, None),
        (100, 15, RED, Some("/a")),
        (140, 15, GREEN, Some("/n")),
        (150, 15, WHITE, None)
    ]
);
case!(
    e_right_aligns_total_line_advance,
    200,
    "text-align:right",
    None,
    "width:40px",
    Some(10),
    &[("#a", (170, 10, 40, 10)), ("#n", (210, 10, 10, 10))],
    &[
        (165, 15, WHITE, None),
        (175, 15, RED, Some("/a")),
        (215, 15, GREEN, Some("/n")),
        (225, 15, WHITE, None)
    ]
);
case!(
    e_positive_margin_gap_is_not_source_whitespace,
    200,
    "",
    None,
    "width:40px;margin-right:7px",
    Some(10),
    &[("#a", (20, 10, 40, 10)), ("#n", (67, 10, 10, 10))],
    &[
        (55, 15, RED, Some("/a")),
        (63, 15, WHITE, None),
        (70, 15, GREEN, Some("/n"))
    ]
);

#[test]
fn e_source_whitespace_uses_the_selected_font_space_advance_once() {
    // This is the sole deliberate inter-box whitespace in the matrix. Its
    // advance is measured by the public font API, never by candidate layout.
    let space = fonts().width(" ", 12.0);
    assert!(space.is_finite() && space >= 2.0 && space < 10.0);
    let n_x = (60.0 + space).round() as i32;
    let doc = document(
        200,
        "font-size:12px",
        &format!(
            "{} {}",
            anchor("a", "width:40px", "A"),
            anchor("n", "width:10px", "N"),
        ),
    );
    check(
        &doc,
        &[("#a", (20, 10, 40, 10)), ("#n", (n_x, 10, 10, 10))],
        &[
            (55, 15, RED, Some("/a")),
            (61, 15, WHITE, None),
            (n_x + 2, 15, GREEN, Some("/n")),
        ],
    );
}

#[test]
fn e_inline_auto_margins_are_zero_not_block_free_space_distribution() {
    for (margins, a, n, gap) in [
        (
            "margin-left:auto;margin-right:auto",
            (60, 10, 40, 10),
            (100, 10, 10, 10),
            None,
        ),
        (
            "margin-left:auto",
            (60, 10, 40, 10),
            (100, 10, 10, 10),
            None,
        ),
        (
            "margin-right:auto",
            (60, 10, 40, 10),
            (100, 10, 10, 10),
            None,
        ),
        (
            "margin-left:auto;margin-right:10px",
            (60, 10, 40, 10),
            (110, 10, 10, 10),
            Some(105),
        ),
        (
            "margin-left:10px;margin-right:auto",
            (70, 10, 40, 10),
            (110, 10, 10, 10),
            Some(65),
        ),
    ] {
        let doc = sequence(
            200,
            "",
            Some(40),
            &format!("width:40px;{margins}"),
            "A",
            Some(10),
        );
        check(
            &doc,
            &[("#p", (20, 10, 40, 10)), ("#a", a), ("#n", n)],
            &[
                (25, 15, BLUE, Some("/p")),
                (75, 15, RED, Some("/a")),
                (n.0 + 2, 15, GREEN, Some("/n")),
            ],
        );
        if let Some(x) = gap {
            check(&doc, &[], &[(x, 15, WHITE, None)]);
        }
    }
}

#[test]
fn e_text_alignment_still_positions_an_inline_auto_margin_line() {
    for (align, p, a, n) in [
        (
            "center",
            (75, 10, 40, 10),
            (115, 10, 40, 10),
            (155, 10, 10, 10),
        ),
        (
            "right",
            (130, 10, 40, 10),
            (170, 10, 40, 10),
            (210, 10, 10, 10),
        ),
    ] {
        let doc = sequence(
            200,
            &format!("text-align:{align}"),
            Some(40),
            "width:40px;margin-left:auto;margin-right:auto",
            "A",
            Some(10),
        );
        check(
            &doc,
            &[("#p", p), ("#a", a), ("#n", n)],
            &[
                (p.0 + 5, 15, BLUE, Some("/p")),
                (a.0 + 5, 15, RED, Some("/a")),
                (n.0 + 2, 15, GREEN, Some("/n")),
            ],
        );
    }
}

#[test]
fn e_definite_width_auto_height_text_measures_and_emits_the_same_three_lines() {
    // Fixed authored width50 admits one MMMM word but not a pair in the pinned
    // font. These premises fail explicitly for an unsuitable font override;
    // they never infer expected height from the renderer or a second layout.
    let mut font = fonts();
    let word = font.width("MMMM", 12.0);
    let space = font.width(" ", 12.0);
    assert!(word.is_finite() && word > 0.0 && word <= 50.0);
    assert!(word * 2.0 + space > 50.0);
    for css in [
        "width:50px;padding-left:10px;padding-right:10px",
        "box-sizing:border-box;width:70px;padding-left:10px;padding-right:10px",
    ] {
        // C70 forces N onto the following line. No relative baseline/vertical-
        // alignment claim is smuggled into the required auto-height control.
        let doc = sequence(
            70,
            "",
            None,
            &format!("{css};height:auto;font-size:12px;line-height:12px"),
            "MMMM MMMM MMMM",
            Some(10),
        );
        check(
            &doc,
            &[
                ("#a", (20, 10, 70, 36)),
                ("#n", (20, 46, 10, 10)),
                ("#container", (20, 10, 70, 46)),
            ],
            &[
                (25, 15, RED, Some("/a")),
                (85, 40, RED, Some("/a")),
                (25, 51, GREEN, Some("/n")),
                (95, 15, WHITE, None),
            ],
        );
        let a = doc.query_selector(0, "#a").unwrap().unwrap();
        let text = doc.nodes[a].children[0];
        assert_eq!(doc.nodes[text].text, "MMMM MMMM MMMM");
        for scale in [1.0, 2.0] {
            let frame = render(&doc, scale);
            let positions: Vec<_> = frame
                .boxes
                .iter()
                .filter(|item| item.node == text)
                .map(|item| (item.x, item.y, item.height))
                .collect();
            assert_eq!(positions, [(30, 10, 12), (30, 22, 12), (30, 34, 12)]);
        }
    }
}
