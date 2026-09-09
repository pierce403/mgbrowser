//! Independent public-API acceptance for private nullish-member fault context.
//! The adopted docs/JAVASCRIPT.md contract preserves the thrown JavaScript Value
//! and evaluates no extra code. Only escaping host diagnostics gain the suffix.
//! Inputs are authored fixtures, not website source or a substitute JS engine.

use mg_butane::runtime::{Host, Runtime, Value};

const CAUGHT: &str = "TypeError: property access on null or undefined";
const UNCAUGHT: &str =
    "Uncaught JavaScript exception: TypeError: property access on null or undefined";

struct NoIo;
impl Host for NoIo {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        panic!("unexpected host get: {object}.{key}");
    }
    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("unexpected host set: {object}.{key}");
    }
    fn call(&mut self, name: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("unexpected host call: {name}");
    }
}

// Additive host-only context: keep the complete original member block and
// explicitly author each immediate producer expectation, never infer it from
// the source string or actual output. Property producers also specify their key.
fn assert_member(error: &str, operation: &str, base: &str, key: &str, producer: &str) {
    assert_eq!(
        error,
        format!(
            "{UNCAUGHT} [member operation={operation} base={base} key={key}] [producer kind={producer}]"
        )
    );
    assert!(error.is_ascii(), "{error:?}");
    assert!(error.len() <= 256, "{} bytes: {error:?}", error.len());
}

fn diagnostic(source: &str, operation: &str, base: &str, key: &str, producer: &str) -> Runtime {
    let mut runtime = Runtime::new();
    let error = runtime.execute(source, &mut NoIo).unwrap_err();
    assert_member(&error, operation, base, key, producer);
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none());
    runtime
}

fn yes(source: &str) {
    let value = Runtime::new()
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("diagnostic semantic case failed: {error}\n{source}"));
    assert_eq!(value, Value::Bool(true), "{source}");
}

fn plain(source: &str, message: &str) {
    let error = Runtime::new().execute(source, &mut NoIo).unwrap_err();
    assert_eq!(error, message, "{source}");
}

const OPERATIONS: &[(&str, &str)] = &[
    ("base.length;", "resolve-read"),
    ("base.length=1;", "resolve-write-target"),
    ("base.length+=1;", "resolve-compound-target"),
    ("base.length++;", "resolve-update-target"),
    ("delete base.length;", "resolve-delete-target"),
    ("base.length();", "resolve-call-target"),
    ("for(base.length in {only:1}){}", "resolve-for-in-target"),
];

#[test]
fn seven_origin_operations_distinguish_null_and_undefined() {
    for base in ["null", "undefined"] {
        for (expression, operation) in OPERATIONS {
            diagnostic(
                &format!("var base={base};{expression}"),
                operation,
                base,
                "length",
                "binding",
            );
        }
    }
}

#[test]
fn every_operation_preserves_the_exact_caught_primitive_string() {
    for base in ["null", "undefined"] {
        for (expression, _) in OPERATIONS {
            yes(&format!(
                "var base={base},caught=false,completed=false;try{{{expression}completed=true;}}catch(error){{caught=typeof error==='string'&&error==='{CAUGHT}'&&String(error)==='{CAUGHT}';}}caught&&!completed;"
            ));
        }
    }
}

#[test]
fn catch_and_explicit_rethrow_discard_the_annotation() {
    for base in ["null", "undefined"] {
        plain(
            &format!("try{{{base}.length;}}catch(error){{throw error;}}"),
            UNCAUGHT,
        );
        plain(
            &format!("var saved;try{{{base}.length;}}catch(error){{saved=error;}}throw saved;"),
            UNCAUGHT,
        );
    }
}

#[test]
fn catch_binding_assignment_does_not_attach_context_to_an_equal_string() {
    plain(
        "var saved;try{null.length;}catch(error){saved=error;}function later(){throw saved;}later();",
        UNCAUGHT,
    );
    plain(&format!("throw '{CAUGHT}';"), UNCAUGHT);
}

#[test]
fn normal_finally_preserves_origin_and_earlier_effects() {
    let runtime = diagnostic(
        "var before=0,after=0;try{before=1;null.length=7;}finally{after=2;}",
        "resolve-write-target",
        "null",
        "length",
        "expression",
    );
    assert_eq!(runtime.get_global("before"), Value::Number(1.0));
    assert_eq!(runtime.get_global("after"), Value::Number(2.0));
}

