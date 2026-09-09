//! Independently authored core Symbol semantics. Primary references:
//! https://tc39.es/ecma262/multipage/fundamental-objects.html#sec-symbol-objects
//! https://tc39.es/ecma262/multipage/abstract-operations.html#sec-toprimitive
//! https://tc39.es/ecma262/multipage/text-processing.html#sec-string-constructor
//! https://tc39.es/ecma262/multipage/ecmascript-language-expressions.html#sec-relational-operators-runtime-semantics-evaluation
//! No existing engine, page source, iterator protocol or unsupported syntax.

use mg_butane::runtime::{Host, Runtime, Value};

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

fn yes(source: &str) {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime.execute(source, &mut NoIo).unwrap(),
        Value::Bool(true),
        "{source}"
    );
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none());
}

fn type_error(setup: &str, expression: &str) {
    // Existing Error prototype fidelity is partial; assert TypeError evidence
    // without assuming full native descriptor or instanceof behavior.
    yes(&format!(
        "var caught=false,completed=false;{setup}try{{({expression});completed=true;}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&!completed;"
    ));
}

#[test]
fn symbol_primitives_have_identity_type_and_truthiness_not_description_identity() {
    yes(
        "var first=Symbol('same'),second=Symbol('same');typeof first==='symbol'&&first===first&&first!==second&&first!=second&&Boolean(first)===true&&!(!first)&&first!==String(first)&&first!=null&&first!=undefined;",
    );
}

#[test]
fn boxing_keeps_symbol_identity_but_not_wrapper_identity_or_type() {
    yes(
        "var symbol=Symbol('value'),boxed=Object(symbol),other=Object(symbol);typeof boxed==='object'&&boxed!==symbol&&boxed==symbol&&symbol==boxed&&boxed!==other&&boxed!=other&&Object(boxed)===boxed&&boxed.valueOf()===symbol&&Boolean(boxed)===true;",
    );
    yes(
        "var first=Symbol('same'),second=Symbol('same');Object(first)!=second&&Object(first)!==Object(second)&&first!==second;",
    );
}

#[test]
fn descriptions_distinguish_absence_empty_and_utf16_without_defining_identity() {
    yes(
        "var absent=Symbol(),explicit=Symbol(undefined),empty=Symbol(''),text=Symbol('\\uD800😀');absent.description===undefined&&explicit.description===undefined&&empty.description===''&&Symbol(null).description==='null'&&Symbol(42).description==='42'&&text.description==='\\uD800😀'&&text.description.length===3&&String(text)==='Symbol(\\uD800😀)'&&Object(text).description===text.description&&absent!==explicit;",
    );
}

#[test]
fn symbol_is_callable_but_new_rejects_before_description_coercion() {
    yes(
        "var called=0,caught=false,description={toString:function(){called++;return 'x';}};try{new Symbol(description);}catch(e){caught=String(e).indexOf('TypeError')>=0;}called===0&&caught&&typeof Symbol.call(null,'called')==='symbol';",
    );
    type_error("var symbol=Symbol('x');", "Symbol(symbol)");
}

#[test]
fn description_conversion_uses_string_hint_and_propagates_exceptions() {
    yes(
        "var order='',description={toString:function(){order+='s';return 'named';},valueOf:function(){order+='v';return 7;}};var symbol=Symbol(description);symbol.description==='named'&&order==='s';",
    );
    yes(
        "var hintSeen='',receiver=false,description={};description[Symbol.toPrimitive]=function(hint){hintSeen=hint;receiver=this===description;return 'hooked';};var symbol=Symbol(description);symbol.description==='hooked'&&hintSeen==='string'&&receiver;",
    );
    yes(
        "var caught=0,completed=false;try{Symbol({toString:function(){throw 37;}});completed=true;}catch(e){caught=e;}caught===37&&!completed;",
    );
}

