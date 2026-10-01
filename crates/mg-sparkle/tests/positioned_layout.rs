//! Generic, script-free positioning fixtures. These are not live-site evidence.
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

fn frame(document: &Document, scroll: i32, scale: f32) -> Frame {
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
            width: 320,
            height: 200,
            scroll,
        },
        &Controls::default(),
        scale,
    )
}

fn styled(frame: &Frame) {
    assert!(
        !frame.diagnostics.entries.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            "css-parse" | "style-fallback" | "layout-fallback"
        )),
        "must use the actual styled layout: {:?}",
        frame.diagnostics
    );
}

fn rect<'a>(document: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let id = document.query_selector(0, selector).unwrap().unwrap();
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
    frame.hits.iter().rev().find_map(|hit| {
        if x >= hit.x && y >= hit.y && x < hit.x + hit.w as i32 && y < hit.y + hit.h as i32 {
            if let Action::Link { href, .. } = &hit.action {
                return Some(href.as_str());
            }
        }
        None
    })
}

#[test]
fn fixed_box_stays_at_viewport_coordinates_and_does_not_grow_document() {
    let doc = document(
        "<div style='height:600px'></div><a id=fixed href=/fixed style='position:fixed;top:12px;right:10px;width:40px;height:25px;background:#00aa44'></a>",
    );
    let baseline = frame(&document("<div style='height:600px'></div>"), 0, 1.0).content_height;
    for scale in [1.0, 1.25, 2.0] {
        for scroll in [0, 50, 300] {
            let frame = frame(&doc, scroll, scale);
            styled(&frame);
            assert_eq!(geometry(rect(&doc, &frame, "#fixed")), (270, 12, 40, 25));
            assert_eq!(pixel(&frame, 280, 20), 0x00aa44);
            assert_eq!(hit(&frame, 280, 20), Some("https://fixture.example/fixed"));
            assert_eq!(frame.content_height, baseline);
        }
    }
    let far = document(
        "<div style='height:20px'></div><div style='position:absolute;left:0;top:1000px;width:10px;height:20px'></div><div style='position:fixed;left:0;top:2000px;width:10px;height:20px'><div style='height:4000px'></div><div style='position:absolute;left:0;top:5000px;width:10px;height:20px'></div></div>",
    );
    let far_frame = frame(&far, 0, 1.0);
    styled(&far_frame);
    assert_eq!(
        far_frame.content_height, 1020,
        "absolute overflow is scroll reachable; fixed overflow is not"
    );
}

#[test]
fn relative_containing_padding_box_resolves_insets_and_box_sizing() {
    let doc = document(
        "<div id=parent style='position:relative;left:5px;top:7px;width:200px;height:100px;padding:10px;border:2px solid;margin:20px'><div id=content style='position:absolute;left:10px;top:8px;width:40px;height:20px;padding:3px;border:2px solid'></div><div id=border style='position:absolute;right:10px;bottom:8px;width:60px;height:30px;box-sizing:border-box;padding:5px;border:2px solid'></div></div>",
    );
    let frame = frame(&doc, 0, 1.0);
    styled(&frame);
    assert_eq!(geometry(rect(&doc, &frame, "#parent")), (25, 27, 224, 124));
    assert_eq!(geometry(rect(&doc, &frame, "#content")), (37, 37, 50, 30));
    assert_eq!(geometry(rect(&doc, &frame, "#border")), (177, 111, 60, 30));
}

#[test]
fn opposite_insets_stretch_and_percentages_use_containing_padding_box() {
    let doc = document(
        "<div style='position:relative;width:200px;height:100px;padding:10px;border:2px solid;margin:20px'><div id=stretch style='position:absolute;left:10%;right:10px;top:5px;bottom:15px;padding:3px;border:2px solid'></div></div>",
    );
    let frame = frame(&doc, 0, 1.0);
    styled(&frame);
    assert_eq!(geometry(rect(&doc, &frame, "#stretch")), (44, 27, 188, 100));
}

#[test]
fn auto_height_uses_wrapped_content_without_making_percentage_children_definite() {
    let text = "alpha beta gamma delta epsilon";
    let reference_doc = document(&format!(
        "<div id=reference style='width:80px'>{text}</div>"
    ));
    let reference = frame(&reference_doc, 0, 1.0);
    let doc = document(&format!(
        "<div id=wrapped style='position:absolute;left:10px;top:10px;max-width:80px'>{text}</div><div style='position:absolute;left:120px;top:10px;width:100px;min-height:80px'><div id=auto style='height:50%'>line</div></div><div style='position:absolute;left:120px;top:100px;width:100px;height:80px'><div id=definite style='height:50%'>line</div></div>"
    ));
    let frame = frame(&doc, 0, 1.0);
    styled(&frame);
    assert_eq!(
        geometry(rect(&doc, &frame, "#wrapped")),
        (
            10,
            10,
            80,
            rect(&reference_doc, &reference, "#reference").height
        )
    );
    assert_eq!(rect(&doc, &frame, "#auto").height, 20);
    assert_eq!(rect(&doc, &frame, "#definite").height, 40);
}

