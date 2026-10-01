use mg_sparkle::{
    document::{Document, parse},
    style::{
        self, AlignKeyword, AlignSafety, BoxSizing, ComputedStyle, Display, FlexBasis,
        FlexDirection, FlexWrap, GridAutoFlow, GridLine, Length, Overflow, Position,
        StyleDiagnostics, TrackBreadth, TrackSize,
    },
};

fn compute(html: &str) -> (Document, Vec<ComputedStyle>, StyleDiagnostics) {
    let doc = parse(
        &format!("<html><body>{html}</body></html>"),
        "https://fixture.example/",
    );
    let mut diagnostics = StyleDiagnostics::default();
    let styles = style::compute_styles_with_diagnostics(
        &doc,
        &doc.stylesheets,
        (1024., 768.),
        &mut diagnostics,
    )
    .unwrap();
    (doc, styles, diagnostics)
}
fn at<'a>(doc: &Document, styles: &'a [ComputedStyle], id: &str) -> &'a ComputedStyle {
    &styles[doc.query_selector(0, id).unwrap().unwrap()]
}

#[test]
fn typed_flex_and_alignment_keep_computed_values_without_obsolete_block_warning() {
    let (doc, styles, diagnostics) = compute(
        "<div id=x style='display:inline-flex;flex-direction:column-reverse;flex-wrap:wrap-reverse;flex:2 3 25%;order:-2;gap:12px 5%;align-content:space-between;justify-content:space-evenly;align-items:safe center;align-self:baseline;justify-items:end;justify-self:auto;box-sizing:border-box'>text</div>",
    );
    let x = at(&doc, &styles, "#x");
    assert_eq!(x.display, Display::InlineFlex);
    let l = &x.layout;
    assert_eq!(l.flex_direction, FlexDirection::ColumnReverse);
    assert_eq!(l.flex_wrap, FlexWrap::WrapReverse);
    assert_eq!(
        (l.flex_grow, l.flex_shrink, l.flex_basis),
        (2., 3., FlexBasis::Length(Length::Percent(0.25)))
    );
    assert_eq!(l.order, -2);
    assert_eq!(l.gap, [Length::Px(12.), Length::Percent(0.05)]);
    assert_eq!(l.align_content.keyword, AlignKeyword::SpaceBetween);
    assert_eq!(l.justify_content.keyword, AlignKeyword::SpaceEvenly);
    assert_eq!(l.align_items.keyword, AlignKeyword::Center);
    assert_eq!(l.align_items.safety, AlignSafety::Safe);
    assert_eq!(l.align_self.keyword, AlignKeyword::Baseline);
    assert_eq!(l.justify_items.keyword, AlignKeyword::End);
    assert_eq!(l.box_sizing, BoxSizing::BorderBox);
    assert!(!l.unsupported);
    assert!(
        !diagnostics
            .entries
            .iter()
            .any(|d| d.kind == "css-unsupported" && d.message.contains("display: inline-flex"))
    );
    assert!(
        diagnostics
            .entries
            .iter()
            .all(|d| !d.message.contains("using block layout"))
    );
    assert!(!diagnostics.entries.iter().any(|d| d.kind == "css-parse"));
}

#[test]
fn twelve_column_grid_repeat_and_eight_four_spans_are_typed() {
    let (doc, styles, diagnostics) = compute(
        "<div id=g style='display:grid;grid-template-columns:repeat(12,minmax(0,1fr));grid-template-rows:24px auto;grid-auto-rows:minmax(10px,max-content);grid-auto-flow:row dense;gap:16px'><div id=a style='grid-column:span 8;grid-row:1 / -1'>A</div><div id=b style='grid-column:span 4'>B</div></div>",
    );
    let g = at(&doc, &styles, "#g");
    assert_eq!(g.display, Display::Grid);
    let grid = g.layout.grid.as_ref().unwrap();
    assert_eq!(
        grid.template_columns,
        vec![TrackSize::MinMax(TrackBreadth::Length(Length::Px(0.)), TrackBreadth::Fr(1.)); 12]
    );
    assert_eq!(
        grid.template_rows,
        [
            TrackSize::Breadth(TrackBreadth::Length(Length::Px(24.))),
            TrackSize::Breadth(TrackBreadth::Auto)
        ]
    );
    assert_eq!(
        grid.auto_rows,
        [TrackSize::MinMax(
            TrackBreadth::Length(Length::Px(10.)),
            TrackBreadth::MaxContent
        )]
    );
    assert_eq!(grid.auto_flow, GridAutoFlow::RowDense);
    assert_eq!(
        at(&doc, &styles, "#a").layout.grid_column,
        [GridLine::Span(8), GridLine::Auto]
    );
    assert_eq!(
        at(&doc, &styles, "#a").layout.grid_row,
        [GridLine::Line(1), GridLine::Line(-1)]
    );
    assert_eq!(
        at(&doc, &styles, "#b").layout.grid_column[0],
        GridLine::Span(4)
    );
    assert!(!g.layout.unsupported);
    assert!(!diagnostics.entries.iter().any(|d| d.kind == "css-parse"));
}

