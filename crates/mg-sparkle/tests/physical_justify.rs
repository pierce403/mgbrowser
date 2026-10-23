//! Authored public-render controls for the frozen physical-justification scope.
//! No upstream bytes, script runtime, network resources or WPT pass claims.
use mg_sparkle::{
    document::{Document, parse},
    paint::Fonts,
    render::{Action, Controls, Frame, LayoutBox, Viewport, render_scaled},
};

type Rect = (i32, i32, u32, u32);

const RED: u32 = 0xaa0000;
const GREEN: u32 = 0x00aa00;
const BLUE: u32 = 0x0000aa;
const SAFE_REVERSE_REASON: &str = "safe physical justification in reverse flex is not implemented";

fn document(body: &str) -> Document {
    parse(
        &format!(
            "<html><head><style>html,body{{margin:0}} *{{margin:0;padding:0;border:0;font-size:12px;line-height:20px}} a{{display:block;text-decoration:none}} table{{border-spacing:0}} td{{vertical-align:top}}</style></head><body>{body}</body></html>"
        ),
        "https://fixture.example/",
    )
}

fn render(document: &Document, scale: f32) -> Frame {
    let mut fonts = Fonts::from_bytes(
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap(),
    )
    .unwrap();
    render_scaled(
        document,
        &mut fonts,
        Viewport {
            width: 360,
            height: 240,
            scroll: 0,
        },
        &Controls::default(),
        scale,
    )
}

fn clean_frame(document: &Document, scale: f32) -> Frame {
    let frame = render(document, scale);
    assert_eq!(frame.diagnostics.omitted, 0);
    assert!(
        frame.diagnostics.entries.is_empty(),
        "authored geometry must use diagnostic-free styled layout: {:?}",
        frame.diagnostics
    );
    frame
}

fn rect<'a>(document: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let node = document.query_selector(0, selector).unwrap().unwrap();
    frame
        .boxes
        .iter()
        .rev()
        .find(|item| item.node == node)
        .unwrap()
}

fn geometry(item: &LayoutBox) -> Rect {
    (item.x, item.y, item.width, item.height)
}

fn pixel(frame: &Frame, x: i32, y: i32) -> u32 {
    assert!(x >= 0 && y >= 0);
    let x = frame.canvas.physical_edge(i64::from(x)) as usize;
    let y = frame.canvas.physical_edge(i64::from(y)) as usize;
    assert!(x < frame.canvas.width as usize && y < frame.canvas.height as usize);
    frame.canvas.pixels[y * frame.canvas.width as usize + x]
}

fn assert_hit(frame: &Frame, path: &str, x: i32, y: i32) {
    let hit = frame
        .hits
        .iter()
        .rev()
        .find(|hit| {
            x >= hit.x
                && y >= hit.y
                && i64::from(x) < i64::from(hit.x) + i64::from(hit.w)
                && i64::from(y) < i64::from(hit.y) + i64::from(hit.h)
        })
        .unwrap_or_else(|| panic!("no link at ({x}, {y})"));
    assert!(
        matches!(&hit.action, Action::Link { href, .. } if href == &format!("https://fixture.example{path}")),
        "wrong topmost action at ({x}, {y}): {:?}",
        hit.action
    );
}

fn assert_no_hit(frame: &Frame, x: i32, y: i32) {
    assert!(!frame.hits.iter().any(|hit| {
        x >= hit.x
            && y >= hit.y
            && i64::from(x) < i64::from(hit.x) + i64::from(hit.w)
            && i64::from(y) < i64::from(hit.y) + i64::from(hit.h)
    }));
}

fn assert_item(document: &Document, frame: &Frame, id: &str, expected: Rect, color: u32) {
    assert_eq!(
        geometry(rect(document, frame, &format!("#{id}"))),
        expected,
        "{id}"
    );
    let x = expected.0 + (expected.2 / 2) as i32;
    let y = expected.1 + (expected.3 / 2) as i32;
    assert_eq!(pixel(frame, x, y), color, "{id}");
    assert_hit(frame, &format!("/{id}"), x, y);
}

