//! Generic layout boundary regressions, not website-specific acceptance.
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

fn frame(document: &Document, scale: f32) -> Frame {
    let mut fonts = Fonts::from_bytes(
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap(),
    )
    .unwrap();
    let frame = render_scaled(
        document,
        &mut fonts,
        Viewport {
            width: 320,
            height: 200,
            scroll: 0,
        },
        &Controls::default(),
        scale,
    );
    assert!(
        !frame.diagnostics.entries.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            "css-parse" | "style-fallback" | "layout-fallback"
        )),
        "regression must exercise styled layout: {:?}",
        frame.diagnostics
    );
    frame
}

fn rect<'a>(document: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let id = document.query_selector(0, selector).unwrap().unwrap();
    frame.boxes.iter().rev().find(|r| r.node == id).unwrap()
}

fn geometry(r: &LayoutBox) -> (i32, i32, u32, u32) {
    (r.x, r.y, r.width, r.height)
}

fn link_at(frame: &Frame, x: i32, y: i32) -> Option<&str> {
    frame.hits.iter().rev().find_map(|hit| {
        if x >= hit.x
            && y >= hit.y
            && i64::from(x) < i64::from(hit.x) + i64::from(hit.w)
            && i64::from(y) < i64::from(hit.y) + i64::from(hit.h)
        {
            if let Action::Link { href, .. } = &hit.action {
                return Some(href.as_str());
            }
        }
        None
    })
}

#[test]
fn border_box_cannot_be_smaller_than_its_padding_and_border() {
    let doc = document(
        "<div id=tiny style='box-sizing:border-box;width:1px;max-width:1px;height:1px;max-height:1px;padding:10px;border:2px solid #223344'></div><div id=next style='height:10px'></div>",
    );
    let frame = frame(&doc, 1.0);
    assert_eq!(geometry(rect(&doc, &frame, "#tiny")), (0, 0, 24, 24));
    assert_eq!(rect(&doc, &frame, "#next").y, 24);
    assert_eq!(frame.content_height, 34);
}

#[test]
fn flex_and_grid_scroll_containers_clip_initial_pixels_and_hits_with_diagnostic() {
    for outer in [
        "display:flex",
        "display:grid;grid-template-columns:80px 20px",
    ] {
        for inner in ["display:flex", "display:grid;grid-template-columns:140px"] {
            for overflow in ["auto", "scroll"] {
                let doc = document(&format!(
                    "<div style='{outer};width:160px'><div id=clip style='{inner};width:80px;height:30px;overflow:{overflow}'><a id=wide href=/wide style='display:block;flex:0 0 140px;width:140px;height:60px;background:#aa0000'></a></div><div id=after style='width:20px;height:20px;background:#0000aa'></div></div>"
                ));
                let clip = doc.query_selector(0, "#clip").unwrap().unwrap();
                for scale in [1.0, 1.25, 2.0] {
                    let f = frame(&doc, scale);
                    assert_eq!(geometry(rect(&doc, &f, "#clip")), (0, 0, 80, 30));
                    assert_eq!(geometry(rect(&doc, &f, "#wide")), (0, 0, 80, 30));
                    assert_eq!(rect(&doc, &f, "#after").x, 80);
                    assert_eq!(f.content_height, 30);
                    assert_eq!(link_at(&f, 10, 10), Some("https://fixture.example/wide"));
                    assert_eq!(link_at(&f, 81, 10), None);
                    assert_eq!(link_at(&f, 10, 31), None);
                    let pixel = |x, y| {
                        let x = f.canvas.physical_edge(x) as usize;
                        let y = f.canvas.physical_edge(y) as usize;
                        f.canvas.pixels[y * f.canvas.width as usize + x]
                    };
                    assert_eq!(pixel(10, 10), 0xaa0000);
                    assert_eq!(pixel(90, 10), 0x0000aa);
                    assert_eq!(pixel(10, 31), 0xffffff);
                    assert!(
                        f.diagnostics.entries.iter().any(|d| d.node == Some(clip)
                            && d.kind == "css-unsupported"
                            && d.message
                                .contains("require element scrolling, which is not implemented")),
                        "{outer};{inner};{overflow}: {:?}",
                        f.diagnostics
                    );
                }
            }
        }
    }
}

#[test]
fn inline_replaced_border_box_dimensions_do_not_add_edges_twice() {
    let mut doc = document(
        "<style>img,input{box-sizing:border-box;width:40px;height:30px;padding:5px;border:2px solid #223344}</style><div><img id=image src=/image.svg></div><div><input id=input></div>",
    );
    doc.resources.insert("https://fixture.example/image.svg".into(), ResourceData {
        bytes: b"<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20' fill='#aa0000'/></svg>".to_vec(),
        content_type: "image/svg+xml".into(),
    });
    let frame = frame(&doc, 1.0);
    assert_eq!(geometry(rect(&doc, &frame, "#image")), (0, 0, 40, 30));
    assert_eq!(geometry(rect(&doc, &frame, "#input")), (0, 30, 40, 30));
    assert_eq!(frame.content_height, 60);
}