#[test]
fn an_inner_caught_fault_in_finally_does_not_replace_the_pending_fault() {
    let runtime = diagnostic(
        "var saved='';try{null.length;}finally{try{undefined.name();}catch(error){saved=error;}}",
        "resolve-read",
        "null",
        "length",
        "expression",
    );
    assert_eq!(runtime.get_global("saved"), Value::text(CAUGHT));
}

#[test]
fn replacement_throw_in_finally_replaces_context_as_well_as_value() {
    plain(
        "try{null.length;}finally{throw 'replacement';}",
        "Uncaught JavaScript exception: replacement",
    );
    diagnostic(
        "try{null.length;}finally{undefined.name();}",
        "resolve-call-target",
        "undefined",
        "name",
        "binding",
    );
    plain(
        "try{null.length;}finally{try{undefined.name;}catch(error){throw error;}}",
        UNCAUGHT,
    );
}

#[test]
fn return_and_loop_control_in_finally_override_pending_context() {
    yes(r#"
        function returned(){try{null.length;}finally{return 7;}}
        var value=returned(),turns=0;
        for(var i=0;i<2;i++){try{null.length;}finally{turns++;continue;}}
        outer:while(true){try{undefined.name;}finally{break outer;}}
        value===7 && turns===2;
    "#);
}

#[test]
fn a_new_fault_from_catch_gets_only_its_own_context() {
    diagnostic(
        "try{null.length;}catch(error){undefined.name=7;}",
        "resolve-write-target",
        "undefined",
        "name",
        "binding",
    );
}

#[test]
fn base_and_key_expressions_run_once_in_the_original_order() {
    let runtime = diagnostic(
        "var order='';function base(){order+='B';return null;}function key(){order+='K';return 'length';}base()[key()];",
        "resolve-read",
        "null",
        "length",
        "user-call",
    );
    assert_eq!(runtime.get_global("order"), Value::text("BK"));
}

#[test]
fn nullish_rejection_never_coerces_the_evaluated_object_key() {
    let runtime = diagnostic(
        r#"
        var touched=0,key={toString:function(){touched++;return 'length';},valueOf:function(){touched++;return 7;}};
        key[Symbol.toPrimitive]=function(hint){touched++;throw 'must not run';};
        null[key];
    "#,
        "resolve-read",
        "null",
        "<object>",
        "expression",
    );
    assert_eq!(runtime.get_global("touched"), Value::Number(0.0));
}

#[test]
fn base_or_key_expression_exceptions_keep_precedence_and_do_not_gain_context() {
    let mut runtime = Runtime::new();
    let error = runtime.execute(
        "var calls=0;function base(){throw 37;}function key(){calls++;return 'length';}base()[key()];",
        &mut NoIo,
    ).unwrap_err();
    assert_eq!(error, "Uncaught JavaScript exception: 37");
    assert_eq!(runtime.get_global("calls"), Value::Number(0.0));
    plain(
        "null[(function(){throw 38;})()];",
        "Uncaught JavaScript exception: 38",
    );
}

#[test]
fn an_inner_reference_failure_is_not_relabelled_by_outer_target_operations() {
    diagnostic(
        "null.length.name=7;",
        "resolve-read",
        "null",
        "length",
        "expression",
    );
    diagnostic(
        "null.length.name();",
        "resolve-read",
        "null",
        "length",
        "expression",
    );
    diagnostic(
        "delete null.length.name;",
        "resolve-read",
        "null",
        "length",
        "expression",
    );
    diagnostic(
        "null[undefined.name];",
        "resolve-read",
        "undefined",
        "name",
        "binding",
    );
}

#[test]
fn a_fault_from_key_coercion_keeps_the_inner_operation_context() {
    diagnostic(
        "var object={},key={toString:function(){return null.name;}};object[key]();",
        "resolve-read",
        "null",
        "name",
        "expression",
    );
}

#[test]
fn rejected_targets_precede_assignment_rhs_and_call_arguments() {
    for (expression, operation) in [
        ("null.length=(effects++,7);", "resolve-write-target"),
        ("null.length+=(effects++,7);", "resolve-compound-target"),
        ("null.length(effects++);", "resolve-call-target"),
    ] {
        let runtime = diagnostic(
            &format!("var effects=0;{expression}"),
            operation,
            "null",
            "length",
            "expression",
        );
        assert_eq!(runtime.get_global("effects"), Value::Number(0.0));
    }
    diagnostic(
        "++null.length;",
        "resolve-update-target",
        "null",
        "length",
        "expression",
    );
    diagnostic(
        "--undefined.length;",
        "resolve-update-target",
        "undefined",
        "length",
        "binding",
    );
}

#[test]
fn for_in_resolves_reference_only_after_rhs_and_only_for_an_actual_key() {
    let runtime = diagnostic(
        "var order='';function rhs(){order+='R';return {only:1};}function base(){order+='B';return null;}for(base().length in rhs()){order+='body';}",
        "resolve-for-in-target",
        "null",
        "length",
        "user-call",
    );
    assert_eq!(runtime.get_global("order"), Value::text("RB"));
    yes(
        "var calls=0;function base(){calls++;return null;}for(base().length in {}){}for(base().length in null){}calls===0;",
    );
}

#[test]
fn short_circuit_and_conditional_branches_never_report_unevaluated_accesses() {
    yes(r#"
        var effects=0;
        false && null[(effects++,'length')];
        true || undefined[(effects++,'name')];
        true ? 1 : null[(effects++,'message')];
        false ? undefined[(effects++,'call')] : 2;
        effects===0 && typeof undeclaredFixtureName==='undefined';
    "#);
    diagnostic(
        "typeof null.length;",
        "resolve-read",
        "null",
        "length",
        "expression",
    );
    diagnostic(
        "new null.constructor();",
        "resolve-read",
        "null",
        "constructor",
        "expression",
    );
}

#[test]
fn complete_declared_standard_key_vocabulary_is_named_without_guessing() {
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
        diagnostic(
            &format!("null['{key}'];"),
            "resolve-read",
            "null",
            key,
            "expression",
        );
    }
}

#[test]
fn key_matching_uses_exact_evaluated_ascii_contents_not_source_spelling() {
    diagnostic(
        r"null['\u006cength'];",
        "resolve-read",
        "null",
        "length",
        "expression",
    );
    diagnostic(
        "null['len'+'gth'];",
        "resolve-read",
        "null",
        "length",
        "expression",
    );
    for key in [
        "Length",
        "length-extra",
        " length",
        "length ",
        "0",
        "",
        "ℓength",
        "length😀",
    ] {
        diagnostic(
            &format!("null['{key}'];"),
            "resolve-read",
            "null",
            "<string>",
            "expression",
        );
    }
    diagnostic(
        r"null['length\u0000'];",
        "resolve-read",
        "null",
        "<string>",
        "expression",
    );
}

#[test]
fn primitive_key_categories_never_reveal_their_values_or_descriptions() {
    for (key, category) in [
        ("123456789", "<number>"),
        ("NaN", "<number>"),
        ("Infinity", "<number>"),
        ("true", "<boolean>"),
        ("false", "<boolean>"),
        ("null", "<null>"),
        ("undefined", "<undefined>"),
        ("Symbol('private-symbol-label')", "<symbol>"),
        ("Symbol.toPrimitive", "<symbol>"),
        ("{}", "<object>"),
        ("[]", "<object>"),
        ("Object('private-boxed-value')", "<object>"),
        ("function privateName(){}", "<function>"),
        ("parseInt", "<native>"),
    ] {
        diagnostic(
            &format!("null[{key}];"),
            "resolve-read",
            "null",
            category,
            "expression",
        );
    }
}

#[test]
fn arbitrary_strings_controls_urls_and_surrogates_are_redacted_by_default() {
    for key in [
        "private_identifier",
        "https://example.test/private?token=owned-secret#fragment",
        "owned-secret\\nforged-line",
        "owned-secret\\r\\nsecond-line",
        "\\uD800",
        "😀",
    ] {
        diagnostic(
            &format!("null['{key}'];"),
            "resolve-read",
            "null",
            "<string>",
            "expression",
        );
    }
}

#[test]
fn long_public_string_and_opaque_handle_keys_produce_the_same_fixed_bound() {
    for (value, category) in [
        (Value::String(vec![b'x' as u16; 16_384]), "<string>"),
        (
            Value::Host("private-host:https://example.test/private".into()),
            "<host>",
        ),
        (
            Value::Native("private.native:https://example.test/private".into()),
            "<native>",
        ),
    ] {
        let mut runtime = Runtime::new();
        runtime.set_global("key", value);
        let error = runtime.execute("null[key];", &mut NoIo).unwrap_err();
        assert_member(&error, "resolve-read", "null", category, "expression");
        assert!(!error.contains("private"));
        assert!(!error.contains("https://"));
    }
}

#[test]
fn caught_faults_and_unrelated_errors_do_not_leave_stale_execute_context() {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime
            .execute("try{null.length;}catch(error){}7;", &mut NoIo)
            .unwrap(),
        Value::Number(7.0)
    );
    assert_eq!(
        runtime.execute("throw 'later';", &mut NoIo).unwrap_err(),
        "Uncaught JavaScript exception: later"
    );
    let error = runtime.execute("undefined.name;", &mut NoIo).unwrap_err();
    assert_member(&error, "resolve-read", "undefined", "name", "binding");
    assert_eq!(
        runtime.execute("42;", &mut NoIo).unwrap(),
        Value::Number(42.0)
    );
    assert_eq!(
        runtime.execute("throw 'again';", &mut NoIo).unwrap_err(),
        "Uncaught JavaScript exception: again"
    );
}

