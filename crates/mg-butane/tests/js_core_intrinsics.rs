//! Independently authored acceptance for docs/CORE_INTRINSICS.md.
//! ES5.1 sections 15, 15.2.4, 15.4.4, 15.5.4, 15.6.2/4 and 15.7.2/4.
//! No website source, replacement runtime, accessor syntax or descriptor API.

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

const CORE: [&str; 5] = ["Object", "Array", "String", "Number", "Boolean"];

fn clean(runtime: &Runtime) {
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none(), "{report:?}");
    assert!(!runtime.is_fatal());
}

fn yes(source: &str) {
    let mut runtime = Runtime::new();
    let value = runtime
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("core intrinsic case failed: {error}\n{source}"));
    assert_eq!(value, Value::Bool(true), "{source}");
    clean(&runtime);
}

fn type_error(setup: &str, expression: &str) {
    // Ordinary runtime failures may still be strings, not Error instances.
    yes(&format!(
        "{setup}var caught=false,finished=false;try{{({expression});finished=true;}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&!finished;"
    ));
}

#[test]
fn all_five_backlinks_are_own_non_enumerable_original_constructors() {
    for constructor in CORE {
        yes(&format!(
            "var p={constructor}.prototype;var names=Object.getOwnPropertyNames(p);p.constructor==={constructor}&&p.hasOwnProperty('constructor')&&names.indexOf('constructor')>=0&&Object.keys(p).indexOf('constructor')<0;"
        ));
    }
}

#[test]
fn backlinks_are_writable_configurable_and_use_ordinary_readdition() {
    for constructor in CORE {
        yes(&format!(
            "var p={constructor}.prototype,original=p.constructor,replacement={{mark:7}};p.constructor=replacement;var replaced=p.constructor===replacement&&Object.keys(p).indexOf('constructor')<0;var removed=delete p.constructor;var absent=!p.hasOwnProperty('constructor');p.constructor=original;replaced&&removed&&absent&&p.constructor==={constructor}&&Object.keys(p).indexOf('constructor')>=0;"
        ));
    }
}

#[test]
fn inherited_constructor_reads_and_child_shadows_keep_owner_separate() {
    for constructor in CORE {
        yes(&format!(
            "var p={constructor}.prototype,child=Object.create(p),original=p.constructor;var inherited=child.constructor==={constructor}&&!child.hasOwnProperty('constructor')&&('constructor' in child);child.constructor=17;var own=child.constructor===17&&p.constructor===original;var removed=delete child.constructor;inherited&&own&&removed&&child.constructor===original&&!child.hasOwnProperty('constructor');"
        ));
    }
}

#[test]
fn constructor_prototype_properties_remain_readonly_and_nondeletable() {
    for constructor in CORE {
        yes(&format!(
            "var original={constructor}.prototype;{constructor}.prototype={{wrong:true}};var removed=delete {constructor}.prototype;{constructor}.prototype===original&&!removed&&{constructor}.hasOwnProperty('prototype')&&Object.keys({constructor}).indexOf('prototype')<0;"
        ));
    }
}

