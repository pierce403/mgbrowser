//! Regression coverage for real-site CSS, SVG, layout, and classic-script compatibility
//! uncovered during live multi-site audits (`slashdot.org`, `lobste.rs`, `en.wikipedia.org`,
//! `reddit.com`, and Cloudflare-protected sites).

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

#[test]
fn webidl_window_navigator_iframe_and_media_prototypes_enforce_brand_and_descriptors() {
    let html = r#"<!doctype html><html><body>
        <iframe id="f"></iframe>
        <video id="v"></video>
        <audio id="a"></audio>
        <div id="out"></div>
        <script>
        var checks = [];
        checks.push("winProto:" + (Object.getPrototypeOf(window) === Window.prototype));
        checks.push("winInst:" + (window instanceof Window));
        checks.push("winTag:" + Object.prototype.toString.call(window));
        checks.push("navProto:" + (Object.getPrototypeOf(navigator) === Navigator.prototype));
        checks.push("navInst:" + (navigator instanceof Navigator));
        checks.push("pluginsInst:" + (navigator.plugins instanceof PluginArray));
        checks.push("mimesInst:" + (navigator.mimeTypes instanceof MimeTypeArray));
        var wdDesc = Object.getOwnPropertyDescriptor(Navigator.prototype, "webdriver");
        checks.push("wdOwn:" + Object.prototype.hasOwnProperty.call(navigator, "webdriver"));
        checks.push("wdGet:" + (typeof (wdDesc && wdDesc.get) === "function" && navigator.webdriver === false));
        var brandThrew = false;
        try { wdDesc.get.call(window); } catch (e) { brandThrew = e instanceof TypeError; }
        checks.push("wdBrand:" + brandThrew);
        var f = document.getElementById("f");
        checks.push("ifInst:" + (f instanceof HTMLIFrameElement));
        var cwDesc = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, "contentWindow");
        checks.push("cwGet:" + (typeof (cwDesc && cwDesc.get) === "function"));
        var cw = f.contentWindow;
        checks.push("cwTag:" + Object.prototype.toString.call(cw));
        checks.push("cwWin:" + (cw.window === cw && cw.self === cw && cw.parent === window));
        checks.push("cwEval:" + (typeof cw.eval === "function" && typeof cw.Navigator === "function"));
        var detached = document.createElement("iframe");
        checks.push("detachedNull:" + (detached.contentWindow === null && detached.contentDocument === null));
        var v = document.getElementById("v");
        var a = document.getElementById("a");
        checks.push("mediaInst:" + (v instanceof HTMLVideoElement && v instanceof HTMLMediaElement && a instanceof HTMLAudioElement));
        checks.push("canPlay:" + (typeof HTMLMediaElement.prototype.canPlayType === "function" && v.canPlayType("video/mp4") === ""));
        document.title = checks.join(",");
        </script>
    </body></html>"#;
    let out = execute(Request {
        url: "https://www.google.com/search?q=ninjas".into(),
        html: html.into(),
        external_scripts: None,
    });
    assert!(out.errors.is_empty(), "errors: {:?}", out.errors);
    let doc = parse(&out.html, "https://www.google.com/search?q=ninjas");
    assert_eq!(
        doc.title,
        "winProto:true,winInst:true,winTag:[object Window],navProto:true,navInst:true,\
         pluginsInst:true,mimesInst:true,wdOwn:false,wdGet:true,wdBrand:true,ifInst:true,\
         cwGet:true,cwTag:[object Window],cwWin:true,cwEval:true,detachedNull:true,\
         mediaInst:true,canPlay:true"
    );
}

