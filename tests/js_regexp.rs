//! Independently authored RegExp integration cases. No website source or other
//! engine supplies the expected results. ES5.1 sections 7.8.5, 15.10 and 15.5.4:
//! https://262.ecma-international.org/5.1/ . Resource caps are project policy.

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
        .unwrap_or_else(|error| panic!("Local RegExp case failed: {error}\n{source}"))
}
fn yes(source: &str) {
    assert_eq!(value(source), Value::Bool(true), "{source}");
}
fn text(source: &str, expected: &str) {
    assert_eq!(value(source), Value::text(expected), "{source}");
}

#[test]
fn literal_goals_preserve_division_comments_and_statement_context() {
    for source in [
        "var n=12;n/=3;n/2;",
        "({value:8}).value/4;",
        "var n={valueOf:function(){return 8;}}/4;n;",
        "var n=12;n\n/2/3;",
        "var f=function(){return 8;};f()/4;",
    ] {
        assert_eq!(value(source), Value::Number(2.0), "{source}");
    }
    yes("if(true) /a/.test('a');");
    yes("function ready(){return 8;} /a/.test('a');");
    yes("var found=false;block:{found=true;} /a/.test('a') && found;");
    yes("var result=/* before */ /a/.test('a'); result; // after");
    yes(r"/[\/\]]+/.test('/]');");
    yes(r"/=/g.test('=');");
    assert_eq!(value("12 / /x/.exec('x').length;"), Value::Number(12.0));
}

#[test]
fn literals_are_fresh_and_constructor_identity_is_es5_shaped() {
    yes(
        "function make(){return /x/g;}var a=make(),b=make();a.lastIndex=1;a!==b && b.lastIndex===0;",
    );
    yes(
        "var r=/x/im;r.lastIndex=3;var clone=new RegExp(r);RegExp(r)===r && clone!==r && clone.ignoreCase && clone.multiline && clone.lastIndex===0;",
    );
    yes("var caught=false;try{RegExp(/x/,'i');}catch(e){caught=true;}caught;");
    yes("new RegExp().test('anything') && RegExp(undefined).source==='(?:)';");
}

#[test]
fn captures_include_input_index_and_unmatched_undefined() {
    text(
        "var m=/(a)(b)?/.exec('xa!');m[0]+':'+m[1]+':'+m[2]+':'+m.index+':'+m.input+':'+m.length;",
        "a:a:undefined:1:xa!:3",
    );
    yes("/z/.exec('abc')===null && !/z/.test('abc');");
    yes("var m=/(?:a)(b)/.exec('ab');m.length===2 && m[1]==='b';");
}

#[test]
fn global_last_index_advances_resets_and_counts_utf16_units() {
    text(
        "var r=/a/g;var a=r.exec('aba'),first=r.lastIndex,b=r.exec('aba'),second=r.lastIndex,c=r.exec('aba');a.index+':'+first+':'+b.index+':'+second+':'+c+':'+r.lastIndex;",
        "0:1:2:3:null:0",
    );
    yes("var r=/a/;r.lastIndex=9;r.exec('ba').index===1 && r.lastIndex===9;");
    yes(
        "var r=/./g;r.exec('😀');r.lastIndex===1 && r.exec('😀')[0].charCodeAt(0)===56832 && r.lastIndex===2;",
    );
    yes("var r=/a/g;r.lastIndex=1.9;r.exec('ba').index===1 && r.lastIndex===2;");
    yes("var r=/a/g;r.lastIndex=-1;r.exec('a')===null && r.lastIndex===0;");
    yes("var r=/(?:)/g;r.lastIndex=1;r.exec('ab').index===1 && r.lastIndex===1;");
}

#[test]
fn metadata_is_readonly_but_last_index_is_writable_and_not_enumerable() {
    yes(
        "var r=/word/igm;r.source='wrong';r.global=false;r.ignoreCase=false;r.multiline=false;r.lastIndex=2;r.source==='word' && r.global && r.ignoreCase && r.multiline && r.lastIndex===2;",
    );
    yes("var r=/x/g;!(delete r.lastIndex) && !(delete r.source) && Object.keys(r).length===0;");
    text("/x/mi.toString();", "/x/im");
    yes("RegExp.prototype.test('anything') && Object.getPrototypeOf(/x/)===RegExp.prototype;");
    text("Object.prototype.toString.call(/x/);", "[object RegExp]");
}

