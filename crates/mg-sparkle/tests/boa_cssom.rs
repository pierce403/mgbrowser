//! Additive modern declaration controls, not upstream WPT numerator entries.
//! Frozen before the parser-backed binding increment. Legacy raw-style tests
//! remain unchanged. Full interfaces, expandos, enumeration and PutForwards
//! are intentionally not required by this first-slice roster.
#![cfg(feature = "modern")]

use mg_sparkle::{
    document::{self, Document},
    js_browser::{Request, boa::BoaPageRealm},
    page_session::{
        DefaultAction, InputKind, LifecyclePhase, RealmState, SessionInput, SessionReply, TaskPump,
    },
};

const URL: &str = "https://example.test/owned-cssom";

fn page(source: &str) -> Request {
    Request {
        url: URL.into(),
        external_scripts: None,
        html: format!(
            r#"<!doctype html><html><head><title>before</title></head><body>
            <div id="target"></div><button id="button">Owned control</button>
            <script>'use strict';
            function check(value, message) {{ if (!value) throw new Error(message); }}
            function equal(actual, expected, message) {{
                if (actual !== expected) throw new Error(message);
            }}
            function typeError(callback, message) {{
                let caught = false;
                try {{ callback(); }} catch (error) {{ caught = error instanceof TypeError; }}
                check(caught, message);
            }}
            {source}
            document.title = 'passed';
            </script></body></html>"#
        ),
    }
}

fn projected(reply: &SessionReply) -> Document {
    document::project_nodes(
        reply
            .snapshot
            .as_ref()
            .expect("real snapshot")
            .nodes
            .clone(),
        URL,
    )
    .expect("validated owned projection")
}

fn work(reply: &SessionReply) -> (usize, usize, usize) {
    let report = reply.boa.as_ref().expect("real modern accounting");
    assert!(reply.accounting_valid(), "{report:?}");
    assert!(report.fatal_reason.is_none(), "{report:?}");
    (
        report.jobs_admitted,
        report.jobs_executed,
        report.pending_jobs,
    )
}

fn clean(reply: &SessionReply) {
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(reply.navigation.is_none());
    assert_eq!(reply.default_action, DefaultAction::None);
    assert!(!reply.lifecycle.expect("modern lifecycle").initial_failure);
    work(reply);
}

fn loaded(source: &str) -> (BoaPageRealm, SessionReply) {
    let (realm, initial) = BoaPageRealm::start(page(source));
    clean(&initial);
    assert_eq!(initial.scripts_executed, 1);
    assert_eq!(initial.revision, 0);
    assert_eq!(
        initial.lifecycle.unwrap().phase,
        LifecyclePhase::Interactive
    );
    assert_eq!(work(&initial), (2, 0, 2));
    assert_eq!(projected(&initial).title, "passed");
    let mut realm = realm.expect("retained modern realm");
    let mut reply = initial;
    for (phase, expected_work) in [
        (LifecyclePhase::DomContentLoaded, (2, 1, 1)),
        (LifecyclePhase::LoadComplete, (2, 2, 0)),
    ] {
        let wake = reply
            .task_wake
            .as_ref()
            .expect("real mandatory lifecycle task");
        assert_eq!(wake.delay_ms, 0);
        let next = realm.pump_tasks(TaskPump {
            wake_token: wake.token,
        });
        clean(&next);
        assert_eq!(next.revision, reply.revision + 1);
        assert_eq!(next.scripts_executed, 0);
        assert_eq!(next.lifecycle.unwrap().phase, phase);
        assert_eq!(work(&next), expected_work);
        next.boa
            .as_ref()
            .unwrap()
            .validate_after(reply.boa.as_ref().unwrap())
            .unwrap();
        assert_eq!(projected(&next).title, "passed");
        reply = next;
    }
    assert!(reply.task_wake.is_none());
    (realm, reply)
}

fn successful(source: &str) -> SessionReply {
    loaded(source).1
}

fn click(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let target = projected(previous)
        .query_selector(0, "#button")
        .unwrap()
        .unwrap();
    let reply = realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    });
    clean(&reply);
    assert_eq!(reply.revision, previous.revision + 1);
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(reply.outcome.click_canceled, Some(true));
    assert_eq!(work(&reply), (2, 2, 0));
    assert!(reply.task_wake.is_none());
    reply
        .boa
        .as_ref()
        .unwrap()
        .validate_after(previous.boa.as_ref().unwrap())
        .unwrap();
    reply
}

