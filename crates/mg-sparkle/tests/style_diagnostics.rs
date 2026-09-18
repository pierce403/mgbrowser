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
        "<div id=flex style='display:flex;position:absolute;overflow:hidden'>Text</div><div id=grid style='display:grid'>Grid</div>",
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
    let flex = document.query_selector(0, "#flex").unwrap().unwrap();
    let grid = document.query_selector(0, "#grid").unwrap().unwrap();
    assert_eq!(styles[flex].display, Display::Block);
    assert_eq!(styles[grid].display, Display::Block);
    for (node, text) in [
        (flex, "display: flex"),
        (flex, "position: absolute"),
        (flex, "overflow-x: hidden"),
    ] {
        let diagnostic = report
            .entries
            .iter()
            .find(|d| d.node == Some(node) && d.message.contains(text))
            .unwrap_or_else(|| panic!("missing {text}: {report:?}"));
        assert_eq!(diagnostic.kind, "css-unsupported");
        assert_eq!((diagnostic.line, diagnostic.column), (None, None));
    }
    // The current Stylo preference profile rejects grid during parsing, before
    // it can become a computed value. Report that real error, not a fabricated
    // renderer fallback or an incidental preference change.
    let rejected_grid = report
        .entries
        .iter()
        .find(|d| d.node == Some(grid) && d.message.contains("display:grid"))
        .unwrap();
    assert_eq!(rejected_grid.kind, "css-parse");
    assert_eq!(rejected_grid.line, Some(1));
    assert_eq!(frame(&document).diagnostics, report);
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