#[test]
fn invoke_annotates_only_the_current_uncaught_fault_and_preserves_caught_values() {
    let mut runtime = Runtime::new();
    runtime.execute("function fail(){return null.length;}function recover(){try{undefined.name;}catch(error){return error;}}", &mut NoIo).unwrap();
    let fail = runtime.get_global("fail");
    let recover = runtime.get_global("recover");
    let error = runtime
        .invoke(fail.clone(), Value::Null, vec![], &mut NoIo)
        .unwrap_err();
    assert_member(&error, "resolve-read", "null", "length", "expression");
    assert_eq!(
        runtime
            .invoke(recover, Value::Null, vec![], &mut NoIo)
            .unwrap(),
        Value::text(CAUGHT)
    );
    let error = runtime
        .invoke(fail, Value::Null, vec![], &mut NoIo)
        .unwrap_err();
    assert_member(&error, "resolve-read", "null", "length", "expression");
    assert_eq!(
        runtime.execute("throw 9;", &mut NoIo).unwrap_err(),
        "Uncaught JavaScript exception: 9"
    );
}

#[test]
fn direct_indirect_dynamic_and_bound_calls_preserve_the_original_inner_fault() {
    for source in [
        "eval('null.length');",
        "(0,eval)('null.length');",
        "Function('return null.length;')();",
        "(function(){return null.length;}).bind(null)();",
        "(function(){return null.length;}).call(null);",
        "(function(){return null.length;}).apply(null,[]);",
    ] {
        diagnostic(source, "resolve-read", "null", "length", "expression");
    }
}

