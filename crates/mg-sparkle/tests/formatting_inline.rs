//! Independent public controls for the adopted atomic-inline addendum.
//! Literal outer-box arithmetic, not upstream bodies or candidate output.
//! The original atomic_inline.rs controls remain a separate frozen suite.
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
const AFTER: u32 = 0x123456;
const NEXT: Option<(u32, u32)> = Some((10, 10));
const CONTEXTS: [&str; 3] = [
    "display:inline-flex;flex-direction:row",
    "display:inline-flex;flex-direction:column",
    "display:inline-grid",
];
const CHILD: &str = "<span id=child style='display:block;width:4px;height:4px'>A</span>";
const ASYMMETRIC: &str = "padding-left:10px;padding-right:20px;border-left:2px solid black;border-right:3px solid black;margin-left:7px;margin-right:11px";

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

fn document(width: u32, container_css: &str, body: &str, after: &str) -> Document {
    // Only A is a formatting root. P/N are normal atomic inline siblings in a
    // block whose width, not the wider ancestor or residual space, is the basis.
    parse(
        &format!(
            "<html><head><style>html,body{{margin:0;background:#ffffff}}*{{margin:0;padding:0;border:0;font-size:1px;line-height:0}}.atom{{display:inline-block;height:10px;text-decoration:none}}#a{{background:#aa0000;color:#aa0000}}#p{{background:#0000aa;color:#0000aa}}#n{{background:#00aa00;color:#00aa00}}</style></head><body><div style='width:320px;padding-top:10px;padding-left:20px'><div id=container style='width:{width}px;{container_css}'>{body}</div>{after}</div></body></html>"
        ),
        "https://fixture.example/",
    )
}

fn anchor(id: &str, css: &str, text: &str) -> String {
    format!("<a class=atom id={id} href=/{id} style='{css}'>{text}</a>")
}

fn sequence(
    context: &str,
    width: u32,
    container_css: &str,
    prefix: Option<u32>,
    css: &str,
    content: Option<&str>,
    next: Option<(u32, u32)>,
) -> Document {
    let p = prefix.map_or_else(String::new, |width| {
        anchor("p", &format!("width:{width}px"), "P")
    });
    let css = format!("{context};{css}");
    let a = content.map_or_else(
        || format!("<span class=atom id=a style='{css}'></span>"),
        |text| anchor("a", &css, text),
    );
    let n = next.map_or_else(String::new, |(width, height)| {
        anchor("n", &format!("width:{width}px;height:{height}px"), "N")
    });
    document(width, container_css, &format!("{p}{a}{n}"), "")
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

fn probe(document: &Document, frame: &Frame, expected: Probe) {
    let (x, y, color, path) = expected;
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
        let node = document
            .query_selector(0, &format!("#{}", path.trim_start_matches('/')))
            .unwrap()
            .unwrap();
        assert!(
            matches!(&hit.action, Action::Link { node: actual, href }
                if *actual == node && href == &format!("https://fixture.example{path}")),
            "wrong topmost hit at ({x}, {y}): {:?}",
            hit.action
        );
    } else {
        assert!(hit.is_none(), "unexpected link at ({x}, {y}): {hit:?}");
    }
}

fn check(context: &str, document: &Document, boxes: &[(&str, Rect)], pixels: &[Probe]) {
    for scale in [1.0, 2.0] {
        let frame = render(document, scale);
        for (selector, expected) in boxes {
            assert_eq!(
                geometry(rect(document, &frame, selector)),
                *expected,
                "{context}: {selector} at scale {scale}"
            );
        }
        for expected in pixels {
            probe(document, &frame, *expected);
        }
    }
}

#[test]
fn margins_are_outside_the_definite_border_for_empty_and_text_roots() {
    for context in CONTEXTS {
        for content in [None, Some("A")] {
            let doc = sequence(
                context,
                200,
                "",
                None,
                "width:16px;margin-left:2px;margin-right:2px",
                content,
                NEXT,
            );
            check(
                context,
                &doc,
                &[
                    ("#a", (22, 10, 16, 10)),
                    ("#n", (40, 10, 10, 10)),
                    ("#container", (20, 10, 200, 10)),
                ],
                &[
                    (21, 15, WHITE, None),
                    (25, 15, RED, content.map(|_| "/a")),
                    (39, 15, WHITE, None),
                    (42, 15, GREEN, Some("/n")),
                ],
            );
        }
    }
}

