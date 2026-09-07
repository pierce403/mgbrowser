//! Independently authored concat semantics: ES5.1 15.4.4.4 plus the documented
//! retained trailing-hole length and existing Symbol/Host extensions.
//! https://262.ecma-international.org/5.1/#sec-15.4.4.4
//! No species/spreadability hooks or broader arguments-descriptor claim.

use mg_deps::js::runtime::{Host, Runtime, Value};

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
        .unwrap_or_else(|error| panic!("concat case failed: {error}\n{source}"));
    assert_eq!(result, Value::Bool(true), "{source}");
}

fn type_error(expression: &str) {
    yes(&format!(
        "var caught=false;try{{{expression};}}catch(e){{caught=String(e).indexOf('TypeError')>=0;}}caught;"
    ));
}

#[test]
fn empty_concat_returns_a_fresh_true_array_with_intrinsic_parent() {
    yes(r#"
        var source=[],a=source.concat(),b=source.concat();
        a!==source && b!==source && a!==b && a.length===0 && b.length===0 &&
            Array.isArray(a) && Object.getPrototypeOf(a)===Array.prototype &&
            Object.keys(a).length===0 && Object.getOwnPropertyNames(a).join(',')==='length';
    "#);
}

#[test]
fn receiver_precedes_all_arguments_and_only_one_array_level_is_flattened() {
    yes(r#"
        var inner=[2],other=[3],source=[1,inner],nested=[other];
        var result=source.concat(nested,4,[5,6]);
        result.length===6 && result[0]===1 && result[1]===inner &&
            result[2]===other && result[3]===4 && result[4]===5 && result[5]===6;
    "#);
}

#[test]
fn repeated_and_cyclic_sources_preserve_identity_without_recursive_flattening() {
    yes(r#"
        var source=[];source[0]=source;var result=source.concat(source);
        result!==source && result.length===2 && result[0]===source &&
            result[1]===source && source.length===1 && source[0]===source;
    "#);
}

#[test]
fn ordinary_array_like_and_inherited_array_objects_remain_single_values() {
    yes(r#"
        var ordinary={0:'zero',length:1},bare=Object.create(null),child=Object.create([7,8]);
        bare[0]='bare';bare.length=1;
        var result=[1].concat(ordinary,bare,child);
        result.length===4 && result[1]===ordinary && result[2]===bare &&
            result[3]===child && !Array.isArray(child);
    "#);
}

#[test]
fn object_function_native_regex_and_error_arguments_keep_exact_identity() {
    yes(r#"
        var object={},pattern=/x/,error=new Error('owned');function fn(){}
        var result=[].concat(object,fn,parseInt,pattern,error);
        result.length===5 && result[0]===object && result[1]===fn &&
            result[2]===parseInt && result[3]===pattern && result[4]===error;
    "#);
}

#[test]
fn generic_object_function_and_native_receivers_are_not_spread() {
    for receiver in [
        "({0:'zero',length:1})",
        "Object.create(null)",
        "fn",
        "parseInt",
        "new Error('x')",
    ] {
        yes(&format!(
            "function fn(){{}}var receiver={receiver},result=Array.prototype.concat.call(receiver,[2],3);result.length===3 && result[0]===receiver && result[1]===2 && result[2]===3;"
        ));
    }
}

#[test]
fn generic_primitive_receivers_are_boxed_but_primitive_arguments_are_not() {
    for (primitive, prototype) in [("7", "Number"), ("false", "Boolean"), ("'ab'", "String")] {
        yes(&format!(
            "var result=Array.prototype.concat.call({primitive},{primitive});result.length===2 && typeof result[0]==='object' && Object.getPrototypeOf(result[0])==={prototype}.prototype && result[0].valueOf()==={primitive} && result[1]==={primitive} && !Array.isArray(result[0]);"
        ));
    }
    yes(r#"
        var key=Symbol('receiver'),result=Array.prototype.concat.call(key,key);
        result.length===2 && typeof result[0]==='object' && result[0]!==key &&
            Object.getPrototypeOf(result[0])===Symbol.prototype &&
            Symbol.prototype.valueOf.call(result[0])===key && result[1]===key;
    "#);
}

#[test]
fn nullish_receivers_reject_but_nullish_arguments_are_present_elements() {
    type_error("Array.prototype.concat.call(null)");
    type_error("Array.prototype.concat.call(undefined)");
    type_error("Array.prototype.concat.apply(null,[])");
    yes(r#"
        var result=[].concat(null,undefined);
        result.length===2 && result.hasOwnProperty(0) && result.hasOwnProperty(1) &&
            result[0]===null && result[1]===undefined;
    "#);
}

#[test]
fn primitive_number_values_are_not_coerced_or_normalized() {
    yes(r#"
        var result=[].concat(-0,NaN,Infinity,-Infinity,true,false);
        result.length===6 && 1/result[0]===-Infinity && result[1]!==result[1] &&
            result[2]===Infinity && result[3]===-Infinity &&
            result[4]===true && result[5]===false;
    "#);
}

#[test]
fn utf16_strings_and_symbols_remain_whole_uncoerced_values() {
    yes(r#"
        var text='\uD800x\uDC00\uD83D\uDE00',key=Symbol('\uD800'),other=Symbol('\uD800');
        var result=[text,key].concat(text,[other]);
        result.length===4 && result[0]===text && result[2]===text &&
            result[0].length===5 && result[0].charCodeAt(0)===55296 &&
            result[0].charCodeAt(2)===56320 && result[0].charCodeAt(3)===55357 &&
            result[0].charCodeAt(4)===56832 && result[1]===key && result[3]===other &&
            key!==other && typeof result[1]==='symbol';
    "#);
}

#[test]
fn holes_undefined_and_trailing_logical_length_are_distinct() {
    yes(r#"
        var source=[,undefined,,],argument=Array(2),result=source.concat(argument);
        source.length===3 && argument.length===2 && result.length===5 &&
            !result.hasOwnProperty(0) && result.hasOwnProperty(1) && result[1]===undefined &&
            !result.hasOwnProperty(2) && !result.hasOwnProperty(3) &&
            !result.hasOwnProperty(4) && !(4 in result) && Object.keys(result).join(',')==='1';
    "#);
    yes(
        "var result=Array(2).concat(Array(3));result.length===5 && Object.keys(result).length===0 && !(0 in result) && !(4 in result);",
    );
}

#[test]
fn inherited_numeric_properties_are_read_and_copied_to_own_result_slots() {
    yes(r#"
        Array.prototype[1]='inherited';
        var source=Array(3);source[0]='own';var result=source.concat(Array(2));
        var captured=result.length===5 && result[0]==='own' && result[1]==='inherited' &&
            result[4]==='inherited' && result.hasOwnProperty(1) &&
            result.hasOwnProperty(4) && !result.hasOwnProperty(2) && !source.hasOwnProperty(1);
        delete Array.prototype[1];
        captured && result[1]==='inherited' && result[4]==='inherited' && !(1 in source);
    "#);
}

#[test]
fn nearest_inherited_numeric_owner_wins_but_explicit_undefined_shadows_it() {
    yes(r#"
        Object.prototype[0]='far';Object.prototype[2]='ancestor';
        Array.prototype[0]='near';
        var source=Array(3);source[1]=undefined;var result=source.concat();
        var captured=result[0]==='near' && result[1]===undefined && result[2]==='ancestor' &&
            result.hasOwnProperty(0) && result.hasOwnProperty(1) && result.hasOwnProperty(2);
        delete Array.prototype[0];delete Object.prototype[0];delete Object.prototype[2];
        captured && Object.keys(result).join(',')==='0,1,2';
    "#);
    yes(
        "Array.prototype[0]='inherited';var source=[undefined],result=source.concat();result.hasOwnProperty(0) && result[0]===undefined;",
    );
}

#[test]
fn concat_does_not_copy_nonindex_string_or_symbol_properties() {
    yes(r#"
        var key=Symbol('extra'),source=[1];source.extra=2;source['01']=3;
        source['-1']=4;source[key]=5;var result=source.concat();
        result.length===1 && result[0]===1 && !result.hasOwnProperty('extra') &&
            !result.hasOwnProperty('01') && !result.hasOwnProperty('-1') &&
            Object.getOwnPropertySymbols(result).length===0 &&
            source.extra===2 && source[key]===5;
    "#);
}

#[test]
fn result_slots_are_independent_writable_enumerable_and_configurable() {
    yes(r#"
        var object={value:1},source=[object,'original'],argument=['argument'];
        var result=source.concat(argument);
        result[0].value=2;result[1]='changed';delete result[2];result.length=1;
        result!==source && result!==argument && result[0]===object &&
            object.value===2 && source.length===2 && source[1]==='original' &&
            argument.length===1 && argument[0]==='argument' && Object.keys(result).join(',')==='0';
    "#);
}

#[test]
fn argument_expressions_finish_left_to_right_before_arrays_are_copied() {
    yes(r#"
        var trail='',source=[0],later=[1];
        function first(){trail+='first;';source[0]=2;return later;}
        function second(){trail+='second;';later[1]=3;return 4;}
        var result=source.concat(first(),second());
        trail==='first;second;' && result.join(',')==='2,1,3,4' &&
            source.join(',')==='2' && later.join(',')==='1,3';
    "#);
}

#[test]
fn concat_never_coerces_items_or_uses_array_like_length() {
    yes(r#"
        var calls=0,item={0:'entry',length:{valueOf:function(){calls++;throw 'length';}}};
        item.toString=function(){calls++;throw 'string';};
        item.valueOf=function(){calls++;throw 'value';};
        item[Symbol.toPrimitive]=function(){calls++;throw 'primitive';};
        var result=Array.prototype.concat.call(item,item,[item]);
        calls===0 && result.length===3 && result[0]===item && result[1]===item && result[2]===item;
    "#);
}

#[test]
fn intrinsic_array_result_ignores_overwritten_global_and_source_constructor() {
    yes(r#"
        var IntrinsicArray=Array,parent=Array.prototype,source=[1];
        source.constructor=function(){throw 'source constructor ran';};
        Array=function(){throw 'global constructor ran';};
        var result=parent.concat.call(source,[2]);
        IntrinsicArray.isArray(result) && Object.getPrototypeOf(result)===parent &&
            result instanceof IntrinsicArray && result.length===2 && result[0]===1 && result[1]===2;
    "#);
}

#[test]
fn displayed_tags_and_ordinary_spreadability_names_do_not_control_spreading() {
    yes(r#"
        var array=[1],fake={0:2,length:1},pretend=Symbol('Symbol.isConcatSpreadable');
        array[Symbol.toStringTag]='Object';fake[Symbol.toStringTag]='Array';
        array.isConcatSpreadable=false;fake.isConcatSpreadable=true;
        array[pretend]=false;fake[pretend]=true;
        var result=[].concat(array,fake);
        result.length===2 && result[0]===1 && result[1]===fake &&
            Array.isArray(array) && !Array.isArray(fake) &&
            Symbol.isConcatSpreadable===undefined && Symbol.species===undefined;
    "#);
}

#[test]
fn array_prototype_is_a_true_array_and_index_growth_affects_its_concat() {
    yes(r#"
        var p=Array.prototype,initial=Array.isArray(p) && p.length===0 &&
            Object.getPrototypeOf(p)===Object.prototype &&
            Object.prototype.toString.call(p)==='[object Array]';
        p[2]='prototype';var result=p.concat(['tail']);
        initial && p.length===3 && result.length===4 && !result.hasOwnProperty(0) &&
            !result.hasOwnProperty(1) && result.hasOwnProperty(2) &&
            result[2]==='prototype' && result[3]==='tail';
    "#);
}

#[test]
fn arguments_have_their_own_brand_object_parent_and_are_never_spread() {
    yes(r#"
        function capture(){return arguments;}
        var args=capture(1,2),result=[].concat(args),generic=Array.prototype.concat.call(args,[3]);
        !Array.isArray(args) && Object.getPrototypeOf(args)===Object.prototype &&
            Object.prototype.toString.call(args)==='[object Arguments]' &&
            typeof args.concat==='undefined' && result.length===1 && result[0]===args &&
            generic.length===2 && generic[0]===args && generic[1]===3 &&
            args.length===2 && args[0]===1 && args[1]===2 && args.callee===capture;
    "#);
}

#[test]
fn arguments_remain_unmapped_snapshots_after_concat_and_function_return() {
    yes(r#"
        function capture(value){
            var saved=arguments,result=[].concat(saved);value='formal';saved[0]='snapshot';
            return function(){return value==='formal' && result[0]===saved && saved[0]==='snapshot' && saved.callee===capture;};
        }
        var a=capture('a'),b=capture('b');a!==b && a() && b();
    "#);
    yes(
        "function capture(){return arguments;}var args=capture(2,7);Math.max.apply(null,args)===7 && !Array.isArray(args);",
    );
}

#[test]
fn arguments_tag_override_never_changes_concat_identity_or_brand_predicate() {
    yes(r#"
        function capture(){return arguments;}var args=capture(1);
        args[Symbol.toStringTag]='Array';var result=[].concat(args);
        var tagged=Object.prototype.toString.call(args)==='[object Array]';
        delete args[Symbol.toStringTag];
        tagged && !Array.isArray(args) && result.length===1 && result[0]===args &&
            Object.prototype.toString.call(args)==='[object Arguments]';
    "#);
}

#[test]
fn call_and_apply_forward_each_argument_without_changing_concat_rules() {
    yes(r#"
        var source=[1],a=[2],b={0:3,length:1};
        var call=Array.prototype.concat.call(source,a,b),apply=Array.prototype.concat.apply(source,[a,b]);
        call!==apply && call.length===3 && apply.length===3 &&
            call[0]===1 && call[1]===2 && call[2]===b &&
            apply[0]===1 && apply[1]===2 && apply[2]===b;
    "#);
}

#[test]
fn host_receivers_and_arguments_are_opaque_single_values_without_callbacks() {
    let mut runtime = Runtime::new();
    runtime.set_global("receiver", Value::Host("concat-receiver".into()));
    runtime.set_global("argument", Value::Host("concat-argument".into()));
    assert_eq!(
        runtime
            .execute(
                "var result=Array.prototype.concat.call(receiver,argument,[receiver]);result.length===3 && result[0]===receiver && result[1]===argument && result[2]===receiver;",
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn existing_string_concat_remains_a_distinct_string_operation() {
    yes(
        "String.prototype.concat.call('one',2,'three')==='one2three' && typeof ''.concat('x')==='string';",
    );
}
