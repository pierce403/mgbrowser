use mg_sparkle::{
    document::parse,
    style::{
        BackgroundBox, GradientDirection, Length, StyleDiagnostics, compute_styles_with_diagnostics,
    },
};

fn compute(css: &str) -> (mg_sparkle::style::ComputedStyle, StyleDiagnostics) {
    let document = parse(
        &format!("<style>#target{{{css}}}</style><div id=target></div>"),
        "https://fixture.example/",
    );
    let mut diagnostics = StyleDiagnostics::default();
    let styles = compute_styles_with_diagnostics(
        &document,
        &document.stylesheets,
        (800., 600.),
        &mut diagnostics,
    )
    .unwrap();
    let node = document.query_selector(0, "#target").unwrap().unwrap();
    (styles[node].clone(), diagnostics)
}

#[test]
fn actual_two_stop_transparent_white_background_keeps_four_pixel_stop_and_clip() {
    let (style, diagnostics) = compute(
        "background-image:linear-gradient(90deg,rgba(255,255,255,0),white 4px);background-clip:content-box",
    );
    let gradient = style.background_gradient.unwrap();
    assert_eq!(
        gradient.direction,
        GradientDirection::Angle(std::f32::consts::FRAC_PI_2)
    );
    assert_eq!(gradient.stops[0].position, Length::Percent(0.));
    assert_eq!(gradient.stops[0].color.rgb, 0xffffff);
    assert_eq!(gradient.stops[0].color.alpha, 0.);
    assert_eq!(gradient.stops[1].position, Length::Px(4.));
    assert_eq!(gradient.stops[1].color.rgb, 0xffffff);
    assert_eq!(gradient.stops[1].color.alpha, 1.);
    assert_eq!(gradient.origin, BackgroundBox::Padding);
    assert_eq!(gradient.clip, BackgroundBox::Content);
    assert_eq!(style.background_clip, BackgroundBox::Content);
    assert_eq!(gradient.repeat, [true; 2]);
    assert!(style.background_images.is_empty());
    assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
}

#[test]
fn colors_use_cascade_current_color_and_supported_direction_forms() {
    let (style, diagnostics) = compute(
        "color:#123456;background:linear-gradient(to bottom right,currentColor 20%,rgba(30,40,50,.5)) content-box padding-box no-repeat",
    );
    let gradient = style.background_gradient.unwrap();
    assert_eq!(
        gradient.direction,
        GradientDirection::Corner {
            right: true,
            bottom: true
        }
    );
    assert_eq!(gradient.stops[0].color.rgb, 0x123456);
    assert_eq!(gradient.stops[0].position, Length::Percent(0.2));
    assert_eq!(gradient.stops[1].color.rgb, 0x1e2832);
    assert_eq!(gradient.stops[1].color.alpha, 0.5);
    assert_eq!(gradient.stops[1].position, Length::Percent(1.));
    assert_eq!(gradient.origin, BackgroundBox::Content);
    assert_eq!(gradient.clip, BackgroundBox::Padding);
    assert_eq!(gradient.repeat, [false; 2]);
    assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
    for (direction, radians) in [
        ("to top", 0.),
        ("to right", std::f32::consts::FRAC_PI_2),
        ("to bottom", std::f32::consts::PI),
        ("to left", std::f32::consts::PI * 1.5),
    ] {
        let (style, diagnostics) = compute(&format!(
            "background-image:linear-gradient({direction},red,blue)"
        ));
        assert_eq!(
            style.background_gradient.unwrap().direction,
            GradientDirection::Angle(radians)
        );
        assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
    }
}

#[test]
fn unsupported_forms_are_omitted_with_explicit_node_diagnostics_not_coerced() {
    for css in [
        "background-image:radial-gradient(red,blue)",
        "background-image:conic-gradient(red,blue)",
        "background-image:repeating-linear-gradient(red,blue)",
        "background-image:linear-gradient(red,green,blue)",
        "background-image:linear-gradient(red,20%,blue)",
        "background-image:linear-gradient(red calc(2px + 10%),blue)",
        "background-image:linear-gradient(in oklab,red,blue)",
        "background-image:linear-gradient(red,blue),linear-gradient(white,black)",
        "background-image:linear-gradient(red,blue);background-size:20px 30px",
        "background-image:linear-gradient(red,blue);background-position:2px 0",
        "background-image:linear-gradient(red,blue);background-attachment:fixed",
        "background-image:linear-gradient(red,blue);background-repeat:round",
        "background-image:linear-gradient(red 1000001px,blue)",
    ] {
        let (style, diagnostics) = compute(css);
        assert_eq!(style.background_gradient, None, "{css}");
        assert!(
            diagnostics
                .entries
                .iter()
                .any(|entry| entry.kind == "css-unsupported"
                    && entry.node.is_some()
                    && entry.message.contains("CSS gradient omitted")),
            "{css}: {diagnostics:?}"
        );
        assert!(
            diagnostics
                .entries
                .iter()
                .all(|entry| entry.message.len() <= StyleDiagnostics::MAX_MESSAGE_BYTES)
        );
    }
}

#[test]
fn default_backgrounds_remain_unchanged_and_color_uses_last_layer_clip() {
    let (style, diagnostics) = compute("background-color:#123456");
    assert_eq!(style.background_gradient, None);
    assert_eq!(style.background_clip, BackgroundBox::Border);
    assert_eq!(style.background_color.rgb, 0x123456);
    assert!(diagnostics.entries.is_empty());
    let (style, diagnostics) =
        compute("background-image:none,none,none;background-clip:border-box,content-box");
    assert_eq!(style.background_clip, BackgroundBox::Border);
    assert_eq!(style.background_gradient, None);
    assert!(diagnostics.entries.is_empty());
    let (style, diagnostics) =
        compute("background-image:none,none;background-clip:border-box,content-box");
    assert_eq!(style.background_clip, BackgroundBox::Content);
    assert!(diagnostics.entries.is_empty());
}
