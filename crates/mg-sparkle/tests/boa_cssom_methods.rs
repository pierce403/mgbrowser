//! Additive public controls for docs/CSSOM_METHODS.md, frozen before implementation.
//! These are owned assertions, not upstream WPT results. Private quota/foreign-
//! realm controls and fresh restricted-worker checks remain separate gates.
#![cfg(feature = "modern")]

use mg_sparkle::{
    document::{self, Document},
    js_browser::{Request, boa::BoaPageRealm},
    page_session::{
        DefaultAction, InputKind, LifecyclePhase, RealmState, SessionInput, SessionReply, TaskPump,
    },
};

const URL: &str = "https://example.test/owned-cssom-methods";

fn page(source: &str) -> Request {
    Request {
        url: URL.into(),
        external_scripts: None,
        html: format!(
            r#"<!doctype html><html><head><title>before</title></head><body>
            <div id="target"></div><button id="button">Owned methods control</button>
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
            function hostError(callback, message) {{
                let caught = false;
                try {{ callback(); }} catch (error) {{ caught = true; }}
                check(caught, message);
            }}
            function methods(style) {{
                for (const name of ['getPropertyValue', 'setProperty',
                                    'removeProperty', 'getPropertyPriority']) {{
                    equal(typeof style[name], 'function', 'requires actual callable ' + name);
                }}
                return style;
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
    assert!(!reply.lifecycle.expect("real lifecycle").initial_failure);
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
        let wake = reply.task_wake.as_ref().expect("real lifecycle task");
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
fn four_methods_have_stable_same_realm_identity_and_public_metadata() {
    successful(
        r#"
        const first = methods(document.getElementById('target').style);
        const second = methods(document.createElement('span').style);
        for (const pair of [['getPropertyValue', 1], ['setProperty', 2],
                            ['removeProperty', 1], ['getPropertyPriority', 1]]) {
            const name = pair[0], method = first[name];
            check(name in first, 'structural method presence');
            equal(first[name], method, 'repeated lookup identity');
            equal(second[name], method, 'per-realm not per-owner method identity');
            equal(method.name, name, 'public method name');
            equal(method.length, pair[1], 'public required arity metadata');
        }
        equal(first.setProperty('width', '12px'), undefined, 'set returns undefined');
        equal(first.getPropertyPriority('width'), '', 'priority returns string');
        equal(first.removeProperty('width'), '12px', 'remove returns old specified value');
        equal(first.removeProperty('width'), '', 'absent remove returns empty string');
        // This increment corrects style metadata only, not the older DOM factory.
        equal(document.getElementById('target').setAttribute.name, 'host.dom.setAttribute', 'other DOM name unchanged');
        equal(document.getElementById('target').setAttribute.length, 1, 'other DOM length unchanged');
    "#,
    );
}

#[test]
fn extracted_borrowed_and_bound_methods_use_the_actual_receiver() {
    successful(
        r#"
        const first = methods(document.getElementById('target').style);
        const second = methods(document.createElement('span').style);
        first.setProperty('width', '11px', 'important');
        const set = first.setProperty, remove = first.removeProperty;
        const priority = first.getPropertyPriority;
        equal(set.call(second, 'width', '23px', 'important'), undefined, 'borrowed set');
        equal(priority.call(second, 'width'), 'important', 'borrowed priority');
        equal(remove.bind(second)('width'), '23px', 'bound removal');
        set.bind(second)('height', '7px');
        equal(second.getPropertyValue('height'), '7px', 'bound set');
        equal(priority.bind(second)('height'), '', 'bound priority');
        equal(first.getPropertyValue('width'), '11px', 'first owner is not captured');
        equal(first.getPropertyPriority('width'), 'important', 'first importance unchanged');
        equal(remove.call(second, 'height'), '7px', 'borrowed remove');
    "#,
    );
}

#[test]
fn invalid_receivers_reject_without_proxy_traps_or_argument_conversion() {
    successful(
        r#"
        const element = document.getElementById('target');
        const style = methods(element.style);
        let conversions = 0, traps = 0;
        const argument = { toString() { conversions++; return 'width'; } };
        const proxy = new Proxy(style, {
            get() { traps++; throw new Error('receiver getter'); },
            getPrototypeOf() { traps++; throw new Error('receiver prototype getter'); }
        });
        const revoked = Proxy.revocable(style, {}); revoked.revoke();
        for (const receiver of [{}, Object.create(style), element, document, window,
                                null, undefined, 3, proxy, revoked.proxy]) {
            typeError(() => style.setProperty.call(receiver, argument, argument, argument), 'bad set receiver');
            typeError(() => style.removeProperty.call(receiver, argument), 'bad remove receiver');
            typeError(() => style.getPropertyPriority.call(receiver, argument), 'bad priority receiver');
            typeError(() => style.setProperty.call(receiver), 'bad receiver with missing arity');
        }
        const set = style.setProperty, remove = style.removeProperty, priority = style.getPropertyPriority;
        typeError(() => set(argument, argument), 'bare set');
        typeError(() => remove(argument), 'bare remove');
        typeError(() => priority(argument), 'bare priority');
        equal(conversions, 0, 'receiver validation precedes all conversions');
        equal(traps, 0, 'receiver validation does not consult author Proxy');
    "#,
    );
}

#[test]
fn required_arguments_precede_conversions_and_extra_arguments_are_ignored() {
    successful(
        r#"
        const style = methods(document.getElementById('target').style);
        let conversions = 0;
        const poison = { toString() { conversions++; throw new Error('unexpected conversion'); } };
        typeError(() => style.setProperty(), 'set requires two');
        typeError(() => style.setProperty(poison), 'set one argument still missing value');
        typeError(() => style.removeProperty(), 'remove requires one');
        typeError(() => style.getPropertyPriority(), 'priority requires one');
        equal(conversions, 0, 'arity check precedes argument conversion');
        style.setProperty('width', '12px', '', poison);
        equal(style.getPropertyPriority('width', poison), '', 'priority ignores second argument');
        equal(style.removeProperty('width', poison), '12px', 'remove ignores second argument');
        equal(style.removeProperty(undefined), '', 'explicit undefined is a property name');
        equal(style.removeProperty(null), '', 'property null converts to name');
        equal(style.getPropertyPriority(undefined), '', 'undefined priority-query name');
        equal(style.getPropertyPriority(null), '', 'null priority-query name');
        equal(conversions, 0, 'extra arguments never converted');
    "#,
    );
}

#[test]
fn all_three_set_arguments_convert_once_in_order_before_css_checks() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        let order = '';
        function argument(mark, value) { return { toString() { order += mark; return value; } }; }
        style.setProperty(argument('n', 'width'), argument('v', '12px'), argument('p', 'important'));
        equal(order, 'nvp', 'set conversion order');
        equal(style.getPropertyValue('width'), '12px', 'converted value');
        equal(style.getPropertyPriority('width'), 'important', 'converted priority');
        const before = element.getAttribute('style');
        order = '';
        style.setProperty(argument('n', 'not-an-owned-property'), argument('v', '1px'), argument('p', 'bad'));
        equal(order, 'nvp', 'unknown property and invalid priority still convert all');
        equal(element.getAttribute('style'), before, 'unknown set preserves raw bytes');
        order = '';
        equal(style.removeProperty(argument('n', 'missing-property')), '', 'converted missing remove');
        equal(order, 'n', 'remove converts once');
        order = '';
        equal(style.getPropertyPriority(argument('n', 'width')), 'important', 'converted priority query');
        equal(order, 'n', 'priority converts once');
    "#,
    );
}

#[test]
fn conversion_exceptions_keep_identity_and_skip_later_arguments() {
    successful(
        r#"
        const style = methods(document.getElementById('target').style);
        style.setProperty('width', '12px');
        const sentinel = { owned: true };
        for (const stop of ['n', 'v', 'p']) {
            let order = '', caught;
            function argument(mark, value) { return { toString() {
                order += mark; if (mark === stop) throw sentinel; return value;
            } }; }
            try { style.setProperty(argument('n', 'width'), argument('v', '27px'), argument('p', 'important')); }
            catch (error) { caught = error; }
            equal(caught, sentinel, 'set preserves thrown value identity');
            equal(order, stop === 'n' ? 'n' : stop === 'v' ? 'nv' : 'nvp', 'conversion stops at throw');
            equal(style.getPropertyValue('width'), '12px', 'failed set preserves width');
        }
        for (const method of [style.removeProperty, style.getPropertyPriority]) {
            let caught;
            try { method.call(style, { toString() { throw sentinel; } }); }
            catch (error) { caught = error; }
            equal(caught, sentinel, 'single-name operation preserves thrown identity');
        }
    "#,
    );
}

#[test]
fn symbol_conversion_is_not_masked_by_unknown_names_or_empty_removal() {
    successful(
        r#"
        const style = methods(document.getElementById('target').style);
        style.setProperty('width', '12px');
        for (const symbol of [Symbol('owned'), Object(Symbol('owned'))]) {
            typeError(() => style.setProperty(symbol, '1px'), 'Symbol name');
            typeError(() => style.setProperty('width', symbol), 'Symbol value');
            typeError(() => style.setProperty('unknown-owned-name', symbol), 'unknown still converts value');
            typeError(() => style.setProperty('width', '1px', symbol), 'Symbol priority');
            typeError(() => style.setProperty('width', '', symbol), 'empty removal still converts priority');
            typeError(() => style.removeProperty(symbol), 'Symbol remove name');
            typeError(() => style.getPropertyPriority(symbol), 'Symbol priority-query name');
        }
        equal(style.getPropertyValue('width'), '12px', 'Symbol failures preserve declaration');
    "#,
    );
}

#[test]
fn null_empty_undefined_and_optional_priority_follow_webidl_rules() {
    successful(
        r#"
        const style = methods(document.getElementById('target').style);
        for (const priority of [null, undefined, '']) {
            style.setProperty('width', '12px', 'important');
            style.setProperty('width', '12px', priority);
            equal(style.getPropertyPriority('width'), '', 'empty-like priority clears importance');
        }
        style.setProperty('width', '12px', 'important');
        style.setProperty('width', '12px');
        equal(style.getPropertyPriority('width'), '', 'omitted priority defaults empty');
        style.setProperty('width', undefined);
        equal(style.getPropertyValue('width'), '12px', 'undefined value is invalid CSS not removal');
        style.setProperty('width', null, 'bad');
        equal(style.getPropertyValue('width'), '', 'annotated null removes despite invalid priority');
        style.setProperty('width', '13px', 'important');
        let calls = 0;
        style.setProperty('width', '', { toString() { calls++; return 'bad'; } });
        equal(calls, 1, 'empty value still converts priority');
        equal(style.getPropertyValue('width'), '', 'empty removal precedes semantic priority rejection');
    "#,
    );
}

#[test]
fn css_property_names_casefold_without_camel_case_or_cssfloat_aliases() {
    successful(
        r#"
        const style = methods(document.getElementById('target').style);
        style.setProperty('MARGIN-LEFT', '4px', 'important');
        equal(style.getPropertyValue('margin-left'), '4px', 'ordinary CSS name folds ASCII case');
        equal(style.getPropertyPriority('Margin-Left'), 'important', 'priority name folds case');
        style.setProperty('marginLeft', '99px');
        equal(style.getPropertyValue('margin-left'), '4px', 'camelCase is not CSS method spelling');
        equal(style.removeProperty('marginLeft'), '', 'camelCase removal is unknown');
        equal(style.getPropertyPriority('marginLeft'), '', 'camelCase query is unknown');
        equal(style.removeProperty('MARGIN-LEFT'), '4px', 'remove folds ordinary CSS case');
        style.setProperty('cssFloat', 'right');
        equal(style.getPropertyValue('float'), '', 'cssFloat is not CSS method alias');
        style.setProperty('FLOAT', 'left');
        equal(style.cssFloat, 'left', 'CSS name and named accessor share declaration');
    "#,
    );
}

#[test]
fn importance_replacement_is_unconditional_and_priority_is_not_trimmed() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        for (const priority of ['important', 'IMPORTANT', 'ImPoRtAnT']) {
            style.setProperty('width', '12px', priority);
            equal(style.getPropertyPriority('width'), 'important', 'accepted ASCII case variant');
            equal(style.getPropertyValue('width'), '12px', 'same-value importance change');
            style.setProperty('width', '12px', '');
            equal(style.getPropertyPriority('width'), '', 'same value clears importance');
        }
        const raw = ' WIDTH : 12px !important ; height:7px ';
        element.setAttribute('style', raw);
        for (const priority of [' important', 'important ', '!important', '\timportant', 'important\n', 'İMPORTANT']) {
            style.setProperty('width', '99px', priority);
            equal(element.getAttribute('style'), raw, 'invalid priority preserves exact raw attribute');
            equal(style.getPropertyPriority('width'), 'important', 'old priority retained');
        }
    "#,
    );
}

#[test]
fn semantic_noops_preserve_raw_spelling_and_do_not_create_absent_attributes() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        equal(element.getAttribute('style'), null, 'attribute initially absent');
        style.setProperty('width', '-1px');
        style.setProperty('unknown-owned-name', '12px');
        style.setProperty('width', '12px', 'bad');
        style.setProperty('width', '', 'bad');
        equal(style.removeProperty('width'), '', 'absent known removal');
        equal(style.getPropertyPriority('width'), '', 'absent priority');
        equal(element.getAttribute('style'), null, 'all no-ops preserve absent attribute');
        const raw = ' WIDTH : 12px !important ; malformed ; height:7px ';
        element.setAttribute('style', raw);
        equal(style.getPropertyPriority('WIDTH'), 'important', 'reads parse without rewriting');
        equal(style.removeProperty('unknown-owned-name'), '', 'unknown remove');
        equal(style.removeProperty('color'), '', 'absent known remove');
        style.setProperty('width', '12px', 'important');
        style.setProperty('width', '-1px');
        style.setProperty('', '3px');
        style.setProperty('unknown-owned-name', '3px');
        equal(element.getAttribute('style'), raw, 'equal/no-op operations retain exact old spelling');
    "#,
    );
}

