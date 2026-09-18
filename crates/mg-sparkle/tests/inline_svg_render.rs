//! Public inline-SVG rendering, not screenshots or claims about a live site.
use mg_sparkle::{
    document::{Document, ResourceData, parse},
    paint::Fonts,
    render::{Action, Controls, Frame, LayoutBox, Viewport, render_scaled},
};

fn document(body: &str) -> Document {
    parse(
        &format!(
            "<html><head><style>html,body{{margin:0;background:white}} *{{margin:0;padding:0;border:0;font-size:12px;line-height:20px}} a{{text-decoration:none}}</style></head><body>{body}</body></html>"
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
    let frame = render_scaled(
        document,
        &mut fonts,
        Viewport {
            width: 320,
            height: 200,
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
        "must use the actual styled path: {:?}",
        frame.diagnostics
    );
    frame
}

fn supported(frame: &Frame) {
    assert!(
        !frame
            .diagnostics
            .entries
            .iter()
            .any(|diagnostic| diagnostic.kind == "image-unsupported"),
        "SVG must actually decode rather than use an image fallback: {:?}",
        frame.diagnostics
    );
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
fn inline_svg_is_one_replaced_box_with_real_current_color_cascade() {
    let doc = document(
        "<style>.icon{display:block;width:24px;height:12px;fill:currentColor;color:#2468ac}.child{color:#ac6824}</style><svg id=icon class=icon viewBox='0 0 2 1' fill='none'><g class=child><path id=path d='M0 0H2V1H0Z'/></g></svg>",
    );
    let frame = frame(&doc, 0, 1.0);
    supported(&frame);
    assert_eq!(geometry(rect(&doc, &frame, "#icon")), (0, 0, 24, 12));
    assert_eq!(pixel(&frame, 12, 6), 0xac6824);
    assert_eq!(pixel(&frame, 30, 6), 0xffffff);
    let path = doc.query_selector(0, "#path").unwrap().unwrap();
    assert!(
        !frame.boxes.iter().any(|rect| rect.node == path),
        "SVG shapes must not become separate HTML layout boxes"
    );
}

#[test]
fn inline_svg_alpha_blends_once_onto_the_page() {
    let doc = document(
        "<style>#icon{display:block;width:24px;height:12px;fill:currentColor;color:rgba(32,96,160,.5);fill-opacity:.5}</style><svg id=icon viewBox='0 0 2 1'><rect width='2' height='1'/></svg>",
    );
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, 0, scale);
        supported(&frame);
        assert_eq!(
            pixel(&frame, 12, 6),
            0xc7d7e7,
            "quarter-opacity color over white at scale {scale}"
        );
        assert_eq!(geometry(rect(&doc, &frame, "#icon")), (0, 0, 24, 12));
    }
}

#[test]
fn flex_svg_width_uses_natural_viewbox_aspect_ratio() {
    let doc = document(
        "<style>#row{display:flex;gap:10px;align-items:flex-start} #wide{flex:none;width:80px;height:auto;fill:#2468ac} #small{flex:none;fill:#ac6824}</style><div id=row><svg id=wide viewBox='0 0 4 2'><rect width='4' height='2'/></svg><svg id=small width='40' height='20' viewBox='0 0 4 2'><rect width='4' height='2'/></svg></div>",
    );
    let frame = frame(&doc, 0, 1.0);
    supported(&frame);
    assert_eq!(geometry(rect(&doc, &frame, "#wide")), (0, 0, 80, 40));
    assert_eq!(geometry(rect(&doc, &frame, "#small")), (90, 0, 40, 20));
    assert_eq!(rect(&doc, &frame, "#row").height, 40);
    assert_eq!(pixel(&frame, 40, 20), 0x2468ac);
    assert_eq!(pixel(&frame, 110, 10), 0xac6824);
}

#[test]
fn grid_svg_percent_width_uses_allocated_track_and_intrinsic_aspect() {
    let doc = document(
        "<style>#grid{display:grid;width:210px;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px;align-items:start} #grid>svg{width:100%;height:auto} #wide{fill:#2468ac} #tall{fill:#ac6824}</style><div id=grid><svg id=wide viewBox='0 0 2 1'><rect width='2' height='1'/></svg><svg id=tall viewBox='0 0 1 2'><rect width='1' height='2'/></svg></div>",
    );
    let frame = frame(&doc, 0, 1.0);
    supported(&frame);
    assert_eq!(geometry(rect(&doc, &frame, "#wide")), (0, 0, 100, 50));
    assert_eq!(geometry(rect(&doc, &frame, "#tall")), (110, 0, 100, 200));
    assert_eq!(rect(&doc, &frame, "#grid").height, 200);
    assert_eq!(pixel(&frame, 50, 25), 0x2468ac);
    assert_eq!(pixel(&frame, 160, 150), 0xac6824);
}

#[test]
fn svg_pixels_and_anchor_hits_share_scrolled_overflow_clip_at_each_scale() {
    let doc = document(
        "<style>#clip{width:50px;height:25px;overflow:hidden;margin-left:10px} a{display:block;width:100px;height:50px} svg{display:block;width:100px;height:50px;fill:#2468ac}</style><div style='height:30px'></div><div id=clip><a id=link href=/icon><svg id=icon viewBox='0 0 2 1'><rect width='2' height='1'/></svg></a></div><div style='height:400px'></div>",
    );
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, 20, scale);
        supported(&frame);
        // Public inspection boxes, like hit regions, expose the visible clip.
        assert_eq!(geometry(rect(&doc, &frame, "#icon")), (10, 10, 50, 25));
        assert_eq!(pixel(&frame, 20, 20), 0x2468ac);
        for (x, y) in [(9, 20), (60, 20), (20, 35)] {
            assert_eq!(pixel(&frame, x, y), 0xffffff);
            assert_eq!(
                hit(&frame, x, y),
                None,
                "clipped SVG has no link at ({x}, {y}) scale {scale}"
            );
        }
        assert_eq!(hit(&frame, 20, 20), Some("https://fixture.example/icon"));
    }
}

