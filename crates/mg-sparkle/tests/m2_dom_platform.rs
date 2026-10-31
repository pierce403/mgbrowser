#![cfg(feature = "modern")]
//! Milestone 2 DOM, CSS3 Selectors, Events, Interactive Forms, Geometry, and Web Platform APIs.
use mg_sparkle::{
    document::{self, Document, Item},
    js_browser::{Request, boa::BoaPageRealm},
    page_session::{
        InputKind, RealmState, SessionInput, SessionReply, TaskPump,
        platform::{PlatformCommand, PlatformInit, ViewportState},
    },
};

const PAGE_URL: &str = "http://127.0.0.1:7878/app/index.html";

fn start_with_seed(
    html: &str,
    platform: PlatformInit,
    seed: [u8; 32],
) -> (BoaPageRealm, SessionReply) {
    let (realm, reply) = BoaPageRealm::start_with_platform(
        Request {
            external_scripts: None,
            url: PAGE_URL.into(),
            html: html.into(),
        },
        mg_sparkle::page_session::observer::ObserverInit::Disabled,
        platform,
        seed,
    );
    assert_eq!(
        reply.state,
        RealmState::Ready,
        "init errors: {:?}",
        reply.errors
    );
    assert!(reply.errors.is_empty(), "init errors: {:?}", reply.errors);
    (realm.expect("live BoaPageRealm"), reply)
}

fn start(html: &str) -> (BoaPageRealm, SessionReply) {
    start_with_seed(html, PlatformInit::default(), [0x42; 32])
}

fn drain_zero_delay_tasks(realm: &mut BoaPageRealm, mut reply: SessionReply) -> SessionReply {
    for _ in 0..16 {
        let Some(wake) = reply.task_wake.as_ref().filter(|w| w.delay_ms == 0) else {
            break;
        };
        reply = realm.pump_tasks(TaskPump {
            wake_token: wake.token,
        });
        assert_eq!(
            reply.state,
            RealmState::Ready,
            "task errors: {:?}",
            reply.errors
        );
        assert!(reply.errors.is_empty(), "task errors: {:?}", reply.errors);
    }
    reply
}

fn project(reply: &SessionReply) -> Document {
    document::project_nodes(
        reply.snapshot.as_ref().expect("snapshot").nodes.clone(),
        PAGE_URL,
    )
    .expect("valid projected document")
}

fn node(reply: &SessionReply, selector: &str) -> usize {
    project(reply)
        .query_selector(0, selector)
        .unwrap()
        .unwrap_or_else(|| panic!("missing selector {selector}"))
}

fn text_of(reply: &SessionReply, selector: &str) -> String {
    let doc = project(reply);
    let root = doc
        .query_selector(0, selector)
        .unwrap()
        .unwrap_or_else(|| panic!("missing selector {selector}"));
    let mut stack = vec![root];
    let mut out = String::new();
    while let Some(id) = stack.pop() {
        if doc.nodes[id].tag == "#text" {
            out.push_str(&doc.nodes[id].text);
        }
        stack.extend(doc.nodes[id].children.iter().rev().copied());
    }
    out
}

fn click(realm: &mut BoaPageRealm, target: usize) -> SessionReply {
    let r = realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    });
    assert_eq!(r.state, RealmState::Ready, "click errors: {:?}", r.errors);
    assert!(r.errors.is_empty(), "click errors: {:?}", r.errors);
    r
}

