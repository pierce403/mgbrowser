//! Independent authored call-receiver cases, not website source or an engine oracle.
//! ES5.1 sections 11.2.3, 10.4.3, 15.2.4.2, 15.2.4.4 and 15.3.4.3--5:
//! https://262.ecma-international.org/5.1/#sec-11.2.3
//! A non-property call supplies undefined; only an ordinary non-strict function
//! performs global substitution/primitive boxing. Builtins keep their receiver.
//! Host getter tests use the existing embedding API, not new script accessors.

use mg_deps::js::runtime::{Host, Runtime, Value};

struct NoIo;
impl Host for NoIo {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        panic!("unexpected Host get: {object}.{key}");
    }
    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("unexpected Host set: {object}.{key}");
    }
    fn call(&mut self, name: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("unexpected Host call: {name}");
    }
}

fn yes(source: &str) {
    let mut runtime = Runtime::new();
    let result = runtime
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("receiver case failed: {error}\n{source}"));
    assert_eq!(result, Value::Bool(true), "{source}");
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none(), "{report:?}");
}

fn detached_type_error(method: &str) {
    // Ordinary exceptions may remain strings; this adds no Error normalization.
    yes(&format!(
        "var method={method},caught=false,completed=false;try{{method();completed=true;}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&!completed;"
    ));
}

#[test]
fn bare_native_alias_matches_explicit_undefined_receiver() {
    yes(
        "var tag=Object.prototype.toString;var explicit=tag.call(undefined);explicit==='[object Undefined]'&&tag()===explicit;",
    );
}

#[test]
fn local_native_alias_does_not_inherit_the_callers_this() {
    yes(
        "function run(){var tag=Object.prototype.toString;return tag();}run.call({mark:7})==='[object Undefined]';",
    );
}

#[test]
fn comma_callee_discards_the_member_reference() {
    yes("(0,Object.prototype.toString)()==='[object Undefined]';");
}

#[test]
fn conditional_callee_discards_either_selected_member_reference() {
    for choice in ["true", "false"] {
        yes(&format!(
            "var holder={{tag:Object.prototype.toString}};({choice}?holder.tag:holder.tag)()==='[object Undefined]';"
        ));
    }
}

#[test]
fn returned_native_callable_has_no_implicit_factory_receiver() {
    yes("function factory(){return Object.prototype.toString;}factory()()==='[object Undefined]';");
}

#[test]
fn explicit_call_and_apply_keep_nullish_native_receivers_distinct() {
    yes(
        "var tag=Object.prototype.toString;tag.call(undefined)==='[object Undefined]'&&tag.apply(undefined,[])==='[object Undefined]'&&tag.call(null)==='[object Null]'&&tag.apply(null,[])==='[object Null]';",
    );
}

#[test]
fn detached_object_value_of_rejects_undefined_receiver() {
    detached_type_error("Object.prototype.valueOf");
}

#[test]
fn detached_generic_string_method_rejects_before_stringifying_a_global_object() {
    detached_type_error("String.prototype.toUpperCase");
}

#[test]
fn detached_generic_array_concat_rejects_instead_of_appending_a_global_object() {
    detached_type_error("Array.prototype.concat");
}

#[test]
fn detached_branded_string_methods_remain_type_errors() {
    for method in ["String.prototype.valueOf", "String.prototype.toString"] {
        detached_type_error(method);
    }
}

#[test]
fn ordinary_non_strict_functions_still_receive_the_global_object() {
    yes(
        "function receiver(){return this;}var holder={method:receiver};receiver()===globalThis&&(0,holder.method)()===globalThis&&(true?holder.method:holder.method)()===globalThis&&receiver.call(undefined)===globalThis&&receiver.call(null)===globalThis;",
    );
}

#[test]
fn ordinary_function_primitive_boxing_keeps_identity_and_utf16_rules() {
    yes(
        r"function receiver(){return this;}var a=receiver.call(7),b=receiver.call(7),s=receiver.call('\uD800x'),f=receiver.call(false);a!==b&&a.valueOf()===7&&b.valueOf()===7&&Object.getPrototypeOf(a)===Number.prototype&&s.valueOf()==='\uD800x'&&f.valueOf()===false;",
    );
}

#[test]
fn member_and_grouped_member_calls_keep_the_original_receiver() {
    yes(
        "function receiver(){return this;}var owner={method:receiver,tag:Object.prototype.toString};owner.method()===owner&&(owner.method)()===owner&&((owner.method))()===owner&&owner.tag()==='[object Object]'&&(owner.tag)()==='[object Object]';",
    );
}

#[test]
fn native_primitive_member_receivers_are_not_replaced_or_preboxed() {
    yes(
        r"'ab'.toUpperCase()==='AB'&&'\uD800x'.valueOf()==='\uD800x'&&(7).toString()==='7'&&false.valueOf()===false;",
    );
}