#[test]
fn internal_errors_and_genuine_error_diagnostics_do_not_gain_member_context() {
    plain(
        "var n=0;n();",
        "Uncaught JavaScript exception: TypeError: value is not callable",
    );
    plain(
        "Object.prototype.valueOf.call(null);",
        "Uncaught JavaScript exception: TypeError: valueOf on null or undefined",
    );
    plain(
        "throw new TypeError('owned error');",
        "Uncaught JavaScript exception: TypeError: owned error",
    );
    let mut runtime = Runtime::new();
    let error = runtime
        .invoke(
            Value::Native("Object.valueOf".into()),
            Value::Null,
            vec![],
            &mut NoIo,
        )
        .unwrap_err();
    assert_eq!(
        error,
        "Uncaught JavaScript exception: TypeError: valueOf on null or undefined"
    );
}

#[test]
fn named_versus_redacted_equal_size_keys_do_not_change_realm_accounting() {
    let mut reports = Vec::new();
    for (key, category) in [("length", "length"), ("secret", "<string>")] {
        let mut runtime = Runtime::new();
        let function = runtime
            .execute("function target(key){return null[key];}target;", &mut NoIo)
            .unwrap();
        let error = runtime
            .invoke(function, Value::Null, vec![Value::text(key)], &mut NoIo)
            .unwrap_err();
        assert_member(&error, "resolve-read", "null", category, "expression");
        reports.push(runtime.allocation_report());
    }
    assert_eq!(reports[0], reports[1]);
}

