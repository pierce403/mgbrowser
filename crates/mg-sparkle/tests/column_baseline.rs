//! Generic horizontal-LTR synthesized column baselines, not a WPT rewrite.
use mg_sparkle::{
    document::{Document, parse},
    paint::Fonts,
    render::{Action, Controls, Frame, LayoutBox, Viewport, render_scaled},
};

fn document(body: &str) -> Document {
    parse(
        &format!(
            "<html><head><style>html,body{{margin:0}} *{{margin:0;padding:0;border:0;font-size:12px;line-height:20px}} a{{text-decoration:none}}</style></head><body>{body}</body></html>"
        ),
        "https://fixture.example/",
    )
}

fn frame(document: &Document, scale: f32) -> Frame {
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
            width: 300,
            height: 200,
            scroll: 0,
        },
        &Controls::default(),
        scale,
    )
}

fn styled(frame: &Frame) {
    assert!(
        frame.diagnostics.entries.is_empty(),
        "{:?}",
        frame.diagnostics
    );
}

fn rect<'a>(document: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let id = document.query_selector(0, selector).unwrap().unwrap();
    frame.boxes.iter().rev().find(|r| r.node == id).unwrap()
}

fn pixel(frame: &Frame, x: i64, y: i64) -> u32 {
    let x = frame.canvas.physical_edge(x) as usize;
    let y = frame.canvas.physical_edge(y) as usize;
    frame.canvas.pixels[y * frame.canvas.width as usize + x]
}

#[test]
fn unequal_boxes_share_a_left_border_baseline_and_preserve_pixels_and_hits() {
    for direction in ["column", "column-reverse"] {
        for wrap in ["nowrap", "wrap", "wrap-reverse"] {
            let doc = document(&format!(
                "<div style='display:flex;flex-direction:{direction};flex-wrap:{wrap};align-items:baseline'><a id=a href=/a style='width:80px;height:20px;background:#aa0000'></a><div id=b style='width:120px;height:30px;background:#00aa00'></div></div>"
            ));
            for scale in [1.0, 1.25, 2.0] {
                let f = frame(&doc, scale);
                styled(&f);
                let a = rect(&doc, &f, "#a");
                let b = rect(&doc, &f, "#b");
                let x = if wrap == "wrap-reverse" { 180 } else { 0 };
                assert_eq!((a.x, b.x), (x, x), "{direction}/{wrap}");
                assert_eq!((a.width, b.width), (80, 120));
                assert_eq!(
                    (a.y, b.y),
                    if direction == "column" {
                        (0, 20)
                    } else {
                        (30, 0)
                    }
                );
                assert_eq!(pixel(&f, i64::from(x + 10), i64::from(a.y + 10)), 0xaa0000);
                let hit = f
                    .hits
                    .iter()
                    .find(
                        |h| matches!(&h.action, Action::Link { href, .. } if href.ends_with("/a")),
                    )
                    .unwrap();
                assert_eq!((hit.x, hit.y, hit.w, hit.h), (a.x, a.y, 80, 20));
            }
        }
    }
}

#[test]
fn equal_left_margins_preserve_intrinsic_group_width_with_unequal_right_margins() {
    for left in [-4, 0, 8] {
        for wrap in ["nowrap", "wrap-reverse"] {
            let doc = document(&format!(
                "<div id=root style='display:flex;flex-direction:column;flex-wrap:{wrap};align-items:baseline;width:max-content'><div id=a style='width:80px;height:20px;margin-left:{left}px;margin-right:17px'></div><div id=b style='width:120px;height:30px;margin-left:{left}px;margin-right:3px'></div></div><div id=after style='height:10px'></div>"
            ));
            let f = frame(&doc, 1.0);
            styled(&f);
            assert_eq!(rect(&doc, &f, "#root").width, (left + 123) as u32);
            assert_eq!(rect(&doc, &f, "#a").x, left);
            assert_eq!(rect(&doc, &f, "#b").x, left);
            assert_eq!(rect(&doc, &f, "#after").y, 50);
        }
    }
}

#[test]
fn automatic_margins_and_explicit_nonbaseline_alignment_do_not_join_the_group() {
    let doc = document(
        "<div style='display:flex;flex-direction:column;flex-wrap:wrap-reverse;align-items:baseline'><div id=a style='width:80px;height:20px'></div><div id=b style='width:120px;height:20px;align-self:baseline'></div><div id=auto style='width:40px;height:20px;margin-left:auto'></div><div id=end style='width:20px;height:20px;align-self:end'></div><div id=normal style='width:10px;height:20px;align-self:normal'></div></div>",
    );
    let f = frame(&doc, 1.0);
    styled(&f);
    assert_eq!(rect(&doc, &f, "#a").x, 180);
    assert_eq!(rect(&doc, &f, "#b").x, 180);
    assert_eq!(rect(&doc, &f, "#auto").x, 260);
    assert_eq!(rect(&doc, &f, "#end").x, 280);
    assert_eq!(rect(&doc, &f, "#normal").x, 290);
}