#[test]
fn large_data_image_attribute_materializes_inline_image_resource() {
    // Build a valid 1x1 GIF89a with a ~57 KB standard comment extension so its
    // base64 data:image/gif URL exceeds 75 KB (matching Google SERP inline thumbnails).
    let mut gif = vec![
        b'G', b'I', b'F', b'8', b'9', b'a', // Header
        1, 0, 1, 0, 0x80, 0, 0, // Logical screen descriptor (1x1, GCT 2 colors)
        0x12, 0x34, 0x56, 0x00, 0x00, 0x00, // GCT #0 = #123456, #1 = #000000
        0x21, 0xFE, // Comment Extension
    ];
    for _ in 0..230 {
        gif.push(250);
        gif.extend(std::iter::repeat_n(b'g', 250));
    }
    gif.push(0); // End of comment sub-blocks
    gif.extend_from_slice(&[
        0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, // Image descriptor (0,0 1x1)
        2, 2, 0x44, 0x01, 0,    // LZW minimum code size 2, pixel 0, block terminator
        0x3B, // Trailer
    ]);
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut data_url = String::from("data:image/gif;base64,");
    for chunk in gif.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let v = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        data_url.push(alphabet[((v >> 18) & 63) as usize] as char);
        data_url.push(alphabet[((v >> 12) & 63) as usize] as char);
        data_url.push(if chunk.len() > 1 {
            alphabet[((v >> 6) & 63) as usize] as char
        } else {
            '='
        });
        data_url.push(if chunk.len() > 2 {
            alphabet[(v & 63) as usize] as char
        } else {
            '='
        });
    }
    assert!(data_url.len() > 75_000, "len={}", data_url.len());

    let html = format!(
        r#"<!doctype html><html><body><img id="thumb" width="16" height="16"><script>
        var u = "{data_url}";
        var img = document.getElementById("thumb");
        img.src = u;
        document.title = "len:" + img.getAttribute("src").length;
        </script></body></html>"#
    );
    let (_, reply) = BoaPageRealm::start_with_platform_and_profile(
        Request {
            url: "https://www.google.com/search?q=ninjas&gbv=2".into(),
            html,
            external_scripts: None,
        },
        mg_sparkle::page_session::observer::ObserverInit::Disabled,
        mg_sparkle::page_session::platform::PlatformInit::default(),
        [0u8; 32],
        mg_butane::modern::ResourceProfile::InteractivePageV1,
    );
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);
    let snap = reply.snapshot.expect("snapshot");
    let doc = document::project_nodes(snap.nodes, "https://www.google.com/search?q=ninjas&gbv=2")
        .expect("project_nodes");
    assert_eq!(doc.title, format!("len:{}", data_url.len()));
    let res = doc
        .resources
        .get(&data_url)
        .expect("materialized data:image/gif");
    assert_eq!(res.content_type, "image/gif");
    let decoded = images::decode(&res.bytes, &res.content_type, None, None).unwrap();
    assert_eq!((decoded.width, decoded.height), (1, 1));
    assert_eq!(&decoded.pixels[..4], &[0x12, 0x34, 0x56, 0xFF]);
}

#[test]
fn webidl_window_mouse_event_and_image_surface_covers_audited_properties() {
    let html = r#"<!doctype html><html><body>
        <iframe id="ifr"></iframe>
        <script>
        var winProps = [
            "name","status","closed","frames","length","frameElement","origin",
            "screenX","screenY","screenLeft","screenTop","clientInformation",
            "offscreenBuffering","event","locationbar","menubar","personalbar",
            "scrollbars","statusbar","toolbar","external","alert","blur",
            "cancelIdleCallback","captureEvents","close","confirm","createImageBitmap",
            "find","focus","getSelection","moveBy","moveTo","open","print","prompt",
            "releaseEvents","reportError","requestIdleCallback","resizeBy","resizeTo","stop"
        ];
        var missingWin = winProps.filter(function(k) { return !(k in window); });
        for (var i = 0; i < winProps.length; i++) { void window[winProps[i]]; }

        var ev = document.createEvent("MouseEvents");
        ev.initMouseEvent("click", true, true, window, 1, 10, 20, 30, 40, false, false, true, false, 0, null);
        var evProps = [
            "screenX","screenY","x","y","offsetX","offsetY","movementX","movementY",
            "fromElement","toElement","layerX","layerY","getModifierState","view","which"
        ];
        var missingEv = evProps.filter(function(k) { return ev[k] === undefined; });
        var evUnknownOk = (ev.nonExistentCanary === undefined) && !("nonExistentCanary" in ev);
        var modOk = (ev.getModifierState("Shift") === true && ev.getModifierState("Control") === false);
        var coordsOk = (ev.screenX === 10 && ev.screenY === 20 && ev.x === 30 && ev.y === 40 && ev.which === 1 && ev.view === window);

        var img = document.createElement("img");
        var imgProps = [
            "baseURI","nodeValue","ENTITY_REFERENCE_NODE","ENTITY_NODE","NOTATION_NODE",
            "isDefaultNamespace","isEqualNode","isSameNode","lookupNamespaceURI",
            "lookupPrefix","normalize","part","attachShadow","checkVisibility",
            "getAttributeNS","getAttributeNode","getAttributeNodeNS","getElementsByTagNameNS",
            "hasAttributeNS","hasAttributes","removeAttributeNS","removeAttributeNode",
            "requestFullscreen","scroll","scrollBy","scrollIntoViewIfNeeded","scrollTo",
            "setAttributeNS","setAttributeNode","setAttributeNodeNS","translate","inert",
            "accessKey","draggable","spellcheck","autocapitalize","enterKeyHint","inputMode",
            "outerText","nonce","autofocus","useMap","naturalWidth","naturalHeight",
            "complete","currentSrc","fetchPriority","lowsrc","align","hspace","vspace",
            "longDesc","border","x","y","decode"
        ];
        var missingImg = imgProps.filter(function(k) { return img[k] === undefined; });

        var ifr = document.getElementById("ifr");
        var subDoc = ifr.contentDocument;
        var subDocOk = !("nonExistentCanary" in subDoc) && (subDoc.nonExistentCanary === undefined) && (ifr.contentWindow.frameElement === ifr);
        window.name = "mgtest";
        var winMetaOk = (window.name === "mgtest" && window.origin === "https://www.google.com" && window.length === 1 && window.getSelection().isCollapsed === true);

        document.title = [
            "mw:" + missingWin.length,
            "me:" + missingEv.length,
            "mi:" + missingImg.length,
            "eu:" + evUnknownOk,
            "mo:" + modOk,
            "co:" + coordsOk,
            "sd:" + subDocOk,
            "wm:" + winMetaOk
        ].join(",");
        </script>
    </body></html>"#;
    let out = execute(Request {
        url: "https://www.google.com/search?q=ninjas".into(),
        html: html.into(),
        external_scripts: None,
    });
    assert!(out.errors.is_empty(), "errors: {:?}", out.errors);
    let doc = parse(&out.html, "https://www.google.com/search?q=ninjas");
    assert_eq!(
        doc.title,
        "mw:0,me:0,mi:0,eu:true,mo:true,co:true,sd:true,wm:true"
    );
}

