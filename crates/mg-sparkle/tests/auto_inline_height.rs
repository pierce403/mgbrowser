//! Authored public controls for docs/AUTO_INLINE_HEIGHT.md.
//! Heights are frozen box arithmetic, not upstream or candidate output.
//! Existing atomic-inline and formatting-inline tests remain unchanged.
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
const CONTEXTS: [&str; 4] = [
    "display:inline-block",
    "display:inline-flex;flex-direction:row",
    "display:inline-flex;flex-direction:column",
    "display:inline-grid",
];
const VERTICAL_EDGES: &str = "padding-top:2px;padding-bottom:4px;border-top:1px solid black;border-bottom:1px solid black;margin-top:3px;margin-bottom:5px";
const CHILD: &str = "<a id=child class=child href=/child>C</a>";

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

fn document(width: u32, line_css: &str, body: &str) -> Document {
    // The line content starts at(22,32). The following block starts outside the
    // atomic paint's horizontal range so it cannot conceal vertical overflow.
    // There is no source whitespace between atomic siblings or before AFTER.
    parse(
        &format!(
            "<!doctype html><html><head><style>html,body{{margin:0;background:#ffffff}}*{{margin:0;padding:0;border:0;font-size:1px;line-height:0}}a{{text-decoration:none}}.atom{{background:#aa0000}}.child{{display:block;width:30px;height:12px;flex:none;background:#00aa00;color:#00aa00}}#after{{display:block;width:10px;height:3px;margin-left:50px;background:#123456;color:#123456}}</style></head><body><div id=stage style='width:300px;padding-left:20px;padding-top:30px'><div id=line style='width:{width}px;border:2px solid black;{line_css}'>{body}</div><a id=after href=/after>N</a></div></body></html>"
        ),
        "https://fixture.example/",
    )
}

fn atom(id: &str, context: &str, css: &str, content: &str) -> String {
    format!("<span id={id} class=atom style='{context};{css}'>{content}</span>")
}

fn single(context: &str, css: &str, line_css: &str, content: &str) -> Document {
    document(200, line_css, &atom("a", context, css, content))
}

fn render(document: &Document, scale: f32) -> Frame {
    let frame = render_scaled(
        document,
        &mut fonts(),
        Viewport {
            width: 384,
            height: 192,
            scroll: 0,
        },
        &Controls::default(),
        scale,
    );
    assert_eq!(frame.diagnostics.omitted, 0);
    assert!(
        frame.diagnostics.entries.is_empty(),
        "authored height controls require diagnostic-free styled layout: {:?}",
        frame.diagnostics
    );
    assert_eq!(frame.canvas.width, (384.0 * scale) as u32);
    assert_eq!(frame.canvas.height, (192.0 * scale) as u32);
    frame
}

fn rect<'a>(document: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let node = document.query_selector(0, selector).unwrap().unwrap();
    frame
        .boxes
        .iter()
        .rev()
        .find(|item| item.node == node)
        .unwrap_or_else(|| panic!("missing public box for {selector}"))
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
    let px = frame.canvas.physical_edge(i64::from(x)) as usize;
    let py = frame.canvas.physical_edge(i64::from(y)) as usize;
    assert!(px < frame.canvas.width as usize && py < frame.canvas.height as usize);
    assert_eq!(
        frame.canvas.pixels[py * frame.canvas.width as usize + px],
        color,
        "pixel({x},{y}) at scale {}",
        frame.canvas.scale()
    );
    let hit = frame.hits.iter().rev().find(|hit| contains(hit, x, y));
    if let Some(path) = path {
        let hit = hit.unwrap_or_else(|| panic!("missing topmost link at({x},{y})"));
        let node = document
            .query_selector(0, &format!("#{}", path.trim_start_matches('/')))
            .unwrap()
            .unwrap();
        assert!(
            matches!(&hit.action, Action::Link { node: actual, href }
                if *actual == node && href == &format!("https://fixture.example{path}")),
            "wrong topmost link at({x},{y}): {:?}",
            hit.action
        );
    } else {
        assert!(hit.is_none(), "unexpected link at({x},{y}): {hit:?}");
    }
}