#[test]
fn equal_physical_declaration_moves_after_a_competing_logical_declaration() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        element.setAttribute('style', 'margin-left:1px;margin-inline-start:2px');
        equal(style.getPropertyValue('margin-left'), '1px', 'physical specified value before');
        equal(style.getPropertyPriority('margin-left'), '', 'physical priority before');
        style.setProperty('margin-left', '1px');
        equal(element.getAttribute('style'), 'margin-inline-start: 2px; margin-left: 1px;',
              'equal assignment still moves physical declaration after logical competitor');
        equal(style.getPropertyValue('margin-left'), '1px', 'specified value unchanged');
        equal(style.getPropertyPriority('margin-left'), '', 'priority unchanged');
    "#,
    );
}

#[test]
fn single_value_parser_rejects_invalid_values_and_declaration_injection() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        style.setProperty('width', '12px', 'important');
        const before = element.getAttribute('style');
        for (const value of ['-1px', '12px 13px', '17px;height:99px', '17px !important']) {
            style.setProperty('width', value, 'important');
            equal(element.getAttribute('style'), before, 'invalid whole value is atomic no-op');
            equal(style.getPropertyValue('width'), '12px', 'old width retained');
            equal(style.getPropertyPriority('width'), 'important', 'old importance retained');
            equal(style.getPropertyValue('height'), '', 'no injected second declaration');
        }
    "#,
    );
}