#[test]
fn relative_insets_move_paint_after_baseline_alignment_without_moving_peers() {
    for (inset, offset) in [("left:7px", 7), ("right:5px", -5), ("left:10%", 30)] {
        let doc = document(&format!(
            "<div style='display:flex;flex-direction:column;flex-wrap:wrap-reverse;align-items:baseline'><div id=a style='width:80px;height:20px;position:relative;{inset}'></div><div id=b style='width:120px;height:30px'></div></div>"
        ));
        let f = frame(&doc, 1.0);
        styled(&f);
        assert_eq!(rect(&doc, &f, "#a").x, 180 + offset, "{inset}");
        assert_eq!(rect(&doc, &f, "#b").x, 180, "{inset}");
    }
}

#[test]
fn text_and_anonymous_items_match_an_independently_left_aligned_group() {
    let contents = "Anonymous text before<span style='font-size:25px;line-height:30px'>A larger line</span><span style='font-size:10px'>Another longer text line</span>";
    let actual = document(&format!(
        "<div style='display:flex;flex-direction:column;flex-wrap:wrap-reverse;align-items:baseline'>{contents}</div>"
    ));
    let reference = document(&format!(
        "<div style='display:flex;flex-direction:column;align-items:flex-start;width:max-content;margin-left:auto'>{contents}</div>"
    ));
    let actual = frame(&actual, 1.0);
    let reference = frame(&reference, 1.0);
    styled(&actual);
    styled(&reference);
    assert_eq!(actual.canvas.pixels, reference.canvas.pixels);
}

#[test]
fn zero_width_items_join_the_proven_single_line_group() {
    let doc = document(
        "<div style='display:flex;flex-direction:column;flex-wrap:wrap-reverse;align-items:baseline'><div id=zero style='width:0;height:20px'><div id=overflow style='width:10px;height:10px;background:#aa0000'></div></div><div id=wide style='width:120px;height:30px'></div></div>",
    );
    let f = frame(&doc, 1.0);
    styled(&f);
    assert_eq!(rect(&doc, &f, "#zero").x, 180);
    assert_eq!(rect(&doc, &f, "#overflow").x, 180);
    assert_eq!(rect(&doc, &f, "#wide").x, 180);
    assert_eq!(pixel(&f, 185, 5), 0xaa0000);
}

#[test]
fn unsupported_group_sizing_and_hidden_line_membership_are_diagnosed() {
    for (outer, child, message) in [
        ("", "margin-left:7px", "unequal left margins"),
        (
            "width:max-content",
            "margin-left:7px",
            "unequal left margins",
        ),
        ("height:30px;flex-wrap:wrap", "", "finite-height wrapping"),
        (
            "height:30px;flex-wrap:wrap-reverse",
            "",
            "finite-height wrapping",
        ),
    ] {
        let doc = document(&format!(
            "<div style='display:flex;flex-direction:column;align-items:baseline;{outer}'><div style='width:80px;height:20px;{child}'>first</div><div style='width:120px;height:30px'>second</div></div>"
        ));
        let f = frame(&doc, 1.0);
        assert!(
            f.diagnostics
                .entries
                .iter()
                .any(|d| d.kind == "layout-fallback" && d.message.contains(message)),
            "{message}: {:?}",
            f.diagnostics
        );
        assert!(
            f.canvas.pixels.iter().any(|p| *p != 0xffffff),
            "readable fallback remains"
        );
    }
}

#[test]
fn single_participant_row_and_grid_paths_remain_unchanged() {
    let single = document(
        "<div style='display:flex;flex-direction:column;flex-wrap:wrap-reverse;align-items:baseline;height:30px'><div id=a style='width:40px;height:20px'></div></div>",
    );
    let f = frame(&single, 1.0);
    styled(&f);
    assert_eq!(rect(&single, &f, "#a").x, 260);
    let row = document(
        "<div style='display:flex;align-items:baseline'><div id=a style='width:80px;height:20px'></div><div id=b style='width:120px;height:40px'></div></div>",
    );
    let f = frame(&row, 1.0);
    styled(&f);
    assert_eq!((rect(&row, &f, "#a").x, rect(&row, &f, "#a").y), (0, 20));
    assert_eq!((rect(&row, &f, "#b").x, rect(&row, &f, "#b").y), (80, 0));
    let grid = document(
        "<div style='display:grid;grid-template-columns:80px 120px;align-items:start'><div id=a style='height:20px'></div><div id=b style='height:40px'></div></div>",
    );
    let f = frame(&grid, 1.0);
    styled(&f);
    assert_eq!((rect(&grid, &f, "#a").x, rect(&grid, &f, "#a").y), (0, 0));
    assert_eq!((rect(&grid, &f, "#b").x, rect(&grid, &f, "#b").y), (80, 0));
}