#[test]
fn absolute_structured_button_measures_children_instead_of_replaced_label() {
    let doc = document(
        "<button id=button style='position:absolute;display:flex;left:10px;top:20px;width:40px;padding:5px;background:#dddddd'><svg id=icon width=20 height=30 viewBox='0 0 20 30'><rect width=20 height=30 fill='#aa2200'/></svg></button>",
    );
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, 0, scale);
        styled(&frame);
        assert_eq!(geometry(rect(&doc, &frame, "#button")), (10, 20, 50, 40));
        assert_eq!(geometry(rect(&doc, &frame, "#icon")), (15, 25, 20, 30));
        assert_eq!(pixel(&frame, 20, 30), 0xaa2200);
        assert_eq!(pixel(&frame, 55, 55), 0xdddddd);
        let id = doc.query_selector(0, "#button").unwrap().unwrap();
        assert!(frame.hits.iter().any(
            |hit| matches!(hit.action, Action::Submit(node) if node == id)
                && (hit.x, hit.y, hit.w, hit.h) == (10, 20, 50, 40)
        ));
    }
}

#[test]
fn zero_width_positioned_container_keeps_visible_overflow_and_honors_hidden_clip() {
    for overflow in ["visible", "hidden"] {
        let doc = document(&format!(
            "<div style='height:30px'></div><div id=anchor style='position:relative;width:0;height:40px;overflow:{overflow}'><a id=card href=/forecast style='position:absolute;left:0;top:0;width:120px;height:40px;background:#aabbcc'></a></div><div id=after style='height:10px'></div>"
        ));
        let frame = frame(&doc, 0, 1.0);
        styled(&frame);
        assert_eq!(geometry(rect(&doc, &frame, "#anchor")), (0, 30, 0, 40));
        assert_eq!(geometry(rect(&doc, &frame, "#after")), (0, 70, 320, 10));
        if overflow == "visible" {
            assert_eq!(pixel(&frame, 60, 50), 0xaabbcc);
            assert_eq!(
                hit(&frame, 60, 50),
                Some("https://fixture.example/forecast")
            );
        } else {
            assert_eq!(pixel(&frame, 60, 50), 0xffffff);
            assert_eq!(hit(&frame, 60, 50), None);
        }
    }
}

#[test]
fn absolute_descendants_honor_every_nonvisible_element_overflow_mode() {
    for overflow in ["visible", "hidden", "clip", "auto", "scroll"] {
        let doc = document(&format!(
            "<div style='position:relative;width:60px;height:40px;overflow:{overflow}'><a id=child href=/child style='position:absolute;left:40px;top:20px;width:40px;height:60px;background:#aa2200'></a></div>"
        ));
        for scale in [1.0, 1.25, 2.0] {
            let frame = frame(&doc, 0, scale);
            styled(&frame);
            assert_eq!(pixel(&frame, 50, 30), 0xaa2200, "{overflow}");
            assert_eq!(hit(&frame, 50, 30), Some("https://fixture.example/child"));
            let visible = overflow == "visible";
            assert_eq!(
                geometry(rect(&doc, &frame, "#child")),
                (
                    40,
                    20,
                    if visible { 40 } else { 20 },
                    if visible { 60 } else { 20 }
                ),
                "{overflow} at {scale}"
            );
            for (x, y) in [(65, 30), (50, 50)] {
                assert_eq!(
                    pixel(&frame, x, y),
                    if visible { 0xaa2200 } else { 0xffffff },
                    "{overflow}"
                );
                assert_eq!(
                    hit(&frame, x, y),
                    visible.then_some("https://fixture.example/child"),
                    "{overflow}"
                );
            }
            assert_eq!(
                frame.content_height,
                if visible { 80 } else { 40 },
                "{overflow}"
            );
        }
    }
    // The propagated body donor is still a viewport clip, not the body's
    // first document-coordinate box, including for separately painted abspos.
    let doc = document(
        "<style>html,body{height:100%}body{overflow:auto}</style><a href=/below style='position:absolute;left:0;top:240px;width:40px;height:40px;background:#aa2200'></a>",
    );
    let scrolled = frame(&doc, 240, 1.0);
    styled(&scrolled);
    assert_eq!(scrolled.content_height, 280);
    assert_eq!(pixel(&scrolled, 10, 10), 0xaa2200);
    assert_eq!(
        hit(&scrolled, 10, 10),
        Some("https://fixture.example/below")
    );
}

