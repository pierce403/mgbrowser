//! Authored local DOM/JavaScript bridge regressions. These execute no public
//! page content and establish no browser isolation or Google compatibility claim.

use mg_deps::{
    document::{self, Document, Item},
    js_browser::{MAX_SOURCE, Reply, Request, execute},
};

const URL: &str = "https://example.test/docs/page.html?q=fixture#part";

fn page(head: &str, body: &str, script: &str) -> Reply {
    execute(Request {
        url: URL.into(),
        html: format!(
            "<!doctype html><html><head><title>Initial title</title>{head}</head><body><p id=sentinel>Readable local fixture</p><p id=output>Unchanged output</p>{body}<script>{script}</script></body></html>"
        ),
    })
}

fn rendered(reply: &Reply) -> Document {
    document::parse_with_scripting(&reply.html, URL, true)
}

fn content(document: &Document, selector: &str) -> String {
    let root = document.query_selector(0, selector).unwrap().unwrap();
    let mut pending = vec![root];
    let mut result = String::new();
    while let Some(id) = pending.pop() {
        let node = &document.nodes[id];
        if node.tag == "#text" {
            result.push_str(&node.text);
        } else {
            pending.extend(node.children.iter().rev().copied());
        }
    }
    result
}

fn readable(document: &Document) {
    assert_eq!(content(document, "#sentinel"), "Readable local fixture");
    assert!(document.items.iter().any(|item| {
        matches!(item, Item::Text { text, .. } if text.contains("Readable local fixture"))
    }));
}

#[test]
fn object_create_descriptors_build_the_frozen_real_form() {
    let html = include_str!("fixtures/script/object-create.html");
    assert!(
        document::parse_with_scripting(html, URL, true)
            .forms
            .is_empty()
    );
    let reply = execute(Request {
        url: URL.into(),
        html: html.into(),
    });
    assert!(
        reply.applied && reply.errors.is_empty(),
        "{:?}",
        reply.errors
    );
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    let report = reply.allocations.unwrap();
    assert!(report.is_valid() && report.first_rejected.is_none());
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    let doc = rendered(&reply);
    assert_eq!(doc.title, "Object.create descriptor local fixture");
    assert_eq!(doc.forms.len(), 1);
    assert_eq!(doc.forms[0].action, "https://example.test/search");
    assert_eq!(
        content(&doc, "#status"),
        "Object.create descriptor form ready"
    );
    for (name, value) in [("q", ""), ("source", "fixture")] {
        let node = doc
            .nodes
            .iter()
            .find(|node| node.tag == "input" && node.attr("name") == Some(name))
            .unwrap();
        assert_eq!(node.attr("value").unwrap_or(""), value);
    }
}

#[test]
fn static_operator_storage_allows_the_frozen_real_form() {
    let html = include_str!("fixtures/script/static-operators.html");
    assert!(
        mg_deps::document::parse_with_scripting(html, URL, true)
            .forms
            .is_empty()
    );
    let reply = execute(Request {
        url: URL.into(),
        html: html.into(),
    });
    assert!(
        reply.applied && reply.errors.is_empty(),
        "{:?}",
        reply.errors
    );
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    let report = reply.allocations.unwrap();
    assert!(report.is_valid() && report.first_rejected.is_none());
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    let doc = rendered(&reply);
    assert_eq!(doc.title, "Static operators local fixture");
    assert_eq!(doc.forms.len(), 1);
    assert_eq!(doc.forms[0].action, "https://example.test/search");
    assert_eq!(content(&doc, "#status"), "Static operators form ready");
    assert!(
        doc.items
            .iter()
            .any(|item| matches!(item, Item::Input { name, .. } if name == "q"))
    );
}

#[test]
fn core_intrinsics_create_the_frozen_real_form() {
    let reply = execute(Request {
        url: URL.into(),
        html: include_str!("fixtures/script/core-intrinsics.html").into(),
    });
    assert!(
        reply.applied && reply.errors.is_empty(),
        "{:?}",
        reply.errors
    );
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    let doc = rendered(&reply);
    assert_eq!(doc.title, "Core intrinsic local fixture");
    assert_eq!(doc.forms.len(), 1);
    assert_eq!(doc.forms[0].action, "https://example.test/search");
    assert!(
        doc.items
            .iter()
            .any(|item| matches!(item, Item::Input { name, .. } if name == "q"))
    );
    assert!(content(&doc, "#status").contains("Recovered constructors and primitive prototypes"));
}

#[test]
fn numeric_constructor_conversion_error_preserves_dom_effect_and_later_recovery() {
    let reply = execute(Request {
        url: URL.into(),
        html: "<body><p id='state'>initial</p><script>var marker={};try{new Number({valueOf:function(){document.getElementById('state').textContent='before';throw marker;}});}catch(e){if(e!==marker)throw 'wrong error';document.getElementById('state').textContent+=' caught';}</script><script>document.getElementById('state').textContent+=' recovered '+new Boolean().valueOf();</script></body>".into(),
    });
    assert!(
        reply.applied && reply.errors.is_empty(),
        "{:?}",
        reply.errors
    );
    assert_eq!(reply.scripts_executed, 2);
    assert_eq!(
        content(&rendered(&reply), "#state"),
        "before caught recovered false"
    );
    assert!(reply.navigation.is_none());
}

