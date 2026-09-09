//! Authored expression-parser regressions, without website/challenge code or
//! another JavaScript engine. Semantics: ECMA-262 5.1 sections 7.9 and 11,
//! https://262.ecma-international.org/5.1/ . Resource bounds are project policy.
//! No test increases Rust's thread-stack size.

use mg_butane::runtime::{Host, Runtime, Value};
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

fn grouped(expression: &str, depth: usize) -> String {
    format!("{}{expression}{}", "(".repeat(depth), ")".repeat(depth))
}
fn quoted(source: &str) -> String {
    serde_json::to_string(source).unwrap()
}
fn value(source: &str) -> Value {
    Runtime::new()
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("Local expression case failed: {error}\n{source}"))
}
fn yes(source: &str) {
    assert_eq!(value(source), Value::Bool(true), "{source}");
}
fn text(source: &str, expected: &str) {
    assert_eq!(value(source), Value::text(expected), "{source}");
}

#[test]
fn sixty_four_grouping_levels_evaluate_without_changing_ast_meaning() {
    assert_eq!(value(&grouped("6*7", 64)), Value::Number(42.0));
    yes(&format!("var input=41;{}===42;", grouped("input+1", 64)));
    text(&grouped("'😀'.length+':'+(9-3-2)", 64), "2:4");
}

#[test]
fn direct_eval_and_grouped_intrinsic_keep_lexical_bindings_and_this() {
    let source = quoted(&grouped("local+this.offset", 64));
    assert_eq!(
        value(&format!(
            "var local=100;function run(){{var local=40;return eval({source});}}run.call({{offset:2}});"
        )),
        Value::Number(42.0),
    );
    let intrinsic = grouped("eval", 64);
    assert_eq!(
        value(&format!(
            "var local=100;function run(){{var local=41;return {intrinsic}('local+1');}}run();"
        )),
        Value::Number(42.0),
    );
}

#[test]
fn indirect_eval_groups_execute_in_the_global_environment() {
    let source = quoted(&grouped("local+2", 64));
    for callee in ["alias", "(0,eval)", "holder.evaluate"] {
        assert_eq!(
            value(&format!(
                "var local=40,alias=eval,holder={{evaluate:eval}};function run(){{var local=90;return {callee}({source});}}run();"
            )),
            Value::Number(42.0),
            "{callee}",
        );
    }
}

#[test]
fn function_constructor_bodies_accept_groups_without_merging_parameter_grammar() {
    let body = quoted(&format!("return {};", grouped("left+right", 64)));
    for constructor in ["Function", "new Function"] {
        assert_eq!(
            value(&format!(
                "var run={constructor}('left','right',{body});run(40,2);"
            )),
            Value::Number(42.0),
        );
    }
    yes(
        "var caught=false;try{Function('(value)','return value;');}catch(e){caught=e.name==='SyntaxError';}caught;",
    );
}

#[test]
fn grouped_members_preserve_reference_this_and_evaluation_order() {
    text(
        &format!(
            "var n=10,object={{n:3,read:function(){{return this.n;}}}};{}()+':'+(0,object.read)();",
            grouped("object.read", 64)
        ),
        "3:10",
    );
    text(
        "var trace='';function base(){trace+='b';return {method:function(a,b){trace+='c';return a+b;}};}
         function key(){trace+='k';return 'method';}function arg(n){trace+=n;return n;}
         var result=((base())[key()])(arg(1),arg(2));trace+':'+result;",
        "bk12c:3",
    );
}

#[test]
fn grouped_arrays_objects_and_calls_keep_holes_and_left_to_right_effects() {
    let array = grouped("[step(1),,{value:step(2)},step(3)]", 64);
    text(
        &format!(
            "var trace='';function step(n){{trace+=n;return n;}}var data={array};trace+':'+data.length+':'+data[2].value+':'+(1 in data);"
        ),
        "123:4:2:false",
    );
    let object = grouped(
        "{first:[1,2],second:{read:function(x){return x+this.value;},value:40}}",
        64,
    );
    assert_eq!(
        value(&format!("({object}).second.read(2);")),
        Value::Number(42.0)
    );
}