#[test]
fn vertical_edges_and_margins_use_outer_height_without_a_baseline_oracle() {
    // Border height12+4=16, outer height3+16+3=22. The sentinel has the
    // same outer height; no relative baseline or glyph ascent is prescribed.
    for context in CONTEXTS {
        for content in [None, Some("A")] {
            let doc = sequence(
                context,
                200,
                "",
                None,
                "width:24px;height:12px;border-bottom:4px solid black;margin:3px",
                content,
                Some((10, 22)),
            );
            let action = content.map(|_| "/a");
            check(
                context,
                &doc,
                &[
                    ("#a", (23, 13, 24, 16)),
                    ("#n", (50, 10, 10, 22)),
                    ("#container", (20, 10, 200, 22)),
                ],
                &[
                    (21, 15, WHITE, None),
                    (25, 15, RED, action),
                    (25, 26, BLACK, action),
                    (25, 30, WHITE, None),
                    (52, 15, GREEN, Some("/n")),
                ],
            );
        }
    }
}

#[test]
fn horizontal_padding_is_added_once_for_empty_text_and_fixed_child_roots() {
    for context in CONTEXTS {
        for content in [None, Some("A"), Some(CHILD)] {
            let doc = sequence(
                context,
                200,
                "",
                None,
                "width:64px;padding-left:32px;padding-right:32px",
                content,
                NEXT,
            );
            check(
                context,
                &doc,
                &[
                    ("#a", (20, 10, 128, 10)),
                    ("#n", (148, 10, 10, 10)),
                    ("#container", (20, 10, 200, 10)),
                ],
                &[
                    (140, 15, RED, content.map(|_| "/a")),
                    (147, 15, RED, content.map(|_| "/a")),
                    (150, 15, GREEN, Some("/n")),
                ],
            );
            if content == Some(CHILD) {
                check(context, &doc, &[("#child", (52, 10, 4, 4))], &[]);
            }
        }
    }
}

#[test]
fn asymmetric_content_and_border_sizing_count_each_edge_once() {
    for context in CONTEXTS {
        for (sizing, a, n, right, gap) in [
            ("content-box", (27, 10, 75, 10), (113, 10, 10, 10), 101, 108),
            ("border-box", (27, 10, 40, 10), (78, 10, 10, 10), 66, 73),
        ] {
            let doc = sequence(
                context,
                200,
                "",
                None,
                &format!("width:40px;box-sizing:{sizing};{ASYMMETRIC}"),
                Some(CHILD),
                NEXT,
            );
            check(
                context,
                &doc,
                &[
                    ("#a", a),
                    ("#child", (39, 10, 4, 4)),
                    ("#n", n),
                    ("#container", (20, 10, 200, 10)),
                ],
                &[
                    (27, 15, BLACK, Some("/a")),
                    (45, 15, RED, Some("/a")),
                    (right, 15, BLACK, Some("/a")),
                    (gap, 15, WHITE, None),
                    (n.0 + 2, 15, GREEN, Some("/n")),
                ],
            );
        }
    }
}

#[test]
fn prefixed_percentages_keep_the_real_containing_width_for_both_sizing_boxes() {
    for context in CONTEXTS {
        for (sizing, a, n, right, gap) in [
            (
                "content-box",
                (70, 10, 135, 10),
                (210, 10, 10, 10),
                204,
                208,
            ),
            ("border-box", (70, 10, 100, 10), (175, 10, 10, 10), 169, 173),
        ] {
            let doc = sequence(
                context,
                200,
                "",
                Some(40),
                &format!(
                    "width:50%;padding-left:10%;padding-right:5%;border-left:2px solid black;border-right:3px solid black;margin-left:5%;margin-right:2.5%;box-sizing:{sizing}"
                ),
                Some(CHILD),
                NEXT,
            );
            check(
                context,
                &doc,
                &[
                    ("#p", (20, 10, 40, 10)),
                    ("#a", a),
                    ("#child", (92, 10, 4, 4)),
                    ("#n", n),
                    ("#container", (20, 10, 200, 10)),
                ],
                &[
                    (25, 15, BLUE, Some("/p")),
                    (65, 15, WHITE, None),
                    (70, 15, BLACK, Some("/a")),
                    (100, 15, RED, Some("/a")),
                    (right, 15, BLACK, Some("/a")),
                    (gap, 15, WHITE, None),
                    (n.0 + 2, 15, GREEN, Some("/n")),
                ],
            );
        }
    }
}

