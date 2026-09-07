//! Independently authored local cases, not an imported conformance corpus.
//! Language references: ECMA-262 5.1 sections 7.9, 12.6-12.9, 12.12 and 12.14:
//! https://262.ecma-international.org/5.1/
//! Resource exhaustion below is mgbrowser policy, not ECMAScript behavior.

use mg_deps::js::runtime::{Host, Runtime, Value};
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

fn value(source: &str) -> Value {
    Runtime::new()
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("Local control-flow case failed: {error}\n{source}"))
}

fn text(source: &str, expected: &str) {
    assert_eq!(value(source), Value::text(expected), "{source}");
}

#[test]
fn labeled_block_break_exits_exactly_the_named_statement() {
    text(
        "var trace=''; outer:{ trace+='a'; inner:{ trace+='b'; break outer; } trace+='wrong'; } trace+='c'; trace;",
        "abc",
    );
    text(
        "var trace=''; outer:{ inner:{ trace+='a'; break inner; } trace+='b'; } trace+='c'; trace;",
        "abc",
    );
    text(
        "var trace=''; choice:if(true){ trace+='a'; break choice; trace+='wrong'; } trace+='b'; trace;",
        "ab",
    );
}

#[test]
fn stacked_labels_refer_to_the_same_for_while_and_do_loop() {
    for (source, expected) in [
        (
            "var n=0,trace=''; first:second:for(;n<3;n++){trace+=n;if(n<2)continue first;break second;} trace+':'+n;",
            "012:2",
        ),
        (
            "var n=0,trace=''; first:second:while(n<5){n++;trace+=n;if(n<3)continue second;break first;} trace+':'+n;",
            "123:3",
        ),
        (
            "var n=0,trace=''; first:second:do{n++;if(n<3)continue first;trace+=n;}while(n<3); trace+':'+n;",
            "3:3",
        ),
    ] {
        text(source, expected);
    }
}

#[test]
fn outer_continue_runs_for_update_and_test_without_inner_update() {
    text(
        "var i=0,tests=0,updates=0,inner=0,innerUpdates=0,tail=0;
         outer:for(;(tests++,i<3);(updates++,i++)){
             for(var j=0;j<2;(innerUpdates++,j++)){inner++;continue outer;}
             tail++;
         }
         i+':'+tests+':'+updates+':'+inner+':'+innerUpdates+':'+tail;",
        "3:4:3:3:0:0",
    );
}

#[test]
fn outer_continue_rechecks_while_and_do_conditions() {
    text(
        "var n=0,tests=0,tail=0; outer:while((tests++,n<3)){
             n++;for(var j=0;j<2;j++){continue outer;}tail++;
         } n+':'+tests+':'+tail;",
        "3:4:0",
    );
    text(
        "var n=0,tests=0,tail=0; outer:do{
             n++;for(var j=0;j<2;j++){continue outer;}tail++;
         }while((tests++,n<3)); n+':'+tests+':'+tail;",
        "3:3:0",
    );
}

#[test]
fn unlabeled_break_only_exits_the_inner_loop() {
    text(
        "var trace='',i=0; outer:for(;i<3;i++){
             for(var j=0;j<3;j++){trace+=i;break;}
             trace+='x';if(i===1)break outer;
         } trace+':'+i;",
        "0x1x:1",
    );
}

#[test]
fn finally_preserves_pending_labeled_break_and_continue() {
    text(
        "var trace=''; target:{
             try{trace+='t';break target;}catch(e){trace+='wrong';}finally{trace+='f';}
             trace+='wrong';
         } trace+='a';trace;",
        "tfa",
    );
    text(
        "var trace=''; outer:for(var i=0;i<2;i++){
             try{trace+='t'+i;continue outer;}finally{trace+='f'+i;}
             trace+='wrong';
         } trace;",
        "t0f0t1f1",
    );
}

#[test]
fn finally_can_replace_break_with_continue_and_continue_with_break() {
    text(
        "var trace='',i=0; outer:for(;i<2;i++){
             try{trace+='t'+i;break outer;}finally{trace+='f'+i;continue outer;}
         } trace+':'+i;",
        "t0f0t1f1:2",
    );
    text(
        "var trace='',i=0; outer:for(;i<2;i++){
             try{continue outer;}finally{trace+='f';break outer;}
         } trace+':'+i;",
        "f:0",
    );
}

#[test]
fn finally_can_replace_the_target_of_a_labeled_break() {
    text(
        "var trace=''; outer:{inner:{
             try{trace+='t';break outer;}finally{trace+='f';break inner;}
         }trace+='i';}trace+='o';trace;",
        "tfio",
    );
}

#[test]
fn nested_finally_blocks_run_in_order_before_labeled_exit() {
    text(
        "var trace=''; target:{
             try{try{break target;}finally{trace+='i';}}finally{trace+='o';}
             trace+='wrong';
         } trace;",
        "io",
    );
}

