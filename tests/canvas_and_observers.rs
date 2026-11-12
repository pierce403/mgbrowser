//! Milestone 5 integration tests for HTML5 `<canvas>` 2D, ES `<script type="module">`,
//! DOM Observers (`MutationObserver`, `ResizeObserver`, `IntersectionObserver`),
//! Custom Elements v1 (`window.customElements`), Web Animations (`Element.animate`,
//! `getAnimations`), and `window.visualViewport`.

use mg_sparkle::{
    document, images,
    js_browser::{
        ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource, Request,
        boa::BoaPageRealm,
    },
    page_session::{
        LifecyclePhase, RealmState, SessionReply, TaskPump,
        observer::ObserverInit,
        platform::{
            PlatformCommand, PlatformInit, ScriptFetchResponse, ViewportState, base64_encode,
        },
    },
};

fn pump_until_settled(realm: &mut BoaPageRealm, mut reply: SessionReply) -> SessionReply {
    let mut errors = Vec::new();
    let mut fetch_requests = Vec::new();
    for _ in 0..64 {
        errors.append(&mut reply.errors);
        fetch_requests.append(&mut reply.fetch_requests);
        if reply.state != RealmState::Ready || reply.navigation.is_some() {
            break;
        }
        let Some(wake) = reply.task_wake.clone() else {
            break;
        };
        if wake.delay_ms > 50 {
            break;
        }
        reply = realm.pump_tasks(TaskPump {
            wake_token: wake.token,
        });
    }
    errors.append(&mut reply.errors);
    fetch_requests.append(&mut reply.fetch_requests);
    reply.errors = errors;
    reply.fetch_requests = fetch_requests;
    reply
}

fn run_platform_page(
    url: &str,
    html: String,
    external_scripts: Option<Vec<ExternalScriptSource>>,
) -> (BoaPageRealm, SessionReply) {
    let (realm_opt, reply) = BoaPageRealm::start_with_platform(
        Request {
            url: url.into(),
            html,
            external_scripts,
        },
        ObserverInit::Disabled,
        PlatformInit::default(),
        [7u8; 32],
    );
    let mut realm = realm_opt.expect("BoaPageRealm started");
    let settled = pump_until_settled(&mut realm, reply);
    (realm, settled)
}

fn projected(reply: &SessionReply, url: &str) -> document::Document {
    let mut doc = document::project_nodes(
        reply.snapshot.as_ref().expect("snapshot").nodes.clone(),
        url,
    )
    .expect("project_nodes");
    doc.materialize_inline_canvas_resources();
    doc
}

fn node_text(doc: &document::Document, id: usize) -> String {
    let mut out = String::new();
    let mut stack = vec![id];
    while let Some(cur) = stack.pop() {
        let node = &doc.nodes[cur];
        if node.tag == "#text" {
            out.push_str(&node.text);
        } else {
            stack.extend(node.children.iter().rev().copied());
        }
    }
    out
}

#[test]
fn canvas_2d_default_dimensions_context_identity_and_reset() {
    let html = r#"<!doctype html><html><body>
        <canvas id="c"></canvas>
        <p id="out"></p>
        <script>
            const c = document.getElementById('c');
            const initial = `${c.width}x${c.height}`;
            const ctx1 = c.getContext('2d');
            const ctx2 = c.getContext('2d');
            const same2d = ctx1 === ctx2 && ctx1.canvas === c && c instanceof HTMLCanvasElement;
            const webgl = c.getContext('webgl');
            const unknown = c.getContext('experimental-unknown');
            ctx1.fillStyle = '#ff0000';
            ctx1.fillRect(0, 0, 10, 10);
            const beforeReset = Array.from(ctx1.getImageData(2, 2, 1, 1).data).join(',');
            c.width = 64;
            c.height = 48;
            const afterReset = Array.from(ctx1.getImageData(2, 2, 1, 1).data).join(',');
            const attrs = `${c.getAttribute('width')}x${c.getAttribute('height')}`;
            document.getElementById('out').textContent =
                `${initial}|${same2d}|${webgl}|${unknown}|${beforeReset}|${afterReset}|${attrs}`;
        </script>
    </body></html>"#;

    let (_realm, reply) = run_platform_page("https://example.test/canvas", html.into(), None);
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);
    let doc = projected(&reply, "https://example.test/canvas");
    let out = doc.query_selector(0, "#out").unwrap().unwrap();
    assert_eq!(
        node_text(&doc, out),
        "300x150|true|null|null|255,0,0,255|0,0,0,0|64x48"
    );
}