#[test]
fn numeric_and_percentage_minmax_apply_to_the_declared_sizing_box() {
    let cases = [
        ("width:80px;max-width:50px", "content-box", 85, 123),
        ("width:20px;min-width:50px", "content-box", 85, 123),
        (
            "width:80px;min-width:70px;max-width:50px",
            "content-box",
            105,
            143,
        ),
        ("width:80px;max-width:50px", "border-box", 50, 88),
        ("width:20px;min-width:50px", "border-box", 50, 88),
        (
            "width:80px;min-width:70px;max-width:50px",
            "border-box",
            70,
            108,
        ),
    ];
    for context in CONTEXTS {
        for (sizing, box_sizing, border_width, next_x) in cases {
            let doc = sequence(
                context,
                200,
                "",
                None,
                &format!("{sizing};box-sizing:{box_sizing};{ASYMMETRIC}"),
                Some(CHILD),
                NEXT,
            );
            check(
                context,
                &doc,
                &[
                    ("#a", (27, 10, border_width, 10)),
                    ("#child", (39, 10, 4, 4)),
                    ("#n", (next_x, 10, 10, 10)),
                    ("#container", (20, 10, 200, 10)),
                ],
                &[
                    (27, 15, BLACK, Some("/a")),
                    (45, 15, RED, Some("/a")),
                    (26 + border_width as i32, 15, BLACK, Some("/a")),
                    (next_x - 3, 15, WHITE, None),
                    (next_x + 2, 15, GREEN, Some("/n")),
                ],
            );
        }
        // Min30% wins max20%, with basis200 even after a40px prefix.
        let doc = sequence(
            context,
            200,
            "",
            Some(40),
            &format!("width:25%;min-width:30%;max-width:20%;{ASYMMETRIC}"),
            Some(CHILD),
            NEXT,
        );
        check(
            context,
            &doc,
            &[
                ("#p", (20, 10, 40, 10)),
                ("#a", (67, 10, 95, 10)),
                ("#child", (79, 10, 4, 4)),
                ("#n", (173, 10, 10, 10)),
                ("#container", (20, 10, 200, 10)),
            ],
            &[
                (67, 15, BLACK, Some("/a")),
                (90, 15, RED, Some("/a")),
                (161, 15, BLACK, Some("/a")),
                (170, 15, WHITE, None),
                (175, 15, GREEN, Some("/n")),
            ],
        );
    }
}

#[test]
fn signed_advances_and_later_topmost_hits_are_not_clamped_to_border_width() {
    for context in CONTEXTS {
        let doc = sequence(
            context,
            200,
            "",
            Some(60),
            "width:20px;margin-left:-30px",
            Some("A"),
            Some((50, 10)),
        );
        check(
            context,
            &doc,
            &[
                ("#p", (20, 10, 60, 10)),
                ("#a", (50, 10, 20, 10)),
                ("#n", (70, 10, 50, 10)),
                ("#container", (20, 10, 200, 10)),
            ],
            &[
                (25, 15, BLUE, Some("/p")),
                (55, 15, RED, Some("/a")),
                (75, 15, GREEN, Some("/n")),
                (119, 15, GREEN, Some("/n")),
                (121, 15, WHITE, None),
            ],
        );
        let overlap = sequence(
            context,
            200,
            "",
            Some(60),
            "width:50px;margin-right:-10px",
            Some("A"),
            Some((5, 10)),
        );
        check(
            context,
            &overlap,
            &[
                ("#a", (80, 10, 50, 10)),
                ("#n", (120, 10, 5, 10)),
                ("#container", (20, 10, 200, 10)),
            ],
            &[
                (90, 15, RED, Some("/a")),
                (122, 15, GREEN, Some("/n")),
                (127, 15, RED, Some("/a")),
            ],
        );
    }
}

