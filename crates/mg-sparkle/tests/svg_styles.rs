use mg_sparkle::{
    document::{Document, parse},
    style::{
        Color, ComputedStyle, Display, StyleDiagnostics, SvgPaint, compute_styles_with_diagnostics,
    },
};

fn compute(html: &str) -> (Document, Vec<ComputedStyle>, StyleDiagnostics) {
    let document = parse(html, "https://fixture.example/");
    let mut diagnostics = StyleDiagnostics::default();
    let styles = compute_styles_with_diagnostics(
        &document,
        &document.stylesheets,
        (800., 600.),
        &mut diagnostics,
    )
    .unwrap();
    (document, styles, diagnostics)
}

fn at<'a>(document: &Document, styles: &'a [ComputedStyle], selector: &str) -> &'a ComputedStyle {
    &styles[document.query_selector(0, selector).unwrap().unwrap()]
}

fn color(paint: SvgPaint, rgb: u32, alpha: f32) {
    let SvgPaint::Color(value) = paint else {
        panic!("expected color, got {paint:?}")
    };
    assert_eq!(value.rgb, rgb);
    assert!(
        (value.alpha - alpha).abs() < 0.000001,
        "{value:?}, expected alpha {alpha}"
    );
}

#[test]
fn presentation_paints_participate_below_author_rules_and_importants() {
    let (document, styles, diagnostics) = compute(
        "<style>#a{fill:#123456;stroke:currentColor}#b{fill:green!important}#c{fill:blue}</style><svg><path id=a fill=red stroke=blue style='color:#445566'/><path id=b fill=red style='fill:blue'/><path id=c fill=none style='fill:red!important'/><path id=none fill=none /><path id=default /></svg>",
    );
    color(at(&document, &styles, "#a").svg_fill, 0x123456, 1.);
    color(at(&document, &styles, "#a").svg_stroke, 0x445566, 1.);
    color(at(&document, &styles, "#b").svg_fill, 0x008000, 1.);
    color(at(&document, &styles, "#c").svg_fill, 0xff0000, 1.);
    assert_eq!(at(&document, &styles, "#none").svg_fill, SvgPaint::None);
    assert_eq!(
        at(&document, &styles, "#default").svg_fill,
        SvgPaint::Color(Color { rgb: 0, alpha: 1. })
    );
    assert_eq!(
        at(&document, &styles, "#default").svg_stroke,
        SvgPaint::None
    );
    assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
}

#[test]
fn inherited_current_color_resolves_at_each_element_and_paint_opacity_is_not_doubled() {
    let (document, styles, diagnostics) = compute(
        "<svg style='color:#112233;fill:currentColor;stroke:currentColor;fill-opacity:.5;stroke-opacity:.25'><g id=group><path id=child style='color:#445566'/><path id=none fill=none /><path id=rgba fill='rgba(138,180,248,.24)' stroke='rgba(1,2,3,.8)' fill-opacity='.5' stroke-opacity='.25' style='opacity:.1'/></g></svg>",
    );
    color(at(&document, &styles, "#group").svg_fill, 0x112233, 0.5);
    color(at(&document, &styles, "#child").svg_fill, 0x445566, 0.5);
    color(at(&document, &styles, "#child").svg_stroke, 0x445566, 0.25);
    assert_eq!(at(&document, &styles, "#none").svg_fill, SvgPaint::None);
    color(at(&document, &styles, "#rgba").svg_fill, 0x8ab4f8, 0.12);
    color(at(&document, &styles, "#rgba").svg_stroke, 0x010203, 0.2);
    assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
}

#[test]
fn svg_color_presentation_attribute_cascades_and_resolves_inherited_current_color() {
    let (document, styles, diagnostics) = compute(
        "<style>#override{color:#abcdef}#important{color:green!important}</style><div id=html color=red></div><svg color='#112233' fill=currentColor stroke=currentColor><path id=base /><g color='#445566'><path id=inherited /></g><path id=override color=red /><path id=important color=red style='color:blue'/><path id=invalid color='red;display:none'/></svg>",
    );
    assert_eq!(at(&document, &styles, "#html").color.rgb, 0);
    for (selector, rgb) in [
        ("#base", 0x112233),
        ("#inherited", 0x445566),
        ("#override", 0xabcdef),
        ("#important", 0x008000),
        ("#invalid", 0x112233),
    ] {
        let style = at(&document, &styles, selector);
        assert_eq!(style.color.rgb, rgb);
        color(style.svg_fill, rgb, 1.);
        color(style.svg_stroke, rgb, 1.);
        assert_ne!(style.display, Display::None);
    }
    assert_eq!(diagnostics.entries.len(), 1);
    assert_eq!(diagnostics.entries[0].kind, "css-parse");
    assert!(diagnostics.entries[0].message.contains("attribute color"));
}

