//! Public-render checks for used gradient boxes, not just style snapshots.
use mg_sparkle::{
    document::parse,
    paint::Fonts,
    render::{Action, Controls, Frame, Viewport, render_scaled},
};

fn frame(body: &str, scale: f32, scroll: i32) -> Frame {
    let document = parse(
        &format!(
            "<html><head><style>html,body{{margin:0;background:white}}*{{margin:0;padding:0;border:0}}</style></head><body>{body}</body></html>"
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
    let frame = render_scaled(
        &document,
        &mut fonts,
        Viewport {
            width: 40,
            height: 24,
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

fn pixel(frame: &Frame, x: i32, y: i32) -> u32 {
    let px = frame.canvas.physical_edge(i64::from(x)) as usize;
    let py = frame.canvas.physical_edge(i64::from(y)) as usize;
    frame.canvas.pixels[py * frame.canvas.width as usize + px]
}

#[test]
fn auto_height_gradient_uses_final_box_instead_of_empty_decoration_placeholder() {
    let frame = frame(
        "<div style='width:8px;background:black;background-image:linear-gradient(90deg,rgba(255,255,255,0),white 4px)'><div style='height:2px'></div></div>",
        1.,
        0,
    );
    assert_eq!(
        &frame.canvas.pixels[..8],
        &[
            0x202020, 0x606060, 0x9f9f9f, 0xdfdfdf, 0xffffff, 0xffffff, 0xffffff, 0xffffff
        ]
    );
    assert_eq!(pixel(&frame, 0, 1), 0x202020);
    assert_eq!(pixel(&frame, 0, 2), 0xffffff);
    assert_eq!(pixel(&frame, 8, 0), 0xffffff);
}

#[test]
fn content_clip_preserves_padding_and_border_and_does_not_reset_gradient_origin() {
    let frame = frame(
        "<div style='width:8px;height:4px;padding:2px;border:1px solid blue;background:black;background-image:linear-gradient(90deg,rgba(255,255,255,0),white 4px);background-clip:content-box'></div>",
        1.,
        0,
    );
    assert_eq!(pixel(&frame, 0, 3), 0x0000ff);
    assert_eq!(pixel(&frame, 1, 3), 0xffffff);
    assert_eq!(pixel(&frame, 2, 3), 0xffffff);
    assert_eq!(pixel(&frame, 3, 3), 0x9f9f9f);
    assert_eq!(pixel(&frame, 4, 3), 0xdfdfdf);
    assert_eq!(pixel(&frame, 5, 3), 0xffffff);
    assert_eq!(pixel(&frame, 12, 3), 0xffffff);
    assert_eq!(pixel(&frame, 13, 3), 0x0000ff);
    assert_eq!(pixel(&frame, 3, 2), 0xffffff);
    assert_eq!(pixel(&frame, 3, 7), 0xffffff);
    assert_eq!(pixel(&frame, 3, 9), 0x0000ff);
}

#[test]
fn positioned_gradient_tracks_flex_shift_scroll_scaling_and_link_hit() {
    let body = "<div style='display:flex;align-items:center;justify-content:center;width:30px;height:12px'><a href=/done style='display:block;width:8px;height:4px;background:black;background-image:linear-gradient(90deg,transparent,white 4px)'></a></div>";
    for scale in [1., 1.25, 2.] {
        for scroll in [0, 4] {
            let frame = frame(body, scale, scroll);
            assert_eq!(pixel(&frame, 10, 4 - scroll), 0xffffff);
            let first = pixel(&frame, 11, 4 - scroll);
            assert!(
                first != 0xffffff && first != 0,
                "gradient begins at moved box: {first:x}, scale {scale}"
            );
            assert_eq!(pixel(&frame, 16, 4 - scroll), 0xffffff);
            let hit = frame.hits.iter().find(|hit| matches!(&hit.action,Action::Link {href,..} if href == "https://fixture.example/done")).unwrap();
            assert_eq!((hit.x, hit.y, hit.w, hit.h), (11, 4 - scroll, 8, 4));
        }
    }
}