#[test]
fn grid_preferences_apply_before_inline_and_sheet_parsing() {
    let (doc, styles, report) = compute(
        "<style>#a{display:grid;grid-template-columns:repeat(2,10px 20%)}</style><div id=a></div><span id=b style='display:inline-grid;grid-template-columns:1fr'></span>",
    );
    assert_eq!(at(&doc, &styles, "#a").display, Display::Grid);
    assert_eq!(
        at(&doc, &styles, "#a")
            .layout
            .grid
            .as_ref()
            .unwrap()
            .template_columns
            .len(),
        4
    );
    assert_eq!(at(&doc, &styles, "#b").display, Display::InlineGrid);
    assert!(!report.entries.iter().any(|d| d.kind == "css-parse"));
}

#[test]
fn intrinsic_minimum_width_is_explicit_and_unrelated_loss_stays_unsupported() {
    use style::IntrinsicSize;
    let (doc, styles, _) = compute(
        "<div id=a style='min-width:min-content'></div><div id=b style='min-width:max-content'></div><div id=c style='min-width:min-content;width:calc(50% + 2px)'></div><div id=d style='width:min-content'></div>",
    );
    assert_eq!(
        at(&doc, &styles, "#a").layout.min_width_intrinsic,
        Some(IntrinsicSize::MinContent)
    );
    assert_eq!(
        at(&doc, &styles, "#b").layout.min_width_intrinsic,
        Some(IntrinsicSize::MaxContent)
    );
    assert!(!at(&doc, &styles, "#a").layout.unsupported);
    assert!(!at(&doc, &styles, "#b").layout.unsupported);
    assert!(at(&doc, &styles, "#c").layout.unsupported);
    assert!(at(&doc, &styles, "#d").layout.unsupported);
}

#[test]
fn absent_writing_containment_and_ratio_semantics_have_explicit_fallbacks() {
    for (property, value) in [
        ("direction", "rtl"),
        ("writing-mode", "vertical-rl"),
        ("contain", "layout"),
        ("contain", "style"),
        ("aspect-ratio", "3 / 2"),
    ] {
        let (doc, styles, report) =
            compute(&format!("<div id=x style='{property}:{value}'>text</div>"));
        let id = doc.query_selector(0, "#x").unwrap().unwrap();
        if report
            .entries
            .iter()
            .any(|d| d.kind == "css-parse" && d.message.contains(property))
        {
            // Some properties are disabled by the pinned Stylo preferences.
            // Keep that parser rejection; do not enable unrelated preferences.
            assert!(!styles[id].layout.unsupported, "{property}:{value}");
            continue;
        }
        assert!(
            styles[id].layout.unsupported,
            "{property}:{value}: {report:?}"
        );
        assert!(
            report.entries.iter().any(|d| d.node == Some(id)
                && d.kind == "css-unsupported"
                && d.message.contains(property)),
            "{property}:{value}"
        );
    }
}

#[test]
fn physical_position_overflow_and_z_index_are_not_confused_with_taffy_defaults() {
    let (doc, styles, _) = compute(
        "<div id=a style='position:fixed;top:12px;left:25%;right:auto;bottom:-4px;overflow:hidden;z-index:-3'></div><div id=b style='position:relative;overflow:clip'></div><div id=c></div>",
    );
    let a = &at(&doc, &styles, "#a").layout;
    assert_eq!(a.position, Position::Fixed);
    assert_eq!(
        a.inset,
        [
            Length::Px(12.),
            Length::Auto,
            Length::Px(-4.),
            Length::Percent(0.25)
        ]
    );
    assert_eq!(a.overflow, [Overflow::Hidden; 2]);
    assert_eq!(a.z_index, Some(-3));
    assert_eq!(at(&doc, &styles, "#b").layout.overflow, [Overflow::Clip; 2]);
    assert_eq!(at(&doc, &styles, "#c").layout.position, Position::Static);
    assert_eq!(
        at(&doc, &styles, "#c").layout.box_sizing,
        BoxSizing::ContentBox
    );
}