#[test]
fn registry_matches_string_keys_not_fresh_symbol_descriptions() {
    yes(
        "var first=Symbol.for('entry'),again=Symbol.for('entry'),fresh=Symbol('entry');first===again&&first!==fresh&&first.description==='entry'&&Symbol.keyFor(first)==='entry'&&Symbol.keyFor(fresh)===undefined&&Symbol.for(42)===Symbol.for('42')&&Symbol.for()===Symbol.for('undefined')&&Symbol.keyFor(Symbol.for())==='undefined';",
    );
    yes(
        "var lone=Symbol.for('\\uD800'),replacement=Symbol.for('�');lone!==replacement&&lone===Symbol.for('\\uD800')&&Symbol.keyFor(lone)==='\\uD800'&&Symbol.for('😀')===Symbol.for('😀')&&Symbol.for('')===Symbol.for('');",
    );
    yes(
        "var order='',key={toString:function(){order+='s';return 'converted';},valueOf:function(){order+='v';return 9;}};Symbol.for(key)===Symbol.for('converted')&&order==='s';",
    );
    type_error("var symbol=Symbol('x');", "Symbol.for(symbol)");
}

#[test]
fn key_for_requires_a_primitive_symbol_without_coercing_an_object() {
    for expression in [
        "Symbol.keyFor()",
        "Symbol.keyFor('entry')",
        "Symbol.keyFor(1)",
        "Symbol.keyFor(Object(symbol))",
    ] {
        type_error("var symbol=Symbol.for('entry');", expression);
    }
    yes(
        "var called=false,caught=false,value={valueOf:function(){called=true;return Symbol.for('entry');},toString:function(){called=true;return 'entry';}};try{Symbol.keyFor(value);}catch(e){caught=String(e).indexOf('TypeError')>=0;}caught&&!called;",
    );
}

#[test]
fn value_of_and_to_string_are_branded_not_description_or_prototype_shims() {
    yes(
        "var symbol=Symbol('named'),boxed=Object(symbol);Symbol.prototype.valueOf.call(symbol)===symbol&&Symbol.prototype.valueOf.call(boxed)===symbol&&Symbol.prototype.toString.call(symbol)==='Symbol(named)'&&Symbol.prototype.toString.call(boxed)==='Symbol(named)'&&Symbol.prototype.toString.call(Symbol())==='Symbol()'&&Symbol.prototype.toString.call(Symbol(''))==='Symbol()';",
    );
    for receiver in [
        "{}",
        "1",
        "'named'",
        "null",
        "undefined",
        "Symbol.prototype",
        "Object.create(Symbol.prototype)",
        "Object.create(Object(symbol))",
    ] {
        for method in ["valueOf", "toString"] {
            type_error(
                "var symbol=Symbol('named');",
                &format!("Symbol.prototype.{method}.call({receiver})"),
            );
        }
    }
}

#[test]
fn description_getter_keeps_receiver_branding_and_is_not_an_own_string_field() {
    yes(
        "var symbol=Symbol('named'),boxed=Object(symbol);!Object.prototype.hasOwnProperty.call(boxed,'description')&&boxed.description==='named';",
    );
    type_error("", "Symbol.prototype.description");
    type_error("", "Object.create(Symbol.prototype).description");
    type_error(
        "var symbol=Symbol('named');",
        "Object.create(Object(symbol)).description",
    );
}

#[test]
fn symbol_to_primitive_method_returns_the_brand_value_and_ignores_hint() {
    yes(
        "var symbol=Symbol('named'),boxed=Object(symbol),method=Symbol.prototype[Symbol.toPrimitive];method.call(symbol,'default')===symbol&&method.call(boxed,'number')===symbol&&method.call(boxed,'string')===symbol&&method.call(boxed,'not-a-hint')===symbol&&method.call(boxed)===symbol;",
    );
    for receiver in [
        "{}",
        "Symbol.prototype",
        "Object.create(Symbol.prototype)",
        "null",
    ] {
        type_error(
            "",
            &format!("Symbol.prototype[Symbol.toPrimitive].call({receiver},'default')"),
        );
    }
}

#[test]
fn explicit_string_accepts_only_primitive_symbols_not_boxed_or_implicit_coercions() {
    yes(
        "var symbol=Symbol('named'),convert=String;String(symbol)==='Symbol(named)'&&convert(symbol)==='Symbol(named)'&&String.call(null,symbol)==='Symbol(named)'&&String(Symbol())==='Symbol()'&&String(Symbol(''))==='Symbol()';",
    );
    for expression in [
        "new String(symbol)",
        "String(Object(symbol))",
        "''+symbol",
        "symbol+''",
        "''+Object(symbol)",
        "Array(symbol).join(',')",
        "'prefix'.concat(symbol)",
    ] {
        type_error("var symbol=Symbol('named');", expression);
    }
}