#[test]
fn standalone_and_form_rich_controls_projection_and_activation() {
    let html = r#"<!doctype html>
    <html><body>
      <input id="solo-text" value="standalone">
      <input id="solo-chk" type="checkbox">
      <select id="solo-sel">
        <optgroup label="Group A">
          <option value="a1">One</option>
          <option value="a2" selected>Two</option>
        </optgroup>
        <option value="b1" disabled>Disabled</option>
      </select>
      <textarea id="solo-ta">line1</textarea>
      <button id="solo-btn" type="button">Action</button>

      <form id="f" action="/submit">
        <label id="lbl-c1" for="c1">Accept</label>
        <input id="c1" type="checkbox" name="accept" value="yes">
        <input id="c-block" type="checkbox" name="blocked" checked>
        <input id="r1" type="radio" name="plan" value="free" checked>
        <input id="r2" type="radio" name="plan" value="pro">
        <input id="txt" name="note" value="initial">
        <button id="rst" type="reset">Reset</button>
      </form>
      <p id="out"></p>
      <script>
        const log = [];
        const f = document.getElementById('f');
        const c1 = document.getElementById('c1');
        const cBlock = document.getElementById('c-block');
        const r1 = document.getElementById('r1');
        const r2 = document.getElementById('r2');
        const sel = document.getElementById('solo-sel');
        const txt = document.getElementById('txt');

        c1.addEventListener('click', () => log.push('c1-click:' + c1.checked));
        c1.addEventListener('input', () => log.push('c1-input:' + c1.checked));
        c1.addEventListener('change', () => log.push('c1-change:' + c1.checked));

        cBlock.addEventListener('click', (e) => {
          log.push('cb-during:' + cBlock.checked);
          e.preventDefault();
        });
        cBlock.addEventListener('change', () => log.push('cb-change-MUST-NOT-FIRE'));

        r2.addEventListener('change', () => log.push('r2-change:' + r1.checked + ',' + r2.checked));
        sel.addEventListener('input', () => log.push('sel-input:' + sel.value + ':' + sel.selectedIndex));
        sel.addEventListener('change', () => log.push('sel-change:' + sel.value));

        document.getElementById('solo-btn').addEventListener('click', () => {
          txt.value = 'dirty';
          const els = f.elements.length;
          f.reset();
          log.push('after-reset:' + txt.value + ':c1=' + c1.checked + ':r1=' + r1.checked + ':r2=' + r2.checked + ':els=' + els);
        });

        function render() {
          document.getElementById('out').textContent = log.join('|');
        }
        window.addEventListener('click', render);
        window.addEventListener('change', render);
      </script>
    </body></html>"#;

    let (mut realm, init) = start(html);
    let init = drain_zero_delay_tasks(&mut realm, init);
    let doc = project(&init);
    // Verify standalone controls outside <form> are projected into Document.items!
    assert!(
        doc.items
            .iter()
            .any(|it| matches!(it, Item::Input { form: None, value, .. } if value == "standalone"))
    );
    assert!(
        doc.items
            .iter()
            .any(|it| matches!(it, Item::Toggle { form: None, .. }))
    );
    assert!(doc.items.iter().any(|it| matches!(
        it,
        Item::Select {
            form: None,
            selected: Some(1),
            ..
        }
    )));
    assert!(
        doc.items
            .iter()
            .any(|it| matches!(it, Item::Button { form: None, .. }))
    );

    // 1. Click <label for="c1"> -> forwards activation to #c1!
    let lbl = node(&init, "#lbl-c1");
    let r = click(&mut realm, lbl);
    assert_eq!(
        text_of(&r, "#out"),
        "c1-click:true|c1-input:true|c1-change:true"
    );

    // 2. Click #c-block -> pre-activation flips to false during click, preventDefault reverts to true!
    let c_block = node(&r, "#c-block");
    let r = click(&mut realm, c_block);
    let doc = project(&r);
    assert!(
        doc.nodes[c_block].has("checked"),
        "preventDefault must revert checkbox state"
    );
    assert!(text_of(&r, "#out").ends_with("cb-during:false"));

    // 3. Click radio #r2 -> unchecks #r1 and checks #r2!
    let r2 = node(&r, "#r2");
    let r = click(&mut realm, r2);
    assert!(text_of(&r, "#out").contains("r2-change:false,true"));

    // 4. PlatformCommand::SelectOption on #solo-sel -> selects index 0 ("a1") and fires input + change!
    let sel = node(&r, "#solo-sel");
    let r = realm.platform_command(PlatformCommand::SelectOption {
        select_node: sel,
        selected_index: 0,
    });
    assert_eq!(r.state, RealmState::Ready, "{:?}", r.errors);
    assert!(text_of(&r, "#out").contains("sel-input:a1:0|sel-change:a1"));

    // 5. Click #solo-btn -> modifies txt.value, checks f.elements.length, calls f.reset()!
    let btn = node(&r, "#solo-btn");
    let r = click(&mut realm, btn);
    assert!(
        text_of(&r, "#out").contains("after-reset:initial:c1=false:r1=true:r2=false:els=6"),
        "got: {}",
        text_of(&r, "#out")
    );
}