#[test]
fn constructor_member_and_call_precedence_remain_distinct() {
    for source in [
        "function Box(n){this.value=n;}new (Box)(42).value;",
        "function Box(n){this.value=n;}var namespace={Box:Box};new namespace.Box(42).value;",
        "function Box(n){this.value=n;}function select(){return Box;}new (select())(42).value;",
        "function Factory(){return function(){return 42;};}new Factory()();",
        "function Box(){this.value=42;}function Factory(){return Box;}(new new Factory()).value;",
    ] {
        assert_eq!(value(source), Value::Number(42.0), "{source}");
    }
    assert_eq!(
        value(&format!(
            "function Box(n){{this.value=n;}}new {}(42).value;",
            grouped("Box", 64)
        )),
        Value::Number(42.0),
    );
}

#[test]
fn unary_and_update_operators_work_on_grouped_references() {
    text(
        &format!("var n=2,old={}++;old+':'+n;", grouped("n", 64)),
        "2:3",
    );
    yes(
        "var object={value:1};++((object).value)===2 && delete ((object).value) && !('value' in object);",
    );
    yes(&format!(
        "!{} && ~{}===-3 && typeof {}==='number';",
        grouped("false", 64),
        grouped("2", 64),
        grouped("2", 64)
    ));
    assert_eq!(value("void (1+2);"), Value::Undefined);
}

#[test]
fn assignment_is_right_associative_with_ordered_reference_evaluation() {
    text(
        "var trace='',a={},b={};function target(name,object){trace+=name;return object;}
         function rhs(){trace+='R';return 7;}target('A',a).value=target('B',b).value=rhs();
         trace+':'+a.value+':'+b.value;",
        "ABR:7:7",
    );
    text("var a=0,b=0,c=0;a=b=c=4;a+':'+b+':'+c;", "4:4:4");
    yes("var a=2,b=3;a+=b*=4;a===14 && b===12;");
    yes(&format!(
        "var a=0,b=0;{}=b=42;a===42 && b===42;",
        grouped("a", 64)
    ));
}

#[test]
fn conditional_sequence_and_binary_precedence_preserve_short_circuits() {
    text(
        "var trace='';function mark(letter,value){trace+=letter;return value;}
         var result=mark('a',false)?mark('x',1):mark('b',true)?mark('c',42):mark('y',2);
         trace+':'+result;",
        "abc:42",
    );
    yes("var value=0;true?(value=1,value+=2):(value=99);value===3;");
    for (source, expected) in [
        ("1+2*3;", 7.0),
        ("20-8-3;", 9.0),
        ("24/3/2;", 4.0),
        ("1+2<<2;", 12.0),
        ("6&3|8^1;", 11.0),
        ("0 || 2 && 4;", 4.0),
        ("1,2,3;", 3.0),
    ] {
        assert_eq!(value(source), Value::Number(expected), "{source}");
    }
    yes("var called=false;false && (called=true);true || (called=true);!called;");
}

#[test]
fn no_in_contexts_allow_grouped_and_nested_expression_grammars() {
    for initializer in [
        "('x' in object)",
        "Boolean('x' in object)",
        "['x' in object][0]",
        "({value:'x' in object}).value",
        "true ? 'x' in object : false",
    ] {
        yes(&format!(
            "var object={{x:1}},count=0;for(var ready={initializer};ready;ready=false){{count++;}}count===1;"
        ));
    }
    text(
        "var object={x:1},before;for(var key=(before=('x' in object)) in {only:1}){}before+':'+key;",
        "true:only",
    );
    text("var slot={};for((slot).key in {only:1}){}slot.key;", "only");
}

#[test]
fn regex_and_division_keep_the_grammar_selected_lexical_goal() {
    yes(&grouped("/a[\\/]/.test('a/')", 64));
    assert_eq!(
        value(&format!(
            "var n=12;{} / /xx/.source.length;",
            grouped("n", 64)
        )),
        Value::Number(6.0)
    );
    yes("if(true) (/a/.test('a'));else false;");
    yes("var result=(function(){return /a/.test('a');})();result;");
    assert_eq!(value("var n=12;n/=3;(n)/2;"), Value::Number(2.0));
    assert_eq!(value("var n=12;n\n/2/3;"), Value::Number(2.0));
    yes("var quotient=12/ /* comment */ 3;quotient===4 && /[/*]/.test('/');");
}

