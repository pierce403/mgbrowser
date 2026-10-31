//! Regression coverage for real-site CSS, SVG, layout, and classic-script compatibility
//! uncovered during live multi-site audits (`slashdot.org`, `lobste.rs`, `en.wikipedia.org`,
//! `reddit.com`, and Cloudflare-protected sites).
#![cfg(feature = "modern")]

use mg_sparkle::{
    document::{self, Document, parse},
    images,
    js_browser::{
        Request,
        boa::{BoaPageRealm, execute},
    },
    page_session::{LifecyclePhase, RealmState, SessionReply, TaskPump},
    paint::Fonts,
    render::{Controls, Frame, Viewport, render_scaled},
    style::{StyleDiagnostics, compute_styles_with_diagnostics},
};

fn fonts() -> Fonts {
    Fonts::from_bytes(
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap(),
    )
    .unwrap()
}

fn render_doc(document: &Document) -> Frame {
    let mut fonts = fonts();
    render_scaled(
        document,
        &mut fonts,
        Viewport {
            width: 360,
            height: 240,
            scroll: 0,
        },
        &Controls::default(),
        1.0,
    )
}

#[test]
fn slashdot_legacy_moz_border_radius_and_inline_svg_attributes_and_large_viewbox_render_cleanly() {
    let html = r#"<!doctype html>
<html>
<head>
<style>
  html, body { margin: 0; background: #ffffff; }
  .card {
    width: 120px;
    height: 48px;
    background: #006666;
    -moz-border-radius: .6em;
    -moz-box-sizing: border-box;
    padding: 8px;
  }
</style>
</head>
<body>
  <div class="card">
    <svg xmlns="http://www.w3.org/2000/svg"
         xmlns:xlink="http://www.w3.org/1999/xlink"
         xml:space="preserve"
         style="width: 20px; height: 20px; fill: #ffcc00"
         viewBox="0 260 1792 1260">
      <defs><path id="unused" d="M0 0h10v10z"/></defs>
      <rect x="0" y="260" width="1792" height="1260" style="opacity: 0.9"/>
    </svg>
  </div>
</body>
</html>"#;
    let doc = parse(html, "https://slashdot.example/");
    let frame = render_doc(&doc);
    assert!(
        frame.diagnostics.entries.is_empty(),
        "expected zero CSS or SVG diagnostics: {:?}",
        frame.diagnostics.entries
    );
    // Verify that default intrinsic SVG rasterization with a large vector viewBox also scales down
    // proportionally within the pixel cap instead of failing check_size.
    let raw_svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 260 1792 1260"><rect x="0" y="260" width="1792" height="1260" fill="#d00"/></svg>"##;
    let decoded = images::decode(raw_svg, "image/svg+xml", None, None)
        .expect("large viewBox SVG scales into budget");
    assert!(decoded.width > 0 && decoded.height > 0);
    assert!((decoded.width as u64) * (decoded.height as u64) <= 1024 * 1024);
}

#[test]
fn vendor_prefix_normalization_preserves_exact_line_and_column_diagnostics() {
    let html = "<!doctype html><html><head><style>\n.box { -moz-border-radius: 4px; font-weignt: normal; }\n</style></head><body><div class='box'>x</div></body></html>";
    let doc = parse(html, "https://slashdot.example/");
    let mut diagnostics = StyleDiagnostics::default();
    compute_styles_with_diagnostics(&doc, &doc.stylesheets, (360.0, 240.0), &mut diagnostics)
        .expect("styled computation succeeds");
    assert_eq!(diagnostics.entries.len(), 1);
    let diag = &diagnostics.entries[0];
    assert_eq!(diag.kind, "css-parse");
    assert_eq!(diag.line, Some(2));
    assert!(
        diag.message.contains("font-weignt"),
        "unexpected diagnostic: {diag:?}"
    );
}

#[test]
fn wikipedia_inline_relative_containing_ancestor_does_not_trigger_layout_fallback() {
    let html = r#"<!doctype html>
<html>
<head>
<style>
  body { margin: 8px; }
  .inline-rel { position: relative; }
  .badge { position: absolute; top: 2px; left: 4px; width: 16px; height: 12px; background: #0055aa; }
</style>
</head>
<body>
  <p>Rust is a systems programming language <span class="inline-rel">with inline relative container<span class="badge">!</span></span> and styled layout.</p>
</body>
</html>"#;
    let doc = parse(html, "https://wiki.example/Rust");
    let frame = render_doc(&doc);
    assert!(
        !frame
            .diagnostics
            .entries
            .iter()
            .any(|e| matches!(e.kind, "layout-fallback" | "style-fallback")),
        "inline position:relative ancestor must not force whole-page fallback: {:?}",
        frame.diagnostics.entries
    );
}

#[test]
fn annex_b_html_comments_iframe_challenge_expandos_and_document_write_execute_without_errors() {
    let script = r#"
        <!-- legacy HTML open comment in classic script (Slashdot 6sense pattern)
        const preservedStr = "<!-- not a comment -->";
        const preservedTpl = `prefix ${1 + 2} <!-- still template -->`;
        const preservedRx = /<!--|-->/;
        let n = 2;
        let dec = (n --> 0) && (n === 1);
        --> legacy HTML close comment at line start
        /* inline block */ --> close comment after single-line block comment
        if (preservedStr.length !== 22 || !preservedTpl.includes("<!--") || !preservedRx.test("<!--") || !dec) {
            throw new Error("Annex B scanner corrupted JS token");
        }

        // document.currentScript & document.write during parsing
        const cs = document.currentScript;
        if (!cs || cs.tagName !== "SCRIPT") {
            throw new Error("document.currentScript missing");
        }
        document.write('<span id="written" title="from-write">written-ok</span>');

        // DOM expando properties + reflected attributes + Cloudflare blank iframe pattern
        const box = document.getElementById("written");
        box.__customExpando = { count: 7 };
        if (box.__customExpando.count !== 7 || box.title !== "from-write") {
            throw new Error("expando or reflected attribute failed");
        }
        box.tabIndex = 2;
        if (box.tabIndex !== 2) {
            throw new Error("tabIndex reflection failed");
        }

        const ifr = document.createElement("iframe");
        ifr.height = 1;
        ifr.width = 1;
        if (ifr.height !== "1" || ifr.width !== "1") {
            throw new Error("HTMLIFrameElement width/height reflection failed");
        }
        if (ifr.contentDocument !== null || ifr.contentWindow !== null) {
            throw new Error("disconnected iframe should have null contentDocument/contentWindow");
        }
        document.body.appendChild(ifr);
        const subDoc = ifr.contentDocument || ifr.contentWindow.document;
        if (!subDoc || !ifr.contentWindow || ifr.contentWindow.document !== subDoc) {
            throw new Error("connected blank iframe missing contentDocument/contentWindow");
        }
        const innerScript = subDoc.createElement("script");
        innerScript.innerHTML = "window.__CF$cv$params = { r: 'test' };";
        subDoc.getElementsByTagName("head")[0].appendChild(innerScript);

        Promise.resolve().then(() => {
            if (document.currentScript !== null) {
                throw new Error("document.currentScript must be null in microtasks");
            }
            document.title = "compat-ok";
        });
    "#;
    let req = Request {
        external_scripts: None,
        url: "https://slashdot.example/".into(),
        html: format!(
            "<!doctype html><html><head><title>init</title></head><body><script>{script}</script></body></html>"
        ),
    };
    let (mut realm, mut reply): (Option<BoaPageRealm>, SessionReply) = BoaPageRealm::start(req);
    let mut all_errors = std::mem::take(&mut reply.errors);
    while let Some(wake) = reply.task_wake.clone() {
        if reply.state != RealmState::Ready
            || reply
                .lifecycle
                .is_some_and(|l| l.phase == LifecyclePhase::LoadComplete)
        {
            break;
        }
        reply = realm.as_mut().unwrap().pump_tasks(TaskPump {
            wake_token: wake.token,
        });
        all_errors.append(&mut reply.errors);
    }
    assert!(
        all_errors.is_empty(),
        "unexpected JS errors: {all_errors:?}"
    );
    let doc = document::project_nodes(
        reply.snapshot.as_ref().unwrap().nodes.clone(),
        "https://slashdot.example/",
    )
    .unwrap();
    assert_eq!(doc.title, "compat-ok");
    let written = doc
        .nodes
        .iter()
        .find(|n| n.attr("id") == Some("written"))
        .expect("#written inserted by document.write");
    assert_eq!(written.attr("tabindex"), Some("2"));
}

#[test]
fn strict_mode_readonly_idl_assignment_throws_while_sloppy_mode_is_silent() {
    let reply = execute(Request {
        external_scripts: None,
        url: "https://example.test/".into(),
        html: r#"<!doctype html><html><head><title>before</title></head><body><div id="d"></div><script>
            const d = document.getElementById("d");
            d.tagName = "SPAN";
            if (d.tagName !== "DIV") throw new Error("tagName mutated");
            let strictThrew = false;
            try {
                (function() { "use strict"; d.tagName = "SPAN"; })();
            } catch (e) {
                strictThrew = e instanceof TypeError;
            }
            if (!strictThrew) throw new Error("strict assignment to readonly IDL did not throw TypeError");
            document.title = "readonly-ok";
        </script></body></html>"#
            .into(),
    });
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(reply.html.contains("<title>readonly-ok</title>"));
}

#[test]
fn viewbox_only_inline_svg_fills_containing_width_instead_of_viewbox_coordinates() {
    let html = r#"<!doctype html><html><head><style>
        body { margin: 0; }
        .social-link { display: block; width: 20px; line-height: 20px; }
        .social-link i { position: relative; }
        .social-link svg { position: relative; fill: red; }
    </style></head><body>
        <a class="social-link" href="https://example.test/yt">
            <i><svg id="yt" viewBox="0 260 1792 1260" xmlns="http://www.w3.org/2000/svg">
                <rect x="0" y="260" width="1792" height="1260"/>
            </svg></i>
        </a>
    </body></html>"#;
    let doc = parse(html, "https://slashdot.example/");
    let yt_id = doc
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("yt"))
        .expect("#yt node");
    let frame = render_doc(&doc);
    assert!(
        frame.diagnostics.entries.is_empty(),
        "unexpected diagnostics: {:?}",
        frame.diagnostics.entries
    );
    let yt_box = frame
        .boxes
        .iter()
        .find(|b| b.node == yt_id)
        .expect("#yt layout box");
    assert_eq!(
        yt_box.width, 20,
        "viewBox-only inline SVG should size to 20px container width"
    );
    assert!(
        (13..=15).contains(&yt_box.height),
        "expected ~14px proportional height for 1792x1260 viewBox at 20px width, got {}",
        yt_box.height
    );
}

#[test]
fn flex_item_float_measurement_does_not_leak_floats_into_clear_both_children() {
    let html = r#"<!doctype html><html><head><style>
        body { margin: 0; display: flex; flex-direction: column; gap: 10px; }
        header { height: 30px; background: #eee; }
        ol { margin: 0; padding: 0; list-style: none; }
        li.story { clear: both; }
        div.voters { float: left; width: 40px; height: 40px; background: #ccc; }
        div.details { min-height: 40px; }
    </style></head><body>
        <header>Header</header>
        <main id="inside">
            <ol>
                <li id="s1" class="story"><div class="voters">1</div><div class="details">First story</div></li>
                <li id="s2" class="story"><div class="voters">2</div><div class="details">Second story</div></li>
            </ol>
        </main>
    </body></html>"#;
    let doc = parse(html, "https://lobsters.example/");
    let s1_id = doc
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("s1"))
        .expect("#s1");
    let s2_id = doc
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("s2"))
        .expect("#s2");
    let frame = render_doc(&doc);
    let s1_box = frame
        .boxes
        .iter()
        .find(|b| b.node == s1_id)
        .expect("#s1 visible in initial viewport");
    let s2_box = frame
        .boxes
        .iter()
        .find(|b| b.node == s2_id)
        .expect("#s2 visible in initial viewport");
    assert_eq!(
        s1_box.y, 40,
        "first story must start right below 30px header + 10px gap"
    );
    assert_eq!(
        s2_box.y, 80,
        "second story must clear right below first 40px story"
    );
}

#[test]
fn vector_2022_grid_template_areas_renders_two_column_desktop_layout() {
    let html = r#"<!doctype html><html><head><style>
        body { margin: 0; }
        .mw-page-container-inner {
            display: grid;
            column-gap: 24px;
            grid-template: min-content 1fr min-content / 200px minmax(0, 1fr);
            grid-template-areas:
                'siteNotice siteNotice'
                'columnStart pageContent'
                'footer footer';
        }
        #notice { grid-area: siteNotice; height: 20px; }
        #sidebar { grid-area: columnStart; height: 120px; }
        #content { grid-area: pageContent; height: 300px; }
        #footer { grid-area: footer; height: 40px; }
    </style></head><body>
        <div class="mw-page-container-inner">
            <div id="notice">Notice</div>
            <div id="sidebar">TOC</div>
            <div id="content">Article</div>
            <div id="footer">Footer</div>
        </div>
    </body></html>"#;
    let doc = parse(html, "https://en.wikipedia.example/wiki/Rust");
    let sidebar_id = doc
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("sidebar"))
        .expect("#sidebar");
    let content_id = doc
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("content"))
        .expect("#content");
    let footer_id = doc
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("footer"))
        .expect("#footer");
    let frame = render_doc(&doc);
    assert!(
        frame
            .diagnostics
            .entries
            .iter()
            .all(|d| !matches!(d.kind, "layout-fallback" | "style-fallback")),
        "expected styled grid layout without fallback: {:?}",
        frame.diagnostics.entries
    );
    let sidebar_box = frame
        .boxes
        .iter()
        .find(|b| b.node == sidebar_id)
        .expect("#sidebar box");
    let content_box = frame
        .boxes
        .iter()
        .find(|b| b.node == content_id)
        .expect("#content box");
    let footer_box = frame
        .boxes
        .iter()
        .find(|b| b.node == footer_id)
        .expect("#footer box");
    assert_eq!(sidebar_box.x, 0);
    assert_eq!(sidebar_box.width, 200);
    assert_eq!(content_box.x, 224);
    assert_eq!(sidebar_box.y, 20);
    assert_eq!(content_box.y, 20);
    assert_eq!(footer_box.y, 320);
}
