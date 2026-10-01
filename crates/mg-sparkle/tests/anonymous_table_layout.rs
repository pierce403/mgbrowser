//! Pure orphan-cell wrappers only: no fabricated DOM or site-specific layout.
use mg_sparkle::{
    document::{Document, parse},
    paint::Fonts,
    render::{Action, Controls, Frame, LayoutBox, Viewport, render_scaled},
};

fn document(body: &str) -> Document {
    parse(
        &format!(
            "<html><head><style>html,body{{margin:0;background:white}} *{{margin:0;padding:0;border:0;border-spacing:0;font-size:12px;line-height:20px}} a{{text-decoration:none}}</style></head><body>{body}</body></html>"
        ),
        "https://fixture.example/",
    )
}

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

fn frame(doc: &Document, width: u32, scale: f32) -> Frame {
    render_scaled(
        doc,
        &mut fonts(),
        Viewport {
            width,
            height: 300,
            scroll: 0,
        },
        &Controls::default(),
        scale,
    )
}

fn styled(frame: &Frame) {
    assert!(
        !frame.diagnostics.entries.iter().any(|d| matches!(
            d.kind,
            "css-parse" | "layout-fallback" | "style-fallback" | "image-unsupported"
        )),
        "{:?}",
        frame.diagnostics
    );
}

fn rect<'a>(doc: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let id = doc.query_selector(0, selector).unwrap().unwrap();
    frame.boxes.iter().rev().find(|r| r.node == id).unwrap()
}

fn geometry(rect: &LayoutBox) -> (i32, i32, u32, u32) {
    (rect.x, rect.y, rect.width, rect.height)
}

fn pixel(frame: &Frame, x: i32, y: i32) -> u32 {
    let x = frame.canvas.physical_edge(i64::from(x)) as usize;
    let y = frame.canvas.physical_edge(i64::from(y)) as usize;
    frame.canvas.pixels[y * frame.canvas.width as usize + x]
}

fn hit(frame: &Frame, x: i32, y: i32) -> Option<&str> {
    frame.hits.iter().rev().find_map(|h| {
        if x >= h.x && y >= h.y && x < h.x + h.w as i32 && y < h.y + h.h as i32 {
            if let Action::Link { href, .. } = &h.action {
                return Some(href.as_str());
            }
        }
        None
    })
}

#[test]
fn orphan_cells_share_one_row_inside_a_flex_toolbar_at_all_scales() {
    let doc = document(
        "<style>#bar{display:flex;height:48px;align-items:center} .cell{display:table-cell;height:48px;vertical-align:middle} a{display:inline-block;width:48px;height:48px;padding:12px;box-sizing:border-box} svg{display:block;width:24px;height:24px}</style><div id=bar><div id=tools><div id=one class=cell><a id=first href=/first><svg viewBox='0 0 1 1' fill='#2468ac'><rect width='1' height='1'/></svg></a></div> \n <div id=two class=cell><a id=second href=/second><svg viewBox='0 0 1 1' fill='#ac6824'><rect width='1' height='1'/></svg></a></div></div></div>",
    );
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, 320, scale);
        styled(&frame);
        assert_eq!(geometry(rect(&doc, &frame, "#tools")), (0, 0, 96, 48));
        assert_eq!(geometry(rect(&doc, &frame, "#one")), (0, 0, 48, 48));
        assert_eq!(geometry(rect(&doc, &frame, "#two")), (48, 0, 48, 48));
        assert_eq!(geometry(rect(&doc, &frame, "#first")), (0, 0, 48, 48));
        assert_eq!(geometry(rect(&doc, &frame, "#second")), (48, 0, 48, 48));
        assert_eq!(pixel(&frame, 20, 20), 0x2468ac);
        assert_eq!(pixel(&frame, 68, 20), 0xac6824);
        assert_eq!(hit(&frame, 2, 2), Some("https://fixture.example/first"));
        assert_eq!(hit(&frame, 50, 2), Some("https://fixture.example/second"));
        assert_eq!(hit(&frame, 98, 2), None);
    }
}

#[test]
fn anonymous_row_does_not_inherit_parent_height_or_paint_parent_twice() {
    let doc = document(
        "<style>#parent{width:200px;height:100px;background:rgba(0,0,0,.5)} .cell{display:table-cell;vertical-align:middle} a{display:block;width:20px;background:#2468ac}</style><div id=parent><div id=one class=cell><a id=short href=/short style='height:10px'></a></div><div id=two class=cell><a id=tall href=/tall style='height:30px'></a></div></div>",
    );
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, 320, scale);
        styled(&frame);
        assert_eq!(geometry(rect(&doc, &frame, "#parent")), (0, 0, 200, 100));
        assert_eq!(geometry(rect(&doc, &frame, "#short")), (0, 10, 20, 10));
        assert_eq!(geometry(rect(&doc, &frame, "#tall")), (20, 0, 20, 30));
        assert_eq!(pixel(&frame, 100, 50), 0x808080);
        assert_eq!(pixel(&frame, 1, 1), 0x808080);
        assert_eq!(hit(&frame, 1, 1), None);
        assert_eq!(hit(&frame, 1, 11), Some("https://fixture.example/short"));
        let id = doc.query_selector(0, "#parent").unwrap().unwrap();
        assert_eq!(frame.boxes.iter().filter(|r| r.node == id).count(), 1);
    }
}

