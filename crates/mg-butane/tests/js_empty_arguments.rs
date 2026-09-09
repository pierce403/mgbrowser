//! Independent semantic preservation cases for empty-only lazy arguments.
//! ES5.1 10.5/10.6 and 10.4.2 define the binding and eval relationships:
//! https://262.ecma-international.org/5.1/#sec-10.5
//! https://262.ecma-international.org/5.1/#sec-10.6
//! Keep the documented unmapped/indexed-storage approximation; these are not
//! allocation-benefit tests or an expansion of arguments descriptor support.

use mg_butane::runtime::{Host, Runtime, Value};

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

fn yes(source: &str) {
    let result = Runtime::new()
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("arguments case failed: {error}\n{source}"));
    assert_eq!(result, Value::Bool(true), "{source}");
}

#[test]
fn omitted_explicit_undefined_and_missing_formals_remain_distinct() {
    yes(r#"
        function take(first,missing){
            return {snapshot:arguments,first:first,missing:missing};
        }
        var empty=take(),one=take(undefined),two=take(undefined,undefined);
        empty.snapshot.length===0 && !(0 in empty.snapshot) &&
            one.snapshot.length===1 && one.snapshot.hasOwnProperty(0) &&
            one.snapshot[0]===undefined && !(1 in one.snapshot) &&
            two.snapshot.length===2 && two.snapshot.hasOwnProperty(1) &&
            empty.first===undefined && empty.missing===undefined &&
            one.missing===undefined;
    "#);
}

#[test]
fn repeated_reads_share_one_object_but_distinct_calls_do_not() {
    yes(r#"
        function take(){return [arguments,arguments,arguments===arguments];}
        var a=take(),b=take();a[0]===a[1] && a[2] && b[0]===b[1] && b[2] &&
            a[0]!==b[0] && a[0].length===0 && b[0].length===0;
    "#);
}

#[test]
fn empty_snapshot_has_arguments_brand_intrinsic_parent_and_callee() {
    yes(r#"
        function take(){return arguments;}var saved=take;
        var a=take(),parent=Object.prototype;take=function(){return 9;};
        typeof a==='object' && !Array.isArray(a) &&
            Object.getPrototypeOf(a)===parent && a.callee===saved &&
            a.hasOwnProperty('callee') && a.hasOwnProperty('length') &&
            Object.prototype.toString.call(a)==='[object Arguments]' &&
            a.length===0 && Object.getOwnPropertySymbols(a).length===0;
    "#);
    yes(r#"
        var fn=function local(){return arguments;};var a=fn();
        a.callee===fn && a.callee.name==='local';
    "#);
}

#[test]
fn typeof_void_and_boolean_read_routes_preserve_the_binding() {
    yes(r#"
        function take(){
            var type=typeof arguments,discarded=void arguments;
            return type==='object' && discarded===undefined && Boolean(arguments) &&
                !!arguments && (arguments ? true : false) && arguments.length===0;
        }take();
    "#);
}

#[test]
fn sequence_short_circuit_and_conditional_reads_keep_exact_identity() {
    yes(r#"
        function take(){
            var a=(0,arguments),b=false||arguments,c=true&&arguments;
            return a===b && b===c && (true?arguments:null)===a &&
                (false?null:arguments)===a && (arguments||null)===a;
        }take();
    "#);
}

#[test]
fn member_reflection_and_value_container_reads_observe_the_same_object() {
    yes(r#"
        function take(){
            var a={kept:arguments},b=[arguments],names=Object.getOwnPropertyNames(arguments);
            return a.kept===b[0] && a.kept===arguments &&
                ('length' in arguments) && ('callee' in arguments) &&
                !('0' in arguments) && names.length===2 && arguments['callee']===take &&
                Object.prototype.hasOwnProperty.call(arguments,'length');
        }take();
    "#);
}

#[test]
fn simple_assignment_replaces_only_the_local_arguments_binding() {
    yes(r#"
        var arguments='global';
        function take(){arguments={mark:7};return arguments.mark===7;}
        function absent(){arguments=undefined;return typeof arguments==='undefined';}
        take() && absent() && arguments==='global';
    "#);
}

#[test]
fn assignment_rhs_reads_and_aliases_survive_later_replacement() {
    yes(r#"
        function take(){
            var saved;arguments=(saved=arguments);var same=arguments===saved;
            arguments={replacement:true};saved.mark=9;
            return same && saved.length===0 && saved.callee===take && saved.mark===9 &&
                arguments.replacement && saved!==arguments;
        }take();
    "#);
}

#[test]
fn compound_assignment_and_updates_read_before_replacing_the_binding() {
    yes(r#"
        function append(){arguments+='!';return arguments==='[object Arguments]!';}
        function increment(){var old=arguments++;return old!==old && arguments!==arguments;}
        function decrement(){var value=--arguments;return value!==value && arguments!==arguments;}
        append() && increment() && decrement();
    "#);
}

#[test]
fn bare_var_declarations_and_unvisited_hoists_do_not_reset_arguments() {
    yes(r#"
        function take(){
            var arguments;var first=arguments;
            if(false){var arguments=9;}
            for(var arguments;false;){}
            switch(0){case 1:var arguments=11;}
            return arguments===first && first.length===0 && first.callee===take;
        }take();
    "#);
    yes("function take(){return arguments.length===0;var arguments;}take();");
}

#[test]
fn var_initializers_read_the_old_binding_or_replace_it_normally() {
    yes(r#"
        function same(){var arguments=arguments;return arguments.length===0 && arguments.callee===same;}
        function replace(){var arguments=13;return arguments===13;}
        function sequence(){var original=arguments,arguments={old:arguments};return arguments.old===original;}
        same() && replace() && sequence();
    "#);
}

#[test]
fn function_declarations_named_arguments_win_before_body_execution() {
    yes(r#"
        function take(){
            var before=arguments;return typeof before==='function' && before()===7 &&
                before===arguments;
            function arguments(){return 7;}
        }
        function last(){
            return arguments()===2;
            function arguments(){return 1;}function arguments(){return 2;}
        }
        take() && take(undefined) && last();
    "#);
}

#[test]
fn parameter_named_arguments_suppresses_the_special_binding_even_when_missing() {
    yes(r#"
        function take(arguments){return arguments;}
        function duplicate(arguments,arguments){return arguments;}
        function declared(arguments){var arguments;return arguments;}
        var object={};
        take()===undefined && take(undefined)===undefined && take(object)===object &&
            duplicate(7)===undefined && duplicate(7,8)===8 && declared()===undefined &&
            declared(object)===object;
    "#);
}

#[test]
fn deleting_the_binding_fails_before_and_after_observation() {
    yes(r#"
        function take(){
            var before=delete arguments,a=arguments,after=delete arguments;
            var inEval=eval('delete arguments');
            return !before && !after && !inEval && arguments===a && a.callee===take;
        }take() && take(undefined);
    "#);
}

#[test]
fn for_in_assignment_replaces_the_binding_without_creating_a_global() {
    yes(r#"
        var arguments='global';
        function reference(){for(arguments in {key:1}){}return arguments==='key';}
        function declared(){for(var arguments in {other:1}){}return arguments==='other';}
        reference() && declared() && arguments==='global';
    "#);
}

#[test]
fn catch_parameter_shadows_arguments_without_replacing_the_function_binding() {
    yes(r#"
        function take(){
            var seen=false;try{throw 7;}catch(arguments){
                seen=arguments===7;arguments=8;seen=seen && arguments===8;
            }
            return seen && arguments.length===0 && arguments.callee===take;
        }take();
    "#);
}

#[test]
fn direct_eval_reads_and_redeclares_the_same_arguments_binding() {
    yes(r#"
        function take(){
            var a=eval('arguments'),type=(eval)('typeof arguments');
            eval('var arguments;');
            return a===arguments && type==='object' && eval('arguments.callee')===take &&
                eval('arguments.length')===0 && eval('arguments===a');
        }take();
    "#);
}

#[test]
fn direct_eval_assignment_and_function_declaration_replace_the_local_binding() {
    yes(r#"
        function assign(){eval('arguments=17;');return arguments===17;}
        function initialized(){eval('var arguments=19;');return arguments===19;}
        function declared(){
            eval('function arguments(){return 23;}');
            return typeof arguments==='function' && arguments()===23;
        }
        assign() && initialized() && declared();
    "#);
}

#[test]
fn direct_eval_in_catch_keeps_lexical_initializer_and_variable_scope_distinct() {
    yes(r#"
        function take(){
            var caught=false;
            try{throw 'caught';}catch(arguments){
                eval("var arguments='changed';var made=31;");
                caught=arguments==='changed' && eval('arguments')==='changed';
            }
            return caught && made===31 && arguments.length===0 && arguments.callee===take;
        }take();
    "#);
}

#[test]
fn indirect_eval_observes_global_arguments_not_the_callers_snapshot() {
    yes(r#"
        var arguments='global',indirect=eval,holder={run:eval};
        function take(){
            var seen=indirect('arguments')==='global' &&
                (0,eval)('arguments')==='global' && holder.run('arguments')==='global';
            indirect("arguments='changed';");
            return seen && arguments.length===0 && arguments.callee===take;
        }take() && arguments==='changed';
    "#);
    yes(r#"
        function take(){return (0,eval)('typeof arguments')==='undefined' && arguments.length===0;}
        take();
    "#);
}

#[test]
fn dynamic_function_and_nested_functions_have_their_own_arguments() {
    yes(r#"
        function make(){return Function('return arguments;');}
        var generated=make(7),a=generated(),b=generated(undefined);
        a.length===0 && a.callee===generated && b.length===1 &&
            b.hasOwnProperty(0) && b[0]===undefined && b.callee===generated && a!==b;
    "#);
    yes(r#"
        function outer(){
            var outside=arguments;
            function inner(){return [arguments,outside];}
            var pair=inner();return pair[0]!==outside && pair[0].callee===inner &&
                pair[0].length===0 && pair[1]===outside && outside.callee===outer;
        }outer();
    "#);
}

#[test]
fn call_and_apply_distinguish_empty_lists_from_a_present_undefined_slot() {
    yes(r#"
        function take(){return arguments;}
        var direct=take(),called=take.call(null),empty=take.apply(null,[]);
        var omitted=take.apply(null),nil=take.apply(null,null),undef=take.apply(null,undefined);
        var present=take.call(null,undefined),hole=take.apply(null,Array(1));
        direct.length===0 && called.length===0 && empty.length===0 && omitted.length===0 &&
            nil.length===0 && undef.length===0 && direct!==called && called!==empty &&
            present.length===1 && present.hasOwnProperty(0) && present[0]===undefined &&
            hole.length===1 && hole.hasOwnProperty(0) && hole[0]===undefined &&
            called.callee===take && hole.callee===take;
    "#);
}

#[test]
fn new_calls_keep_snapshot_and_constructed_receiver_identities_separate() {
    yes(r#"
        function Keep(){this.saved=arguments;this.self=this;}
        var a=new Keep(),b=new Keep(undefined);
        a instanceof Keep && a.self===a && a.saved!==a && a.saved.callee===Keep &&
            a.saved.length===0 && b.saved.length===1 && b.saved.hasOwnProperty(0) &&
            b.saved[0]===undefined && a.saved!==b.saved;
    "#);
    yes(r#"
        function Replace(){return arguments;}var value=new Replace();
        value.callee===Replace && value.length===0 && !Array.isArray(value) &&
            Object.getPrototypeOf(value)===Object.prototype && !(value instanceof Replace);
    "#);
}

#[test]
fn escaped_aliases_and_rebound_lexical_bindings_survive_later_calls() {
    yes(r#"
        function make(){
            var saved=arguments;
            return {saved:saved,read:function(){return saved;},
                replace:function(value){saved=value;return saved;}};
        }
        var a=make(),b=make(),old=a.saved,other={};
        old!==b.saved && a.read()===old && b.read()===b.saved &&
            a.replace(other)===other && a.read()===other && old.length===0 &&
            old.callee===make && b.read()===b.saved;
    "#);
}

#[test]
fn recursive_empty_calls_do_not_replace_the_active_outer_snapshot() {
    yes(r#"
        var depth=0,inside;
        function take(){
            var own=arguments;
            if(depth===0){depth=1;inside=take();depth=0;}
            return own===arguments ? own : null;
        }
        var outside=take();outside!==inside && outside.length===0 && inside.length===0 &&
            outside.callee===take && inside.callee===take;
    "#);
}

#[test]
fn nonempty_utf16_symbol_and_object_values_retain_existing_unmapped_behavior() {
    yes(r#"
        var text='\uD800x\uDC00',symbol=Symbol('owned'),object={mark:1};
        function take(first,second,third){
            var snapshot=arguments;first='changed';snapshot[1]='snapshot';
            return snapshot.length===3 && snapshot[0]===text && snapshot[0].length===3 &&
                snapshot[0].charCodeAt(0)===55296 && snapshot[0].charCodeAt(2)===56320 &&
                second===symbol && snapshot[1]==='snapshot' && third===object &&
                snapshot[2]===object && snapshot.callee===take;
        }take(text,symbol,object);
    "#);
}

#[test]
fn snapshot_tag_concat_and_borrowed_indexed_methods_keep_current_behavior() {
    yes(r#"
        function take(){return arguments;}var a=take(),key=Symbol('key');
        a[key]=7;a[Symbol.toStringTag]='Custom';var whole=[].concat(a);
        var push=Array.prototype.push.call(a,'first'),removed=Array.prototype.pop.call(a);
        whole.length===1 && whole[0]===a && !Array.isArray(a) &&
            Object.prototype.toString.call(a)==='[object Custom]' && a[key]===7 &&
            Object.getOwnPropertySymbols(a).length===2 && push===1 && removed==='first' &&
            a.length===0 && a.callee===take;
    "#);
}

#[test]
fn ordinary_throws_and_dynamic_syntax_errors_preserve_recovery_and_escaped_values() {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime
            .execute(
                r#"
                    var saved,finished=false;
                    function fail(){try{throw arguments;}finally{finished=true;}}
                    try{fail();}catch(error){saved=error;}
                    saved.length===0 && saved.callee===fail && finished;
                "#,
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .execute(
                r#"
                    function recover(){
                        var caught=false;
                        try{eval('(');}catch(error){caught=String(error).indexOf('SyntaxError')>=0;}
                        return caught && arguments.length===0 && arguments.callee===recover;
                    }
                    recover() && saved.length===0 && saved.callee===fail;
                "#,
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn public_invoke_preserves_empty_nonempty_and_post_return_object_identity() {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute("function take(){return arguments;}take;", &mut NoIo)
        .unwrap();
    let first = runtime
        .invoke(function.clone(), Value::Null, vec![], &mut NoIo)
        .unwrap();
    let second = runtime
        .invoke(function.clone(), Value::Undefined, vec![], &mut NoIo)
        .unwrap();
    let present = runtime
        .invoke(function, Value::Null, vec![Value::Undefined], &mut NoIo)
        .unwrap();
    assert!(matches!(&first, Value::Object(_)));
    assert!(matches!(&second, Value::Object(_)));
    assert_ne!(first, second);
    runtime.set_global("first", first);
    runtime.set_global("second", second);
    runtime.set_global("present", present);
    assert_eq!(
        runtime
            .execute(
                "first.length===0 && second.length===0 && first!==second && first.callee===take && second.callee===take && present.length===1 && present.hasOwnProperty(0) && present[0]===undefined && present.callee===take;",
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
}
