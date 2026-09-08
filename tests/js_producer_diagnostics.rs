//! Authored public-runtime tests for one immediate producer, not provenance history.
//! No website input, substitute evaluator, public diagnostic-state API or cap changes.
use mg_deps::js::runtime::{Host, Runtime, Value};

const ORIGINAL: &str = "TypeError: property access on null or undefined";
const PREFIX: &str =
    "Uncaught JavaScript exception: TypeError: property access on null or undefined";

#[derive(Default)]
struct Fixture {
    gets: Vec<String>,
    calls: usize,
}
impl Host for Fixture {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        assert_eq!(object, "fixture");
        self.gets.push(key.into());
        match key {
            "length" => Ok(Value::Undefined),
            "name" => Ok(Value::Null),
            "method" => Ok(Value::Native("host.fixture.null".into())),
            "denied" => Err("authored host rejection".into()),
            _ => panic!("unexpected authored Host get"),
        }
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("diagnostic invoked an unexpected Host setter")
    }
    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String> {
        assert_eq!(name, "host.fixture.null");
        assert_eq!(this, Value::Host("fixture".into()));
        assert!(args.is_empty());
        self.calls += 1;
        Ok(Value::Null)
    }
}

fn assert_diagnostic(
    error: &str,
    operation: &str,
    base: &str,
    key: &str,
    kind: &str,
    producer_key: Option<&str>,
) {
    let producer_key = producer_key
        .map(|key| format!(" key={key}"))
        .unwrap_or_default();
    assert_eq!(
        error,
        format!(
            "{PREFIX} [member operation={operation} base={base} key={key}] [producer kind={kind}{producer_key}]"
        )
    );
    assert!(error.is_ascii());
    assert!(
        error.len() <= 256,
        "diagnostic exceeded its fixed ASCII bound"
    );
}

fn check(source: &str, base: &str, kind: &str, producer_key: Option<&str>) -> Runtime {
    let mut runtime = Runtime::new();
    let mut host = Fixture::default();
    let error = runtime.execute(source, &mut host).unwrap_err();
    assert_diagnostic(&error, "resolve-read", base, "name", kind, producer_key);
    assert!(host.gets.is_empty() && host.calls == 0);
    assert!(runtime.allocation_report().is_valid());
    runtime
}

fn yes(source: &str) {
    let mut host = Fixture::default();
    assert_eq!(
        Runtime::new().execute(source, &mut host).unwrap(),
        Value::Bool(true)
    );
    assert!(host.gets.is_empty() && host.calls == 0);
}

#[test]
fn missing_own_property_is_distinct_from_present_undefined_and_null() {
    check(
        "({}).length.name;",
        "undefined",
        "missing-property",
        Some("length"),
    );
    check(
        "({length:undefined}).length.name;",
        "undefined",
        "present-property",
        Some("length"),
    );
    check(
        "({length:null}).length.name;",
        "null",
        "present-property",
        Some("length"),
    );
}

#[test]
fn inherited_presence_and_nearer_undefined_shadow_are_observed_once() {
    check(
        "Object.create({length:undefined}).length.name;",
        "undefined",
        "present-property",
        Some("length"),
    );
    check(
        "var child=Object.create({length:null});child.length=undefined;child.length.name;",
        "undefined",
        "present-property",
        Some("length"),
    );
    check(
        "var child=Object.create({length:null});child.length=undefined;delete child.length;child.length.name;",
        "null",
        "present-property",
        Some("length"),
    );
    check(
        "Object.create(null).length.name;",
        "undefined",
        "missing-property",
        Some("length"),
    );
}

#[test]
fn array_holes_and_present_undefined_indices_do_not_collapse() {
    check(
        "Array(1)[0].name;",
        "undefined",
        "missing-property",
        Some("<string>"),
    );
    check(
        "[undefined][0].name;",
        "undefined",
        "present-property",
        Some("<string>"),
    );
    check(
        "Array.prototype[0]=undefined;Array(1)[0].name;",
        "undefined",
        "present-property",
        Some("<string>"),
    );
    // The existing primitive-string shortcut does not walk the prototype here;
    // do not claim it proved absence or silently expand its old semantics.
    check("'x'[1].name;", "undefined", "expression", None);
    check(
        "String.prototype[9]=null;'x'[9].name;",
        "undefined",
        "expression",
        None,
    );
}