#[test]
fn opacity_attributes_cascade_below_css_and_clamp_without_affecting_global_opacity() {
    let (document, styles, diagnostics) = compute(
        "<style>#a{fill-opacity:.75!important;stroke-opacity:.5}</style><svg><path id=a fill=red stroke=blue fill-opacity='.1' stroke-opacity='.2' style='fill-opacity:.25;opacity:.1'/><path id=b fill=red stroke=blue fill-opacity='2' stroke-opacity='-1'/></svg>",
    );
    color(at(&document, &styles, "#a").svg_fill, 0xff0000, 0.75);
    color(at(&document, &styles, "#a").svg_stroke, 0x0000ff, 0.5);
    color(at(&document, &styles, "#b").svg_fill, 0xff0000, 1.);
    color(at(&document, &styles, "#b").svg_stroke, 0x0000ff, 0.);
    assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
}

#[test]
fn svg_presentation_values_cannot_inject_other_declarations_or_important() {
    let (document, styles, diagnostics) = compute(
        "<svg><path id=a fill='red;display:none'/><path id=b stroke='red!important'/><path id=c fill='red}path{display:none}'/><path id=d fill-opacity='.2;fill:red'/><path id=e data-fill='red' d='fill:red'/></svg>",
    );
    for selector in ["#a", "#b", "#c", "#d", "#e"] {
        let style = at(&document, &styles, selector);
        color(style.svg_fill, 0, 1.);
        assert_eq!(style.svg_stroke, SvgPaint::None);
        assert_ne!(style.display, Display::None);
    }
    assert_eq!(diagnostics.entries.len(), 4);
    for diagnostic in &diagnostics.entries {
        assert_eq!(diagnostic.kind, "css-parse");
        assert!(diagnostic.node.is_some());
        assert!(diagnostic.source.contains("SVG presentation attribute"));
        assert!(diagnostic.message.contains("ignored"));
    }
}

#[test]
fn presentation_hints_stop_at_foreign_object_and_restart_at_nested_svg() {
    let (document, styles, diagnostics) = compute(
        "<div id=html fill=red stroke=green></div><svg><foreignObject><div id=foreign fill=red></div><svg><path id=nested fill=blue /></svg></foreignObject><path id=after fill=red /></svg>",
    );
    color(at(&document, &styles, "#html").svg_fill, 0, 1.);
    assert_eq!(at(&document, &styles, "#html").svg_stroke, SvgPaint::None);
    color(at(&document, &styles, "#foreign").svg_fill, 0, 1.);
    color(at(&document, &styles, "#nested").svg_fill, 0x0000ff, 1.);
    color(at(&document, &styles, "#after").svg_fill, 0xff0000, 1.);
    assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
}

#[test]
fn paint_servers_context_paints_and_context_opacity_remain_explicitly_unsupported() {
    let (document, styles, diagnostics) = compute(
        "<svg><path id=server fill='url(#gradient) red'/><path id=context style='fill:context-fill;stroke:context-stroke'/><path id=opacity style='fill-opacity:context-fill-opacity;stroke-opacity:context-stroke-opacity'/></svg>",
    );
    assert_eq!(
        at(&document, &styles, "#server").svg_fill,
        SvgPaint::Unsupported
    );
    for selector in ["#context", "#opacity"] {
        assert_eq!(
            at(&document, &styles, selector).svg_fill,
            SvgPaint::Unsupported
        );
        assert_eq!(
            at(&document, &styles, selector).svg_stroke,
            SvgPaint::Unsupported
        );
    }
    assert_eq!(diagnostics.entries.len(), 5, "{diagnostics:?}");
    for diagnostic in &diagnostics.entries {
        assert_eq!(diagnostic.kind, "css-unsupported");
        assert!(diagnostic.node.is_some());
        assert!(diagnostic.message.contains("bounded inline icon renderer"));
    }
}

#[test]
fn oversized_presentation_values_are_bounded_and_cannot_override_neighbors() {
    let (document, styles, diagnostics) = compute(&format!(
        "<svg><path id=x fill='{}' stroke=red /></svg>",
        "x".repeat(1025),
    ));
    color(at(&document, &styles, "#x").svg_fill, 0, 1.);
    color(at(&document, &styles, "#x").svg_stroke, 0xff0000, 1.);
    assert_eq!(diagnostics.entries.len(), 1);
    assert!(diagnostics.entries[0].message.len() <= StyleDiagnostics::MAX_MESSAGE_BYTES);
}