#[test]
fn details_summary_dialog_and_arbitrary_div_onclick() {
    let html = r#"<!doctype html>
    <html><body>
      <details id="det"><summary id="sum">More</summary><p>Secret</p></details>
      <dialog id="dlg"><p>Modal</p></dialog>
      <div id="card" onclick="window.__cardClicked = (window.__cardClicked || 0) + 1">Card</div>
      <button id="run" type="button">Run</button>
      <p id="out"></p>
      <script>
        const det = document.getElementById('det');
        const dlg = document.getElementById('dlg');
        const events = [];
        det.addEventListener('toggle', () => events.push('toggle:' + det.open));
        dlg.addEventListener('close', () => events.push('dlg-close:' + dlg.returnValue + ':' + dlg.open));
        document.getElementById('card').addEventListener('click', () => {
          document.getElementById('out').textContent = 'card:' + window.__cardClicked;
        });
        document.getElementById('run').addEventListener('click', () => {
          dlg.showModal();
          const wasOpen = dlg.open;
          let dupErr = '';
          try { dlg.showModal(); } catch (e) { dupErr = e.name; }
          dlg.close('confirmed');
          document.getElementById('out').textContent =
            [wasOpen, dupErr, dlg.returnValue, events.join(',')].join('|');
        });
      </script>
    </body></html>"#;

    let (mut realm, init) = start(html);
    let init = drain_zero_delay_tasks(&mut realm, init);

    // Click arbitrary <div id="card">
    let card = node(&init, "#card");
    let r = click(&mut realm, card);
    assert_eq!(text_of(&r, "#out"), "card:1");

    // Click <summary> -> opens <details> and queues async toggle task!
    let sum = node(&r, "#sum");
    let r = click(&mut realm, sum);
    let det = node(&r, "#det");
    assert!(project(&r).nodes[det].has("open"));
    let r = drain_zero_delay_tasks(&mut realm, r);

    // Click #run -> exercises dialog.showModal(), InvalidStateError on duplicate showModal, dialog.close('confirmed')!
    let run = node(&r, "#run");
    let r = click(&mut realm, run);
    let r = drain_zero_delay_tasks(&mut realm, r);
    assert_eq!(
        text_of(&r, "#out"),
        "true|InvalidStateError|confirmed|toggle:true,dlg-close:confirmed:false"
    );
}