#[test]
fn function_native_and_deleted_virtual_property_paths_report_actual_presence() {
    check(
        "function f(){};f.missing.name;",
        "undefined",
        "missing-property",
        Some("<string>"),
    );
    check(
        "Number.isNaN=undefined;Number.isNaN.name;",
        "undefined",
        "present-property",
        Some("<string>"),
    );
    check(
        "delete Number.isNaN;Number.isNaN.name;",
        "undefined",
        "missing-property",
        Some("<string>"),
    );
    check(
        "Number.privateName.name;",
        "undefined",
        "missing-property",
        Some("<string>"),
    );
}

#[test]
fn binding_reads_stop_attribution_in_globals_parameters_and_returned_values() {
    check(
        "var value=({}).length;value.name;",
        "undefined",
        "binding",
        None,
    );
    check(
        "function f(value){value.name;}f(({}).length);",
        "undefined",
        "binding",
        None,
    );
    check(
        "function f(){return ({}).length;}var value=f();value.name;",
        "undefined",
        "binding",
        None,
    );
    check("var value=null;value.name;", "null", "binding", None);
}

#[test]
fn expression_results_do_not_trace_selected_or_assigned_subexpressions() {
    for source in [
        "(0,({}).length).name;",
        "(true?({}).length:null).name;",
        "(false||({}).length).name;",
        "(true&&({}).length).name;",
        "var value;(value=({}).length).name;",
        "(void 0).name;",
    ] {
        check(source, "undefined", "expression", None);
    }
    check("null.name;", "null", "expression", None);
}

#[test]
fn user_call_reports_dispatch_not_its_return_expression() {
    check(
        "(function(){return ({}).length;})().name;",
        "undefined",
        "user-call",
        None,
    );
    check(
        "(function(){return null;})().name;",
        "null",
        "user-call",
        None,
    );
    check("(function(){})().name;", "undefined", "user-call", None);
}

#[test]
fn native_call_includes_call_apply_eval_and_dynamically_created_functions() {
    check("[].pop().name;", "undefined", "native-call", None);
    check(
        "(function(){return null;}).call(null).name;",
        "null",
        "native-call",
        None,
    );
    check(
        "(function(){return null;}).apply(null,[]).name;",
        "null",
        "native-call",
        None,
    );
    check("eval('null').name;", "null", "native-call", None);
    check(
        "Function('return null;')().name;",
        "null",
        "user-call",
        None,
    );
}

#[test]
fn bound_call_does_not_flatten_to_the_ultimate_target_kind() {
    check(
        "var f=(function(){return null;}).bind(null);f().name;",
        "null",
        "bound-call",
        None,
    );
    check(
        "var f=Array.prototype.pop.bind([]);f().name;",
        "undefined",
        "bound-call",
        None,
    );
    check(
        "var f=(function(){return null;}).bind(null).bind({});f().name;",
        "null",
        "bound-call",
        None,
    );
    check(
        "var f=(function(){return null;}).bind(null);f.call(null).name;",
        "null",
        "native-call",
        None,
    );
}

#[test]
fn host_get_is_not_claimed_to_be_a_missing_host_capability() {
    for (key, base) in [("length", "undefined"), ("name", "null")] {
        let mut runtime = Runtime::new();
        runtime.set_global("fixture", Value::Host("fixture".into()));
        let mut host = Fixture::default();
        let error = runtime
            .execute(&format!("fixture.{key}.name;"), &mut host)
            .unwrap_err();
        assert_diagnostic(&error, "resolve-read", base, "name", "host-get", Some(key));
        assert_eq!(host.gets, [key]);
        assert_eq!(host.calls, 0);
    }
}

