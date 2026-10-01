//! Public-render contracts for generic flex/grid layout, not a live-site fixture.
use mg_sparkle::{
    document::{Document, ResourceData, parse},
    paint::Fonts,
    render::{Action, Controls, Frame, LayoutBox, Viewport, render_scaled},
};

fn document(body: &str) -> Document {
    parse(
        &format!(
            "<html><head><style>html,body{{margin:0}} *{{margin:0;padding:0;border:0;font-size:12px;line-height:20px}} table{{border-spacing:0}} a{{text-decoration:none}}</style></head><body>{body}</body></html>"
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

fn frame(document: &Document, width: u32, height: u32, scroll: i32, scale: f32) -> Frame {
    let frame = render_scaled(
        document,
        &mut fonts(),
        Viewport {
            width,
            height,
            scroll,
        },
        &Controls::default(),
        scale,
    );
    assert!(
        !frame.diagnostics.entries.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            "css-parse" | "style-fallback" | "layout-fallback"
        )),
        "formatting must use the real styled path: {:?}",
        frame.diagnostics
    );
    frame
}

fn rect<'a>(document: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let id = document.query_selector(0, selector).unwrap().unwrap();
    frame
        .boxes
        .iter()
        .rev()
        .find(|rect| rect.node == id)
        .unwrap()
}

fn geometry(rect: &LayoutBox) -> (i32, i32, u32, u32) {
    (rect.x, rect.y, rect.width, rect.height)
}

fn assert_link_at(frame: &Frame, path: &str, x: i32, y: i32) {
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
        "wrong topmost action: {:?}",
        hit.action
    );
}

fn pixel(frame: &Frame, x: i32, y: i32) -> u32 {
    let x = frame.canvas.physical_edge(i64::from(x)) as usize;
    let y = frame.canvas.physical_edge(i64::from(y)) as usize;
    frame.canvas.pixels[y * frame.canvas.width as usize + x]
}

#[test]
fn flex_shrinks_and_grows_text_side_without_shrinking_fixed_thumbnail() {
    let doc = document(
        "<div id=row style='display:flex;gap:12px;align-items:flex-start'><div id=thumbnail style='flex:0 0 120px;height:80px'></div><div id=copy style='flex:1 1 260px;min-width:0;height:30px'></div></div>",
    );
    for width in [320, 480] {
        let frame = frame(&doc, width, 200, 0, 1.0);
        assert_eq!(geometry(rect(&doc, &frame, "#thumbnail")), (0, 0, 120, 80));
        assert_eq!(
            geometry(rect(&doc, &frame, "#copy")),
            (132, 0, width - 132, 30)
        );
        assert_eq!(geometry(rect(&doc, &frame, "#row")), (0, 0, width, 80));
    }
}

#[test]
fn flex_wrap_uses_line_height_and_separate_row_column_gaps() {
    let doc = document(
        "<div id=row style='display:flex;flex-wrap:wrap;gap:10px 15px;align-items:flex-start'><div id=a style='flex:0 0 100px;height:20px'></div><div id=b style='flex:0 0 100px;height:30px'></div><div id=c style='flex:0 0 100px;height:40px'></div></div>",
    );
    let frame = frame(&doc, 250, 200, 0, 1.0);
    assert_eq!(geometry(rect(&doc, &frame, "#a")), (0, 0, 100, 20));
    assert_eq!(geometry(rect(&doc, &frame, "#b")), (115, 0, 100, 30));
    assert_eq!(geometry(rect(&doc, &frame, "#c")), (0, 40, 100, 40));
    assert_eq!(rect(&doc, &frame, "#row").height, 80);
}

#[test]
fn flex_order_moves_paint_and_link_hits_together() {
    let doc = document(
        "<style>#row{display:flex;gap:10px} #row>a{flex:0 0 60px;height:30px}</style><div id=row><a id=a href=/a style='order:2;background:#aa0000'></a><a id=b href=/b style='order:-1;background:#00aa00'></a><a id=c href=/c style='order:0;background:#0000aa'></a></div>",
    );
    let frame = frame(&doc, 300, 100, 0, 1.0);
    for (id, path, x, color) in [
        ("#b", "/b", 0, 0x00aa00),
        ("#c", "/c", 70, 0x0000aa),
        ("#a", "/a", 140, 0xaa0000),
    ] {
        assert_eq!(geometry(rect(&doc, &frame, id)), (x, 0, 60, 30));
        assert_link_at(&frame, path, x + 30, 15);
        assert_eq!(pixel(&frame, x + 30, 15), color);
    }
}

