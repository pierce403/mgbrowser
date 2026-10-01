//! Intrinsic width keywords: generic sizing, not site-specific CSS.
use mg_sparkle::{
    document::{Document, parse},
    paint::Fonts,
    render::{Controls, Frame, LayoutBox, Viewport, render_scaled},
    style,
};

fn document(body: &str) -> Document {
    parse(
        &format!(
            "<style>html,body{{margin:0}} *{{margin:0;padding:0;border:0;font-size:12px;line-height:20px}}</style>{body}"
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
fn frame(document: &Document, width: u32, scale: f32) -> Frame {
    let frame = render_scaled(
        document,
        &mut fonts(),
        Viewport {
            width,
            height: 300,
            scroll: 0,
        },
        &Controls::default(),
        scale,
    );
    assert!(
        !frame
            .diagnostics
            .entries
            .iter()
            .any(|d| matches!(d.kind, "css-parse" | "style-fallback" | "layout-fallback")),
        "{:?}",
        frame.diagnostics
    );
    frame
}
fn rect<'a>(doc: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let id = doc.query_selector(0, selector).unwrap().unwrap();
    frame.boxes.iter().rev().find(|r| r.node == id).unwrap()
}

#[test]
fn replaced_fit_content_transfers_definite_height_through_natural_ratio() {
    for context in [
        "display:block",
        "display:flex;align-items:start",
        "position:relative",
    ] {
        for (sizing, height, edges) in [("content-box", 60, 0), ("border-box", 84, 24)] {
            for constraints in [
                "width:200px;max-width:fit-content",
                "width:20px;min-width:fit-content;max-width:30px",
            ] {
                let position = if context == "position:relative" {
                    "position:absolute"
                } else {
                    ""
                };
                let decoration = if edges == 0 {
                    ""
                } else {
                    "padding:10px;border:2px solid"
                };
                let doc = document(&format!(
                    "<style>#fit{{display:block;box-sizing:{sizing};height:{height}px;{constraints};{decoration};{position}}}</style><div style='{context};width:400px;height:160px'><svg id=fit width='40' height='20' viewBox='0 0 40 20'><rect width='40' height='20' fill='red'/></svg></div>"
                ));
                for scale in [1.0, 1.25, 2.0] {
                    let f = frame(&doc, 420, scale);
                    let r = rect(&doc, &f, "#fit");
                    assert_eq!(
                        (r.width, r.height),
                        (120 + edges, 60 + edges),
                        "{context} {sizing} {constraints} scale={scale}"
                    );
                }
            }
        }
    }
}

#[test]
fn block_fit_content_clamps_stretch_between_content_minimum_and_maximum() {
    let doc = document(
        "<div id=fit style='width:fit-content;padding:3px;border:2px solid;margin:0 5px'>mmmm mmmm</div>",
    );
    let mut font = fonts();
    let min = font.width("mmmm", 12.0);
    let max = font.width("mmmm mmmm", 12.0);
    for width in [220, 80, 40] {
        for scale in [1.0, 1.25, 2.0] {
            let frame = frame(&doc, width, scale);
            let r = rect(&doc, &frame, "#fit");
            let expected = max.min(min.max((width as f32 - 20.0).max(0.0))) + 10.0;
            assert_eq!(
                (r.x, r.width),
                (5, expected.ceil() as u32),
                "width={width} scale={scale}"
            );
            assert!(r.height >= 30);
        }
    }
}

#[test]
fn fit_content_edges_and_numeric_min_max_apply_in_the_declared_sizing_box() {
    for (sizing, constraint, expected) in [
        ("content-box", "", 94),
        ("border-box", "", 94),
        ("content-box", "max-width:70px", 84),
        ("border-box", "max-width:70px", 70),
        ("content-box", "min-width:120px", 134),
        ("border-box", "min-width:120px", 120),
    ] {
        let doc = document(&format!(
            "<div id=fit style='width:fit-content;padding:5px;border:2px solid;box-sizing:{sizing};{constraint}'><div style='width:80px;height:20px'></div></div>"
        ));
        assert_eq!(
            rect(&doc, &frame(&doc, 240, 1.0), "#fit").width,
            expected,
            "{sizing} {constraint}"
        );
    }
}

#[test]
fn flex_and_grid_items_keep_explicit_fit_content_instead_of_stretching() {
    for container in ["display:flex", "display:grid;grid-template-columns:1fr"] {
        let doc = document(&format!(
            "<div style='{container};width:240px'><div id=fit style='width:fit-content;box-sizing:border-box;padding:5px;border:2px solid'><div style='width:80px;height:20px'></div></div></div>"
        ));
        for scale in [1.0, 1.25, 2.0] {
            let frame = frame(&doc, 320, scale);
            let r = rect(&doc, &frame, "#fit");
            assert_eq!((r.width, r.height), (94, 34), "{container} scale={scale}");
        }
    }
}

#[test]
fn positioned_fit_content_does_not_become_auto_opposite_inset_stretch() {
    let doc = document(
        "<div style='position:relative;width:200px;height:100px'><div id=fit style='position:absolute;left:20px;right:40px;top:0;width:fit-content;padding:5px;border:2px solid'><div style='width:80px;height:20px'></div></div><div id=auto style='position:absolute;left:20px;right:40px;top:50px;height:20px'></div></div>",
    );
    let frame = frame(&doc, 320, 1.0);
    let fit = rect(&doc, &frame, "#fit");
    let auto = rect(&doc, &frame, "#auto");
    assert_eq!((fit.x, fit.width), (20, 94));
    assert_eq!((auto.x, auto.width), (20, 140));
}

#[test]
fn fit_content_snapshot_is_distinct_and_does_not_clear_unrelated_loss() {
    let doc = document(
        "<div id=fit style='width:fit-content'></div><div id=auto style='width:auto'></div><div id=bad style='width:fit-content;height:calc(50% + 3px)'></div><div id=function style='width:fit-content(50px)'></div>",
    );
    let styles = style::compute_styles(&doc, &doc.stylesheets, (320.0, 300.0)).unwrap();
    let at = |selector| &styles[doc.query_selector(0, selector).unwrap().unwrap()];
    assert!(at("#fit").layout.width_fit_content);
    assert!(!at("#fit").layout.unsupported);
    assert!(!at("#auto").layout.width_fit_content);
    assert!(at("#bad").layout.width_fit_content);
    assert!(at("#bad").layout.unsupported);
    assert!(!at("#function").layout.width_fit_content);
    assert!(at("#function").layout.unsupported);
}

#[test]
fn fit_content_maximum_caps_numeric_width_but_numeric_minimum_wins() {
    for (sizing, minimum, expected) in [
        ("content-box", "", 94),
        ("border-box", "", 94),
        ("content-box", "min-width:120px", 134),
        ("border-box", "min-width:120px", 120),
    ] {
        let doc = document(&format!(
            "<div id=fit style='width:200px;max-width:fit-content;box-sizing:{sizing};padding:5px;border:2px solid;{minimum}'><div style='width:80px;height:20px'></div></div>"
        ));
        assert_eq!(
            rect(&doc, &frame(&doc, 320, 1.0), "#fit").width,
            expected,
            "{sizing} {minimum}"
        );
    }
}

#[test]
fn flex_fit_content_maximum_prevents_growth_past_real_content() {
    let doc = document(
        "<div style='display:flex;width:240px'><div id=fit style='display:flex;flex-grow:1;min-width:0;max-width:fit-content;box-sizing:border-box;padding:5px;border:2px solid'><div style='width:80px;height:20px'></div></div></div>",
    );
    for scale in [1.0, 1.25, 2.0] {
        let f = frame(&doc, 320, scale);
        assert_eq!(rect(&doc, &f, "#fit").width, 94);
    }
}

#[test]
fn positioned_maximum_clamps_opposite_insets_without_changing_auto_width_rules() {
    let doc = document(
        "<div style='position:relative;width:200px;height:100px'><div id=fit style='position:absolute;left:20px;right:40px;top:0;max-width:fit-content;padding:5px;border:2px solid'><div style='width:80px;height:20px'></div></div></div>",
    );
    let f = frame(&doc, 320, 1.0);
    let r = rect(&doc, &f, "#fit");
    assert_eq!((r.x, r.width), (20, 94));
}

#[test]
fn replaced_maximum_uses_natural_content_and_preserves_auto_height_ratio() {
    for (constraint, expected_height) in [
        ("", 20),
        ("max-height:25px", 20),
        ("max-height:10px", 10),
        ("min-height:30px", 30),
    ] {
        let doc = document(&format!(
            "<style>#fit{{display:block;width:80px;height:auto;max-width:fit-content;{constraint}}}</style><svg id=fit width=40 height=20 viewBox='0 0 40 20'><rect width=40 height=20 fill='red'/></svg>"
        ));
        let f = frame(&doc, 320, 1.0);
        let r = rect(&doc, &f, "#fit");
        assert_eq!((r.width, r.height), (40, expected_height), "{constraint}");
    }
}

#[test]
fn max_keyword_preservation_keeps_function_and_unresolved_grid_area_truthful() {
    let doc = document(
        "<div id=fit style='max-width:fit-content'></div><div id=bad style='max-width:fit-content(50px)'></div>",
    );
    let styles = style::compute_styles(&doc, &doc.stylesheets, (320., 300.)).unwrap();
    let fit = &styles[doc.query_selector(0, "#fit").unwrap().unwrap()];
    let bad = &styles[doc.query_selector(0, "#bad").unwrap().unwrap()];
    assert!(fit.layout.max_width_fit_content && !fit.layout.unsupported);
    assert!(!bad.layout.max_width_fit_content && bad.layout.unsupported);
    let doc = document(
        "<div style='display:grid;grid-template-columns:1fr 1fr'><div style='width:200px;max-width:fit-content'>text</div></div>",
    );
    let f = render_scaled(
        &doc,
        &mut fonts(),
        Viewport {
            width: 320,
            height: 300,
            scroll: 0,
        },
        &Controls::default(),
        1.0,
    );
    assert!(
        f.diagnostics
            .entries
            .iter()
            .any(|d| d.kind == "layout-fallback" && d.message.contains("resolved grid-area width")),
        "{:?}",
        f.diagnostics
    );
}

#[test]
fn fit_content_minimum_wins_numeric_maximum_in_block_flex_and_positioned_paths() {
    for context in ["display:block", "display:flex", "position:relative"] {
        for sizing in ["content-box", "border-box"] {
            for width in ["20px", "fit-content"] {
                let position = if context == "position:relative" {
                    "position:absolute;left:0;right:0;top:0;"
                } else {
                    ""
                };
                let doc = document(&format!(
                    "<div style='{context};width:240px;height:100px'><div id=fit style='{position}width:{width};min-width:fit-content;max-width:30px;box-sizing:{sizing};padding:5px;border:2px solid'><div style='width:80px;height:20px'></div></div></div>"
                ));
                let f = frame(&doc, 320, 1.0);
                assert_eq!(
                    rect(&doc, &f, "#fit").width,
                    94,
                    "{context};{sizing};width:{width}"
                );
            }
        }
    }
}

#[test]
fn fit_content_minimum_clamps_available_text_width_and_combines_with_maximum() {
    let mut font = fonts();
    let min = font.width("mmmm", 12.0);
    let max = font.width("mmmm mmmm", 12.0);
    for constraint in [
        "width:0;min-width:fit-content",
        "width:fit-content;min-width:fit-content;max-width:fit-content",
    ] {
        let doc = document(&format!(
            "<div id=fit style='{constraint};padding:3px;border:2px solid;margin:0 5px'>mmmm mmmm</div>"
        ));
        for width in [220, 80, 40] {
            let f = frame(&doc, width, 1.0);
            let expected = max.min(min.max((width as f32 - 20.0).max(0.0))) + 10.0;
            assert_eq!(
                rect(&doc, &f, "#fit").width,
                expected.ceil() as u32,
                "{constraint};available={width}"
            );
        }
    }
}

#[test]
fn min_keyword_is_typed_but_function_and_grid_area_remain_explicit_boundaries() {
    let doc = document(
        "<div id=fit style='min-width:fit-content'></div><div id=bad style='min-width:fit-content(50px)'></div>",
    );
    let styles = style::compute_styles(&doc, &doc.stylesheets, (320., 300.)).unwrap();
    let fit = &styles[doc.query_selector(0, "#fit").unwrap().unwrap()];
    let bad = &styles[doc.query_selector(0, "#bad").unwrap().unwrap()];
    assert_eq!(
        fit.layout.min_width_intrinsic,
        Some(style::IntrinsicSize::FitContent)
    );
    assert!(!fit.layout.unsupported);
    assert_eq!(bad.layout.min_width_intrinsic, None);
    assert!(bad.layout.unsupported);
    let doc = document(
        "<div style='display:grid;grid-template-columns:1fr 1fr'><div style='min-width:fit-content'>text</div></div>",
    );
    let f = render_scaled(
        &doc,
        &mut fonts(),
        Viewport {
            width: 320,
            height: 300,
            scroll: 0,
        },
        &Controls::default(),
        1.0,
    );
    assert!(
        f.diagnostics
            .entries
            .iter()
            .any(|d| d.kind == "layout-fallback"
                && d.message.contains(
                    "fit-content min-width on a grid item requires its resolved grid-area width"
                )),
        "{:?}",
        f.diagnostics
    );
}
