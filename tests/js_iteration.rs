//! Independently authored local for-in/switch regressions; no website source or
//! alternate JavaScript engine supplies these expectations. ECMA-262 5.1
//! sections 12.6.4, 12.7, 12.8, 12.11 and 12.14:
//! https://262.ecma-international.org/5.1/
//! Snapshot ordering/addition behavior and resource caps are mgbrowser policy.

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
        .unwrap_or_else(|error| panic!("Local iteration case failed: {error}\n{source}"))
}
fn yes(source: &str) {
    assert_eq!(value(source), Value::Bool(true), "{source}");
}
fn text(source: &str, expected: &str) {
    assert_eq!(value(source), Value::text(expected), "{source}");
}

#[test]
fn for_in_evaluates_rhs_once_and_reference_again_for_each_key() {
    text(
        "var trace='',slot={},seen={};
         function rhs(){trace+='R';return {first:1,second:2};}
         function target(){trace+='L';return slot;}
         function property(){trace+='K';return 'key';}
         for(target()[property()] in rhs()){trace+='B';seen[slot.key]=true;}
         trace+':'+seen.first+':'+seen.second;",
        "RLKBLKB:true:true",
    );
    text(
        "var trace='',slot={};function target(){trace+='L';return slot;}
         for(target().key in {}){trace+='B';}
         for(target().key in null){trace+='B';}
         for(target().key in undefined){trace+='B';}trace;",
        "",
    );
    // Assignment reference uses the current target, not one cached before RHS.
    yes(
        "var first={},second={},target=first,n=0;
         for(target.key in {a:1,b:2}){n++;target=second;}
         n===2 && typeof first.key==='string' && typeof second.key==='string' && first.key!==second.key;",
    );
}

#[test]
fn var_initializer_runs_before_rhs_even_when_no_iteration_occurs() {
    for rhs in ["null", "undefined", "{}"] {
        text(
            &format!(
                "var trace='';for(var key=(trace+='I','seed') in (trace+='R',{rhs})){{trace+='B';}}trace+':'+key;"
            ),
            "IR:seed",
        );
    }
    text(
        "var trace='';for(var key=(trace+='I','seed') in (trace+='R',{leaf:1})){trace+='B'+key;}trace;",
        "IRBleaf",
    );
    // An initializer is not a per-key assignment and runs even before RHS fails.
    text(
        "var trace='';function rhs(){trace+='R';throw 5;}
         try{for(var key=(trace+='I','seed') in rhs()){trace+='B';}}catch(e){trace+='C'+e;}
         trace+':'+key;",
        "IRC5:seed",
    );
}

#[test]
fn for_in_declarations_hoist_but_assign_through_the_catch_scope() {
    yes(
        "var key='global';function run(){var before=key;for(var key in {}){}return before===undefined && key===undefined;}run() && key==='global';",
    );
    text(
        "function run(){var inside;try{throw 'caught';}catch(key){
           for(var key='initial' in {leaf:1}){inside=key;}
         }return inside+':'+typeof key;}run();",
        "leaf:undefined",
    );
    text(
        "var key='outer',inside='';try{throw 'caught';}catch(key){
           for(var key='initial' in null){}inside=key;
         }inside+':'+key;",
        "initial:outer",
    );
    text(
        "function run(){var inside;try{throw 'caught';}catch(key){
           eval('for(var key in {leaf:1}){inside=key;}');
         }return inside+':'+typeof key;}run();",
        "leaf:undefined",
    );
}

#[test]
fn null_undefined_and_scalar_primitives_have_no_own_enumerable_keys() {
    yes(
        "var count=0,key='unchanged';for(key in null)count++;for(key in undefined)count++;
         for(key in 8)count++;for(key in true)count++;for(key in false)count++;
         count===0 && key==='unchanged';",
    );
    yes("var count=0;for(var key in Object(8))count++;for(key in Object(true))count++;count===0;");
}

#[test]
fn own_and_inherited_names_are_unique_and_shadowed_by_nearer_properties() {
    yes(
        "var ancestor={far:1,shared:2},parent=Object.create(ancestor);parent.near=3;parent.shared=4;
         var object=Object.create(parent);object.own=5;object.shared=6;
         var seen={},count=0,duplicate=false;
         for(var key in object){if(seen[key])duplicate=true;seen[key]=object[key];count++;}
         count===4 && !duplicate && seen.far===1 && seen.near===3 && seen.own===5 && seen.shared===6;",
    );
    yes(
        "var object=Object.create(null);object.only=1;var count=0;for(var key in object){count++;}count===1 && key==='only';",
    );
}

#[test]
fn host_enumeration_is_explicitly_unsupported_without_host_access() {
    let mut runtime = Runtime::new();
    runtime.set_global("external", Value::Host("document".into()));
    let error = runtime
        .execute("for(var key in external){}", &mut NoIo)
        .unwrap_err();
    assert!(
        error.contains("Unsupported") && error.contains("host"),
        "{error}"
    );
}