fn pair(container: &str, a: &str, b: &str) -> Document {
    document(&format!(
        "<div id=container style='{container}'><a id=a href=/a style='background:#aa0000;{a}'></a><a id=b href=/b style='background:#00aa00;{b}'></a></div>"
    ))
}

#[test]
fn physical_edges_do_not_reverse_with_flex_item_order() {
    // Literal rectangles are the independent geometry oracle, not a reimplementation
    // of the adapter's keyword resolver. A and B differ on both axes.
    let cases = [
        ("row", "left", (0, 0, 20, 20), (20, 0, 30, 30)),
        ("row", "right", (50, 0, 20, 20), (70, 0, 30, 30)),
        ("row-reverse", "left", (30, 0, 20, 20), (0, 0, 30, 30)),
        ("row-reverse", "right", (80, 0, 20, 20), (50, 0, 30, 30)),
        ("column", "left", (0, 0, 20, 20), (0, 20, 30, 30)),
        ("column", "right", (0, 0, 20, 20), (0, 20, 30, 30)),
        ("column-reverse", "left", (0, 30, 20, 20), (0, 0, 30, 30)),
        ("column-reverse", "right", (0, 30, 20, 20), (0, 0, 30, 30)),
    ];
    for display in ["flex", "inline-flex"] {
        for (direction, keyword, a, b) in cases {
            let doc = pair(
                &format!(
                    "display:{display};vertical-align:top;width:100px;height:100px;align-items:flex-start;flex-direction:{direction};justify-content:{keyword}"
                ),
                "width:20px;height:20px;flex-shrink:0",
                "width:30px;height:30px;flex-shrink:0",
            );
            let frame = clean_frame(&doc, 1.0);
            assert_eq!(geometry(rect(&doc, &frame, "#container")), (0, 0, 100, 100));
            assert_item(&doc, &frame, "a", a, RED);
            assert_item(&doc, &frame, "b", b, GREEN);
        }
    }
}

#[test]
fn one_item_and_zero_free_space_keep_physical_edges() {
    for (direction, keyword, expected) in [
        ("row", "left", (0, 0, 20, 20)),
        ("row", "right", (80, 0, 20, 20)),
        ("row-reverse", "left", (0, 0, 20, 20)),
        ("row-reverse", "right", (80, 0, 20, 20)),
        ("column", "left", (0, 0, 20, 20)),
        ("column", "right", (0, 0, 20, 20)),
        ("column-reverse", "left", (0, 0, 20, 20)),
        ("column-reverse", "right", (0, 0, 20, 20)),
    ] {
        let doc = document(&format!(
            "<div style='display:flex;width:100px;height:100px;align-items:flex-start;flex-direction:{direction};justify-content:{keyword}'><a id=a href=/a style='width:20px;height:20px;flex-shrink:0;background:#aa0000'></a></div>"
        ));
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", expected, RED);
    }
    for keyword in ["left", "right"] {
        let doc = pair(
            &format!(
                "display:flex;width:100px;height:40px;flex-direction:row-reverse;justify-content:{keyword}"
            ),
            "width:40px;height:20px;flex-shrink:0",
            "width:60px;height:20px;flex-shrink:0",
        );
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (60, 0, 40, 20), RED);
        assert_item(&doc, &frame, "b", (0, 0, 60, 20), GREEN);
    }
}

#[test]
fn physical_group_padding_border_gap_pixels_and_hits_scale_together() {
    for (keyword, a, b, gap_x) in [("left", 17, 47, 42), ("right", 57, 87, 82)] {
        let doc = pair(
            &format!(
                "display:flex;width:100px;height:40px;margin-left:10px;margin-top:12px;padding:5px;border:2px solid #222222;gap:10px;justify-content:{keyword};align-items:flex-start"
            ),
            "width:20px;height:20px;flex-shrink:0",
            "width:30px;height:20px;flex-shrink:0",
        );
        let mut logical = None;
        for scale in [1.0, 1.25, 2.0] {
            let frame = clean_frame(&doc, scale);
            assert_eq!(
                geometry(rect(&doc, &frame, "#container")),
                (10, 12, 114, 54)
            );
            assert_item(&doc, &frame, "a", (a, 19, 20, 20), RED);
            assert_item(&doc, &frame, "b", (b, 19, 30, 20), GREEN);
            assert_eq!(pixel(&frame, 11, 13), 0x222222);
            assert_eq!(pixel(&frame, 13, 15), 0xffffff);
            assert_eq!(pixel(&frame, gap_x, 25), 0xffffff);
            assert_no_hit(&frame, gap_x, 25);
            assert_eq!(frame.canvas.width, (360.0 * scale).round() as u32);
            assert_eq!(frame.canvas.height, (240.0 * scale).round() as u32);
            let value = (frame.boxes.clone(), format!("{:?}", frame.hits));
            if let Some(before) = &logical {
                assert_eq!(&value, before, "scale must not change logical geometry");
            } else {
                logical = Some(value);
            }
        }
    }
}

