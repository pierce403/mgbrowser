//! Authored local dynamic-compilation regressions, using no page or challenge
//! source and no alternate JavaScript engine. References: ECMA-262 5.1 sections
//! 10.4.2, 10.5, 15.1.2.1 and 15.3.2.1: https://262.ecma-international.org/5.1/
//! Resource-limit behavior is mgbrowser policy, not ECMAScript conformance.

use mg_deps::js::runtime::{Host, Runtime, Value};
use std::{sync::mpsc, time::Duration};

struct NoIo;
impl Host for NoIo {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        panic!("Unexpected host read: {object}.{key}");
    }
    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("Unexpected host write: {object}.{key}");
    }
    fn call(&mut self, name: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("Unexpected host call: {name}");
    }
}

fn value(source: &str) -> Value {
    Runtime::new()
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("Local dynamic-code case failed: {error}\n{source}"))
}

fn text(source: &str, expected: &str) {
    assert_eq!(value(source), Value::text(expected), "{source}");
}

#[test]
fn function_call_and_constructor_compile_callable_bodies() {
    for source in [
        "Function('a','b','return a+b;')(2,3);",
        "var f=new Function('a,b','return a+b;');f(2,3);",
        "Function('a,b','c','return a+b+c;')(1,1,3);",
        "Function('return 5;')();",
    ] {
        assert_eq!(value(source), Value::Number(5.0), "{source}");
    }
    assert_eq!(value("Function()();"), Value::Undefined);
    assert_eq!(value("var f=new Function();f();"), Value::Undefined);
    assert_eq!(
        value("Function('a','a','return a;')(1,2);"),
        Value::Number(2.0),
    );
    assert_eq!(
        value("var Box=Function('v','this.value=v;');var box=new Box(13);box.value;"),
        Value::Number(13.0),
    );
}

#[test]
fn function_arguments_are_evaluated_then_coerced_in_parameter_body_order() {
    text(
        "var trace='';
         function argument(name,source){trace+='e'+name;return {toString:function(){trace+='s'+name;return source;}};}
         var f=Function(argument('1','a'),argument('2','b'),argument('3','return a+b;'));
         trace+':'+f(4,5);",
        "e1e2e3s1s2s3:9",
    );
    text(
        "var trace='',caught=0;
         var parameter={toString:function(){trace+='p';throw 11;}};
         var body={toString:function(){trace+='b';return 'return 1;';}};
         try{Function(parameter,body);}catch(e){caught=e;}
         trace+':'+caught;",
        "p:11",
    );
    // Even an invalid parameter grammar is checked only after body ToString.
    text(
        "var trace='',caught=false;
         try{Function('bad)',{toString:function(){trace+='body';return 'return 1;';}});}catch(e){caught=true;}
         trace+':'+caught;",
        "body:true",
    );
}

#[test]
fn function_construction_does_not_execute_the_body() {
    text(
        "var marker=0;var f=Function('marker=3;return marker;');var before=marker;var after=f();before+':'+after;",
        "0:3",
    );
}

#[test]
fn constructed_function_uses_global_scope_not_the_callers_closure() {
    text(
        "var place='global';function build(){var place='caller';return Function('return place;');}build()();",
        "global",
    );
    text(
        "function build(){var secret=7;return new Function('return typeof secret;');}build()();",
        "undefined",
    );
    assert_eq!(
        value("Function('seed','return function(){return seed;};')(8)();"),
        Value::Number(8.0),
    );
    assert_eq!(
        value("Function('return this.mark;').call({mark:6});"),
        Value::Number(6.0),
    );
}

#[test]
fn direct_eval_reads_and_changes_caller_variables_and_uses_caller_this() {
    text(
        "var x=1;function run(){var x=2;eval('x=x+3;var added=7;');return x+':'+added+':'+eval('this.mark');}
         run.call({mark:9})+':'+x;",
        "5:7:9:1",
    );
    // Parentheses preserve the eval identifier reference, unlike a comma.
    assert_eq!(
        value("var x=1;function run(){var x=9;return (eval)('x');}run();"),
        Value::Number(9.0),
    );
}

#[test]
fn eval_function_declarations_capture_the_current_lexical_environment() {
    assert_eq!(
        value(
            "function make(){var local=7;eval('function read(){return local;}');local=9;return read;}make()();"
        ),
        Value::Number(9.0),
    );
    text(
        "function run(){var name='function';try{throw 'caught';}catch(name){
             eval(\"var created='added';function read(){return name;}\");
         }return created+':'+read()+':'+name;}run();",
        "added:caught:function",
    );
}

