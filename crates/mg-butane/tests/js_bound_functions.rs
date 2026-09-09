//! Independently authored bound-function semantics for the adopted ES5-shaped
//! contract: https://262.ecma-international.org/5.1/#sec-15.3.4.5
//! No generated wrapper source, private implementation access, external engine,
//! or website source is an oracle. Resource limits have a separate test suite.
//! Preserve existing native arity/constructor exclusions and the noncallable
//! Function.prototype; do not require modern bound names or full descriptors.

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
    let mut runtime = Runtime::new();
    let value = runtime
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("bound function case failed: {error}\n{source}"));
    assert_eq!(value, Value::Bool(true), "{source}");
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none());
}

fn type_error(setup: &str, expression: &str) {
    // Ordinary interpreter exceptions may still be strings. No requirement for
    // unrelated Error-object normalization or instanceof behavior is added.
    yes(&format!(
        "{setup}var caught=false,completed=false;try{{({expression});completed=true;}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&!completed;"
    ));
}

#[test]
fn bind_returns_distinct_callable_identities_with_the_intrinsic_parent() {
    yes(r#"
        function Target(a){return a;}
        var first=Target.bind(null),second=Target.bind(null);
        first!==second && first!==Target && typeof first==='function' &&
            Object.getPrototypeOf(first)===Function.prototype &&
            Object.prototype.toString.call(first)==='[object Function]' &&
            first(3)===3 && second(4)===4 && Target(5)===5;
    "#);
}

#[test]
fn bound_receiver_overrides_member_call_call_and_apply_receivers() {
    yes(r#"
        var original={x:4},other={x:99};
        function Sum(a,b){return this.x+a+b;}
        var bound=Sum.bind(original,2);other.method=bound;
        bound(3)===9 && other.method(3)===9 && bound.call(other,3)===9 &&
            bound.apply(other,[3])===9 && original.x===4 && other.x===99;
    "#);
}

#[test]
fn receiver_is_retained_without_coercion_and_boxed_only_by_the_target() {
    yes(r#"
        var coercions=0,receiver={toString:function(){coercions++;return 'bad';},
            valueOf:function(){coercions++;return 1;}};
        function This(){return this;}
        var objectBound=This.bind(receiver),numberBound=This.bind(7),
            stringBound=This.bind('\uD800x'),booleanBound=This.bind(false);
        var first=numberBound(),second=numberBound();
        objectBound()===receiver && coercions===0 && first!==second &&
            first.valueOf()===7 && second.valueOf()===7 &&
            Object.getPrototypeOf(first)===Number.prototype &&
            stringBound().valueOf()==='\uD800x' && booleanBound().valueOf()===false &&
            This.bind(null)()===globalThis && This.bind(undefined)()===globalThis;
    "#);
}

#[test]
fn binding_and_invocation_preserve_argument_evaluation_order_and_no_coercion() {
    yes(r#"
        var order='',coerced=0,payload={toString:function(){coerced++;return 'bad';}};
        function mark(label,value){order+=label;return value;}
        function Target(a,b,c){order+='T';return a===payload&&b===2&&c===3;}
        var bound=Target.bind(mark('R',null),mark('A',payload),mark('B',2));
        var before=order,result=bound(mark('C',3));
        before==='RAB' && order==='RABCT' && result && coerced===0;
    "#);
}

#[test]
fn prefix_arguments_distinguish_empty_undefined_and_ordered_values() {
    yes(r#"
        function describe(){return arguments.length+':'+arguments[0]+':'+arguments[1]+':'+arguments[2];}
        describe.bind(null)()==='0:undefined:undefined:undefined' &&
            describe.bind(null,undefined)()==='1:undefined:undefined:undefined' &&
            describe.bind(null,1)(2,3)==='3:1:2:3' &&
            describe.bind(null,1,2,3)()==='3:1:2:3';
    "#);
}

#[test]
fn rebinding_keeps_innermost_receiver_and_prefixes_before_outer_arguments() {
    yes(r#"
        var first={x:'first'},second={x:'second'},third={x:'third'};
        function Target(){return this.x+':'+Array.prototype.join.call(arguments,',');}
        var inner=Target.bind(first,'a'),middle=inner.bind(second,'b','c'),outer=middle.bind(third,'d');
        outer('e')==='first:a,b,c,d,e' && middle('f')==='first:a,b,c,f' &&
            inner('g')==='first:a,g' && outer.call(third,'h')==='first:a,b,c,d,h';
    "#);
}

#[test]
fn only_original_target_owns_callee_formals_and_arguments_snapshot() {
    yes(r#"
        function Target(a,b){var original=a;arguments[0]='changed';
            return arguments.callee===Target && a===original && b===2 && arguments.length===2;}
        function Empty(){return arguments.callee===Empty && arguments.length===0 &&
            Object.prototype.toString.call(arguments)==='[object Arguments]' && !Array.isArray(arguments);}
        Target.bind(null,1).bind({},2)() && Empty.bind({}).bind(null)();
    "#);
}

#[test]
fn noncallable_targets_reject_without_attempting_receiver_coercion() {
    for target in [
        "null",
        "undefined",
        "0",
        "'text'",
        "{}",
        "Function.prototype",
        "Object.create(function(){})",
    ] {
        type_error("", &format!("Function.prototype.bind.call({target},null)"));
    }
    yes(r#"
        var touched=0,target={length:{valueOf:function(){touched++;return 3;}},
            toString:function(){touched++;return 'target';}},caught=false;
        try{Function.prototype.bind.call(target,{toString:function(){touched++;return 'this';}});}
        catch(error){caught=String(error).indexOf('TypeError')>=0;}
        caught && touched===0;
    "#);
}

#[test]
fn length_captures_existing_numeric_arity_and_rebinding_reduces_to_zero() {
    yes(r#"
        function Target(a,b,c){}
        var zero=Target.bind(null),one=Target.bind(null,1),two=one.bind(null,2),
            many=two.bind(null,3,4,5),native=parseInt.bind(null,'17');
        zero.length===3 && one.length===2 && two.length===1 && many.length===0 &&
            native.length===parseInt.length-1 && Target.length===3;
    "#);
}

#[test]
fn initial_metadata_has_restricted_names_without_a_name_or_prototype_field() {
    yes(r#"
        function Target(a,b){}var bound=Target.bind(null,1),names=Object.getOwnPropertyNames(bound),seen='';
        for(var key in bound)seen+=key;
        names.length===3 && names.indexOf('length')>=0 && names.indexOf('caller')>=0 &&
            names.indexOf('arguments')>=0 && !bound.hasOwnProperty('name') &&
            !bound.hasOwnProperty('prototype') && bound.name===undefined && bound.prototype===undefined &&
            Object.keys(bound).length===0 && Object.getOwnPropertySymbols(bound).length===0 &&
            seen==='' && bound.length===1 && 'length' in bound && 'caller' in bound && 'arguments' in bound;
    "#);
}

#[test]
fn length_remains_readonly_nonenumerable_and_nondeletable_through_children() {
    yes(r#"
        function Target(a,b){}var bound=Target.bind(null),child=Object.create(bound);
        bound.length=9;child.length=8;
        bound.length===2 && child.length===2 && !child.hasOwnProperty('length') &&
            !(delete bound.length) && delete child.length &&
            Object.keys(bound).length===0 && Object.keys(child).length===0;
    "#);
}

#[test]
fn restricted_reads_throw_for_bound_and_inherited_receivers() {
    for receiver in ["bound", "child", "deep"] {
        for key in ["caller", "arguments"] {
            type_error(
                "function Target(){}var bound=Target.bind(null),child=Object.create(bound),deep=Object.create(child);",
                &format!("{receiver}['{key}']"),
            );
        }
    }
}

#[test]
fn restricted_writes_throw_after_rhs_but_compound_reads_precede_rhs() {
    for receiver in ["bound", "child", "deep"] {
        for key in ["caller", "arguments"] {
            yes(&format!(
                "function Target(){{}}var bound=Target.bind(null),child=Object.create(bound),deep=Object.create(child),steps=0,first=false,second=false;try{{{receiver}['{key}']=(steps++,7);}}catch(error){{first=String(error).indexOf('TypeError')>=0;}}try{{{receiver}['{key}']+=(steps++,8);}}catch(error){{second=String(error).indexOf('TypeError')>=0;}}first&&second&&steps===1&&!child.hasOwnProperty('{key}')&&!deep.hasOwnProperty('{key}');"
            ));
        }
    }
}

#[test]
fn restricted_metadata_is_safe_to_reflect_and_cannot_be_deleted() {
    yes(r#"
        var bound=(function(){}).bind(null),child=Object.create(bound);
        var own=bound.hasOwnProperty('caller')&&bound.hasOwnProperty('arguments');
        var inherited=('caller' in child)&&('arguments' in child)&&!child.hasOwnProperty('caller');
        own && inherited && !(delete bound.caller) && !(delete bound.arguments) &&
            delete child.caller && delete child.arguments && Object.keys(bound).length===0 &&
            Object.getOwnPropertyNames(child).length===0 && typeof Function.prototype==='object';
    "#);
}

#[test]
fn assigned_name_and_prototype_are_ordinary_writable_enumerable_deletable_fields() {
    yes(r#"
        var bound=(function(){return 3;}).bind(null),first={},second={};
        bound.name='first';bound.prototype=first;
        var initial=bound.name==='first'&&bound.prototype===first&&Object.keys(bound).join(',')==='name,prototype';
        bound.name='second';bound.prototype=second;
        var changed=bound.name==='second'&&bound.prototype===second&&bound()===3;
        var nameRemoved=delete bound.name,prototypeRemoved=delete bound.prototype;
        initial && changed && nameRemoved && prototypeRemoved && !bound.hasOwnProperty('name') &&
            !bound.hasOwnProperty('prototype') && bound.name===undefined && bound.prototype===undefined;
    "#);
}

#[test]
fn inherited_name_and_prototype_can_be_shadowed_without_changing_the_owner() {
    yes(r#"
        var bound=(function(){}).bind(null),child=Object.create(bound),parentValue={},childValue={};
        bound.name='parent';bound.prototype=parentValue;child.name='child';child.prototype=childValue;
        var shadow=child.name==='child'&&child.prototype===childValue&&bound.name==='parent'&&bound.prototype===parentValue;
        delete child.name;delete child.prototype;
        shadow && child.name==='parent' && child.prototype===parentValue &&
            !child.hasOwnProperty('name') && !child.hasOwnProperty('prototype');
    "#);
}

#[test]
fn nonenumerable_metadata_shadows_ancestors_but_absent_name_and_prototype_do_not() {
    yes(r#"
        Function.prototype.length=91;Function.prototype.caller=92;Function.prototype.arguments=93;
        Function.prototype.name='far';Function.prototype.prototype='far-prototype';
        var bound=(function(a){}).bind(null),child=Object.create(bound),seen='';child.local=1;
        for(var key in child)seen+=key+',';
        seen==='local,name,prototype,' && bound.length===1 && bound.name==='far' &&
            bound.prototype==='far-prototype' && !bound.hasOwnProperty('name') && !bound.hasOwnProperty('prototype');
    "#);
}

#[test]
fn ordinary_fields_follow_existing_enumeration_delete_and_readd_policy() {
    yes(r#"
        var bound=(function(){}).bind(null);bound.first=1;bound.second=2;
        delete bound.first;bound.first=3;var seen='';
        for(var key in bound){seen+=key+',';if(key==='second'){delete bound.first;bound.later=4;}}
        seen==='second,' && Object.keys(bound).join(',')==='second,later' && bound.later===4;
    "#);
}

#[test]
fn construction_ignores_bound_receiver_and_bound_own_prototype() {
    yes(r#"
        var ignored={sentinel:1};function Target(a,b){this.sum=a+b;}
        var targetPrototype=Target.prototype,bound=Target.bind(ignored,3);bound.prototype={wrong:true};
        var made=new bound(4);
        made.sum===7 && ignored.sum===undefined && Object.getPrototypeOf(made)===targetPrototype &&
            made.constructor===Target && made instanceof Target && made instanceof bound && made.wrong===undefined;
    "#);
}

#[test]
fn nested_bound_construction_preserves_prefix_order_and_original_callee() {
    yes(r#"
        function Target(a,b,c){this.text=a+':'+b+':'+c;this.original=arguments.callee===Target;}
        var first=Target.bind({ignored:1},'inner'),second=first.bind({ignored:2},'outer');
        var made=new second('last');
        made.text==='inner:outer:last' && made.original && made instanceof first &&
            made instanceof second && Object.getPrototypeOf(made)===Target.prototype;
    "#);
}

#[test]
fn construction_observes_target_prototype_after_argument_evaluation() {
    yes(r#"
        function Target(value){this.value=value;}var old=Target.prototype,bound=Target.bind(null),next={mark:9};
        var made=new bound((Target.prototype=next,7));
        made.value===7 && made.mark===9 && Object.getPrototypeOf(made)===next &&
            old!==next && old.constructor===Target && made instanceof bound;
    "#);
}

#[test]
fn bound_instance_checks_delegate_to_current_target_and_not_assigned_bound_prototype() {
    yes(r#"
        function Target(){}var bound=Target.bind(null),outer=bound.bind(null),old=new bound(),next={};
        Target.prototype=next;bound.prototype=old;outer.prototype=old;var fresh=new outer();
        !(old instanceof Target) && !(old instanceof bound) && !(old instanceof outer) &&
            fresh instanceof Target && fresh instanceof bound && fresh instanceof outer &&
            Object.getPrototypeOf(fresh)===next;
    "#);
}

#[test]
fn constructor_object_returns_override_receiver_but_primitive_returns_do_not() {
    yes(r#"
        var object={},functionValue=function(){},symbol=Symbol('return');
        function ObjectResult(){return object;}function FunctionResult(){return functionValue;}
        function PrimitiveResult(){this.saved=1;return symbol;}
        var A=ObjectResult.bind(null),B=FunctionResult.bind(null),C=PrimitiveResult.bind(null),made=new C();
        new A()===object && new B()===functionValue && made.saved===1 && made instanceof C &&
            Object.getPrototypeOf(made)===PrimitiveResult.prototype;
    "#);
}

#[test]
fn typed_target_prototypes_preserve_function_and_native_identity() {
    for prototype in ["Other", "Array"] {
        yes(&format!(
            "function Target(){{this.value=7;}}function Other(){{}}Target.prototype={prototype};var bound=Target.bind(null),made=new bound();Object.getPrototypeOf(made)==={prototype}&&made.value===7&&made instanceof Target&&made instanceof bound;"
        ));
    }
}

#[test]
fn primitive_target_prototype_keeps_constructor_fallback_and_instance_errors() {
    for prototype in ["null", "undefined", "0", "false", "'text'", "Symbol('p')"] {
        yes(&format!(
            "function Target(){{this.value=4;}}var intrinsic=Object.prototype;Target.prototype={prototype};var bound=Target.bind(null),made=new bound(),caught=false;try{{made instanceof bound;}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}Object.getPrototypeOf(made)===intrinsic&&made.value===4&&caught&&!(3 instanceof bound)&&!(null instanceof bound)&&!(Symbol('left') instanceof bound);"
        ));
    }
}

#[test]
fn supported_native_constructors_delegate_without_becoming_user_wrappers() {
    yes(r#"
        var A=Array.bind({wrong:1},1,2),O=Object.bind(null),S=String.bind(null,'\uD800x'),
            R=RegExp.bind(null,'a','g'),E=TypeError.bind(null,'message'),
            N=Number.bind(null,'7'),B=Boolean.bind(null,false);
        var a=new A(3),o=new O(),s=new S(),r=new R(),e=new E(),n=new N(),b=new B();
        Array.isArray(a) && a.join(',')==='1,2,3' && Object.getPrototypeOf(a)===Array.prototype &&
            Object.getPrototypeOf(o)===Object.prototype && s.valueOf()==='\uD800x' &&
            Object.getPrototypeOf(s)===String.prototype && r.source==='a' && r.global &&
            Object.getPrototypeOf(e)===TypeError.prototype && e.message==='message' &&
            n.valueOf()===7 && b.valueOf()===false && n instanceof N && b instanceof B &&
            Object.getPrototypeOf(n)===Number.prototype && Object.getPrototypeOf(b)===Boolean.prototype;
    "#);
}

#[test]
fn binding_does_not_grant_excluded_native_constructor_capabilities() {
    for target in ["parseInt", "Math.abs", "Array.prototype.slice"] {
        yes(&format!(
            "var bound={target}.bind(null),caught=false;try{{new bound();}}catch(error){{caught=String(error).length>0;}}caught;"
        ));
    }
    yes(r#"
        var touched=0,value={toString:function(){touched++;return 'bad';}},bound=Symbol.bind(null,value),caught=false;
        try{new bound();}catch(error){caught=String(error).indexOf('TypeError')>=0;}
        caught && touched===0 && typeof Symbol.bind(null,'ok')()==='symbol';
    "#);
}

#[test]
fn bound_eval_is_indirect_even_when_called_through_an_eval_identifier() {
    yes(r#"
        var marker='global',global=globalThis,original=eval,bound=original.bind({wrong:1});
        function first(){var marker='local';return bound('marker')+':'+bound('this===global');}
        function second(eval){var marker='local';return eval('marker');}
        var prefixed=original.bind(null,'marker');
        first()==='global:true' && second(bound)==='global' && prefixed()==='global' &&
            bound(7)===7 && bound({value:1}).value===1;
    "#);
}

#[test]
fn bound_call_and_apply_helpers_use_real_target_dispatch() {
    yes(r#"
        function Target(a,b){return this.x+a+b;}
        var invoke=Function.prototype.call.bind(Target),apply=Function.prototype.apply.bind(Target),
            bound=Target.bind({x:3},4);
        invoke({x:1},2,3)===6 && apply({x:2},[3,4])===9 &&
            Function.prototype.call.call(bound,{x:99},5)===12 &&
            Function.prototype.apply.call(bound,{x:99},[6])===13;
    "#);
}

#[test]
fn bound_dynamic_functions_keep_global_scope_and_factory_closures_keep_capture() {
    yes(r#"
        var globalNumber=7;
        function build(){var globalNumber=100;return new (Function.bind(null,'value','return globalNumber+value;'))();}
        function factory(start){var state=start;return function(step){state+=step;return state;}.bind(null,2);}
        var dynamic=build().bind(null,3),first=factory(1),second=factory(10);
        dynamic()===10 && first()===3 && first()===5 && second()===12 && first!==second;
    "#);
}

#[test]
fn utf16_symbols_and_object_payloads_keep_identity_without_coercion() {
    yes(r#"
        var symbol=Symbol('same'),other=Symbol('same'),object={value:1},text='\uD800😀';
        function Target(a,b,c){return a===symbol && a!==other && b===object && c===text &&
            c.length===3 && this.valueOf()===symbol && object.value===9;}
        var bound=Target.bind(symbol,symbol,object,text);object.value=9;
        bound() && symbol!==other && bound!==Target;
    "#);
}

#[test]
fn symbol_keys_do_not_alias_restricted_strings_and_coerced_keys_still_restrict() {
    yes(r#"
        var bound=(function(){}).bind(null),key=Symbol('caller'),other=Symbol('caller'),order='',caught=false;
        bound[key]=7;
        var converted={toString:function(){order+='key';return 'caller';}};
        try{bound[converted];}catch(error){caught=String(error).indexOf('TypeError')>=0;}
        caught && order==='key' && bound[key]===7 && bound[other]===undefined &&
            Object.getOwnPropertySymbols(bound)[0]===key && bound.hasOwnProperty(key) &&
            Object.keys(bound).length===0 && delete bound[key] && Object.getOwnPropertySymbols(bound).length===0;
    "#);
}

#[test]
fn retained_public_bound_handle_survives_global_replacement_and_later_scripts() {
    let mut runtime = Runtime::new();
    let bound = runtime
        .execute(
            "var receiver={value:2};function Target(a){return this.value+a;}var bound=Target.bind(receiver,3);bound;",
            &mut NoIo,
        )
        .unwrap();
    assert!(matches!(bound, Value::Function(_)));
    runtime
        .execute("receiver.value=8;Target=0;bound=null;", &mut NoIo)
        .unwrap();
    assert_eq!(
        runtime
            .invoke(bound.clone(), Value::Null, vec![], &mut NoIo)
            .unwrap(),
        Value::Number(11.0)
    );
    runtime.set_global("saved", bound.clone());
    assert_eq!(runtime.get_global("saved"), bound);
    assert_eq!(
        runtime.execute("saved();", &mut NoIo).unwrap(),
        Value::Number(11.0)
    );
}

#[derive(Default)]
struct RecordingHost {
    calls: Vec<(String, Value, Vec<Value>)>,
}

impl Host for RecordingHost {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        panic!("unexpected host get: {object}.{key}");
    }

    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("unexpected host set: {object}.{key}");
    }

    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String> {
        assert_eq!(name, "host.fixture.echo");
        self.calls.push((name.into(), this, args));
        Ok(Value::Number(17.0))
    }
}

#[test]
fn existing_host_native_callable_keeps_bound_receiver_arguments_and_authority() {
    let mut runtime = Runtime::new();
    let mut host = RecordingHost::default();
    runtime.set_global("method", Value::Native("host.fixture.echo".into()));
    runtime.set_global("receiver", Value::Host("fixture".into()));
    assert_eq!(
        runtime
            .execute(
                r"var bound=method.bind(receiver,'\uD800');typeof bound==='function';",
                &mut host
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert!(host.calls.is_empty(), "binding must not invoke the Host");
    assert_eq!(
        runtime.execute("bound.call(null,9);", &mut host).unwrap(),
        Value::Number(17.0)
    );
    assert_eq!(host.calls.len(), 1);
    assert_eq!(host.calls[0].1, Value::Host("fixture".into()));
    assert_eq!(
        host.calls[0].2,
        vec![Value::String(vec![0xd800]), Value::Number(9.0)]
    );
    assert_eq!(
        runtime
            .execute(
                "var rejected=false;try{new bound();}catch(error){rejected=true;}rejected;",
                &mut host
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        host.calls.len(),
        1,
        "new must not grant Host constructor authority"
    );
}

#[test]
fn opaque_host_values_cannot_bind_and_unknown_natives_never_dispatch_to_host() {
    let mut runtime = Runtime::new();
    let mut host = RecordingHost::default();
    runtime.set_global("opaque", Value::Host("fixture".into()));
    runtime.set_global("unknown", Value::Native("fixture.not_implemented".into()));
    assert_eq!(
        runtime.execute(r#"
            var hostRejected=false,unknownRejected=false;
            try{Function.prototype.bind.call(opaque,null);}catch(error){hostRejected=String(error).indexOf('TypeError')>=0;}
            var bound=unknown.bind(null);
            try{bound();}catch(error){unknownRejected=String(error).length>0;}
            hostRejected && unknownRejected;
        "#, &mut host).unwrap(),
        Value::Bool(true)
    );
    assert!(host.calls.is_empty());
}

#[test]
fn thrown_values_preserve_identity_and_catchable_failures_do_not_poison_later_calls() {
    yes(r#"
        var failure={},attempts=0;function Target(){attempts++;if(attempts===1)throw failure;return 7;}
        var bound=Target.bind(null),caught=false;try{bound();}catch(error){caught=error===failure;}
        caught && bound()===7 && attempts===2;
    "#);
}