#[test]
fn bound_native_receiver_survives_alias_member_and_comma_calls() {
    yes(
        "var tag=Object.prototype.toString,undefinedTag=tag.bind(undefined),nullTag=tag.bind(null),holder={method:undefinedTag};undefinedTag()==='[object Undefined]'&&holder.method()==='[object Undefined]'&&(0,holder.method)()==='[object Undefined]'&&nullTag()==='[object Null]';",
    );
}

#[test]
fn bound_ordinary_receiver_is_not_overridden_by_the_call_expression() {
    yes(
        "function receiver(){return this;}var owner={mark:4},bound=receiver.bind(owner),holder={method:bound};bound()===owner&&holder.method()===owner&&(0,holder.method)()===owner&&bound.call(null)===owner&&receiver.bind(undefined)()===globalThis;",
    );
}

#[test]
fn direct_eval_preserves_caller_lexical_environment_and_this() {
    yes(
        "var value='global';function run(){var value='local';return eval('value')==='local'&&eval('this')===this;}run.call({mark:8});",
    );
}

#[test]
fn indirect_eval_keeps_global_scope_and_global_this() {
    yes(
        "var value='global';function run(){var value='local',alias=eval,holder={method:eval};return alias('value')==='global'&&(0,eval)('this')===globalThis&&(true?eval:eval)('this')===globalThis&&holder.method('this')===globalThis;}run.call({mark:9});",
    );
}

#[test]
fn arguments_run_before_noncallable_rejection_but_not_a_nullish_reference() {
    yes(
        "var effects=0,caught=0,holder={method:undefined};try{holder.method(effects++);}catch(error){caught++;}try{null.method(effects++);}catch(error){caught++;}effects===1&&caught===2;",
    );
}

#[test]
fn callee_base_key_and_arguments_evaluate_once_in_order() {
    yes(
        "var order='',owner={method:function(a,b){order+='T';return this===owner&&a===1&&b===2;}};function base(){order+='B';return owner;}function key(){order+='K';return 'method';}function argument(label,value){order+=label;owner.method=0;return value;}var result=base()[key()](argument('A',1),argument('C',2));result&&order==='BKACT';",
    );
}

#[test]
fn argument_exception_precedes_detached_native_execution() {
    yes(
        "var tag=Object.prototype.toString,effects=0,caught=false;function argument(){effects++;throw 'argument';}try{tag(argument());}catch(error){caught=error==='argument';}caught&&effects===1;",
    );
}

#[test]
fn call_result_nullish_fault_does_not_run_later_arguments_or_change_recovery() {
    yes(
        "var order='',caught=false;function base(){order+='B';return undefined;}function key(){order+='K';return 'method';}try{base()[key()](order+='A');}catch(error){caught=error==='TypeError: property access on null or undefined';}caught&&order==='BK';",
    );
}

struct OrderedGetter {
    result: Value,
    order: String,
    reject: bool,
}
impl Host for OrderedGetter {
    fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
        panic!("getter fixture uses only registered native callbacks");
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("getter fixture never assigns its accessor");
    }
    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "host.fixture.get" => {
                self.order.push('G');
                assert_eq!(this, Value::Object(0), "actual global getter receiver");
                assert!(args.is_empty());
                if self.reject {
                    Err("authored getter rejection".into())
                } else {
                    Ok(self.result.clone())
                }
            }
            "host.fixture.argument" => {
                self.order.push('A');
                assert!(args.is_empty());
                // This test does not prescribe the separate embedding policy for
                // detached Host callbacks; it observes only evaluation order.
                Ok(Value::Number(2.0))
            }
            _ => panic!("unexpected native callback: {name}"),
        }
    }
}

fn getter_fixture(reject: bool) -> (Runtime, OrderedGetter) {
    let mut runtime = Runtime::new();
    let target = runtime
        .execute(
            "function Target(value){return this===globalThis&&value===2;}Target;",
            &mut NoIo,
        )
        .unwrap();
    runtime.set_global_accessor(
        "entry",
        "fixture",
        "entry",
        Value::Native("host.fixture.get".into()),
    );
    runtime.set_global("argument", Value::Native("host.fixture.argument".into()));
    (
        runtime,
        OrderedGetter {
            result: target,
            order: String::new(),
            reject,
        },
    )
}

#[test]
fn global_accessor_get_runs_once_before_arguments_and_preserves_its_receiver() {
    let (mut runtime, mut host) = getter_fixture(false);
    assert_eq!(
        runtime.execute("entry(argument());", &mut host).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(host.order, "GA");
    assert!(runtime.allocation_report().is_valid());
}

#[test]
fn throwing_callee_getter_prevents_arguments_and_allows_later_recovery() {
    let (mut runtime, mut host) = getter_fixture(true);
    assert_eq!(
        runtime
            .execute("entry(argument());", &mut host)
            .unwrap_err(),
        "Uncaught JavaScript exception: authored getter rejection"
    );
    assert_eq!(host.order, "G");
    assert_eq!(
        runtime.execute("42;", &mut host).unwrap(),
        Value::Number(42.0)
    );
    assert!(runtime.allocation_report().first_rejected.is_none());
}