#[test]
fn stylo_selectors_dom_manipulation_template_domparser_and_css() {
    let html = r#"<!doctype html>
    <html><body>
      <div id="heroBanner" class="box primary" data-role="nav-main">
        <ul id="list"><li class="item">A</li><li class="item active">B</li><li class="item">C</li></ul>
      </div>
      <template id="tpl"><span class="badge">NEW</span></template>
      <div id="mount"></div>
      <p id="out"></p>
      <script>
        const checks = [];
        // 1. Complex CSS3 selectors via Stylo
        const second = document.querySelector('#list > li.item:nth-child(2):not(.disabled)');
        const afterActive = document.querySelector('li.active + li');
        const allNotActive = document.querySelectorAll('#list li:not(.active)');
        checks.push('sel:' + second.textContent + ',' + afterActive.textContent + ',' + allNotActive.length);
        checks.push('match:' + second.matches('[data-role="nav-main"] .active') + ',' + second.closest('[data-role^="nav"]').id);

        // 2. WindowProperties named access
        checks.push('winprop:' + (window.heroBanner === document.getElementById('heroBanner')));

        // 3. DocumentFragment, cloneNode, prepend/append/before/after/replaceWith/replaceChildren
        const mount = document.getElementById('mount');
        const frag = document.createDocumentFragment();
        const p1 = document.createElement('b'); p1.textContent = '1';
        const p2 = document.createElement('i'); p2.textContent = '2';
        frag.append(p1, p2);
        mount.appendChild(frag);
        p1.before('0');
        p2.after('3');
        const clone = mount.cloneNode(true);
        checks.push('tree:' + mount.textContent + ':clone=' + clone.textContent + ':contains=' + mount.contains(p2));

        // 4. outerHTML & insertAdjacentHTML
        p2.outerHTML = '<u>TWO</u>';
        mount.insertAdjacentHTML('beforeend', '<em>!</em>');
        checks.push('html:' + mount.innerHTML);

        // 5. <template>.content & DOMParser & CSS.supports / CSS.escape
        const tpl = document.getElementById('tpl');
        const tplClone = tpl.content.cloneNode(true);
        mount.prepend(tplClone);
        const parsed = new DOMParser().parseFromString('<section><h2 id="p-h">Title</h2></section>', 'text/html');
        checks.push('parsed:' + parsed.querySelector('#p-h').textContent + ':badge=' + mount.querySelector('.badge').textContent);
        checks.push('css:' + CSS.supports('display', 'flex') + ',' + CSS.supports('display: not-a-display-value') + ',' + CSS.escape('a.b'));

        document.getElementById('out').textContent = checks.join(' | ');
      </script>
    </body></html>"#;

    let (mut realm, init) = start(html);
    let r = drain_zero_delay_tasks(&mut realm, init);
    assert_eq!(
        text_of(&r, "#out"),
        "sel:B,C,2 | match:true,heroBanner | winprop:true | tree:0123:clone:0123:contains=true | html:0<b>1</b><u>TWO</u>3<em>!</em> | parsed:Title:badge=NEW | css:true,false,a\\.b"
            .replace("clone:0123", "clone=0123")
    );
}

#[test]
fn modern_events_capture_once_passive_abort_signal_and_constructors() {
    let html = r#"<!doctype html>
    <html><body>
      <div id="outer"><button id="inner" type="button">Hit</button></div>
      <input id="inp">
      <p id="out"></p>
      <script>
        const seq = [];
        const outer = document.getElementById('outer');
        const inner = document.getElementById('inner');
        const ac = new AbortController();

        outer.addEventListener('ping', (e) => seq.push('cap:' + e.eventPhase + ':' + e.detail.v), { capture: true });
        inner.addEventListener('ping', (e) => seq.push('once:' + e.detail.v), { once: true });
        inner.addEventListener('ping', (e) => { e.preventDefault(); seq.push('passive:' + e.defaultPrevented); }, { passive: true });
        outer.addEventListener('ping', () => seq.push('aborted-MUST-NOT-RUN'), { signal: ac.signal });
        let abortFired = false;
        ac.signal.addEventListener('abort', () => { abortFired = true; });
        ac.abort('stop');

        const ev1 = new CustomEvent('ping', { bubbles: true, cancelable: true, detail: { v: 7 } });
        const r1 = inner.dispatchEvent(ev1);
        const ev2 = new CustomEvent('ping', { bubbles: true, cancelable: true, detail: { v: 8 } });
        const r2 = inner.dispatchEvent(ev2);

        const mev = new MouseEvent('mousedown', { clientX: 42, clientY: 99, button: 1, ctrlKey: true });
        const kev = new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', shiftKey: true });
        const inp = document.getElementById('inp');
        let focused = false;
        inp.addEventListener('focus', () => { focused = (document.activeElement === inp); });
        inp.focus();
        const wasActive = document.activeElement === inp;
        inp.blur();
        const afterBlur = document.activeElement === document.body;

        document.getElementById('out').textContent = [
          abortFired, r1, r2, seq.join(','),
          mev.clientX + ':' + mev.button + ':' + mev.ctrlKey,
          kev.key + ':' + kev.shiftKey,
          focused + ':' + wasActive + ':' + afterBlur
        ].join(' | ');
      </script>
    </body></html>"#;

    let (mut realm, init) = start(html);
    let r = drain_zero_delay_tasks(&mut realm, init);
    assert_eq!(
        text_of(&r, "#out"),
        "true | true | true | cap:1:7,once:7,passive:false,cap:1:8,passive:false | 42:1:true | Enter:true | true:true:true"
    );
}