#[test]
fn custom_property_case_priority_and_quoted_tokens_remain_independent() {
    successful(
        r#"
        const style = methods(document.getElementById('target').style);
        style.setProperty('--A', 'One', 'important');
        style.setProperty('--a', 'two');
        style.setProperty('--token', '"a;b:c"', 'IMPORTANT');
        equal(style.getPropertyValue('--A'), 'One', 'uppercase custom value');
        equal(style.getPropertyValue('--a'), 'two', 'lowercase custom value');
        equal(style.getPropertyPriority('--A'), 'important', 'uppercase custom importance');
        equal(style.getPropertyPriority('--a'), '', 'lowercase custom importance');
        equal(style.getPropertyPriority('--TOKEN'), '', 'custom priority query case-sensitive');
        equal(style.removeProperty('--TOKEN'), '', 'custom remove case-sensitive');
        equal(style.removeProperty('--token'), '"a;b:c"', 'quoted delimiter return serialization');
        equal(style.getPropertyValue('--token'), '', 'exact custom property removed');
        equal(style.getPropertyValue('--A'), 'One', 'unrelated custom retained');
        equal(style.removeProperty('--A'), 'One', 'return value excludes importance');
        equal(style.getPropertyValue('--a'), 'two', 'lowercase custom survives uppercase removal');
    "#,
    );
}