#[test]
fn property_method_has_required_arity_and_ignores_extra_arguments() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        equal(typeof style.getPropertyValue, 'function', 'genuine method');
        style.width = '12px';
        typeError(() => style.getPropertyValue(), 'required property argument');
        equal(style.getPropertyValue(undefined), '', 'explicit undefined is a string');
        equal(style.getPropertyValue(null), '', 'property null has no empty annotation');
        let ignored = 0;
        const extra = { toString() { ignored++; throw new Error('ignored argument'); } };
        equal(style.getPropertyValue('width', extra), '12px', 'first argument only');
        equal(ignored, 0, 'extra argument never converted');
    "#,
    );
}

#[test]
fn forged_proxy_and_wrong_kind_receivers_fail_before_argument_coercion() {
    successful(
        r#"
        const element = document.getElementById('target');
        const style = element.style;
        const get = style.getPropertyValue;
        equal(typeof get, 'function', 'negative receiver controls require a genuine method');
        let conversions = 0, traps = 0;
        const argument = { toString() { conversions++; return 'width'; } };
        const proxy = new Proxy(style, {
            get() { traps++; throw new Error('receiver get trap'); },
            getPrototypeOf() { traps++; throw new Error('receiver prototype trap'); }
        });
        const revoked = Proxy.revocable(style, {});
        revoked.revoke();
        const bad = [{}, Object.create(style), element, document, window,
                     null, undefined, 3, proxy, revoked.proxy];
        for (const receiver of bad) {
            typeError(() => get.call(receiver, argument), 'invalid style receiver');
        }
        typeError(() => get(argument), 'bare method call');
        equal(conversions, 0, 'receiver precedes coercion');
        equal(traps, 0, 'receiver validation does not invoke proxy traps');
    "#,
    );
}

#[test]
fn extracted_method_uses_its_actual_style_receiver() {
    successful(
        r#"
        const first = document.getElementById('target').style;
        const second = document.createElement('span').style;
        first.width = '11px'; second.width = '23px';
        const get = first.getPropertyValue;
        equal(get.call(second, 'width'), '23px', 'borrowed method uses second');
        equal(get.bind(second)('width'), '23px', 'bound actual receiver');
        equal(first.getPropertyValue('width'), '11px', 'first stays independent');
    "#,
    );
}

#[test]
fn string_conversion_runs_once_and_preserves_thrown_identity() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        style.width = '12px';
        let calls = 0;
        equal(style.getPropertyValue({ toString() { calls++; return 'width'; } }),
              '12px', 'converted property');
        equal(calls, 1, 'property converted once');
        style.width = { toString() { calls++; return '17px'; } };
        equal(calls, 2, 'value converted once');
        equal(style.getPropertyValue('width'), '17px', 'converted value');
        const sentinel = { owned: true };
        let caught;
        try { style.width = { toString() { throw sentinel; } }; } catch (error) { caught = error; }
        equal(caught, sentinel, 'value conversion keeps thrown identity');
        equal(style.getPropertyValue('width'), '17px', 'no outer mutation on throw');
        caught = undefined;
        try { style.getPropertyValue({ toString() { throw sentinel; } }); } catch (error) { caught = error; }
        equal(caught, sentinel, 'property conversion keeps thrown identity');
        for (const value of [Symbol('owned'), Object(Symbol('owned'))]) {
            typeError(() => { style.width = value; }, 'named Symbol rejects');
            typeError(() => { style.cssText = value; }, 'cssText Symbol rejects');
            typeError(() => style.getPropertyValue(value), 'property Symbol rejects');
        }
        equal(style.getPropertyValue('width'), '17px', 'Symbol paths preserve state');
    "#,
    );
}

#[test]
fn named_null_removes_but_undefined_is_an_invalid_value() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        style.width = '12px'; style.width = null;
        equal(style.getPropertyValue('width'), '', 'annotated null removes');
        style.width = '12px'; style.width = undefined;
        equal(style.getPropertyValue('width'), '12px', 'undefined is invalid CSS');
        style.width = '';
        equal(style.getPropertyValue('width'), '', 'empty removes');
        equal(style.cssText, '', 'empty assignment stores no declaration');
        style.width = '12px'; style.cssText = null;
        equal(style.cssText, '', 'cssText parses string null as replacement');
        style.width = '12px'; style.cssText = undefined;
        equal(style.cssText, '', 'cssText parses string undefined as replacement');
    "#,
    );
}