#[test]
fn grouping_does_not_invent_automatic_semicolons_or_remove_restricted_ones() {
    assert_eq!(value("var n=1;n\n++n;"), Value::Number(2.0));
    assert_eq!(
        value(&format!(
            "function run(){{return\n{};}}run();",
            grouped("42", 64)
        )),
        Value::Undefined
    );
    assert_eq!(
        value("var call=function(n){return n+1;};call\n(41);"),
        Value::Number(42.0)
    );
    assert_eq!(
        value("var object={value:42};object\n['value'];"),
        Value::Number(42.0)
    );
    assert_eq!(value("('use strict');42;"), Value::Number(42.0));
}

#[test]
fn nested_function_bodies_return_to_pending_group_call_and_member_work() {
    let function = grouped(
        "function(seed){return {run:function(delta){return seed+delta;}};}",
        64,
    );
    assert_eq!(
        value(&format!("({function})(40).run(2);")),
        Value::Number(42.0)
    );
    let inner = grouped("function(){return 42;}", 24);
    let outer = grouped(&format!("function(){{return ({inner})();}}"), 24);
    assert_eq!(value(&format!("({outer})();")), Value::Number(42.0));
}

fn rejects_without_prefix_effects(source: &str) {
    let mut runtime = Runtime::new();
    runtime.execute("var marker=1;", &mut NoIo).unwrap();
    let error = runtime
        .execute(&format!("marker=9;var introduced=2;{source}"), &mut NoIo)
        .unwrap_err();
    assert!(error.contains("SyntaxError"), "{source}: {error}");
    assert_eq!(runtime.get_global("marker"), Value::Number(1.0));
    assert_eq!(runtime.get_global("introduced"), Value::Undefined);
    assert_eq!(
        runtime.execute("marker+1;", &mut NoIo).unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn malformed_group_and_container_endings_reject_before_any_prefix_effects() {
    let complete = grouped("1", 64);
    rejects_without_prefix_effects(&complete[..complete.len() - 1]);
    rejects_without_prefix_effects(&format!("{complete})"));
    for source in [
        "()",
        "(1,)",
        "f((1,))",
        "({value:(1]})",
        "[(1,)]",
        "true?(1):",
        "new (function(){}) (",
        "(function(){return (1;})",
    ] {
        rejects_without_prefix_effects(source);
    }
}

#[test]
fn invalid_grouped_assignment_and_update_targets_are_whole_script_errors() {
    for source in [
        "((1))=2;",
        "(a+b)++;",
        "++(a?b:c);",
        "((a,b))=1;",
        "(function(){})=2;",
        "new Box()=2;",
        "((a=1))=2;",
    ] {
        rejects_without_prefix_effects(source);
    }
}

#[test]
fn malformed_dynamic_groups_are_catchable_and_do_not_execute_prefixes() {
    let malformed = format!("marker=9;{}1{}", "(".repeat(64), ")".repeat(63));
    for operation in [
        format!("eval({})", quoted(&malformed)),
        format!("(0,eval)({})", quoted(&malformed)),
        format!("Function({})", quoted(&malformed)),
    ] {
        yes(&format!(
            "var marker=1,caught=false;try{{{operation};}}catch(e){{caught=e.name==='SyntaxError';}}caught && marker===1;"
        ));
    }
}

fn source_limit_is_latched(source: String, expected: &'static str) {
    let (send, receive) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let mut runtime = Runtime::new();
        runtime.execute("var marker=1;", &mut NoIo).unwrap();
        let error = runtime
            .execute(&format!("marker=9;{source}"), &mut NoIo)
            .unwrap_err();
        let marker = runtime.get_global("marker");
        let later = runtime.execute("marker=7;", &mut NoIo).unwrap_err();
        send.send((error, marker, later, runtime.get_global("marker")))
            .unwrap();
    });
    let (error, marker, later, after) = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("Bounded local source must terminate");
    thread.join().unwrap();
    assert!(
        error.contains("parser limit exhausted") && error.contains(expected),
        "{error}"
    );
    assert_eq!(marker, Value::Number(1.0));
    assert_eq!(after, marker);
    assert_eq!(later, error);
}

#[test]
fn source_token_and_ast_node_caps_still_fail_before_prefixes_and_latch() {
    let exactly_one_mib = format!("{}42;", " ".repeat(1024 * 1024 - 3));
    assert_eq!(value(&exactly_one_mib), Value::Number(42.0));
    source_limit_is_latched(" ".repeat(1024 * 1024), "Source exceeds one MiB");
    source_limit_is_latched(";".repeat(100_001), "Token limit exceeded");
    source_limit_is_latched("1\n".repeat(50_001), "AST node limit exceeded");
}