#[test]
fn names_and_values_use_scalar_replacement_without_corrupting_valid_pairs() {
    successful(
        r#"
        const style = methods(document.getElementById('target').style);
        const lone = String.fromCharCode(0xd800), pair = String.fromCharCode(0xd83e, 0xdd80);
        style.setProperty('--' + lone, '"' + lone + '"', 'important');
        equal(style.getPropertyValue('--\ufffd'), '"\ufffd"', 'name and value use replacement scalar');
        equal(style.getPropertyPriority('--' + lone), 'important', 'priority query converts name');
        equal(style.removeProperty('--' + lone), '"\ufffd"', 'remove converts name and returns scalar string');
        style.setProperty('--pair', '"café ' + pair + '"');
        equal(style.getPropertyValue('--pair'), '"café ' + pair + '"', 'valid pair and ordinary Unicode retained');
        style.setProperty('width', '12px');
        style.setProperty('width', '99px', lone);
        equal(style.getPropertyValue('width'), '12px', 'replaced priority is semantically invalid');
    "#,
    );
}

#[test]
fn margin_shorthand_updates_priority_and_all_longhands_atomically() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        style.setProperty('width', '12px');
        style.setProperty('margin', '1px 2px', 'important');
        equal(style.getPropertyValue('margin'), '1px 2px', 'shorthand serialization');
        equal(style.getPropertyPriority('margin'), 'important', 'all constituents important');
        for (const pair of [['margin-top', '1px'], ['margin-right', '2px'],
                            ['margin-bottom', '1px'], ['margin-left', '2px']]) {
            equal(style.getPropertyValue(pair[0]), pair[1], 'longhand value');
            equal(style.getPropertyPriority(pair[0]), 'important', 'longhand importance');
        }
        const before = element.getAttribute('style');
        style.setProperty('margin', '1px 2px 3px 4px 5px');
        equal(element.getAttribute('style'), before, 'invalid shorthand leaves all old components');
        equal(style.removeProperty('margin'), '1px 2px', 'removal returns prior shorthand');
        for (const name of ['margin', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left']) {
            equal(style.getPropertyValue(name), '', 'all mapped values removed');
            equal(style.getPropertyPriority(name), '', 'all mapped priorities removed');
        }
        equal(style.getPropertyValue('width'), '12px', 'unrelated property retained');
    "#,
    );
}