#[test]
fn forward_row_safe_overflow_is_distinct_from_default_and_unsafe_right() {
    for (value, a, b) in [
        ("left", 60, 130),
        ("unsafe left", 60, 130),
        ("safe left", 60, 130),
        ("right", 10, 80),
        ("unsafe right", 10, 80),
        ("safe right", 60, 130),
    ] {
        let doc = pair(
            &format!(
                "display:flex;width:100px;height:30px;margin-left:60px;justify-content:{value}"
            ),
            "width:70px;height:20px;flex-shrink:0",
            "width:80px;height:20px;flex-shrink:0",
        );
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (a, 0, 70, 20), RED);
        assert_item(&doc, &frame, "b", (b, 0, 80, 20), GREEN);
    }
}

#[test]
fn fitting_safe_and_unsafe_values_retain_requested_flex_and_grid_edges() {
    for (value, a, b) in [
        ("left", 0, 20),
        ("unsafe left", 0, 20),
        ("safe left", 0, 20),
        ("right", 50, 70),
        ("unsafe right", 50, 70),
        ("safe right", 50, 70),
    ] {
        for display in ["flex", "grid"] {
            let doc = pair(
                &format!(
                    "display:{display};width:100px;height:40px;grid-template-columns:20px 30px;grid-template-rows:30px;justify-content:{value};align-items:start"
                ),
                "width:20px;height:20px;flex-shrink:0",
                "width:30px;height:30px;flex-shrink:0",
            );
            let frame = clean_frame(&doc, 1.0);
            assert_item(&doc, &frame, "a", (a, 0, 20, 20), RED);
            assert_item(&doc, &frame, "b", (b, 0, 30, 30), GREEN);
        }
    }
}

#[test]
fn reversed_row_unsafe_overflow_keeps_physical_group_edges() {
    for (value, a, b) in [
        ("left", 140, 60),
        ("unsafe left", 140, 60),
        ("right", 90, 10),
        ("unsafe right", 90, 10),
    ] {
        let doc = pair(
            &format!(
                "display:flex;width:100px;height:30px;margin-left:60px;flex-direction:row-reverse;justify-content:{value}"
            ),
            "width:70px;height:20px;flex-shrink:0",
            "width:80px;height:20px;flex-shrink:0",
        );
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (a, 0, 70, 20), RED);
        assert_item(&doc, &frame, "b", (b, 0, 80, 20), GREEN);
    }
}

#[test]
fn column_physical_keywords_pack_at_top_with_nonshrinking_overflow() {
    for value in [
        "left",
        "right",
        "safe left",
        "safe right",
        "unsafe left",
        "unsafe right",
    ] {
        let doc = pair(
            &format!(
                "display:flex;width:40px;height:100px;flex-direction:column;justify-content:{value};align-items:flex-start"
            ),
            "width:20px;height:70px;flex-shrink:0",
            "width:30px;height:80px;flex-shrink:0",
        );
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (0, 0, 20, 70), RED);
        assert_item(&doc, &frame, "b", (0, 70, 30, 80), GREEN);
    }
    for value in ["left", "right", "unsafe left", "unsafe right"] {
        let doc = pair(
            &format!(
                "display:flex;width:40px;height:100px;flex-direction:column-reverse;justify-content:{value};align-items:flex-start"
            ),
            "width:20px;height:70px;flex-shrink:0",
            "width:30px;height:80px;flex-shrink:0",
        );
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (0, 80, 20, 70), RED);
        assert_item(&doc, &frame, "b", (0, 0, 30, 80), GREEN);
    }
}