#[test]
fn nonenumerable_regex_and_array_metadata_shadow_enumerable_ancestors() {
    yes(
        "Object.prototype.source=1;Object.prototype.global=1;Object.prototype.ignoreCase=1;
         Object.prototype.multiline=1;Object.prototype.lastIndex=1;
         var regex=/x/g;regex.extra=2;var count=0,seen='';
         for(var key in regex){count++;seen=key;}count===1 && seen==='extra';",
    );
    yes("Object.prototype.length=8;var array=[3],count=0,seen='';
         for(var key in array){count++;seen=key;}count===1 && seen==='0';");
}

#[test]
fn arrays_visit_present_indices_and_custom_properties_but_not_holes_or_length() {
    yes(
        "var array=[7,,9];array.note=11;array.length=6;var seen={},count=0;
         for(var key in array){seen[key]=array[key];count++;}
         count===3 && seen['0']===7 && seen['2']===9 && seen.note===11 && !('length' in seen) && !('1' in seen);",
    );
    // Deterministic project ordering: numeric virtual indices precede stored
    // insertion-ordered keys, followed by the nearest prototype's own keys.
    text(
        "Array.prototype.inherited=1;var array=[];array[3]=1;array[1]=1;array.z=1;array.a=1;
         var names=[];for(var key in array){names.push(key);}names.join(',');",
        "1,3,z,a,inherited",
    );
}

#[test]
fn functions_and_native_functions_expose_custom_but_not_intrinsic_keys() {
    yes(
        "Object.prototype.length=1;Object.prototype.name=1;Object.prototype.prototype=1;
         Function.prototype.inherited=2;function sample(a,b){}sample.extra=3;
         var seen={},count=0;for(var key in sample){seen[key]=sample[key];count++;}
         count===2 && seen.extra===3 && seen.inherited===2;",
    );
    yes(
        "Object.prototype.keys=1;Object.prototype.create=1;Object.prototype.getPrototypeOf=1;
         Object.extra=2;var seen={},count=0;for(var key in Object){seen[key]=Object[key];count++;}
         count===1 && seen.extra===2;",
    );
}

#[test]
fn primitive_and_boxed_strings_enumerate_utf16_indices_not_codepoints() {
    for string in ["'😀x'", "Object('😀x')"] {
        text(
            &format!(
                "var input={string},keys=[],units=[];for(var key in input){{keys.push(key);units.push(input[key].charCodeAt(0));}}keys.join(',')+':'+units.join(',');"
            ),
            "0,1,2:55357,56832,120",
        );
    }
    yes(
        "Object.prototype.length=1;var string=Object('x');string.extra=2;
         var seen={},count=0;for(var key in string){seen[key]=string[key];count++;}
         count===2 && seen['0']==='x' && seen.extra===2;",
    );
    text(
        "var string=Object('ab');Object.keys(string).join(',')+':'+Object.getOwnPropertyNames(string).join(',');",
        "0,1:0,1,length",
    );
}

#[test]
fn deleted_future_keys_are_skipped_without_revisiting_any_name() {
    // Deletes every remaining candidate without depending on enumeration order.
    yes("var object={a:1,b:2,c:3},count=0;
         for(var key in object){count++;delete object.a;delete object.b;delete object.c;}
         count===1;");
    yes(
        "var parent={a:1,b:2,c:3},object=Object.create(parent),count=0;
         for(var key in object){count++;delete parent.a;delete parent.b;delete parent.c;}
         count===1;",
    );
}

#[test]
fn added_names_wait_for_the_next_enumeration_snapshot() {
    yes("var object={a:1,b:2},seen={},count=0;
         for(var key in object){seen[key]=true;count++;object['new'+key]=3;}
         var next=0;for(key in object){next++;}
         count===2 && seen.a && seen.b && next===4;");
    // This is the explicit original-owner snapshot policy, not a claim that
    // ECMAScript mandates this result for all prototype mutations.
    text(
        "var parent={later:1},object=Object.create(parent);object.first=1;var names=[];
         for(var key in object){names.push(key);object.later=2;}
         names.join(',');",
        "first",
    );
}

#[test]
fn for_in_break_continue_and_stacked_labels_target_the_correct_loop() {
    text(
        "var trace='',updates=0;
         outer:for(var i=0;i<2;i++,updates++){
           first:second:for(var key in {only:1}){trace+=i;continue outer;}
           trace+='wrong';
         }trace+':'+updates;",
        "01:2",
    );
    yes("var count=0;first:second:for(var key in {a:1,b:2}){count++;continue first;}count===2;");
    text(
        "var trace='';outer:for(var key in {a:1,b:2}){
           for(var inner in {one:1,two:2}){trace+='i';break;}
           trace+='o';break outer;
         }trace;",
        "io",
    );
}

#[test]
fn switch_uses_strict_equality_and_never_coerces_selector_objects() {
    yes(
        "var count=0,object={valueOf:function(){count++;return 1;},toString:function(){count++;return '1';}},result='';
         switch(object){case 1:result='number';break;case {}:result='other';break;case object:result='same';break;}
         count===0 && result==='same';",
    );
    text(
        "function choose(input){switch(input){case '1':return 'string';case 1:return 'number';case true:return 'bool';case null:return 'null';case undefined:return 'undefined';default:return 'none';}}
         choose(1)+':'+choose('1')+':'+choose(true)+':'+choose(null)+':'+choose(undefined);",
        "number:string:bool:null:undefined",
    );
    text(
        "var result='';switch(0/0){case 0/0:result+='wrong';break;default:result+='nan';}
         switch(-0){case 0:result+='zero';break;default:result+='wrong';}result;",
        "nanzero",
    );
}

#[test]
fn switch_evaluates_discriminant_once_and_stops_selectors_after_match() {
    text(
        "var trace='';function mark(name,value){trace+=name;return value;}
         switch(mark('D',2)){
           case mark('a',1):trace+='A';break;
           case mark('b',2):trace+='B';
           case mark('c',3):trace+='C';
         }trace;",
        "DabBC",
    );
    text(
        "var trace='';function fail(){trace+='X';throw 7;}
         switch(1){case 1:trace+='A';case fail():trace+='B';}trace;",
        "AB",
    );
    text(
        "var trace='';function fail(){trace+='X';throw 7;}
         try{switch(2){case 1:trace+='wrong';break;case fail():trace+='wrong';default:trace+='wrong';}}catch(e){trace+='C'+e;}trace;",
        "XC7",
    );
}

#[test]
fn default_in_the_middle_waits_for_all_selectors_and_falls_through() {
    for (input, expected) in [(1, "DaAFBC"), (2, "DabBC"), (9, "DabcFBC")] {
        text(
            &format!(
                "var trace='';function mark(name,value){{trace+=name;return value;}}
                 switch(mark('D',{input})){{
                   case mark('a',1):trace+='A';
                   default:trace+='F';
                   case mark('b',2):trace+='B';
                   case mark('c',3):trace+='C';
                 }}trace;"
            ),
            expected,
        );
    }
    text(
        "var trace='';switch(3){default:trace+='D';case 1:trace+='A';case 2:trace+='B';}trace;",
        "DAB",
    );
    text(
        "var trace='';switch(3){case 1:trace+='A';default:trace+='D';}trace;",
        "D",
    );
}

#[test]
fn switch_var_declarations_hoist_across_unselected_clauses() {
    yes("var hidden='global';function run(){var before=hidden;
           switch(0){case 1:var hidden=7;break;default:break;}
           return before===undefined && hidden===undefined;
         }run() && hidden==='global';");
    yes(
        "function run(){switch(0){case 1:for(var key in {one:1}){}break;default:break;}return key===undefined;}run();",
    );
}

#[test]
fn switch_break_is_local_while_continue_and_labels_reach_enclosing_loops() {
    text(
        "var trace='',updates=0;outer:for(var i=0;i<3;i++,updates++){
           switch(i){case 0:trace+='a';break;case 1:trace+='b';continue;default:trace+='c';break outer;}
           trace+='x';
         }trace+':'+updates;",
        "axbc:2",
    );
    text(
        "var trace='';label:switch(1){case 1:for(var i=0;i<2;i++){trace+='L';break;}trace+='S';break label;default:trace+='wrong';}trace;",
        "LS",
    );
    text(
        "var trace='';switch(1){case 1:switch(2){case 2:trace+='I';break;}trace+='O';break;}trace;",
        "IO",
    );
}

#[test]
fn eval_preserves_completion_values_through_empty_cases_and_breaks() {
    for (source, expected) in [
        ("3;switch(99){case 1:4;}", 3.0),
        ("3;switch(99){}", 3.0),
        ("switch(1){case 1:7;break;}", 7.0),
        ("switch(1){case 1:7;case 2:break;}", 7.0),
        ("switch(1){case 1:7;default:;case 2:break;}", 7.0),
        ("switch(1){case 0:4;default:;case 1:7;case 2:break;}", 7.0),
        ("label:switch(1){case 1:7;case 2:break label;}", 7.0),
        ("3;for(var key in {}){}", 3.0),
        ("for(var key in {one:1}){7;break;}", 7.0),
        ("for(var key in {one:1}){7;continue;}", 7.0),
    ] {
        assert_eq!(
            value(&format!("eval('{source}');")),
            Value::Number(expected),
            "{source}"
        );
    }
    assert_eq!(value("eval('switch(9){}');"), Value::Undefined);
}

#[test]
fn finally_preserves_or_overrides_iteration_and_switch_abrupt_completions() {
    text(
        "var trace='';for(var key in {one:1}){try{trace+='T';break;}finally{trace+='F';}trace+='wrong';}trace;",
        "TF",
    );
    text(
        "var trace='';for(var i=0;i<2;i++){
           switch(i){case 0:try{trace+='T';break;}finally{trace+='F';continue;}
           default:trace+='D';break;}trace+='A';
         }trace;",
        "TFDA",
    );
    text(
        "function run(){for(var key in {one:1}){try{return 'T';}finally{break;}}return 'B';}run();",
        "B",
    );
    text(
        "function run(){switch(1){case 1:try{break;}finally{return 'F';}}return 'wrong';}run();",
        "F",
    );
    assert_eq!(
        value("eval('switch(1){case 1:try{7;break;}finally{9;}}');"),
        Value::Number(7.0)
    );
    assert_eq!(
        value("eval('for(var key in {one:1}){try{7;continue;}finally{9;}}');"),
        Value::Number(7.0)
    );
}