#[test]
fn styled_inline_anchor_wrapping_heading_exposes_non_zero_bounding_client_rect() {
    let html = r#"<!doctype html><html><head>
        <style>body { margin: 8px; } h3 { margin: 0; font-size: 20px; }</style>
    </head><body>
        <a id="hit" href="https://en.wikipedia.org/wiki/Ninja"><h3>Ninja - Wikipedia</h3></a>
        <a id="empty" href="/empty"></a>
        <script>
        var hit = document.getElementById("hit").getBoundingClientRect();
        var hitRects = document.getElementById("hit").getClientRects();
        var emp = document.getElementById("empty").getBoundingClientRect();
        document.title = [
            "hw:" + (hit.width > 40),
            "hh:" + (hit.height >= 16),
            "hr:" + (hitRects.length >= 1),
            "ew:" + (emp.width === 0 && emp.height === 0)
        ].join(",");
        </script>
    </body></html>"#;
    let out = execute(Request {
        url: "https://www.google.com/search?q=ninjas".into(),
        html: html.into(),
        external_scripts: None,
    });
    assert!(out.errors.is_empty(), "errors: {:?}", out.errors);
    let doc = parse(&out.html, "https://www.google.com/search?q=ninjas");
    assert_eq!(doc.title, "hw:true,hh:true,hr:true,ew:true");
}

#[test]
fn sloppy_direct_eval_dynamic_var_bindings_stay_isolated_across_repeated_calls() {
    let html = r#"<!doctype html><html><body><script>
        function runStep(code, seed) {
            var local = seed;
            return eval(code);
        }
        var r1 = runStep("var dynA = local + 1; var dynB = dynA * 2; (function(){ return dynB + local; })()", 10);
        var r2 = runStep("var dynC = local + 3; (function(){ return (typeof dynA) + ':' + dynC; })()", 20);
        var r3 = runStep("var dynA = 99; function innerFn(){ return dynA - local; } innerFn()", 30);
        document.title = [r1, r2, r3].join("|");
    </script></body></html>"#;
    let out = execute(Request {
        url: "https://www.google.com/search?q=ninjas".into(),
        html: html.into(),
        external_scripts: None,
    });
    assert!(out.errors.is_empty(), "errors: {:?}", out.errors);
    let doc = parse(&out.html, "https://www.google.com/search?q=ninjas");
    assert_eq!(doc.title, "32|undefined:23|69");
}

