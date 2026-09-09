//! Independent representation-only acceptance for docs/STATIC_OPERATORS.md.
//! All expected language results must pass both the frozen old and new parser.
//! No constructed AST operator fields, page source or alternative interpreter.

use mg_butane::{
    runtime::{Host, Runtime, Value},
    syntax,
};

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
    let result = runtime
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("operator case failed: {error}\n{source}"));
    clean(&runtime);
    result
}
fn yes(source: &str) {
    assert_eq!(value(source), Value::Bool(true), "{source}");
}
fn text(source: &str, expected: &str) {
    assert_eq!(value(source), Value::text(expected), "{source}");
}

#[test]
fn every_unary_spelling_preserves_its_value_and_effects() {
    for (source, expected) in [("+'12';", 12.0), ("-'2';", -2.0), ("~5;", -6.0)] {
        assert_eq!(value(source), Value::Number(expected), "{source}");
    }
    yes("!0===true&&!({})===false&&1/(-0)===-Infinity;");
    text("typeof notDeclared;", "undefined");
    text("typeof function(){};", "function");
    yes("var count=0,result=void(count+=1);result===undefined&&count===1;");
    yes("var object={x:1};delete object.x&&!('x' in object);");
}

#[test]
fn prefix_and_postfix_update_return_numbers_and_touch_computed_targets_once() {
    for (operator, prefix, expected, stored) in [
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
            "var trace='',object={{x:'3'}};function base(){{trace+='b';return object;}}function key(){{trace+='k';return 'x';}}var result={expression};trace==='bk'&&result==={expected}&&object.x==={stored}&&typeof result==='number';"
        ));
    }
}

#[test]
fn arithmetic_shift_and_bitwise_binary_spellings_keep_explicit_values() {
    for (source, expected) in [
        ("13+5;", 18.0),
        ("13-5;", 8.0),
        ("13*5;", 65.0),
        ("13/5;", 2.6),
        ("13%5;", 3.0),
        ("16<<2;", 64.0),
        ("-16>>2;", -4.0),
        ("-16>>>2;", 1_073_741_820.0),
        ("13&6;", 4.0),
        ("13|6;", 15.0),
        ("13^6;", 11.0),
    ] {
        assert_eq!(value(source), Value::Number(expected), "{source}");
    }
}

#[test]
fn relational_equality_membership_and_instance_binary_spellings_are_distinct() {
    for expression in [
        "3<4",
        "4>3",
        "3<=3",
        "4>=4",
        "'3'==3",
        "!('3'!=3)",
        "'3'!==3",
        "!('3'===3)",
        "3===3",
        "null==undefined",
        "null!==undefined",
    ] {
        yes(&format!("({expression});"));
    }
    yes(
        "var parent={inherited:undefined},child=Object.create(parent);child.own=1;('own' in child)&&('inherited' in child)&&!('missing' in child);",
    );
    yes(
        "function Base(){}function Other(){}var object=new Base();object instanceof Base&&!(object instanceof Other)&&!(3 instanceof Base);",
    );
}

#[test]
fn all_assignment_spellings_return_and_store_the_explicit_result() {
    for (operator, left, right, expected) in [
        ("=", 13, 5, 5.0),
        ("+=", 13, 5, 18.0),
        ("-=", 13, 5, 8.0),
        ("*=", 13, 5, 65.0),
        ("/=", 13, 5, 2.6),
        ("%=", 13, 5, 3.0),
        ("<<=", 16, 2, 64.0),
        (">>=", -16, 2, -4.0),
        (">>>=", -16, 2, 1_073_741_820.0),
        ("&=", 13, 6, 4.0),
        ("|=", 13, 6, 15.0),
        ("^=", 13, 6, 11.0),
    ] {
        let source =
            format!("var n={left},result=(n {operator} {right});result===n&&n==={expected};");
        yes(&source);
    }
}

#[test]
fn binary_precedence_and_left_associativity_remain_unchanged() {
    for (source, expected) in [
        ("1+2*3;", 7.0),
        ("24/3/2;", 4.0),
        ("20-8-3;", 9.0),
        ("1+2<<2;", 12.0),
        ("32>>1+1;", 8.0),
        ("6&3|8^1;", 11.0),
        ("0||2&&4;", 4.0),
        ("1,2,3;", 3.0),
    ] {
        assert_eq!(value(source), Value::Number(expected), "{source}");
    }
    yes("1+2*3===7&&1<<2<5&&3<4===true;");
}

