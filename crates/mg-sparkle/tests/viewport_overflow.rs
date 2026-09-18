//! CSS viewport propagation, not a site-specific scrolling exception.
//! Primary contract: https://www.w3.org/TR/css-overflow-3/#overflow-propagation
//! These tests supply the viewport offset directly. They do not claim support
//! for independently scrolling arbitrary elements or root scrollbar controls.
use mg_sparkle::{
    document::{Document, parse},
    paint::Fonts,
    render::{Action, Controls, Frame, Viewport, render_scaled},
};

fn document(root: &str, body: &str, contents: &str) -> Document {
    parse(
        &format!(
            "<html style='{root}'><head><style>*{{margin:0;padding:0;border:0;font-size:12px;line-height:20px}}</style></head><body style='{body}'>{contents}</body></html>"
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

fn pixel(frame: &Frame, x: i32, y: i32) -> u32 {
    let x = frame.canvas.physical_edge(i64::from(x)) as usize;
    let y = frame.canvas.physical_edge(i64::from(y)) as usize;
    frame.canvas.pixels[y * frame.canvas.width as usize + x]
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

const LONG_CONTENT: &str = "<div style='height:300px'></div><a id=target href=/target style='display:block;width:80px;height:40px;background:#aa2200'></a><div style='height:100px'></div>";

#[test]
fn visible_html_propagates_body_overflow_without_clipping_first_viewport() {
    for overflow in ["overflow:auto", "overflow:scroll", "overflow-y:scroll"] {
        let doc = document(
            "height:100%;overflow:visible",
            &format!("height:100%;{overflow}"),
            LONG_CONTENT,
        );
        for scale in [1.0, 1.25, 2.0] {
            let top = frame(&doc, 0, scale);
            assert_eq!(top.content_height, 440, "{overflow}");
            let scrolled = frame(&doc, 300, scale);
            assert_eq!(scrolled.content_height, 440, "{overflow}");
            assert_eq!(pixel(&scrolled, 10, 10), 0xaa2200, "{overflow}, {scale}");
            assert_eq!(
                link_at(&scrolled, 10, 10),
                Some("https://fixture.example/target")
            );
            let id = doc.query_selector(0, "#target").unwrap().unwrap();
            let rect = scrolled.boxes.iter().rev().find(|r| r.node == id).unwrap();
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (0, 0, 80, 40));
        }
    }
}

#[test]
fn root_overflow_is_viewport_relative_even_with_short_root_box() {
    // Hidden/clip concern user scrolling separately. A supplied programmatic
    // viewport offset still paints the newly visible document coordinates.
    for overflow in ["auto", "scroll", "hidden", "clip"] {
        let doc = document(
            &format!("height:100%;overflow:{overflow}"),
            "height:100%;overflow:visible",
            LONG_CONTENT,
        );
        let scrolled = frame(&doc, 300, 1.0);
        assert_eq!(pixel(&scrolled, 10, 10), 0xaa2200, "{overflow}");
        assert_eq!(
            link_at(&scrolled, 10, 10),
            Some("https://fixture.example/target")
        );
        assert_eq!(scrolled.content_height, 440);
    }
}

#[test]
fn body_does_not_propagate_when_html_already_supplies_viewport_overflow() {
    let doc = document(
        "height:100%;overflow:auto",
        "height:100%;overflow:hidden",
        LONG_CONTENT,
    );
    let top = frame(&doc, 0, 1.0);
    assert_eq!(
        top.content_height, 200,
        "ordinary body clipping remains in force"
    );
    let scrolled = frame(&doc, 300, 1.0);
    assert_eq!(pixel(&scrolled, 10, 10), 0xffffff);
    assert!(scrolled.hits.is_empty());
}

#[test]
fn non_root_auto_and_scroll_remain_clipped_and_explicitly_unsupported() {
    for overflow in ["auto", "scroll"] {
        let doc = document(
            "",
            "",
            &format!(
                "<div style='width:100px;height:60px;overflow:{overflow}'><div style='height:120px'></div><a href=/inside style='display:block;height:20px;background:#aa2200'></a></div><a href=/outside style='display:block;width:100px;height:30px;background:#0022aa'></a>"
            ),
        );
        let frame = frame(&doc, 0, 1.0);
        assert_eq!(frame.content_height, 90, "{overflow}");
        assert_eq!(pixel(&frame, 10, 70), 0x0022aa);
        assert_eq!(
            link_at(&frame, 10, 70),
            Some("https://fixture.example/outside")
        );
        assert!(
            !frame.hits.iter().any(
                |hit| matches!(&hit.action,Action::Link{href,..} if href.ends_with("/inside"))
            )
        );
        assert!(
            frame
                .diagnostics
                .entries
                .iter()
                .any(|d| d.kind == "css-unsupported" && d.message.contains("scroll")),
            "element scrolling must remain a reported gap: {:?}",
            frame.diagnostics
        );
    }
}

#[test]
fn propagated_overflow_still_clips_to_exact_physical_frame() {
    let doc = document(
        "height:100%;overflow:visible",
        "height:100%;overflow-y:scroll",
        "<a href=/wide style='display:block;margin-left:-40px;width:400px;height:400px;background:#aa2200'></a>",
    );
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(&doc, 300, scale);
        assert_eq!(
            frame.canvas.pixels.len(),
            frame.canvas.width as usize * frame.canvas.height as usize
        );
        let edge = frame.canvas.physical_edge(100) as usize;
        for y in [0, edge - 1, edge, frame.canvas.height as usize - 1] {
            for x in [0, frame.canvas.width as usize - 1] {
                assert_eq!(
                    frame.canvas.pixels[y * frame.canvas.width as usize + x],
                    if y < edge { 0xaa2200 } else { 0xffffff },
                    "scale{scale} pixel({x},{y})"
                );
            }
        }
        assert_eq!(link_at(&frame, 0, 99), Some("https://fixture.example/wide"));
        assert_eq!(
            link_at(&frame, 319, 99),
            Some("https://fixture.example/wide")
        );
        assert_eq!(link_at(&frame, 10, 100), None);
        for hit in &frame.hits {
            assert!(
                hit.x >= 0
                    && hit.y >= 0
                    && hit.x as u32 + hit.w <= 320
                    && hit.y as u32 + hit.h <= 200
            );
        }
    }
}