fn check(label: &str, document: &Document, boxes: &[(&str, Rect)], pixels: &[Probe]) {
    for scale in [1.0, 2.0] {
        let frame = render(document, scale);
        for (selector, expected) in boxes {
            assert_eq!(
                geometry(rect(document, &frame, selector)),
                *expected,
                "{label}: {selector} at scale {scale}"
            );
        }
        for expected in pixels {
            probe(document, &frame, *expected);
        }
    }
}

#[test]
fn auto_width_fixed_child_sets_parent_border_and_following_block_advance() {
    for context in CONTEXTS {
        let doc = single(context, "", "", CHILD);
        check(
            context,
            &doc,
            &[
                ("#a", (22, 32, 30, 12)),
                ("#child", (22, 32, 30, 12)),
                ("#line", (20, 30, 204, 16)),
                ("#after", (70, 46, 10, 3)),
            ],
            &[
                (20, 35, BLACK, None),
                (25, 35, GREEN, Some("/child")),
                (60, 35, WHITE, None),
                (72, 47, AFTER, Some("/after")),
                (72, 49, WHITE, None),
            ],
        );
    }
}

#[test]
fn vertical_padding_borders_and_margins_contribute_twenty_eight_once() {
    // Content12 + padding2/4 + borders1/1 = border20; margins3/5 => outer28.
    for context in CONTEXTS {
        let doc = single(context, VERTICAL_EDGES, "", CHILD);
        check(
            context,
            &doc,
            &[
                ("#a", (22, 35, 30, 20)),
                ("#child", (22, 38, 30, 12)),
                ("#line", (20, 30, 204, 32)),
                ("#after", (70, 62, 10, 3)),
            ],
            &[
                (25, 34, WHITE, None),
                (25, 35, BLACK, None),
                (25, 37, RED, None),
                (25, 40, GREEN, Some("/child")),
                (25, 54, BLACK, None),
                (25, 57, WHITE, None),
                (72, 63, AFTER, Some("/after")),
            ],
        );
    }
}

#[test]
fn specified_height_not_the_overflow_paint_envelope_sets_the_line() {
    for context in CONTEXTS {
        let doc = single(context, "height:7px", "", CHILD);
        check(
            context,
            &doc,
            &[
                ("#a", (22, 32, 30, 7)),
                ("#child", (22, 32, 30, 12)),
                ("#line", (20, 30, 204, 11)),
                ("#after", (70, 41, 10, 3)),
            ],
            &[
                (25, 35, GREEN, Some("/child")),
                (25, 42, GREEN, Some("/child")),
                (25, 44, WHITE, None),
                (72, 42, AFTER, Some("/after")),
            ],
        );
    }
}

#[test]
fn numeric_height_constraints_and_minimum_precedence_preserve_child_overflow() {
    for context in CONTEXTS {
        for (css, height, after_y) in [
            ("min-height:18px", 18, 52),
            ("max-height:8px", 8, 42),
            ("min-height:18px;max-height:8px", 18, 52),
        ] {
            let doc = single(context, css, "", CHILD);
            check(
                &format!("{context};{css}"),
                &doc,
                &[
                    ("#a", (22, 32, 30, height)),
                    ("#child", (22, 32, 30, 12)),
                    ("#line", (20, 30, 204, height + 4)),
                    ("#after", (70, after_y, 10, 3)),
                ],
                &[
                    (25, 35, GREEN, Some("/child")),
                    (25, 43, GREEN, Some("/child")),
                    (72, after_y + 1, AFTER, Some("/after")),
                ],
            );
        }
    }
}