#[test]
fn assignment_and_conditional_are_right_associative_with_ordered_references() {
    text(
        "var trace='',a={},b={};function target(name,object){trace+=name;return object;}function rhs(){trace+='R';return 7;}target('A',a).value=target('B',b).value=rhs();trace+':'+a.value+':'+b.value;",
        "ABR:7:7",
    );
    yes("var a=2,b=3;a+=b*=4;a===14&&b===12;");
    text(
        "var trace='';function mark(name,result){trace+=name;return result;}var result=mark('a',false)?mark('x',1):mark('b',true)?mark('c',42):mark('y',2);trace+':'+result;",
        "abc:42",
    );
}

#[test]
fn logical_and_conditional_short_circuits_preserve_identity_and_skip_work() {
    yes(r#"
        var calls=0,object={},other={},key=Symbol('key');
        function fail(){calls++;throw 'unexpected';}
        var a=object||fail(),b=0&&fail(),c=object&&other,d=0||key;
        var e=true?object:fail(),f=false?fail():other;
        a===object&&b===0&&c===other&&d===key&&e===object&&f===other&&calls===0;
    "#);
    yes("var n=0;false&&(n=1);true||(n=2);true?(n=3,n+=4):(n=99);n===7;");
}

#[test]
fn numeric_binary_operands_evaluate_before_ordered_single_conversions() {
    for operator in ["-", "*", "/", "%", "<<", ">>", ">>>", "&", "|", "^"] {
        text(
            &format!(
                "var trace='',left={{valueOf:function(){{trace+='l';return 8;}}}},right={{valueOf:function(){{trace+='r';return 2;}}}};function operand(name,value){{trace+=name;return value;}}operand('L',left) {operator} operand('R',right);trace;"
            ),
            "LRlr",
        );
    }
}

#[test]
fn addition_and_comparison_use_their_distinct_coercion_hints() {
    for (operator, hint) in [
        ("+", "default"),
        ("<", "number"),
        (">", "number"),
        ("<=", "number"),
        (">=", "number"),
    ] {
        text(
            &format!(
                "var trace='',left={{}},right={{}};left[Symbol.toPrimitive]=function(h){{trace+='l:'+h+';';return 2;}};right[Symbol.toPrimitive]=function(h){{trace+='r:'+h+';';return 3;}};function operand(name,value){{trace+=name;return value;}}operand('L',left) {operator} operand('R',right);trace;"
            ),
            &format!("LRl:{hint};r:{hint};"),
        );
    }
}

#[test]
fn compound_assignment_resolves_reference_then_rhs_then_old_value_conversion() {
    text(
        "var trace='',object={x:{valueOf:function(){trace+='v';return 10;}}};function base(){trace+='b';return object;}function key(){trace+='k';return 'x';}function rhs(){trace+='r';return 3;}var result=(base()[key()]-=rhs());trace+':'+result+':'+object.x;",
        "bkrv:7:7",
    );
    text(
        "var value='a';var result=(value+='b');result+':'+value;",
        "ab:ab",
    );
}

#[test]
fn conversion_failure_preserves_identity_and_suppresses_later_conversion() {
    for operator in ["+", "-", "*", "/", "%", "<<", ">>", ">>>", "&", "|", "^"] {
        yes(&format!(
            "var marker={{}},trace='',left={{valueOf:function(){{trace+='l';throw marker;}}}},right={{valueOf:function(){{trace+='r';return 2;}}}};function operand(name,value){{trace+=name;return value;}}var caught=false;try{{operand('L',left) {operator} operand('R',right);}}catch(error){{caught=error===marker;}}caught&&trace==='LRl';"
        ));
    }
    yes(
        "var marker={},left={valueOf:function(){throw marker;}},right=0,caught=false;try{left+=(right++,2);}catch(e){caught=e===marker;}caught&&right===1&&typeof left==='object';",
    );
}

#[test]
fn symbolic_numeric_errors_do_not_call_rhs_conversion() {
    for operator in ["-", "*", "/", "%", "<<", ">>", ">>>", "&", "|", "^"] {
        yes(&format!(
            "var trace='',symbol=Symbol('left'),right={{valueOf:function(){{trace+='c';return 2;}}}},caught=false;function rhs(){{trace+='r';return right;}}try{{symbol {operator} rhs();}}catch(e){{caught=String(e).indexOf('TypeError')>=0;}}caught&&trace==='r';"
        ));
    }
    yes(
        "var symbol=Symbol('x'),caught=false;try{+symbol;}catch(e){caught=String(e).indexOf('TypeError')>=0;}caught&&typeof symbol==='symbol';",
    );
}

#[test]
fn membership_rejects_primitive_rhs_before_key_coercion() {
    yes(r#"
        var trace='',key={toString:function(){trace+='k';return 'x';}},caught=false;
        function left(){trace+='l';return key;}function right(){trace+='r';return 3;}
        try{left() in right();}catch(e){caught=String(e).indexOf('TypeError')>=0;}
        caught&&trace==='lr';
    "#);
}

#[test]
fn exception_finally_and_later_execution_do_not_change_with_operator_storage() {
    let mut runtime = Runtime::new();
    assert_eq!(runtime.execute(
        "var marker={},trace='',caught=false;try{+{valueOf:function(){trace+='v';throw marker;}};}catch(e){caught=e===marker;trace+='c';}finally{trace+='f';}caught&&trace==='vcf';",
        &mut NoIo,
    ).unwrap(), Value::Bool(true));
    assert_eq!(
        runtime.execute("6*7;", &mut NoIo).unwrap(),
        Value::Number(42.0)
    );
    clean(&runtime);
    yes("function run(){try{throw 1;}finally{return 7*6;}}run()===42;");
}

#[test]
fn nullish_compound_target_keeps_exact_diagnostic_and_skips_rhs() {
    let mut runtime = Runtime::new();
    let error = runtime
        .execute(
            "var touched=false;null.length+=(touched=true,1);",
            &mut NoIo,
        )
        .unwrap_err();
    assert_eq!(
        error,
        "Uncaught JavaScript exception: TypeError: property access on null or undefined [member operation=resolve-compound-target base=null key=length] [producer kind=expression]"
    );
    assert_eq!(runtime.get_global("touched"), Value::Bool(false));
    assert_eq!(
        runtime.execute("21*2;", &mut NoIo).unwrap(),
        Value::Number(42.0)
    );
    clean(&runtime);
}

#[test]
fn utf16_operator_results_preserve_code_units_without_normalization() {
    yes(r#"
        var high='\uD800',low='\uDC00',lone='\uDFFF',joined=high+low;
        var text=high;text+=lone;
        joined.length===2&&joined.charCodeAt(0)===55296&&joined.charCodeAt(1)===56320&&
            text.length===2&&text.charCodeAt(1)===57343&&high<low&&low<lone&&
            high===high&&high!==low&&(''+lone).charCodeAt(0)===57343;
    "#);
}

#[test]
fn operator_like_user_keys_and_strings_remain_normal_mutable_data() {
    yes(r#"
        var object={'+':'plus','instanceof':'word','>>>=':'shift'};
        var key='+';object[key]+='!';object['in']='membership';
        object['+']==='plus!'&&object.instanceof==='word'&&object['>>>=']==='shift'&&
            ('in' in object)&&delete object['>>>=' ]&&!('>>>=' in object);
    "#);
}

#[test]
fn division_regex_comments_and_keyword_tokens_keep_lexical_context() {
    yes(
        "var n=12;n/=3;var value=n / /xx/.source.length;value===2&&/[/*]/.test('/')&&12/ /*comment*/ 3===4;",
    );
    yes("var object={instanceof:'data',in:1};('in' in object)&&object.instanceof==='data';");
    yes("if(true){/a/.test('a');}var result=(function(){return /b/.test('b');})();result;");
}

#[test]
fn noin_contexts_reenable_in_only_inside_the_existing_subgrammars() {
    for expression in [
        "('x' in object)",
        "Boolean('x' in object)",
        "['x' in object][0]",
        "({ready:'x' in object}).ready",
        "true ? 'x' in object : false",
    ] {
        yes(&format!(
            "var object={{x:1}},count=0;for(var ready={expression};ready;ready=false){{count++;}}count===1;"
        ));
    }
    yes(
        "var object={x:1},seen=false;for(var key=(seen=('x' in object)) in {only:1}){}seen&&key==='only';",
    );
}

#[test]
fn asi_does_not_merge_postfix_updates_or_break_continued_division() {
    assert_eq!(value("var n=1;n\n++n;"), Value::Number(2.0));
    assert_eq!(value("var n=12;n\n/2/3;"), Value::Number(2.0));
    assert_eq!(
        value("function run(){return\n1+2;}run();"),
        Value::Undefined
    );
    yes("var n=3;var old=n/* no newline */--;old===3&&n===2;");
}

fn reject_without_effects(suffix: &str) {
    let mut runtime = Runtime::new();
    runtime.execute("var marker=1;", &mut NoIo).unwrap();
    let source = format!("marker=9;var introduced=2;{suffix}");
    let error = runtime.execute(&source, &mut NoIo).unwrap_err();
    assert!(error.contains("SyntaxError"), "{suffix}: {error}");
    assert_eq!(runtime.get_global("marker"), Value::Number(1.0));
    assert_eq!(runtime.get_global("introduced"), Value::Undefined);
    assert_eq!(
        runtime.execute("marker+1;", &mut NoIo).unwrap(),
        Value::Number(2.0)
    );
    clean(&runtime);
}

#[test]
fn malformed_targets_and_unadopted_operators_still_reject_the_whole_script() {
    for source in [
        "(1)=2;",
        "(a+b)++;",
        "++(a?b:c);",
        "(a,b)=1;",
        "a+=;",
        "2**3;",
        "a**=2;",
        "a??b;",
        "a??=b;",
        "a&&=b;",
        "a||=b;",
    ] {
        reject_without_effects(source);
    }
}

#[test]
fn cloned_parsed_statements_keep_all_operator_spellings_after_source_drop() {
    // Clone the public statement vector instead of constructing any AST field;
    // this compiles with both owned String and canonical static operator fields.
    let source = String::from("+1;1*2;var n=3;n>>>=1;n--;('x' in {});({} instanceof Object);");
    let parsed = syntax::parse(&source).unwrap();
    let copied = parsed.0.clone();
    drop(parsed);
    drop(source);
    let description = format!("{copied:?}");
    for spelling in ["+", "*", ">>>=", "--", "in", "instanceof"] {
        assert!(
            description.contains(&format!("\"{spelling}\"")),
            "{description}"
        );
    }
    let second = copied.clone();
    drop(copied);
    assert_eq!(format!("{second:?}"), description);
}

#[test]
fn retained_closures_survive_source_drop_and_preserve_separate_captured_state() {
    let mut runtime = Runtime::new();
    let source = String::from(
        "function factory(seed){return function(step){seed+=step;return (seed*3-1)>>1;};}factory;",
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
            vec![Value::Number(10.0)],
            &mut NoIo,
        )
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(
        runtime
            .invoke(
                first.clone(),
                Value::Undefined,
                vec![Value::Number(1.0)],
                &mut NoIo
            )
            .unwrap(),
        Value::Number(4.0)
    );
    assert_eq!(
        runtime
            .invoke(
                second,
                Value::Undefined,
                vec![Value::Number(2.0)],
                &mut NoIo
            )
            .unwrap(),
        Value::Number(17.0)
    );
    assert_eq!(
        runtime
            .invoke(first, Value::Undefined, vec![Value::Number(2.0)], &mut NoIo)
            .unwrap(),
        Value::Number(7.0)
    );
    clean(&runtime);
}

#[test]
fn direct_indirect_eval_and_function_keep_operator_code_and_scope_lifetimes() {
    yes(r#"
        var globalValue=40;
        function direct(){var local=2;return eval('(function(step){local+=step;return local*3;})');}
        var first=direct(),second=direct();
        var indirect=(0,eval)('(function(step){return globalValue+step;})');
        var made=Function('seed','return function(step){seed^=step;return seed>>>1;};')(14);
        first!==second&&first(1)===9&&first(2)===15&&second(2)===12&&
            indirect(2)===42&&made(2)===6&&made(4)===4;
    "#);
}

#[test]
fn dynamic_invalid_operators_are_catchable_without_prefix_effects() {
    for source in ["marker=9;1**2;", "marker=9;(1)=2;", "marker=9;a&&=b;"] {
        let quoted = serde_json::to_string(source).unwrap();
        for operation in [
            format!("eval({quoted})"),
            format!("(0,eval)({quoted})"),
            format!("Function({quoted})"),
        ] {
            yes(&format!(
                "var marker=1,caught=false;try{{{operation};}}catch(e){{caught=e.name==='SyntaxError';}}caught&&marker===1;"
            ));
        }
    }
}