#[test]
fn partial_and_mixed_shorthands_are_removed_despite_empty_old_serialization() {
    successful(
        r#"
        const style = methods(document.getElementById('target').style);
        for (const raw of ['margin-left:3px!important;width:8px',
                          'margin-top:1px!important;margin-right:2px;margin-bottom:1px;margin-left:2px;width:8px']) {
            style.cssText = raw;
            equal(style.getPropertyValue('margin'), '', 'partial or mixed shorthand cannot serialize');
            equal(style.getPropertyPriority('margin'), '', 'not all constituents present and important');
            equal(style.removeProperty('margin'), '', 'return old empty shorthand serialization');
            for (const name of ['margin-top', 'margin-right', 'margin-bottom', 'margin-left']) {
                equal(style.getPropertyValue(name), '', 'remaining partial/mixed constituents still removed');
            }
            equal(style.getPropertyValue('width'), '8px', 'unrelated width retained');
        }
    "#,
    );
}

#[test]
fn other_shorthands_replace_mixed_priorities_and_return_canonical_values() {
    successful(
        r#"
        const style = methods(document.getElementById('target').style);
        style.cssText = 'flex-grow:9!important;flex-shrink:8;flex-basis:7px!important';
        style.setProperty('flex', '2 1 13px', 'important');
        equal(style.getPropertyValue('flex'), '2 1 13px', 'generic flex replacement');
        equal(style.getPropertyPriority('flex'), 'important', 'all flex priorities replaced');
        style.setProperty('flex', '2 1 13px');
        equal(style.getPropertyPriority('flex'), '', 'same shorthand clears all priorities');
        equal(style.removeProperty('flex'), '2 1 13px', 'flex removal serialized return');
        for (const name of ['flex-grow', 'flex-shrink', 'flex-basis']) {
            equal(style.getPropertyValue(name), '', 'all flex components removed');
        }
        style.setProperty('flex-flow', 'wrap-reverse column-reverse', 'important');
        equal(style.getPropertyPriority('flex-flow'), 'important', 'flex-flow priority');
        equal(style.removeProperty('flex-flow'), 'column-reverse wrap-reverse', 'canonical order not input echo');
        style.setProperty('opacity', '.5');
        equal(style.removeProperty('opacity'), '0.5', 'canonical numeric removal');
    "#,
    );
}

