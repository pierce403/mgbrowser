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

#[test]
fn mdn_named_grid_lines_place_content_and_full_columns_accurately() {
    let html = r#"<!doctype html><html><head><style>
        html, body { margin: 0; }
        .reference-layout {
            display: grid;
            grid-template-columns:
                [extended-full-start] 16px
                [full-start] 1fr
                [content-start] minmax(0, 768px)
                [content-end] 1fr
                [full-end] 16px
                [extended-full-end];
        }
        #bleed { grid-column: extended-full; height: 24px; }
        #full { grid-column: full; height: 32px; }
        #content { grid-column: content; height: 120px; }
    </style></head><body>
        <main class="reference-layout">
            <div id="bleed">Banner</div>
            <div id="full">Header</div>
            <article id="content">Article</article>
        </main>
    </body></html>"#;
    let doc = parse(html, "https://developer.mozilla.example/en-US/docs/Web/CSS");
    let mut fonts = fonts();
    let frame = render_scaled(
        &doc,
        &mut fonts,
        Viewport {
            width: 1200,
            height: 600,
            scroll: 0,
        },
        &Controls::default(),
        1.0,
    );
    assert!(
        frame.diagnostics.entries.is_empty(),
        "expected zero diagnostics on MDN named-line grid: {:?}",
        frame.diagnostics.entries
    );
    let box_of = |id: &str| {
        let node = doc.query_selector(0, id).unwrap().unwrap();
        frame.boxes.iter().find(|b| b.node == node).unwrap()
    };
    let bleed = box_of("#bleed");
    let full = box_of("#full");
    let content = box_of("#content");
    assert_eq!((bleed.x, bleed.width, bleed.y), (0, 1200, 0));
    assert_eq!((full.x, full.width, full.y), (16, 1168, 24));
    // Remaining 1168 - 768 = 400px split across two 1fr gutters => 200px each, so 16 + 200 = 216.
    assert_eq!(
        (content.x, full.width, content.width, content.y),
        (216, 1168, 768, 56)
    );
}