#[test]
fn worker_layout_cache_handles_over_sixty_four_inline_stylesheets_without_zero_rects() {
    let mut styles = String::new();
    for i in 0..72 {
        styles.push_str(&format!("<style>.s{i} {{ padding-left: 1px; }}</style>"));
    }
    let html = format!(
        r#"<!doctype html><html><head>{styles}</head><body>
        <div id="card" class="s0" style="width: 240px; height: 36px;">Result card</div>
        <script>
            var r = document.getElementById("card").getBoundingClientRect();
            document.title = Math.round(r.width) + "x" + Math.round(r.height);
        </script>
        </body></html>"#
    );
    let out = execute(Request {
        url: "https://www.google.com/search?q=ninjas".into(),
        html,
        external_scripts: None,
    });
    assert!(out.errors.is_empty(), "errors: {:?}", out.errors);
    let doc = parse(&out.html, "https://www.google.com/search?q=ninjas");
    assert_eq!(doc.title, "241x36");
}

#[test]
fn nested_list_dropdown_keeps_submenu_inside_parent_li_so_display_none_hides_overflow_menu() {
    let html = r##"<!doctype html><html><head><style>
        body { margin: 0; background: #ffffff; }
        ul { list-style: none; margin: 0; padding: 0; }
        .overflow_menu { display: none; background: #ff0000; width: 200px; height: 120px; }
    </style></head><body>
        <ul class="scopes">
            <li id="images"><a href="/images">Images</a>
            <li id="dots">
                <a href="#">More</a>
                <ul class="overflow_menu">
                    <li id="translate"><a href="/translator">Translate</a>
                    <li id="travel"><a href="/travel">Travel</a>
                </ul>
            <li id="news"><a href="/news">News</a>
        </ul>
    </body></html>"##;
    let doc = parse(html, "https://www.bing.com/");
    let dots = doc
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("dots"))
        .expect("#dots");
    let menu = doc
        .nodes
        .iter()
        .position(|n| n.attr("class") == Some("overflow_menu"))
        .expect(".overflow_menu");
    let translate = doc
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("translate"))
        .expect("#translate");
    assert_eq!(
        doc.nodes[menu].parent, dots,
        "nested ul.overflow_menu must stay inside #dots li"
    );
    assert_eq!(
        doc.nodes[translate].parent, menu,
        "nested li#translate must stay inside ul.overflow_menu"
    );
    let frame = render_doc(&doc);
    assert!(
        !frame.canvas.pixels.contains(&0xff0000),
        "display:none submenu must not paint leaked list items"
    );
}

#[test]
fn css_mask_image_and_webkit_mask_paint_tinted_alpha_icon() {
    let mask_svg = r##"data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect x="4" y="4" width="12" height="12" fill="black"/></svg>"##;
    let html = format!(
        r##"<!doctype html><html><head><style>
        body {{ margin: 0; background: #ffffff; }}
        #icon {{
            width: 20px;
            height: 20px;
            background-color: #1a73e8;
            -webkit-mask-image: url('{mask_svg}');
            -webkit-mask-size: 20px 20px;
        }}
    </style></head><body><div id="icon"></div></body></html>"##
    );
    let mut doc = parse(&html, "https://duckduckgo.com/");
    let (ct, raw) = images::decode_data_url(mask_svg).expect("decode mask svg");
    doc.resources.insert(
        mask_svg.to_owned(),
        document::ResourceData {
            bytes: raw,
            content_type: ct,
        },
    );
    let frame = render_doc(&doc);
    assert_eq!(
        frame.canvas.pixels[0], 0xffffff,
        "transparent outer mask ring must stay white"
    );
    let center = frame.canvas.pixels[10 * (frame.canvas.width as usize) + 10];
    assert_eq!(
        center, 0x1a73e8,
        "opaque mask interior must tint with background-color"
    );
}