#[test]
fn finally_preserves_return_or_replaces_it_with_labeled_control() {
    text(
        "var trace='';function answer(){target:{try{return 3;}finally{trace+='f';}}}
         answer()+':'+trace;",
        "3:f",
    );
    assert_eq!(
        value("function answer(){exit:{try{return 1;}finally{break exit;}}return 2;}answer();"),
        Value::Number(2.0),
    );
    assert_eq!(
        value(
            "function answer(){var hits=0;outer:for(var i=0;i<2;i++){try{return 4;}finally{hits++;continue outer;}}return hits;}answer();"
        ),
        Value::Number(2.0),
    );
}

#[test]
fn finally_return_or_throw_can_replace_a_labeled_break() {
    assert_eq!(
        value(
            "function answer(){outer:while(true){try{break outer;}finally{return 7;}}return 9;}answer();"
        ),
        Value::Number(7.0),
    );
    assert_eq!(
        value(
            "var result=0;try{outer:{try{break outer;}finally{throw 7;}}}catch(e){result=e;}result;"
        ),
        Value::Number(7.0),
    );
    text(
        "var trace='';outer:{try{throw 1;}catch(e){trace+='c'+e;throw 2;}finally{trace+='f';break outer;}}trace;",
        "c1f",
    );
}

#[test]
fn labels_can_be_reused_after_their_scope_and_inside_a_new_function() {
    assert_eq!(
        value("var result=0;same:{result++;break same;}same:{result++;break same;}result;"),
        Value::Number(2.0),
    );
    assert_eq!(
        value(
            "var result=0;same:{var f=function(){same:{break same;}return 7;};result=f();break same;}result;"
        ),
        Value::Number(7.0),
    );
    text(
        "var trace='';lower:{Lower:{trace+='a';break Lower;}trace+='b';break lower;}trace;",
        "ab",
    );
}

#[test]
fn invalid_labels_reject_the_whole_script_before_any_prefix_runs() {
    for invalid in [
        "dup:{dup:{}}",
        "dup:dup:while(false){}",
        "while(false){break missing;}",
        "while(false){continue missing;}",
        "block:{while(false){continue block;}}",
        "block:if(true)while(false){continue block;}",
        "block:{break;}",
        "block:{continue;}",
        "while(false){var f=function(){break;};}",
        "while(false){var f=function(){continue;};}",
        "outer:while(false){var f=function(){break outer;};}",
        "outer:while(false){var f=function(){continue outer;};}",
        "outer:{var f=function(){break outer;};}",
    ] {
        let mut runtime = Runtime::new();
        runtime.set_global("marker", Value::Number(0.0));
        let source = format!("marker=1;{invalid}");
        assert!(
            runtime.execute(&source, &mut NoIo).is_err(),
            "Invalid label context accepted: {invalid}",
        );
        assert_eq!(
            runtime.get_global("marker"),
            Value::Number(0.0),
            "Invalid script executed its prefix: {invalid}",
        );
    }
}

#[test]
fn newline_after_break_or_continue_does_not_bind_the_next_identifier() {
    // `missing` is an unreachable expression statement, not a missing label.
    for separator in ["\n", "\r\n", "/* line\nbreak */", "// comment\n"] {
        text(
            &format!(
                "var trace='';outer:for(var i=0;i<2;i++){{for(var j=0;j<2;j++){{trace+='i';break{separator}missing;}}trace+='o';}}trace;"
            ),
            "ioio",
        );
        text(
            &format!(
                "var trace='';outer:for(var i=0;i<2;i++){{for(var j=0;j<2;j++){{trace+='i';continue{separator}missing;}}trace+='o';}}trace;"
            ),
            "iioiio",
        );
    }
}

#[test]
fn comments_without_a_newline_preserve_the_label_operand() {
    text(
        "var trace='';outer:for(var i=0;i<2;i++){trace+='a';break/*same line*/outer;}trace;",
        "a",
    );
    text(
        "var trace='';outer:for(var i=0;i<2;i++){for(var j=0;j<2;j++){trace+='a';continue/*same line*/outer;}trace+='wrong';}trace;",
        "aa",
    );
}

#[test]
fn labeled_loop_fuel_exhaustion_cannot_be_caught_or_overridden_by_finally() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let mut runtime = Runtime::new();
        let result = runtime.execute(
            "var caught=false,after=false;
             function busy(){
                 try{endless:for(;;){continue endless;}}
                 catch(e){caught=true;return 1;}
                 finally{return 2;}
             }
             busy();after=true;",
            &mut NoIo,
        );
        let caught = runtime.get_global("caught");
        let after = runtime.get_global("after");
        let later = runtime.execute("after=true;42;", &mut NoIo);
        let after_later = runtime.get_global("after");
        sender
            .send((result, caught, after, later, after_later))
            .unwrap();
    });
    // A broken fuel loop cannot make this test wait forever. The test process
    // terminates a detached evaluator if this bounded receive ever fails.
    let (result, caught, after, later, after_later) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("Labeled loop must exhaust fuel within the local test deadline");
    thread.join().expect("local evaluator thread");
    let error = result.expect_err("Exhaustion must not become a catch/finally return");
    assert!(error.contains("fuel"), "Expected fuel exhaustion: {error}");
    assert_eq!(caught, Value::Bool(false));
    assert_eq!(after, Value::Bool(false));
    assert_eq!(later.expect_err("Exhaustion must remain latched"), error);
    assert_eq!(after_later, Value::Bool(false));
}