#[test]
fn callback_family_and_borrowed_collections_create_the_frozen_real_form() {
    let reply = execute(Request {
        url: URL.into(),
        html: include_str!("fixtures/script/array-callbacks.html").into(),
    });
    assert!(
        reply.applied && reply.errors.is_empty(),
        "{:?}",
        reply.errors
    );
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    let doc = rendered(&reply);
    assert_eq!(doc.title, "Array callback local fixture");
    assert_eq!(doc.forms.len(), 1);
    assert_eq!(doc.forms[0].action, "https://example.test/search");
    assert!(
        doc.items
            .iter()
            .any(|item| matches!(item, Item::Input{name,..} if name=="q"))
    );
    assert!(reply.html.contains("Verified q") && reply.html.contains("Verified source"));
}

#[test]
fn borrowed_callbacks_preserve_collection_snapshots_and_detached_node_identity() {
    let reply = page(
        "",
        "<div id=items><span class=entry>A</span><span class=entry>B</span></div>",
        r#"
        var entries=document.querySelectorAll('.entry'),seen='',original=entries[1];
        Array.prototype.forEach.call(entries,function(node,i,object){
            if(object!==entries)throw 'changed collection identity';
            seen+=node.textContent;
            if(i===0){
                document.getElementById('items').removeChild(original);
                var added=document.createElement('span');added.className='entry';
                added.textContent='C';document.getElementById('items').appendChild(added);
            }
        });
        var kept=Array.prototype.filter.call(entries,function(node){return node===original;});
        var fresh=document.querySelectorAll('.entry');
        document.getElementById('output').textContent=seen+'|'+entries.length+'|'+fresh.length+'|'+
            kept[0].textContent+'|'+Array.prototype.map.call(fresh,function(node){return node.textContent;}).join('');
    "#,
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(content(&rendered(&reply), "#output"), "AB|2|2|B|AC");
    readable(&rendered(&reply));
}

#[test]
fn collection_callback_error_preserves_earlier_effects_and_allows_a_later_script() {
    let reply = execute(Request {
        url: URL.into(),
        html: "<body><p id=output>Old</p><span>A</span><span>B</span><script>var nodes=document.querySelectorAll('span');Array.prototype.forEach.call(nodes,function(node,i){document.getElementById('output').textContent=node.textContent;if(i===0)throw 'local callback stop';});document.title='must not run';</script><script>document.title='Later script ready';</script></body>".into(),
    });
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(
        reply.errors,
        ["Inline script 1: Uncaught JavaScript exception: local callback stop"]
    );
    assert_eq!(content(&rendered(&reply), "#output"), "A");
    assert_eq!(rendered(&reply).title, "Later script ready");
}

#[test]
fn lookup_and_title_use_connected_nodes_only() {
    let reply = page(
        "",
        "",
        r#"
        var output = document.getElementById('output');
        var node = document.createElement('div');
        node.id = 'transient';
        node.textContent = 'Detached content';
        var states = '' + (document.getElementById('transient') === null);
        document.body.appendChild(node);
        states += ',' + (document.getElementById('transient') === node);
        document.body.removeChild(node);
        states += ',' + (document.getElementById('transient') === null);
        var original = document.head.querySelector('title');
        document.head.removeChild(original);
        states += ',' + (document.title === '');
        original.textContent = 'Detached title';
        document.title = 'Connected title';
        output.textContent = states + ',' + document.title + ',' + original.textContent;
    "#,
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    let document = rendered(&reply);
    assert_eq!(document.title, "Connected title");
    assert_eq!(
        content(&document, "#output"),
        "true,true,true,true,Connected title,Detached title"
    );
    assert_eq!(document.query_selector(0, "#transient").unwrap(), None);
    readable(&document);
}

#[test]
fn url_getters_resolve_against_connected_base_without_changing_document_url() {
    let reply = page(
        "<base href='https://assets.example.test/pkg/'>",
        "<a id=link href='../guide?q=x#section'>Guide</a><img id=picture src='img/icon.png'><form id=form action='../submit'></form>",
        r#"
            document.getElementById('output').textContent = document.baseURI + '|' +
                document.getElementById('link').href + '|' +
                document.getElementById('picture').src + '|' +
                document.getElementById('form').action + '|' + document.URL + '|' + location.href;
        "#,
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(
        content(&rendered(&reply), "#output"),
        format!(
            "https://assets.example.test/pkg/|https://assets.example.test/guide?q=x#section|https://assets.example.test/pkg/img/icon.png|https://assets.example.test/submit|{URL}|{URL}"
        )
    );
    assert!(reply.navigation.is_none());
}

#[test]
fn detached_base_does_not_change_resolution() {
    let reply = page(
        "<base id=original-base href='https://assets.example.test/pkg/'>",
        "<a id=link href='next'>Next</a>",
        r#"
            var original = document.getElementById('original-base');
            document.head.removeChild(original);
            original.href = 'https://detached.example.test/';
            var replacement = document.createElement('base');
            replacement.href = '/replacement/';
            var before = document.getElementById('link').href;
            document.head.appendChild(replacement);
            document.getElementById('output').textContent = before + '|' + document.getElementById('link').href;
        "#,
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(
        content(&rendered(&reply), "#output"),
        "https://example.test/docs/next|https://example.test/replacement/next"
    );
}

#[test]
fn rejected_mutations_preserve_existing_children_and_visible_content() {
    let cases = [
        (
            "<input id=target value=kept>",
            "document.getElementById('target').innerHTML = '<b>Replacement</b>'",
            "",
        ),
        (
            "<input id=target value=kept>",
            "document.getElementById('target').textContent = 'Replacement'",
            "",
        ),
        (
            "<script id=target type=application/json>{\"kept\":true}</script>",
            "document.getElementById('target').innerHTML = 'Replacement'",
            "{\"kept\":true}",
        ),
        (
            "<script id=target type=application/json>{\"kept\":true}</script>",
            "document.getElementById('target').textContent = 'Replacement'",
            "{\"kept\":true}",
        ),
        (
            "<script id=target type=application/json>{\"kept\":true}</script>",
            "document.getElementById('target').firstChild.textContent = 'Replacement'",
            "{\"kept\":true}",
        ),
        (
            "<div id=target>Kept text</div>",
            "document.getElementById('target').firstChild.innerHTML = 'Replacement'",
            "Kept text",
        ),
    ];
    for (body, mutation, expected) in cases {
        let reply = page("", body, mutation);
        assert!(
            !reply.errors.is_empty(),
            "mutation unexpectedly succeeded: {mutation}"
        );
        assert_eq!(reply.scripts_executed, 0, "{mutation}");
        let document = rendered(&reply);
        assert_eq!(content(&document, "#target"), expected, "{mutation}");
        assert_eq!(content(&document, "#output"), "Unchanged output");
        readable(&document);
    }
}

#[test]
fn ordinary_text_nodes_can_update_without_replacing_their_parent() {
    let reply = page(
        "",
        "<p id=target>Original</p>",
        "document.getElementById('target').firstChild.textContent = 'Updated & readable';",
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    let document = rendered(&reply);
    assert_eq!(content(&document, "#target"), "Updated & readable");
    assert_eq!(document.query_selector_all(0, "#target").unwrap().len(), 1);
    readable(&document);
}

#[test]
fn cyclic_appends_fail_before_detaching_existing_nodes() {
    for script in [
        "var node = document.getElementById('target'); node.appendChild(node);",
        "document.getElementById('inside').appendChild(document.getElementById('target'));",
    ] {
        let reply = page(
            "",
            "<div id=target><span id=inside>Kept child</span></div>",
            script,
        );
        assert!(
            reply
                .errors
                .iter()
                .any(|error| error.to_ascii_lowercase().contains("cycle")),
            "{:?}",
            reply.errors
        );
        let document = rendered(&reply);
        let target = document.query_selector(0, "#target").unwrap().unwrap();
        let inside = document.query_selector(0, "#inside").unwrap().unwrap();
        assert_eq!(document.nodes[inside].parent, target);
        assert_eq!(content(&document, "#target"), "Kept child");
        readable(&document);
    }
}

#[test]
fn excessive_detached_subtree_depth_fails_without_attaching_it() {
    let reply = page(
        "",
        "<div id=target>Kept target</div>",
        r#"
        var root = document.createElement('div');
        var parent = root;
        for (var i = 0; i < 253; i++) {
            var child = document.createElement('div');
            parent.appendChild(child);
            parent = child;
        }
        document.getElementById('target').appendChild(root);
    "#,
    );
    assert!(
        reply
            .errors
            .iter()
            .any(|error| error.to_ascii_lowercase().contains("depth")),
        "{:?}",
        reply.errors
    );
    let document = rendered(&reply);
    let target = document.query_selector(0, "#target").unwrap().unwrap();
    assert_eq!(
        document.query_selector_all(target, "div").unwrap(),
        Vec::<usize>::new()
    );
    assert_eq!(content(&document, "#target"), "Kept target");
    readable(&document);
}

#[test]
fn inner_html_checks_combined_depth_before_replacing_children() {
    let body = format!(
        "{}<div id=target>Kept nested target</div>{}",
        "<div>".repeat(20),
        "</div>".repeat(20)
    );
    let fragment = format!("{}Replacement{}", "<div>".repeat(240), "</div>".repeat(240));
    let reply = page(
        "",
        &body,
        &format!("document.getElementById('target').innerHTML = '{fragment}';"),
    );
    assert!(
        reply
            .errors
            .iter()
            .any(|error| error.to_ascii_lowercase().contains("depth")),
        "{:?}",
        reply.errors
    );
    let document = rendered(&reply);
    let target = document.query_selector(0, "#target").unwrap().unwrap();
    assert!(
        document
            .query_selector_all(target, "div")
            .unwrap()
            .is_empty()
    );
    assert_eq!(content(&document, "#target"), "Kept nested target");
    readable(&document);
}

#[test]
fn source_limit_preserves_the_input_and_never_claims_execution() {
    let source = format!(
        "<p>Readable source limit fixture</p>{}",
        " ".repeat(MAX_SOURCE)
    );
    let reply = execute(Request {
        url: URL.into(),
        html: source.clone(),
    });
    assert_eq!(reply.html, source);
    assert_eq!(reply.scripts_executed, 0);
    assert!(reply.navigation.is_none());
    assert!(
        reply
            .errors
            .iter()
            .any(|error| error.contains("source/URL limit"))
    );
    assert!(document::parse(&reply.html, URL).items.iter().any(|item| matches!(item, Item::Text { text, .. } if text.contains("Readable source limit fixture"))));
}

#[test]
fn external_scripts_count_toward_the_limit_and_are_never_counted_as_executed() {
    let external: String = (0..33)
        .map(|i| format!("<script src='/local-{i}.js'></script>"))
        .collect();
    let reply = page(
        "",
        &external,
        "document.getElementById('output').textContent = 'Should not run';",
    );
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(
        reply
            .errors
            .iter()
            .filter(|error| error.contains("External script loading"))
            .count(),
        32
    );
    assert!(
        reply
            .errors
            .iter()
            .any(|error| error.contains("Script count limit"))
    );
    let document = rendered(&reply);
    assert_eq!(content(&document, "#output"), "Unchanged output");
    readable(&document);
}

#[test]
fn unsupported_external_sources_remain_errors_when_an_inline_script_succeeds() {
    let reply = page(
        "",
        "<script src='/one.js'></script><script src='/two.js'></script>",
        "document.getElementById('output').textContent = 'Inline ran';",
    );
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(reply.errors.len(), 2);
    assert!(
        reply
            .errors
            .iter()
            .all(|error| error.contains("External script loading"))
    );
    assert_eq!(content(&rendered(&reply), "#output"), "Inline ran");
}

#[test]
fn scripts_and_load_callbacks_preserve_order_and_ready_state() {
    let reply = page(
        "",
        r#"
        <script>
            var output = document.getElementById('output');
            var trail = 'first:' + document.readyState;
            document.addEventListener('DOMContentLoaded', function(event) {
                trail += '|dom:' + document.readyState + ':' + event.type;
                output.textContent = trail;
            });
            window.addEventListener('load', function() {
                trail += '|load-first:' + document.readyState;
                output.textContent = trail;
            });
        </script>
        <script>
            trail += '|second';
            document.addEventListener('DOMContentLoaded', function() {
                trail += '|dom-second';
                output.textContent = trail;
            });
            addEventListener('load', function() {
                trail += '|load-second';
                output.textContent = trail;
            });
        </script>
    "#,
        r#"
        trail += '|third';
        window.onload = function() {
            trail += '|onload';
            output.textContent = trail;
        };
    "#,
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 3);
    assert_eq!(
        content(&rendered(&reply), "#output"),
        "first:loading|second|third|dom:interactive:DOMContentLoaded|dom-second|load-first:complete|load-second|onload"
    );
}

#[test]
fn listener_receiver_normalization_preserves_bare_registration_and_removal() {
    let reply = page(
        "",
        "",
        r#"
        var add=addEventListener,remove=removeEventListener,trail='',rejected=0;
        function removed(){trail+='wrong';}
        add('load',removed);remove('load',removed);
        add('load',function(){trail+='B';});
        window.addEventListener.call(undefined,'load',function(){trail+='U';});
        window.addEventListener.call(null,'load',function(){trail+='N';});
        var domAdd=document.addEventListener;
        domAdd('load',function(){trail+='D';});
        try{add.call({},'load',removed);}catch(e){rejected++;}
        try{domAdd.call(3,'load',removed);}catch(e){rejected++;}
        window.addEventListener('load',function(){
            document.getElementById('output').textContent=trail+':'+rejected;
        });
    "#,
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(content(&rendered(&reply), "#output"), "BUND:2");
    assert!(reply.navigation.is_none());
    assert!(reply.allocations.unwrap().first_rejected.is_none());
}

#[test]
fn symbol_dom_setters_reject_before_mutation_or_navigation() {
    for value in ["Symbol('value')", "Object(Symbol('value'))"] {
        for assignment in [
            "document.title",
            "document.getElementById('output').textContent",
            "document.getElementById('output').innerText",
            "document.getElementById('output').innerHTML",
            "document.getElementById('target').value",
            "document.getElementById('target').name",
            "document.getElementById('link').href",
            "document.getElementById('output').style.cssText",
            "document.getElementById('output').style.color",
            "location.href",
            "document.location",
            "window.location",
            "location",
        ] {
            let reply = page(
                "",
                "<input id=target name=kept value=original><a id=link href='/kept'>Kept link</a>",
                &format!("{assignment} = {value};document.title='Incorrect later effect';"),
            );
            assert_eq!(
                reply.errors.len(),
                1,
                "{assignment} = {value}: {:?}",
                reply.errors
            );
            assert!(
                reply.errors[0].contains("TypeError"),
                "{assignment} = {value}: {:?}",
                reply.errors
            );
            assert!(reply.navigation.is_none(), "{assignment} = {value}");
            assert_eq!(reply.scripts_executed, 0);
            assert!(reply.allocations.unwrap().first_rejected.is_none());
            let doc = rendered(&reply);
            assert_eq!(doc.title, "Initial title");
            assert_eq!(content(&doc, "#output"), "Unchanged output");
            let target = doc.query_selector(0, "#target").unwrap().unwrap();
            assert_eq!(doc.nodes[target].attr("value"), Some("original"));
            assert_eq!(doc.nodes[target].attr("name"), Some("kept"));
            let link = doc.query_selector(0, "#link").unwrap().unwrap();
            assert_eq!(doc.nodes[link].attr("href"), Some("/kept"));
            let output = doc.query_selector(0, "#output").unwrap().unwrap();
            assert_eq!(doc.nodes[output].attr("style"), None);
            readable(&doc);
        }
    }
}

#[test]
fn symbol_dom_string_arguments_reject_without_consuming_diagnostic_text() {
    for value in ["Symbol('output')", "Object(Symbol('output'))"] {
        for call in [
            "document.getElementById(V)",
            "document.querySelector(V)",
            "document.querySelectorAll(V)",
            "document.getElementsByTagName(V)",
            "document.createElement(V)",
            "document.createTextNode(V)",
            "document.body.getAttribute(V)",
            "document.body.removeAttribute(V)",
            "document.body.setAttribute(V, 'value')",
            "document.body.setAttribute('data-test', V)",
            "document.addEventListener(V, function(){})",
            "window.addEventListener(V, function(){})",
            "document.querySelectorAll('p').item(V)",
            "location.assign(V)",
            "location.replace(V)",
        ] {
            let call = call.replace('V', value);
            let reply = page(
                "",
                "",
                &format!("{call};document.title='Incorrect effect';"),
            );
            assert_eq!(reply.errors.len(), 1, "{call}: {:?}", reply.errors);
            assert!(
                reply.errors[0].contains("TypeError"),
                "{call}: {:?}",
                reply.errors
            );
            assert!(reply.navigation.is_none(), "{call}");
            let doc = rendered(&reply);
            assert_eq!(doc.title, "Initial title");
            assert_eq!(content(&doc, "#output"), "Unchanged output");
            let body = doc.query_selector(0, "body").unwrap().unwrap();
            assert_eq!(doc.nodes[body].attr("data-test"), None);
            readable(&doc);
        }
    }
}

#[test]
fn dom_coercion_hooks_receive_string_hint_and_preserve_argument_order() {
    let reply = page(
        "",
        "",
        r#"
        var trail='';
        function text(value, marker) {
            var object={};
            object[Symbol.toPrimitive]=function(hint) {
                if(this!==object || hint!=='string') throw 'wrong conversion';
                trail+=marker;return value;
            };
            return object;
        }
        var node=document.createElement(text('p','element;'));
        node.id=text('created','id;');
        node.textContent=text('Rust & café','text;');
        node.setAttribute(text('data-test','name;'),text('retained','value;'));
        document.body.appendChild(node);
        document.getElementById('output').textContent=trail;
    "#,
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    let doc = rendered(&reply);
    assert_eq!(content(&doc, "#output"), "element;id;text;name;value;");
    assert_eq!(content(&doc, "#created"), "Rust & café");
    let created = doc.query_selector(0, "#created").unwrap().unwrap();
    assert_eq!(doc.nodes[created].attr("data-test"), Some("retained"));
    readable(&doc);
}

#[test]
fn failed_second_dom_conversion_keeps_attribute_but_not_prior_hook_side_effects() {
    let reply = page(
        "",
        "<p id=target data-test=kept>Kept child</p>",
        r#"
        var trail='';var key={};var value={};
        key[Symbol.toPrimitive]=function(hint){trail+='key:'+hint+';';return 'data-test';};
        value[Symbol.toPrimitive]=function(hint){trail+='value:'+hint+';';return Symbol('no text');};
        try{document.getElementById('target').setAttribute(key,value);}
        catch(error){trail+='caught';}
        document.getElementById('output').textContent=trail;
    "#,
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    let doc = rendered(&reply);
    assert_eq!(content(&doc, "#output"), "key:string;value:string;caught");
    let target = doc.query_selector(0, "#target").unwrap().unwrap();
    assert_eq!(doc.nodes[target].attr("data-test"), Some("kept"));
    assert_eq!(content(&doc, "#target"), "Kept child");
}

#[test]
fn failed_first_dom_conversion_does_not_run_the_second_argument_hook() {
    for failure in ["return Symbol('not a name');", "throw 42;"] {
        let reply = page(
            "",
            "<p id=target data-test=kept>Kept child</p>",
            &format!(
                r#"
            var trail='';var key={{}};var value={{}};
            key[Symbol.toPrimitive]=function(hint){{trail+='key:'+hint+';';{failure}}};
            value[Symbol.toPrimitive]=function(hint){{trail+='incorrect value;';return 'changed';}};
            try{{document.getElementById('target').setAttribute(key,value);}}
            catch(error){{trail+='caught';}}
            document.getElementById('output').textContent=trail;
        "#
            ),
        );
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        let doc = rendered(&reply);
        assert_eq!(content(&doc, "#output"), "key:string;caught");
        let target = doc.query_selector(0, "#target").unwrap().unwrap();
        assert_eq!(doc.nodes[target].attr("data-test"), Some("kept"));
        assert_eq!(content(&doc, "#target"), "Kept child");
    }
}

#[test]
fn dom_explicit_symbol_string_is_allowed_but_unused_arguments_are_not_coerced() {
    let reply = page(
        "",
        "",
        r#"
        var unused={};unused[Symbol.toPrimitive]=function(){throw 'unused conversion';};
        var token=Symbol('explicit');
        console.log(token,unused);
        var child=document.createTextNode(String(token),unused);
        var parent=document.getElementById('output');
        parent.textContent='';parent.appendChild(child,unused);
        document.addEventListener('DOMContentLoaded',function(){document.title='Listener ran';},unused);
    "#,
    );
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    let doc = rendered(&reply);
    assert_eq!(doc.title, "Listener ran");
    assert_eq!(content(&doc, "#output"), "Symbol(explicit)");
    readable(&doc);
}

#[test]
fn symbol_host_keys_fail_explicitly_instead_of_aliasing_string_properties() {
    for access in [
        "document[token]",
        "document[token]='Incorrect'",
        "delete document[token]",
        "token in document",
    ] {
        let reply = page(
            "",
            "",
            &format!("var token=Symbol('title');{access};document.title='Incorrect later effect';"),
        );
        assert_eq!(reply.errors.len(), 1, "{access}: {:?}", reply.errors);
        assert!(
            reply.errors[0].to_ascii_lowercase().contains("symbol"),
            "{access}: {:?}",
            reply.errors
        );
        assert_eq!(rendered(&reply).title, "Initial title");
        assert!(reply.navigation.is_none());
        assert!(reply.allocations.unwrap().first_rejected.is_none());
    }
}

#[test]
fn dom_conversion_uses_original_receiver_through_function_and_native_prototypes() {
    for prototype in ["User", "Array"] {
        let reply = page(
            "",
            "",
            &format!(
                r#"
            function User(){{}}
            var parent={prototype};
            var value=Object.create(parent);value.text='Inherited Rust & café';
            parent[Symbol.toPrimitive]=function(hint){{
                if(this!==value || hint!=='string')throw 'Wrong inherited receiver';
                return this.text;
            }};
            document.getElementById('output').textContent=value;
            value.text='Inherited title';document.title=value;
            value.text='/prototype-destination';location.href=value;
        "#
            ),
        );
        assert!(reply.errors.is_empty(), "{prototype}: {:?}", reply.errors);
        assert_eq!(reply.scripts_executed, 1);
        assert_eq!(
            reply.navigation.as_deref(),
            Some("https://example.test/prototype-destination")
        );
        assert!(reply.allocations.unwrap().first_rejected.is_none());
        let doc = rendered(&reply);
        assert_eq!(doc.title, "Inherited title");
        assert_eq!(content(&doc, "#output"), "Inherited Rust & café");
        readable(&doc);
    }
}

#[test]
fn inherited_function_prototype_hook_failure_keeps_dom_target_unchanged() {
    for prototype in ["User", "Array"] {
        let reply = page(
            "",
            "",
            &format!(
                r#"
            function User(){{}}
            var parent={prototype};var value=Object.create(parent);
            parent[Symbol.toPrimitive]=function(hint){{
                if(this!==value || hint!=='string')throw 'Wrong inherited receiver';
                document.title='Hook ran';return Symbol('not DOM text');
            }};
            try{{document.getElementById('output').textContent=value;location.href='/incorrect';}}
            catch(error){{if(String(error)!=='TypeError: cannot convert Symbol to string')throw error;}}
        "#
            ),
        );
        assert!(reply.errors.is_empty(), "{prototype}: {:?}", reply.errors);
        assert!(reply.navigation.is_none());
        assert!(reply.allocations.unwrap().first_rejected.is_none());
        let doc = rendered(&reply);
        assert_eq!(doc.title, "Hook ran");
        assert_eq!(content(&doc, "#output"), "Unchanged output");
        readable(&doc);
    }
}

#[test]
fn error_family_string_conversion_drives_real_dom_text_title_and_navigation() {
    for family in [
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "URIError",
    ] {
        let reply = page(
            "",
            "",
            &format!(
                r#"
                var error=new {family}('Rust & café');
                if(!(error instanceof Error) || error.hasOwnProperty('name'))throw 'Wrong Error instance';
                document.getElementById('output').textContent=error;
                error.message='DOM title';document.title=error;
                error.name='';error.message='/error-destination';location.href=error;
            "#
            ),
        );
        assert!(reply.applied);
        assert!(reply.errors.is_empty(), "{family}: {:?}", reply.errors);
        assert_eq!(reply.scripts_executed, 1);
        assert_eq!(
            reply.navigation.as_deref(),
            Some("https://example.test/error-destination")
        );
        assert!(reply.allocations.unwrap().first_rejected.is_none());
        let doc = rendered(&reply);
        assert_eq!(doc.title, format!("{family}: DOM title"));
        assert_eq!(content(&doc, "#output"), format!("{family}: Rust & café"));
        readable(&doc);
    }
}

#[test]
fn error_message_symbol_conversion_preserves_dom_target_and_prior_hook_effects() {
    let reply = page(
        "",
        "",
        r#"
        var error=TypeError();var message={};
        message[Symbol.toPrimitive]=function(hint){
            if(this!==message || hint!=='string')throw 'Wrong message conversion';
            document.title='Error message hook ran';return Symbol('not DOM text');
        };
        error.message=message;
        try{document.getElementById('output').textContent=error;location.href='/incorrect';}
        catch(caught){if(String(caught)!=='TypeError: cannot convert Symbol to string')throw caught;}
    "#,
    );
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let doc = rendered(&reply);
    assert_eq!(doc.title, "Error message hook ran");
    assert_eq!(content(&doc, "#output"), "Unchanged output");
    readable(&doc);
}

#[test]
fn concat_preserves_host_node_identity_and_drives_dom_text_title_navigation() {
    let reply = page(
        "",
        "",
        r#"
        var node=document.getElementById('output');
        var receiver=Array.prototype.concat.call(node,['Rust & café']);
        if(receiver.length!==2 || receiver[0]!==node)throw 'Host concat identity failed';
        receiver[0].textContent=receiver[1];
        var parts=['Concat'].concat(['DOM','ready']);document.title=parts.join(' ');
        var destination=['/concat'].concat(['-destination']);location.href=destination.join('');
    "#,
    );
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(
        reply.navigation.as_deref(),
        Some("https://example.test/concat-destination")
    );
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let doc = rendered(&reply);
    assert_eq!(doc.title, "Concat DOM ready");
    assert_eq!(content(&doc, "#output"), "Rust & café");
    readable(&doc);
}

#[test]
fn empty_snapshot_read_through_eval_preserves_identity_and_drives_dom() {
    let reply = page(
        "",
        "",
        r#"
        function build(){
            var a=eval('arguments');
            if(a.length!==0 || a.callee!==build || a!==arguments)throw 'Snapshot identity failed';
            document.getElementById('output').textContent='Rust & café';
            document.title='Empty arguments DOM ready';
            location.href='/empty-arguments-destination';
        }
        build();
    "#,
    );
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(
        reply.navigation.as_deref(),
        Some("https://example.test/empty-arguments-destination")
    );
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let document = rendered(&reply);
    assert_eq!(document.title, "Empty arguments DOM ready");
    assert_eq!(content(&document, "#output"), "Rust & café");
    readable(&document);
}

#[test]
fn replaced_empty_arguments_preserve_symbol_rejection_at_dom_boundary() {
    let reply = page(
        "",
        "",
        r#"
        function replace(){
            arguments=Symbol('replacement');
            try{document.getElementById('output').textContent=arguments;location.href='/incorrect';}
            catch(error){if(String(error)!=='TypeError: cannot convert Symbol to string')throw error;}
            document.title='Replaced arguments stay values';
        }
        replace();
    "#,
    );
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let document = rendered(&reply);
    assert_eq!(document.title, "Replaced arguments stay values");
    assert_eq!(content(&document, "#output"), "Unchanged output");
    readable(&document);
}

#[test]
fn inherited_function_default_and_constructed_instance_drive_actual_dom() {
    let reply = page(
        "",
        "",
        r#"
        function Message(){this.text='Rust & café';}
        var child=Object.create(Message);
        var prototype=child.prototype;
        var value=new Message();
        if(prototype.constructor!==Message || Object.getPrototypeOf(value)!==prototype || !(value instanceof Message))throw 'Prototype identity failed';
        document.getElementById('output').textContent=value.text;
        document.title='Function prototype DOM ready';
        location.href='/function-prototype-destination';
    "#,
    );
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(
        reply.navigation.as_deref(),
        Some("https://example.test/function-prototype-destination")
    );
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let document = rendered(&reply);
    assert_eq!(document.title, "Function prototype DOM ready");
    assert_eq!(content(&document, "#output"), "Rust & café");
    readable(&document);
}

#[test]
fn missing_dom_lookup_diagnostic_preserves_effects_and_skips_call_arguments() {
    let reply = page(
        "",
        "",
        r#"
        document.getElementById('output').textContent='Before missing lookup';
        document.querySelector('#authored-private-selector').appendChild((document.title='Incorrect argument',document.createElement('p')));
    "#,
    );
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(
        reply.errors,
        [
            "Inline script 1: Uncaught JavaScript exception: TypeError: property access on null or undefined [member operation=resolve-call-target base=null key=appendChild] [producer kind=host-call]"
        ]
    );
    assert!(reply.navigation.is_none());
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let document = rendered(&reply);
    assert_eq!(document.title, "Initial title");
    assert_eq!(content(&document, "#output"), "Before missing lookup");
    readable(&document);
}

#[test]
fn unknown_dom_property_detection_and_caught_fault_values_remain_unchanged() {
    let reply = page(
        "",
        "",
        r#"
        if(document.authoredMissingProperty!==undefined||document.getElementById('authored-absent')!==null)throw 'Missing values changed';
        var caught='';try{document.authoredMissingProperty.textContent='incorrect';}catch(error){caught=error;}
        if(caught!=='TypeError: property access on null or undefined')throw 'Caught value changed';
        document.getElementById('output').textContent='Unchanged catch, Rust & café';
        document.title='Diagnostic recovery ready';
    "#,
    );
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(reply.navigation.is_none());
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let document = rendered(&reply);
    assert_eq!(document.title, "Diagnostic recovery ready");
    assert_eq!(
        content(&document, "#output"),
        "Unchanged catch, Rust & café"
    );
    readable(&document);
}

#[test]
fn bound_startup_callback_and_host_method_create_actual_dom() {
    let reply = page(
        "",
        "",
        r#"
        var make=document.createElement.bind(document,'p');
        function ready(prefix){
            var node=make();node.id='bound-output';node.textContent=prefix+this.text;document.body.appendChild(node);
            document.title='Bound callback DOM ready';
            location.href='/bound-callback-destination';
        }
        document.addEventListener('DOMContentLoaded',ready.bind({text:'café'},'Rust & '));
    "#,
    );
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(
        reply.navigation.as_deref(),
        Some("https://example.test/bound-callback-destination")
    );
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let document = rendered(&reply);
    assert_eq!(document.title, "Bound callback DOM ready");
    assert_eq!(content(&document, "#bound-output"), "Rust & café");
    readable(&document);
}

#[test]
fn bound_symbol_result_keeps_dom_rejection_without_mutation_or_navigation() {
    let reply = page(
        "",
        "",
        r#"
        function identity(value){return value;}
        var value=identity.bind(null,Symbol('bound'));
        try{document.getElementById('output').textContent=value();location.href='/incorrect';}
        catch(error){if(String(error)!=='TypeError: cannot convert Symbol to string')throw error;}
        document.title='Bound value stays a Symbol';
    "#,
    );
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let document = rendered(&reply);
    assert_eq!(document.title, "Bound value stays a Symbol");
    assert_eq!(content(&document, "#output"), "Unchanged output");
    readable(&document);
}

#[test]
fn unread_prototype_replacement_keeps_symbol_dom_rejection_and_target() {
    let reply = page(
        "",
        "",
        r#"
        function Unread(){}
        Unread.prototype=Symbol('replacement');
        try{document.getElementById('output').textContent=Unread.prototype;location.href='/incorrect';}
        catch(error){if(String(error)!=='TypeError: cannot convert Symbol to string')throw error;}
        document.title='Prototype replacement stays a value';
    "#,
    );
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let document = rendered(&reply);
    assert_eq!(document.title, "Prototype replacement stays a value");
    assert_eq!(content(&document, "#output"), "Unchanged output");
    readable(&document);
}

#[test]
fn concat_keeps_symbol_values_until_dom_conversion_rejects_without_mutation() {
    let reply = page(
        "",
        "",
        r#"
        var value=Symbol('not DOM text');var result=[].concat([value]);
        if(result.length!==1 || result[0]!==value)throw 'Concat changed Symbol';
        document.title='Concat Symbol preserved';
        try{document.getElementById('output').textContent=result[0];location.href='/incorrect';}
        catch(error){if(String(error)!=='TypeError: cannot convert Symbol to string')throw error;}
    "#,
    );
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    assert!(reply.allocations.unwrap().first_rejected.is_none());
    let doc = rendered(&reply);
    assert_eq!(doc.title, "Concat Symbol preserved");
    assert_eq!(content(&doc, "#output"), "Unchanged output");
    readable(&doc);
}

#[test]
fn fatal_dom_coercion_cannot_run_handlers_or_a_later_script() {
    let reply = page(
        "",
        r#"
        <script>
            var value={};value[Symbol.toPrimitive]=function(){while(true){}};
            try{document.title=value;}
            catch(error){document.title='Incorrect catch';}
            finally{document.title='Incorrect finally';}
        </script>
    "#,
        "document.title='Incorrect later script';location.href='/incorrect';",
    );
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(reply.errors.len(), 2, "{:?}", reply.errors);
    assert!(
        reply
            .errors
            .iter()
            .all(|error| error.contains("JavaScript fuel exhausted"))
    );
    assert!(reply.navigation.is_none());
    let doc = rendered(&reply);
    assert_eq!(doc.title, "Initial title");
    readable(&doc);
}
