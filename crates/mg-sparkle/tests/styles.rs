use mg_sparkle::document::{Document, StylesheetSource, parse, parse_with_scripting};
use mg_sparkle::style::{
    ComputedStyle, Display, Length, TextAlign, VerticalAlign, WhiteSpace, compute_styles,
};

fn at<'a>(document: &Document, styles: &'a [ComputedStyle], selector: &str) -> &'a ComputedStyle {
    &styles[document.query_selector(0, selector).unwrap().unwrap()]
}

fn computed(html: &str, width: f32) -> (Document, Vec<ComputedStyle>) {
    let document = parse(html, "https://fixture.example/path/page.html");
    let styles = compute_styles(&document, &document.stylesheets, (width, 768.0)).unwrap();
    (document, styles)
}

#[test]
fn selectors_cascade_inheritance_and_inline_importance() {
    let (document, styles) = computed(
        r#"<html><head><style>
      body { font: 10pt Verdana, sans-serif; color:#123456 }
      .meta {font-size:80%} .meta a {color:red}
      a.story {color:green} #story {color:blue!important}
      .meta > a + span {color:#334455}
    </style></head><body><div class=meta><a id=story class=story href=/x style="color:orange">Story</a><span id=after>meta</span></div></body></html>"#,
        1024.0,
    );
    let story = at(&document, &styles, "#story");
    assert_eq!(story.color.rgb, 0x0000ff);
    assert!((story.font_size - 10.666667).abs() < 0.02);
    assert_eq!(story.font_families, ["Verdana", "sans-serif"]);
    assert!(story.underline);
    assert_eq!(at(&document, &styles, "#after").color.rgb, 0x334455);
}

#[test]
fn desktop_media_and_stylesheet_source_order() {
    let mut document = parse(
        "<html><body><div id=target>text</div></body></html>",
        "https://fixture.example/",
    );
    document.stylesheets = vec![
        StylesheetSource {
            css: "#target {color:red} @media(max-width:800px){#target{color:blue}}".into(),
            base_url: document.base_url.clone(),
            media: "screen".into(),
        },
        StylesheetSource {
            css: "#target {color:green}".into(),
            base_url: document.base_url.clone(),
            media: "print".into(),
        },
    ];
    let desktop = compute_styles(&document, &document.stylesheets, (1024.0, 768.0)).unwrap();
    let narrow = compute_styles(&document, &document.stylesheets, (640.0, 768.0)).unwrap();
    assert_eq!(at(&document, &desktop, "#target").color.rgb, 0xff0000);
    assert_eq!(at(&document, &narrow, "#target").color.rgb, 0x0000ff);
    document.stylesheets.push(StylesheetSource {
        css: "#target{color:#112233}".into(),
        base_url: document.base_url.clone(),
        media: String::new(),
    });
    let later = compute_styles(&document, &document.stylesheets, (1024.0, 768.0)).unwrap();
    assert_eq!(at(&document, &later, "#target").color.rgb, 0x112233);
}

#[test]
fn media_attribute_is_parsed_as_media_not_interpolated_css() {
    let mut document = parse(
        "<html><body><div id=target>text</div></body></html>",
        "https://fixture.example/",
    );
    document.stylesheets.push(StylesheetSource {
        css: "#target{color:red}".into(),
        base_url: document.base_url.clone(),
        media: "print {} #target { color: blue } @media screen".into(),
    });
    let styles = compute_styles(&document, &document.stylesheets, (1024.0, 768.0)).unwrap();
    assert_eq!(at(&document, &styles, "#target").color.rgb, 0x000000);
}

#[test]
fn table_presentation_hints_are_below_author_rules() {
    let (document, styles) = computed(
        r#"<html><head><style>
        table {min-width:796px} #cell {background:#abcdef;padding:3px}
    </style></head><body><center><table id=panel width=85% bgcolor=#f6f6ef cellpadding=5 cellspacing=0><tr><td id=cell bgcolor=#ff6600 align=right valign=top height=20>Text</td><td id=other>other</td></tr></table></center></body></html>"#,
        1024.0,
    );
    let panel = at(&document, &styles, "#panel");
    assert_eq!(panel.display, Display::Table);
    assert_eq!(panel.width, Length::Percent(0.85));
    assert_eq!(panel.min_width, Length::Px(796.0));
    assert_eq!(panel.background_color.rgb, 0xf6f6ef);
    assert_eq!(panel.border_spacing, [0.0; 2]);
    assert_eq!(panel.text_align, TextAlign::Start);
    let cell = at(&document, &styles, "#cell");
    assert_eq!(cell.display, Display::TableCell);
    assert_eq!(cell.background_color.rgb, 0xabcdef);
    assert_eq!(cell.padding, [Length::Px(3.0); 4]);
    assert_eq!(cell.height, Length::Px(20.0));
    assert_eq!(cell.text_align, TextAlign::Right);
    assert_eq!(cell.vertical_align, VerticalAlign::Top);
    assert_eq!(
        at(&document, &styles, "#other").padding,
        [Length::Px(5.0); 4]
    );
}

#[test]
fn background_layers_keep_url_and_source_base() {
    let mut document = parse(
        "<html><body><a id=arrow href=/story></a></body></html>",
        "https://fixture.example/page",
    );
    document.stylesheets.push(StylesheetSource {
        css: "#arrow {background-image:linear-gradient(transparent,transparent),url(../icons/triangle.svg);background-size:1px 1px,10px 8px;width:10px;height:10px;display:inline-block}".into(),
        base_url:"https://assets.example/css/main.css".into(), media:String::new(),
    });
    let styles = compute_styles(&document, &document.stylesheets, (1280.0, 800.0)).unwrap();
    let arrow = at(&document, &styles, "#arrow");
    assert_eq!(arrow.display, Display::InlineBlock);
    assert_eq!(arrow.background_images.len(), 1);
    assert_eq!(
        arrow.background_images[0].url,
        "https://assets.example/icons/triangle.svg"
    );
    assert_eq!(arrow.background_images[0].width, Length::Px(10.0));
    assert_eq!(arrow.background_images[0].height, Length::Px(8.0));
}

#[test]
fn unsupported_declaration_preserves_neighbors_and_hidden_content() {
    let (document, styles) = computed(
        r#"<html><head><style>
        #x {color:#102030;unknown-property:invalid;line-height:1.5;white-space:nowrap}
    </style></head><body><p id=x hidden>Hidden</p><a id=link href=/x>Link</a></body></html>"#,
        1024.0,
    );
    let x = at(&document, &styles, "#x");
    assert_eq!(x.display, Display::None);
    assert_eq!(x.color.rgb, 0x102030);
    assert_eq!(x.line_height, Some(24.0));
    assert_eq!(x.white_space, WhiteSpace::NoWrap);
    assert_eq!(at(&document, &styles, "#link").color.rgb, 0x0000ee);
}

#[test]
fn invalid_topology_and_stylesheet_overflow_fail_at_boundary() {
    let mut document = parse("<p>Text</p>", "https://fixture.example/");
    document.nodes[0].children.push(0);
    assert!(compute_styles(&document, &[], (1024.0, 768.0)).is_err());
    let document = parse("<p>Text</p>", "https://fixture.example/");
    let oversized = StylesheetSource {
        css: " ".repeat(2 * 1024 * 1024 + 1),
        base_url: document.base_url.clone(),
        media: String::new(),
    };
    assert!(compute_styles(&document, &[oversized], (1024.0, 768.0)).is_err());
    let oversized_media = StylesheetSource {
        css: String::new(),
        base_url: document.base_url.clone(),
        media: " ".repeat(2 * 1024 * 1024 + 1),
    };
    assert!(compute_styles(&document, &[oversized_media], (1024.0, 768.0)).is_err());
    assert!(compute_styles(&document, &[], (f32::NAN, 768.0)).is_err());
    assert!(compute_styles(&document, &[], (1024.0, -1.0)).is_err());
}

#[test]
fn computed_lists_and_owned_snapshot_amplification_are_bounded() {
    let document = parse("<p>Text</p>", "https://fixture.example/");
    let rejected = |css: String| {
        let source = StylesheetSource {
            css,
            base_url: document.base_url.clone(),
            media: String::new(),
        };
        assert!(compute_styles(&document, &[source], (1024.0, 768.0)).is_err());
    };
    rejected(format!(
        "*{{background-image:{}}}",
        vec!["url(x)"; 9].join(",")
    ));
    rejected(format!(
        "*{{background-image:url(https://example.org/{})}}",
        "a".repeat(4096)
    ));
    rejected(format!("*{{font-family:'{}'}}", "a".repeat(257)));
    rejected(format!("*{{font-family:{}}}", vec!["serif"; 17].join(",")));

    // Every element and text node is separately represented in the renderer
    // snapshot. Small CSS plus many nodes must not produce unbounded strings.
    let mut document = parse(
        &"<span>Text</span>".repeat(1500),
        "https://fixture.example/",
    );
    document.stylesheets.push(StylesheetSource {
        css: format!(
            "*{{font-family:{}}}",
            vec![format!("'{}'", "a".repeat(200)); 16].join(",")
        ),
        base_url: document.base_url.clone(),
        media: String::new(),
    });
    let error = compute_styles(&document, &document.stylesheets, (1024.0, 768.0)).unwrap_err();
    assert_eq!(error, "computed style snapshot bound exceeded");
}

#[test]
fn inactive_noscript_cannot_be_reenabled_by_author_css() {
    let source = "<html><head><style>noscript{display:block!important}</style></head><body><noscript id=fallback>Fallback</noscript></body></html>";
    for scripting in [false, true] {
        let document = parse_with_scripting(source, "https://fixture.example/", scripting);
        let styles = compute_styles(&document, &document.stylesheets, (1024.0, 768.0)).unwrap();
        assert_eq!(
            at(&document, &styles, "#fallback").display,
            if scripting {
                Display::None
            } else {
                Display::Block
            }
        );
    }
}