#[test]
fn invalid_iteration_and_switch_scripts_reject_before_prefix_effects() {
    for invalid in [
        "for(var a,b in {}){}",
        "for(1 in {}){}",
        "for(a+b in {}){}",
        "for((a,b) in {}){}",
        "for(a=1 in {}){}",
        "switch(1){default:;default:;}",
        "switch(1){case 1:continue;}",
        "label:switch(1){case 1:continue label;}",
        "switch(1){case 1:break missing;}",
        "for(var key in {}){(function(){break;})();}",
        "for(var key in {}){(function(){continue;})();}",
        "switch(1){case 1:(function(){break;})();}",
        "outer:for(var key in {}){(function(){continue outer;})();}",
    ] {
        let mut runtime = Runtime::new();
        runtime.execute("var marker=1;", &mut NoIo).unwrap();
        let error = runtime
            .execute(&format!("marker=9;{invalid}"), &mut NoIo)
            .unwrap_err();
        assert!(error.contains("SyntaxError"), "{invalid}: {error}");
        assert_eq!(
            runtime.get_global("marker"),
            Value::Number(1.0),
            "{invalid}"
        );
        // Ordinary syntax failures do not latch resource exhaustion.
        assert_eq!(
            runtime.execute("marker+1;", &mut NoIo).unwrap(),
            Value::Number(2.0)
        );
    }
}