#[test]
fn overflow_clipping_moves_public_boxes_paint_and_hits_together() {
    let doc = pair(
        "display:flex;width:100px;height:30px;margin-left:60px;justify-content:unsafe right;overflow:hidden",
        "width:70px;height:20px;flex-shrink:0",
        "width:80px;height:20px;flex-shrink:0",
    );
    let frame = clean_frame(&doc, 1.0);
    assert_eq!(geometry(rect(&doc, &frame, "#a")), (60, 0, 20, 20));
    assert_eq!(geometry(rect(&doc, &frame, "#b")), (80, 0, 80, 20));
    assert_eq!(pixel(&frame, 20, 10), 0xffffff);
    assert_no_hit(&frame, 20, 10);
    assert_eq!(pixel(&frame, 65, 10), RED);
    assert_hit(&frame, "/a", 65, 10);
    assert_eq!(pixel(&frame, 120, 10), GREEN);
    assert_hit(&frame, "/b", 120, 10);
}

#[test]
fn auto_main_margins_take_positive_free_space_before_physical_justification() {
    for keyword in ["left", "right"] {
        for (direction, a_css, b_css, a_x, b_x) in [
            ("row", "margin-left:auto", "", 50, 70),
            ("row", "margin-left:auto;margin-right:auto", "", 25, 70),
            ("row", "margin-left:auto", "margin-right:auto", 25, 45),
            ("row-reverse", "margin-left:auto", "", 80, 0),
        ] {
            let doc = pair(
                &format!(
                    "display:flex;width:100px;height:30px;flex-direction:{direction};justify-content:{keyword}"
                ),
                &format!("width:20px;height:20px;flex-shrink:0;{a_css}"),
                &format!("width:30px;height:20px;flex-shrink:0;{b_css}"),
            );
            let frame = clean_frame(&doc, 1.0);
            assert_item(&doc, &frame, "a", (a_x, 0, 20, 20), RED);
            assert_item(&doc, &frame, "b", (b_x, 0, 30, 20), GREEN);
        }
    }
}

#[test]
fn overflowing_auto_main_margin_stays_zero_instead_of_absorbing_negative_space() {
    for (keyword, a, b) in [("left", 60, 130), ("right", 10, 80)] {
        let doc = pair(
            &format!(
                "display:flex;width:100px;height:30px;margin-left:60px;justify-content:{keyword}"
            ),
            "width:70px;height:20px;flex-shrink:0;margin-left:auto",
            "width:80px;height:20px;flex-shrink:0",
        );
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (a, 0, 70, 20), RED);
        assert_item(&doc, &frame, "b", (b, 0, 80, 20), GREEN);
    }
}

#[test]
fn wrapped_lines_justify_independently_and_wrap_reverse_only_changes_cross_order() {
    for (direction, wrap, keyword, a, b, c) in [
        ("row", "wrap", "left", (0, 0), (45, 0), (0, 30)),
        ("row", "wrap", "right", (25, 0), (70, 0), (50, 30)),
        ("row-reverse", "wrap", "left", (35, 0), (0, 0), (0, 30)),
        ("row-reverse", "wrap", "right", (60, 0), (25, 0), (50, 30)),
        ("row", "wrap-reverse", "left", (0, 30), (45, 30), (0, 0)),
        ("row", "wrap-reverse", "right", (25, 30), (70, 30), (50, 0)),
        (
            "row-reverse",
            "wrap-reverse",
            "left",
            (35, 30),
            (0, 30),
            (0, 0),
        ),
        (
            "row-reverse",
            "wrap-reverse",
            "right",
            (60, 30),
            (25, 30),
            (50, 0),
        ),
    ] {
        let doc = document(&format!(
            "<div style='display:flex;width:100px;height:50px;gap:10px 5px;flex-direction:{direction};flex-wrap:{wrap};align-content:flex-start;align-items:flex-start;justify-content:{keyword}'><a id=a href=/a style='width:40px;height:20px;flex-shrink:0;background:#aa0000'></a><a id=b href=/b style='width:30px;height:20px;flex-shrink:0;background:#00aa00'></a><a id=c href=/c style='width:50px;height:20px;flex-shrink:0;background:#0000aa'></a></div>"
        ));
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (a.0, a.1, 40, 20), RED);
        assert_item(&doc, &frame, "b", (b.0, b.1, 30, 20), GREEN);
        assert_item(&doc, &frame, "c", (c.0, c.1, 50, 20), BLUE);
    }
}