#[test]
fn canvas_2d_shapes_gradients_transforms_paths_clip_dashes_and_imagedata() {
    let html = r#"<!doctype html><html><body>
        <canvas id="c" width="120" height="80"></canvas>
        <p id="out"></p>
        <script>
            const c = document.getElementById('c');
            const ctx = c.getContext('2d');

            // Save/restore + transform matrix inspection
            ctx.save();
            ctx.translate(10, 20);
            ctx.scale(2, 3);
            const m = ctx.getTransform();
            const matOk = m.a === 2 && m.d === 3 && m.e === 10 && m.f === 20;
            ctx.restore();
            const mReset = ctx.getTransform();
            const restoreOk = mReset.a === 1 && mReset.d === 1 && mReset.e === 0 && mReset.f === 0;

            // Linear gradient fillRect
            const grad = ctx.createLinearGradient(0, 0, 40, 0);
            grad.addColorStop(0, '#ff0000');
            grad.addColorStop(1, '#0000ff');
            ctx.fillStyle = grad;
            ctx.fillRect(0, 0, 40, 20);
            const leftPx = ctx.getImageData(1, 10, 1, 1).data;
            const rightPx = ctx.getImageData(38, 10, 1, 1).data;
            const gradOk = leftPx[0] > 200 && leftPx[2] < 40 && rightPx[2] > 200 && rightPx[0] < 40;

            // Radial gradient
            const rgrad = ctx.createRadialGradient(60, 20, 1, 60, 20, 15);
            rgrad.addColorStop(0, 'rgba(0, 255, 0, 1)');
            rgrad.addColorStop(1, 'rgba(0, 0, 0, 1)');
            ctx.fillStyle = rgrad;
            ctx.fillRect(45, 5, 30, 30);
            const centerPx = ctx.getImageData(60, 20, 1, 1).data;
            const rgradOk = centerPx[1] > 200 && centerPx[3] === 255;

            // ClearRect
            ctx.clearRect(5, 5, 6, 6);
            const cleared = ctx.getImageData(7, 7, 1, 1).data[3] === 0;

            // Clip + path + curves + roundRect + isPointInPath + lineDash
            ctx.save();
            ctx.beginPath();
            ctx.rect(0, 40, 50, 35);
            const inRect = ctx.isPointInPath(25, 55) && !ctx.isPointInPath(90, 55);
            ctx.clip();
            ctx.fillStyle = 'rgb(20, 180, 90)';
            ctx.beginPath();
            ctx.moveTo(0, 40);
            ctx.lineTo(80, 40);
            ctx.quadraticCurveTo(85, 60, 80, 75);
            ctx.bezierCurveTo(60, 78, 20, 78, 0, 75);
            ctx.closePath();
            ctx.fill();
            ctx.restore();
            const insideClip = ctx.getImageData(25, 55, 1, 1).data;
            const outsideClip = ctx.getImageData(65, 55, 1, 1).data;
            const clipOk = insideClip[1] === 180 && outsideClip[3] === 0;

            ctx.setLineDash([4, 2]);
            const dashOk = ctx.getLineDash().join(',') === '4,2';
            ctx.strokeStyle = '#ffcc00';
            ctx.lineWidth = 2;
            ctx.beginPath();
            ctx.roundRect(75, 45, 30, 20, 4);
            ctx.arc(90, 55, 6, 0, Math.PI * 2);
            ctx.ellipse(90, 55, 8, 4, 0, 0, Math.PI);
            ctx.stroke();

            // Explicit createImageData + putImageData roundtrip
            const img = ctx.createImageData(2, 2);
            img.data.set([
                12, 34, 56, 255,
                78, 90, 123, 255,
                200, 150, 100, 255,
                1, 2, 3, 128
            ]);
            ctx.putImageData(img, 100, 10);
            const readBack = Array.from(ctx.getImageData(100, 10, 2, 1).data).join(',');

            document.getElementById('out').textContent =
                `${matOk}|${restoreOk}|${gradOk}|${rgradOk}|${cleared}|${inRect}|${clipOk}|${dashOk}|${readBack}`;
        </script>
    </body></html>"#;

    let (_realm, reply) = run_platform_page("https://example.test/canvas2", html.into(), None);
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);
    let doc = projected(&reply, "https://example.test/canvas2");
    let out = doc.query_selector(0, "#out").unwrap().unwrap();
    assert_eq!(
        node_text(&doc, out),
        "true|true|true|true|true|true|true|true|12,34,56,255,78,90,123,255"
    );
}

