//! Independent public semantics for docs/OBJECT_CREATE.md.
//! Authored before implementation; no descriptor reflection or accessor syntax.
//! Ordinary TypeErrors may retain the engine's existing thrown-string form.

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

fn clean(runtime: &Runtime) {
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none(), "{report:?}");
    assert!(!runtime.is_fatal());
}
fn value(source: &str) -> Value {
    let mut runtime = Runtime::new();
    let value = runtime
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("Object.create case failed: {error}\n{source}"));
    clean(&runtime);
    value
}
fn yes(source: &str) {
    assert_eq!(value(source), Value::Bool(true), "{source}");
}
fn text(source: &str, expected: &str) {
    assert_eq!(value(source), Value::text(expected), "{source}");
}
fn type_error(expression: &str) {
    yes(&format!(
        "var caught=false;try{{{expression};}}catch(e){{caught=String(e).indexOf('TypeError')>=0;}}caught;"
    ));
}

#[test]
fn legacy_omitted_and_undefined_maps_preserve_all_typed_prototype_identities() {
    for prototype in [
        "null",
        "({seed:7})",
        "User",
        "Array",
        "Math.abs",
        "Object('ab')",
    ] {
        yes(&format!(
            "function User(){{}}var prototype={prototype},a=Object.create(prototype),b=Object.create(prototype,undefined);a!==b&&Object.getPrototypeOf(a)===prototype&&Object.getPrototypeOf(b)===prototype&&typeof a==='object'&&!Array.isArray(a)&&Object.getOwnPropertyNames(a).length===0;"
        ));
    }
    yes(
        "var original=Object.create;Object.create=17;var a=original(null);Object.getPrototypeOf(a)===null&&typeof a==='object';",
    );
}

#[test]
fn argument_expressions_finish_before_prototype_validation() {
    text(
        "var trace='',caught=false;function parent(){trace+='p';return 7;}function properties(){trace+='d';return {x:{value:1}};}try{Object.create(parent(),properties());}catch(e){caught=String(e).indexOf('TypeError')>=0;}trace+':'+caught;",
        "pd:true",
    );
    for prototype in ["undefined", "true", "7", "'text'", "Symbol('p')"] {
        type_error(&format!("Object.create({prototype},{{}})"));
    }
    type_error("Object.create()");
}

#[test]
fn map_primitives_box_but_null_and_primitive_entries_reject() {
    for properties in [
        "false",
        "true",
        "0",
        "NaN",
        "''",
        "Symbol('map')",
        "Object(3)",
    ] {
        yes(&format!(
            "var o=Object.create(null,{properties});Object.getPrototypeOf(o)===null&&Object.getOwnPropertyNames(o).length===0&&Object.getOwnPropertySymbols(o).length===0;"
        ));
    }
    type_error("Object.create(null,null)");
    type_error("Object.create(null,'ab')");
}