#[test]
fn method_css_names_and_named_camel_case_are_separate_mappings() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        style.marginLeft = '4px';
        equal(style['margin-left'], '4px', 'dashed named attribute');
        equal(style.getPropertyValue('MARGIN-LEFT'), '4px', 'CSS method ASCII case fold');
        equal(style.getPropertyValue('marginLeft'), '', 'CSS method does not camel-map');
        style['margin-left'] = '9px';
        equal(style.marginLeft, '9px', 'dashed setter and camel getter share state');
        style.cssFloat = 'left';
        equal(style.getPropertyValue('float'), 'left', 'cssFloat mapping');
        for (const name of ['', 'not-a-real-owned-property', 'width height', 'width:']) {
            equal(style.getPropertyValue(name), '', 'unknown CSS query returns empty');
        }
    "#,
    );
}

#[test]
fn custom_property_names_preserve_case_and_quoted_delimiters() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        style.cssText = '--A:One;--a:two;--token:"a;b:c";width:8px';
        equal(style.getPropertyValue('--A'), 'One', 'uppercase custom property');
        equal(style.getPropertyValue('--a'), 'two', 'lowercase custom property');
        equal(style.getPropertyValue('--TOKEN'), '', 'custom names do not case-fold');
        equal(style.getPropertyValue('--token'), '"a;b:c"', 'quoted delimiters survive');
        style.width = '13px';
        equal(style.getPropertyValue('--token'), '"a;b:c"', 'mutation does not split custom tokens');
        equal(style.getPropertyValue('--A'), 'One', 'unrelated custom state survives');
        equal(style.getPropertyValue('width'), '13px', 'ordinary property changed');
    "#,
    );
}

#[test]
fn cssom_string_uses_the_declared_usv_replacement_boundary() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        const lone = String.fromCharCode(0xd800);
        const pair = String.fromCharCode(0xd83e, 0xdd80);
        style.cssText = '--lone:"' + lone + '";--pair:"café ' + pair + '"';
        const replaced = style.getPropertyValue('--lone');
        check(replaced.includes('\ufffd'), 'lone surrogate becomes replacement');
        check(!replaced.includes(lone), 'isolated surrogate not preserved');
        const retained = style.getPropertyValue('--pair');
        check(retained.includes('café'), 'ordinary non-ASCII retained');
        check(retained.includes(pair), 'valid surrogate pair retained');
    "#,
    );
}

#[test]
fn invalid_single_values_do_not_replace_or_inject_declarations() {
    let reply = successful(
        r#"
        const element = document.getElementById('target');
        const style = element.style;
        style.width = '12px';
        equal(style.cssText, 'width: 12px;', 'single canonical declaration');
        const before = element.getAttribute('style');
        for (const value of ['-1px', '12px 13px', '17px;height:99px', '17px !important']) {
            style.width = value;
            equal(style.getPropertyValue('width'), '12px', 'invalid write is a no-op');
            equal(style.getPropertyValue('height'), '', 'value cannot inject a declaration');
            equal(element.getAttribute('style'), before, 'invalid write does not touch owner');
        }
        style.width = style.getPropertyValue('width');
        equal(style.getPropertyValue('width'), '12px', 'serialization round-trips');
    "#,
    );
    let doc = projected(&reply);
    let node = doc.query_selector(0, "#target").unwrap().unwrap();
    assert_eq!(doc.nodes[node].attr("style"), Some("width: 12px;"));
}

#[test]
fn non_flex_shorthand_has_real_longhand_and_empty_removal_state() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        style.margin = '1px 2px';
        equal(style.getPropertyValue('margin'), '1px 2px', 'canonical margin shorthand');
        equal(style.marginTop, '1px', 'top component');
        equal(style.marginRight, '2px', 'right component');
        equal(style.marginBottom, '1px', 'bottom component');
        equal(style.marginLeft, '2px', 'left component');
        style.marginLeft = '3px';
        equal(style.getPropertyValue('margin'), '1px 2px 1px 3px', 'longhand updates shorthand');
        const before = style.cssText;
        style.margin = '1px 2px 3px 4px 5px';
        equal(style.cssText, before, 'invalid shorthand changes no components');
        style.width = '12px'; style.margin = '';
        for (const name of ['margin', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left']) {
            equal(style.getPropertyValue(name), '', 'all shorthand components removed');
        }
        equal(style.getPropertyValue('width'), '12px', 'unrelated declaration retained');
    "#,
    );
}