#[test]
fn webp_sniff_and_large_interactive_image_normalize_into_renderable_bounds() {
    // Minimal lossless VP8L 2x2 green WebP container.
    let webp_2x2: [u8; 36] = [
        82, 73, 70, 70, 28, 0, 0, 0, 87, 69, 66, 80, 86, 80, 56, 76, 16, 0, 0, 0, 47, 1, 64, 0, 0,
        7, 80, 213, 40, 81, 255, 3, 17, 209, 255, 0,
    ];
    assert_eq!(images::sniff_image_mime(&webp_2x2), Some("image/webp"));
    assert_eq!(
        images::sniff_image_mime(b"\x89PNG\r\n\x1a\nrest"),
        Some("image/png")
    );
    assert_eq!(
        images::sniff_image_mime(b"\xff\xd8\xff\xe0rest"),
        Some("image/jpeg")
    );
    let decoded = images::decode(&webp_2x2, "image/webp", None, None).expect("decode 2x2 webp");
    assert_eq!((decoded.width, decoded.height), (2, 2));

    // Oversized SVG viewBox or raster normalizes cleanly in interactive mode.
    let big_svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60"><rect width="120" height="60" fill="#00aa44"/></svg>"##;
    assert_eq!(images::sniff_image_mime(big_svg), Some("image/svg+xml"));
    let (norm_ct, norm_bytes) =
        images::normalize_interactive_image(big_svg, "image/svg+xml").expect("normalize svg");
    let img = images::decode(&norm_bytes, &norm_ct, None, None).expect("decode normalized");
    assert_eq!((img.width, img.height), (120, 60));
}

