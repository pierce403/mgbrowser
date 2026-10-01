//! Generic max-content width behavior, independent of the WPT reference pages.
use mg_sparkle::{
    document::{Document, parse},
    paint::Fonts,
    render::{Action, Controls, Frame, LayoutBox, Viewport, render_scaled},
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

fn raw_frame(document: &Document, width: u32, scale: f32) -> Frame {
    render_scaled(
        document,
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

fn frame(document: &Document, width: u32, scale: f32) -> Frame {
    let frame = raw_frame(document, width, scale);
    assert!(
        !frame.diagnostics.entries.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            "css-parse" | "style-fallback" | "layout-fallback" | "image-unsupported"
        )),
        "must use the supported styled path: {:?}",
        frame.diagnostics
    );
    frame
}

fn rect<'a>(doc: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let node = doc.query_selector(0, selector).unwrap().unwrap();
    frame
        .boxes
        .iter()
        .rev()
        .find(|item| item.node == node)
        .unwrap()
}

#[test]
fn max_content_snapshot_is_distinct_from_fit_content_numeric_and_auto() {
    let doc = document(
        "<div id=max style='width:max-content'></div><div id=fit style='width:fit-content'></div><div id=auto style='width:auto'></div><div id=numeric style='width:80px'></div><div id=loss style='width:max-content;height:calc(50% + 3px)'></div>",
    );
    let styles = style::compute_styles(&doc, &doc.stylesheets, (320., 300.)).unwrap();
    let at = |selector| &styles[doc.query_selector(0, selector).unwrap().unwrap()];
    assert!(at("#max").layout.width_max_content);
    assert!(!at("#max").layout.width_fit_content);
    assert!(!at("#max").layout.unsupported);
    assert!(at("#fit").layout.width_fit_content);
    for selector in ["#fit", "#auto", "#numeric"] {
        assert!(!at(selector).layout.width_max_content, "{selector}");
    }
    assert!(at("#loss").layout.width_max_content);
    assert!(at("#loss").layout.unsupported);
}

#[test]
fn unsupported_sizing_values_still_produce_explicit_diagnostics() {
    for unsupported in [
        "width:min-content",
        "max-width:max-content",
        "height:max-content",
        "width:fit-content(50px)",
        "width:max-content;height:calc(50% + 3px)",
    ] {
        let doc = document(&format!("<div id=subject style='{unsupported}'>text</div>"));
        let styles = style::compute_styles(&doc, &doc.stylesheets, (320., 300.)).unwrap();
        let node = doc.query_selector(0, "#subject").unwrap().unwrap();
        assert!(styles[node].layout.unsupported, "{unsupported}");
        let f = raw_frame(&doc, 320, 1.0);
        assert!(
            f.diagnostics
                .entries
                .iter()
                .any(|d| d.kind == "css-unsupported"),
            "{unsupported}: {:?}",
            f.diagnostics
        );
    }
}

#[test]
fn block_and_inline_block_keep_full_text_maximum_beyond_available_width() {
    let text = "mmmm mmmm";
    let expected = (fonts().width(text, 12.0) + 14.).ceil() as u32;
    for display in ["block", "inline-block"] {
        for sizing in ["content-box", "border-box"] {
            let doc = document(&format!(
                "<div id=max style='display:{display};width:max-content;box-sizing:{sizing};padding:5px;border:2px solid'>{text}</div>"
            ));
            for width in [40, 80, 240] {
                for scale in [1.0, 1.25, 2.0] {
                    let f = frame(&doc, width, scale);
                    let r = rect(&doc, &f, "#max");
                    assert_eq!(r.width, expected, "{display} {sizing} {width} {scale}");
                    assert_eq!(r.height, 34, "max-content text must stay on one line");
                }
            }
        }
    }
}

#[test]
fn numeric_constraints_use_declared_sizing_box_and_minimum_wins() {
    for (sizing, constraints, expected) in [
        ("content-box", "", 94),
        ("border-box", "", 94),
        ("content-box", "max-width:70px", 84),
        ("border-box", "max-width:70px", 70),
        ("content-box", "min-width:120px", 134),
        ("border-box", "min-width:120px", 120),
        ("content-box", "min-width:120px;max-width:70px", 134),
        ("border-box", "min-width:120px;max-width:70px", 120),
    ] {
        for display in ["block", "inline-block"] {
            let doc = document(&format!(
                "<div id=max style='display:{display};width:max-content;box-sizing:{sizing};padding:5px;border:2px solid;{constraints}'><div style='width:80px;height:20px'></div></div>"
            ));
            for width in [40, 240] {
                let f = frame(&doc, width, 1.0);
                assert_eq!(
                    rect(&doc, &f, "#max").width,
                    expected,
                    "{display} {sizing} {constraints} {width}"
                );
            }
        }
    }
}