#[test]
fn named_shorthand_overrides_prior_important_components_atomically() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        style.cssText = 'flex-grow:9!important;flex-shrink:8;flex-basis:7px!important';
        equal(style.getPropertyValue('flex'), '', 'mixed importance cannot form shorthand');
        style.flex = '2 1 13px';
        equal(style.getPropertyValue('flex'), '2 1 13px', 'named shorthand replaces existing state');
        equal(style.flexGrow, '2', 'grow component');
        equal(style.flexShrink, '1', 'shrink component');
        equal(style.flexBasis, '13px', 'basis component');
        check(!style.cssText.includes('!important'), 'named setter clears old priorities');
        const before = style.cssText;
        style.flex = '2 auto auto';
        equal(style.cssText, before, 'invalid shorthand does not partially update');
        style.flex = '';
        for (const name of ['flex', 'flex-grow', 'flex-shrink', 'flex-basis']) {
            equal(style.getPropertyValue(name), '', 'flex removal clears mapped longhands');
        }
        style.cssText = 'width:12px!important';
        style.width = '12px';
        equal(style.cssText, 'width: 12px;', 'same value still clears importance');
    "#,
    );
}

#[test]
fn flex_flow_and_numeric_specified_values_are_not_raw_input_echoes() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        style.flexFlow = 'wrap-reverse column-reverse';
        equal(style.getPropertyValue('flex-flow'), 'column-reverse wrap-reverse', 'canonical component order');
        equal(style.flexDirection, 'column-reverse', 'direction longhand');
        equal(style.flexWrap, 'wrap-reverse', 'wrap longhand');
        style.opacity = '.5';
        equal(style.getPropertyValue('opacity'), '0.5', 'canonical CSS number');
        const value = style.getPropertyValue('flex-flow');
        style.flexFlow = value;
        equal(style.getPropertyValue('flex-flow'), value, 'canonical shorthand round-trip');
    "#,
    );
}

#[test]
fn css_text_replaces_the_list_but_retains_valid_parsed_siblings() {
    successful(
        r#"
        const element = document.getElementById('target');
        const style = element.style;
        style.width = '12px'; style.height = '7px';
        style.cssText = 'this is not a declaration';
        equal(style.cssText, '', 'invalid replacement empties previous list');
        equal(style.getPropertyValue('width'), '', 'old width removed');
        equal(style.getPropertyValue('height'), '', 'old height removed');
        equal(element.getAttribute('style'), '', 'owner follows empty declaration list');
        style.cssText = 'width:3px;not-a-real-owned-property:9;height:7px;width:5px!important;width:6px';
        equal(style.getPropertyValue('width'), '5px', 'declaration parser keeps important precedence');
        equal(style.getPropertyValue('height'), '7px', 'valid sibling survives invalid declaration');
        equal(style.getPropertyValue('not-a-real-owned-property'), '', 'unknown declaration dropped');
        equal(element.getAttribute('style'), style.cssText, 'owner uses serialized current list');
    "#,
    );
}

#[test]
fn css_text_assignment_updates_an_absent_owner_even_for_empty_declarations() {
    successful(
        r#"
        const element = document.getElementById('target');
        const style = element.style;
        equal(typeof style.getPropertyValue, 'function', 'genuine declaration bridge');
        equal(element.getAttribute('style'), null, 'initial owner attribute absent');
        equal(style.cssText, '', 'empty read');
        equal(element.getAttribute('style'), null, 'read must not create owner attribute');
        style.cssText = '';
        equal(element.getAttribute('style'), '', 'explicit empty setter creates owner attribute');
        element.removeAttribute('style');
        style.cssText = 'not a declaration';
        equal(style.cssText, '', 'invalid full list parses empty');
        equal(element.getAttribute('style'), '', 'invalid replacement still updates owner');
    "#,
    );
}

#[test]
fn escaped_style_reads_attribute_changes_without_rewriting_on_read() {
    successful(
        r#"
        const element = document.getElementById('target');
        const style = element.style;
        equal(element.style, style, 'live style identity');
        const raw = ' width : 3px ; height:7px';
        element.setAttribute('style', raw);
        equal(style.getPropertyValue('width'), '3px', 'escaped wrapper reads new attribute');
        equal(style.getPropertyValue('height'), '7px', 'new second declaration');
        equal(element.getAttribute('style'), raw, 'getPropertyValue does not rewrite raw attribute');
        check(style.cssText.includes('width: 3px;'), 'cssText serializes parsed declarations');
        equal(element.getAttribute('style'), raw, 'cssText getter does not rewrite raw attribute');
        element.removeAttribute('style');
        equal(style.getPropertyValue('width'), '', 'removeAttribute invalidates state');
        equal(style.cssText, '', 'removed owner has empty declaration list');
        equal(element.getAttribute('style'), null, 'reading removed style does not create attribute');
        style.width = '12px';
        equal(element.getAttribute('style'), 'width: 12px;', 'mutation updates actual owner');
    "#,
    );
}