#[test]
fn grid_and_inline_grid_use_inline_axis_not_irrelevant_flex_direction() {
    for display in ["grid", "inline-grid"] {
        for direction in ["column", "column-reverse"] {
            for (keyword, a, b) in [("left", 0, 30), ("right", 40, 70)] {
                let doc = pair(
                    &format!(
                        "display:{display};vertical-align:top;width:100px;height:40px;grid-template-columns:20px 30px;grid-template-rows:30px;column-gap:10px;flex-direction:{direction};justify-content:{keyword};align-items:start"
                    ),
                    "width:20px;height:20px",
                    "width:30px;height:30px",
                );
                let frame = clean_frame(&doc, 1.0);
                assert_item(&doc, &frame, "a", (a, 0, 20, 20), RED);
                assert_item(&doc, &frame, "b", (b, 0, 30, 30), GREEN);
            }
        }
    }
}

#[test]
fn grid_safe_overflow_keeps_track_group_semantics_with_irrelevant_reverse() {
    for (value, a, b) in [
        ("left", 60, 130),
        ("unsafe left", 60, 130),
        ("safe left", 60, 130),
        ("right", 10, 80),
        ("unsafe right", 10, 80),
        ("safe right", 60, 130),
    ] {
        let doc = pair(
            &format!(
                "display:grid;width:100px;height:30px;margin-left:60px;grid-template-columns:70px 80px;grid-template-rows:20px;flex-direction:column-reverse;justify-content:{value};align-items:start"
            ),
            "width:70px;height:20px",
            "width:80px;height:20px",
        );
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (a, 0, 70, 20), RED);
        assert_item(&doc, &frame, "b", (b, 0, 80, 20), GREEN);
    }
}

#[test]
fn grid_item_auto_margins_do_not_consume_track_group_justification_space() {
    for (keyword, a, b) in [("left", 15, 80), ("right", 105, 170)] {
        let doc = pair(
            &format!(
                "display:grid;width:200px;height:40px;grid-template-columns:50px 50px;grid-template-rows:30px;column-gap:10px;justify-content:{keyword};align-items:start"
            ),
            "width:20px;height:20px;margin-left:auto;margin-right:auto",
            "width:30px;height:30px;margin-left:auto",
        );
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (a, 0, 20, 20), RED);
        assert_item(&doc, &frame, "b", (b, 0, 30, 30), GREEN);
    }
}

#[test]
fn safe_physical_reverse_is_explicitly_rejected_even_when_content_fits() {
    for display in ["flex", "inline-flex"] {
        for direction in ["row-reverse", "column-reverse"] {
            for keyword in ["safe left", "safe right"] {
                for size in [20, 80] {
                    let doc = pair(
                        &format!(
                            "display:{display};width:100px;height:100px;flex-direction:{direction};justify-content:{keyword}"
                        ),
                        &format!("width:{size}px;height:{size}px;flex-shrink:0"),
                        &format!("width:{size}px;height:{size}px;flex-shrink:0"),
                    );
                    let frame = render(&doc, 1.0);
                    assert_eq!(frame.diagnostics.omitted, 0);
                    assert!(
                        frame.diagnostics.entries.iter().any(|entry| {
                            entry.kind == "layout-fallback"
                                && entry.message.contains(SAFE_REVERSE_REASON)
                        }),
                        "specific boundary diagnostic required: {:?}",
                        frame.diagnostics
                    );
                    assert!(
                        !frame
                            .diagnostics
                            .entries
                            .iter()
                            .any(|entry| entry.kind == "css-parse")
                    );
                }
            }
        }
    }
}