#[test]
fn escaped_style_tracks_external_attribute_replacement_removal_and_new_mutations() {
    let reply = successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        const raw = ' WIDTH : 3px !important ; height:7px ';
        element.setAttribute('style', raw);
        equal(style.getPropertyPriority('width'), 'important', 'escaped wrapper reads new attribute');
        equal(element.getAttribute('style'), raw, 'priority read does not rewrite raw');
        equal(style.removeProperty('height'), '7px', 'remove uses externally written value');
        equal(style.getPropertyValue('height'), '', 'removed value no longer present');
        element.removeAttribute('style');
        equal(style.getPropertyPriority('width'), '', 'raw removal clears old declaration');
        equal(style.removeProperty('width'), '', 'raw removal leaves no cached width');
        equal(element.getAttribute('style'), null, 'empty remove does not recreate style');
        style.setProperty('width', '19px', 'important');
        equal(element.style, style, 'escaped style identity retained');
        equal(element.getAttribute('style'), 'width: 19px !important;', 'actual owner mutated');
    "#,
    );
    let document = projected(&reply);
    let target = document.query_selector(0, "#target").unwrap().unwrap();
    assert_eq!(
        document.nodes[target].attr("style"),
        Some("width: 19px !important;")
    );
}

#[test]
fn detached_style_remains_live_and_reinsertion_keeps_real_attribute_state() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        style.setProperty('width', '12px', 'important');
        document.body.removeChild(element);
        equal(element.style, style, 'detachment preserves live wrapper identity');
        equal(style.removeProperty('width'), '12px', 'detached removal');
        style.setProperty('height', '8px', 'important');
        equal(style.getPropertyPriority('height'), 'important', 'detached priority');
        document.body.appendChild(element);
        equal(element.getAttribute('style'), 'height: 8px !important;', 'reinserted actual attribute');
        equal(element.style, style, 'reinsertion does not replace held wrapper');
    "#,
    );
}

#[test]
fn conversion_reentry_reads_current_state_after_all_arguments_and_detachment() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        style.setProperty('width', '1px');
        let order = '';
        style.setProperty({ toString() {
            order += 'n'; style.setProperty('height', '6px'); return 'width';
        } }, { toString() {
            order += 'v'; element.setAttribute('style', 'width:2px;height:8px');
            document.body.removeChild(element); return '3px';
        } }, { toString() {
            order += 'p'; style.setProperty('height', '9px'); return 'important';
        } });
        equal(order, 'nvp', 'all conversions completed in order');
        equal(style.getPropertyValue('width'), '3px', 'outer mutation after reentry');
        equal(style.getPropertyValue('height'), '9px', 'outer reads latest inner attribute');
        equal(style.getPropertyPriority('width'), 'important', 'outer importance');
        equal(style.removeProperty({ toString() {
            style.setProperty('width', '4px'); return 'width';
        } }), '4px', 'remove obtains old value after conversion');
        equal(style.getPropertyPriority({ toString() {
            style.setProperty('height', '10px', 'important'); return 'height';
        } }), 'important', 'priority reads post-conversion mutation');
        document.body.appendChild(element);
        equal(element.style, style, 'reentered detached wrapper survives');
        equal(element.getAttribute('style'), 'height: 10px !important;', 'only current declarations retained');
    "#,
    );
}

#[test]
fn conversion_throw_preserves_completed_inner_effects_and_exact_thrown_object() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        const sentinel = { owned: true };
        style.setProperty('width', '12px');
        let caught;
        try { style.setProperty('width', '99px', { toString() {
            element.setAttribute('style', ' WIDTH : 12px ; height:8px ');
            throw sentinel;
        } }); } catch (error) { caught = error; }
        equal(caught, sentinel, 'priority throw identity');
        equal(element.getAttribute('style'), ' WIDTH : 12px ; height:8px ', 'no outer canonicalization after throw');
        equal(style.getPropertyValue('height'), '8px', 'completed inner effect retained');
        caught = undefined;
        try { style.removeProperty({ toString() {
            style.setProperty('height', '9px', 'important'); throw sentinel;
        } }); } catch (error) { caught = error; }
        equal(caught, sentinel, 'remove name throw identity');
        equal(style.getPropertyValue('width'), '12px', 'failed outer removal not committed');
        equal(style.getPropertyPriority('height'), 'important', 'inner importance change retained');
    "#,
    );
}