#[test]
fn eval_var_declarations_in_catch_use_function_scope_but_initializers_resolve_lexically() {
    text(
        "function run(){var e='function',seen='';try{throw 'caught';}catch(e){
             eval(\"var e='inside';var made=8;\");seen=e;
         }return seen+':'+e+':'+made;}run();",
        "inside:function:8",
    );
    text(
        "function run(){try{throw 4;}catch(e){eval('var made=7;');}return made+':'+typeof e;}
         run()+':'+typeof made;",
        "7:undefined:undefined",
    );
}

#[test]
fn later_global_var_and_function_declarations_treat_eval_bindings_differently() {
    // Separate script executions matter: a same-script ordinary declaration is
    // hoisted before the eval and would never create a configurable binding.
    let mut variables = Runtime::new();
    variables
        .execute("eval('var created=1;');", &mut NoIo)
        .unwrap();
    assert_eq!(
        variables
            .execute("var created;delete created;", &mut NoIo)
            .unwrap(),
        Value::Bool(true),
    );
    assert_eq!(variables.get_global("created"), Value::Undefined);

    let mut functions = Runtime::new();
    functions
        .execute("eval('var created=1;');", &mut NoIo)
        .unwrap();
    assert_eq!(
        functions
            .execute("function created(){return 7;}delete created;", &mut NoIo)
            .unwrap(),
        Value::Bool(false),
    );
    assert_eq!(
        functions.execute("created();", &mut NoIo).unwrap(),
        Value::Number(7.0),
    );
}

#[test]
fn indirect_eval_forms_use_global_scope() {
    for call in [
        "alias('place')",
        "holder.invoke('place')",
        "globalThis.eval('place')",
        "(0,eval)('place')",
        "(true?eval:eval)('place')",
        "(eval=eval)('place')",
        "eval.call(null,'place')",
        "eval.apply(null,['place'])",
    ] {
        text(
            &format!(
                "var place='global';function run(){{var place='local',alias=eval,holder={{invoke:eval}};return {call};}}run();"
            ),
            "global",
        );
    }
    text(
        "var place='global';function run(){var place='local',alias=eval;alias(\"var dynamicGlobal=8;place='changed';\");return place;}
         run()+':'+place+':'+dynamicGlobal;",
        "local:changed:8",
    );
}

#[test]
fn indirect_eval_uses_global_this_not_the_call_receiver() {
    assert_eq!(
        value(
            "var mark=3;function run(){return eval.call({mark:8},'this.mark');}run.call({mark:9});"
        ),
        Value::Number(3.0),
    );
}

#[test]
fn eval_returns_non_string_inputs_unchanged_without_coercion() {
    assert_eq!(value("eval();"), Value::Undefined);
    assert_eq!(value("eval(null);"), Value::Null);
    assert_eq!(value("eval(8);"), Value::Number(8.0));
    assert_eq!(value("eval(true);"), Value::Bool(true));
    assert_eq!(
        value("var object={toString:function(){throw 1;}};eval(object)===object;"),
        Value::Bool(true),
    );
    assert_eq!(
        value("var f=function(){return 4;};eval(f)===f;"),
        Value::Bool(true),
    );
}

#[test]
fn replaced_or_shadowed_eval_is_not_treated_as_the_intrinsic() {
    text(
        "eval=function(source){return 'replacement:'+source;};function run(){var local=9;return eval('local');}run();",
        "replacement:local",
    );
    text(
        "function run(eval){return eval('1+2');}run(function(source){return 'received:'+source;});",
        "received:1+2",
    );
    // Conversely, a local binding named eval holding the intrinsic is direct.
    assert_eq!(
        value(
            "var original=eval;function run(eval){var local=9;return eval('local');}run(original);"
        ),
        Value::Number(9.0),
    );
}

#[test]
fn eval_returns_completion_values_and_preserves_thrown_values() {
    assert_eq!(value("eval('1;2;');"), Value::Number(2.0));
    assert_eq!(value("eval('var unused;');"), Value::Undefined);
    assert_eq!(value("eval('3;var unused;');"), Value::Number(3.0));
    assert_eq!(value("eval('block:{7;break block;}');"), Value::Number(7.0));
    assert_eq!(
        value(
            "var token={};var same=false;try{eval('throw token;');}catch(e){same=e===token;}same;"
        ),
        Value::Bool(true),
    );
}

#[test]
fn malformed_eval_source_is_catchable_and_never_executes_its_prefix() {
    text(
        "var marker=0,caught=false;try{eval('marker=99;var = ;');}catch(e){caught=true;}
         caught+':'+marker+':'+eval('2+3');",
        "true:0:5",
    );
}

#[test]
fn dynamic_parse_failures_are_catchable_syntax_errors_with_messages() {
    for operation in [
        "eval('var =;')",
        "eval('return 1;')",
        "Function('bad)','return 1;')",
        "Function('return );')",
    ] {
        text(
            &format!(
                "var result='not thrown';try{{{operation};}}catch(e){{result=e.name+':'+(typeof e.message==='string'&&e.message.length>0);}}result;"
            ),
            "SyntaxError:true",
        );
    }
}

