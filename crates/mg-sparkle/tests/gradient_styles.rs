use mg_sparkle::{
    document::parse,
    style::{
        BackgroundBox, GradientDirection, GradientKind, Length, RadialExtent, RadialShape,
        StyleDiagnostics, compute_styles_with_diagnostics,
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
        gradient.linear_direction(),
        Some(GradientDirection::Angle(std::f32::consts::FRAC_PI_2))
    );
    assert_eq!(gradient.stops[0].position, Some(Length::Percent(0.)));
    assert_eq!(gradient.stops[0].color.rgb, 0xffffff);
    assert_eq!(gradient.stops[0].color.alpha, 0.);
    assert_eq!(gradient.stops[1].position, Some(Length::Px(4.)));
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
        gradient.linear_direction(),
        Some(GradientDirection::Corner {
            right: true,
            bottom: true
        })
    );
    assert_eq!(gradient.stops[0].color.rgb, 0x123456);
    assert_eq!(gradient.stops[0].position, Some(Length::Percent(0.2)));
    assert_eq!(gradient.stops[1].color.rgb, 0x1e2832);
    assert_eq!(gradient.stops[1].color.alpha, 0.5);
    assert_eq!(gradient.stops[1].position, Some(Length::Percent(1.)));
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
            style.background_gradient.unwrap().linear_direction(),
            Some(GradientDirection::Angle(radians))
        );
        assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
    }
}

#[test]
fn multi_stop_repeating_radial_and_sized_gradients_extract_cleanly() {
    let (style, diagnostics) =
        compute("background-image:linear-gradient(90deg,red,20%,green calc(2px + 50%),blue)");
    assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
    let g = style.background_gradient.unwrap();
    assert_eq!(g.stops.len(), 3);
    assert_eq!(g.stops[0].hint_to_next, Some(Length::Percent(0.2)));
    assert!(matches!(
        g.stops[1].position,
        Some(Length::Calc { px, percent, .. })
            if (px - 2.0).abs() < 1e-4 && (percent - 0.5).abs() < 1e-4
    ));

    let (style, diagnostics) = compute(
        "background-image:repeating-linear-gradient(45deg,red 0px,blue 10px);background-size:20px 30px;background-position:2px 4px",
    );
    assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
    let g = style.background_gradient.unwrap();
    assert!(g.repeating);
    assert_eq!(g.size, [Length::Px(20.0), Length::Px(30.0)]);
    assert_eq!(g.position, [Length::Px(2.0), Length::Px(4.0)]);

    let (style, diagnostics) =
        compute("background-image:radial-gradient(circle closest-side at 25% 75%,red,blue)");
    assert!(diagnostics.entries.is_empty(), "{diagnostics:?}");
    let g = style.background_gradient.unwrap();
    assert_eq!(
        g.kind,
        GradientKind::Radial {
            shape: RadialShape::CircleExtent(RadialExtent::ClosestSide),
            center: [Length::Percent(0.25), Length::Percent(0.75)],
        }
    );
}

#[test]
fn unsupported_forms_are_omitted_with_explicit_node_diagnostics_not_coerced() {
    let seventeen_stops = format!(
        "background-image:linear-gradient({})",
        vec!["red"; 17].join(",")
    );
    for css in [
        "background-image:conic-gradient(red,blue)",
        "background-image:linear-gradient(in oklab,red,blue)",
        "background-image:linear-gradient(red,blue),linear-gradient(white,black)",
        "background-image:linear-gradient(red,blue);background-attachment:fixed",
        "background-image:linear-gradient(red,blue);background-repeat:round",
        "background-image:linear-gradient(red 1000001px,blue)",
        seventeen_stops.as_str(),
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