#[test]
fn input_placeholder_and_author_styled_button_render_cleanly() {
    let html = r##"<!doctype html><html><head><style>
        body { margin: 8px; background: #ffffff; }
        input { width: 220px; height: 32px; background: #ffffff; border: 1px solid #dfe1e5; padding: 4px 8px; }
        button { width: 120px; height: 34px; background: #0969da; color: #ffffff; border: none; border-radius: 6px; }
    </style></head><body>
        <input id="q" placeholder="Search Google or type a URL">
        <button id="go">Google Search</button>
    </body></html>"##;
    let doc = parse(html, "https://www.google.com/");
    let frame = render_doc(&doc);
    let has_blue = frame.canvas.pixels.iter().any(|&px| {
        let r = (px >> 16) & 0xff;
        let g = (px >> 8) & 0xff;
        let b = px & 0xff;
        r < 30 && g > 80 && b > 190
    });
    assert!(has_blue, "author button background #0969da must paint");
}

#[test]
fn inner_text_excludes_script_style_and_hidden_subtrees() {
    let html = r#"<!doctype html><html><body>
        <div id="card">
            <style>.x { color: red; }</style>
            <script>var secretToken = 12345;</script>
            <span style="display: none">Hidden menu</span>
            <span hidden>Hidden attr</span>
            <span>Visible Headline</span>
        </div>
        <script>
            var el = document.getElementById("card");
            document.title = el.innerText.trim() + "|" + (el.textContent.indexOf("secretToken") >= 0);
        </script>
    </body></html>"#;
    let out = execute(Request {
        url: "https://www.bing.com/".into(),
        html: html.into(),
        external_scripts: None,
    });
    assert!(out.errors.is_empty(), "errors: {:?}", out.errors);
    let doc = parse(&out.html, "https://www.bing.com/");
    assert_eq!(doc.title, "Visible Headline|true");
}

#[test]
fn boa_platform_init_stylesheets_and_image_dimensions_drive_layout_and_hit_testing() {
    use mg_butane::modern::ResourceProfile;
    use mg_sparkle::page_session::{
        observer::ObserverInit,
        platform::{PlatformInit, ViewportState},
    };
    let html = r#"<!doctype html><html><head>
        <link rel="stylesheet" href="https://github.com/assets/app.css">
    </head><body>
        <div id="ghost" class="sr-only">Overlay</div>
        <button id="action" class="btn">Star</button>
        <img id="thumb" src="https://i.ytimg.com/vi/abc/hq720.jpg">
        <script>
            var hit = document.elementFromPoint(40, 20);
            var img = document.getElementById("thumb");
            document.title = [
                hit ? hit.id : "none",
                img.complete,
                img.naturalWidth + "x" + img.naturalHeight
            ].join("|");
        </script>
    </body></html>"#;
    let init = PlatformInit {
        viewport: ViewportState {
            inner_width: 1280,
            inner_height: 800,
            scroll_x: 0.0,
            scroll_y: 0.0,
            device_pixel_ratio: 1.0,
        },
        stylesheets: vec![(
            "body { margin: 0; } .sr-only { display: none; } .btn { display: block; width: 100px; height: 40px; }".into(),
            "https://github.com/assets/app.css".into(),
            String::new(),
        )],
        image_dimensions: vec![(
            "https://i.ytimg.com/vi/abc/hq720.jpg".into(),
            1280,
            720,
        )],
        ..PlatformInit::default()
    };
    let (realm, reply) = BoaPageRealm::start_with_platform_and_profile(
        Request {
            url: "https://github.com/rust-lang/rust".into(),
            html: html.into(),
            external_scripts: None,
        },
        ObserverInit::Disabled,
        init,
        [0x11; 32],
        ResourceProfile::InteractivePageV1,
    );
    assert!(realm.is_some());
    assert_eq!(reply.state, RealmState::Ready, "errors: {:?}", reply.errors);
    let doc = document::project_nodes(
        reply.snapshot.as_ref().expect("snapshot").nodes.clone(),
        "https://github.com/rust-lang/rust",
    )
    .expect("projected doc");
    assert_eq!(doc.title, "action|true|1280x720");
}

#[test]
fn key_input_enter_syncs_value_fires_keydown_submit_and_navigates_search_form() {
    use mg_butane::modern::ResourceProfile;
    use mg_sparkle::page_session::{
        observer::ObserverInit,
        platform::{PlatformCommand, PlatformInit},
    };
    let html = r#"<!doctype html><html><body>
        <form id="sf" action="/search" method="GET">
            <textarea id="q" name="q" rows="1" enterkeyhint="search"></textarea>
        </form>
        <script>
            window.__events = [];
            var q = document.getElementById("q");
            q.addEventListener("input", function() { window.__events.push("input:" + q.value); });
            q.addEventListener("keydown", function(e) { window.__events.push("key:" + e.key); });
            document.getElementById("sf").addEventListener("submit", function() {
                window.__events.push("submit");
                document.title = window.__events.join(",");
            });
        </script>
    </body></html>"#;
    let parsed = parse(html, "https://www.google.com/");
    let q_node = parsed
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("q"))
        .expect("#q node");
    let (realm, init_reply) = BoaPageRealm::start_with_platform_and_profile(
        Request {
            url: "https://www.google.com/".into(),
            html: html.into(),
            external_scripts: None,
        },
        ObserverInit::Disabled,
        PlatformInit::default(),
        [0x22; 32],
        ResourceProfile::InteractivePageV1,
    );
    let mut realm = realm.expect("interactive realm");
    assert_eq!(init_reply.state, RealmState::Ready);
    let reply = realm.platform_command(PlatformCommand::KeyInput {
        key: "Enter".into(),
        code: "Enter".into(),
        key_code: 13,
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
        target_value: Some((q_node, "rust programming".into())),
    });
    assert_eq!(
        reply.navigation.as_deref(),
        Some("https://www.google.com/search?q=rust+programming")
    );
    let out_doc = document::project_nodes(
        reply.snapshot.as_ref().expect("snapshot").nodes.clone(),
        "https://www.google.com/",
    )
    .expect("projected doc");
    assert_eq!(out_doc.title, "input:rust programming,key:Enter,submit");
}

#[test]
fn shreddit_and_amazon_storefront_fixtures_render_and_interact_cleanly() {
    use mg_butane::modern::ResourceProfile;
    use mg_sparkle::page_session::{
        InputKind, SessionInput, observer::ObserverInit, platform::PlatformInit,
    };
    let html = r##"<!doctype html><html><head><style>
        body { margin: 0; font-family: sans-serif; background: #0e1113; color: #eef1f3; }
        header { display: flex; align-items: center; padding: 8px 16px; background: #131f23; }
        .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; padding: 16px; }
        shreddit-post, .s-result-item { display: block; padding: 12px; background: #1a282d; border-radius: 8px; }
    </style></head><body>
        <header><input id="twotabsearchtextbox" placeholder="Search Reddit & Storefront"></header>
        <main class="grid">
            <shreddit-post id="p1" post-title="Rust 2026 Release"><button id="upvote">Upvote (10)</button></shreddit-post>
            <div class="s-result-item" id="a1"><span>Mechanical Keyboard</span></div>
        </main>
        <script>
            customElements.define("shreddit-post", class extends HTMLElement {
                connectedCallback() { this.setAttribute("data-hydrated", "1"); }
            });
            document.getElementById("upvote").addEventListener("click", function(e) {
                e.target.textContent = "Upvoted (11)";
                document.title = "hydrated=" + document.getElementById("p1").getAttribute("data-hydrated") + "|upvoted";
            });
        </script>
    </body></html>"##;
    let (realm, reply) = BoaPageRealm::start_with_platform_and_profile(
        Request {
            url: "https://www.reddit.com/r/rust/".into(),
            html: html.into(),
            external_scripts: None,
        },
        ObserverInit::Disabled,
        PlatformInit::default(),
        [0x33; 32],
        ResourceProfile::InteractivePageV1,
    );
    let mut realm = realm.expect("shreddit interactive realm");
    assert_eq!(reply.state, RealmState::Ready, "errors: {:?}", reply.errors);
    let doc0 = document::project_nodes(
        reply.snapshot.as_ref().expect("snapshot").nodes.clone(),
        "https://www.reddit.com/r/rust/",
    )
    .expect("projected doc0");
    let btn = doc0
        .nodes
        .iter()
        .position(|n| n.attr("id") == Some("upvote"))
        .expect("#upvote");
    let click_reply = realm.dispatch(SessionInput {
        kind: InputKind::Click { target: btn },
        edits: Vec::new(),
    });
    let doc1 = document::project_nodes(
        click_reply
            .snapshot
            .as_ref()
            .expect("snapshot")
            .nodes
            .clone(),
        "https://www.reddit.com/r/rust/",
    )
    .expect("projected doc1");
    assert_eq!(doc1.title, "hydrated=1|upvoted");
    let frame = render_doc(&doc1);
    assert!(frame.canvas.width > 0 && frame.canvas.height > 0);
}

#[test]
fn body_stylesheet_links_beyond_17_and_interactive_style_budget_work_without_widening_default() {
    let mut links = String::new();
    for i in 0..22 {
        if i == 10 {
            links.push_str("</head><body>");
        }
        links.push_str(&format!(
            r#"<link rel="stylesheet" href="https://github.githubassets.com/assets/chunk-{i}.css">"#
        ));
    }
    let html = format!("<!doctype html><html><head>{links}<h2 class=\"vh\">Nav</h2></body></html>");
    let mut doc = parse(&html, "https://github.com/rust-lang/rust");
    assert_eq!(doc.stylesheet_requests().len(), 17);
    let interactive_reqs = doc.stylesheet_requests_with_limit(65);
    assert_eq!(interactive_reqs.len(), 22);

    let three_mib_sheet = mg_sparkle::document::StylesheetSource {
        css: format!(
            "{} .vh {{ position: absolute; width: 1px; height: 1px; overflow: hidden; }}",
            " ".repeat(3 * 1024 * 1024)
        ),
        base_url: "https://github.githubassets.com/assets/marketing-header.css".into(),
        media: String::new(),
    };
    assert!(
        mg_sparkle::style::compute_styles(
            &doc,
            std::slice::from_ref(&three_mib_sheet),
            (1280.0, 800.0)
        )
        .is_err(),
        "default non-interactive style budget must still reject > 2 MiB"
    );
    doc.set_interactive_style_budget(true);
    let styles = mg_sparkle::style::compute_styles(&doc, &[three_mib_sheet], (1280.0, 800.0))
        .expect("8 MiB interactive style budget");
    assert!(!styles.is_empty());
}

#[test]
fn interactive_cross_origin_https_esm_deep_recursion_and_malformed_attr_keys_roundtrip() {
    use mg_butane::modern::ResourceProfile;
    use mg_sparkle::js_browser::{
        ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource,
    };
    use mg_sparkle::page_session::{observer::ObserverInit, platform::PlatformInit};

    // Malformed adjacent quote attribute `data-a="1"bad="2"` must not poison ArenaSnapshot projection.
    let html = r#"<!doctype html><html><body>
        <div id="probe" data-a="1"bad="2">hello</div>
        <script type="module" src="https://github.githubassets.com/assets/entry.js"></script>
    </body></html>"#;
    let parsed = parse(html, "https://github.com/rust-lang/rust");
    let script_node = parsed
        .nodes
        .iter()
        .position(|n| n.tag == "script")
        .expect("module script node");

    let make_req = || Request {
        url: "https://github.com/rust-lang/rust".into(),
        html: html.into(),
        external_scripts: Some(vec![
            ExternalScriptSource {
                node: script_node,
                source: r#"
                    import { version } from "https://github.githubassets.com/assets/dep.js";
                    function sumDepth(n) { return n <= 0 ? 0 : 1 + sumDepth(n - 1); }
                    document.title = version + ":" + sumDepth(180);
                "#
                .into(),
                provenance: Some(ClassicSourceProvenance {
                    response_url: "https://github.githubassets.com/assets/entry.js".into(),
                    error_visibility: ClassicErrorVisibility::Muted,
                }),
            },
            ExternalScriptSource {
                node: usize::MAX,
                source: r#"export const version = "gh-esm-ok";"#.into(),
                provenance: Some(ClassicSourceProvenance {
                    response_url: "https://github.githubassets.com/assets/dep.js".into(),
                    error_visibility: ClassicErrorVisibility::Muted,
                }),
            },
        ]),
    };
    let make_init = PlatformInit::default;

    // Default PageProcessV1 keeps same-origin module restriction and 64 recursion limit.
    let (default_realm, default_reply) = BoaPageRealm::start_with_platform_and_profile(
        make_req(),
        ObserverInit::Disabled,
        make_init(),
        [0x44; 32],
        ResourceProfile::PageProcessV1,
    );
    assert!(default_realm.is_none());
    assert!(
        default_reply.errors.iter().any(|e| {
            e.contains("Invalid or contradictory classic source provenance")
                || e.contains("Cross-origin module import is not allowed")
        }),
        "expected cross-origin rejection in PageProcessV1, got {:?}",
        default_reply.errors
    );

    // InteractivePageV1 admits HTTPS CORS-fetched modules, deep recursion (180 frames), and clean snapshot projection.
    let (interactive_realm, interactive_reply) = BoaPageRealm::start_with_platform_and_profile(
        make_req(),
        ObserverInit::Disabled,
        make_init(),
        [0x55; 32],
        ResourceProfile::InteractivePageV1,
    );
    assert!(interactive_realm.is_some());
    assert_eq!(
        interactive_reply.state,
        RealmState::Ready,
        "errors: {:?}",
        interactive_reply.errors
    );
    assert!(
        interactive_reply.errors.is_empty(),
        "unexpected errors: {:?}",
        interactive_reply.errors
    );
    let projected = document::project_nodes(
        interactive_reply
            .snapshot
            .as_ref()
            .expect("snapshot")
            .nodes
            .clone(),
        "https://github.com/rust-lang/rust",
    )
    .expect("clean ArenaSnapshot projection despite malformed HTML attr token");
    assert_eq!(projected.title, "gh-esm-ok:180");

    // Regex containing `{` must not mask subsequent static ES module imports.
    let regex_mod = r#"function f(){return /\{/;} import * as p from "./primer-react.js";"#;
    assert_eq!(
        mg_sparkle::js_browser::extract_static_module_specifiers(regex_mod),
        vec!["./primer-react.js".to_string()]
    );

    // InteractivePageV1 must allow AutomationRequest after > 4 MiB DOM allocation,
    // and expose `window.DocumentType` without throwing ReferenceError.
    let mut realm = interactive_realm.unwrap();
    let mut reply = interactive_reply;
    while let Some(wake) = reply.task_wake {
        reply = realm.pump_tasks(mg_sparkle::page_session::TaskPump {
            wake_token: wake.token,
        });
    }
    let prev_rev = reply.revision;
    let (auto_reply, auto_res) = realm.automation(
        mg_sparkle::page_session::automation::AutomationRequest {
            owner: 1,
            operation: mg_sparkle::page_session::automation::Operation::Evaluate {
                source: "const big = 'x'.repeat(5 * 1024 * 1024); const d = document.createElement('div'); d.textContent = big; document.body.appendChild(d); typeof DocumentType === 'function' && !(document instanceof DocumentType)".into(),
                return_by_value: true,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        },
    );
    assert_eq!(auto_reply.revision, prev_rev + 1);
    assert!(matches!(
        auto_res,
        mg_sparkle::page_session::automation::AutomationResult::Value(
            mg_sparkle::page_session::automation::RemoteValue::Boolean(true)
        )
    ));
    let (auto_reply2, auto_res2) =
        realm.automation(mg_sparkle::page_session::automation::AutomationRequest {
            owner: 1,
            operation: mg_sparkle::page_session::automation::Operation::Evaluate {
                source: "document.body.children.length > 0 && Intl.DateTimeFormat().resolvedOptions().timeZone === 'UTC' && new Intl.NumberFormat('en-US').format(42) === '42' && new Intl.PluralRules('en-US').select(1) === 'one' && Intl.Collator().compare('a', 'b') < 0 && new Intl.Locale('en-US').language === 'en'".into(),
                return_by_value: true,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
    assert_eq!(auto_reply2.revision, prev_rev + 2);
    assert!(matches!(
        auto_res2,
        mg_sparkle::page_session::automation::AutomationResult::Value(
            mg_sparkle::page_session::automation::RemoteValue::Boolean(true)
        )
    ));
}
