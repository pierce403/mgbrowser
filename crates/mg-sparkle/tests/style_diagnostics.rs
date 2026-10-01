use mg_sparkle::{
    document::{StylesheetSource, parse},
    paint::Fonts,
    render::{Controls, Frame, Viewport, render},
    style::{Display, StyleDiagnostics, compute_styles, compute_styles_with_diagnostics},
};

fn frame(document: &mg_sparkle::document::Document) -> Frame {
    let mut fonts = Fonts::from_bytes(
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap(),
    )
    .unwrap();
    render(
        document,
        &mut fonts,
        Viewport {
            width: 320,
            height: 240,
            scroll: 0,
        },
        &Controls::default(),
    )
}

#[test]
fn parse_errors_identify_sheet_and_attribute_without_losing_neighboring_styles() {
    let mut document = parse(
        "<p id=x style='color:#123456;mg-unknown-inline:1'>Hello</p>",
        "https://example.test/page",
    );
    document.stylesheets.push(StylesheetSource {
        css: "p {\n mg-unknown-sheet: 1;\n font-size:20px;\n}".into(),
        base_url: "https://example.test/css/main.css".into(),
        media: String::new(),
    });
    let mut report = StyleDiagnostics::default();
    let styles = compute_styles_with_diagnostics(
        &document,
        &document.stylesheets,
        (320., 240.),
        &mut report,
    )
    .unwrap();
    assert_eq!(
        styles,
        compute_styles(&document, &document.stylesheets, (320., 240.)).unwrap()
    );
    let id = document.query_selector(0, "#x").unwrap().unwrap();
    assert_eq!(styles[id].color.rgb, 0x123456);
    assert_eq!(styles[id].font_size, 20.);
    let inline = report
        .entries
        .iter()
        .find(|d| d.message.contains("mg-unknown-inline"))
        .unwrap();
    assert_eq!(inline.kind, "css-parse");
    assert_eq!(inline.node, Some(id));
    assert_eq!(inline.line, Some(1));
    assert!(inline.column.unwrap() > 1);
    assert!(inline.source.contains("style attribute"));
    let sheet = report
        .entries
        .iter()
        .find(|d| d.message.contains("mg-unknown-sheet"))
        .unwrap();
    assert_eq!(sheet.node, None);
    assert_eq!(sheet.line, Some(2));
    assert!(sheet.column.unwrap() >= 1);
    assert!(sheet.source.contains("https://example.test/css/main.css"));
    assert!(sheet.source.contains("stylesheet 1"));
    assert_eq!(report.omitted, 0);
}

#[test]
fn unsupported_computed_properties_report_actual_fallback_and_node() {
    let document = parse(
        "<div id=legacy style='display:flow-root;position:sticky;overflow:auto'>Text</div><div id=absolute style='position:absolute;z-index:-1'>Positioned</div>",
        "https://example.test/",
    );
    let mut report = StyleDiagnostics::default();
    let styles = compute_styles_with_diagnostics(
        &document,
        &document.stylesheets,
        (320., 240.),
        &mut report,
    )
    .unwrap();
    let legacy = document.query_selector(0, "#legacy").unwrap().unwrap();
    let absolute = document.query_selector(0, "#absolute").unwrap().unwrap();
    assert_eq!(styles[legacy].display, Display::Block);
    for (node, text) in [
        (legacy, "display: flow-root"),
        (legacy, "position: sticky"),
        (legacy, "overflow-x: auto"),
        (absolute, "both horizontal insets auto"),
        (absolute, "both vertical insets auto"),
        (absolute, "CSS z-index: -1"),
    ] {
        let diagnostic = report
            .entries
            .iter()
            .find(|d| d.node == Some(node) && d.message.contains(text))
            .unwrap_or_else(|| panic!("missing {text}: {report:?}"));
        assert_eq!(diagnostic.kind, "css-unsupported");
        assert_eq!(diagnostic.source, document.base_url);
        assert_eq!((diagnostic.line, diagnostic.column), (None, None));
    }
    assert_eq!(report.entries.len(), 6);
    assert_eq!(report.omitted, 0);
    assert!(!report.entries.iter().any(|d| d.kind == "css-parse"));
    assert_eq!(frame(&document).diagnostics, report);
}

