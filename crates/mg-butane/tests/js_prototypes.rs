//! Independently authored prototype semantics for the documented ES5-shaped
//! object model. Function/native objects retain identity, not just property bags.
//! https://262.ecma-international.org/5.1/#sec-15.2.3.5
//! https://262.ecma-international.org/5.1/#sec-15.2.3.2
//! Enumeration mutation/order follows docs/JAVASCRIPT.md's bounded policy.

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
        .unwrap_or_else(|error| panic!("prototype case failed: {error}\n{source}"));
    assert_eq!(result, Value::Bool(true), "{source}");
}

fn type_error(setup: &str, expression: &str) {
    yes(&format!(
        "{setup};var caught=false;try{{{expression};}}catch(e){{caught=String(e).indexOf('TypeError')>=0;}}caught;"
    ));
}

#[test]
fn ordinary_and_null_prototype_controls_preserve_exact_identity() {
    yes(r#"
        var parent={value:3},child=Object.create(parent),other=Object.create(parent);
        var bare=Object.create(null);bare.own=4;
        Object.getPrototypeOf(child)===parent && child!==other && child.value===3 &&
        Object.getPrototypeOf(bare)===null && bare.own===4 &&
        !('toString' in bare) && Object.getPrototypeOf(Object.prototype)===null;
    "#);
}

#[test]
fn function_prototypes_round_trip_without_making_children_callable() {
    yes(r#"
        function Parent(){throw 'must not call parent';}
        var child=Object.create(Parent),other=Object.create(Parent),caught=false;
        try{child();}catch(e){caught=String(e).indexOf('TypeError')>=0;}
        Object.getPrototypeOf(child)===Parent && Object.getPrototypeOf(other)===Parent &&
        child!==other && typeof Parent==='function' && typeof child==='object' && caught;
    "#);
}

#[test]
fn native_constructors_and_methods_are_distinct_valid_prototypes() {
    for native in [
        "parseInt",
        "Array",
        "Object.create",
        "Symbol",
        "Math.abs",
        "Symbol.prototype[Symbol.toPrimitive]",
    ] {
        yes(&format!(
            "var parent={native},a=Object.create(parent),b=Object.create(parent);Object.getPrototypeOf(a)===parent && Object.getPrototypeOf(b)===parent && a!==b && typeof a==='object';"
        ));
    }
    yes(
        "Object.getPrototypeOf(Object.create(parseInt))!==Object.getPrototypeOf(Object.create(parseFloat));",
    );
}

#[test]
fn user_and_native_functions_report_their_actual_parent() {
    yes(r#"
        function User(){}
        Object.getPrototypeOf(User)===Function.prototype &&
        Object.getPrototypeOf(parseInt)===Function.prototype &&
        Object.getPrototypeOf(Array)===Function.prototype &&
        Object.getPrototypeOf(Object.create)===Function.prototype &&
        Object.getPrototypeOf(Symbol)===Function.prototype &&
        Object.getPrototypeOf(Function.prototype)===Object.prototype;
    "#);
}

#[test]
fn get_prototype_of_keeps_es5_primitive_rejection_without_boxing() {
    for primitive in [
        "undefined",
        "null",
        "true",
        "false",
        "0",
        "NaN",
        "'abc'",
        "Symbol('x')",
    ] {
        type_error("", &format!("Object.getPrototypeOf({primitive})"));
    }
    type_error("", "Object.getPrototypeOf()");
    yes(r#"
        var called=false,parent={toString:function(){called=true;throw 3;},valueOf:function(){called=true;throw 4;}};
        Object.getPrototypeOf(parent)===Object.prototype && !called;
    "#);
}

#[test]
fn create_rejects_primitives_and_never_coerces_the_prototype() {
    for primitive in ["undefined", "true", "0", "'abc'", "Symbol('x')"] {
        type_error("", &format!("Object.create({primitive})"));
    }
    type_error("", "Object.create()");
    // Validate the prototype before the deliberately unsupported descriptor path.
    type_error("", "Object.create(7,{})");
    yes(r#"
        var called=false,parent={toString:function(){called=true;throw 3;},valueOf:function(){called=true;throw 4;}};
        var child=Object.create(parent,undefined);
        Object.getPrototypeOf(child)===parent && !called;
    "#);
    yes(r#"
        var order='';function parent(){order+='p';return Array;}function properties(){order+='d';return undefined;}
        var child=Object.create(parent(),properties());order==='pd' && Object.getPrototypeOf(child)===Array;
    "#);
}

#[test]
fn new_uses_function_and_native_valued_constructor_prototypes() {
    for parent in ["User", "Array", "parseInt", "Object.create"] {
        yes(&format!(
            "function User(){{throw 'prototype must not run';}}function C(){{this.own=7;}}C.prototype={parent};var child=new C();Object.getPrototypeOf(child)==={parent} && child.own===7 && child instanceof C && typeof child==='object';"
        ));
    }
}

#[test]
fn new_retains_ordinary_primitive_fallback_and_return_override_rules() {
    for prototype in ["undefined", "null", "3", "'text'", "Symbol('p')"] {
        yes(&format!(
            "function C(){{this.own=3;}}C.prototype={prototype};var child=new C();Object.getPrototypeOf(child)===Object.prototype && child.own===3;"
        ));
    }
    yes(r#"
        function Parent(){}function C(){return Parent;}C.prototype=parseInt;
        function D(){return Array;}D.prototype=Parent;
        new C()===Parent && new D()===Array;
    "#);
}

#[test]
fn instanceof_matches_the_typed_chain_not_property_storage_or_names() {
    yes(r#"
        function Parent(){}function Same(){}function C(){}function Other(){}
        C.prototype=Parent;Other.prototype=Same;
        var child=Object.create(Object.create(Parent));
        child instanceof C && !(child instanceof Other) && !(Parent instanceof C) &&
        Parent instanceof Function && Array instanceof Function && parseInt instanceof Function &&
        Array instanceof Object &&
        !(3 instanceof C) && !(Symbol('x') instanceof C) && !(null instanceof C);
    "#);
    type_error("function C(){}C.prototype=1;", "({}) instanceof C");
    type_error("", "({}) instanceof ({})");
    // Primitive LHS returns false before reading an invalid constructor prototype.
    yes("function C(){}C.prototype=1;!(3 instanceof C) && !(null instanceof C);");
}

#[test]
fn function_virtual_metadata_is_inherited_and_remains_readonly() {
    yes(r#"
        function Parent(a,b){}var child=Object.create(Parent),original=Parent.prototype;
        child.length=9;child.name='other';
        var guarded=child.length===2 && child.name==='Parent' &&
            !child.hasOwnProperty('length') && !child.hasOwnProperty('name');
        child.prototype=8;
        var shadowed=child.prototype===8 && Parent.prototype===original && child.hasOwnProperty('prototype');
        var removed=delete child.prototype;
        guarded && shadowed && removed && child.prototype===original &&
        delete child.length && child.length===2;
    "#);
}

#[test]
fn native_virtual_metadata_and_static_functions_remain_visible() {
    yes(r#"
        var arrayChild=Object.create(Array),parseChild=Object.create(parseInt),mathChild=Object.create(Math.abs);
        arrayChild.name==='Array' && arrayChild.length===1 && arrayChild.prototype===Array.prototype &&
        arrayChild.isArray===Array.isArray && arrayChild.isArray([]) &&
        parseChild.name==='parseInt' && parseChild.length===2 &&
        mathChild.name==='abs' && mathChild.length===1 &&
        !arrayChild.hasOwnProperty('isArray') && Object.keys(arrayChild).length===0;
    "#);
}

#[test]
fn native_readonly_metadata_blocks_shadow_but_writable_methods_allow_it() {
    yes(r#"
        var child=Object.create(Array),original=Array.prototype,method=Array.isArray;
        child.name='wrong';child.length=9;child.prototype={};
        var guarded=child.name==='Array' && child.length===1 && child.prototype===original &&
            !child.hasOwnProperty('name') && !child.hasOwnProperty('length') && !child.hasOwnProperty('prototype');
        child.isArray=function(){return 'own';};
        var shadowed=child.isArray()==='own' && Array.isArray===method && child.hasOwnProperty('isArray');
        var removed=delete child.isArray;
        guarded && shadowed && removed && child.isArray===method && delete child.prototype && child.prototype===original;
    "#);
    yes(r#"
        var child=Object.create(Symbol),key=Symbol.toPrimitive;
        child.toPrimitive=Symbol('replacement');
        child.toPrimitive===key && !child.hasOwnProperty('toPrimitive') && delete child.toPrimitive && Symbol.toPrimitive===key;
    "#);
}

#[test]
fn ordinary_inherited_properties_shadow_delete_and_reveal_on_typed_parents() {
    for parent in ["User", "parseInt"] {
        yes(&format!(
            "function User(){{}}var parent={parent};parent.value=1;var child=Object.create(parent);var initial=child.value===1 && 'value' in child && !child.hasOwnProperty('value');child.value=2;var shadow=child.value===2 && parent.value===1;delete child.value;var reveal=child.value===1;delete child.value;delete parent.value;initial && shadow && reveal && !('value' in child) && child.value===undefined;"
        ));
    }
}

#[test]
fn inherited_symbols_keep_identity_shadow_and_own_reflection() {
    for parent in ["User", "Array"] {
        yes(&format!(
            "function User(){{}}var parent={parent},key=Symbol('key'),other=Symbol('key');parent[key]=1;var child=Object.create(parent);var inherited=child[key]===1 && key in child && !(other in child) && Object.getOwnPropertySymbols(child).length===0;child[key]=2;var shadowed=parent[key]===1 && child[key]===2 && child.hasOwnProperty(key);delete child[key];inherited && shadowed && child[key]===1 && Object.getOwnPropertySymbols(child).length===0 && Object.getOwnPropertySymbols(parent)[0]===key;"
        ));
    }
}

#[test]
fn own_reflection_does_not_promote_inherited_metadata_or_symbols() {
    yes(r#"
        function Parent(a){}var inherited=Symbol('parent'),own=Symbol('own');
        Parent.extra=1;Parent[inherited]=2;
        var child=Object.create(Parent);child.local=3;child[own]=4;
        Object.keys(child).join(',')==='local' && Object.getOwnPropertyNames(child).join(',')==='local' &&
        Object.getOwnPropertySymbols(child).length===1 && Object.getOwnPropertySymbols(child)[0]===own &&
        !child.hasOwnProperty('length') && !child.hasOwnProperty(inherited) && child[inherited]===2;
    "#);
}

#[test]
fn for_in_non_enumerable_function_and_native_metadata_shadows_ancestors() {
    yes(r#"
        Object.prototype.length=1;Object.prototype.name=1;Object.prototype.prototype=1;
        Function.prototype.far=2;function Parent(a){}Parent.extra=3;
        var child=Object.create(Parent);child.local=4;var keys='';for(var key in child){keys+=key+',';}
        keys==='local,extra,far,' && child.length===1 && child.name==='Parent';
    "#);
    yes(r#"
        Object.prototype.length=1;Object.prototype.name=1;Object.prototype.prototype=1;Object.prototype.isArray=1;
        Function.prototype.far=2;Array.extra=3;
        var child=Object.create(Array);child.local=4;var keys='';for(var key in child){keys+=key+',';}
        keys==='local,extra,far,' && child.isArray===Array.isArray;
    "#);
}

#[test]
fn for_in_typed_parent_snapshot_skips_deleted_and_new_names_but_reads_current_values() {
    for parent in ["User", "parseInt"] {
        yes(&format!(
            "function User(){{}}var parent={parent};parent.first=1;parent.second=2;parent.third=3;var child=Object.create(parent);child.trigger=0;var seen='';for(var key in child){{if(key==='trigger'){{delete parent.second;parent.added=4;parent.first=10;}}seen+=key+':'+child[key]+',';}}seen==='trigger:0,first:10,third:3,' && child.added===4;"
        ));
    }
}

#[test]
fn for_in_does_not_replace_a_snapshot_owner_after_shadowing_or_deletion() {
    for parent in ["User", "Array"] {
        yes(&format!(
            "function User(){{}}var parent={parent};parent.hidden=1;parent.future=2;var child=Object.create(parent);child.trigger=0;child.hidden=3;var seen='';for(var key in child){{if(key==='trigger'){{delete child.hidden;child.future=4;}}seen+=key+',';}}seen==='trigger,' && child.hidden===1 && child.future===4;"
        ));
    }
}

#[test]
fn delete_and_readd_between_snapshots_updates_creation_order_without_duplicates() {
    for parent in ["User", "parseInt"] {
        yes(&format!(
            "function User(){{}}var parent={parent},a=Symbol('a'),b=Symbol('b');parent.first=1;parent.second=2;parent[a]=3;parent[b]=4;var child=Object.create(parent),before='';for(var key in child)before+=key+',';delete parent.first;parent.first=5;delete parent[a];parent[a]=6;var after='';for(var key in child)after+=key+',';var symbols=Object.getOwnPropertySymbols(parent);before==='first,second,' && after==='second,first,' && symbols.length===2 && symbols[0]===b && symbols[1]===a && child[a]===6;"
        ));
    }
}

#[test]
fn inherited_to_primitive_hooks_receive_original_child_and_exact_hints() {
    for parent in ["User", "Array"] {
        yes(&format!(
            "function User(){{}}var parent={parent},child,seen='',same=true;parent[Symbol.toPrimitive]=function(hint){{same=same&&this===child;seen+=hint+',';return this.value;}};child=Object.create(Object.create(parent));child.value=7;var a=String(child),b=Number(child),c=child+1;same && seen==='string,number,default,' && a==='7' && b===7 && c===8 && !child.hasOwnProperty(Symbol.toPrimitive);"
        ));
    }
}

#[test]
fn inherited_ordinary_conversion_and_method_calls_keep_original_receiver() {
    for parent in ["User", "parseInt"] {
        yes(&format!(
            "function User(){{}}var parent={parent},child,seen='',same=true;parent.toString=function(){{same=same&&this===child;seen+='s';return this.text;}};parent.valueOf=function(){{same=same&&this===child;seen+='v';return this.number;}};parent.read=function(){{return this.text;}};child=Object.create(Object.create(parent));child.text='child';child.number=9;String(child)==='child' && +child===9 && child.read()==='child' && same && seen==='sv';"
        ));
    }
}

#[test]
fn inherited_getter_brand_checks_the_original_receiver_not_an_ancestor() {
    yes(r#"
        var symbol=Symbol('named'),boxed=Object(symbol),middle=Object.create(boxed),child=Object.create(middle);
        var caught=false;try{child.description;}catch(e){caught=String(e).indexOf('TypeError')>=0;}
        caught && boxed.description==='named' && !middle.hasOwnProperty('description') && !child.hasOwnProperty('description');
    "#);
}

#[test]
fn typed_parent_to_string_tag_is_inherited_without_changing_callability() {
    for parent in ["User", "Array"] {
        yes(&format!(
            "function User(){{}}var parent={parent};parent[Symbol.toStringTag]='TypedParent';var child=Object.create(parent);Object.prototype.toString.call(child)==='[object TypedParent]' && typeof child==='object' && typeof parent==='function';"
        ));
    }
}

#[test]
fn multi_hop_identity_survives_later_script_and_dynamic_compilation() {
    let mut runtime = Runtime::new();
    runtime.execute("var parent=eval('(function Parent(a){})');parent.value=3;var middle=Object.create(parent);var child=Function('return Object.create(middle);')();", &mut NoIo).unwrap();
    assert_eq!(runtime.execute("Object.getPrototypeOf(child)===middle && Object.getPrototypeOf(middle)===parent && child.value===3 && child.name==='Parent' && child.length===1;", &mut NoIo).unwrap(), Value::Bool(true));
}