#[test]
fn web_platform_apis_geometry_computed_style_and_timers() {
    let html = r#"<!doctype html>
    <html><head><style>
      body { margin: 0; }
      #box { width: 240px; height: 60px; padding: 10px; border: 2px solid rgb(255, 0, 0); }
    </style></head><body>
      <div id="box">Geometry</div>
      <p id="out"></p>
      <script>
        const u = new URL('../search?a=1&b=two#top', 'https://example.org/dir/page.html');
        u.searchParams.append('c', '3');
        const bytes = new TextEncoder().encode('café');
        const decoded = new TextDecoder().decode(bytes);
        const b64 = btoa('hello!');
        const unb64 = atob(b64);

        const rand = new Uint8Array(16);
        crypto.getRandomValues(rand);
        const nonZero = rand.some((x) => x !== 0);
        const uuid = crypto.randomUUID();
        const uuidOk = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(uuid);

        const orig = { a: [1, 2], m: new Map([['k', 9]]), s: new Set(['x']) };
        orig.self = orig;
        const cloned = structuredClone(orig);
        const cloneOk = cloned !== orig && cloned.self === cloned && cloned.m.get('k') === 9 && cloned.s.has('x');

        const mqMatch = window.matchMedia('(min-width: 800px)').matches;
        const mqMiss = window.matchMedia('(max-width: 400px)').matches;

        const box = document.getElementById('box');
        const cs = getComputedStyle(box);
        const rect = box.getBoundingClientRect();
        const geomOk = rect.width === 264 && rect.height === 84 && box.offsetWidth === 264 && box.clientWidth === 260;

        performance.mark('m1');
        const order = [];
        queueMicrotask(() => order.push('micro'));
        let ticks = 0;
        const iv = setInterval(() => {
          ticks++;
          order.push('iv' + ticks);
          if (ticks === 2) {
            clearInterval(iv);
            requestAnimationFrame((ts) => {
              order.push('raf:' + (typeof ts === 'number'));
              document.getElementById('out').textContent = [
                u.href, bytes.length + ':' + decoded, b64 + ':' + unb64,
                nonZero + ':' + uuidOk, cloneOk, mqMatch + ':' + mqMiss,
                cs.getPropertyValue('border-top-width') + ':' + geomOk,
                order.join(',')
              ].join(' | ');
            });
          }
        }, 0);
      </script>
    </body></html>"#;

    let mut init_platform = PlatformInit::default();
    init_platform.viewport = ViewportState {
        inner_width: 1024,
        inner_height: 768,
        scroll_x: 0.0,
        scroll_y: 0.0,
        device_pixel_ratio: 2.0,
    };
    let (mut realm, init) = start_with_seed(html, init_platform, [0x99; 32]);
    let mut r = init;
    // Pump tasks (even 16ms rAF or clamped 4ms nested interval) by sleeping briefly when delay_ms > 0.
    for _ in 0..16 {
        let Some(wake) = r.task_wake.as_ref() else {
            break;
        };
        if wake.delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(
                u64::from(wake.delay_ms) + 2,
            ));
        }
        r = realm.pump_tasks(TaskPump {
            wake_token: wake.token,
        });
        assert_eq!(r.state, RealmState::Ready, "{:?}", r.errors);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
    }
    assert_eq!(
        text_of(&r, "#out"),
        "https://example.org/search?a=1&b=two&c=3#top | 5:café | aGVsbG8h:hello! | true:true | true | true:false | 2px:true | micro,iv1,iv2,raf:true"
    );
}