#[test]
fn symbols_reject_numeric_conversion_and_relational_ordering() {
    for expression in [
        "Number(symbol)",
        "Number(Object(symbol))",
        "+symbol",
        "-symbol",
        "~symbol",
        "symbol*2",
        "symbol-1",
        "symbol<1",
        "1<symbol",
        "symbol|1",
        "symbol++",
        "array.length=symbol",
        "Math.abs(symbol)",
        "'text'.indexOf('t',symbol)",
        "String.fromCharCode(symbol)",
    ] {
        type_error("var symbol=Symbol('named'),array=[];", expression);
    }
}

#[test]
fn to_primitive_receives_exact_default_number_and_string_hints() {
    yes(
        "var hints='',receivers=true,fallback=false,value={valueOf:function(){fallback=true;return 1;},toString:function(){fallback=true;return 'fallback';}};value[Symbol.toPrimitive]=function(hint){hints+=hint+'|';receivers=receivers&&this===value;return hint==='string'?'key':7;};var add=value+1,number=+value,text=String(value),target={};target[value]=9;add===8&&number===7&&text==='key'&&target.key===9&&hints==='default|number|string|string|'&&receivers&&!fallback;",
    );
    yes(
        "var proto={},child,receiver=false;proto[Symbol.toPrimitive]=function(hint){receiver=this===child;return this.number;};child=Object.create(proto);child.number=11;+child===11&&receiver;",
    );
}

#[test]
fn in_rejects_primitive_rhs_before_converting_the_property_key() {
    // Both operand expressions run first. The RHS object check then precedes
    // ToPropertyKey, so a throwing Symbol.toPrimitive hook must remain untouched.
    for right in [
        "0",
        "true",
        "'text'",
        "null",
        "undefined",
        "Symbol('right')",
    ] {
        yes(&format!(
            "var trail='',caught=false,key={{}};key[Symbol.toPrimitive]=function(hint){{trail+='key';throw 37;}};function left(){{trail+='left;';return key;}}function right(){{trail+='right;';return {right};}}try{{left() in right();}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&trail==='left;right;';"
        ));
    }
}

#[test]
fn numeric_binary_failure_stops_rhs_conversion_but_not_operand_evaluation() {
    // ApplyStringOrNumericBinaryOperator completes left ToNumeric before right
    // ToNumeric. Addition and relational comparisons first get both primitives.
    for operator in ["-", "*", "/", "%", "<<", ">>", ">>>", "&", "^", "|"] {
        for (left_value, conversion_trail) in [
            ("symbol", ""),
            (
                "{valueOf:function(){trail+='left-number;';return symbol;}}",
                "left-number;",
            ),
        ] {
            yes(&format!(
                "var trail='',caught=false,symbol=Symbol('left'),leftValue={left_value},rightValue={{}};rightValue[Symbol.toPrimitive]=function(hint){{trail+='right-number;';return 2;}};function left(){{trail+='left-expression;';return leftValue;}}function right(){{trail+='right-expression;';return rightValue;}}try{{left() {operator} right();}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&trail==='left-expression;right-expression;{conversion_trail}';"
            ));
        }
        yes(&format!(
            "var trail='',caught=false,symbol=Symbol('left'),target=symbol,value={{}};value[Symbol.toPrimitive]=function(hint){{trail+='conversion;';return 2;}};function right(){{trail+='expression;';return value;}}try{{target {operator}= right();}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&trail==='expression;'&&target===symbol;"
        ));
    }
    for (operator, hint) in [
        ("+", "default"),
        ("<", "number"),
        (">", "number"),
        ("<=", "number"),
        (">=", "number"),
    ] {
        yes(&format!(
            "var trail='',caught=false,symbol=Symbol('left'),value={{}};value[Symbol.toPrimitive]=function(hint){{trail+='conversion:'+hint+';';return 2;}};function left(){{trail+='left-expression;';return symbol;}}function right(){{trail+='right-expression;';return value;}}try{{left() {operator} right();}}catch(error){{caught=String(error).indexOf('TypeError')>=0;}}caught&&trail==='left-expression;right-expression;conversion:{hint};';"
        ));
    }
}

