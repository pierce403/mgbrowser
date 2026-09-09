//! Original Error-family cases derived from ES5.1 sections 15.11.1–15.11.7.
//! https://262.ecma-international.org/5.1/#sec-15.11
//! Symbol coercion/tagging follows this project's explicitly adopted extensions;
//! ordinary runtime exception strings remain a separate documented limitation.

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

const FAMILIES: [&str; 6] = [
    "Error",
    "TypeError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "URIError",
];

fn yes(source: &str) {
    let value = Runtime::new()
        .execute(source, &mut NoIo)
        .unwrap_or_else(|error| panic!("Error-family case failed: {error}\n{source}"));
    assert_eq!(value, Value::Bool(true), "{source}");
}

fn type_error(setup: &str, expression: &str) {
    yes(&format!(
        "{setup};var caught=false;try{{{expression};}}catch(e){{caught=String(e).indexOf('TypeError')>=0;}}caught;"
    ));
}

#[test]
fn all_exposed_families_have_distinct_prototypes_and_back_references() {
    yes(r#"
        var constructors=[Error,TypeError,RangeError,ReferenceError,SyntaxError,URIError],ok=true;
        for(var i=0;i<constructors.length;i++){
            var C=constructors[i],p=C.prototype;
            ok=ok && typeof p==='object' && p!==null && C.hasOwnProperty('prototype') &&
                p.hasOwnProperty('constructor') && p.constructor===C &&
                Object.getPrototypeOf(p)===(i===0?Object.prototype:Error.prototype);
            for(var j=0;j<i;j++)ok=ok && p!==constructors[j].prototype;
        }
        ok;
    "#);
}

#[test]
fn constructor_parent_retains_es5_function_prototype() {
    for family in FAMILIES {
        yes(&format!(
            "typeof {family}==='function' && {family}.length===1 && Object.getPrototypeOf({family})===Function.prototype && {family} instanceof Function;"
        ));
    }
}

#[test]
fn call_and_new_create_distinct_instances_of_the_right_family() {
    for family in FAMILIES {
        yes(&format!(
            "var a={family}('one'),b=new {family}('two');a!==b && Object.getPrototypeOf(a)==={family}.prototype && Object.getPrototypeOf(b)==={family}.prototype && a instanceof {family} && b instanceof {family} && a instanceof Error && b instanceof Error && a.constructor==={family} && a.message==='one' && b.message==='two';"
        ));
    }
    yes("!(new TypeError() instanceof RangeError) && !(new Error() instanceof TypeError);");
}

#[test]
fn ordinary_runtime_exception_strings_remain_an_explicit_control() {
    yes(
        "var caught;try{null.field;}catch(e){caught=e;}typeof caught==='string' && caught.indexOf('TypeError:')===0;",
    );
}

#[test]
fn prototype_defaults_are_own_non_enumerable_and_to_string_is_shared() {
    for family in FAMILIES {
        yes(&format!(
            "var p={family}.prototype;p.name==='{family}' && p.message==='' && p.hasOwnProperty('name') && p.hasOwnProperty('message') && Object.keys(p).length===0 && Object.getOwnPropertyNames(p).indexOf('constructor')>=0 && Object.getOwnPropertyNames(p).indexOf('name')>=0 && Object.getOwnPropertyNames(p).indexOf('message')>=0 && p.toString===Error.prototype.toString;"
        ));
        if family != "Error" {
            yes(&format!("!{family}.prototype.hasOwnProperty('toString');"));
        }
    }
    yes(
        "Error.prototype.hasOwnProperty('toString') && Error.prototype.toString.name==='toString' && Error.prototype.toString.length===0;",
    );
}

#[test]
fn constructor_prototype_slots_are_readonly_nonconfigurable_and_shadow_blocking() {
    for family in FAMILIES {
        yes(&format!(
            "var C={family},p=C.prototype,child=Object.create(C);C.prototype={{}};child.prototype={{}};var removed=delete C.prototype;!removed && C.prototype===p && child.prototype===p && !child.hasOwnProperty('prototype') && Object.keys(C).indexOf('prototype')<0 && Object.getOwnPropertyNames(C).indexOf('prototype')>=0;"
        ));
    }
}

#[test]
fn prototype_defaults_and_back_references_are_writable_and_configurable() {
    for family in FAMILIES {
        yes(&format!(
            "var C={family},p=C.prototype;p.name='Renamed';p.message='default';p.constructor=Array;var e=C();var changed=e.name==='Renamed' && e.message==='default' && e.constructor===Array && Object.keys(p).length===0;var removedName=delete p.name,removedMessage=delete p.message,removedConstructor=delete p.constructor;changed && removedName && removedMessage && removedConstructor && !p.hasOwnProperty('name') && !p.hasOwnProperty('message') && !p.hasOwnProperty('constructor');"
        ));
    }
    yes(
        "var method=Error.prototype.toString;Error.prototype.toString=function(){return 'changed';};var changed=String(new Error())==='changed';changed && delete Error.prototype.toString && !Error.prototype.hasOwnProperty('toString') && typeof method==='function';",
    );
}

#[test]
fn omitted_and_undefined_messages_do_not_create_instance_properties() {
    for family in FAMILIES {
        yes(&format!(
            "var a={family}(),b=new {family}(undefined);!a.hasOwnProperty('name') && !b.hasOwnProperty('name') && !a.hasOwnProperty('message') && !b.hasOwnProperty('message') && a.message==='' && b.message==='' && Object.getOwnPropertyNames(a).length===0 && Object.getOwnPropertyNames(b).length===0 && a.name==='{family}';"
        ));
    }
}

#[test]
fn provided_messages_are_converted_and_own_writable_non_enumerable_configurable() {
    for (input, expected) in [("null", "null"), ("false", "false"), ("0", "0"), ("''", "")] {
        for family in FAMILIES {
            yes(&format!(
                "var e={family}({input});var initial=e.message==='{expected}' && e.hasOwnProperty('message') && !e.hasOwnProperty('name') && Object.keys(e).length===0 && Object.getOwnPropertyNames(e).join(',')==='message';e.message='edited';var changed=e.message==='edited' && Object.keys(e).length===0;initial && changed && delete e.message && !e.hasOwnProperty('message') && e.message==='';"
            ));
        }
    }
}

#[test]
fn existing_instances_observe_prototype_changes_and_ordinary_shadow_deletion() {
    yes(r#"
        var e=new TypeError('own');TypeError.prototype.name='Changed';TypeError.prototype.message='inherited';
        var inherited=e.name==='Changed' && e.message==='own' && !e.hasOwnProperty('name');
        e.name='Local';var shadow=e.name==='Local' && TypeError.prototype.name==='Changed';
        delete e.name;delete e.message;
        inherited && shadow && e.name==='Changed' && e.message==='inherited' &&
            String(e)==='Changed: inherited' && Object.keys(e).length===0;
    "#);
}

#[test]
fn constructor_call_and_apply_ignore_this_without_mutating_it() {
    for family in FAMILIES {
        yes(&format!(
            "var target={{name:'old',message:'untouched'}},a={family}.call(target,'one'),b={family}.apply(target,['two']);a!==target && b!==target && a!==b && a instanceof {family} && b instanceof {family} && a.message==='one' && b.message==='two' && target.name==='old' && target.message==='untouched';"
        ));
    }
}

#[test]
fn constructor_argument_expressions_precede_one_string_conversion_and_extras_are_unused() {
    for family in FAMILIES {
        yes(&format!(
            r#"
            var trail='',message={{}},extra={{}};
            message[Symbol.toPrimitive]=function(hint){{trail+='convert:'+hint+';';return 'text';}};
            extra.toString=function(){{throw 'unused argument was converted';}};
            function first(){{trail+='first;';return message;}}
            function second(){{trail+='second;';return extra;}}
            var e={family}(first(),second());
            trail==='first;second;convert:string;' && e.message==='text';
        "#
        ));
    }
}

#[test]
fn constructor_coercion_throws_exact_value_and_symbol_messages_are_rejected() {
    for family in FAMILIES {
        yes(&format!(
            "var marker={{}},seen=0,message={{toString:function(){{seen++;throw marker;}}}},caught;try{{{family}(message);}}catch(e){{caught=e;}}caught===marker && seen===1;"
        ));
        type_error("", &format!("{family}(Symbol('message'))"));
        type_error("", &format!("new {family}(Object(Symbol('message')))"));
    }
}

#[test]
fn messages_and_formatted_names_preserve_utf16_code_units() {
    for family in FAMILIES {
        yes(&format!(
            r#"
            var e={family}('\uD800x\uDC00');e.name='\uDC01';var text=String(e);
            e.message.length===3 && e.message.charCodeAt(0)===55296 && e.message.charCodeAt(2)===56320 &&
                text.length===6 && text.charCodeAt(0)===56321 && text.charCodeAt(3)===55296 && text.charCodeAt(5)===56320;
        "#
        ));
    }
}

#[test]
fn generic_to_string_formats_undefined_empty_and_non_string_fields() {
    for (receiver, expected) in [
        ("{}", "Error"),
        ("{name:undefined,message:undefined}", "Error"),
        ("{name:'',message:''}", ""),
        ("{name:'',message:'message'}", "message"),
        ("{name:'Name',message:''}", "Name"),
        ("{name:'Name',message:'message'}", "Name: message"),
        ("{name:null,message:false}", "null: false"),
        ("{name:0,message:7}", "0: 7"),
        ("Object.create(null)", "Error"),
        ("Object(7)", "Error"),
    ] {
        yes(&format!(
            "Error.prototype.toString.call({receiver})==='{expected}' && Error.prototype.toString.apply({receiver},[])==='{expected}';"
        ));
    }
}

#[test]
fn generic_to_string_supports_function_receivers_and_inherited_fields() {
    yes(r#"
        function Named(){}Named.message='function';
        var parent={name:'Ancestor',message:'parent'},child=Object.create(parent);
        child.message='child';
        Error.prototype.toString.call(Named)==='Named: function' &&
            Error.prototype.toString.call(parseInt)==='parseInt' &&
            Error.prototype.toString.call(child)==='Ancestor: child' &&
            !child.hasOwnProperty('name') && parent.message==='parent';
    "#);
}

#[test]
fn generic_to_string_rejects_primitive_receivers_without_boxing() {
    for receiver in [
        "undefined",
        "null",
        "true",
        "false",
        "0",
        "'text'",
        "Symbol('x')",
    ] {
        type_error("", &format!("Error.prototype.toString.call({receiver})"));
        type_error(
            "",
            &format!("Error.prototype.toString.apply({receiver},[])"),
        );
    }
}

#[test]
fn name_conversion_finishes_before_message_is_read_or_converted() {
    yes(r#"
        var trail='',receiver={message:'old'},name={};
        name.toString=function(){trail+='name-string;';receiver.message={toString:function(){trail+='message-string;';return 'new';}};return {};};
        name.valueOf=function(){trail+='name-value;';return 'Name';};receiver.name=name;
        Error.prototype.toString.call(receiver)==='Name: new' &&
            trail==='name-string;name-value;message-string;';
    "#);
}

#[test]
fn generic_to_string_symbol_hooks_receive_string_hint_and_original_field_receiver() {
    yes(r#"
        var trail='',name={},message={};
        name[Symbol.toPrimitive]=function(hint){trail+=(this===name?'name:':'wrong:')+hint+';';return 'N';};
        message[Symbol.toPrimitive]=function(hint){trail+=(this===message?'message:':'wrong:')+hint+';';return 'M';};
        Error.prototype.toString.call({name:name,message:message})==='N: M' && trail==='name:string;message:string;';
    "#);
}

#[test]
fn formatting_abrupt_name_conversion_skips_message_and_message_throw_preserves_order() {
    yes(r#"
        var marker={},trail='',name={toString:function(){trail+='name';throw marker;}},message={toString:function(){trail+='message';return 'M';}},caught;
        try{Error.prototype.toString.call({name:name,message:message});}catch(e){caught=e;}
        caught===marker && trail==='name';
    "#);
    yes(r#"
        var marker={},trail='',name={toString:function(){trail+='name;';return 'N';}},message={toString:function(){trail+='message;';throw marker;}},caught;
        try{Error.prototype.toString.call({name:name,message:message});}catch(e){caught=e;}
        caught===marker && trail==='name;message;';
    "#);
    type_error(
        "",
        "Error.prototype.toString.call({name:Symbol('n'),message:'m'})",
    );
    type_error(
        "",
        "Error.prototype.toString.call({name:'n',message:Symbol('m')})",
    );
    type_error(
        "var name={};name[Symbol.toPrimitive]=function(){return Symbol('n');};",
        "Error.prototype.toString.call({name:name,message:'m'})",
    );
}

#[test]
fn genuine_error_brand_is_not_inherited_or_inferred_from_names() {
    for family in FAMILIES {
        yes(&format!(
            "var e=new {family}(),p={family}.prototype,child=Object.create(p);Object.prototype.toString.call(e)==='[object Error]' && Object.prototype.toString.call(p)==='[object Error]' && Object.prototype.toString.call(child)==='[object Object]' && child instanceof {family} && Object.prototype.toString.call({{name:'{family}',message:'fake'}})==='[object Object]';"
        ));
    }
}

#[test]
fn symbol_to_string_tag_overrides_fallback_without_changing_error_identity() {
    yes(r#"
        var e=new TypeError('m'),child=Object.create(TypeError.prototype);
        TypeError.prototype[Symbol.toStringTag]='Tagged';
        var inherited=Object.prototype.toString.call(e)==='[object Tagged]' && Object.prototype.toString.call(child)==='[object Tagged]';
        e[Symbol.toStringTag]=7;
        var fallback=Object.prototype.toString.call(e)==='[object Error]';
        delete e[Symbol.toStringTag];delete TypeError.prototype[Symbol.toStringTag];
        inherited && fallback && Object.prototype.toString.call(e)==='[object Error]' &&
            Object.prototype.toString.call(child)==='[object Object]' && e instanceof TypeError && String(e)==='TypeError: m';
    "#);
}

#[test]
fn parser_uri_and_regexp_failures_use_intrinsic_error_families() {
    for (expression, family) in [
        ("eval('var =')", "SyntaxError"),
        ("Function('return )')", "SyntaxError"),
        ("RegExp('[')", "SyntaxError"),
        ("RegExp('x','gg')", "SyntaxError"),
        ("decodeURIComponent('%')", "URIError"),
        (r#"encodeURI('\uD800')"#, "URIError"),
    ] {
        yes(&format!(
            "var caught;try{{{expression};}}catch(e){{caught=e;}}caught instanceof {family} && caught instanceof Error && Object.getPrototypeOf(caught)==={family}.prototype && caught.constructor==={family} && caught.name==='{family}' && !caught.hasOwnProperty('name') && caught.hasOwnProperty('message') && typeof caught.message==='string' && caught.message.length>0 && String(caught).indexOf('{family}: ')===0 && Object.prototype.toString.call(caught)==='[object Error]';"
        ));
    }
}

#[test]
fn internal_errors_keep_intrinsic_linkage_after_globals_are_overwritten() {
    for (family, expression) in [
        ("SyntaxError", "eval('var =')"),
        ("URIError", "decodeURI('%')"),
    ] {
        yes(&format!(
            "var C={family},Base=Error;{family}=function Replacement(){{throw 'replacement ran';}};Error=function Other(){{throw 'base replacement ran';}};var caught;try{{{expression};}}catch(e){{caught=e;}}caught instanceof C && caught instanceof Base && caught.constructor===C && Object.getPrototypeOf(caught)===C.prototype && caught.name==='{family}';"
        ));
    }
}

#[test]
fn uncaught_genuine_errors_have_readable_diagnostics() {
    for family in FAMILIES {
        let error = Runtime::new()
            .execute(
                &format!("throw new {family}('authored message');"),
                &mut NoIo,
            )
            .unwrap_err();
        assert_eq!(
            error,
            format!("Uncaught JavaScript exception: {family}: authored message")
        );
        let error = Runtime::new()
            .execute(&format!("throw new {family}();"), &mut NoIo)
            .unwrap_err();
        assert_eq!(error, format!("Uncaught JavaScript exception: {family}"));
    }
    for (source, family) in [
        ("eval('var =');", "SyntaxError"),
        ("RegExp('[');", "SyntaxError"),
        ("decodeURI('%');", "URIError"),
    ] {
        let error = Runtime::new().execute(source, &mut NoIo).unwrap_err();
        assert!(
            error.starts_with(&format!("Uncaught JavaScript exception: {family}: ")),
            "{error}"
        );
    }
}

struct OrderedFields {
    name: Value,
    message: Value,
    fail: Option<&'static str>,
    trace: Vec<String>,
}

impl Host for OrderedFields {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        assert_eq!(object, "error-fields");
        self.trace.push(format!("get:{key}"));
        if self.fail == Some(key) {
            return Err(format!("TypeError: authored {key} read failure"));
        }
        match key {
            "name" => Ok(self.name.clone()),
            "message" => Ok(self.message.clone()),
            _ => panic!("unexpected error-field read: {key}"),
        }
    }

    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("unexpected host set: {object}.{key}");
    }

    fn call(&mut self, name: &str, _: Value, args: Vec<Value>) -> Result<Value, String> {
        assert_eq!(name, "host.trace");
        let [Value::String(units)] = args.as_slice() else {
            panic!("expected one original string trace argument");
        };
        self.trace.push(String::from_utf16(units).unwrap());
        Ok(Value::Undefined)
    }
}

fn ordered_fields() -> (Runtime, OrderedFields) {
    let mut runtime = Runtime::new();
    runtime.set_global("note", Value::Native("host.trace".into()));
    runtime.set_global("receiver", Value::Host("error-fields".into()));
    runtime
        .execute(
            r#"
                var nameValue={},messageValue={};
                nameValue[Symbol.toPrimitive]=function(hint){note('convert-name:'+hint);return 'Name';};
                messageValue[Symbol.toPrimitive]=function(hint){note('convert-message:'+hint);return 'Message';};
            "#,
            &mut NoIo,
        )
        .unwrap();
    let host = OrderedFields {
        name: runtime.get_global("nameValue"),
        message: runtime.get_global("messageValue"),
        fail: None,
        trace: Vec::new(),
    };
    (runtime, host)
}

#[test]
fn generic_host_receiver_reads_and_field_conversions_interleave_in_spec_order() {
    let (mut runtime, mut host) = ordered_fields();
    let output = runtime
        .execute("Error.prototype.toString.call(receiver);", &mut host)
        .unwrap();
    assert_eq!(output, Value::text("Name: Message"));
    assert_eq!(
        host.trace,
        [
            "get:name",
            "convert-name:string",
            "get:message",
            "convert-message:string"
        ]
    );
}

#[test]
fn host_read_and_symbol_conversion_failures_stop_before_later_operations() {
    for (field, trace) in [
        ("name", vec!["get:name"]),
        (
            "message",
            vec!["get:name", "convert-name:string", "get:message"],
        ),
    ] {
        for symbol_failure in [false, true] {
            let (mut runtime, mut host) = ordered_fields();
            if symbol_failure {
                let symbol = runtime.execute("Symbol('field');", &mut NoIo).unwrap();
                if field == "name" {
                    host.name = symbol;
                } else {
                    host.message = symbol;
                }
            } else {
                host.fail = Some(field);
            }
            let result = runtime
                .execute(
                    "var caught=false;try{Error.prototype.toString.call(receiver);}catch(e){caught=String(e).indexOf('TypeError')>=0;}caught;",
                    &mut host,
                )
                .unwrap();
            assert_eq!(result, Value::Bool(true));
            assert_eq!(
                host.trace, trace,
                "{field}, symbol failure={symbol_failure}"
            );
        }
    }
}