#[test]
fn native_methods_and_internal_removal_do_not_consult_author_prototype_getters() {
    successful(
        r#"
        methods(document.getElementById('target').style);
        let calls = 0;
        for (const name of ['setProperty', 'removeProperty', 'getPropertyPriority', 'getPropertyValue']) {
            Object.defineProperty(Object.prototype, name, { configurable: true, get() {
                calls++; throw new Error('author prototype method getter');
            } });
        }
        const style = methods(document.getElementById('target').style);
        style.setProperty('width', '12px', 'important');
        equal(style.getPropertyPriority('width'), 'important', 'native priority');
        equal(style.getPropertyValue('width'), '12px', 'native value');
        style.setProperty('width', '', 'bad');
        equal(style.getPropertyValue('width'), '', 'internal removal is not author dispatch');
        style.setProperty('height', '7px');
        equal(style.removeProperty('height'), '7px', 'native explicit removal');
        equal(calls, 0, 'no inherited author method getter invoked');
        for (const name of ['setProperty', 'removeProperty', 'getPropertyPriority', 'getPropertyValue']) {
            delete Object.prototype[name];
        }
    "#,
    );
}

#[test]
fn structural_membership_preserves_raw_attribute_and_does_not_invoke_author_getters() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        const raw = 'width:12px; not a declaration ;';
        element.setAttribute('style', raw);
        let calls = 0;
        Object.defineProperty(Object.prototype, 'getPropertyPriority', { configurable: true, get() {
            calls++; throw new Error('inherited method getter');
        } });
        for (const name of ['setProperty', 'removeProperty', 'getPropertyPriority', 'getPropertyValue']) {
            check(name in style, 'method membership');
        }
        equal('notAnOwnedMethod' in style, false, 'unknown method not fabricated');
        equal(element.getAttribute('style'), raw, 'membership does not rewrite raw');
        equal(calls, 0, 'membership does not invoke inherited getter');
        delete Object.prototype.getPropertyPriority;
    "#,
    );
}

#[test]
fn input_byte_ceiling_is_enforced_before_invalid_and_unknown_noops() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        style.setProperty('width', '12px');
        const raw = element.getAttribute('style'), edge = 'q'.repeat(16384), large = edge + 'q';
        style.setProperty(edge, '1px');
        equal(style.removeProperty(edge), '', 'name at ceiling is unknown not oversized');
        equal(style.getPropertyPriority(edge), '', 'priority name at ceiling');
        style.setProperty('width', edge);
        style.setProperty('width', '1px', edge);
        equal(element.getAttribute('style'), raw, 'at-ceiling invalid semantic values are no-ops');
        hostError(() => style.setProperty(large, '1px'), 'oversize set name');
        hostError(() => style.removeProperty(large), 'oversize remove name');
        hostError(() => style.getPropertyPriority(large), 'oversize priority-query name');
        hostError(() => style.setProperty('width', large), 'oversize value');
        hostError(() => style.setProperty('unknown-owned-name', large), 'unknown name does not waive value limit');
        hostError(() => style.setProperty('width', '1px', large), 'oversize priority');
        hostError(() => style.setProperty('width', '', large), 'removal does not waive priority limit');
        equal(element.getAttribute('style'), raw, 'every oversize operation preserves old attribute');
        style.setProperty('height', '7px');
        equal(style.getPropertyValue('height'), '7px', 'ordinary errors leave realm usable');
    "#,
    );
}