#[test]
fn canvas_2d_text_drawimage_todataurl_toblob_and_live_dom_projection() {
    let html = r#"<!doctype html><html><body>
        <canvas id="c" width="48" height="32"></canvas>
        <p id="out"></p>
        <script>
            const off = document.createElement('canvas');
            off.width = 16;
            off.height = 16;
            const octx = off.getContext('2d');
            octx.fillStyle = '#00aa55';
            octx.fillRect(0, 0, 16, 16);

            const c = document.getElementById('c');
            const ctx = c.getContext('2d');
            ctx.drawImage(off, 4, 4);
            ctx.font = '16px sans-serif';
            const measured = ctx.measureText('MG').width;
            ctx.fillStyle = '#ffffff';
            ctx.fillText('MG', 22, 20);

            const dataUrl = c.toDataURL('image/png');
            const hiddenAttr = c.getAttribute('__mg_canvas_src') === null
                && !c.hasAttribute('__mg_canvas_src')
                && !c.getAttributeNames().includes('__mg_canvas_src');

            c.toBlob(blob => {
                document.getElementById('out').textContent =
                    `${measured > 0}|${dataUrl.startsWith('data:image/png;base64,')}|${hiddenAttr}|${blob !== null && blob.type === 'image/png' && blob.size > 40}`;
            }, 'image/png');
        </script>
    </body></html>"#;

    let (_realm, reply) = run_platform_page("https://example.test/canvas3", html.into(), None);
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);
    let doc = projected(&reply, "https://example.test/canvas3");
    let out = doc.query_selector(0, "#out").unwrap().unwrap();
    assert_eq!(node_text(&doc, out), "true|true|true|true");

    // Verify live DOM projection materialized a decodable PNG resource for <canvas>!
    let canvas_id = doc.query_selector(0, "#c").unwrap().unwrap();
    let img_ref = doc
        .effective_image_reference(canvas_id, 1024.0, 1.0)
        .expect("canvas effective_image_reference");
    let res = doc
        .resources
        .get(&img_ref)
        .expect("inline canvas resource materialized");
    let decoded = images::decode(&res.bytes, &res.content_type, None, None).expect("valid PNG");
    assert_eq!((decoded.width, decoded.height), (48, 32));
    // Pixel (8, 8) came from drawImage(off, 4, 4) filled with #00aa55
    let idx = ((8 * 48 + 8) * 4) as usize;
    assert_eq!(&decoded.pixels[idx..idx + 4], &[0x00, 0xaa, 0x55, 0xff]);
}

