//! Inline nowrap text must retain one line inside an ordinary wrapping block.
use mg_sparkle::{
    document::{Document, parse},
    paint::Fonts,
    render::{Controls, Frame, LayoutBox, Viewport, render},
};

fn draw(body: &str) -> (Document, Frame) {
    let doc = parse(
        &format!(
            "<style>html,body{{margin:0;background:white}}\
        body{{font:14px/27px sans-serif}}a{{color:black;text-decoration:none}}\
        .flex{{display:flex;width:200px}}.small{{width:30px}}\
        .nowrap{{white-space:nowrap}}</style>{body}"
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
    let frame = render(
        &doc,
        &mut fonts,
        Viewport {
            width: 240,
            height: 180,
            scroll: 0,
        },
        &Controls::default(),
    );
    assert!(
        frame.diagnostics.entries.is_empty(),
        "{:?}",
        frame.diagnostics
    );
    (doc, frame)
}
fn box_of<'a>(doc: &Document, frame: &'a Frame, id: &str) -> &'a LayoutBox {
    let node = doc.query_selector(0, &format!("#{id}")).unwrap().unwrap();
    frame.boxes.iter().rev().find(|b| b.node == node).unwrap()
}

#[test]
fn inline_nowrap_keeps_intrinsic_flex_label_and_constrained_overflow_on_one_line() {
    let (doc, frame) = draw(
        "<div class=flex><div id=intrinsic><a class=nowrap>For you</a></div></div>\
        <div id=overflow class=small><span class=nowrap>For you</span></div>\
        <div id=normal class=small>For you</div>",
    );
    assert_eq!(box_of(&doc, &frame, "intrinsic").height, 27);
    let overflow = box_of(&doc, &frame, "overflow");
    assert_eq!((overflow.width, overflow.height), (30, 27));
    assert_eq!(box_of(&doc, &frame, "normal").height, 54);
    let text = doc
        .nodes
        .iter()
        .position(|n| {
            n.tag == "#text"
                && n.text == "For you"
                && doc.nodes[n.parent].attr("class") == Some("nowrap")
                && doc.nodes[doc.nodes[n.parent].parent].attr("id") == Some("overflow")
        })
        .unwrap();
    assert!(
        frame
            .boxes
            .iter()
            .any(|b| b.node == text && b.x + b.width as i32 > 30),
        "nowrap text must overflow the constrained box"
    );
}

#[test]
fn inline_nowrap_collapses_internal_and_line_boundary_spaces() {
    let (_, spaces_only) = draw("<div class=small><span class=nowrap> \t\r\n </span></div>");
    let (_, empty) = draw("<div class=small><span class=nowrap></span></div>");
    assert_eq!(spaces_only.canvas.pixels, empty.canvas.pixels);
    assert_eq!(spaces_only.content_height, empty.content_height);
    let (_, spaced) = draw("<div class=small><span class=nowrap> \t For  \n you \t </span></div>");
    let (_, collapsed) = draw("<div class=small><span class=nowrap>For you</span></div>");
    assert_eq!(spaced.canvas.pixels, collapsed.canvas.pixels);
    let (_, spaced) = draw("<div>prefix <span class=nowrap> \t For  you \n </span> suffix</div>");
    let (_, collapsed) = draw("<div>prefix <span class=nowrap>For you</span> suffix</div>");
    assert_eq!(spaced.canvas.pixels, collapsed.canvas.pixels);
}