#[test]
fn return_break_and_continue_cannot_escape_the_eval_program() {
    assert_eq!(
        value(
            "function run(){var caught=false;try{eval('return 9;');}catch(e){caught=true;}return caught;}run();"
        ),
        Value::Bool(true),
    );
    assert_eq!(
        value(
            "var caught=false;outside:{try{eval('break outside;');}catch(e){caught=true;}}caught;"
        ),
        Value::Bool(true),
    );
    assert_eq!(
        value(
            "var caught=0;outside:for(var i=0;i<1;i++){try{eval('continue outside;');}catch(e){caught++;}}caught;"
        ),
        Value::Number(1.0),
    );
    assert_eq!(
        value("eval('function inner(){return 9;}inner();');"),
        Value::Number(9.0),
    );
    assert_eq!(
        value("eval('local:{break local;}while(false){break;}');"),
        Value::Undefined,
    );
}

#[test]
fn function_parameter_and_body_grammars_cannot_spill_into_other_statements() {
    for (parameters, body) in [
        ("a)", "marker=7;return 1;"),
        ("a;marker=7", "return 1;"),
        ("a = 1", "return a;"),
        ("...rest", "return 1;"),
        ("{value}", "return 1;"),
        ("a,b,", "return 1;"),
        ("/*", "*/ return 1;"),
        ("a) {return 0;} function extra(", "return 1;"),
        ("a", "return 1;} marker=7; function extra(){"),
    ] {
        let parameters = serde_json::to_string(parameters).unwrap();
        let body = serde_json::to_string(body).unwrap();
        text(
            &format!(
                "var marker=0,caught=false,constructed=false;try{{Function({parameters},{body});constructed=true;}}catch(e){{caught=true;}}caught+':'+constructed+':'+marker;"
            ),
            "true:false:0",
        );
    }
}

fn assert_latched_dynamic_limit(operation: impl Into<String>) -> String {
    let operation = operation.into();
    let code = operation.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let mut runtime = Runtime::new();
        let source = format!(
            "var caught=false,after=false;function limited(){{try{{{code}}}catch(e){{caught=true;return 1;}}finally{{return 2;}}}}limited();after=true;"
        );
        let result = runtime.execute(&source, &mut NoIo);
        let caught = runtime.get_global("caught");
        let after = runtime.get_global("after");
        let later = runtime.execute("after=true;42;", &mut NoIo);
        let after_later = runtime.get_global("after");
        sender
            .send((result, caught, after, later, after_later))
            .unwrap();
    });
    let (result, caught, after, later, after_later) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("Dynamic compilation/execution must terminate within the local test deadline");
    thread.join().expect("local evaluator thread");
    let error = result.expect_err("Resource exhaustion must bypass JS catch and finally return");
    assert!(
        error.contains("fuel") || error.contains("budget") || error.contains("limit"),
        "Expected a resource-limit diagnostic, got: {error}",
    );
    assert_eq!(caught, Value::Bool(false), "{operation}");
    assert_eq!(after, Value::Bool(false), "{operation}");
    assert_eq!(
        later.expect_err("Resource failure must remain latched"),
        error
    );
    assert_eq!(after_later, Value::Bool(false), "{operation}");
    error
}

#[test]
fn repeated_function_compilation_cannot_reset_or_catch_resource_limits() {
    assert_latched_dynamic_limit("while(true){Function('x','return x;');}");
}

#[test]
fn repeated_eval_compilation_cannot_reset_or_catch_resource_limits() {
    assert_latched_dynamic_limit("while(true){eval('1+1;');}");
}

#[test]
fn dynamically_compiled_body_execution_obeys_the_same_fuel_limit() {
    assert_latched_dynamic_limit("Function('while(true){}')();");
    assert_latched_dynamic_limit("eval('while(true){}');");
}

#[test]
fn caught_invalid_compilations_still_exhaust_the_cumulative_allocation_budget() {
    // Moderately sized invalid inputs force allocation accounting to matter
    // before loop fuel or the environment-count ceiling becomes the limit.
    let comment = "local invalid compilation budget fixture ".repeat(32);
    let invalid_eval = serde_json::to_string(&format!("var =; /*{comment}*/")).unwrap();
    let invalid_parameters =
        serde_json::to_string(&format!("not a parameter /*{comment}*/")).unwrap();
    for operation in [
        format!("eval({invalid_eval})"),
        format!("Function({invalid_parameters},'return 1;')"),
    ] {
        let error = assert_latched_dynamic_limit(format!(
            "while(true){{try{{{operation};}}catch(expectedSyntaxError){{}}}}"
        ));
        assert!(
            error.contains("allocation") || error.contains("budget"),
            "Invalid compilation must consume the cumulative allocation budget: {error}",
        );
    }
}