#[test]
fn es_modules_deferred_order_nomodule_importmap_live_bindings_and_dynamic_import() {
    let html = r#"<!doctype html><html><head>
        <script type="importmap">
            { "imports": { "pkg/counter": "https://example.test/js/counter.js" } }
        </script>
        <script type="module">
            import { count, bump } from "pkg/counter";
            window.log.push(`mod1:before:${count}:${import.meta.url}`);
            await Promise.resolve();
            bump();
            window.log.push(`mod1:after:${count}`);
        </script>
        <script nomodule>
            window.log = window.log || [];
            window.log.push("nomodule-ran");
        </script>
    </head><body>
        <p id="out"></p>
        <script>
            window.log = ["classic"];
            document.addEventListener("DOMContentLoaded", () => {
                window.log.push("dcl");
            });
        </script>
        <script type="module" src="/js/app.js"></script>
    </body></html>"#;

    let parsed = document::parse_with_scripting(html, "https://example.test/page.html", true);
    let app_script_node = parsed
        .nodes
        .iter()
        .position(|n| n.tag == "script" && n.attr("src") == Some("/js/app.js"))
        .expect("app.js script node");

    let external_sources = vec![
        ExternalScriptSource {
            node: app_script_node,
            provenance: Some(ClassicSourceProvenance {
                response_url: "https://example.test/js/app.js".into(),
                error_visibility: ClassicErrorVisibility::SameOrigin,
            }),
            source: r#"
                import { count, bump } from "pkg/counter";
                bump();
                window.log.push(`mod2:${count}:evals=${window.counterEval}`);
                import("./dynamic.js").then(ns => {
                    document.getElementById("out").textContent =
                        `${window.log.join("|")}|dyn=${ns.answer}`;
                });
            "#
            .into(),
        },
        ExternalScriptSource {
            node: usize::MAX,
            provenance: Some(ClassicSourceProvenance {
                response_url: "https://example.test/js/counter.js".into(),
                error_visibility: ClassicErrorVisibility::SameOrigin,
            }),
            source: "export let count = 1;\nexport function bump() { count += 4; }\nwindow.counterEval = (window.counterEval || 0) + 1;\n".into(),
        },
    ];

    let (mut realm, mut reply) = run_platform_page(
        "https://example.test/page.html",
        html.into(),
        Some(external_sources),
    );
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);

    // Fulfill dynamic `import("./dynamic.js")` FetchRequest
    let fetch_req = reply
        .fetch_requests
        .first()
        .cloned()
        .expect("dynamic import issued FetchRequest");
    assert_eq!(fetch_req.url, "https://example.test/js/dynamic.js");

    let dyn_src = b"export const answer = 42;\n";
    reply = realm.platform_command(PlatformCommand::FetchComplete {
        response: ScriptFetchResponse {
            request_id: fetch_req.request_id,
            ok_transport: true,
            error: None,
            status: 200,
            status_text: "OK".into(),
            final_url: fetch_req.url,
            headers: vec![("content-type".into(), "text/javascript".into())],
            body_b64: base64_encode(dyn_src),
            updated_cookies: None,
        },
    });
    reply = pump_until_settled(&mut realm, reply);
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);

    let doc = projected(&reply, "https://example.test/page.html");
    let out = doc.query_selector(0, "#out").unwrap().unwrap();
    assert_eq!(
        node_text(&doc, out),
        "classic|mod1:before:1:https://example.test/page.html|mod1:after:5|mod2:9:evals=1|dcl|dyn=42"
    );

    // Classic-only `BoaPageRealm::start` (without platform) still executes `<script nomodule>` fallback!
    let (_classic_realm, classic_reply) = BoaPageRealm::start(Request {
        url: "https://example.test/classic.html".into(),
        html: r#"<html><body><p id="out">none</p>
            <script type="module">document.getElementById('out').textContent = 'module';</script>
            <script nomodule>document.getElementById('out').textContent = 'nomodule';</script>
        </body></html>"#
            .into(),
        external_scripts: None,
    });
    let classic_doc = projected(&classic_reply, "https://example.test/classic.html");
    let classic_out = classic_doc.query_selector(0, "#out").unwrap().unwrap();
    assert_eq!(node_text(&classic_doc, classic_out), "nomodule");
}

#[test]
fn mutation_observer_childlist_attributes_characterdata_subtree_takerecords_and_disconnect() {
    let html = r#"<!doctype html><html><body>
        <div id="box" data-state="init" title="keep"><span id="txt">hello</span></div>
        <p id="out"></p>
        <script>
            const box = document.getElementById('box');
            const txt = document.getElementById('txt');
            const events = [];
            const mo = new MutationObserver((records, obs) => {
                if (obs === mo) {
                    for (const r of records) {
                        events.push(`${r.type}:${r.attributeName || ''}:${r.oldValue || ''}:+${r.addedNodes.length}:-${r.removedNodes.length}`);
                    }
                }
            });
            mo.observe(box, {
                childList: true,
                attributes: true,
                attributeOldValue: true,
                attributeFilter: ['data-state'],
                characterData: true,
                characterDataOldValue: true,
                subtree: true,
            });

            box.setAttribute('data-state', 'active');
            box.setAttribute('title', 'ignored-by-filter');
            txt.firstChild.data = 'world';
            const child = document.createElement('em');
            box.appendChild(child);

            Promise.resolve().then(() => {
                // Synchronous takeRecords + disconnect check
                box.setAttribute('data-state', 'taken');
                const taken = mo.takeRecords();
                mo.disconnect();
                box.setAttribute('data-state', 'after-disconnect');
                Promise.resolve().then(() => {
                    document.getElementById('out').textContent =
                        `${events.join('|')}||taken=${taken.length}:${taken[0].oldValue}`;
                });
            });
        </script>
    </body></html>"#;

    let (_realm, reply) = run_platform_page("https://example.test/mo", html.into(), None);
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);
    let doc = projected(&reply, "https://example.test/mo");
    let out = doc.query_selector(0, "#out").unwrap().unwrap();
    assert_eq!(
        node_text(&doc, out),
        "attributes:data-state:init:+0:-0|characterData::hello:+0:-0|childList:::+1:-0||taken=1:active"
    );
}

