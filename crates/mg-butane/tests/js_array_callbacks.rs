//! Independent authored acceptance for docs/ARRAY_CALLBACKS.md.
//! ES5.1 15.4.4.16--22: captured length, live HasProperty/Get, ordered calls.
//! No website input, replacement engine, public accessor syntax, or new Host API.

use mg_butane::runtime::{Host, Runtime, Value};

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

const METHODS: [&str; 7] = [
    "forEach",
    "map",
    "filter",
    "some",
    "every",
    "reduce",
    "reduceRight",
];

fn yes(source: &str) {
    let mut runtime = Runtime::new();
    let value = runtime
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("array callback case failed: {error}\n{source}"));
    assert_eq!(value, Value::Bool(true), "{source}");
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none(), "{report:?}");
}

#[test]
fn seven_methods_have_real_non_enumerable_writable_deletable_metadata() {
    for method in METHODS {
        yes(&format!(
            "var p=Array.prototype,method=p.{method};var present=p.hasOwnProperty('{method}')&&typeof method==='function'&&method.length===1&&Object.keys(p).indexOf('{method}')<0&&Object.getOwnPropertyNames(p).indexOf('{method}')>=0;p.{method}=7;var replaced=p.{method}===7;var removed=delete p.{method};present&&replaced&&removed&&!p.hasOwnProperty('{method}');"
        ));
    }
}

