//! Rounded backgrounds, borders and overflow masks share the document-to-viewport transform.
use mg_sparkle::{
    document::parse,
    paint::Fonts,
    render::{Controls, Frame, Viewport, render_scaled},
};

fn render(html: &str, scroll: i32, scale: f32) -> Frame {
    let mut fonts = Fonts::from_bytes(
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap(),
    )
    .unwrap();
    let document = parse(html, "https://fixture.test/");
    let frame = render_scaled(
        &document,
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
        frame.diagnostics.entries.is_empty(),
        "{:?}",
        frame.diagnostics
    );
    frame
}

#[test]
fn fixed_rounded_controls_keep_their_height_when_the_page_scrolls() {
    let html = "<style>html,body{margin:0;background:white}#header{position:fixed;top:8px;left:8px;width:240px;height:48px;border-radius:28px;background:#f0f4f9;border:1px solid #dddddd;box-sizing:border-box}#button{position:fixed;top:12px;left:256px;width:56px;height:40px;border-radius:100px;background:#0b57d0}#page{height:1200px}</style><div id=header></div><div id=button></div><div id=page></div>";
    for scale in [1.0, 1.35, 2.0] {
        let initial = render(html, 0, scale);
        assert!(initial.canvas.pixels.contains(&0xf0f4f9));
        assert!(initial.canvas.pixels.contains(&0x0b57d0));
        for scroll in [64, 338, 600] {
            let scrolled = render(html, scroll, scale);
            assert!(
                initial.canvas.pixels == scrolled.canvas.pixels,
                "fixed rounded controls changed at scroll {scroll}, scale {scale}"
            );
        }
    }
}

#[test]
fn scrolled_rounded_overflow_mask_matches_the_same_box_at_the_viewport_origin() {
    let css = "<style>html,body{margin:0;background:white}#box{width:160px;height:48px;border-radius:24px;border:4px solid #008800;box-sizing:border-box;overflow:hidden;background:red}#child{width:220px;height:160px;background:#0000ff}</style>";
    let box_html = "<div id=box><div id=child></div></div>";
    let origin = format!("{css}{box_html}");
    let displaced = format!("{css}<div style='height:600px'></div>{box_html}");
    for scale in [1.0, 1.35, 2.0] {
        let expected = render(&origin, 0, scale);
        assert!(expected.canvas.pixels.contains(&0x008800));
        assert!(expected.canvas.pixels.contains(&0x0000ff));
        let scrolled = render(&displaced, 600, scale);
        assert!(
            expected.canvas.pixels == scrolled.canvas.pixels,
            "rounded overflow mask changed after viewport translation at scale {scale}"
        );
    }
}