#[test]
fn twelve_column_minmax_fraction_grid_places_eight_and_four_spans() {
    let doc = document(
        "<div id=grid style='display:grid;grid-template-columns:repeat(12,minmax(0,1fr));gap:16px;align-items:start'><div id=main style='grid-column:span 8;height:80px'></div><div id=side style='grid-column:span 4;height:40px'></div></div>",
    );
    for (width, main, side_x, side) in [(1040, 688, 704, 336), (920, 608, 624, 296)] {
        let frame = frame(&doc, width, 300, 0, 1.0);
        assert_eq!(geometry(rect(&doc, &frame, "#main")), (0, 0, main, 80));
        assert_eq!(geometry(rect(&doc, &frame, "#side")), (side_x, 0, side, 40));
        assert_eq!(geometry(rect(&doc, &frame, "#grid")), (0, 0, width, 80));
    }
}

#[test]
fn grid_implicit_row_follows_tallest_previous_item_and_row_gap() {
    let doc = document(
        "<div id=grid style='display:grid;grid-template-columns:repeat(12,minmax(0,1fr));gap:16px;align-items:start'><div style='grid-column:span 8;height:80px'></div><div style='grid-column:span 4;height:40px'></div><div id=next style='grid-column:span 12;height:25px'></div></div>",
    );
    let frame = frame(&doc, 1040, 300, 0, 1.0);
    assert_eq!(geometry(rect(&doc, &frame, "#next")), (0, 96, 1040, 25));
    assert_eq!(rect(&doc, &frame, "#grid").height, 121);
}

#[test]
fn nested_auto_height_card_measures_wrapped_text_table_and_real_image() {
    const COPY: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
    let mut doc = document(&format!(
        "<div id=grid style='display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px;align-items:start'><div id=card style='display:flex;flex-direction:column;gap:6px'><p id=copy>{COPY}</p><table id=facts style='width:100%'><tr><td>one</td><td>two</td></tr><tr><td>three</td><td>four</td></tr></table><img id=image src=/picture.svg style='width:80px;height:40px'></div><div>aside</div></div>"
    ));
    doc.resources.insert("https://fixture.example/picture.svg".into(), ResourceData {
        bytes: b"<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20' fill='#aabbcc'/></svg>".to_vec(),
        content_type: "image/svg+xml".into(),
    });
    let mut previous_height = 0;
    for (width, column) in [(310, 150), (170, 80)] {
        // The existing block text path is the font-dependent wrapping oracle,
        // not guessed character widths or a hard-coded system font signature.
        let reference = document(&format!(
            "<div id=text style='width:{column}px'>{COPY}</div>"
        ));
        let reference_frame = frame(&reference, width, 400, 0, 1.0);
        let text_height = rect(&reference, &reference_frame, "#text").height;
        let frame = frame(&doc, width, 400, 0, 1.0);
        assert_eq!(
            geometry(rect(&doc, &frame, "#copy")),
            (0, 0, column, text_height)
        );
        assert_eq!(
            geometry(rect(&doc, &frame, "#facts")),
            (0, text_height as i32 + 6, column, 40)
        );
        assert_eq!(
            geometry(rect(&doc, &frame, "#image")),
            (0, text_height as i32 + 52, 80, 40)
        );
        let card_height = text_height + 92;
        assert_eq!(rect(&doc, &frame, "#card").height, card_height);
        assert_eq!(rect(&doc, &frame, "#grid").height, card_height);
        assert_eq!(pixel(&frame, 20, text_height as i32 + 72), 0xaabbcc);
        assert!(
            card_height > previous_height,
            "narrowing must increase wrapped card height"
        );
        previous_height = card_height;
    }
}

#[test]
fn anonymous_flex_text_runs_keep_measured_width_and_gaps() {
    let doc = document(
        "<div id=row style='display:flex;gap:10px;align-items:flex-start'>prefix<div id=fixed style='flex:0 0 40px;height:20px'></div>suffix</div>",
    );
    let row = doc.query_selector(0, "#row").unwrap().unwrap();
    let prefix = doc.nodes[row].children[0];
    let suffix = *doc.nodes[row].children.last().unwrap();
    assert_eq!(doc.nodes[prefix].text, "prefix");
    assert_eq!(doc.nodes[suffix].text, "suffix");
    let prefix_width = fonts().width("prefix", 12.0);
    let frame = frame(&doc, 300, 100, 0, 1.0);
    let prefix_box = frame.boxes.iter().find(|rect| rect.node == prefix).unwrap();
    let suffix_box = frame.boxes.iter().find(|rect| rect.node == suffix).unwrap();
    assert_eq!(prefix_box.x, 0);
    assert_eq!(prefix_box.width, prefix_width.ceil() as u32);
    assert_eq!(
        rect(&doc, &frame, "#fixed").x,
        (prefix_width + 10.0).round() as i32
    );
    assert_eq!(suffix_box.x, (prefix_width + 60.0).round() as i32);
    assert_eq!(rect(&doc, &frame, "#row").height, 20);
}