#[test]
fn original_parent_chains_and_empty_array_storage_are_preserved() {
    yes(r#"
        Object.getPrototypeOf(Object.prototype)===null&&
        Object.getPrototypeOf(Array.prototype)===Object.prototype&&
        Object.getPrototypeOf(String.prototype)===Object.prototype&&
        Object.getPrototypeOf(Number.prototype)===Object.prototype&&
        Object.getPrototypeOf(Boolean.prototype)===Object.prototype&&
        Array.isArray(Array.prototype)&&Array.prototype.length===0&&
        !(0 in Array.prototype)&&Object.prototype.toString.call(Array.prototype)==='[object Array]';
    "#);
}

#[test]
fn ordinary_instances_inherit_backlinks_without_own_copies() {
    yes(r#"
        var values=[{},[],Object('text'),Object(3),Object(false)];
        var constructors=[Object,Array,String,Number,Boolean],valid=true;
        for(var i=0;i<values.length;i++){
            valid=valid&&values[i].constructor===constructors[i]&&!values[i].hasOwnProperty('constructor');
        }
        valid;
    "#);
}

#[test]
fn global_reassignment_does_not_rewrite_original_backlinks_or_parents() {
    yes(r#"
        var O=Object,A=Array,S=String,N=Number,B=Boolean;
        var op=O.prototype,ap=A.prototype,sp=S.prototype,np=N.prototype,bp=B.prototype;
        Object=function(){};Array=function(){};String=function(){};Number=function(){};Boolean=function(){};
        var a=[],o={},s=O('x'),n=O(7),b=O(false);
        op.constructor===O&&ap.constructor===A&&sp.constructor===S&&np.constructor===N&&bp.constructor===B&&
        O.getPrototypeOf(o)===op&&O.getPrototypeOf(a)===ap&&O.getPrototypeOf(s)===sp&&
        O.getPrototypeOf(n)===np&&O.getPrototypeOf(b)===bp;
    "#);
}

#[test]
fn string_prototype_has_real_empty_payload_and_length_metadata() {
    yes(r#"
        var p=String.prototype;
        var initial=p.valueOf()===''&&p.toString()===''&&String(p)===''&&
            p.length===0&&p.hasOwnProperty('length')&&!p.hasOwnProperty('0')&&
            Object.getOwnPropertyNames(p).indexOf('length')>=0&&Object.keys(p).indexOf('length')<0;
        p.length=9;var removed=delete p.length;
        initial&&p.length===0&&!removed&&Object.prototype.toString.call(p)==='[object String]';
    "#);
}

#[test]
fn number_and_boolean_prototypes_have_real_positive_zero_and_false_payloads() {
    yes(r#"
        var n=Number.prototype,b=Boolean.prototype;
        n.valueOf()===0&&1/n.valueOf()===Infinity&&n.toString()==='0'&&Number(n)===0&&
            b.valueOf()===false&&b.toString()==='false'&&Boolean(b)===true&&
            Object.prototype.toString.call(n)==='[object Number]'&&
            Object.prototype.toString.call(b)==='[object Boolean]';
    "#);
}

#[test]
fn inherited_prototypes_do_not_forge_branded_receivers_or_run_coercion() {
    for constructor in ["String", "Number", "Boolean"] {
        yes(&format!(
            "var touched=0,p={constructor}.prototype,child=Object.create(p);child.valueOf=function(){{touched++;return 1;}};child.toString=function(){{touched++;return 'x';}};child[Symbol.toPrimitive]=function(){{touched++;return 2;}};var a=false,b=false;try{{p.valueOf.call(child);}}catch(e){{a=String(e).indexOf('TypeError')>=0;}}try{{p.toString.call(child);}}catch(e){{b=String(e).indexOf('TypeError')>=0;}}a&&b&&touched===0;"
        ));
    }
}

#[test]
fn presentation_tags_do_not_create_or_destroy_primitive_payloads() {
    yes(r#"
        var n=Number.prototype,fake={};n[Symbol.toStringTag]='Pretend';fake[Symbol.toStringTag]='Number';
        var caught=false;try{Number.prototype.valueOf.call(fake);}catch(e){caught=String(e).indexOf('TypeError')>=0;}
        n.valueOf()===0&&Object.prototype.toString.call(n)==='[object Pretend]'&&
            Object.prototype.toString.call(fake)==='[object Number]'&&caught;
    "#);
}

#[test]
fn existing_utf16_boxing_and_string_constructor_symbol_distinction_survive() {
    yes(r#"
        var text='\uD800\uDC00\uDFFF',boxed=Object(text),made=new String(text),symbol=Symbol('label');
        var rejected=false;try{new String(symbol);}catch(e){rejected=String(e).indexOf('TypeError')>=0;}
        boxed.valueOf()===text&&made.valueOf()===text&&boxed!==made&&boxed.length===3&&
            boxed[0].charCodeAt(0)===55296&&boxed[2].charCodeAt(0)===57343&&
            Object.getPrototypeOf(made)===String.prototype&&made.constructor===String&&
            String(symbol)==='Symbol(label)'&&rejected;
    "#);
}

#[test]
fn number_construction_distinguishes_omission_undefined_and_signed_zero() {
    yes(r#"
        var absent=new Number(),explicit=new Number(undefined),negative=new Number(-0),positive=new Number(0);
        typeof absent==='object'&&absent!==positive&&absent.valueOf()===0&&1/absent.valueOf()===Infinity&&
            explicit.valueOf()!==explicit.valueOf()&&1/negative.valueOf()===-Infinity&&
            1/positive.valueOf()===Infinity&&Object.getPrototypeOf(absent)===Number.prototype&&
            absent.constructor===Number&&!absent.hasOwnProperty('constructor')&&absent instanceof Number;
    "#);
}

#[test]
fn number_construction_preserves_nan_infinities_and_primitive_conversions() {
    yes(r#"
        var nan=new Number(NaN);
        nan.valueOf()!==nan.valueOf()&&(new Number(Infinity)).valueOf()===Infinity&&
            (new Number(-Infinity)).valueOf()===-Infinity&&(new Number(null)).valueOf()===0&&
            (new Number(true)).valueOf()===1&&(new Number(' 17 ')).valueOf()===17&&
            (new Number('')).valueOf()===0;
    "#);
}

#[test]
fn boolean_construction_uses_truthiness_and_keeps_wrapper_identity() {
    yes(r#"
        var absent=new Boolean(),explicit=new Boolean(undefined),truthy=new Boolean('false');
        typeof absent==='object'&&absent!==explicit&&absent.valueOf()===false&&explicit.valueOf()===false&&
            (new Boolean(null)).valueOf()===false&&(new Boolean(0)).valueOf()===false&&
            (new Boolean(-0)).valueOf()===false&&(new Boolean(NaN)).valueOf()===false&&
            (new Boolean('')).valueOf()===false&&truthy.valueOf()===true&&
            Boolean(absent)===true&&absent instanceof Boolean&&
            Object.getPrototypeOf(absent)===Boolean.prototype&&absent.constructor===Boolean&&
            !absent.hasOwnProperty('constructor');
    "#);
}

#[test]
fn boolean_never_coerces_objects_symbols_or_boxed_falsy_values() {
    yes(r#"
        var touched=0,value={valueOf:function(){touched++;throw 'value';},toString:function(){touched++;throw 'text';}};
        value[Symbol.toPrimitive]=function(){touched++;throw 'hook';};
        (new Boolean(value)).valueOf()===true&&(new Boolean(Symbol('x'))).valueOf()===true&&
            (new Boolean(Object(false))).valueOf()===true&&(new Boolean(Object(0))).valueOf()===true&&
            (new Boolean(new Boolean(false))).valueOf()===true&&touched===0;
    "#);
}

#[test]
fn number_conversion_uses_one_number_hint_with_original_receiver() {
    yes(r#"
        var order='',valid=true,value={valueOf:function(){throw 'fallback';},toString:function(){throw 'fallback';}};
        value[Symbol.toPrimitive]=function(hint){order+=hint;valid=valid&&this===value;return '23';};
        var result=new Number(value);
        result.valueOf()===23&&order==='number'&&valid;
    "#);
}

#[test]
fn number_conversion_falls_back_in_order_without_repeating_reads() {
    yes(r#"
        var order='',value={valueOf:function(){order+='v';return {};},toString:function(){order+='s';return '31';}};
        var result=new Number(value);result.valueOf()===31&&order==='vs';
    "#);
}

#[test]
fn callee_and_all_argument_expressions_run_before_single_number_conversion() {
    yes(r#"
        var trace='',value={valueOf:function(){trace+='v';return 8;}};
        var unused={valueOf:function(){throw 'unused number';},toString:function(){throw 'unused string';}};
        unused[Symbol.toPrimitive]=function(){throw 'unused hook';};
        function target(){trace+='c';return Number;}
        function first(){trace+='a';return value;}
        function extra(){trace+='e';return unused;}
        var result=new (target())(first(),extra());result.valueOf()===8&&trace==='caev';
    "#);
}

#[test]
fn extra_argument_throw_prevents_conversion_and_publication() {
    yes(r#"
        var trace='',marker={},old={},result=old,caught=false;
        var value={valueOf:function(){trace+='v';return 1;}};
        function extra(){trace+='e';throw marker;}
        try{result=new Number(value,extra());}catch(error){caught=error===marker;}
        caught&&trace==='e'&&result===old;
    "#);
}

#[test]
fn number_conversion_exception_keeps_identity_effects_and_finally_order() {
    yes(r#"
        var marker={},old={},result=old,trace='',caught=false;
        var value={valueOf:function(){trace+='v';throw marker;},toString:function(){trace+='s';return '1';}};
        try{result=new Number(value);}catch(error){caught=error===marker;trace+='c';}finally{trace+='f';}
        caught&&trace==='vcf'&&result===old;
    "#);
}

#[test]
fn number_symbol_and_invalid_primitive_hook_results_reject() {
    type_error("var value=Symbol('x');", "new Number(value)");
    type_error(
        "var value={};value[Symbol.toPrimitive]=function(){return Symbol('x');};",
        "new Number(value)",
    );
    type_error(
        "var value={};value[Symbol.toPrimitive]=function(){return {};};",
        "new Number(value)",
    );
}

#[test]
fn ordinary_calls_and_existing_object_array_string_construction_stay_distinct() {
    yes(r#"
        var marker={},o=new Object(marker),a=new Array(3),s=new String('ab');
        Number()===0&&Number(undefined)!==Number(undefined)&&typeof Number('4')==='number'&&
            Boolean()===false&&Boolean({})===true&&typeof Boolean(1)==='boolean'&&
            Object(marker)===marker&&o===marker&&Array.isArray(a)&&a.length===3&&!(0 in a)&&
            s.valueOf()==='ab'&&s!==String('ab')&&Object.getPrototypeOf(s)===String.prototype;
    "#);
}

#[test]
fn bound_number_and_boolean_construct_fresh_target_instances() {
    yes(r#"
        var N=Number.bind(null,'9'),B=Boolean.bind(null,0),n1=new N(),n2=new N(),b=new B();
        n1!==n2&&n1.valueOf()===9&&n2.valueOf()===9&&b.valueOf()===false&&
            n1 instanceof N&&n1 instanceof Number&&b instanceof B&&b instanceof Boolean&&
            Object.getPrototypeOf(n1)===Number.prototype&&Object.getPrototypeOf(b)===Boolean.prototype&&
            !N.hasOwnProperty('prototype')&&!B.hasOwnProperty('prototype');
    "#);
}

#[test]
fn bound_prefixes_outer_arguments_and_ignored_receiver_preserve_order() {
    yes(r#"
        var trace='',receiver={valueOf:function(){throw 'receiver';},toString:function(){throw 'receiver';}};
        receiver[Symbol.toPrimitive]=function(){throw 'receiver';};
        var value={valueOf:function(){trace+='v';return 12;}};
        var unused={valueOf:function(){throw 'extra';},toString:function(){throw 'extra';}};
        var first=Number.bind(receiver,value),second=first.bind(receiver,unused);
        function later(){trace+='e';return unused;}
        var result=new second(later());
        result.valueOf()===12&&trace==='ev'&&result instanceof first&&result instanceof second&&
            result instanceof Number&&second()===12&&trace==='evv';
    "#);
}

#[test]
fn bound_omission_undefined_negative_zero_and_false_remain_distinct() {
    yes(r#"
        var omitted=Number.bind(null),explicit=Number.bind(null,undefined),negative=Number.bind(null,-0);
        var n=new explicit(),b=new (Boolean.bind(null))();
        1/(new omitted()).valueOf()===Infinity&&n.valueOf()!==n.valueOf()&&
            1/(new negative()).valueOf()===-Infinity&&b.valueOf()===false;
    "#);
}

#[test]
fn assigned_bound_prototype_does_not_replace_original_intrinsic_parent() {
    yes(r#"
        var Bound=Number.bind(null,7),fake={wrong:true};Bound.prototype=fake;
        var value=new Bound();
        Object.getPrototypeOf(value)===Number.prototype&&value.wrong===undefined&&
            value instanceof Bound&&value instanceof Number&&Bound.prototype===fake;
    "#);
}

#[test]
fn saved_and_bound_constructors_survive_global_and_backlink_reassignment() {
    yes(r#"
        var O=Object,N=Number,B=Boolean,NP=N.prototype,BP=B.prototype;
        var BoundN=N.bind(null,14),BoundB=B.bind(null,false);
        Number=function(){throw 'replacement';};Boolean=function(){throw 'replacement';};
        NP.constructor=Number;BP.constructor=Boolean;
        var direct=new N(4),bound=new BoundN(),boolean=new BoundB();
        direct.valueOf()===4&&bound.valueOf()===14&&boolean.valueOf()===false&&
            O.getPrototypeOf(direct)===NP&&O.getPrototypeOf(bound)===NP&&O.getPrototypeOf(boolean)===BP&&
            direct instanceof N&&bound instanceof BoundN&&boolean instanceof B;
    "#);
}

#[test]
fn bound_number_conversion_errors_are_catchable_and_keep_identity() {
    yes(r#"
        var marker={},trace='',input={valueOf:function(){trace+='v';throw marker;}};
        var Bound=Number.bind(null,input),old={},result=old,caught=false;
        try{result=new Bound();}catch(error){caught=error===marker;trace+='c';}finally{trace+='f';}
        caught&&result===old&&trace==='vcf';
    "#);
}

#[test]
fn unrelated_native_and_function_prototype_construction_limits_remain() {
    for expression in [
        "new parseInt()",
        "new (Math.abs.bind(null))()",
        "new (Array.prototype.slice.bind(null))()",
    ] {
        yes(&format!(
            "var caught=false;try{{{expression};}}catch(error){{caught=String(error).length>0;}}caught;"
        ));
    }
    type_error("", "new Symbol('x')");
    type_error("", "Function.prototype()");
    type_error("", "Function.prototype.bind.call(Function.prototype,null)");
    yes("typeof Function.prototype==='object'&&Function.prototype.constructor===Function;");
}

#[test]
fn constructor_and_instance_lifetime_crosses_execute_and_public_invoke() {
    let mut runtime = Runtime::new();
    runtime.execute(
        "var Saved=Number,Bound=Number.bind(null,19),held=new Boolean(false);function make(){return new Bound();}Number=function(){throw 'changed';};",
        &mut NoIo,
    ).unwrap();
    let make = runtime.get_global("make");
    let first = runtime
        .invoke(make.clone(), Value::Undefined, Vec::new(), &mut NoIo)
        .unwrap();
    let second = runtime
        .invoke(make, Value::Null, Vec::new(), &mut NoIo)
        .unwrap();
    assert_ne!(first, second);
    runtime.set_global("first", first);
    runtime.set_global("second", second);
    assert_eq!(runtime.execute(
        "first.valueOf()===19&&second.valueOf()===19&&first instanceof Saved&&first instanceof Bound&&held.valueOf()===false&&held.constructor===Boolean;",
        &mut NoIo,
    ).unwrap(), Value::Bool(true));
    clean(&runtime);
}

#[test]
fn eval_and_function_created_closures_retain_original_intrinsics_and_payloads() {
    yes(r#"
        var dynamic=eval('(function(){var N=Number,B=Boolean;return function(){return [new N(6),new B(false)];};})()');
        var compiled=Function('var N=Number;return function(){return new N(-0);};')();
        var saved=Number;Number=function(){throw 'changed';};Boolean=function(){throw 'changed';};
        var values=dynamic(),negative=compiled();
        values[0].valueOf()===6&&values[1].valueOf()===false&&negative instanceof saved&&
            1/negative.valueOf()===-Infinity;
    "#);
}

#[test]
fn ordinary_conversion_failure_does_not_poison_later_execution() {
    let mut runtime = Runtime::new();
    runtime.execute("var marker={};function fail(){return new Number({valueOf:function(){throw marker;}});}", &mut NoIo).unwrap();
    assert!(
        runtime
            .invoke(
                runtime.get_global("fail"),
                Value::Undefined,
                Vec::new(),
                &mut NoIo
            )
            .is_err()
    );
    assert!(!runtime.is_fatal());
    assert_eq!(
        runtime
            .execute(
                "(new Number(42)).valueOf()===42&&(new Boolean(false)).valueOf()===false;",
                &mut NoIo
            )
            .unwrap(),
        Value::Bool(true)
    );
    clean(&runtime);
}