#[test]
fn native_dispatch_does_not_call_author_prototype_methods() {
    successful(
        r#"
        let calls = 0;
        Object.prototype.setProperty = function() { calls++; throw new Error('author setter'); };
        Object.prototype.getPropertyValue = function() { calls++; throw new Error('author getter'); };
        const style = document.getElementById('target').style;
        style.width = '12px';
        equal(style.getPropertyValue('width'), '12px', 'native fixed method dispatch');
        style.width = '';
        equal(style.getPropertyValue('width'), '', 'native internal removal');
        equal(calls, 0, 'no author prototype method invocation');
        delete Object.prototype.setProperty;
        delete Object.prototype.getPropertyValue;
    "#,
    );
}

#[test]
fn proxy_define_and_delete_refusal_remains_unchanged() {
    successful(
        r#"
        const style = document.getElementById('target').style;
        style.width = '12px';
        typeError(() => Object.defineProperty(style, 'width', {value:'99px'}), 'define refused');
        typeError(() => Reflect.deleteProperty(style, 'width'), 'delete refused');
        equal(style.getPropertyValue('width'), '12px', 'refused operations do not mutate');
    "#,
    );
}

#[test]
fn argument_reentry_uses_post_conversion_state_and_keeps_inner_effects() {
    successful(
        r#"
        const element = document.getElementById('target');
        const style = element.style;
        let calls = 0;
        style.width = '1px';
        style.width = { toString() {
            calls++;
            equal(style.getPropertyValue('width'), '1px', 'reentry can read old state');
            style.width = '2px';
            document.body.removeChild(element);
            return '3px';
        } };
        equal(calls, 1, 'value converted once');
        equal(style.getPropertyValue('width'), '3px', 'outer write follows completed conversion');
        equal(element.style, style, 'detached real element keeps same live style');
        equal(style.getPropertyValue({ toString() { style.width = '4px'; return 'width'; } }),
              '4px', 'query reads post-conversion state');
        const sentinel = { owned: true };
        let caught;
        try { style.width = { toString() { style.height = '8px'; throw sentinel; } }; }
        catch (error) { caught = error; }
        equal(caught, sentinel, 'reentrant throw identity');
        equal(style.getPropertyValue('width'), '4px', 'failed outer replacement not committed');
        equal(style.getPropertyValue('height'), '8px', 'completed inner effect retained');
        document.body.appendChild(element);
        equal(element.getAttribute('style'), style.cssText, 'reinserted owner stays coherent');
    "#,
    );
}

#[test]
fn style_wrapper_is_weakly_cached_and_method_does_not_retain_its_first_receiver() {
    let (mut realm, loaded) = loaded(
        r#"
        var weakStyle, savedGet;
        (() => {
            const detached = document.createElement('span');
            const style = detached.style;
            equal(detached.style, style, 'identity while wrapper is held');
            style.width = '21px';
            savedGet = style.getPropertyValue;
            weakStyle = new WeakRef(style);
        })();
        document.getElementById('button').onclick = function(event) {
            event.preventDefault();
            equal(weakStyle.deref(), undefined, 'no strong cached style wrapper');
            const other = document.createElement('span');
            other.style.width = '34px';
            equal(savedGet.call(other.style, 'width'), '34px', 'saved method uses current receiver');
            document.title = 'weak wrapper released';
        };
    "#,
    );
    // Init plus the two genuine lifecycle checkpoints already completed. Do
    // not add test-only ClearKeptObjects or replace the realm before collection.
    boa_gc::force_collect();
    let reply = click(&mut realm, &loaded);
    assert_eq!(projected(&reply).title, "weak wrapper released");
}

#[test]
fn retained_detached_style_remains_live_across_real_checkpoints_and_collection() {
    let (mut realm, loaded) = loaded(
        r#"
        var retainedStyle;
        (() => {
            const element = document.createElement('span');
            document.body.appendChild(element);
            retainedStyle = element.style;
            retainedStyle.width = '55px';
            document.body.removeChild(element);
        })();
        document.getElementById('button').onclick = function(event) {
            event.preventDefault();
            equal(retainedStyle.getPropertyValue('width'), '55px', 'retained detached declaration');
            retainedStyle.width = '89px';
            equal(retainedStyle.getPropertyValue('width'), '89px', 'retained style remains usable');
            document.title = 'retained style live';
        };
    "#,
    );
    boa_gc::force_collect();
    let reply = click(&mut realm, &loaded);
    assert_eq!(projected(&reply).title, "retained style live");
}