#[test]
fn existing_logical_and_flex_relative_keywords_remain_distinct() {
    for (direction, keyword, a, b) in [
        ("row", "start", 0, 20),
        ("row", "end", 50, 70),
        ("row", "flex-start", 0, 20),
        ("row", "flex-end", 50, 70),
        ("row-reverse", "start", 30, 0),
        ("row-reverse", "end", 80, 50),
        ("row-reverse", "flex-start", 80, 50),
        ("row-reverse", "flex-end", 30, 0),
    ] {
        let doc = pair(
            &format!(
                "display:flex;width:100px;height:40px;flex-direction:{direction};justify-content:{keyword}"
            ),
            "width:20px;height:20px;flex-shrink:0",
            "width:30px;height:20px;flex-shrink:0",
        );
        let frame = clean_frame(&doc, 1.0);
        assert_item(&doc, &frame, "a", (a, 0, 20, 20), RED);
        assert_item(&doc, &frame, "b", (b, 0, 30, 20), GREEN);
    }
}

#[test]
fn unrelated_item_legacy_and_wrong_property_refusals_remain_typed() {
    for (css, kind, message) in [
        (
            "align-items:last baseline",
            "layout-fallback",
            "item alignment keyword is not implemented",
        ),
        (
            "justify-items:legacy center",
            "layout-fallback",
            "legacy alignment is not implemented",
        ),
        ("align-content:left", "css-parse", "align-content"),
        ("align-content:right", "css-parse", "align-content"),
    ] {
        let doc = pair(
            &format!("display:flex;width:100px;height:40px;{css}"),
            "width:20px;height:20px",
            "width:30px;height:20px",
        );
        let frame = render(&doc, 1.0);
        assert_eq!(frame.diagnostics.omitted, 0);
        assert!(
            frame
                .diagnostics
                .entries
                .iter()
                .any(|entry| { entry.kind == kind && entry.message.contains(message) }),
            "preserve {css}: {:?}",
            frame.diagnostics
        );
    }
}

#[test]
fn rtl_and_vertical_writing_do_not_silently_gain_support() {
    let rtl = pair(
        "display:flex;width:100px;height:40px;direction:rtl;justify-content:left",
        "width:20px;height:20px",
        "width:30px;height:20px",
    );
    let frame = render(&rtl, 1.0);
    assert!(frame.diagnostics.entries.iter().any(|entry| {
        entry.kind == "css-unsupported" && entry.message.contains("CSS direction:")
    }));
    assert!(
        frame
            .diagnostics
            .entries
            .iter()
            .any(|entry| entry.kind == "layout-fallback")
    );
    let vertical = pair(
        "display:flex;width:100px;height:40px;writing-mode:vertical-rl;justify-content:right",
        "width:20px;height:20px",
        "width:30px;height:20px",
    );
    let frame = render(&vertical, 1.0);
    assert!(
        frame
            .diagnostics
            .entries
            .iter()
            .any(|entry| { entry.kind == "css-parse" && entry.message.contains("writing-mode") })
    );
}

#[test]
fn physical_declarations_on_block_and_table_do_not_create_a_flex_context() {
    for keyword in ["left", "right", "safe left", "safe right"] {
        let block = pair(
            &format!(
                "display:block;width:100px;height:60px;flex-direction:column-reverse;justify-content:{keyword}"
            ),
            "width:20px;height:20px",
            "width:30px;height:30px",
        );
        let frame = clean_frame(&block, 1.0);
        assert_item(&block, &frame, "a", (0, 0, 20, 20), RED);
        assert_item(&block, &frame, "b", (0, 20, 30, 30), GREEN);
        let table = document(&format!(
            "<table style='width:100px;flex-direction:column-reverse;justify-content:{keyword}'><tr><td><a id=a href=/a style='width:20px;height:20px;background:#aa0000'></a><a id=b href=/b style='width:30px;height:30px;background:#00aa00'></a></td></tr></table>"
        ));
        let frame = clean_frame(&table, 1.0);
        assert_item(&table, &frame, "a", (0, 0, 20, 20), RED);
        assert_item(&table, &frame, "b", (0, 20, 30, 30), GREEN);
    }
}