#[test]
fn display_contents_flattens_flex_items_without_a_wrapper_box() {
    let doc = document(
        "<div id=row style='display:flex;gap:10px;align-items:flex-start'><div id=contents style='display:contents'><div id=a style='width:40px;height:20px;flex-shrink:0'></div><div id=b style='width:60px;height:20px;flex-shrink:0'></div></div><div id=c style='width:80px;height:20px;flex-shrink:0'></div></div>",
    );
    let frame = frame(&doc, 300, 100, 0, 1.0);
    assert_eq!(geometry(rect(&doc, &frame, "#a")), (0, 0, 40, 20));
    assert_eq!(geometry(rect(&doc, &frame, "#b")), (50, 0, 60, 20));
    assert_eq!(geometry(rect(&doc, &frame, "#c")), (120, 0, 80, 20));
    let contents = doc.query_selector(0, "#contents").unwrap().unwrap();
    assert!(frame.boxes.iter().all(|rect| rect.node != contents));
    assert_eq!(rect(&doc, &frame, "#row").height, 20);
}

#[test]
fn fractional_flex_geometry_hits_and_pixels_agree_at_all_supported_test_scales() {
    let doc = document(
        "<div id=row style='display:flex;width:207.5px;margin-left:10.5px;gap:7.5px'><a id=a href=/a style='flex:1 1 0px;height:30px;background:#123456'></a><a id=b href=/b style='flex:1 1 0px;height:30px;background:#abcdef'></a></div>",
    );
    let mut original = None;
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, 300, 100, 0, scale);
        assert_eq!(geometry(rect(&doc, &frame, "#row")), (11, 0, 208, 30));
        assert_eq!(geometry(rect(&doc, &frame, "#a")), (11, 0, 100, 30));
        assert_eq!(geometry(rect(&doc, &frame, "#b")), (118, 0, 100, 30));
        assert_link_at(&frame, "/a", 60, 15);
        assert_link_at(&frame, "/b", 168, 15);
        assert_eq!(pixel(&frame, 60, 15), 0x123456);
        assert_eq!(pixel(&frame, 168, 15), 0xabcdef);
        assert_eq!(pixel(&frame, 114, 15), 0xffffff);
        assert!(
            !frame
                .hits
                .iter()
                .any(|hit| hit.x <= 114 && hit.x + hit.w as i32 > 114)
        );
        assert_eq!(frame.canvas.width, (300.0 * scale).round() as u32);
        assert_eq!(frame.canvas.height, (100.0 * scale).round() as u32);
        let logical = (
            frame.boxes.clone(),
            format!("{:?}", frame.hits),
            frame.content_height,
        );
        if let Some(original) = &original {
            assert_eq!(
                &logical, original,
                "physical scale must not alter logical geometry"
            );
        } else {
            original = Some(logical);
        }
    }
}

#[test]
fn scrolled_grid_link_geometry_and_visible_hit_clipping_match() {
    let doc = document(
        "<div style='height:30px'></div><div style='display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px'><a id=a href=/a style='height:80px;background:#123456'></a><a id=b href=/b style='height:80px;background:#abcdef'></a></div><div style='height:200px'></div>",
    );
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, 210, 100, 50, scale);
        for (id, path, x, color) in [("#a", "/a", 0, 0x123456), ("#b", "/b", 110, 0xabcdef)] {
            assert_eq!(geometry(rect(&doc, &frame, id)), (x, -20, 100, 80));
            assert_link_at(&frame, path, x + 50, 20);
            assert_eq!(pixel(&frame, x + 50, 20), color);
            assert!(frame.hits.iter().any(|hit| {
                matches!(&hit.action, Action::Link { href, .. } if href == &format!("https://fixture.example{path}"))
                    && (hit.x, hit.y, hit.w, hit.h) == (x, 0, 100, 60)
            }));
        }
    }
}

#[test]
fn flex_item_box_sizing_applies_padding_and_border_exactly_once() {
    let doc = document(
        "<style>#row{display:flex;gap:8px;align-items:flex-start} #row>div{flex:0 0 auto;width:100px;height:20px;padding:5px;border:2px solid}</style><div id=row><div id=border style='box-sizing:border-box'></div><div id=content style='box-sizing:content-box'></div></div>",
    );
    let frame = frame(&doc, 300, 100, 0, 1.0);
    assert_eq!(geometry(rect(&doc, &frame, "#border")), (0, 0, 100, 20));
    assert_eq!(geometry(rect(&doc, &frame, "#content")), (108, 0, 114, 34));
    assert_eq!(rect(&doc, &frame, "#row").height, 34);
}