#[test]
fn embedded_svg_image_is_rejected_even_when_its_url_is_in_resource_cache() {
    let mut doc = document(
        "<style>svg{display:block;width:20px;height:20px}</style><svg id=icon viewBox='0 0 20 20'><image href='https://fixture.example/embedded.svg' width='20' height='20'/></svg>",
    );
    doc.resources.insert("https://fixture.example/embedded.svg".into(), ResourceData {
        bytes: b"<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><rect width='20' height='20' fill='#ff00ff'/></svg>".to_vec(),
        content_type: "image/svg+xml".into(),
    });
    let frame = frame(&doc, 0, 1.0);
    let id = doc.query_selector(0, "#icon").unwrap().unwrap();
    assert!(
        frame
            .diagnostics
            .entries
            .iter()
            .any(|diagnostic| diagnostic.kind == "image-unsupported"
                && diagnostic.node == Some(id)
                && diagnostic.message.contains("only simple shapes")),
        "{:?}",
        frame.diagnostics
    );
    assert_eq!(
        pixel(&frame, 10, 10),
        0xdddddd,
        "unsupported subtree retains image fallback, never embedded resource pixels"
    );
}

#[test]
fn viewbox_preserve_aspect_ratio_retains_letterboxing() {
    let doc = document(
        "<style>svg{display:block;width:40px;height:40px;fill:#2468ac}</style><svg id=icon viewBox='0 0 20 10' preserveAspectRatio='xMidYMid meet'><rect width='20' height='10'/></svg>",
    );
    let frame = frame(&doc, 0, 1.0);
    supported(&frame);
    assert_eq!(geometry(rect(&doc, &frame, "#icon")), (0, 0, 40, 40));
    assert_eq!(pixel(&frame, 20, 20), 0x2468ac);
    assert_eq!(pixel(&frame, 20, 4), 0xffffff);
    assert_eq!(pixel(&frame, 20, 36), 0xffffff);
}

#[test]
fn structured_button_paints_dom_children_and_keeps_whole_button_action() {
    let doc = document(
        "<style>#control{display:inline-flex;align-items:center;gap:4px;width:100px;height:40px;padding:8px;box-sizing:border-box;background:#eeeeee} #control svg{width:20px;height:10px;fill:#2468ac} #label{display:block;width:20px;height:10px;background:#ac6824}</style><form action=/done><button id=control name=action value=go><svg id=icon viewBox='0 0 2 1'><rect width='2' height='1'/></svg><span id=label></span></button></form>",
    );
    let control = doc.query_selector(0, "#control").unwrap().unwrap();
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, 0, scale);
        supported(&frame);
        let button = rect(&doc, &frame, "#control");
        assert_eq!((button.width, button.height), (100, 40));
        let (x, y) = (button.x, button.y);
        assert_eq!(
            geometry(rect(&doc, &frame, "#icon")),
            (x + 8, y + 15, 20, 10)
        );
        assert_eq!(pixel(&frame, x + 18, y + 20), 0x2468ac);
        assert_eq!(pixel(&frame, x + 42, y + 20), 0xac6824);
        assert_eq!(pixel(&frame, x + 95, y + 35), 0xeeeeee);
        for (cx, cy) in [(x + 18, y + 20), (x + 95, y + 35)] {
            let hit = frame
                .hits
                .iter()
                .rev()
                .find(|hit| {
                    cx >= hit.x
                        && cy >= hit.y
                        && cx < hit.x + hit.w as i32
                        && cy < hit.y + hit.h as i32
                })
                .unwrap();
            assert!(matches!(hit.action, Action::Submit(node) if node == control));
        }
    }
}