#[test]
fn explicit_parent_strut_keeps_existing_bottom_alignment() {
    // Existing behavior: outer12 is shifted by20-12=8, not top/baseline aligned.
    for context in CONTEXTS {
        let doc = single(context, "", "line-height:20px", CHILD);
        check(
            context,
            &doc,
            &[
                ("#a", (22, 40, 30, 12)),
                ("#child", (22, 40, 30, 12)),
                ("#line", (20, 30, 204, 24)),
                ("#after", (70, 54, 10, 3)),
            ],
            &[
                (25, 35, WHITE, None),
                (25, 42, GREEN, Some("/child")),
                (72, 55, AFTER, Some("/after")),
            ],
        );
    }
}

#[test]
fn signed_vertical_margins_change_placement_without_a_negative_line_height() {
    // The positive origin makes the above-line border visible, not clipped.
    // Tuple: CSS, border-y, nonnegative line contribution, following block-y.
    for context in CONTEXTS {
        for (css, border_y, contribution, after_y) in [
            ("margin-top:3px;margin-bottom:5px", 35, 20, 54),
            ("margin-top:-3px;margin-bottom:-2px", 29, 7, 41),
            ("margin-top:-9px;margin-bottom:-8px", 23, 0, 34),
        ] {
            let doc = single(context, css, "", CHILD);
            check(
                &format!("{context};{css}"),
                &doc,
                &[
                    ("#a", (22, border_y, 30, 12)),
                    ("#child", (22, border_y, 30, 12)),
                    ("#line", (20, 30, 204, contribution + 4)),
                    ("#after", (70, after_y, 10, 3)),
                ],
                &[
                    (25, border_y + 2, GREEN, Some("/child")),
                    (72, after_y + 1, AFTER, Some("/after")),
                ],
            );
        }
    }
}

#[test]
fn strut_shifts_negative_outer_height_after_the_existing_zero_floor() {
    // Signed outer12-9-8=-5 clamps to0 for line alignment, so the shift is20.
    for context in CONTEXTS {
        let doc = single(
            context,
            "margin-top:-9px;margin-bottom:-8px",
            "line-height:20px",
            CHILD,
        );
        check(
            context,
            &doc,
            &[
                ("#a", (22, 43, 30, 12)),
                ("#child", (22, 43, 30, 12)),
                ("#line", (20, 30, 204, 24)),
                ("#after", (70, 54, 10, 3)),
            ],
            &[
                (25, 35, WHITE, None),
                (25, 45, GREEN, Some("/child")),
                (72, 55, AFTER, Some("/after")),
            ],
        );
    }
}

#[test]
fn empty_auto_width_roots_have_zero_content_height_but_keep_vertical_edges() {
    for context in CONTEXTS {
        let empty = single(context, "", "", "");
        check(
            context,
            &empty,
            &[
                ("#a", (22, 32, 0, 0)),
                ("#line", (20, 30, 204, 4)),
                ("#after", (70, 34, 10, 3)),
            ],
            &[(25, 35, WHITE, None), (72, 35, AFTER, Some("/after"))],
        );
        // Width remains0: public boxes prove height8/outer16, not a new width.
        let edges = single(context, VERTICAL_EDGES, "", "");
        check(
            context,
            &edges,
            &[
                ("#a", (22, 35, 0, 8)),
                ("#line", (20, 30, 204, 20)),
                ("#after", (70, 50, 10, 3)),
            ],
            &[(25, 40, WHITE, None), (72, 51, AFTER, Some("/after"))],
        );
    }
}

#[test]
fn absolute_child_paint_does_not_establish_auto_content_height() {
    for context in CONTEXTS {
        let doc = single(
            context,
            "position:relative",
            "",
            "<a id=floating href=/floating style='display:block;position:absolute;left:4px;top:5px;width:30px;height:12px;background:#0000aa;color:#0000aa'>F</a>",
        );
        check(
            context,
            &doc,
            &[
                ("#a", (22, 32, 0, 0)),
                ("#floating", (26, 37, 30, 12)),
                ("#line", (20, 30, 204, 4)),
                ("#after", (70, 34, 10, 3)),
            ],
            &[
                (30, 40, BLUE, Some("/floating")),
                (72, 35, AFTER, Some("/after")),
                (60, 40, WHITE, None),
            ],
        );
    }
}