#[test]
fn resize_observer_and_intersection_observer_geometry_and_scroll_transitions() {
    let html = r#"<!doctype html><html><body style="margin:0">
        <div id="sized" style="width:120px;height:45px"></div>
        <div id="near" style="width:80px;height:40px"></div>
        <div id="far" style="position:absolute;top:2000px;left:10px;width:100px;height:60px"></div>
        <p id="out"></p>
        <script>
            const sized = document.getElementById('sized');
            const near = document.getElementById('near');
            const far = document.getElementById('far');
            const roSizes = [];
            const ioLog = [];

            const ro = new ResizeObserver(entries => {
                for (const e of entries) {
                    roSizes.push(`${Math.round(e.contentRect.width)}x${Math.round(e.contentRect.height)}:${Math.round(e.borderBoxSize[0].inlineSize)}`);
                }
                if (roSizes.length === 1) {
                    sized.style.width = '240px';
                } else {
                    ro.disconnect();
                }
            });
            ro.observe(sized);

            const io = new IntersectionObserver(entries => {
                for (const e of entries) {
                    ioLog.push(`${e.target.id}:${e.isIntersecting}:${e.intersectionRatio > 0}`);
                }
                if (ioLog.length === 2) {
                    window.scrollTo(0, 1950);
                } else if (ioLog.length >= 3) {
                    io.disconnect();
                    document.getElementById('out').textContent =
                        `ro=${roSizes.join(',')}|io=${ioLog.join(',')}`;
                }
            }, { threshold: [0, 0.5] });
            io.observe(near);
            io.observe(far);
        </script>
    </body></html>"#;

    let (_realm, reply) = run_platform_page("https://example.test/ro-io", html.into(), None);
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);
    let doc = projected(&reply, "https://example.test/ro-io");
    let out = doc.query_selector(0, "#out").unwrap().unwrap();
    assert_eq!(
        node_text(&doc, out),
        "ro=120x45:120,240x45:240|io=near:true:true,far:false:false,near:false:false,far:true:true"
    );
}