#[test]
fn negative_insets_and_numeric_z_order_keep_pixels_and_links_aligned() {
    let doc = document(
        "<a href=/green style='position:absolute;left:-10px;top:-5px;width:60px;height:40px;background:#00aa00;z-index:2'></a><a href=/red style='position:absolute;left:0;top:0;width:80px;height:50px;background:#aa0000;z-index:-1'></a><a href=/blue style='position:absolute;left:20px;top:10px;width:20px;height:20px;background:#0000aa;z-index:2'></a>",
    );
    let frame = frame(&doc, 0, 1.0);
    styled(&frame);
    for (x, y, color, path) in [
        (5, 5, 0x00aa00, "green"),
        (25, 15, 0x0000aa, "blue"),
        (70, 20, 0xaa0000, "red"),
    ] {
        assert_eq!(pixel(&frame, x, y), color);
        assert_eq!(
            hit(&frame, x, y),
            Some(format!("https://fixture.example/{path}").as_str())
        );
    }
}

#[test]
fn positioned_ancestors_are_allocated_before_nested_boxes_and_fixed_escapes_clips() {
    let doc = document(
        "<div style='position:relative;width:30px;height:30px;overflow:hidden'><div id=outer style='position:absolute;left:10px;top:10px;width:100px;height:60px;z-index:4'><a id=nested href=/nested style='position:absolute;left:5px;top:7px;width:10px;height:10px;background:#55aa55;z-index:-2'></a><a id=fixed href=/fixed style='position:fixed;left:100px;top:40px;width:30px;height:20px;background:#aa55aa'></a></div></div><div style='height:400px'></div>",
    );
    let frame = frame(&doc, 0, 1.0);
    styled(&frame);
    assert_eq!(geometry(rect(&doc, &frame, "#nested")), (15, 17, 10, 10));
    assert_eq!(pixel(&frame, 18, 20), 0x55aa55);
    assert_eq!(geometry(rect(&doc, &frame, "#fixed")), (100, 40, 30, 20));
    assert_eq!(pixel(&frame, 110, 45), 0xaa55aa);
    assert_eq!(hit(&frame, 110, 45), Some("https://fixture.example/fixed"));
}

#[test]
fn hidden_ancestors_and_global_positioned_limit_fail_without_partial_layout() {
    let doc = document(
        "<div style='display:none'><a id=hidden href=/hidden style='position:fixed;width:30px;height:30px;background:red'></a></div><div id=shown style='height:20px'></div>",
    );
    let visible = frame(&doc, 0, 1.0);
    styled(&visible);
    let hidden = doc.query_selector(0, "#hidden").unwrap().unwrap();
    assert!(!visible.boxes.iter().any(|r| r.node == hidden));
    assert!(visible.hits.is_empty());
    for count in [512, 513] {
        let bounded = document(
            &"<div style='position:absolute;left:0;top:0;width:1px;height:1px'></div>"
                .repeat(count),
        );
        let rendered = frame(&bounded, 0, 1.0);
        assert_eq!(
            rendered
                .diagnostics
                .entries
                .iter()
                .any(|diagnostic| diagnostic.kind == "layout-fallback"),
            count > 512
        );
        if count > 512 {
            let failure = rendered
                .diagnostics
                .entries
                .iter()
                .find(|diagnostic| diagnostic.kind == "layout-fallback")
                .unwrap();
            assert!(
                failure.message.contains("positioned node limit (512)"),
                "{}",
                failure.message
            );
            assert!(failure.node.is_some());
        }
    }
    for inset in [-2_000_000, 2_000_000] {
        let extreme = document(&format!(
            "<div style='position:absolute;left:{inset}px;top:0;width:1px;height:1px'></div>"
        ));
        let rejected = frame(&extreme, 0, 1.0);
        assert!(
            rejected
                .diagnostics
                .entries
                .iter()
                .any(|diagnostic| diagnostic.kind == "layout-fallback")
        );
        let failure = rejected
            .diagnostics
            .entries
            .iter()
            .find(|diagnostic| diagnostic.kind == "layout-fallback")
            .unwrap();
        assert!(
            failure
                .message
                .contains("positioned box exceeds finite extent limit"),
            "{}",
            failure.message
        );
        assert!(failure.node.is_some());
    }
}