#[test]
fn dynamic_compilation_keeps_loop_labels_and_break_scopes_local() {
    yes("var caught=0;outer:for(var key in {one:1}){
           try{eval('continue outer;');}catch(e){if(e.name==='SyntaxError')caught++;}
           try{Function('break;');}catch(e){if(e.name==='SyntaxError')caught++;}
         }caught===2;");
    assert_eq!(
        value(
            "Function('var result=0;for(var key in {one:1}){switch(key){case \"one\":result++;break;}}return result;')();"
        ),
        Value::Number(1.0),
    );
}

fn assert_latched_limit(operation: &'static str, expected: &'static str) {
    let (send, receive) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let mut runtime = Runtime::new();
        let source = format!(
            "var caught=false,after=false;
             function run(){{try{{{operation}}}catch(e){{caught=true;return 1;}}finally{{after=true;return 2;}}}}
             run();after=true;"
        );
        let error = runtime.execute(&source, &mut NoIo).unwrap_err();
        let caught = runtime.get_global("caught");
        let after = runtime.get_global("after");
        let later = runtime.execute("after=true;42;", &mut NoIo).unwrap_err();
        let after_later = runtime.get_global("after");
        send.send((error, caught, after, later, after_later))
            .unwrap();
    });
    let (error, caught, after, later, after_later) = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("Local iteration fixture must terminate under its runtime budget");
    thread.join().unwrap();
    assert!(error.contains(expected), "Expected {expected}: {error}");
    assert_eq!(caught, Value::Bool(false));
    assert_eq!(after, Value::Bool(false));
    assert_eq!(later, error);
    assert_eq!(after_later, Value::Bool(false));
}

#[test]
fn switch_and_empty_for_in_consume_the_shared_uncatchable_fuel() {
    assert_latched_limit("while(true){switch(0){case 0:continue;}}", "fuel exhausted");
    assert_latched_limit("while(true){for(var key in null){}}", "fuel exhausted");
}

#[test]
fn enumeration_snapshot_allocations_accumulate_and_latch() {
    assert_latched_limit(
        "var name='x';for(var i=0;i<12;i++)name+=name;
         var object={};object[name]=1;while(true){for(var key in object){}}",
        "allocation budget exhausted",
    );
}