#[test]
fn empty_descriptor_has_present_undefined_and_all_false_defaults() {
    yes(r#"
        var parent={x:9},o=Object.create(parent,{x:{}}),own=Object.prototype.hasOwnProperty;
        var present=own.call(o,'x')&&('x' in o)&&Object.getOwnPropertyNames(o)[0]==='x';
        o.x=7;var removed=delete o.x;
        present&&o.x===undefined&&!removed&&parent.x===9&&Object.keys(o).length===0;
    "#);
}

#[test]
fn all_eight_data_flag_combinations_have_independent_observable_effects() {
    for writable in [false, true] {
        for enumerable in [false, true] {
            for configurable in [false, true] {
                yes(&format!(
                    "var o=Object.create(null,{{x:{{value:3,writable:{writable},enumerable:{enumerable},configurable:{configurable}}}}});var visible=Object.keys(o).length==={};o.x=7;var stored=o.x==={};var removed=delete o.x;var deletion=removed==={configurable};if(removed){{o.x=9;o.x=10;deletion=deletion&&o.x===10&&Object.keys(o)[0]==='x';}}visible&&stored&&deletion;",
                    usize::from(enumerable),
                    if writable { 7 } else { 3 },
                ));
            }
        }
    }
}

#[test]
fn data_values_preserve_identity_numbers_utf16_and_descriptor_independence() {
    yes(r#"
        var object={},symbol=Symbol('same'),fn=function(){return 1;},description={value:object,writable:true};
        var o=Object.create(null,{object:description,symbol:{value:symbol},fn:{value:fn},
            nan:{value:NaN},zero:{value:-0},nil:{value:null},missing:{value:undefined},
            text:{value:'\uD800x\uDFFF'}});
        description.value=17;
        o.object===object&&o.symbol===symbol&&o.fn===fn&&o.nan!==o.nan&&1/o.zero===-Infinity&&
            o.nil===null&&o.missing===undefined&&o.text.length===3&&
            o.text.charCodeAt(0)===55296&&o.text.charCodeAt(2)===57343;
    "#);
}

#[test]
fn boolean_flags_use_truthiness_without_primitive_conversion_callbacks() {
    yes(r#"
        var calls=0,flag={valueOf:function(){calls++;throw 'bad';},toString:function(){calls++;throw 'bad';}};
        flag[Symbol.toPrimitive]=function(){calls++;throw 'bad';};
        var o=Object.create(null,{x:{value:1,writable:flag,enumerable:flag,configurable:flag}});
        o.x=2;var ok=o.x===2&&Object.keys(o)[0]==='x'&&delete o.x;
        ok&&calls===0;
    "#);
    for flag in ["0", "-0", "NaN", "''", "null", "undefined"] {
        yes(&format!(
            "var o=Object.create(null,{{x:{{value:1,writable:{flag},enumerable:{flag},configurable:{flag}}}}});o.x=2;o.x===1&&Object.keys(o).length===0&&!(delete o.x);"
        ));
    }
    yes(
        "var flag=Symbol('truthy'),o=Object.create(null,{x:{value:1,writable:flag,enumerable:flag,configurable:flag}});o.x=2;o.x===2&&Object.keys(o)[0]==='x'&&delete o.x;",
    );
}

#[test]
fn inherited_nonenumerable_descriptor_fields_count_and_unknown_fields_are_ignored() {
    yes(r#"
        var calls=0,parent=Object.create(null,{
            value:{value:7},writable:{value:true},enumerable:{value:true},configurable:{value:true},
            ignored:{get:function(){calls++;throw 'unknown field';}}});
        var descriptor=Object.create(parent),o=Object.create(null,{x:descriptor});
        o.x=8;o.x===8&&Object.keys(o)[0]==='x'&&delete o.x&&calls===0;
    "#);
    yes(
        "var descriptor=Object.create(null);descriptor.value=42;var o=Object.create(null,{x:descriptor});o.x===42;",
    );
}

#[test]
fn entry_values_require_supported_objects_not_primitive_descriptor_values() {
    for entry in [
        "undefined",
        "null",
        "false",
        "3",
        "'value'",
        "Symbol('entry')",
    ] {
        type_error(&format!("Object.create(null,{{x:{entry}}})"));
    }
    yes(r#"
        function Descriptor(){}Descriptor.value=7;
        var native=Object.prototype.valueOf;native.value=9;
        var a=Object.create(null,{x:Descriptor}),b=Object.create(null,{x:native}),
            c=Object.create(null,{x:Object(3)});
        a.x===7&&b.x===9&&c.x===undefined&&Object.prototype.hasOwnProperty.call(c,'x');
    "#);
}

#[test]
fn mixed_field_presence_rejects_even_when_values_are_undefined() {
    for data in ["value", "writable"] {
        for accessor in ["get", "set"] {
            type_error(&format!(
                "Object.create(null,{{x:{{{data}:undefined,{accessor}:undefined}}}})"
            ));
        }
    }
    yes(r#"
        var parent={get:function(){return 7;}},descriptor=Object.create(parent);
        descriptor.get=undefined;
        var o=Object.create(null,{x:descriptor});o.x===undefined&&Object.prototype.hasOwnProperty.call(o,'x');
    "#);
    type_error("Object.create(null,{x:Object.create({get:undefined},{value:{value:undefined}})})");
}

#[test]
fn accessors_accept_undefined_or_callables_and_never_run_during_definition() {
    for field in ["get", "set"] {
        for invalid in [
            "null",
            "false",
            "0",
            "'callable'",
            "({})",
            "Symbol('f')",
            "Function.prototype",
        ] {
            type_error(&format!("Object.create(null,{{x:{{{field}:{invalid}}}}})"));
        }
    }
    yes(r#"
        var calls=0;function get(){calls++;return 1;}function set(v){calls++;}
        var o=Object.create(null,{x:{get:get,set:set},y:{get:undefined,set:undefined}});
        calls===0&&Object.getOwnPropertyNames(o).join(',')==='x,y'&&Object.keys(o).length===0;
    "#);
}

#[test]
fn only_current_own_enumerable_map_entries_are_selected() {
    yes(r#"
        var calls=0,parent={inherited:{value:9}},map=Object.create(parent,{
            visible:{value:{value:7},enumerable:true},
            hidden:{get:function(){calls++;throw 'hidden';}}});
        var o=Object.create(null,map);
        o.visible===7&&!('inherited' in o)&&!('hidden' in o)&&calls===0&&
            Object.getOwnPropertyNames(o).join(',')==='visible';
    "#);
    type_error("Object.create(null,{present:undefined})");
}

#[test]
fn descriptor_local_integer_string_and_symbol_order_does_not_change_existing_keys() {
    yes(r#"
        var map={},a=Symbol('same'),b=Symbol('same');
        map['10']={value:10,enumerable:true};map['2']={value:2,enumerable:true};
        map['01']={value:1,enumerable:true};map['4294967295']={value:5,enumerable:true};
        map['0']={value:0,enumerable:true};map['4294967294']={value:4,enumerable:true};
        map['-0']={value:6,enumerable:true};map['1.0']={value:7,enumerable:true};
        map['a']={value:8,enumerable:true};map[a]={value:'A',enumerable:true};
        map['b']={value:9,enumerable:true};map[b]={value:'B'};
        var before=Object.keys(map).join('|'),o=Object.create(null,map),symbols=Object.getOwnPropertySymbols(o);
        before==='10|2|01|4294967295|0|4294967294|-0|1.0|a|b'&&
            Object.keys(map).join('|')===before&&
            Object.keys(o).join('|')==='0|2|10|4294967294|01|4294967295|-0|1.0|a|b'&&
            symbols.length===2&&symbols[0]===a&&symbols[1]===b&&o[a]==='A'&&o[b]==='B';
    "#);
}

#[test]
fn snapshot_skips_deleted_keys_reads_replacements_and_ignores_new_keys() {
    yes(r#"
        var trace='',map=Object.create({later:{value:99}},{
            first:{enumerable:true,get:function(){
                trace+='f';delete map.later;map.replaced={value:22};
                map.added={value:33};return {value:1};
            }},
            later:{value:{value:2},enumerable:true,configurable:true},
            replaced:{value:{value:3},enumerable:true,writable:true}});
        var o=Object.create(null,map);
        trace==='f'&&o.first===1&&o.replaced===22&&!('later' in o)&&!('added' in o)&&
            Object.getOwnPropertyNames(o).join(',')==='first,replaced';
    "#);
    // Deletion/readdition of an already snapshotted key is a live replacement.
    yes(r#"
        var map=Object.create(null,{
            first:{enumerable:true,get:function(){delete map.next;map.next={value:8};return {value:1};}},
            next:{value:{value:2},enumerable:true,configurable:true}});
        var o=Object.create(null,map);o.next===8;
    "#);
}

#[test]
fn selected_map_getters_run_once_with_map_receiver_and_symbols_follow_strings() {
    yes(r#"
        var trace='',first=Symbol('first'),second=Symbol('second'),payload={},definitions={};
        definitions[first]={enumerable:true,get:function(){trace+=this===map?'A':'!';return {value:'a'};}};
        definitions.z={enumerable:true,get:function(){trace+=this===map?'z':'!';return {value:payload};}};
        definitions[second]={enumerable:true,get:function(){trace+=this===map?'B':'!';return {value:'b'};}};
        var map=Object.create(null,definitions),o=Object.create(null,map);
        o.z===payload&&o.z===payload&&o[first]==='a'&&o[second]==='b'&&trace==='zAB';
    "#);
}

#[test]
fn descriptor_field_getters_have_explicit_data_accessor_and_mixed_order() {
    text(
        r#"
        var trace='';function field(label,value){return {get:function(){trace+=label;return value;}};}
        var descriptor=Object.create(null,{enumerable:field('e',true),configurable:field('c',true),
            value:field('v',7),writable:field('w',true)});
        var o=Object.create(null,{x:descriptor});trace+':'+o.x;
    "#,
        "ecvw:7",
    );
    text(
        r#"
        var trace='';function field(label,value){return {get:function(){trace+=label;return value;}};}
        var descriptor=Object.create(null,{enumerable:field('e',true),configurable:field('c',true),
            get:field('g',function(){return 9;}),set:field('s',undefined)});
        var o=Object.create(null,{x:descriptor});trace+':'+o.x;
    "#,
        "ecgs:9",
    );
    text(
        r#"
        var trace='',caught=false;function field(label,value){return {get:function(){trace+=label;return value;}};}
        var descriptor=Object.create(null,{enumerable:field('e',true),configurable:field('c',true),
            value:field('v',undefined),writable:field('w',undefined),
            get:field('g',undefined),set:field('s',undefined)});
        try{Object.create(null,{x:descriptor});}catch(e){caught=String(e).indexOf('TypeError')>=0;}
        trace+':'+caught;
    "#,
        "ecvwgs:true",
    );
}

#[test]
fn field_presence_is_live_after_earlier_getters_mutate_the_descriptor() {
    yes(r#"
        var trace='',same=false,descriptor=Object.create({value:3},{
            enumerable:{get:function(){
                trace+='e';same=this===descriptor;this.value=7;delete this.writable;return true;
            }},
            writable:{value:true,configurable:true}});
        var o=Object.create(null,{x:descriptor});o.x=99;
        same&&trace==='e'&&o.x===7&&Object.keys(o)[0]==='x';
    "#);
    yes(r#"
        var descriptor=Object.create(null,{enumerable:{get:function(){
            this.get=function(){return 42;};return true;
        }}});
        var o=Object.create(null,{x:descriptor});o.x===42;
    "#);
}

#[test]
fn abrupt_conversion_preserves_identity_effects_and_skips_later_work() {
    yes(r#"
        var marker={},trace='',previous={},result=previous,poison=0;
        var parent=Object.create(null,{first:{set:function(){poison++;}}});
        var map=Object.create(null,{
            first:{enumerable:true,get:function(){trace+='a';return {value:1};}},
            second:{enumerable:true,get:function(){trace+='b';throw marker;}},
            third:{enumerable:true,get:function(){trace+='c';return {value:3};}}});
        var caught=false;try{result=Object.create(parent,map);}catch(e){caught=e===marker;}
        caught&&result===previous&&trace==='ab'&&poison===0;
    "#);
    text(
        r#"
        var trace='',caught=false,descriptor=Object.create(null,{
            enumerable:{get:function(){trace+='e';return true;}},
            get:{get:function(){trace+='g';return null;}},
            set:{get:function(){trace+='s';return function(){};}}});
        try{Object.create(null,{x:descriptor});}catch(e){caught=String(e).indexOf('TypeError')>=0;}
        trace+':'+caught;
    "#,
        "eg:true",
    );
    yes(r#"
        var calls=0,map=Object.create(null,{x:{enumerable:true,get:function(){calls++;return {value:1};}}});
        var caught=false;try{Object.create(7,map);}catch(e){caught=String(e).indexOf('TypeError')>=0;}
        caught&&calls===0;
    "#);
}

#[test]
fn own_and_inherited_getters_receive_the_original_object_and_zero_arguments() {
    yes(r#"
        var receivers=[],arity=-1,o=Object.create(null,{x:{get:function(){
            receivers.push(this);arity=arguments.length;return this.marker;
        },enumerable:true}});
        o.marker=7;var child=Object.create(o);child.marker=9;
        var first=o.x,second=child.x;
        first===7&&second===9&&arity===0&&receivers[0]===o&&receivers[1]===child&&
            !Object.prototype.hasOwnProperty.call(child,'x');
    "#);
}

#[test]
fn inherited_setters_receive_one_uncoerced_value_without_creating_a_shadow() {
    yes(r#"
        var receiver,arity=-1,calls=0,o=Object.create(null,{x:{set:function(value){
            receiver=this;arity=arguments.length;calls++;this.saved=value;return 'ignored';
        }}});
        var value={},first=(o.x=value),child=Object.create(o),symbol=Symbol('v'),second=(child.x=symbol);
        first===value&&second===symbol&&o.saved===value&&child.saved===symbol&&receiver===child&&
            calls===2&&arity===1&&!Object.prototype.hasOwnProperty.call(child,'x');
    "#);
}

#[test]
fn missing_accessor_halves_preserve_presence_and_ignore_non_strict_writes() {
    yes(r#"
        var gets=0,sets=0,o=Object.create(null,{
            read:{get:function(){gets++;return 3;}},
            write:{set:function(v){sets++;}},
            neither:{get:undefined,set:undefined}});
        var child=Object.create(o),a=(o.read=9),b=(child.read=11);
        var one=o.read,two=child.read,unread=o.write,empty=o.neither;
        o.neither=7;
        a===9&&b===11&&one===3&&two===3&&gets===2&&sets===0&&unread===undefined&&empty===undefined&&
            Object.prototype.hasOwnProperty.call(o,'neither')&&
            !Object.prototype.hasOwnProperty.call(child,'read')&&o.neither===undefined;
    "#);
}

#[test]
fn compound_and_update_operations_call_getters_and_setters_in_language_order() {
    text(
        r#"
        var trace='',stored={valueOf:function(){trace+='v';return 5;}},o=Object.create(null,{
            x:{get:function(){trace+='g';return stored;},set:function(value){trace+='s';stored=value;}}});
        function base(){trace+='b';return o;}function key(){trace+='k';return 'x';}
        function rhs(){trace+='r';return 2;}var result=(base()[key()]-=rhs());
        trace+':'+result+':'+stored;
    "#,
        "bkgrvs:3:3",
    );
    for (operator, prefix, returned, stored) in [
        ("++", true, 4, 4),
        ("++", false, 3, 4),
        ("--", true, 2, 2),
        ("--", false, 3, 2),
    ] {
        let expression = if prefix {
            format!("{operator}base()[key()]")
        } else {
            format!("base()[key()]{operator}")
        };
        yes(&format!(
            "var trace='',stored='3',o=Object.create(null,{{x:{{get:function(){{trace+='g';return stored;}},set:function(v){{trace+='s';stored=v;}}}}}});function base(){{trace+='b';return o;}}function key(){{trace+='k';return 'x';}}var result={expression};trace==='bkgs'&&result==={returned}&&stored==={stored};"
        ));
    }
    yes(r#"
        var marker={},trace='',o=Object.create(null,{x:{
            get:function(){trace+='g';return 1;},set:function(){trace+='s';throw marker;}}}),caught=false;
        try{o.x+=2;}catch(e){caught=e===marker;}caught&&trace==='gs';
    "#);
}

#[test]
fn bound_and_native_accessor_callbacks_keep_existing_receiver_rules() {
    yes(r#"
        var fixed={base:10},seen,arity=-1,argument;
        function read(extra){return this.base+extra;}
        function write(prefix,value){seen=this;arity=arguments.length;argument=prefix+value;}
        var o=Object.create(null,{x:{get:read.bind(fixed,2),set:write.bind(fixed,'p')},
            self:{get:Object.prototype.valueOf}});
        var result=(o.x=3);
        o.x===12&&o.self===o&&result===3&&seen===fixed&&arity===2&&argument==='p3';
    "#);
}

#[test]
fn reflection_and_deletion_do_not_invoke_accessor_bodies() {
    yes(r#"
        var calls=0,key=Symbol('key'),definitions={
            hidden:{get:function(){calls++;throw 'hidden';}},
            visible:{get:function(){calls++;throw 'visible';},enumerable:true,configurable:true}};
        definitions[key]={get:function(){calls++;throw 'symbol';},configurable:true};
        var o=Object.create(null,definitions),own=Object.prototype.hasOwnProperty;
        var metadata=Object.getOwnPropertyNames(o).join(',')==='hidden,visible'&&
            Object.keys(o).join(',')==='visible'&&Object.getOwnPropertySymbols(o)[0]===key&&
            own.call(o,'hidden')&&own.call(o,key)&&('hidden' in o)&&(key in o);
        var deleted=delete o.visible,protectedName=delete o.hidden,deletedSymbol=delete o[key];
        metadata&&deleted&&!protectedName&&deletedSymbol&&calls===0&&
            Object.getOwnPropertyNames(o).join(',')==='hidden'&&Object.getOwnPropertySymbols(o).length===0;
    "#);
}

#[test]
fn fresh_definitions_bypass_inherited_readonly_fields_and_setters() {
    yes(r#"
        var calls=0,parent=Object.create(null,{x:{set:function(){calls++;}}});
        var o=Object.create(parent,{x:{value:7,writable:true}});
        o.x=8;o.x===8&&calls===0&&Object.prototype.hasOwnProperty.call(o,'x');
    "#);
    for parent in ["User", "Array"] {
        yes(&format!(
            "function User(a){{}}var parent={parent},payload={{}},o=Object.create(parent,{{length:{{value:7,writable:true}},name:{{value:'child'}},prototype:{{value:payload}}}});o.length=8;o.length===8&&o.name==='child'&&o.prototype===payload&&Object.getPrototypeOf(o)===parent&&typeof o==='object'&&!Array.isArray(o);"
        ));
    }
    yes(
        "var parent=Object('ab'),o=Object.create(parent,{'0':{value:'x'},length:{value:9}});o[0]==='x'&&o.length===9&&parent[0]==='a'&&parent.length===2&&!Array.isArray(o);",
    );
    yes(
        "var parent=Object.create(null,{w:{value:1,writable:true},r:{value:2}}),child=Object.create(parent);child.w=3;child.r=4;child.w===3&&parent.w===1&&child.r===2&&Object.prototype.hasOwnProperty.call(child,'w')&&!Object.prototype.hasOwnProperty.call(child,'r');",
    );
}

#[test]
fn special_property_names_do_not_change_the_chosen_internal_prototype() {
    yes(r#"
        var parent={},payload={},map={};
        map['__proto__']={value:payload,enumerable:true};
        map.constructor={value:42};map.prototype={value:7};
        var o=Object.create(parent,map);
        Object.getPrototypeOf(o)===parent&&o.__proto__===payload&&o.constructor===42&&o.prototype===7&&
            Object.getOwnPropertyNames(o).join(',')==='__proto__,constructor,prototype';
    "#);
}

#[test]
fn symbol_accessors_and_existing_primitive_hook_keep_original_receivers() {
    yes(r#"
        var key=Symbol('key'),other=Symbol('key'),receiver,stored,definitions={};
        definitions[key]={get:function(){receiver=this;return stored;},set:function(v){receiver=this;stored=v;}};
        definitions[other]={value:9};
        var o=Object.create(null,definitions),child=Object.create(o),payload={};
        child[key]=payload;var result=child[key];
        result===payload&&receiver===child&&o[other]===9&&
            Object.getOwnPropertySymbols(o).length===2&&Object.getOwnPropertySymbols(child).length===0;
    "#);
    yes(r#"
        var calls=0,getterThis,hookThis,hint,definitions={};
        definitions[Symbol.toPrimitive]={get:function(){
            calls++;getterThis=this;return function(h){hookThis=this;hint=h;return 13;};
        }};
        var o=Object.create(null,definitions),number=+o;
        number===13&&getterThis===o&&hookThis===o&&hint==='number'&&calls===1&&
            Object.getOwnPropertySymbols(o)[0]===Symbol.toPrimitive;
    "#);
}

#[test]
fn retained_dynamic_accessors_and_ordinary_faults_preserve_lifetime_and_recovery() {
    let mut runtime = Runtime::new();
    let source = String::from(
        "function factory(seed){return Object.create(null,{value:{get:function(){return seed;},set:function(v){seed=v;}}});}factory;",
    );
    let factory = runtime.execute(&source, &mut NoIo).unwrap();
    drop(source);
    let first = runtime
        .invoke(
            factory.clone(),
            Value::Undefined,
            vec![Value::Number(2.0)],
            &mut NoIo,
        )
        .unwrap();
    let second = runtime
        .invoke(
            factory,
            Value::Undefined,
            vec![Value::Number(7.0)],
            &mut NoIo,
        )
        .unwrap();
    assert_ne!(first, second);
    runtime.set_global("first", first);
    runtime.set_global("second", second);
    assert_eq!(
        runtime
            .execute(
                "first.value=4;first.value===4&&second.value===7;",
                &mut NoIo
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(runtime.execute(
        "var o=Object.create(null,{length:{get:function(){return undefined;}}});o.length.name;",
        &mut NoIo,
    ).unwrap_err(), "Uncaught JavaScript exception: TypeError: property access on null or undefined [member operation=resolve-read base=undefined key=name] [producer kind=getter-result key=length]");
    assert_eq!(
        runtime
            .execute(
                "var o=Object.create(null,{length:{set:function(){}}});o.length.name;",
                &mut NoIo,
            )
            .unwrap_err(),
        "Uncaught JavaScript exception: TypeError: property access on null or undefined [member operation=resolve-read base=undefined key=name] [producer kind=present-property key=length]"
    );
    assert_eq!(
        runtime.execute("21*2;", &mut NoIo).unwrap(),
        Value::Number(42.0)
    );
    clean(&runtime);
    yes(r#"
        function direct(){var local=2;return eval("Object.create(null,{x:{get:function(){return local;},set:function(v){local=v;}}})");}
        var a=direct(),b=direct(),made=Function('seed','return Object.create(null,{x:{get:function(){return seed;},set:function(v){seed=v;}}});')(7);
        a.x=4;made.x=9;a!==b&&a.x===4&&b.x===2&&made.x===9;
    "#);
    yes(r#"
        var marker={},trace='',o=Object.create(null,{x:{get:function(){trace+='g';throw marker;}}}),caught=false;
        try{o.x;}catch(e){caught=e===marker;trace+='c';}finally{trace+='f';}
        caught&&trace==='gcf';
    "#);
}