#[test]
fn exec_coerces_input_before_last_index_and_rejects_incompatible_receivers() {
    text(
        "var order='',r=/a/;r.lastIndex={valueOf:function(){order+='i';return 8;}};var found=r.exec({toString:function(){order+='s';return 'a';}});order+':'+found[0];",
        "si:a",
    );
    yes("var r=/z/;r.lastIndex=9;r.exec('a')===null && r.lastIndex===0;");
    yes("var caught=false;try{RegExp.prototype.exec.call({},'a');}catch(e){caught=true;}caught;");
    yes("var caught=false;try{RegExp.prototype.test.call('a','a');}catch(e){caught=true;}caught;");
}

#[test]
fn constructors_coerce_pattern_then_flags_and_reject_bad_patterns() {
    text(
        "var order='';var r=RegExp({toString:function(){order+='p';return 'a';}},{toString:function(){order+='f';return 'i';}});order+':'+r.test('A');",
        "pf:true",
    );
    for source in [
        "RegExp('(')",
        "RegExp('[z-a]')",
        "RegExp('*')",
        "RegExp('x','gg')",
        "RegExp('x','u')",
    ] {
        yes(&format!(
            "var caught=false;try{{{source};}}catch(e){{caught=e.name==='SyntaxError' && e.message.length>0;}}caught;"
        ));
    }
}