#[test]
fn to_primitive_validates_callable_and_primitive_result_without_fallback() {
    for hook in ["0", "false", "''", "{}"] {
        type_error(
            &format!("var value={{}};value[Symbol.toPrimitive]={hook};"),
            "String(value)",
        );
    }
    yes(
        "var fallback=false,caught=false,value={toString:function(){fallback=true;return 'fallback';}};value[Symbol.toPrimitive]=function(hint){return {};};try{String(value);}catch(e){caught=String(e).indexOf('TypeError')>=0;}caught&&!fallback;",
    );
    yes(
        "var caught=0,fallback=false,value={valueOf:function(){fallback=true;return 1;}};value[Symbol.toPrimitive]=function(hint){throw 37;};try{+value;}catch(e){caught=e;}caught===37&&!fallback;",
    );
}

#[test]
fn missing_or_null_primitive_hook_keeps_ordinary_method_order() {
    yes(
        "var order='',value={valueOf:function(){order+='v';return 7;},toString:function(){order+='s';return 'text';}};value[Symbol.toPrimitive]=null;var number=+value,text=String(value),sum=value+1;number===7&&text==='text'&&sum===8&&order==='vsv';",
    );
    yes(
        "var order='',value={toString:function(){order+='s';return {};},valueOf:function(){order+='v';return 'last';}};value[Symbol.toPrimitive]=undefined;String(value)==='last'&&order==='sv';",
    );
    type_error(
        "var value={valueOf:function(){return {};},toString:function(){return {};}};value[Symbol.toPrimitive]=null;",
        "+value",
    );
}

#[test]
fn primitive_hook_can_return_a_symbol_but_string_and_number_conversion_still_reject() {
    yes(
        "var symbol=Symbol('key'),hints='',value={};value[Symbol.toPrimitive]=function(hint){hints+=hint;return symbol;};value==symbol&&hints==='default';",
    );
    for expression in ["String(value)", "+value", "''+value"] {
        type_error(
            "var symbol=Symbol('key'),value={};value[Symbol.toPrimitive]=function(hint){return symbol;};",
            expression,
        );
    }
}

#[test]
fn binary_operands_are_evaluated_before_left_to_right_primitive_conversion() {
    yes(
        "var order='',left={},right={};left[Symbol.toPrimitive]=function(hint){order+='pl';return 1;};right[Symbol.toPrimitive]=function(hint){order+='pr';return 2;};function argument(name,value){order+='e'+name;return value;}var result=argument('l',left)+argument('r',right);result===3&&order==='elerplpr';",
    );
    yes(
        "var order='',left={},right={},caught=0;left[Symbol.toPrimitive]=function(hint){order+='pl';throw 37;};right[Symbol.toPrimitive]=function(hint){order+='pr';return 2;};function argument(name,value){order+='e'+name;return value;}try{argument('l',left)+argument('r',right);}catch(e){caught=e;}caught===37&&order==='elerpl';",
    );
}

#[test]
fn to_string_tag_uses_inherited_string_values_without_coercing_other_values() {
    yes(
        "var tag=Symbol.toStringTag,proto={},value=Object.create(proto),array=[],fn=function(){};proto[tag]='Inherited';array[tag]='Vector';fn[tag]='Callable';Object.prototype.toString.call(value)==='[object Inherited]'&&Object.prototype.toString.call(array)==='[object Vector]'&&Object.prototype.toString.call(fn)==='[object Callable]'&&Object.prototype.toString.call(Symbol('x'))==='[object Symbol]'&&Object.prototype.toString.call(Object(Symbol('x')))==='[object Symbol]';",
    );
    yes(
        "var called=false,value={};value[Symbol.toStringTag]={toString:function(){called=true;return 'Wrong';}};var first=Object.prototype.toString.call(value);value[Symbol.toStringTag]=7;var second=Object.prototype.toString.call(value);first==='[object Object]'&&second==='[object Object]'&&!called;",
    );
}

#[test]
fn supported_well_known_identities_cannot_be_overwritten_or_deleted() {
    yes(
        "var primitive=Symbol.toPrimitive,tag=Symbol.toStringTag;Symbol.toPrimitive=Symbol('replacement');Symbol.toStringTag=Symbol('replacement');var deletedPrimitive=delete Symbol.toPrimitive,deletedTag=delete Symbol.toStringTag;typeof primitive==='symbol'&&typeof tag==='symbol'&&primitive!==tag&&Symbol.keyFor(primitive)===undefined&&Symbol.keyFor(tag)===undefined&&Symbol.toPrimitive===primitive&&Symbol.toStringTag===tag&&!deletedPrimitive&&!deletedTag;",
    );
}

