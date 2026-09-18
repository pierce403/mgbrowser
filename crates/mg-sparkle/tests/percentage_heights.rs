//! Height percentages must not accidentally use the containing block's width.
use mg_sparkle::{
    document::{Document, ResourceData, parse},
    paint::Fonts,
    render::{Action, Controls, Frame, LayoutBox, Viewport, render},
};

fn document(body: &str) -> Document {
    parse(
        &format!(
            "<html><head><style>html,body{{margin:0}} *{{font-size:12px;line-height:20px}} input,td{{padding:0;border:0}}</style></head><body>{body}</body></html>"
        ),
        "https://fixture.example/",
    )
}

fn frame(document: &Document, width: u32, height: u32, scroll: i32) -> Frame {
    let mut fonts = Fonts::from_bytes(
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap(),
    )
    .unwrap();
    let frame = render(
        document,
        &mut fonts,
        Viewport {
            width,
            height,
            scroll,
        },
        &Controls::default(),
    );
    assert!(
        !frame
            .diagnostics
            .entries
            .iter()
            .any(|d| d.kind == "layout-fallback")
    );
    frame
}

fn rect<'a>(document: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let node = document.query_selector(0, selector).unwrap().unwrap();
    frame.boxes.iter().rev().find(|b| b.node == node).unwrap()
}

#[test]
fn definite_height_chain_is_independent_of_available_width() {
    let doc = document(
        "<div style='height:200px'><div id=half style='height:50%'><div id=quarter style='height:25%'></div></div></div>",
    );
    for width in [240, 800] {
        let frame = frame(&doc, width, 400, 0);
        assert_eq!(rect(&doc, &frame, "#half").height, 100);
        assert_eq!(rect(&doc, &frame, "#quarter").height, 25);
    }
}

#[test]
fn auto_height_breaks_percentage_chain_even_when_minimum_is_definite() {
    let doc = document(
        "<div style='height:200px'><div style='min-height:160px;max-height:180px'><div id=auto style='height:90%;min-height:80%;max-height:10%'><div style='height:30px'></div></div></div></div>",
    );
    for width in [240, 800] {
        let frame = frame(&doc, width, 400, 0);
        assert_eq!(rect(&doc, &frame, "#auto").height, 30);
    }
}

#[test]
fn root_percentages_use_viewport_height_and_zero_is_definite() {
    let doc = document(
        "<style>html,body{height:100%}</style><div id=half style='height:50%'></div><div style='height:0'><div id=zero style='height:100%'></div></div>",
    );
    for (width, height) in [(240, 400), (800, 300)] {
        let frame = frame(&doc, width, height, 0);
        assert_eq!(rect(&doc, &frame, "#half").height, height / 2);
        assert_eq!(rect(&doc, &frame, "#zero").height, 0);
    }
}

#[test]
fn minimum_and_maximum_percentages_use_height_and_minimum_wins() {
    let doc = document(
        "<div style='height:200px'><div id=max style='height:90%;min-height:20%;max-height:25%'></div><div id=min style='height:10%;min-height:37.5%;max-height:20%'></div><div style='height:300px;max-height:50%'><div id=clamped-parent style='height:50%'></div></div></div>",
    );
    for width in [240, 800] {
        let frame = frame(&doc, width, 400, 0);
        assert_eq!(rect(&doc, &frame, "#max").height, 50);
        assert_eq!(rect(&doc, &frame, "#min").height, 75);
        assert_eq!(rect(&doc, &frame, "#clamped-parent").height, 50);
    }
}

#[test]
fn specified_block_height_does_not_expand_to_contain_overflow() {
    let doc = document(
        "<div id=parent style='height:20px'><div style='height:60px'></div></div><div id=next style='height:10px'></div>",
    );
    let frame = frame(&doc, 240, 400, 0);
    assert_eq!(rect(&doc, &frame, "#parent").height, 20);
    assert_eq!(rect(&doc, &frame, "#next").y, 20);
    assert_eq!(frame.content_height, 60);
}

#[test]
fn controls_resolve_percentage_height_only_in_definite_containing_block() {
    let doc = document(
        "<div style='height:200px'><input id=fixed style='width:80px;height:50%;min-height:40%;max-height:45%'></div><div><input id=auto style='width:80px;height:50%;min-height:40%;max-height:45%'></div>",
    );
    for width in [240, 800] {
        let frame = frame(&doc, width, 400, 0);
        assert_eq!(rect(&doc, &frame, "#fixed").height, 90);
        assert_eq!(rect(&doc, &frame, "#auto").height, 20);
    }
}

#[test]
fn images_preserve_auto_width_ratio_wrapping_and_scrolled_hits() {
    let mut doc = document(
        "<div style='height:200px;text-align:center'><a href='/story'><img id=image src='/picture.svg' style='height:50%;max-height:40%'></a></div><div><img id=auto src='/picture.svg' style='height:50%;min-height:40px'></div>",
    );
    doc.resources.insert("https://fixture.example/picture.svg".into(), ResourceData {
        bytes: b"<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20' fill='red'/></svg>".to_vec(),
        content_type: "image/svg+xml".into(),
    });
    for width in [240, 800] {
        let first = frame(&doc, width, 400, 0);
        let scrolled = frame(&doc, width, 400, 10);
        let image = rect(&doc, &first, "#image");
        assert_eq!((image.width, image.height), (160, 80));
        assert_eq!(image.x, (width as i32 - 160) / 2);
        let auto = rect(&doc, &first, "#auto");
        assert_eq!((auto.width, auto.height), (80, 40));
        assert_eq!(rect(&doc, &scrolled, "#image").y, image.y - 10);
        assert_eq!(rect(&doc, &scrolled, "#image").height, image.height);
        assert!(scrolled.hits.iter().any(|hit| {
            matches!(&hit.action, Action::Link {href, ..} if href == "https://fixture.example/story")
                && hit.x == image.x && hit.y == 0 && hit.w == 160 && hit.h == 70
        }));
    }
}

#[test]
fn percentage_padding_still_uses_width_not_height() {
    let doc = document(
        "<div style='height:200px;width:200px'><div id=padded style='height:50%;padding:10%'></div></div>",
    );
    let frame = frame(&doc, 800, 400, 0);
    assert_eq!(rect(&doc, &frame, "#padded").height, 140);
}

#[test]
fn row_percentage_never_uses_table_width() {
    let doc = document(
        "<table cellspacing=0><tr id=auto style='height:50%'><td style='height:10px'></td></tr></table><table style='height:200px' cellspacing=0><tr id=fixed style='height:50%'><td id=cell style='height:50%'></td></tr></table>",
    );
    for width in [240, 800] {
        let frame = frame(&doc, width, 400, 0);
        assert_eq!(rect(&doc, &frame, "#auto").height, 10);
        assert_eq!(rect(&doc, &frame, "#fixed").height, 100);
        assert_eq!(rect(&doc, &frame, "#cell").height, 50);
    }
}