#[test]
fn custom_elements_v1_upgrade_lifecycle_constructor_attributes_and_instanceof() {
    let html = r#"<!doctype html><html><body>
        <x-counter id="pre" step="2"></x-counter>
        <div id="plain"></div>
        <p id="out"></p>
        <script>
            const log = [];
            let definedResolved = false;
            customElements.whenDefined('x-counter').then(ctor => {
                definedResolved = (ctor === XCounter);
                updateOut();
            });

            class XCounter extends HTMLElement {
                static get observedAttributes() { return ['step']; }
                constructor() {
                    super();
                    this._count = 10;
                    log.push(`ctor:${this.id || 'new'}`);
                }
                connectedCallback() {
                    log.push(`conn:${this.id}`);
                }
                disconnectedCallback() {
                    log.push(`disc:${this.id}`);
                }
                attributeChangedCallback(name, oldVal, newVal) {
                    log.push(`attr:${name}:${oldVal}->${newVal}`);
                }
                get count() { return this._count; }
                set count(v) { this._count = Number(v); }
                bump() {
                    const step = Number(this.getAttribute('step') || '1');
                    this._count += step;
                    return this._count;
                }
            }

            customElements.define('x-counter', XCounter);
            const pre = document.getElementById('pre');
            const plain = document.getElementById('plain');
            const created = document.createElement('x-counter');
            created.id = 'created';
            document.body.appendChild(created);
            const constructed = new XCounter();
            constructed.id = 'constructed';

            const bumped = pre.bump();
            pre.count = 25;
            pre.setAttribute('step', '5');
            created.remove();

            const checks = [
                pre instanceof XCounter,
                pre instanceof HTMLElement,
                constructed instanceof XCounter,
                !(plain instanceof XCounter),
                customElements.get('x-counter') === XCounter,
                customElements.getName(XCounter) === 'x-counter',
                bumped === 12,
                pre.count === 25,
            ].every(Boolean);

            function updateOut() {
                document.getElementById('out').textContent =
                    `${checks}|${definedResolved}|${log.join(',')}`;
            }
            updateOut();
        </script>
    </body></html>"#;

    let (_realm, reply) = run_platform_page("https://example.test/ce", html.into(), None);
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);
    let doc = projected(&reply, "https://example.test/ce");
    let out = doc.query_selector(0, "#out").unwrap().unwrap();
    assert_eq!(
        node_text(&doc, out),
        "true|true|ctor:pre,attr:step:null->2,conn:pre,ctor:new,conn:created,ctor:new,attr:step:2->5,disc:created"
    );
}

#[test]
fn web_animations_element_animate_getanimations_promises_events_and_visual_viewport() {
    let html = r#"<!doctype html><html><body>
        <div id="box" style="width:20px"></div>
        <p id="out"></p>
        <script>
            const box = document.getElementById('box');
            const events = [];

            const anim = box.animate(
                [
                    { width: '20px', opacity: '0.2' },
                    { width: '180px', opacity: '0.9' }
                ],
                { duration: 250, fill: 'forwards', id: 'grow' }
            );
            anim.onfinish = e => events.push(`finish:${e.currentTime}`);
            const listedBefore = box.getAnimations().length === 1 && document.getAnimations().length === 1;
            const filledWidth = box.style.width;
            const filledOpacity = box.style.opacity;

            const anim2 = box.animate({ height: ['10px', '60px'] }, { duration: 100, fill: 'forwards' });
            anim2.oncancel = () => events.push('cancel2');
            const heightBeforeCancel = box.style.height;
            anim2.cancel();
            const heightAfterCancel = box.style.height;

            const vvEvents = [];
            window.visualViewport.addEventListener('resize', () => {
                vvEvents.push(`resize:${window.visualViewport.width}x${window.visualViewport.height}@${window.visualViewport.scale}:dpr=${window.devicePixelRatio}`);
                render();
            });
            window.visualViewport.addEventListener('scroll', () => {
                vvEvents.push(`scroll:${window.visualViewport.pageTop}`);
                render();
            });

            let finishedResolved = false;
            anim.finished.then(a => {
                finishedResolved = (a === anim && a.id === 'grow');
                render();
            });

            function render() {
                document.getElementById('out').textContent =
                    `${listedBefore}|${filledWidth}|${filledOpacity}|${heightBeforeCancel}->${heightAfterCancel}|${finishedResolved}|${events.join(',')}|${vvEvents.join(',')}`;
            }
            render();
        </script>
    </body></html>"#;

    let (mut realm, mut reply) = run_platform_page("https://example.test/waapi", html.into(), None);
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);
    assert_eq!(reply.lifecycle.unwrap().phase, LifecyclePhase::LoadComplete);

    // Sync visualViewport resize + scroll from platform host
    reply = realm.platform_command(PlatformCommand::ViewportSync {
        viewport: ViewportState {
            inner_width: 1024,
            inner_height: 640,
            scroll_x: 0.0,
            scroll_y: 120.0,
            device_pixel_ratio: 1.5,
        },
    });
    reply = pump_until_settled(&mut realm, reply);
    assert!(reply.errors.is_empty(), "errors: {:?}", reply.errors);

    let doc = projected(&reply, "https://example.test/waapi");
    let out = doc.query_selector(0, "#out").unwrap().unwrap();
    assert_eq!(
        node_text(&doc, out),
        "true|180px|0.9|60px->|true|cancel2,finish:250|resize:1024x640@1:dpr=1.5,scroll:120"
    );
}