#[test]
fn inherited_symbol_primitive_hook_is_nonwritable_but_configurable() {
    yes(
        "var symbol=Symbol('named'),boxed=Object(symbol),hook=Symbol.toPrimitive,original=Symbol.prototype[hook];boxed[hook]=function(){return 7;};var ignored=boxed[hook]===original&&!Object.prototype.hasOwnProperty.call(boxed,hook);var removed=delete Symbol.prototype[hook];boxed[hook]=function(hint){return 7;};ignored&&removed&&Object.prototype.hasOwnProperty.call(boxed,hook)&&+boxed===7;",
    );
}

#[test]
fn symbol_prototype_tag_does_not_make_the_prototype_a_boxed_symbol() {
    yes(
        "var tag=Symbol.toStringTag;Object.prototype.toString.call(Symbol.prototype)==='[object Symbol]'&&Symbol.prototype[tag]==='Symbol';",
    );
    type_error("", "Symbol.prototype.valueOf()");
    // The tag is configurable, unlike the constructor's well-known identities.
    // Once removed, Object#toString uses Object, not a hard-coded Symbol brand.
    yes(
        "var symbol=Symbol('named'),boxed=Object(symbol),tag=Symbol.toStringTag;Symbol.prototype[tag]='Changed';boxed[tag]='Changed';var ignored=Symbol.prototype[tag]==='Symbol'&&boxed[tag]==='Symbol'&&!Object.prototype.hasOwnProperty.call(boxed,tag);var removed=delete Symbol.prototype[tag];ignored&&removed&&Object.prototype.toString.call(Symbol.prototype)==='[object Object]'&&Object.prototype.toString.call(symbol)==='[object Object]'&&Object.prototype.toString.call(boxed)==='[object Object]';",
    );
}

#[test]
fn recursive_primitive_hooks_fail_fatally_on_the_default_stack() {
    const CHILD: &str = "MGBROWSER_SYMBOL_COERCION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        for pending_unary in [1, 32] {
            let mut runtime = Runtime::new();
            let source = format!(
                "var caught=false,finalized=false,value={{}};value[Symbol.toPrimitive]=function(hint){{return {}value;}};try{{+value;}}catch(e){{caught=true;}}finally{{finalized=true;}}",
                "+ ".repeat(pending_unary)
            );
            let error = runtime.execute(&source, &mut NoIo).unwrap_err();
            assert!(
                error.contains("limit") || error.contains("exhausted"),
                "{error}"
            );
            assert!(
                !error.contains("parser"),
                "Must reach hook evaluation: {error}"
            );
            assert_eq!(runtime.get_global("caught"), Value::Bool(false));
            assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
            let first = runtime.allocation_report();
            assert!(first.is_valid());
            assert_eq!(
                runtime
                    .execute("var later_effect=1;", &mut NoIo)
                    .unwrap_err(),
                error
            );
            assert_eq!(runtime.get_global("later_effect"), Value::Undefined);
            assert_eq!(
                runtime
                    .invoke(
                        Value::Native("Number".into()),
                        Value::Undefined,
                        vec![],
                        &mut NoIo
                    )
                    .unwrap_err(),
                error
            );
            assert_eq!(runtime.allocation_report(), first);
        }
        return;
    }
    use std::{
        io::Read,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            // Reap our child even if a wait, output read or assertion fails.
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "recursive_primitive_hooks_fail_fatally_on_the_default_stack",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env_remove("RUST_MIN_STACK")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "Owned Symbol-hook test child exceeded deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    fn output(reader: impl Read) -> String {
        let mut bytes = Vec::new();
        reader.take(65_537).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 65_536, "Owned child output exceeded 64 KiB");
        String::from_utf8_lossy(&bytes).into_owned()
    }
    let stdout = output(child.0.stdout.take().unwrap());
    let stderr = output(child.0.stderr.take().unwrap());
    assert!(
        status.success(),
        "Default-stack child {status}\n{stdout}\n{stderr}"
    );
}
