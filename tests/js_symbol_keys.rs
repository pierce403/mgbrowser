//! Authored core Symbol/property-key cases. These do not assert support for
//! deferred well-known-symbol protocols such as iteration or custom RegExp use.
//! Semantics follow the core Symbol, ToPropertyKey, OrdinaryOwnPropertyKeys and
//! Object.getOwnPropertySymbols algorithms, not description-string shims.

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
    assert_eq!(
        Runtime::new().execute(source, &mut NoIo).unwrap(),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn same_description_symbols_are_distinct_property_keys() {
    yes(r#"
        var first = Symbol('same'), second = Symbol('same');
        var object = {same:'string'};
        object[first] = 'first';
        object[second] = 'second';
        typeof first === 'symbol' && first !== second &&
        object[first] === 'first' && object[second] === 'second' &&
        object.same === 'string';
        "#);
}

#[test]
fn symbol_descriptions_do_not_become_array_indices_or_length() {
    yes(r#"
        var zero = Symbol('0'), length = Symbol('length');
        var array = ['index'];
        array[zero] = 'symbol index';
        array[length] = 99;
        var before = array.length === 1 && array[0] === 'index' &&
            array[zero] === 'symbol index' && array[length] === 99 &&
            zero in array && length in array &&
            array.hasOwnProperty(zero) && array.hasOwnProperty(length);
        array.length = 0;
        var after = array.length === 0 && !(0 in array) &&
            array[zero] === 'symbol index' && array[length] === 99;
        var removed = delete array[zero];
        before && after && removed && !(zero in array) &&
            array[length] === 99 && array.length === 0;
        "#);
}

#[test]
fn symbols_named_like_builtins_are_ordinary_independent_keys() {
    yes(r#"
        var stringKey = Symbol('toString'), ownKey = Symbol('hasOwnProperty');
        var object = {};
        object[stringKey] = 7;
        object[ownKey] = 8;
        typeof object.toString === 'function' &&
        typeof object.hasOwnProperty === 'function' &&
        object[stringKey] === 7 && object[ownKey] === 8 &&
        object.hasOwnProperty(stringKey) && object.hasOwnProperty(ownKey);
        "#);
}

#[test]
fn symbol_lookup_shadow_assignment_and_deletion_follow_prototypes() {
    yes(r#"
        var key = Symbol('shared'), other = Symbol('shared');
        var parent = {};
        parent[key] = 10;
        var child = Object.create(parent);
        var inherited = child[key] === 10 && key in child &&
            !child.hasOwnProperty(key) && !(other in child);
        child[key] = 20;
        var shadowed = child[key] === 20 && parent[key] === 10 &&
            child.hasOwnProperty(key);
        var removed = delete child[key];
        var revealed = child[key] === 10 && key in child &&
            !child.hasOwnProperty(key);
        var inheritedDelete = delete child[key];
        var parentDelete = delete parent[key];
        inherited && shadowed && removed && revealed && inheritedDelete &&
            parentDelete && !(key in child) && child[key] === undefined;
        "#);
}

#[test]
fn string_enumeration_and_symbol_reflection_are_separate() {
    yes(r#"
        var inherited = Symbol('parent'), first = Symbol('plain');
        var second = Symbol('second');
        var parent = {parentText:1};
        parent[inherited] = 'hidden from string enumeration';
        var object = Object.create(parent);
        object.plain = 2;
        object[first] = 3;
        object.second = 4;
        object[second] = 5;
        var keys = Object.keys(object), names = Object.getOwnPropertyNames(object);
        var symbols = Object.getOwnPropertySymbols(object), seen = '';
        for (var key in object) seen += key + ',';
        keys.join(',') === 'plain,second' && names.join(',') === 'plain,second' &&
            seen === 'plain,second,parentText,' && symbols.length === 2 &&
            symbols[0] === first && symbols[1] === second;
        "#);
}

#[test]
fn own_symbol_creation_order_survives_updates_and_moves_after_delete_readd() {
    yes(r#"
        var a = Symbol('same'), b = Symbol('same'), c = Symbol('same');
        var object = {};
        object[b] = 2;
        object.text = 'string does not occupy a symbol position';
        object[a] = 1;
        object[c] = 3;
        object[a] = 11;
        var original = Object.getOwnPropertySymbols(object);
        delete object[b];
        var shortened = Object.getOwnPropertySymbols(object);
        object[b] = 22;
        var readded = Object.getOwnPropertySymbols(object);
        original.length === 3 && original[0] === b && original[1] === a &&
            original[2] === c && shortened.length === 2 &&
            shortened[0] === a && shortened[1] === c &&
            readded.length === 3 && readded[0] === a &&
            readded[1] === c && readded[2] === b && object[a] === 11;
        "#);
}

#[test]
fn own_symbol_reflection_excludes_inherited_keys_and_returns_fresh_arrays() {
    yes(r#"
        var key = Symbol('key'), parent = {};
        parent[key] = 1;
        var child = Object.create(parent);
        var inheritedOnly = Object.getOwnPropertySymbols(child);
        child[key] = 2;
        var first = Object.getOwnPropertySymbols(child);
        var second = Object.getOwnPropertySymbols(child);
        first[0] = 'edited result';
        inheritedOnly.length === 0 && first !== second &&
            second.length === 1 && second[0] === key && child[key] === 2 &&
            Object.getOwnPropertySymbols(parent)[0] === key;
        "#);
}

#[test]
fn user_and_native_functions_retain_symbol_properties_without_metadata_aliases() {
    yes(r#"
        var key = Symbol('length'), fn = function(value) {}, builtin = Number;
        fn[key] = 'user';
        builtin[key] = 'native';
        var before = fn.length === 1 && fn[key] === 'user' &&
            builtin[key] === 'native' && fn.hasOwnProperty(key) &&
            builtin.hasOwnProperty(key) &&
            Object.getOwnPropertySymbols(fn)[0] === key &&
            Object.getOwnPropertySymbols(builtin)[0] === key;
        delete fn[key];
        before && !(key in fn) && key in builtin && builtin[key] === 'native';
        "#);
}

#[test]
fn boxed_string_virtual_index_and_length_do_not_shadow_symbol_keys() {
    yes(r#"
        var zero = Symbol('0'), length = Symbol('length'), text = Object('ab');
        text[zero] = 'symbol zero';
        text[length] = 7;
        var child = Object.create(text);
        child[zero] = 'child';
        var own = Object.getOwnPropertySymbols(text);
        text[0] === 'a' && text.length === 2 &&
            text[zero] === 'symbol zero' && text[length] === 7 &&
            child[zero] === 'child' && child[length] === 7 &&
            own.length === 2 && own[0] === zero && own[1] === length &&
            Object.keys(text).join(',') === '0,1';
        "#);
}

#[test]
fn symbol_references_evaluate_once_in_assignment_update_and_delete_order() {
    yes(r#"
        var symbol = Symbol('key'), object = {}, log = '';
        function key() { log += 'k'; return symbol; }
        function value() { log += 'v'; return 3; }
        object[key()] = value();
        object[key()] += value();
        var before = object[symbol];
        var previous = object[key()]++;
        var after = object[symbol];
        var removed = delete object[key()];
        log === 'kvkvkk' && before === 6 && previous === 6 && after === 7 &&
            removed && !object.hasOwnProperty(symbol);
        "#);
}

#[test]
fn boxed_symbol_property_keys_preserve_primitive_identity() {
    yes(r#"
        var symbol = Symbol('same'), wrapper = Object(symbol), object = {};
        object[wrapper] = 7;
        var found = object[symbol] === 7 && symbol in object &&
            wrapper in object && object.hasOwnProperty(wrapper) &&
            Object.getOwnPropertySymbols(object)[0] === symbol;
        var removed = delete object[wrapper];
        found && removed && !(symbol in object);
        "#);
}

#[test]
fn descriptions_with_nul_and_lone_surrogates_remain_only_descriptions() {
    yes(r#"
        var first = Symbol('\uD800\u0000same'), second = Symbol('\uD800\u0000same');
        var object = {};
        object[first] = 1;
        object[second] = 2;
        var symbols = Object.getOwnPropertySymbols(object);
        first !== second && symbols.length === 2 &&
            symbols[0] === first && symbols[1] === second &&
            object[first] === 1 && object[second] === 2;
        "#);
}

#[test]
fn symbol_identity_survives_closures_and_later_eval_and_function_calls() {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime
            .execute(
                r#"
                var key = Symbol('retained'), object = {};
                object[key] = 7;
                function capture(symbol) {
                    return function(target) { return target[symbol]; };
                }
                var reader = capture(key);
                reader(object) === 7;
                "#,
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .execute(
                r#"
                var alias = eval('key');
                eval('object[alias] = 8;');
                var dynamic = Function('symbol', 'target', 'return target[symbol];');
                var globalReader = Function('return object[key];');
                alias === key && reader(object) === 8 &&
                    dynamic(key, object) === 8 && globalReader() === 8 &&
                    Object.getOwnPropertySymbols(object)[0] === key;
                "#,
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn independent_dynamic_creations_with_equal_descriptions_never_merge() {
    yes(r#"
        var compile = Function('return Symbol("dynamic");');
        var first = compile(), second = compile(), third = eval('Symbol("dynamic")');
        var object = {};
        object[first] = 1;
        object[second] = 2;
        object[third] = 3;
        var symbols = Object.getOwnPropertySymbols(object);
        first !== second && first !== third && second !== third &&
            symbols.length === 3 && symbols[0] === first &&
            symbols[1] === second && symbols[2] === third &&
            object[first] + object[second] + object[third] === 6;
        "#);
}