#[test]
fn inline_auto_margins_are_zero_without_disabling_parent_text_alignment() {
    for context in CONTEXTS {
        for (margins, a_x, n_x, gap) in [
            ("margin-left:auto;margin-right:auto", 60, 100, None),
            ("margin-left:auto;margin-right:10px", 60, 110, Some(105)),
            ("margin-left:10px;margin-right:auto", 70, 110, Some(65)),
        ] {
            let doc = sequence(
                context,
                200,
                "",
                Some(40),
                &format!("width:40px;{margins}"),
                Some("A"),
                NEXT,
            );
            check(
                context,
                &doc,
                &[
                    ("#p", (20, 10, 40, 10)),
                    ("#a", (a_x, 10, 40, 10)),
                    ("#n", (n_x, 10, 10, 10)),
                    ("#container", (20, 10, 200, 10)),
                ],
                &[
                    (a_x + 5, 15, RED, Some("/a")),
                    (n_x + 2, 15, GREEN, Some("/n")),
                ],
            );
            if let Some(x) = gap {
                check(context, &doc, &[], &[(x, 15, WHITE, None)]);
            }
        }
        for (alignment, a_x, n_x) in [("center", 95, 135), ("right", 170, 210)] {
            let doc = sequence(
                context,
                200,
                &format!("text-align:{alignment}"),
                None,
                "width:40px;margin-left:auto;margin-right:auto",
                Some("A"),
                NEXT,
            );
            check(
                context,
                &doc,
                &[
                    ("#a", (a_x, 10, 40, 10)),
                    ("#n", (n_x, 10, 10, 10)),
                    ("#container", (20, 10, 200, 10)),
                ],
                &[
                    (a_x - 1, 15, WHITE, None),
                    (a_x + 5, 15, RED, Some("/a")),
                    (n_x + 2, 15, GREEN, Some("/n")),
                ],
            );
        }
    }
}

#[test]
fn wrapping_and_overflow_keep_declared_width_and_signed_outer_admission() {
    for context in CONTEXTS {
        let overwide = sequence(context, 200, "", None, "width:240px", Some("A"), NEXT);
        check(
            context,
            &overwide,
            &[
                ("#a", (20, 10, 240, 10)),
                ("#n", (20, 20, 10, 10)),
                ("#container", (20, 10, 200, 20)),
            ],
            &[
                (250, 15, RED, Some("/a")),
                (265, 15, WHITE, None),
                (25, 25, GREEN, Some("/n")),
            ],
        );
        for (width, a_y, flow_height) in [(120, 10, 10), (119, 20, 20)] {
            let doc = sequence(
                context,
                width,
                "",
                Some(60),
                "width:50px;margin-left:10px",
                Some("A"),
                None,
            );
            let a_x = if width == 120 { 90 } else { 30 };
            check(
                context,
                &doc,
                &[
                    ("#p", (20, 10, 60, 10)),
                    ("#a", (a_x, a_y, 50, 10)),
                    ("#container", (20, 10, width, flow_height)),
                ],
                &[
                    (25, 15, BLUE, Some("/p")),
                    (a_x + 5, a_y + 5, RED, Some("/a")),
                    (a_x + 5, a_y + 11, WHITE, None),
                ],
            );
        }
        for (width, n_x, n_y, flow_height) in [(105, 120, 10, 10), (104, 20, 20, 20)] {
            let doc = sequence(
                context,
                width,
                "",
                Some(60),
                "width:50px;margin-right:-10px",
                Some("A"),
                Some((5, 10)),
            );
            check(
                context,
                &doc,
                &[
                    ("#a", (80, 10, 50, 10)),
                    ("#n", (n_x, n_y, 5, 10)),
                    ("#container", (20, 10, width, flow_height)),
                ],
                &[
                    (90, 15, RED, Some("/a")),
                    (n_x + 2, n_y + 5, GREEN, Some("/n")),
                ],
            );
        }
    }
}

#[test]
fn fixed_child_auto_height_controls_the_real_line_metrics_and_following_flow() {
    // The definite200px border occupies its whole line. A single fixed child
    // makes its auto height12; N must start the next line at22, not a guessed
    // font strut/replaced height. Equal12px outer heights add no baseline rule.
    for context in CONTEXTS {
        let a = anchor(
            "a",
            &format!("{context};width:180px;padding-left:10px;padding-right:10px;height:auto"),
            "<span id=child style='display:block;width:4px;height:12px'>X</span>",
        );
        let n = anchor("n", "width:10px;height:12px", "N");
        let doc = document(
            200,
            "",
            &format!("{a}{n}"),
            "<div id=after style='width:10px;height:3px;background:#123456'></div>",
        );
        check(
            context,
            &doc,
            &[
                ("#a", (20, 10, 200, 12)),
                ("#child", (30, 10, 4, 12)),
                ("#n", (20, 22, 10, 12)),
                ("#container", (20, 10, 200, 24)),
                ("#after", (20, 34, 10, 3)),
            ],
            &[
                (100, 15, RED, Some("/a")),
                (219, 21, RED, Some("/a")),
                (25, 25, GREEN, Some("/n")),
                (35, 25, WHITE, None),
                (25, 35, AFTER, None),
                (25, 38, WHITE, None),
            ],
        );
    }
}