#[test]
fn wikipedia_portal_svg_sprite_use_style_and_transparent_gradient_render_cleanly() {
    let sprite_svg =
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="80" viewBox="0 0 40 80">
        <style>.top { fill: #cc0000; } .bottom { fill: #0066cc; }</style>
        <defs><rect id="tile" width="40" height="40"/></defs>
        <use href="#tile" x="0" y="0" class="top"/>
        <use href="#tile" x="0" y="40" class="bottom"/>
    </svg>"##;
    let html = r#"<!doctype html><html><head><style>
        body { margin: 0; background: #ffffff; }
        #wordmark {
            width: 40px;
            height: 40px;
            color: transparent;
            font-size: 32px;
            overflow: hidden;
            background-image: linear-gradient(transparent, transparent), url(sprite.svg);
            background-position: 0 -40px;
            background-repeat: no-repeat;
        }
    </style></head><body><div id="wordmark">Wikipedia</div></body></html>"#;
    let mut doc = parse(html, "https://www.wikipedia.example/");
    doc.resources.insert(
        "https://www.wikipedia.example/sprite.svg".into(),
        mg_sparkle::document::ResourceData {
            bytes: sprite_svg.to_vec(),
            content_type: "image/svg+xml".into(),
        },
    );
    let mut fonts = fonts();
    let frame = render_scaled(
        &doc,
        &mut fonts,
        Viewport {
            width: 120,
            height: 120,
            scroll: 0,
        },
        &Controls::default(),
        1.0,
    );
    assert!(
        frame.diagnostics.entries.is_empty(),
        "expected zero diagnostics on Wikipedia sprite + transparent gradient: {:?}",
        frame.diagnostics.entries
    );
    // Pixel (20, 20) inside the 40x40 box should paint the bottom half of the sprite (#0066cc), not red (#cc0000).
    let inside = frame.canvas.pixels[20 * 120 + 20];
    assert_eq!(
        inside, 0x0066cc,
        "expected #0066cc sprite slice at (20,20), got {inside:#08x}"
    );
    // Pixel (20, 50) below the 40x40 box must remain clipped to white (#ffffff).
    let below = frame.canvas.pixels[50 * 120 + 20];
    assert_eq!(
        below, 0xffffff,
        "sprite overflow below element box must be clipped, got {below:#08x}"
    );
}

#[test]
fn google_search_send_beacon_method_override_timing_substr_and_iframe_intrinsics_work() {
    let html = r#"<!doctype html><html><head><title>init</title></head><body>
        <div id="yvlrue" style="display:none">Fallback link</div>
        <iframe id="f"></iframe>
        <script>
            if (typeof navigator.sendBeacon !== "function") throw "missing sendBeacon";
            if (navigator.sendBeacon("/gen_204?cad=sg_trbl", "") !== true) throw "sendBeacon empty failed";
            if (navigator.sendBeacon("/gen_204", new Uint8Array([1, 2, 3])) !== true) throw "sendBeacon u8 failed";
            if (navigator.sendBeacon("/gen_204", new URLSearchParams("a=1&b=2")) !== true) throw "sendBeacon params failed";
            if (navigator.sendBeacon("/gen_204", "x".repeat(70000)) !== false) throw "sendBeacon >64KiB must return false";

            let threwBadScheme = false;
            try { navigator.sendBeacon("javascript:alert(1)"); } catch (e) { threwBadScheme = e instanceof TypeError; }
            if (!threwBadScheme) throw "sendBeacon javascript: must throw TypeError";

            let threwDetached = false;
            const unbound = navigator.sendBeacon;
            try { unbound("/gen_204"); } catch (e) { threwDetached = e instanceof TypeError; }
            if (!threwDetached) throw "unbound sendBeacon must throw TypeError";

            // Readonly IDL data attributes on navigator remain non-writable.
            const origUa = navigator.userAgent;
            navigator.userAgent = "spoof";
            if (navigator.userAgent !== origUa) throw "userAgent should stay readonly";
            let strictReadonly = false;
            try { (function() { "use strict"; navigator.webdriver = true; })(); } catch (e) { strictReadonly = e instanceof TypeError; }
            if (!strictReadonly || navigator.webdriver !== false) throw "webdriver should stay readonly";

            // WebIDL operations and expandos on navigator/console are writable in strict mode too.
            const calls = [];
            (function() {
                "use strict";
                navigator.sendBeacon = function(url, data) { calls.push(url + ":" + data); return true; };
                navigator.__customTag = "ok";
            })();
            if (navigator.__customTag !== "ok") throw "navigator expando failed";

            // Annex-B string/date compatibility & Navigation Timing + iframe intrinsics.
            if ("https://www.google.com/search".substr(0, 6) !== "https:") throw "substr prefix failed";
            if ("abcdef".substr(-3, 2) !== "de" || "abcdef".substr(2) !== "cdef") throw "substr negative/omitted failed";
            if ("  hi ".trimLeft() !== "hi " || " hi  ".trimRight() !== " hi") throw "trimLeft/Right failed";
            if (typeof Date.prototype.toGMTString !== "function") throw "missing toGMTString";
            if (!performance.timing || !(performance.timing.navigationStart > 0)) throw "missing performance.timing";
            if (performance.timing.responseStart < performance.timing.navigationStart) throw "bad responseStart";
            if (!performance.navigation || performance.navigation.type !== 0) throw "missing performance.navigation";

            const subWin = document.getElementById("f").contentWindow;
            if (!subWin || typeof subWin.String !== "function" || subWin.String("abc") !== "abc") {
                throw "iframe contentWindow missing String constructor";
            }

            setTimeout(function() {
                document.getElementById("yvlrue").setAttribute("style", "");
                navigator.sendBeacon("/gen_204?cad=sg_trbl&ei=test", "");
                document.title = calls.join("|");
            }, 0);
        </script>
    </body></html>"#;

    let (realm_opt, init) = BoaPageRealm::start(Request {
        url: "https://www.google.com/search?q=ninjas".into(),
        html: html.into(),
        external_scripts: None,
    });
    assert!(init.errors.is_empty(), "init errors: {:?}", init.errors);
    assert_eq!(init.state, RealmState::Ready);
    let mut realm = realm_opt.expect("live realm");
    let mut last = init;
    for seq in 1..=8 {
        let Some(wake) = last.task_wake else { break };
        if wake.delay_ms != 0 {
            std::thread::sleep(std::time::Duration::from_millis(wake.delay_ms as u64));
        }
        last = realm.pump_tasks(TaskPump {
            wake_token: wake.token,
        });
        assert!(
            last.errors.is_empty(),
            "pump {seq} errors: {:?}",
            last.errors
        );
    }
    let snap = last.snapshot.expect("final snapshot");
    let final_doc =
        document::project_nodes(snap.nodes, "https://www.google.com/search?q=ninjas").unwrap();
    assert_eq!(final_doc.title, "/gen_204?cad=sg_trbl&ei=test:");
    let yv = final_doc.query_selector(0, "#yvlrue").unwrap().unwrap();
    assert_eq!(final_doc.nodes[yv].attr("style"), Some(""));
}