#[test]
fn literal_validation_rejects_whole_script_before_prefix_side_effects() {
    for literal in ["/(/", "/[z-a]/", "/x/gg", "/x/u", "/unterminated", "/a\nb/"] {
        let mut runtime = Runtime::new();
        runtime.execute("var marker=1;", &mut NoIo).unwrap();
        let error = runtime
            .execute(&format!("marker=9;{literal};"), &mut NoIo)
            .unwrap_err();
        assert!(error.contains("SyntaxError"), "{literal}: {error}");
        assert_eq!(runtime.get_global("marker"), Value::Number(1.0));
    }
    yes(r#"eval('/\\d+/.test("123")');"#);
    yes(r#"Function('return /a+/.test("aaa");')();"#);
}

#[test]
fn greedy_lazy_alternation_and_capture_repetition_are_observable() {
    text("/a+/.exec('aaa')[0]+':'+/a+?/.exec('aaa')[0];", "aaa:a");
    text("/(ab|a)b/.exec('ab')[1];", "a");
    text("/a{2,3}/.exec('aaaa')[0];", "aaa");
    yes("var m=/(a|(b))+/.exec('ba');m[1]==='a' && m[2]===undefined;");
    yes("/(?:a?)*b/.test('b') && /(?:){2}/.test('');");
}

#[test]
fn anchors_classes_boundaries_and_case_are_not_lossy_unicode_matching() {
    yes(r"/^\w+\s\d+$/.test('A_ 19') && !/^\w+$/.test('é');");
    yes(r"/\bcat\b/.test('a cat!') && !/\bcat\b/.test('scatter');");
    yes(r"/^b$/m.test('a\nb\nc') && !/^b$/.test('a\nb\nc');");
    yes(r"!/^a$/.test('a\n') && /^a$/m.test('a\n');");
    yes(r"/^[^a-c]+$/.test('XYZ') && !/^[^a-c]+$/.test('Xa');");
    yes(r"!/^.$/.test('\n') && /^[\s\S]$/.test('\n');");
    yes(r"/é/i.test('É') && !/[a-z]/i.test('\u0131') && !/[a-z]/i.test('\u017f');");
    yes(r"/^.$/.test('\ud800') && /\uD800/.test('\ud800');");
}

#[test]
fn backreferences_and_lookahead_preserve_capture_semantics() {
    yes(r"/^(ab)\1$/.test('abab') && !/^(ab)\1$/.test('abac');");
    yes(r"/^(a)?\1b$/.test('b');");
    text(r"/a(?=(b))/.exec('ab')[1];", "b");
    yes(r"/a(?!b)/.test('ac') && !/a(?!b)/.test('ab');");
    yes(r"/(?=(a+))a\1/.exec('aa')===null;");
}

#[test]
fn string_match_and_search_have_distinct_last_index_behavior() {
    text(r"'a12b3'.match(/\d+/g).join(',');", "12,3");
    yes("'abc'.match(/z/g)===null && 'abc'.match(/(b)/)[1]==='b';");
    yes("var r=/a/g;r.lastIndex=7;'ba'.search(r)===1 && r.lastIndex===7;");
    yes("var r=/(?:)/g;'ab'.match(r).length===3 && r.lastIndex===0;");
    assert_eq!(value(r"'a.b'.search('\\.');"), Value::Number(1.0));
}

#[test]
fn replacement_supports_strings_captures_and_prefix_suffix_expansion() {
    text("'aba'.replace('a','X');", "Xba");
    text("'aba'.replace(/a/g,'X');", "XbX");
    text("'ab'.replace(/(a)(b)/,'$2$1');", "ba");
    text(
        "'xabz'.replace(/(a)(b)/,\"[$$][$&][$`][$'][$1][$2]\");",
        "x[$][ab][x][z][a][b]z",
    );
    text("'b'.replace(/(a)?b/,'<$1>');", "<>");
    text("'ab'.replace(/(?:)/g,'-');", "-a-b-");
}

#[test]
fn replacement_callbacks_receive_capture_offset_and_original_input() {
    text(
        r"var calls='';var result='a1b2'.replace(/(\d)/g,function(all,digit,offset,input){calls+=digit+offset;return '['+digit+']';});result+':'+calls;",
        "a[1]b[2]:1123",
    );
    yes(
        "var seen=false;'b'.replace(/(a)?b/,function(all,capture,offset,input){seen=capture===undefined && offset===0 && input==='b';return 7;});seen;",
    );
    text(
        "'xa'.replace('a',function(all,offset,input){return offset+input;});",
        "x1xa",
    );
}

#[test]
fn splitting_handles_strings_captured_separators_empty_matches_and_limits() {
    text("'a,b,c'.split(',').join('|');", "a|b|c");
    text("'abc'.split('').join('|');", "a|b|c");
    text(r"'a1b2'.split(/(\d)/).join('|');", "a|1|b|2|");
    text(r"'a1b2'.split(/(\d)/,3).join('|');", "a|1|b");
    text("'ab'.split(/(?:)/).join('|');", "a|b");
    yes("''.split(/(?:)/).length===0 && ''.split(/a/).length===1;");
    yes("var r=/,/g;r.lastIndex=7;'a,b'.split(r).length===2 && r.lastIndex===7;");
    yes("'a,b'.split(',',0).length===0 && 'a,b'.split(undefined).length===1;");
}

#[test]
fn regex_objects_work_across_eval_and_constructor_created_functions() {
    yes(r"var r=eval('/a/g');r.test('a') && r.lastIndex===1;");
    yes(r"var f=Function('text','return /^q$/.test(text);');f('q') && !f('other');");
}

#[test]
fn matching_and_compilation_share_uncatchable_latched_budgets() {
    for operation in [
        "var r=/a/;while(true){r.test('a');}",
        "while(true){new RegExp('a');}",
    ] {
        let (send, receive) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let mut runtime = Runtime::new();
            let source = format!(
                "var caught=false,after=false;try{{{operation}}}catch(e){{caught=true;}}finally{{after=true;}}"
            );
            let error = runtime.execute(&source, &mut NoIo).unwrap_err();
            let caught = runtime.get_global("caught");
            let after = runtime.get_global("after");
            let later = runtime.execute("1", &mut NoIo).unwrap_err();
            send.send((error, caught, after, later)).unwrap();
        });
        let (error, caught, after, later) = receive
            .recv_timeout(Duration::from_secs(5))
            .expect("RegExp operation must terminate");
        thread.join().unwrap();
        assert!(
            error.contains("budget") || error.contains("fuel") || error.contains("limit"),
            "{error}"
        );
        assert_eq!(caught, Value::Bool(false));
        assert_eq!(after, Value::Bool(false));
        assert_eq!(later, error);
    }
}

#[test]
fn matcher_consumes_the_callers_fuel_without_a_private_reset() {
    use mg_deps::js::regexp::{Error, Regex};
    let pattern: Vec<u16> = "a".encode_utf16().collect();
    let input: Vec<u16> = "a".encode_utf16().collect();
    let regex = Regex::compile(&pattern, "").unwrap();
    let mut fuel = 0;
    assert!(matches!(
        regex.find(&input, 0, &mut fuel),
        Err(Error::Limit(_))
    ));
    let mut fuel = 100;
    let found = regex.find(&input, 0, &mut fuel).unwrap().unwrap();
    assert_eq!((found.start, found.end), (0, 1));
    assert!(fuel < 100);
    let remaining = fuel;
    regex.find(&input, 0, &mut fuel).unwrap();
    assert!(fuel < remaining);
}