#[test]
fn implemented_layout_paths_do_not_report_obsolete_fallbacks() {
    let document = parse(
        "<div style='display:flex;overflow:hidden;width:120px'><span>Flex</span></div><div style='display:grid;grid-template-columns:30px 40px;overflow:clip'><span>Grid</span></div><div style='position:relative;left:2px'>Relative</div><div style='position:absolute;left:0;top:100px'>Absolute</div><div style='position:fixed;right:0;bottom:0'>Fixed</div>",
        "https://example.test/",
    );
    let mut report = StyleDiagnostics::default();
    compute_styles_with_diagnostics(&document, &document.stylesheets, (320., 240.), &mut report)
        .unwrap();
    assert!(report.entries.is_empty(), "{report:?}");
    let rendered = frame(&document);
    assert_eq!(rendered.diagnostics, report);
    assert!(rendered.content_height > 0);
}

#[test]
fn rounded_corner_gap_is_node_scoped_bounded_and_does_not_change_square_paint() {
    let html = |radius: &str| {
        format!(
            "<div id=card style='width:40px;height:20px;background:red;border-radius:{radius}'></div>"
        )
    };
    let square = parse(&html("0"), "https://example.test/");
    let rounded = parse(&html("18px"), "https://example.test/");
    let baseline = frame(&square);
    let diagnosed = frame(&rounded);
    assert_eq!(baseline.canvas.pixels, diagnosed.canvas.pixels);
    assert_eq!(baseline.boxes, diagnosed.boxes);
    assert_eq!(
        format!("{:?}", baseline.hits),
        format!("{:?}", diagnosed.hits)
    );
    assert!(baseline.diagnostics.entries.is_empty());
    let node = rounded.query_selector(0, "#card").unwrap().unwrap();
    assert_eq!(diagnosed.diagnostics.entries.len(), 1);
    let diagnostic = &diagnosed.diagnostics.entries[0];
    assert_eq!(diagnostic.kind, "css-unsupported");
    assert_eq!(diagnostic.node, Some(node));
    assert_eq!(diagnostic.source, rounded.base_url);
    assert!(diagnostic.message.contains("border-radius"));
    assert!(diagnostic.message.contains("square corners"));
    let many = parse(
        &"<div style='border-radius:50%'></div>".repeat(100),
        "https://example.test/",
    );
    let mut report = StyleDiagnostics::default();
    compute_styles_with_diagnostics(&many, &many.stylesheets, (320., 240.), &mut report).unwrap();
    assert_eq!(report.entries.len(), StyleDiagnostics::MAX_ENTRIES);
    assert_eq!(report.omitted, 100 - StyleDiagnostics::MAX_ENTRIES);
}