#[test]
fn exact_fit_and_wrapped_auto_width_siblings_use_twelve_pixel_lines() {
    for context in CONTEXTS {
        let a = atom(
            "a",
            context,
            "",
            "<a id=ac class=child href=/ac style='background:#aa0000;color:#aa0000'>A</a>",
        );
        let b = atom("b", context, "", "<a id=bc class=child href=/bc>B</a>");
        for (width, bx, by, line_height, after_y) in [(60, 52, 32, 12, 46), (59, 22, 44, 24, 58)] {
            let doc = document(width, "", &format!("{a}{b}"));
            check(
                &format!("{context};width:{width}"),
                &doc,
                &[
                    ("#a", (22, 32, 30, 12)),
                    ("#ac", (22, 32, 30, 12)),
                    ("#b", (bx, by, 30, 12)),
                    ("#bc", (bx, by, 30, 12)),
                    ("#line", (20, 30, width + 4, line_height + 4)),
                    ("#after", (70, after_y, 10, 3)),
                ],
                &[
                    (25, 35, RED, Some("/ac")),
                    (bx + 3, by + 3, GREEN, Some("/bc")),
                    (72, after_y + 1, AFTER, Some("/after")),
                ],
            );
        }
    }
}

#[test]
fn nested_auto_width_line_metrics_reach_the_outer_parent_and_following_flow() {
    for context in CONTEXTS {
        // Holder's normal inline flow contains another auto-width atomic root;
        // no text strut or definite width on INNER can conceal its line height.
        let inner = atom("inner", context, VERTICAL_EDGES, CHILD);
        let content = format!("<span id=holder style='display:block;width:30px'>{inner}</span>");
        let doc = single(context, "", "", &content);
        check(
            context,
            &doc,
            &[
                ("#a", (22, 32, 30, 28)),
                ("#holder", (22, 32, 30, 28)),
                ("#inner", (22, 35, 30, 20)),
                ("#child", (22, 38, 30, 12)),
                ("#line", (20, 30, 204, 32)),
                ("#after", (70, 62, 10, 3)),
            ],
            &[
                (25, 35, BLACK, None),
                (25, 40, GREEN, Some("/child")),
                (25, 57, RED, None),
                (72, 63, AFTER, Some("/after")),
            ],
        );
    }
}

#[test]
fn ordinary_inline_block_percentage_height_uses_only_a_definite_parent() {
    // A parent's min-height40 is not a definite height. Only height40 supplies
    // Some40, so A's50% is20 rather than its auto content height12.
    for (line_css, a_height, line_height, after_y) in [
        ("", 12, 12, 46),
        ("min-height:40px", 12, 40, 74),
        ("height:40px", 20, 40, 74),
    ] {
        let doc = single("display:inline-block", "height:50%", line_css, CHILD);
        check(
            line_css,
            &doc,
            &[
                ("#a", (22, 32, 30, a_height)),
                ("#child", (22, 32, 30, 12)),
                ("#line", (20, 30, 204, line_height + 4)),
                ("#after", (70, after_y, 10, 3)),
            ],
            &[
                (25, 35, GREEN, Some("/child")),
                (72, after_y + 1, AFTER, Some("/after")),
            ],
        );
    }
}

#[test]
fn auto_height_and_minimum_do_not_make_a_percentage_descendant_definite() {
    let content = "<span id=percent style='display:block;width:30px;height:50%'><a id=child class=child href=/child>C</a></span>";
    for (css, a_height, after_y) in [("", 12, 46), ("min-height:40px", 40, 74)] {
        let doc = single("display:inline-block", css, "", content);
        check(
            css,
            &doc,
            &[
                ("#a", (22, 32, 30, a_height)),
                ("#percent", (22, 32, 30, 12)),
                ("#child", (22, 32, 30, 12)),
                ("#line", (20, 30, 204, a_height + 4)),
                ("#after", (70, after_y, 10, 3)),
            ],
            &[
                (25, 35, GREEN, Some("/child")),
                (72, after_y + 1, AFTER, Some("/after")),
            ],
        );
    }
}