#[test]
fn auto_margins_center_intrinsic_border_box_but_do_not_squeeze_it() {
    let doc = document(
        "<div id=max style='width:max-content;margin-left:auto;margin-right:auto;padding:5px;border:2px solid'><div style='width:80px;height:20px'></div></div>",
    );
    for (width, x) in [(240, 73), (94, 0), (60, 0)] {
        for scale in [1.0, 1.25, 2.0] {
            let f = frame(&doc, width, scale);
            let r = rect(&doc, &f, "#max");
            assert_eq!((r.x, r.width), (x, 94), "width={width} scale={scale}");
        }
    }
}

#[test]
fn flex_and_grid_items_keep_explicit_maximum_instead_of_stretching_or_clamping() {
    for container in ["display:flex", "display:grid;grid-template-columns:1fr"] {
        for available in [60, 240] {
            let doc = document(&format!(
                "<div style='{container};width:{available}px'><div id=max style='width:max-content;flex-shrink:0;box-sizing:border-box;padding:5px;border:2px solid'><div style='width:80px;height:20px'></div></div></div>"
            ));
            for scale in [1.0, 1.25, 2.0] {
                let f = frame(&doc, 320, scale);
                let r = rect(&doc, &f, "#max");
                assert_eq!(
                    (r.width, r.height),
                    (94, 34),
                    "{container} available={available} scale={scale}"
                );
            }
        }
    }
}

#[test]
fn flex_and_grid_numeric_constraints_use_the_selected_sizing_box() {
    for container in ["display:flex", "display:grid;grid-template-columns:1fr"] {
        for (sizing, constraints, expected) in [
            ("content-box", "max-width:70px", 84),
            ("border-box", "max-width:70px", 70),
            ("content-box", "min-width:120px;max-width:70px", 134),
            ("border-box", "min-width:120px;max-width:70px", 120),
        ] {
            let doc = document(&format!(
                "<div style='{container};width:240px'><div id=max style='width:max-content;flex:none;box-sizing:{sizing};padding:5px;border:2px solid;{constraints}'><div style='width:80px;height:20px'></div></div></div>"
            ));
            let f = frame(&doc, 320, 1.0);
            assert_eq!(
                rect(&doc, &f, "#max").width,
                expected,
                "{container} {sizing} {constraints}"
            );
        }
    }
}

#[test]
fn max_content_is_a_flex_basis_not_an_implicit_no_shrink_rule() {
    for (flex, expected) in [("flex-shrink:1;min-width:0", 40), ("flex:none", 94)] {
        let doc = document(&format!(
            "<div style='display:flex;width:100px'><div id=max style='width:max-content;{flex};padding:5px;border:2px solid'><div style='width:80px;height:20px'></div></div><div style='width:60px;flex:none;height:20px'></div></div>"
        ));
        for scale in [1.0, 1.25, 2.0] {
            let f = frame(&doc, 320, scale);
            assert_eq!(
                rect(&doc, &f, "#max").width,
                expected,
                "{flex} scale={scale}"
            );
        }
    }
}

#[test]
fn flex_inline_flex_and_grid_containers_measure_child_rows_and_gaps() {
    for container in [
        "display:flex",
        "display:inline-flex",
        "display:grid;grid-template-columns:40px 60px",
    ] {
        let doc = document(&format!(
            "<div id=max style='{container};width:max-content;gap:10px;padding:5px;border:2px solid;box-sizing:border-box'><div style='width:40px;height:20px;flex:none'></div><div style='width:60px;height:20px;flex:none'></div></div>"
        ));
        for width in [60, 240] {
            for scale in [1.0, 1.25, 2.0] {
                let f = frame(&doc, width, scale);
                let r = rect(&doc, &f, "#max");
                assert_eq!(
                    (r.width, r.height),
                    (124, 34),
                    "{container} {width} {scale}"
                );
            }
        }
    }
}

#[test]
fn positioned_max_content_does_not_turn_into_opposite_inset_auto_fill() {
    for position in ["absolute", "fixed"] {
        let doc = document(&format!(
            "<div style='position:relative;width:200px;height:100px'><div id=max style='position:{position};left:20px;right:40px;top:0;width:max-content;padding:5px;border:2px solid'><div style='width:80px;height:20px'></div></div><div id=auto style='position:{position};left:20px;right:40px;top:50px;height:20px'></div></div>"
        ));
        for scale in [1.0, 1.25, 2.0] {
            let f = frame(&doc, 200, scale);
            let max = rect(&doc, &f, "#max");
            let auto = rect(&doc, &f, "#auto");
            assert_eq!((max.x, max.width), (20, 94), "{position} scale={scale}");
            assert_eq!((auto.x, auto.width), (20, 140), "{position} scale={scale}");
        }
    }
}