#[test]
fn host_call_has_no_retained_method_name_or_receiver_handle() {
    let mut runtime = Runtime::new();
    runtime.set_global("fixture", Value::Host("fixture".into()));
    let mut host = Fixture::default();
    let error = runtime
        .execute("fixture.method().name;", &mut host)
        .unwrap_err();
    assert_diagnostic(&error, "resolve-read", "null", "name", "host-call", None);
    assert_eq!(host.gets, ["method"]);
    assert_eq!(host.calls, 1);
    assert!(!error.contains("fixture"));
}

#[test]
fn public_configured_getter_distinguishes_property_and_binding_reads() {
    for (source, kind, key) in [
        ("globalThis.length.name;", "getter-result", Some("length")),
        (
            "Object.create(globalThis).length.name;",
            "getter-result",
            Some("length"),
        ),
        ("length.name;", "binding", None),
    ] {
        let mut runtime = Runtime::new();
        let mut host = Fixture::default();
        let getter = runtime
            .execute(
                "var getterCalls=0;(function(){getterCalls++;return undefined;});",
                &mut host,
            )
            .unwrap();
        runtime.set_global_accessor("length", "fixture", "length", getter);
        let error = runtime.execute(source, &mut host).unwrap_err();
        assert_diagnostic(&error, "resolve-read", "undefined", "name", kind, key);
        assert_eq!(runtime.get_global("getterCalls"), Value::Number(1.0));
        assert!(host.gets.is_empty() && host.calls == 0);
    }
}

#[test]
fn successful_key_coercion_occurs_once_and_classifies_the_converted_key() {
    let runtime = check(
        "var calls=0,key={toString:function(){calls++;return 'length';}};({})[key].name;",
        "undefined",
        "missing-property",
        Some("length"),
    );
    assert_eq!(runtime.get_global("calls"), Value::Number(1.0));
    let runtime = check(
        "var calls=0,key={};key[Symbol.toPrimitive]=function(){calls++;return 'length';};({length:null})[key].name;",
        "null",
        "present-property",
        Some("length"),
    );
    assert_eq!(runtime.get_global("calls"), Value::Number(1.0));
}

#[test]
fn converted_scalar_keys_are_strings_while_symbol_identity_stays_redacted() {
    for key in [
        "0",
        "-42",
        "true",
        "false",
        "null",
        "undefined",
        "NaN",
        "Infinity",
    ] {
        check(
            &format!("({{}})[{key}].name;"),
            "undefined",
            "missing-property",
            Some("<string>"),
        );
    }
    check(
        "var key=Symbol('private description');({})[key].name;",
        "undefined",
        "missing-property",
        Some("<symbol>"),
    );
    check(
        "var key=Symbol('private description'),object={};object[key]=null;object[key].name;",
        "null",
        "present-property",
        Some("<symbol>"),
    );
}

#[test]
fn whitelist_is_shared_by_present_and_missing_property_producers() {
    let keys = [
        "prototype",
        "constructor",
        "length",
        "name",
        "message",
        "call",
        "apply",
        "bind",
        "toString",
        "valueOf",
        "forEach",
        "map",
        "filter",
        "some",
        "every",
        "reduce",
        "push",
        "pop",
        "shift",
        "unshift",
        "slice",
        "join",
        "concat",
        "indexOf",
        "includes",
        "reverse",
        "appendChild",
        "removeChild",
        "insertBefore",
        "remove",
        "addEventListener",
        "removeEventListener",
        "querySelector",
        "querySelectorAll",
        "getElementById",
        "getElementsByTagName",
        "getElementsByClassName",
        "createElement",
        "createTextNode",
        "setAttribute",
        "getAttribute",
        "hasAttribute",
        "removeAttribute",
        "textContent",
        "innerHTML",
        "innerText",
        "style",
        "classList",
        "className",
        "id",
        "parentNode",
        "parentElement",
        "firstChild",
        "lastChild",
        "nextSibling",
        "previousSibling",
        "ownerDocument",
        "documentElement",
        "head",
        "body",
        "children",
        "childNodes",
        "forms",
        "elements",
        "document",
        "navigator",
        "location",
        "href",
        "search",
        "cookie",
        "userAgent",
        "getComputedStyle",
        "onload",
        "onclick",
        "submit",
        "focus",
    ];
    for key in keys {
        check(
            &format!("Object.create(null)['{key}'].name;"),
            "undefined",
            "missing-property",
            Some(key),
        );
        check(
            &format!(
                "var object=Object.create(null);object['{key}']=undefined;object['{key}'].name;"
            ),
            "undefined",
            "present-property",
            Some(key),
        );
    }
}