#[test]
fn legacy_table_cell_clipping_boundary_is_explicit_without_changing_table_output() {
    let html = |overflow: &str| {
        format!(
            "<table style='border-spacing:0'><tr><td id=cell style='overflow:{overflow};font-size:13.328125px'><a href='/next'>Title <span style='font-size:10.671875px'>domain.example</span></a></td><td>Other</td></tr></table>"
        )
    };
    let visible = parse(&html("visible"), "https://example.test/");
    let clipped = parse(&html("hidden"), "https://example.test/");
    let id = clipped.query_selector(0, "#cell").unwrap().unwrap();
    let baseline = frame(&visible);
    let diagnosed = frame(&clipped);
    assert_eq!(baseline.canvas.pixels, diagnosed.canvas.pixels);
    assert_eq!(baseline.boxes, diagnosed.boxes);
    assert_eq!(baseline.content_height, diagnosed.content_height);
    let hits = |f: &Frame| {
        f.hits
            .iter()
            .map(|h| (h.x, h.y, h.w, h.h, h.action.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(hits(&baseline), hits(&diagnosed));
    assert!(baseline.diagnostics.entries.is_empty());
    assert_eq!(diagnosed.diagnostics.entries.len(), 1);
    let diagnostic = &diagnosed.diagnostics.entries[0];
    assert_eq!(diagnostic.kind, "css-unsupported");
    assert_eq!(diagnostic.node, Some(id));
    assert_eq!(diagnostic.source, clipped.base_url);
    assert!(diagnostic.message.contains("table-cell overflow clipping"));
    assert_eq!((diagnostic.line, diagnostic.column), (None, None));
}

#[test]
fn count_and_utf8_text_limits_explicitly_report_truncation() {
    let document = parse(
        &format!(
            "<div style='{}mg-{}:1'>Hello</div>",
            "mg-unknown:1;".repeat(100),
            "é".repeat(2000)
        ),
        &format!("https://example.test/{}", "é".repeat(500)),
    );
    let mut report = StyleDiagnostics::default();
    compute_styles_with_diagnostics(&document, &document.stylesheets, (320., 240.), &mut report)
        .unwrap();
    assert_eq!(report.entries.len(), StyleDiagnostics::MAX_ENTRIES);
    assert!(report.omitted > 0);
    assert!(
        report
            .entries
            .iter()
            .all(|d| d.source.len() <= StyleDiagnostics::MAX_SOURCE_BYTES
                && d.message.len() <= StyleDiagnostics::MAX_MESSAGE_BYTES)
    );
    assert!(
        report
            .entries
            .iter()
            .any(|d| d.truncated && d.source.ends_with("[truncated]"))
    );

    let mut large = parse(
        &format!("<div style='mg-{}:1'>Hello</div>", "é".repeat(2000)),
        "https://example.test/",
    );
    let mut report = StyleDiagnostics::default();
    compute_styles_with_diagnostics(&large, &large.stylesheets, (320., 240.), &mut report).unwrap();
    assert_eq!(report.entries.len(), 1);
    assert!(report.entries[0].truncated);
    assert!(report.entries[0].message.ends_with("[truncated]"));
    assert!(report.entries[0].message.len() <= StyleDiagnostics::MAX_MESSAGE_BYTES);
    // A rejected style input still returns its original error, not fake success.
    large.stylesheets = vec![
        StylesheetSource {
            css: String::new(),
            base_url: large.base_url.clone(),
            media: String::new()
        };
        65
    ];
    assert_eq!(
        compute_styles_with_diagnostics(&large, &large.stylesheets, (320., 240.), &mut report)
            .unwrap_err(),
        "stylesheet input bound exceeded"
    );
}

#[test]
fn diagnostics_do_not_change_pixels_geometry_or_hits() {
    let baseline = parse(
        "<p style='color:blue'><a href='/next'>Hello</a></p>",
        "https://example.test/",
    );
    let malformed = parse(
        "<p style='color:blue;mg-unsupported:value'><a href='/next'>Hello</a></p>",
        "https://example.test/",
    );
    let baseline = frame(&baseline);
    let diagnosed = frame(&malformed);
    assert_eq!(baseline.canvas.pixels, diagnosed.canvas.pixels);
    assert_eq!(baseline.boxes, diagnosed.boxes);
    assert_eq!(baseline.content_height, diagnosed.content_height);
    let hits = |f: &Frame| {
        f.hits
            .iter()
            .map(|h| (h.x, h.y, h.w, h.h, h.action.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(hits(&baseline), hits(&diagnosed));
    assert!(baseline.diagnostics.entries.is_empty());
    assert_eq!(diagnosed.diagnostics.entries.len(), 1);
}

#[test]
fn actual_style_and_layout_fallback_reasons_survive_full_diagnostic_buffer() {
    let mut document = parse("<p>Still readable</p>", "https://example.test/");
    let baseline = frame(&document);
    document.stylesheets = vec![
        StylesheetSource {
            css: String::new(),
            base_url: document.base_url.clone(),
            media: String::new()
        };
        65
    ];
    let fallback = frame(&document);
    assert_eq!(fallback.canvas.pixels, baseline.canvas.pixels);
    assert_eq!(fallback.boxes, baseline.boxes);
    assert!(fallback.diagnostics.entries.iter().any(
        |d| d.kind == "style-fallback" && d.message.contains("stylesheet input bound exceeded")
    ));

    let document = parse(
        &format!(
            "<p style='{}'>{}</p>",
            "mg-unknown:1;".repeat(100),
            "a".repeat(16_385)
        ),
        "https://example.test/",
    );
    let fallback = frame(&document);
    assert_eq!(
        fallback.diagnostics.entries.len(),
        StyleDiagnostics::MAX_ENTRIES
    );
    assert!(fallback.diagnostics.omitted > 0);
    assert_eq!(
        fallback.diagnostics.entries.last().unwrap().kind,
        "layout-fallback"
    );
    assert!(fallback.content_height > 0);
}
