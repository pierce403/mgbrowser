//! Independent semantic preservation for deferred user-function prototypes.
//! ES5.1 13.2, 13.2.2 and 15.3.5.2/3 establish identity and attributes:
//! https://262.ecma-international.org/5.1/#sec-13.2
//! https://262.ecma-international.org/5.1/#sec-15.3.5.3
//! Retain the documented partial descriptors and bounded prototype traversal.
//! Allocation benefits and first-admission failures have separate tests.

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
        .unwrap_or_else(|error| panic!("function prototype case failed: {error}\n{source}"));
    assert_eq!(result, Value::Bool(true), "{source}");
}

#[test]
fn each_function_has_one_distinct_ordinary_default_with_its_own_backlink() {
    yes(r#"
        function First(){}function Second(){}
        var a=First.prototype,b=Second.prototype;
        a===First.prototype && b===Second.prototype && a!==b &&
            typeof a==='object' && !Array.isArray(a) && a.constructor===First &&
            b.constructor===Second && Object.getPrototypeOf(a)===Object.prototype &&
            Object.getPrototypeOf(First)===Function.prototype;
    "#);
}

#[test]
fn repeated_factory_closures_share_code_but_not_prototypes_or_callees() {
    yes(r#"
        function make(value){return function Same(){return value;};}
        var a=make(7),b=make(9),pa=a.prototype,pb=b.prototype;
        a!==b && pa!==pb && pa.constructor===a && pb.constructor===b &&
            pa.constructor()===7 && pb.constructor()===9 && a.name===b.name;
    "#);
}

#[test]
fn arguments_callee_and_named_self_read_the_same_default() {
    yes(r#"
        var callable=function Named(){
            return arguments.callee===Named && arguments.callee.prototype===Named.prototype;
        };
        callable() && callable.prototype.constructor===callable;
    "#);
}

#[test]
fn metadata_only_queries_report_a_real_non_enumerable_own_prototype() {
    yes(r#"
        function F(a,b){}
        var names=Object.getOwnPropertyNames(F),keys=Object.keys(F),walk='';
        for(var key in F)walk+=key;
        names.join(',')==='length,name,prototype' && keys.length===0 && walk==='' &&
            F.hasOwnProperty('prototype') && Object.prototype.hasOwnProperty.call(F,'prototype') &&
            ('prototype' in F) && F.length===2 && F.name==='F' &&
            Object.getOwnPropertySymbols(F).length===0 && F.prototype.constructor===F;
    "#);
}

#[test]
fn failed_delete_before_or_after_read_does_not_remove_the_default() {
    yes(r#"
        function F(){}var firstDelete=delete F.prototype,p=F.prototype;
        var secondDelete=delete F['prototype'];
        !firstDelete && !secondDelete && F.prototype===p && p.constructor===F &&
            F.hasOwnProperty('prototype') && ('prototype' in F);
    "#);
}

#[test]
fn replacing_an_unread_prototype_keeps_its_original_property_attributes() {
    yes(r#"
        function F(){}var replacement={mark:3};F.prototype=replacement;
        var removed=delete F.prototype,keys=Object.keys(F),names=Object.getOwnPropertyNames(F);
        !removed && F.prototype===replacement && F.hasOwnProperty('prototype') &&
            keys.length===0 && names.join(',')==='length,name,prototype' &&
            !replacement.hasOwnProperty('constructor') &&
            replacement.constructor===Object.prototype.constructor;
    "#);
}

#[test]
fn every_primitive_replacement_is_preserved_without_becoming_a_new_default() {
    for replacement in ["undefined", "null", "false", "0", "'text'", "Symbol('p')"] {
        yes(&format!(
            "function F(){{}}var value={replacement};F.prototype=value;var removed=delete F.prototype;F.prototype===value && !removed && F.hasOwnProperty('prototype') && Object.keys(F).length===0;"
        ));
    }
}

#[test]
fn replacing_a_read_default_preserves_the_old_backlink_and_instances() {
    yes(r#"
        function F(){this.own=1;}var old=F.prototype,first=new F(),next={mark:2};
        F.prototype=next;var second=new F();
        old!==next && old.constructor===F && Object.getPrototypeOf(first)===old &&
            Object.getPrototypeOf(second)===next && !(first instanceof F) && second instanceof F &&
            first.constructor===F && first.own===1 && second.mark===2;
    "#);
}

#[test]
fn default_constructor_backlink_can_be_changed_deleted_and_readded_independently() {
    yes(r#"
        function F(){}function Other(){}var p=F.prototype;
        var initial=Object.keys(p).length===0 && p.hasOwnProperty('constructor');
        p.constructor=Other;var changed=p.constructor===Other && F.prototype===p;
        var removed=delete p.constructor,noOwn=!p.hasOwnProperty('constructor');
        var revealed=p.constructor===Object.prototype.constructor;
        p.constructor=F;
        initial && changed && removed && noOwn && revealed && p.constructor===F &&
            Object.keys(p).join(',')==='constructor' && F.prototype===p;
    "#);
}

#[test]
fn readonly_function_name_and_length_are_unaffected_by_prototype_replacement() {
    yes(r#"
        function F(first,second){}var replacement={};F.prototype=replacement;
        F.name='renamed';F.length=17;
        var removedName=delete F.name,removedLength=delete F.length;
        F.name==='F' && F.length===2 && !removedName && !removedLength &&
            F.prototype===replacement && Object.keys(F).length===0;
    "#);
}

#[test]
fn inherited_reads_materialize_the_actual_owner_not_the_receiver() {
    yes(r#"
        function F(){}var child=Object.create(F),deep=Object.create(child);
        var p=deep.prototype;
        p===child.prototype && p===F.prototype && p.constructor===F &&
            !child.hasOwnProperty('prototype') && !deep.hasOwnProperty('prototype') &&
            Object.getPrototypeOf(child)===F && Object.getPrototypeOf(deep)===child;
    "#);
}

#[test]
fn child_writes_shadow_then_delete_reveals_the_same_owner_default() {
    yes(r#"
        function F(){}var child=Object.create(F),own={mark:8};
        child.prototype=own;var shadowed=child.prototype===own && child.hasOwnProperty('prototype');
        var parent=F.prototype,removed=delete child.prototype;
        shadowed && removed && parent!==own && parent.constructor===F &&
            child.prototype===parent && !child.hasOwnProperty('prototype') && F.prototype===parent;
    "#);
}

#[test]
fn own_metadata_shadows_far_prototype_names_during_enumeration() {
    yes(r#"
        Object.prototype.prototype='far';Function.prototype.inherited=1;
        function F(){}F.extra=2;var child=Object.create(F);child.local=3;
        var seen='';for(var key in child)seen+=key+',';
        seen==='local,extra,inherited,' && !child.hasOwnProperty('prototype') &&
            'prototype' in child && child.prototype===F.prototype &&
            F.prototype.constructor===F && typeof child.prototype==='object';
    "#);
}

#[test]
fn non_enumerable_prototype_does_not_enter_a_snapshot_after_replacement() {
    yes(r#"
        function F(){}F.first=1;F.second=2;var seen='',replacement={};
        for(var key in F){
            if(key==='first'){F.prototype=replacement;delete F.second;F.third=3;}
            seen+=key+',';
        }
        seen==='first,' && F.prototype===replacement && Object.keys(F).join(',')==='first,third';
    "#);
}

#[test]
fn symbol_keys_named_prototype_do_not_alias_the_string_property() {
    yes(r#"
        function F(){}var key=Symbol('prototype'),other=Symbol('prototype'),value={};
        F[key]=value;var p=F.prototype,symbols=Object.getOwnPropertySymbols(F);
        var distinct=F[key]===value && F[other]===undefined && symbols.length===1 &&
            symbols[0]===key && p!==value && p.constructor===F;
        delete F[key];
        distinct && F.prototype===p && !F.hasOwnProperty(key) && F.hasOwnProperty('prototype');
    "#);
}

#[test]
fn computed_property_key_hooks_run_normally_for_reads_and_writes() {
    yes(r#"
        function F(){}var trace='',key={toString:function(){trace+='k';return 'prototype';}};
        var first=F[key],replacement={};F[key]=replacement;
        trace==='kk' && first.constructor===F && F.prototype===replacement && first!==replacement;
    "#);
}

#[test]
fn new_uses_the_original_default_before_executing_the_constructor_body() {
    yes(r#"
        var observed;
        function F(){observed=Object.getPrototypeOf(this);this.mark=7;}
        var instance=new F();
        observed===F.prototype && Object.getPrototypeOf(instance)===observed &&
            instance.constructor===F && instance instanceof F && instance.mark===7;
    "#);
}

#[test]
fn new_argument_effects_precede_the_prototype_read_and_body_replacement_is_later() {
    yes(r#"
        var before={before:1},after={after:2},trace='';
        function F(value){trace+='b';this.value=value;F.prototype=after;}
        function argument(){trace+='a';F.prototype=before;return 9;}
        var instance=new F(argument());
        trace==='ab' && Object.getPrototypeOf(instance)===before && instance.before===1 &&
            instance.value===9 && F.prototype===after && !(instance instanceof F);
    "#);
}

#[test]
fn primitive_constructor_prototypes_fall_back_to_intrinsic_object_prototype() {
    for replacement in ["undefined", "null", "true", "4", "'p'", "Symbol('p')"] {
        yes(&format!(
            "var intrinsic=Object.prototype,getPrototype=Object.getPrototypeOf;function F(){{this.mark=3;}}F.prototype={replacement};Object=function Fake(){{}};var value=new F();value.mark===3 && value.constructor!==F && getPrototype(value)===intrinsic;"
        ));
    }
}

#[test]
fn function_and_native_prototype_values_retain_their_real_identities() {
    for prototype in ["Parent", "Array", "parseInt"] {
        yes(&format!(
            "function Parent(){{throw 'prototype must not run';}}function F(){{this.mark=7;}}F.prototype={prototype};var child=new F();Object.getPrototypeOf(child)==={prototype} && child instanceof F && child.mark===7 && typeof child==='object';"
        ));
    }
}

#[test]
fn constructor_object_return_overrides_the_allocated_receiver() {
    yes(r#"
        var result={};function F(){return result;}var p=F.prototype;
        function ReturnFunction(){return F;}
        new F()===result && !(result instanceof F) && new ReturnFunction()===F &&
            F.prototype===p && p.constructor===F;
    "#);
}

#[test]
fn primitive_instanceof_lhs_short_circuits_but_noncallable_rhs_still_rejects() {
    yes(r#"
        function F(){}F.prototype=17;
        !(0 instanceof F) && !(null instanceof F) && !(undefined instanceof F) &&
            !(false instanceof F) && !('x' instanceof F) && !(Symbol('x') instanceof F);
    "#);
    for expression in [
        "({}) instanceof F",
        "3 instanceof ({})",
        "({}) instanceof null",
    ] {
        yes(&format!(
            "function F(){{}}F.prototype=17;var caught=false;try{{{expression};}}catch(e){{caught=String(e).indexOf('TypeError')>=0;}}caught;"
        ));
    }
}

#[test]
fn instanceof_uses_current_prototype_identity_not_constructor_backlink() {
    yes(r#"
        function F(){}function Other(){}var p=F.prototype,child=Object.create(p);
        p.constructor=Other;var matches=child instanceof F && !(child instanceof Other);
        F.prototype={};var noLonger=!(child instanceof F);F.prototype=p;
        matches && noLonger && child instanceof F && child.constructor===Other;
    "#);
}

#[test]
fn instanceof_retains_the_documented_sixty_four_ancestor_hit_boundary() {
    yes(r#"
        function F(){}var tail=F.prototype;
        for(var count=0;count<64;count++)tail=Object.create(tail);
        tail instanceof F;
    "#);
    let mut runtime = Runtime::new();
    let error = runtime
        .execute(
            r#"
                var caught=false,finished=false,after=false;
                function F(){}var tail=F.prototype;
                for(var count=0;count<65;count++)tail=Object.create(tail);
                try{tail instanceof F;}catch(e){caught=true;}finally{finished=true;}
                after=true;
            "#,
            &mut NoIo,
        )
        .unwrap_err();
    assert!(error.contains("prototype depth limit"), "{error}");
    for name in ["caught", "finished", "after"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
    assert_eq!(
        runtime.execute("after=true;", &mut NoIo).unwrap_err(),
        error
    );
}

#[test]
fn retained_defaults_and_backlinks_survive_arena_growth_and_lost_globals() {
    yes(r#"
        function make(value){return function(){return value;};}
        var first=make(7),last=first.prototype;first=null;
        for(var count=0;count<150;count++){
            var f=make(count);var unrelated=f.prototype;unrelated.mark=count;
        }
        last.constructor()===7 && last.constructor.prototype===last &&
            last!==unrelated && unrelated.constructor()===149 && last.mark===undefined;
    "#);
}

#[test]
fn default_backlinks_keep_closed_over_state_and_original_function_identity() {
    yes(r#"
        function make(){var value=1;return function(){value++;return value;};}
        var fn=make(),p=fn.prototype,old=fn;fn=function(){return 99;};
        p.constructor===old && p.constructor()===2 && old()===3 &&
            p.constructor()===4 && fn()===99 && old.prototype===p;
    "#);
}

#[test]
fn direct_and_indirect_eval_functions_have_distinct_defaults_and_correct_scopes() {
    yes(r#"
        var place='global';
        function make(){
            var place='local';return [eval('(function(){return place;})'),
                (0,eval)('(function(){return place;})')];
        }
        var pair=make(),a=pair[0],b=pair[1];
        a.prototype!==b.prototype && a.prototype.constructor===a && b.prototype.constructor===b &&
            a.prototype.constructor()==='local' && b.prototype.constructor()==='global';
    "#);
}

#[test]
fn function_constructor_and_call_apply_preserve_default_and_instance_identity() {
    yes(r#"
        var F=Function('value','this.value=value;return value;'),G=new Function('return 11;');
        var p=F.prototype,receiver={};
        var called=F.call(receiver,3),applied=F.apply(receiver,[4]),instance=new F(7);
        p.constructor===F && G.prototype.constructor===G && p!==G.prototype &&
            called===3 && applied===4 && receiver.value===4 && instance.value===7 &&
            Object.getPrototypeOf(instance)===p && instance instanceof F && G()===11 && F.prototype===p;
    "#);
}

#[test]
fn native_constructor_prototypes_and_readonly_attributes_remain_unchanged() {
    // Backlinks were absent at the original storage baseline; core intrinsics
    // now provides them. Keep native identity and attribute preservation checks.
    yes(r#"
        var array=Array.prototype,error=Error.prototype,symbol=Symbol.prototype;
        var arrayConstructor=array.constructor;
        function F(){}F.prototype={};
        Array.prototype={};Error.prototype={};Symbol.prototype={};
        var removedArray=delete Array.prototype,removedError=delete Error.prototype;
        Array.prototype===array && Error.prototype===error && Symbol.prototype===symbol &&
            !removedArray && !removedError && Array.isArray(array) &&
            array.constructor===arrayConstructor && error.constructor===Error && symbol.constructor===Symbol &&
            !parseInt.hasOwnProperty('prototype');
    "#);
}

#[test]
fn public_function_values_and_later_scripts_keep_plain_identity_handles() {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute("function F(){return 7;}F;", &mut NoIo)
        .unwrap();
    assert!(matches!(&function, Value::Function(_)));
    assert_eq!(
        runtime
            .invoke(function.clone(), Value::Null, vec![], &mut NoIo)
            .unwrap(),
        Value::Number(7.0)
    );
    runtime.set_global("alias", function.clone());
    assert_eq!(runtime.get_global("alias"), function);
    let prototype = runtime.execute("alias.prototype;", &mut NoIo).unwrap();
    assert!(matches!(&prototype, Value::Object(_)));
    runtime.set_global("saved", prototype);
    assert_eq!(
        runtime
            .execute(
                "saved===F.prototype && saved===alias.prototype && saved.constructor===alias && new alias() instanceof F;",
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
}