#[test]
fn replaced_max_content_preserves_natural_and_definite_height_ratios_with_edges() {
    for context in [
        "display:block",
        "display:flex;align-items:start",
        "display:grid;grid-template-columns:60px;align-items:start",
        "position:relative",
    ] {
        for (sizing, height, expected) in [
            ("content-box", "auto", (54, 34)),
            ("border-box", "auto", (54, 34)),
            ("content-box", "60px", (134, 74)),
            ("border-box", "74px", (134, 74)),
        ] {
            let position = if context == "position:relative" {
                "position:absolute;left:0;right:0"
            } else {
                ""
            };
            let doc = document(&format!(
                "<style>#max{{display:block;flex:none;width:max-content;height:{height};box-sizing:{sizing};padding:5px;border:2px solid;{position}}}</style><div style='{context};width:60px;height:180px'><svg id=max width=40 height=20 viewBox='0 0 40 20'><rect width=40 height=20 fill=red /></svg></div>"
            ));
            for scale in [1.0, 1.25, 2.0] {
                let f = frame(&doc, 320, scale);
                let r = rect(&doc, &f, "#max");
                assert_eq!(
                    (r.width, r.height),
                    expected,
                    "{context} {sizing} {height} {scale}"
                );
            }
        }
    }
}

#[test]
fn replaced_flex_shrink_transfers_resolved_content_width_to_auto_height() {
    for sizing in ["content-box", "border-box"] {
        for (available, expected) in [(60, (40, 27)), (40, (20, 17))] {
            let doc = document(&format!(
                "<style>#max{{display:block;flex-shrink:1;min-width:0;width:max-content;height:auto;box-sizing:{sizing};padding:5px;border:2px solid}}</style><div style='display:flex;align-items:start;width:{available}px'><svg id=max width=40 height=20 viewBox='0 0 40 20'><rect width=40 height=20 fill=red /></svg><div style='width:20px;height:20px;flex:none'></div></div>"
            ));
            for scale in [1.0, 1.25, 2.0] {
                let f = frame(&doc, 320, scale);
                let r = rect(&doc, &f, "#max");
                assert_eq!(
                    (r.width, r.height),
                    expected,
                    "{sizing} available={available} scale={scale}"
                );
            }
        }
    }
}

#[test]
fn structured_button_intrinsic_width_keeps_children_and_whole_control_action() {
    let doc = document(
        "<style>#icon{width:20px;height:10px}</style><form action=/done><button id=max name=action value=go style='display:inline-flex;width:max-content;gap:4px;align-items:center;padding:5px;border:2px solid;box-sizing:border-box'><svg id=icon viewBox='0 0 2 1'><rect width=2 height=1 fill=red /></svg><span id=label style='display:block;width:40px;height:10px;background:blue'></span></button></form>",
    );
    let control = doc.query_selector(0, "#max").unwrap().unwrap();
    for width in [40, 240] {
        for scale in [1.0, 1.25, 2.0] {
            let f = frame(&doc, width, scale);
            let r = rect(&doc, &f, "#max");
            assert_eq!((r.width, r.height), (78, 24), "{width} {scale}");
            let icon = rect(&doc, &f, "#icon");
            let label = rect(&doc, &f, "#label");
            assert_eq!((icon.width, label.width, label.x - icon.x), (20, 40, 24));
            // Layout may overflow the viewport; input remains clipped to the
            // visible control rather than accepting clicks outside the page.
            let visible_width = r.width.min(width.saturating_sub(r.x.max(0) as u32));
            assert!(
                f.hits.iter().any(|hit| {
                    matches!(hit.action, Action::Submit(node) if node == control)
                        && hit.x == r.x
                        && hit.y == r.y
                        && hit.w == visible_width
                        && hit.h == r.height
                }),
                "width={width} scale={scale} box={r:?} hits={:?}",
                f.hits
            );
        }
    }
}

#[test]
fn nested_max_content_contributions_respect_numeric_constraints_before_parent_measurement() {
    for (sizing, minimum, expected) in [
        ("content-box", "", 64),
        ("border-box", "", 50),
        ("content-box", "min-width:80px", 94),
        ("border-box", "min-width:80px", 80),
    ] {
        let doc = document(&format!(
            "<div id=outer style='width:max-content'><div id=inner style='width:max-content;max-width:50px;{minimum};box-sizing:{sizing};padding:5px;border:2px solid'>mmmm mmmm mmmm</div></div>"
        ));
        for width in [40, 240] {
            let f = frame(&doc, width, 1.0);
            assert_eq!(
                rect(&doc, &f, "#inner").width,
                expected,
                "{sizing} {minimum}"
            );
            assert_eq!(
                rect(&doc, &f, "#outer").width,
                expected,
                "{sizing} {minimum}"
            );
        }
    }
}