#[test]
fn utf8_limits_apply_after_scalar_conversion_and_canonical_overflow_is_atomic() {
    successful(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        style.setProperty('width', '12px');
        const raw = element.getAttribute('style');
        const exact = 'é'.repeat(8192), over = exact + 'x';
        style.setProperty('width', exact);
        hostError(() => style.setProperty('width', over), 'UTF8 byte limit not UTF16 unit count');
        hostError(() => style.setProperty('width', '1px', over), 'priority UTF8 limit');
        const lone = String.fromCharCode(0xd800);
        hostError(() => style.setProperty('width', lone.repeat(5462)), 'replacement scalar consumes three bytes');
        hostError(() => style.setProperty('--big', 'x'.repeat(16380)), 'canonical output exceeds ceiling although input fits');
        equal(element.getAttribute('style'), raw, 'canonical overflow does not partially replace owner');
        equal(style.getPropertyValue('--big'), '', 'oversize canonical custom declaration never committed');
        equal(style.removeProperty('width'), '12px', 'ordinary failures do not poison later removal');
    "#,
    );
}

#[test]
fn actual_lifecycle_and_repeated_clicks_retain_methods_state_and_unchanged_work() {
    let (mut realm, loaded) = loaded(
        r#"
        const element = document.getElementById('target'), style = methods(element.style);
        const savedSet = style.setProperty, savedRemove = style.removeProperty;
        const savedPriority = style.getPropertyPriority;
        style.setProperty('width', '12px', 'important');
        let stages = '', clicks = 0;
        document.addEventListener('DOMContentLoaded', function() {
            stages += 'd'; style.setProperty('height', '7px');
        });
        window.addEventListener('load', function() {
            stages += 'l';
            equal(style.getPropertyPriority('width'), 'important', 'load sees initial importance');
            equal(style.getPropertyValue('height'), '7px', 'load sees DCL edit');
        });
        document.getElementById('button').onclick = function(event) {
            event.preventDefault(); clicks++;
            equal(stages, 'dl', 'actual lifecycle tasks completed in order');
            equal(element.style, style, 'escaped style survives real checkpoints');
            equal(style.setProperty, savedSet, 'set method identity survives');
            equal(style.removeProperty, savedRemove, 'remove method identity survives');
            equal(style.getPropertyPriority, savedPriority, 'priority method identity survives');
            savedSet.call(style, 'width', clicks === 1 ? '27px' : '31px', clicks === 1 ? 'important' : '');
            equal(savedRemove.call(style, 'height'), clicks === 1 ? '7px' : '', 'later removal return');
            equal(savedPriority.call(style, 'width'), clicks === 1 ? 'important' : '', 'later priority');
            document.title = 'methods click ' + clicks;
        };
    "#,
    );
    let first = click(&mut realm, &loaded);
    assert_eq!(projected(&first).title, "methods click 1");
    let first_document = projected(&first);
    let target = first_document
        .query_selector(0, "#target")
        .unwrap()
        .unwrap();
    assert_eq!(
        first_document.nodes[target].attr("style"),
        Some("width: 27px !important;")
    );
    let second = click(&mut realm, &first);
    assert_eq!(projected(&second).title, "methods click 2");
    let second_document = projected(&second);
    let target = second_document
        .query_selector(0, "#target")
        .unwrap()
        .unwrap();
    assert_eq!(
        second_document.nodes[target].attr("style"),
        Some("width: 31px;")
    );
}

#[test]
fn saved_methods_do_not_strongly_retain_their_first_style_receiver() {
    let (mut realm, loaded) = loaded(
        r#"
        var weakStyle, savedSet, savedRemove, savedPriority;
        (() => {
            const detached = document.createElement('span'), style = methods(detached.style);
            style.setProperty('width', '21px');
            savedSet = style.setProperty; savedRemove = style.removeProperty;
            savedPriority = style.getPropertyPriority; weakStyle = new WeakRef(style);
        })();
        document.getElementById('button').onclick = function(event) {
            event.preventDefault();
            equal(weakStyle.deref(), undefined, 'method cache does not retain first wrapper');
            const other = methods(document.createElement('span').style);
            savedSet.call(other, 'width', '34px', 'important');
            equal(savedPriority.call(other, 'width'), 'important', 'saved priority uses current receiver');
            equal(savedRemove.call(other, 'width'), '34px', 'saved removal uses current receiver');
            document.title = 'method wrappers released';
        };
    "#,
    );
    // Real Init, DOMContentLoaded and load checkpoints have already completed.
    // Do not replace the realm or manufacture ClearKeptObjects for this check.
    boa_gc::force_collect();
    let reply = click(&mut realm, &loaded);
    assert_eq!(projected(&reply).title, "method wrappers released");
}