#[derive(Default)]
struct FixtureHost {
    gets: Vec<(String, String)>,
    calls: usize,
}
impl Host for FixtureHost {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        assert_eq!(object, "fixture");
        self.gets.push((object.into(), key.into()));
        match key {
            "missing" => Ok(Value::Undefined),
            "absent" => Ok(Value::Null),
            "denied" => Err("Fixture Host capability denied".into()),
            "method" => Ok(Value::Native("host.fixture.null".into())),
            _ => panic!("unexpected fixture key: {key}"),
        }
    }
    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("unexpected host set: {object}.{key}");
    }
    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String> {
        assert_eq!(name, "host.fixture.null");
        assert_eq!(this, Value::Host("fixture".into()));
        assert!(args.is_empty());
        self.calls += 1;
        Ok(Value::Null)
    }
}

#[test]
fn successful_missing_host_reads_remain_values_not_capability_errors() {
    let mut runtime = Runtime::new();
    let mut host = FixtureHost::default();
    runtime.set_global("fixture", Value::Host("fixture".into()));
    assert_eq!(
        runtime
            .execute(
                "fixture.missing===undefined&&fixture.absent===null;",
                &mut host
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(host.gets.len(), 2);
    assert_eq!(host.calls, 0);
}

#[test]
fn downstream_host_nullish_errors_add_no_host_reads_or_calls() {
    for (key, base) in [("missing", "undefined"), ("absent", "null")] {
        let mut runtime = Runtime::new();
        let mut host = FixtureHost::default();
        runtime.set_global("fixture", Value::Host("fixture".into()));
        let error = runtime
            .execute(&format!("fixture.{key}.length;"), &mut host)
            .unwrap_err();
        assert_member(
            &error,
            "resolve-read",
            base,
            "length",
            "host-get key=<string>",
        );
        assert_eq!(host.gets, vec![("fixture".into(), key.into())]);
        assert_eq!(host.calls, 0);
    }
}

#[test]
fn explicit_host_errors_remain_distinct_and_unannotated() {
    let mut runtime = Runtime::new();
    let mut host = FixtureHost::default();
    runtime.set_global("fixture", Value::Host("fixture".into()));
    let error = runtime
        .execute("fixture.denied.length;", &mut host)
        .unwrap_err();
    assert_eq!(
        error,
        "Uncaught JavaScript exception: Fixture Host capability denied"
    );
    assert_eq!(host.gets, vec![("fixture".into(), "denied".into())]);
    assert_eq!(host.calls, 0);
}

#[test]
fn host_call_result_is_checked_once_but_rejected_call_target_skips_arguments() {
    let mut runtime = Runtime::new();
    let mut host = FixtureHost::default();
    runtime.set_global("fixture", Value::Host("fixture".into()));
    let error = runtime
        .execute("fixture.method().length;", &mut host)
        .unwrap_err();
    assert_member(&error, "resolve-read", "null", "length", "host-call");
    assert_eq!(host.gets, vec![("fixture".into(), "method".into())]);
    assert_eq!(host.calls, 1);
    let error = runtime
        .execute("null.length(fixture.method());", &mut host)
        .unwrap_err();
    assert_member(
        &error,
        "resolve-call-target",
        "null",
        "length",
        "expression",
    );
    assert_eq!(host.gets.len(), 1);
    assert_eq!(host.calls, 1);
}

#[test]
fn host_symbol_key_rejection_is_not_mislabeled_as_nullish_context() {
    let mut runtime = Runtime::new();
    let mut host = FixtureHost::default();
    runtime.set_global("fixture", Value::Host("fixture".into()));
    let error = runtime
        .execute("fixture[Symbol('private-description')];", &mut host)
        .unwrap_err();
    assert_eq!(
        error,
        "Uncaught JavaScript exception: Unsupported JavaScript behavior: Symbol keys on host objects"
    );
    assert!(host.gets.is_empty());
    assert_eq!(host.calls, 0);
}
