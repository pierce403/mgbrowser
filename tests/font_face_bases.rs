//! Downloaded-face regressions: CSS source order must not turn normal text italic.
use mg_sparkle::{
    document::{
        ResourceData, font_face_base_rules_for_document, font_face_rules_for_document, parse,
    },
    paint::Fonts,
    render::{Controls, Viewport, render},
};

#[test]
fn normal_base_selection_respects_style_weight_unicode_and_family_cap() {
    let doc = parse("<p>News</p>", "https://fixture.example/");
    let mut css = String::new();
    for i in 0..20 {
        css.push_str(&format!(
            "@font-face{{font-family:Unused{i};src:url(/cyr{i}.ttf);unicode-range:U+0400-04FF}}"
        ));
    }
    css.push_str(
        "@font-face{font-family:News;src:url(/italic.ttf);font-style:italic}\
        @font-face{font-family:News;src:url(/bold.ttf);font-weight:bold}\
        @font-face{font-family:news;src:url(/medium.ttf);font-weight:500}\
        @font-face{font-family:NEWS;src:url(/regular.ttf);font-weight:400}\
        @font-face{font-family:OnlyItalic;src:url(/only-italic.ttf);font-style:oblique 10deg}\
        @font-face{font-family:Variable;src:url(/variable.ttf);font-weight:100 900}",
    );
    assert_eq!(font_face_rules_for_document(&css, &doc).len(), 6);
    assert_eq!(
        font_face_base_rules_for_document(&css, &doc),
        vec![
            ("NEWS".into(), "/regular.ttf".into()),
            ("Variable".into(), "/variable.ttf".into()),
        ]
    );
    for i in 0..20 {
        css.push_str(&format!(
            "@font-face{{font-family:Extra{i};src:url(/extra{i}.ttf);font-weight:500}}"
        ));
    }
    assert_eq!(font_face_base_rules_for_document(&css, &doc).len(), 16);
    // A better candidate after the cap can still replace an admitted family's base.
    css.push_str("@font-face{font-family:Extra0;src:url(/extra0-regular.ttf);font-weight:400}");
    assert_eq!(
        font_face_base_rules_for_document(&css, &doc)[2].1,
        "/extra0-regular.ttf"
    );
}

#[test]
fn italic_first_downloaded_family_renders_normal_text_with_regular_bytes() {
    let regular = std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").unwrap();
    let slanted =
        std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Oblique.ttf").unwrap();
    let draw = |include_italic: bool| {
        let first = if include_italic {
            "@font-face{font-family:News;src:url(/italic.ttf);font-style:italic}"
        } else {
            ""
        };
        let mut doc = parse(
            &format!(
                "<style>{first}\
            @font-face{{font-family:News;src:url(/normal.ttf);font-weight:400}}\
            body{{margin:0;background:white;font:20px News}}\
            </style><div>Publisher and reading metadata</div>"
            ),
            "https://fixture.example/",
        );
        for (name, bytes) in [("normal", regular.clone()), ("italic", slanted.clone())] {
            doc.resources.insert(
                format!("https://fixture.example/{name}.ttf"),
                ResourceData {
                    content_type: "font/ttf".into(),
                    bytes,
                },
            );
        }
        let mut fonts = Fonts::from_bytes(regular.clone()).unwrap();
        let frame = render(
            &doc,
            &mut fonts,
            Viewport {
                width: 400,
                height: 60,
                scroll: 0,
            },
            &Controls::default(),
        );
        assert!(
            frame.diagnostics.entries.is_empty(),
            "{:?}",
            frame.diagnostics
        );
        frame.canvas.pixels
    };
    assert_eq!(draw(true), draw(false));
}
