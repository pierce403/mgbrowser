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