#[test]
fn foreach_passes_exact_arguments_original_object_and_receiver_in_order() {
    yes(r#"
        var source=[2,4],receiver={mark:7},trace='',valid=true;
        var result=source.forEach(function(value,index,object){
            valid=valid&&arguments.length===3&&this===receiver&&object===source&&typeof index==='number';
            trace+=index+':'+value+';';return 'ignored';
        },receiver);
        result===undefined&&valid&&trace==='0:2;1:4;';
    "#);
}

#[test]
fn map_preserves_holes_but_visits_explicit_undefined_and_trailing_length() {
    yes(r#"
        var source=[,,undefined,4,,],trace='';
        var result=source.map(function(value,index,object){
            if(object!==source)throw 'wrong object';trace+=index;return value===undefined?'present':value*2;
        });
        trace==='23'&&result!==source&&result.length===5&&!(0 in result)&&!(1 in result)&&
            result.hasOwnProperty(2)&&result[2]==='present'&&result[3]===8&&!(4 in result)&&
            source[3]===4&&Array.isArray(result)&&Object.getPrototypeOf(result)===Array.prototype;
    "#);
}

#[test]
fn filter_keeps_the_value_read_before_callback_mutation() {
    yes(r#"
        var first={mark:1},second={mark:2},source=[first,second],receiver={},valid=true;
        var result=source.filter(function(value,index,object){
            valid=valid&&arguments.length===3&&object===source&&this===receiver;
            object[index]='replacement';return true;
        },receiver);
        valid&&result!==source&&result.length===2&&result[0]===first&&result[1]===second&&
            source[0]==='replacement'&&source[1]==='replacement'&&
            result.hasOwnProperty(0)&&result.hasOwnProperty(1);
    "#);
}

#[test]
fn some_and_every_short_circuit_using_truthiness_without_coercion() {
    yes(r#"
        var touched=0,trace='',truthy={valueOf:function(){throw 'coerced';},toString:function(){throw 'coerced';}};
        var some=[0,1,2].some(function(value,index){trace+=index;return value?truthy:0;});
        var first=trace;trace='';
        var every=[1,0,2].every(function(value,index){trace+=index;return value?truthy:'';});
        some===true&&every===false&&first==='01'&&trace==='01'&&touched===0;
    "#);
}

#[test]
fn empty_non_reductions_validate_callbacks_and_return_their_identities() {
    yes(r#"
        var calls=0;function callback(){calls++;throw 'visited empty';}
        var a=[],mapped=a.map(callback),filtered=a.filter(callback);
        a.forEach(callback)===undefined&&a.some(callback)===false&&a.every(callback)===true&&
            calls===0&&mapped!==a&&filtered!==a&&mapped!==filtered&&
            mapped.length===0&&filtered.length===0&&Array.isArray(mapped)&&Array.isArray(filtered);
    "#);
}

#[test]
fn reductions_use_four_arguments_and_opposite_index_orders() {
    yes(r#"
        var source=[1,2,3],left='',right='',valid=true;
        var a=source.reduce(function(acc,value,index,object){
            valid=valid&&arguments.length===4&&object===source&&this===globalThis;
            left+=index;return acc*10+value;
        },0);
        var b=source.reduceRight(function(acc,value,index,object){
            valid=valid&&arguments.length===4&&object===source&&this===globalThis;
            right+=index;return acc*10+value;
        },0);
        valid&&a===123&&b===321&&left==='012'&&right==='210';
    "#);
}

#[test]
fn reductions_distinguish_omitted_initial_from_explicit_undefined() {
    for method in ["reduce", "reduceRight"] {
        yes(&format!(
            "var calls=0,seen=false;function callback(acc,value,index,object){{calls++;seen=acc===undefined&&value===7&&index===0&&object===source;return 9;}}var source=[7],seeded=source.{method}(callback),explicit=source.{method}(callback,undefined);seeded===7&&explicit===9&&calls===1&&seen;"
        ));
    }
}

#[test]
fn reductions_empty_or_only_holes_throw_without_initial_value() {
    for method in ["reduce", "reduceRight"] {
        for source in ["[]", "[,,,]"] {
            yes(&format!(
                "var calls=0,caught=false;function callback(){{calls++;return 1;}}var source={source};try{{source.{method}(callback);}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}var explicit=source.{method}(callback,undefined);caught&&calls===0&&explicit===undefined;"
            ));
        }
    }
}

#[test]
fn reductions_seed_from_first_present_element_in_their_direction() {
    yes(r#"
        var source=[,2,,4,],left='',right='';
        var a=source.reduce(function(acc,value,index){left+=index;return acc*10+value;});
        var b=source.reduceRight(function(acc,value,index){right+=index;return acc*10+value;});
        a===24&&b===42&&left==='3'&&right==='1';
    "#);
}

#[test]
fn inherited_indices_are_present_and_map_becomes_own_storage() {
    yes(r#"
        var parent={1:7},source=Object.create(parent);source.length=4;source[3]=9;
        var trace='',result=Array.prototype.map.call(source,function(value,index,object){
            if(object!==source)throw 'wrong receiver';trace+=index;return value+1;
        });
        var right=Array.prototype.reduceRight.call(source,function(acc,value){return acc*10+value;},0);
        trace==='13'&&result.length===4&&!result.hasOwnProperty(0)&&result.hasOwnProperty(1)&&
            !result.hasOwnProperty(2)&&result.hasOwnProperty(3)&&result[1]===8&&result[3]===10&&
            !source.hasOwnProperty(1)&&parent[1]===7&&right===97;
    "#);
}

#[test]
fn callbacks_observe_deleted_replaced_and_added_in_range_properties() {
    for method in ["forEach", "map", "filter", "some", "every"] {
        let continuation = if method == "every" { "true" } else { "false" };
        yes(&format!(
            "var source=[1,2,3],trace='';source.{method}(function(value,index,object){{trace+=index+':'+value+';';if(index===0){{delete object[1];object[2]=30;object[3]=40;}}return {continuation};}});trace==='0:1;2:30;'&&source.length===4;"
        ));
    }
}

#[test]
fn shrinking_then_readding_does_not_change_captured_map_length() {
    yes(r#"
        var source=[1,2,3],trace='';
        var result=source.map(function(value,index,object){
            trace+=index;if(index===0){object.length=1;object[2]=9;object[3]=10;}return value;
        });
        trace==='02'&&result.length===3&&result[0]===1&&!(1 in result)&&result[2]===9&&
            !result.hasOwnProperty(3)&&source.length===4;
    "#);
}

#[test]
fn later_inherited_additions_and_deletions_are_not_snapshotted() {
    yes(r#"
        var parent={1:'old'},source=Object.create(parent),trace='';
        source.length=3;source[0]='first';
        Array.prototype.forEach.call(source,function(value,index){
            trace+=index+':'+value+';';if(index===0){delete parent[1];parent[2]='new';}
        });
        trace==='0:first;2:new;'&&!source.hasOwnProperty(2);
    "#);
}

#[test]
fn reduce_right_observes_mutation_of_future_lower_indices() {
    yes(r#"
        var source=[1,2,3],trace='';
        var result=source.reduceRight(function(acc,value,index,object){
            trace+=index;if(index===2){delete object[1];object[0]=8;object[3]=9;}return acc*10+value;
        },0);
        result===38&&trace==='20'&&source.length===4;
    "#);
}

#[test]
fn length_conversion_runs_once_before_callback_and_is_not_revisited() {
    for method in METHODS {
        let callback = if method == "reduce" || method == "reduceRight" {
            "function(acc,value,index,object){trace+='C';object.length=99;return acc+value;},0"
        } else {
            "function(value,index,object){trace+='C';object.length=99;return true;}"
        };
        // some legitimately stops at its first truthy result.
        let trace = if method == "some" { "LC" } else { "LCC" };
        yes(&format!(
            "var trace='',source={{0:2,1:3,length:{{valueOf:function(){{trace+='L';return 2;}}}}}};Array.prototype.{method}.call(source,{callback});trace==='{trace}';"
        ));
    }
}

#[test]
fn length_uses_uint32_not_clamping_floor_or_unbounded_numeric_length() {
    for (length, expected) in [
        ("undefined", 0),
        ("null", 0),
        ("NaN", 0),
        ("Infinity", 0),
        ("-Infinity", 0),
        ("-0.9", 0),
        ("0.9", 0),
        ("'2.8'", 2),
        ("4294967296", 0),
        ("4294967297", 1),
        ("-4294967296", 0),
        ("-4294967295", 1),
    ] {
        yes(&format!(
            "var calls=0,source={{0:'a',1:'b',length:{length}}};var result=Array.prototype.map.call(source,function(value){{calls++;return value;}});result.length==={expected}&&calls==={expected};"
        ));
    }
}

#[test]
fn length_conversion_abrupt_completion_precedes_callback_validation() {
    for method in METHODS {
        yes(&format!(
            "var marker={{}},trace='',source={{length:{{valueOf:function(){{trace+='L';throw marker;}}}}}},callback={{valueOf:function(){{trace+='C';return 0;}}}},caught;try{{Array.prototype.{method}.call(source,callback);}}catch(error){{caught=error;}}caught===marker&&trace==='L';"
        ));
    }
}

#[test]
fn symbol_length_fails_without_coercing_or_invoking_callback() {
    for method in METHODS {
        yes(&format!(
            "var calls=0,caught=false,source={{length:Symbol('length')}};try{{Array.prototype.{method}.call(source,function(){{calls++;}});}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&calls===0;"
        ));
    }
}

#[test]
fn empty_traversals_reject_noncallables_without_coercing_them() {
    for method in METHODS {
        for callback in ["undefined", "null", "3", "'callback'", "{}"] {
            yes(&format!(
                "var caught=false;try{{[].{method}({callback});}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught;"
            ));
        }
        yes(&format!(
            "var touched=0,caught=false,callback={{valueOf:function(){{touched++;return 1;}},toString:function(){{touched++;return 'function';}}}};try{{[].{method}(callback);}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&touched===0;"
        ));
    }
}

#[test]
fn nullish_receivers_reject_without_callback_or_receiver_coercion() {
    for method in METHODS {
        for receiver in ["null", "undefined"] {
            yes(&format!(
                "var touched=0,caught=false,argument={{toString:function(){{touched++;throw 'coerced';}}}};try{{Array.prototype.{method}.call({receiver},function(){{touched++;}},argument);}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&touched===0;"
            ));
        }
    }
}

#[test]
fn generic_ordinary_and_null_prototype_receivers_support_all_seven_methods() {
    yes(r#"
        var source=Object.create(null);source[0]=4;source[2]=8;source.length=3;
        var p=Array.prototype,trace='';
        p.forEach.call(source,function(value,index,object){if(object!==source)throw 'identity';trace+=index;});
        var mapped=p.map.call(source,function(value){return value+1;});
        var filtered=p.filter.call(source,function(value){return value>4;});
        trace==='02'&&mapped.length===3&&mapped[0]===5&&!(1 in mapped)&&mapped[2]===9&&
            filtered.length===1&&filtered[0]===8&&p.some.call(source,function(value){return value===8;})&&
            p.every.call(source,function(value){return value>0;})&&
            p.reduce.call(source,function(acc,value){return acc+value;},0)===12&&
            p.reduceRight.call(source,function(acc,value){return acc*10+value;},0)===84;
    "#);
}

#[test]
fn function_and_native_receivers_keep_identity_and_existing_numeric_arity() {
    yes(r#"
        function source(a,b){}source[0]=3;source[1]=5;
        parseInt[0]=7;parseInt[1]=9;
        var valid=true,p=Array.prototype;
        var a=p.map.call(source,function(value,index,object){valid=valid&&object===source;return value+index;});
        var b=p.filter.call(parseInt,function(value,index,object){valid=valid&&object===parseInt;return true;});
        valid&&a.length===2&&a[0]===3&&a[1]===6&&b.length===2&&b[0]===7&&b[1]===9&&
            !Array.isArray(source)&&!Array.isArray(parseInt);
    "#);
}

#[test]
fn primitive_string_is_boxed_once_and_visits_each_utf16_code_unit() {
    yes(r#"
        var source='\uD800x\uDC00',receiver,valid=true,trace='';
        var result=Array.prototype.map.call(source,function(value,index,object){
            if(index===0)receiver=object;
            valid=valid&&object===receiver&&typeof object==='object'&&object.valueOf()===source&&
                Object.getPrototypeOf(object)===String.prototype&&arguments.length===3;
            trace+=index;return value;
        });
        valid&&trace==='012'&&result.length===3&&result[0]==='\uD800'&&result[1]==='x'&&
            result[2]==='\uDC00'&&result.join('')===source;
    "#);
}

#[test]
fn existing_boxed_receiver_identity_and_primitive_prototype_indices_are_used() {
    yes(r#"
        var boxed=Object('xy'),seen,valid=true;
        Array.prototype.forEach.call(boxed,function(value,index,object){seen=object;valid=valid&&object===boxed;});
        Number.prototype.length=1;Number.prototype[0]='number';
        Boolean.prototype.length=1;Boolean.prototype[0]='boolean';
        Symbol.prototype.length=1;Symbol.prototype[0]='symbol';
        var symbol=Symbol('key'),n,b,s;
        Array.prototype.forEach.call(7,function(value,index,object){n=object;valid=valid&&value==='number'&&index===0;});
        Array.prototype.forEach.call(false,function(value,index,object){b=object;valid=valid&&value==='boolean'&&index===0;});
        Array.prototype.forEach.call(symbol,function(value,index,object){s=object;valid=valid&&value==='symbol'&&index===0;});
        valid&&seen===boxed&&n.valueOf()===7&&b.valueOf()===false&&s.valueOf()===symbol&&
            typeof n==='object'&&typeof b==='object'&&typeof s==='object';
    "#);
}

#[test]
fn omitted_null_and_primitive_thisargs_follow_existing_callback_call_semantics() {
    yes(r#"
        var source=[1,2],valid=true,prior;
        source.forEach(function(){valid=valid&&this===globalThis;});
        source.forEach(function(){valid=valid&&this===globalThis;},null);
        source.forEach(function(){
            valid=valid&&typeof this==='object'&&this.valueOf()===7&&this!==prior;prior=this;
        },7);
        var a=source.map(Object.prototype.toString),b=source.map(Object.prototype.toString,null);
        valid&&a[0]==='[object Undefined]'&&a[1]==='[object Undefined]'&&
            b[0]==='[object Null]'&&b[1]==='[object Null]';
    "#);
}

#[test]
fn native_callbacks_receive_real_indices_and_preserved_thisarg() {
    yes(r#"
        var parsed=['10','10','10'].map(parseInt);
        var strings=[1,2].map(String.prototype.valueOf,'held');
        var reduction=[1,2].reduce(Object.prototype.toString,0);
        parsed[0]===10&&isNaN(parsed[1])&&parsed[2]===2&&
            strings[0]==='held'&&strings[1]==='held'&&reduction==='[object Undefined]';
    "#);
}

#[test]
fn bound_callbacks_preserve_prefix_receiver_and_eventual_argument_lists() {
    yes(r#"
        var receiver={bias:4},source=[1,2],valid=true;
        function mapper(prefix,value,index,object){
            valid=valid&&this===receiver&&arguments.length===4&&object===source;
            return prefix+value+index+this.bias;
        }
        function reducer(prefix,acc,value,index,object){
            valid=valid&&this===receiver&&arguments.length===5&&object===source;
            return acc+prefix+value;
        }
        var mapped=source.map(mapper.bind(receiver,10),{wrong:1});
        var reduced=source.reduce(reducer.bind(receiver,1),0);
        valid&&mapped[0]===15&&mapped[1]===17&&reduced===5;
    "#);
}

#[test]
fn input_expression_order_precedes_length_and_extra_arguments_are_not_coerced() {
    yes(r#"
        var trace='',unused={toString:function(){throw 'coerced extra';}};
        function source(){trace+='R';return {0:3,length:{valueOf:function(){trace+='L';return 1;}}};}
        function callback(){trace+='C';return function(value){trace+='V';return value;};}
        function receiver(){trace+='T';return {};}
        function extra(){trace+='X';return unused;}
        var result=Array.prototype.map.call(source(),callback(),receiver(),extra());
        var a=[1].reduce(function(acc,value){return acc+value;},2,unused);
        trace==='RCTXLV'&&result[0]===3&&a===3;
    "#);
}

#[test]
fn callback_exception_keeps_identity_stops_visits_and_publishes_no_partial_result() {
    for method in METHODS {
        yes(&format!(
            "var marker={{}},caught,visits=0,finished=0,result='unchanged';try{{result=[1,2,3].{method}(function(){{visits++;throw marker;}},0);}}catch(error){{caught=error;}}finally{{finished++;}}caught===marker&&visits===1&&finished===1&&result==='unchanged';"
        ));
    }
}

#[test]
fn map_and_filter_results_are_intrinsic_without_constructor_or_tag_observation() {
    yes(r#"
        var map=Array.prototype.map,filter=Array.prototype.filter,prototype=Array.prototype,source=[2,4],calls=0;
        source.constructor={valueOf:function(){throw 'constructor coerced';}};
        source[Symbol.toStringTag]={toString:function(){throw 'tag coerced';}};
        Array=function(){calls++;throw 'global Array called';};
        var a=map.call(source,function(value){return value+1;});
        var b=filter.call(source,function(value){return value===4;});
        calls===0&&Object.getPrototypeOf(a)===prototype&&Object.getPrototypeOf(b)===prototype&&
            a!==b&&a[0]===3&&a[1]===5&&b.length===1&&b[0]===4;
    "#);
}

#[test]
fn filter_truthiness_keeps_falsey_source_values_and_symbol_identity() {
    yes(r#"
        var symbol=Symbol('kept'),source=[0,false,'',null,undefined,symbol],calls=0;
        var truthy={valueOf:function(){throw 'coerced';},toString:function(){throw 'coerced';}};
        var result=source.filter(function(){calls++;return truthy;});
        calls===6&&result.length===6&&result[0]===0&&result[1]===false&&result[2]===''&&
            result[3]===null&&result.hasOwnProperty(4)&&result[4]===undefined&&result[5]===symbol;
    "#);
}

#[test]
fn reduction_accumulators_keep_object_symbol_and_undefined_identity() {
    yes(r#"
        var symbol=Symbol('seed'),calls=0,acc={sum:0};
        var same=[,symbol,].reduce(function(){calls++;throw 'unexpected seed call';});
        var result=[2,3].reduce(function(current,value){if(current!==acc)throw 'identity';current.sum+=value;return current;},acc);
        var seen=false,final=[1,2].reduceRight(function(current,value,index){
            if(index===0)seen=current===undefined;return undefined;
        },symbol);
        same===symbol&&calls===0&&result===acc&&acc.sum===5&&seen&&final===undefined;
    "#);
}

#[test]
fn callbacks_and_results_survive_origin_execution_and_dynamic_compilation() {
    let mut runtime = Runtime::new();
    runtime
        .execute(
            "var total=0;function factory(step){return function(value){total+=step;return value+step;};}var callback=factory(3);var mapped=[1,2].map(callback);",
            &mut NoIo,
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(
                "var compiled=Function('items','return items.map(function(value){return value*2;});');var later=compiled(mapped);var indirect=eval;indirect('[1].forEach(callback)');total===9&&mapped[0]===4&&mapped[1]===5&&later[0]===8&&later[1]===10;",
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert!(runtime.allocation_report().is_valid());
}

#[test]
fn ordinary_callback_errors_do_not_poison_later_public_execute_or_invoke() {
    let mut runtime = Runtime::new();
    let error = runtime
        .execute(
            "var calls=0;[1,2].map(function(){calls++;throw 'stop';});",
            &mut NoIo,
        )
        .unwrap_err();
    assert!(error.contains("stop"), "{error}");
    assert_eq!(runtime.get_global("calls"), Value::Number(1.0));
    let callback = runtime
        .execute(
            "function callback(value){return value+1;}callback;",
            &mut NoIo,
        )
        .unwrap();
    assert_eq!(
        runtime
            .invoke(
                callback,
                Value::Undefined,
                vec![Value::Number(4.0)],
                &mut NoIo
            )
            .unwrap(),
        Value::Number(5.0)
    );
    assert_eq!(
        runtime
            .execute("[3].map(callback)[0]===4;", &mut NoIo)
            .unwrap(),
        Value::Bool(true)
    );
    assert!(runtime.allocation_report().first_rejected.is_none());
}