#[test]
fn anonymous_columns_use_shared_intrinsics_and_wrap_cell_content() {
    let doc = document(
        "<div id=parent style='width:80px'><div id=one style='display:table-cell'>mmmm mmmm mmmm</div><div id=two style='display:table-cell'>mmmm mmmm mmmm</div></div>",
    );
    let frame = frame(&doc, 320, 1.0);
    styled(&frame);
    let one = rect(&doc, &frame, "#one");
    let two = rect(&doc, &frame, "#two");
    assert_eq!(one.y, two.y);
    assert!(one.width >= fonts().width("mmmm", 12.0).floor() as u32);
    assert_eq!(two.x, one.x + one.width as i32);
    assert_eq!(one.height, 60);
    assert_eq!(two.height, 60);
    assert_eq!(rect(&doc, &frame, "#parent").height, 60);
}

#[test]
fn inline_block_cell_container_is_one_inline_unit() {
    let doc = document(
        "<style>.group{display:inline-block}.cell{display:table-cell;width:20px;height:10px}</style><div style='width:65px'><span id=first class=group><span class=cell></span><span class=cell></span></span><span id=second class=group><span class=cell></span><span class=cell></span></span></div>",
    );
    let frame = frame(&doc, 320, 1.0);
    styled(&frame);
    assert_eq!(rect(&doc, &frame, "#first").width, 40);
    assert_eq!(rect(&doc, &frame, "#second").width, 40);
    assert!(rect(&doc, &frame, "#second").y > rect(&doc, &frame, "#first").y);
}

#[test]
fn numeric_border_box_width_and_minimum_do_not_double_count_intrinsic_edges() {
    for property in ["width", "min-width"] {
        let doc = document(&format!(
            "<div style='display:flex'><div id=outer><div style='display:inline-block;{property}:48px;padding:10px;border:2px solid;box-sizing:border-box'><div style='width:24px;height:10px'></div></div></div></div>"
        ));
        let frame = frame(&doc, 320, 1.0);
        styled(&frame);
        assert_eq!(rect(&doc, &frame, "#outer").width, 48, "{property}");
    }
}

#[test]
fn mixed_ordinary_content_is_explicitly_unsupported_and_not_reordered() {
    let doc = document(
        "<div><div id=cell style='display:table-cell;width:20px;height:10px'></div><div id=ordinary style='height:10px'></div></div>",
    );
    let frame = frame(&doc, 320, 1.0);
    styled(&frame);
    assert!(
        frame
            .diagnostics
            .entries
            .iter()
            .any(|d| d.kind == "css-unsupported" && d.message.contains("Anonymous tables mixed")),
        "{:?}",
        frame.diagnostics
    );
    assert!(rect(&doc, &frame, "#ordinary").y >= 10);
}

#[test]
fn anonymous_column_bound_is_fail_closed_not_silent_truncation() {
    for count in [256, 257] {
        let doc = document(&format!(
            "<div>{}</div>",
            "<span style='display:table-cell;width:1px;height:1px'></span>".repeat(count)
        ));
        let frame = frame(&doc, 320, 1.0);
        if count == 256 {
            styled(&frame);
            let cell_ids: Vec<_> = doc
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.tag == "span")
                .map(|(i, _)| i)
                .collect();
            assert!(
                cell_ids
                    .iter()
                    .all(|id| frame.boxes.iter().any(|r| r.node == *id))
            );
        } else {
            assert!(
                frame
                    .diagnostics
                    .entries
                    .iter()
                    .any(|d| d.kind == "layout-fallback"
                        && d.message.contains("anonymous table column limit")),
                "{:?}",
                frame.diagnostics
            );
        }
    }
}

#[test]
fn hidden_ancestor_does_not_admit_unrendered_anonymous_columns() {
    let doc = document(&format!(
        "<div style='display:none'><div>{}</div></div><div id=visible style='height:20px;background:#2468ac'></div>",
        "<span style='display:table-cell;width:1px;height:1px'></span>".repeat(257)
    ));
    let frame = frame(&doc, 320, 1.0);
    styled(&frame);
    assert_eq!(rect(&doc, &frame, "#visible").y, 0);
    assert_eq!(pixel(&frame, 1, 1), 0x2468ac);
}