#[test]
fn arbitrary_keys_and_names_cannot_forge_or_expand_the_suffix() {
    let mut runtime = Runtime::new();
    let mut host = Fixture::default();
    let reader = runtime
        .execute("(function(key){({})[key].name;});", &mut host)
        .unwrap();
    for key in [
        "private\n[producer kind=forged]https://fixture.invalid/".repeat(1000),
        "\u{2603}".repeat(4096),
    ] {
        let error = runtime
            .invoke(
                reader.clone(),
                Value::Undefined,
                vec![Value::text(&key)],
                &mut host,
            )
            .unwrap_err();
        assert_diagnostic(
            &error,
            "resolve-read",
            "undefined",
            "name",
            "missing-property",
            Some("<string>"),
        );
    }
    assert!(host.gets.is_empty() && host.calls == 0);
}

#[test]
fn later_key_effects_cannot_replace_the_already_observed_producer() {
    let runtime = check(
        "var object={length:undefined},calls=0;function key(){calls++;delete object.length;return 'name';}object.length[key()];",
        "undefined",
        "present-property",
        Some("length"),
    );
    assert_eq!(runtime.get_global("calls"), Value::Number(1.0));
    let runtime = check(
        "var object={},calls=0;function key(){calls++;object.length=null;return 'name';}object.length[key()];",
        "undefined",
        "missing-property",
        Some("length"),
    );
    assert_eq!(runtime.get_global("calls"), Value::Number(1.0));
}

#[test]
fn later_key_reentry_and_caught_failures_do_not_overwrite_local_origin() {
    let runtime = check(
        "var calls=0;function key(){try{[].pop().privateKey;}catch(e){calls++;}return 'name';}({length:undefined}).length[key()];",
        "undefined",
        "present-property",
        Some("length"),
    );
    assert_eq!(runtime.get_global("calls"), Value::Number(1.0));
    let runtime = check(
        "var calls=0;function key(){(function(){calls++;return null;})();return 'name';}({}).length[key()];",
        "undefined",
        "missing-property",
        Some("length"),
    );
    assert_eq!(runtime.get_global("calls"), Value::Number(1.0));
}

#[test]
fn an_inner_base_or_key_failure_keeps_its_own_origin() {
    let runtime = check(
        "var calls=0;function key(){calls++;return 'length';}({}).length.name[key()];",
        "undefined",
        "missing-property",
        Some("length"),
    );
    assert_eq!(runtime.get_global("calls"), Value::Number(0.0));
    check(
        "({length:undefined}).length[({}).body.name];",
        "undefined",
        "missing-property",
        Some("body"),
    );
    check(
        "(function(){({}).body.name;})().length;",
        "undefined",
        "missing-property",
        Some("body"),
    );
}

#[test]
fn outer_nullish_rejection_still_precedes_key_coercion() {
    let runtime = {
        let mut runtime = Runtime::new();
        let mut host = Fixture::default();
        let error = runtime
            .execute(
                "var calls=0,key={toString:function(){calls++;throw 1;}};({}).length[key];",
                &mut host,
            )
            .unwrap_err();
        assert_diagnostic(
            &error,
            "resolve-read",
            "undefined",
            "<object>",
            "missing-property",
            Some("length"),
        );
        runtime
    };
    assert_eq!(runtime.get_global("calls"), Value::Number(0.0));
}