#[test]
fn allocated_auto_height_does_not_create_a_definite_percentage_basis() {
    for formatting in [
        "display:flex;align-items:flex-start",
        "display:grid;grid-template-columns:80px;align-items:start;justify-items:start",
    ] {
        let doc = document(&format!(
            "<div id=context style='{formatting}'><div id=auto style='width:80px'><div id=percent style='height:50%'><div style='height:20px'></div></div><div id=tail style='height:60px'></div></div></div><div id=next style='height:10px'></div>"
        ));
        let frame = frame(&doc, 1.0);
        assert_eq!(
            geometry(rect(&doc, &frame, "#auto")),
            (0, 0, 80, 80),
            "{formatting}"
        );
        assert_eq!(rect(&doc, &frame, "#percent").height, 20, "{formatting}");
        assert_eq!(rect(&doc, &frame, "#tail").y, 20, "{formatting}");
        assert_eq!(rect(&doc, &frame, "#next").y, 80, "{formatting}");
        assert_eq!(frame.content_height, 90, "{formatting}");
    }
}

#[test]
fn fully_clipped_far_child_does_not_inflate_scroll_extent() {
    for overflow in ["hidden", "clip"] {
        let doc = document(&format!(
            "<div id=clip style='width:50px;height:20px;overflow:{overflow}'><a id=far href=/far style='display:block;margin-top:1000px;height:20px;background:#aa0000'></a></div><div id=next style='height:10px'></div>"
        ));
        let frame = frame(&doc, 1.0);
        assert_eq!(rect(&doc, &frame, "#next").y, 20);
        assert_eq!(frame.content_height, 30, "overflow:{overflow}");
        assert!(
            !frame
                .hits
                .iter()
                .any(|h| matches!(&h.action, Action::Link { href, .. } if href.ends_with("/far")))
        );
    }
    let visible = document(
        "<div style='width:50px;height:20px;overflow:visible'><div style='margin-top:1000px;height:20px'></div></div><div style='height:10px'></div>",
    );
    assert_eq!(
        frame(&visible, 1.0).content_height,
        1020,
        "visible overflow must remain reachable"
    );
}

#[test]
fn fractional_clip_edges_agree_for_pixels_inspector_and_link_hits() {
    let doc = document(
        "<div style='margin-left:10.6px;width:5px;height:12px;overflow:hidden'><a id=link href=/edge style='display:block;width:20px;height:12px;background:#aa0000'></a></div>",
    );
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, scale);
        assert_eq!(
            geometry(rect(&doc, &frame, "#link")),
            (11, 0, 5, 12),
            "scale {scale}"
        );
        for x in 0..25 {
            assert_eq!(
                link_at(&frame, x, 5),
                (11..16)
                    .contains(&x)
                    .then_some("https://fixture.example/edge"),
                "hit x={x}, scale={scale}"
            );
        }
        let y = frame.canvas.physical_edge(5) as usize;
        let left = frame.canvas.physical_edge(11) as usize;
        let right = frame.canvas.physical_edge(16) as usize;
        for x in 0..frame.canvas.physical_edge(25) as usize {
            let expected = if (left..right).contains(&x) {
                0xaa0000
            } else {
                0xffffff
            };
            assert_eq!(
                frame.canvas.pixels[y * frame.canvas.width as usize + x],
                expected,
                "physical x={x}, scale={scale}"
            );
        }
    }
}

#[test]
fn positioned_child_uses_full_table_cell_border_after_vertical_alignment() {
    for (alignment, content_y) in [("middle", 40), ("bottom", 80)] {
        let doc = document(&format!(
            "<table style='width:200px'><tr><td id=cell style='position:relative;vertical-align:{alignment};width:100px'><div id=copy style='height:20px'></div><a id=bottom href=/bottom style='position:absolute;left:0;bottom:0;width:10px;height:10px;background:#aa0000'></a></td><td style='width:100px'><div style='height:100px'></div></td></tr></table>"
        ));
        let frame = frame(&doc, 1.0);
        assert_eq!(rect(&doc, &frame, "#copy").y, content_y, "{alignment}");
        assert_eq!(
            geometry(rect(&doc, &frame, "#bottom")),
            (0, 90, 10, 10),
            "{alignment}"
        );
        assert_eq!(
            link_at(&frame, 5, 95),
            Some("https://fixture.example/bottom")
        );
        assert_eq!(frame.content_height, 100);
    }
}

#[test]
fn relative_offset_moves_visuals_without_moving_following_flow() {
    let doc = document(
        "<a id=moved href=/moved style='display:block;position:relative;left:7px;top:13px;width:30px;height:20px;background:#aa0000'></a><div id=next style='height:10px'></div>",
    );
    let frame = frame(&doc, 1.0);
    assert_eq!(geometry(rect(&doc, &frame, "#moved")), (7, 13, 30, 20));
    assert_eq!(rect(&doc, &frame, "#next").y, 20);
    assert_eq!(
        link_at(&frame, 8, 14),
        Some("https://fixture.example/moved")
    );
    assert_eq!(frame.content_height, 33);
}
