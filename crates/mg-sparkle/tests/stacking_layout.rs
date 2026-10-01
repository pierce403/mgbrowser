//! Numeric positioned-context fixtures, not full CSS stacking or live evidence.
use mg_sparkle::{
    document::parse,
    paint::Fonts,
    render::{Action, Controls, Frame, Viewport, render_scaled},
};

fn frame(body: &str, scale: f32) -> Frame {
    let doc = parse(
        &format!(
            "<html><head><style>*{{margin:0;padding:0;border:0;font-size:12px;line-height:20px}}</style></head><body>{body}</body></html>"
        ),
        "https://fixture.example/",
    );
    let mut fonts = Fonts::from_bytes(
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap(),
    )
    .unwrap();
    render_scaled(
        &doc,
        &mut fonts,
        Viewport {
            width: 320,
            height: 200,
            scroll: 0,
        },
        &Controls::default(),
        scale,
    )
}

fn styled(frame: &Frame) {
    assert!(
        !frame
            .diagnostics
            .entries
            .iter()
            .any(|d| matches!(d.kind, "css-parse" | "layout-fallback" | "style-fallback")),
        "{:?}",
        frame.diagnostics
    );
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
fn numeric_relative_ancestor_contains_high_z_absolute_descendant() {
    let source = "<div style='position:relative;z-index:1;width:80px;height:60px'><a href=/deep style='position:absolute;z-index:999;left:0;top:0;width:80px;height:60px;background:#aa2200'></a></div><a href=/later style='display:block;position:relative;z-index:2;left:40px;top:-60px;width:80px;height:60px;background:#0022aa'></a>";
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(source, scale);
        styled(&frame);
        assert_eq!(
            frame.content_height, 120,
            "paint ordering must not alter normal flow"
        );
        assert_eq!(pixel(&frame, 10, 10), 0xaa2200);
        assert_eq!(hit(&frame, 10, 10), Some("https://fixture.example/deep"));
        assert_eq!(pixel(&frame, 60, 10), 0x0022aa);
        assert_eq!(hit(&frame, 60, 10), Some("https://fixture.example/later"));
        assert_eq!(pixel(&frame, 100, 10), 0x0022aa);
    }
}

#[test]
fn negative_context_descendant_stays_above_own_background_below_normal_contents() {
    let negative = frame(
        "<div style='position:relative;z-index:1;width:80px;height:80px;background:#0022aa'><a href=/negative style='position:absolute;z-index:-5;left:0;top:0;width:60px;height:60px;background:#00aa22'></a><a href=/normal style='display:block;width:20px;height:20px;margin:10px;background:#aa2200'></a></div>",
        1.0,
    );
    styled(&negative);
    assert_eq!(pixel(&negative, 5, 5), 0x00aa22);
    assert_eq!(
        hit(&negative, 5, 5),
        Some("https://fixture.example/negative")
    );
    assert_eq!(pixel(&negative, 15, 15), 0xaa2200);
    assert_eq!(
        hit(&negative, 15, 15),
        Some("https://fixture.example/normal")
    );
    assert_eq!(pixel(&negative, 70, 70), 0x0022aa);
    let root = frame(
        "<style>body{background:#0022aa;height:100px}</style><a href=/negative style='position:absolute;z-index:-1;left:0;top:0;width:20px;height:20px;background:#aa2200'></a>",
        1.0,
    );
    styled(&root);
    assert_eq!(pixel(&root, 10, 10), 0xaa2200);
    assert_eq!(pixel(&root, 30, 10), 0x0022aa);
}

#[test]
fn same_z_contexts_follow_dom_order_without_descendant_escape() {
    let frame = frame(
        "<div style='position:relative;z-index:2;width:60px;height:40px'><a href=/earlier style='position:absolute;z-index:20;left:0;top:0;width:60px;height:40px;background:#aa2200'></a></div><a href=/later style='display:block;position:relative;z-index:2;top:-40px;width:60px;height:40px;background:#0022aa'></a>",
        1.0,
    );
    styled(&frame);
    assert_eq!(pixel(&frame, 10, 10), 0x0022aa);
    assert_eq!(hit(&frame, 10, 10), Some("https://fixture.example/later"));
    assert_eq!(frame.content_height, 80);
}

#[test]
fn child_stacking_context_hit_uses_emitter_not_inherited_anchor_owner() {
    let frame = frame(
        "<a href=/ancestor style='display:block;width:60px;height:40px'><span style='display:block;position:relative;z-index:2;width:60px;height:40px;background:#aa2200'></span></a><a href=/sibling style='display:block;position:relative;z-index:1;top:-40px;width:60px;height:40px;background:#0022aa'></a>",
        1.0,
    );
    styled(&frame);
    assert_eq!(pixel(&frame, 10, 10), 0xaa2200);
    assert_eq!(
        hit(&frame, 10, 10),
        Some("https://fixture.example/ancestor")
    );
}

#[test]
fn reordered_paints_keep_their_clips_and_hit_order() {
    let source = "<div style='position:relative;z-index:1;width:40px;height:40px;overflow:hidden'><a href=/clipped style='position:absolute;z-index:30;left:20px;top:0;width:80px;height:60px;background:#aa2200'></a></div><a href=/later style='display:block;position:relative;z-index:2;left:60px;top:-40px;width:40px;height:40px;background:#0022aa'></a>";
    for scale in [1.0, 1.25, 2.0] {
        let frame = frame(source, scale);
        styled(&frame);
        assert_eq!(pixel(&frame, 30, 10), 0xaa2200);
        assert_eq!(hit(&frame, 30, 10), Some("https://fixture.example/clipped"));
        assert_eq!(pixel(&frame, 50, 10), 0xffffff);
        assert_eq!(hit(&frame, 50, 10), None);
        assert_eq!(pixel(&frame, 70, 10), 0x0022aa);
        assert_eq!(hit(&frame, 70, 10), Some("https://fixture.example/later"));
        assert_eq!(pixel(&frame, 30, 50), 0xffffff);
        assert_eq!(frame.content_height, 80);
    }
}

#[test]
fn context_admission_is_bounded_with_a_concrete_diagnostic() {
    for (count, expected_fallback) in [(512, false), (513, true)] {
        let frame = frame(
            &"<div style='position:relative;z-index:1;width:1px;height:0'></div>".repeat(count),
            1.0,
        );
        let failure = frame
            .diagnostics
            .entries
            .iter()
            .find(|d| d.kind == "layout-fallback");
        assert_eq!(failure.is_some(), expected_fallback, "count{count}");
        if let Some(failure) = failure {
            assert!(
                failure.message.contains("stacking context limit (512)"),
                "{failure:?}"
            );
            assert!(failure.node.is_some());
        }
    }
}