#[test]
fn structural_ast_and_explicit_work_depth_stay_bounded() {
    source_limit_is_latched(
        format!("{}1;", "1+".repeat(128)),
        "AST depth limit exceeded",
    );
    source_limit_is_latched(grouped("1", 1024), "limit");
    source_limit_is_latched(format!("{}1{};", "[".repeat(256), "]".repeat(256)), "limit");
    let nested = format!(
        "{}1{}",
        "(function(){return ".repeat(128),
        ";})".repeat(128)
    );
    source_limit_is_latched(nested, "limit");
}

fn dynamic_limit_is_latched(operation: String, expected: &str) -> Value {
    let (send, receive) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let mut runtime = Runtime::new();
        let source = format!(
            "var caught=false,after=false,completed=0;
             function limited(){{try{{{operation}}}catch(e){{caught=true;return 1;}}finally{{after=true;return 2;}}}}
             limited();after=true;"
        );
        let error = runtime.execute(&source, &mut NoIo).unwrap_err();
        let caught = runtime.get_global("caught");
        let after = runtime.get_global("after");
        let completed = runtime.get_global("completed");
        let later = runtime.execute("after=true;42;", &mut NoIo).unwrap_err();
        send.send((
            error,
            caught,
            after,
            completed,
            later,
            runtime.get_global("after"),
        ))
        .unwrap();
    });
    let (error, caught, after, completed, later, after_later) = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("Bounded dynamic fixture must terminate");
    thread.join().unwrap();
    assert!(error.contains(expected), "{error}");
    assert_eq!(caught, Value::Bool(false));
    assert_eq!(after, Value::Bool(false));
    assert_eq!(after_later, Value::Bool(false));
    assert_eq!(later, error);
    completed
}

#[test]
fn dynamic_parser_limits_bypass_catch_finally_and_remain_latched() {
    let payloads = [
        grouped("1", 1024),
        "1\n".repeat(50_001),
        ";".repeat(100_001),
    ];
    for payload in payloads {
        for operation in [
            format!("eval({});", quoted(&payload)),
            format!("Function({});", quoted(&payload)),
        ] {
            assert_eq!(
                dynamic_limit_is_latched(operation, "parser limit exhausted"),
                Value::Number(0.0)
            );
        }
    }
}

#[test]
fn repeated_valid_and_invalid_group_compilation_share_cumulative_budgets() {
    let comment = "/* local grouping allocation accounting */".repeat(24);
    let valid = quoted(&format!("{};{comment}", grouped("1+1", 64)));
    let invalid = quoted(&format!("{}1{};{comment}", "(".repeat(64), ")".repeat(63)));
    for operation in [
        format!("while(true){{eval({valid});completed++;}}"),
        format!("while(true){{Function({valid});completed++;}}"),
        format!("while(true){{try{{eval({invalid});}}catch(syntax){{completed++;}}}}"),
        format!("while(true){{try{{Function({invalid});}}catch(syntax){{completed++;}}}}"),
    ] {
        let completed = dynamic_limit_is_latched(operation, "allocation budget exhausted");
        assert!(
            matches!(completed, Value::Number(count) if count>0.0),
            "{completed:?}"
        );
    }
}

#[test]
fn mixed_expression_and_function_recursion_stays_bounded_on_default_stack() {
    const CHILD: &str = "MGBROWSER_EXPRESSION_LIMIT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        // Isolate any Rust stack abort from the parent suite. These are original
        // finite source fixtures, not a larger-stack workaround or other engine.
        for depth in [8, 32, 96] {
            eprintln!("Local pending-unary recursion depth {depth}");
            let mut runtime = Runtime::new();
            let source = format!(
                "function recurse(){{return {}recurse();}}recurse();",
                "+ ".repeat(depth)
            );
            let error = runtime.execute(&source, &mut NoIo).unwrap_err();
            assert!(
                error.contains("limit") || error.contains("exhausted"),
                "{error}"
            );
            assert!(
                !error.contains("parser"),
                "Fixture must reach evaluator: {error}"
            );
            assert_eq!(runtime.execute("42;", &mut NoIo).unwrap_err(), error);
        }
        return;
    }
    use std::{
        process::{Command, Stdio},
        time::Instant,
    };
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "mixed_expression_and_function_recursion_stays_bounded_on_default_stack",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env_remove("RUST_MIN_STACK")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Start only this owned default-stack test child");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("Owned pending-expression test child exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Default-stack child {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