#[test]
fn text_only_button_keeps_legacy_control_path_beside_structured_button() {
    let reference = document("<form><button id=plain>Plain</button></form>");
    let original = frame(&reference, 0, 1.0);
    let doc = document(
        "<style>#structured{display:flex;width:80px;height:30px} #structured svg{width:20px;height:10px;fill:#2468ac}</style><form><button id=plain>Plain</button><button id=structured><svg id=icon viewBox='0 0 2 1'><rect width='2' height='1'/></svg></button></form>",
    );
    let rendered = frame(&doc, 0, 1.0);
    supported(&rendered);
    let old = rect(&reference, &original, "#plain");
    let new = rect(&doc, &rendered, "#plain");
    assert_eq!(geometry(old), geometry(new));
    for y in old.y..old.y + old.height as i32 {
        for x in old.x..old.x + old.width as i32 {
            assert_eq!(pixel(&original, x, y), pixel(&rendered, x, y));
        }
    }
    assert_eq!(rect(&doc, &rendered, "#icon").width, 20);
    let plain = doc.query_selector(0, "#plain").unwrap().unwrap();
    assert!(
        rendered
            .hits
            .iter()
            .any(|hit| matches!(hit.action, Action::Submit(id) if id == plain))
    );
}

#[test]
fn svg_css_hidden_shapes_and_groups_match_presentation_attributes() {
    for (property, value) in [("display", "none"), ("visibility", "hidden")] {
        for tag in ["rect", "g"] {
            let content = |attribute: &str, rule: &str| {
                let shape = if tag == "rect" {
                    format!("<rect class=hidden width='20' height='10' fill='blue' {attribute}/>")
                } else {
                    format!(
                        "<g class=hidden {attribute}><rect width='20' height='10' fill='blue'/></g>"
                    )
                };
                format!(
                    "<style>svg{{display:block}}{rule}</style><svg id=icon width='20' height='10'><rect width='20' height='10' fill='red'/>{shape}</svg>"
                )
            };
            let css = document(&content("", &format!(".hidden{{{property}:{value}}}")));
            let attr = document(&content(&format!("{property}='{value}'"), ""));
            for scale in [1., 1.25, 2.] {
                let css_frame = frame(&css, 0, scale);
                let attr_frame = frame(&attr, 0, scale);
                supported(&css_frame);
                supported(&attr_frame);
                assert_eq!(
                    css_frame.canvas.pixels, attr_frame.canvas.pixels,
                    "{property}, {tag}, {scale}"
                );
                assert_eq!(pixel(&css_frame, 5, 5), 0xff0000);
            }
        }
    }
}

#[test]
fn svg_visible_child_overrides_hidden_visibility_but_not_display_none_ancestor() {
    for (property, value, expected) in [
        ("display", "none", 0xff0000),
        ("visibility", "hidden", 0x008000),
    ] {
        let content = |attribute: &str, rule: &str| {
            format!(
                "<style>svg{{display:block}}{rule}</style><svg id=icon width='20' height='10'><rect width='20' height='10' fill='red'/><g class=hidden {attribute}><rect width='10' height='10' fill='blue'/><rect class=shown x='10' width='10' height='10' fill='green' visibility='visible'/></g></svg>"
            )
        };
        let css = document(&content(
            "",
            &format!(".hidden{{{property}:{value}}}.shown{{visibility:visible}}"),
        ));
        let attr = document(&content(&format!("{property}='{value}'"), ""));
        let css_frame = frame(&css, 0, 1.);
        let attr_frame = frame(&attr, 0, 1.);
        supported(&css_frame);
        supported(&attr_frame);
        assert_eq!(css_frame.canvas.pixels, attr_frame.canvas.pixels);
        assert_eq!(pixel(&css_frame, 5, 5), 0xff0000);
        assert_eq!(pixel(&css_frame, 15, 5), expected);
    }
}

#[test]
fn svg_author_display_and_visibility_override_presentation_attributes() {
    let doc = document(
        "<style>svg{display:block}.shown{display:inline!important;visibility:visible!important}</style><svg id=icon width='20' height='10'><g class=shown display='none' visibility='hidden'><rect width='20' height='10' fill='red'/></g></svg>",
    );
    let rendered = frame(&doc, 0, 1.);
    supported(&rendered);
    assert_eq!(pixel(&rendered, 10, 5), 0xff0000);
}

#[test]
fn hidden_svg_root_can_paint_explicitly_visible_child_without_root_link_hit() {
    let doc = document(
        "<style>svg{display:block}.hidden{visibility:hidden}.shown{visibility:visible}</style><a href=/hidden><svg id=icon class=hidden width='20' height='10'><rect width='10' height='10' fill='blue'/><rect class=shown x='10' width='10' height='10' fill='green'/></svg></a>",
    );
    let rendered = frame(&doc, 0, 1.);
    supported(&rendered);
    assert_eq!(pixel(&rendered, 5, 5), 0xffffff);
    assert_eq!(pixel(&rendered, 15, 5), 0x008000);
    assert!(rendered.hits.is_empty());
    let rejected = document(
        "<style>.hidden{visibility:hidden}</style><svg class=hidden width='20' height='10' aria-label='hidden'><image href='https://fixture.example/not-fetched'/></svg>",
    );
    let rendered = frame(&rejected, 0, 1.);
    assert!(
        rendered
            .diagnostics
            .entries
            .iter()
            .any(|entry| entry.kind == "image-unsupported")
    );
    assert!(
        rendered
            .canvas
            .pixels
            .iter()
            .all(|&pixel| pixel == 0xffffff)
    );
}