#[test]
fn all_seven_member_operations_keep_the_same_immediate_producer() {
    for (tail, operation) in [
        (".name;", "resolve-read"),
        (".name=1;", "resolve-write-target"),
        (".name+=1;", "resolve-compound-target"),
        (".name++;", "resolve-update-target"),
        (".name();", "resolve-call-target"),
    ] {
        let error = Runtime::new()
            .execute(&format!("({{}}).length{tail}"), &mut Fixture::default())
            .unwrap_err();
        assert_diagnostic(
            &error,
            operation,
            "undefined",
            "name",
            "missing-property",
            Some("length"),
        );
    }
    for (source, operation) in [
        ("delete ({}).length.name;", "resolve-delete-target"),
        (
            "for(({}).length.name in {only:1}){}",
            "resolve-for-in-target",
        ),
    ] {
        let error = Runtime::new()
            .execute(source, &mut Fixture::default())
            .unwrap_err();
        assert_diagnostic(
            &error,
            operation,
            "undefined",
            "name",
            "missing-property",
            Some("length"),
        );
    }
}

#[test]
fn caught_value_and_explicit_rethrow_do_not_expose_or_retain_context() {
    yes(&format!(
        "var saved;try{{({{}}).length.name;}}catch(e){{saved=e;}}typeof saved==='string'&&saved==='{ORIGINAL}';"
    ));
    let error = Runtime::new()
        .execute(
            "try{({}).length.name;}catch(e){throw e;}",
            &mut Fixture::default(),
        )
        .unwrap_err();
    assert_eq!(error, PREFIX);
}

#[test]
fn normal_finally_retains_pending_origin_despite_an_inner_caught_failure() {
    let runtime = check(
        "var effects=0;try{({length:null}).length.name;}finally{try{[].pop().length;}catch(e){effects++;}}",
        "null",
        "present-property",
        Some("length"),
    );
    assert_eq!(runtime.get_global("effects"), Value::Number(1.0));
}

#[test]
fn finally_throw_or_return_replaces_both_value_and_producer() {
    check(
        "try{({length:null}).length.name;}finally{({}).body.name;}",
        "undefined",
        "missing-property",
        Some("body"),
    );
    let error = Runtime::new()
        .execute(
            "try{({}).length.name;}finally{throw 'replacement';}",
            &mut Fixture::default(),
        )
        .unwrap_err();
    assert_eq!(error, "Uncaught JavaScript exception: replacement");
    yes("function f(){try{({}).length.name;}finally{return 7;}}f()===7;");
}

#[test]
fn later_execute_and_public_invoke_have_no_stale_producer_state() {
    let mut runtime = check(
        "({}).length.name;",
        "undefined",
        "missing-property",
        Some("length"),
    );
    let mut host = Fixture::default();
    assert_eq!(
        runtime.execute("42;", &mut host).unwrap(),
        Value::Number(42.0)
    );
    let error = runtime.execute("null.name;", &mut host).unwrap_err();
    assert_diagnostic(&error, "resolve-read", "null", "name", "expression", None);
    let callback = runtime
        .execute("(function(value){value.name;});", &mut host)
        .unwrap();
    let error = runtime
        .invoke(callback, Value::Undefined, vec![Value::Null], &mut host)
        .unwrap_err();
    assert_diagnostic(&error, "resolve-read", "null", "name", "binding", None);
    assert_eq!(
        runtime.execute("throw 'last';", &mut host).unwrap_err(),
        "Uncaught JavaScript exception: last"
    );
}

#[test]
fn failed_host_calls_and_unresolved_identifiers_are_not_successful_producers() {
    let mut runtime = Runtime::new();
    runtime.set_global("fixture", Value::Host("fixture".into()));
    let mut host = Fixture::default();
    let error = runtime
        .execute("fixture.denied.name;", &mut host)
        .unwrap_err();
    assert_eq!(
        error,
        "Uncaught JavaScript exception: authored host rejection"
    );
    assert_eq!(host.gets, ["denied"]);
    assert_eq!(host.calls, 0);
    let error = runtime
        .execute("absentIdentifier.name;", &mut host)
        .unwrap_err();
    assert!(error.contains("ReferenceError") && !error.contains("[producer "));
}