#[test]
fn text_inherits_typography_but_not_owned_grid_vectors_or_item_constraints() {
    let (doc, styles, _) = compute(
        "<div id=g style='display:grid;grid-template-columns:repeat(128,1fr);min-width:min-content;color:#123456;font-size:19px;order:3'>hello<b>world</b>again</div>",
    );
    let parent = at(&doc, &styles, "#g");
    assert_eq!(
        parent.layout.grid.as_ref().unwrap().template_columns.len(),
        128
    );
    for (id, node) in doc
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.tag == "#text")
    {
        assert!(
            styles[id].layout.grid.is_none(),
            "text {} cloned grid",
            node.text
        );
        assert_eq!(styles[id].layout.order, 0);
        assert_eq!(styles[id].layout.min_width_intrinsic, None);
        assert_eq!(styles[id].color.rgb, 0x123456);
        assert_eq!(styles[id].font_size, 19.);
    }
}

#[test]
fn unsupported_grid_and_mixed_lengths_are_diagnosed_not_silently_admitted() {
    for rule in [
        "grid-template-columns:repeat(auto-fit,20px)",
        "grid-template-columns:[named] 1fr",
        "grid-template-columns:fit-content(10px)",
        "grid-template-areas:\"a a\"",
        "grid-template-columns:calc(20% + 1px)",
    ] {
        let (doc, styles, report) =
            compute(&format!("<div id=x style='display:grid;{rule}'></div>"));
        let x = at(&doc, &styles, "#x");
        assert!(x.layout.unsupported, "{rule}");
        assert!(x.layout.grid.is_none(), "partial grid for {rule}");
        assert!(
            report
                .entries
                .iter()
                .any(|d| d.message.contains("not represented")),
            "{rule}"
        );
    }
    for rule in [
        "width:calc(50% + 2px)",
        "flex-basis:min-content",
        "left:calc(20% - 4px)",
        "gap:calc(1px + 2%)",
        "grid-column:named",
        "padding:calc(10% + 2px)",
    ] {
        let (doc, styles, report) =
            compute(&format!("<div id=x style='display:flex;{rule}'></div>"));
        assert!(at(&doc, &styles, "#x").layout.unsupported, "{rule}");
        assert!(
            report
                .entries
                .iter()
                .any(|d| d.message.contains("not represented")),
            "{rule}"
        );
    }
}

#[test]
fn expanded_and_implicit_track_and_numeric_placement_limits_reject() {
    for rule in [
        "grid-template-columns:repeat(129,1fr)",
        "grid-template-columns:repeat(1000000000,1px 1fr)",
        "grid-row:129",
        "grid-column:span 129",
        "grid-column:-129",
    ] {
        let doc = parse(
            &format!("<div style='display:grid;{rule}'></div>"),
            "https://fixture.example/",
        );
        let error = style::compute_styles(&doc, &doc.stylesheets, (1024., 768.)).unwrap_err();
        assert!(error.contains("bound exceeded"), "{rule}: {error}");
    }
    let doc = parse(
        &format!(
            "<div style='display:grid;grid-auto-rows:{}'></div>",
            "1px ".repeat(129)
        ),
        "https://fixture.example/",
    );
    assert!(
        style::compute_styles(&doc, &doc.stylesheets, (1024., 768.))
            .unwrap_err()
            .contains("implicit grid track bound")
    );
}

#[test]
fn owned_grid_snapshot_amplification_uses_existing_heap_bound() {
    let doc = parse(
        &format!(
            "<style>div{{display:grid;grid-template-columns:repeat(128,1fr);grid-template-rows:repeat(128,1fr)}}</style>{}",
            "<div></div>".repeat(2000)
        ),
        "https://fixture.example/",
    );
    assert_eq!(
        style::compute_styles(&doc, &doc.stylesheets, (1024., 768.)).unwrap_err(),
        "computed style snapshot bound exceeded"
    );
}
