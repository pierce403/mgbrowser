//! The sandbox is installed only in owned executable children, never this test
//! harness. These local fixtures exercise containment, not live website code.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::{
    io::{Read, Write},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Also clean up if an assertion, pipe write, or wait unexpectedly fails.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(args: &[&str], input: &[u8], timeout: Duration) -> (ExitStatus, String, String) {
    // Test inputs are tiny, so stdin cannot fill its pipe before we begin the
    // timed wait. The normal worker parent separately tests bounded bulk I/O.
    assert!(input.len() < 4096);
    let mut child = OwnedChild(
        Command::new(env!("CARGO_BIN_EXE_mgbrowser"))
            .args(args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start owned script-worker test child"),
    );
    child
        .0
        .stdin
        .take()
        .expect("child stdin")
        .write_all(input)
        .expect("write tiny local fixture");
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("observe owned test child") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "script-worker test child timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    fn output(reader: impl Read) -> String {
        let mut bytes = Vec::new();
        reader
            .take(65_537)
            .read_to_end(&mut bytes)
            .expect("read bounded test output");
        assert!(bytes.len() <= 65_536, "test child output exceeded 64 KiB");
        String::from_utf8(bytes).expect("test child output is UTF-8")
    }
    let stdout = output(child.0.stdout.take().expect("child stdout"));
    let stderr = output(child.0.stderr.take().expect("child stderr"));
    (status, stdout, stderr)
}

#[test]
fn isolation_probes_really_run_and_owned_children_are_reaped() {
    let (status, stdout, stderr) = run(&["--script-worker-selftest"], &[], Duration::from_secs(15));
    assert!(status.success(), "selftest {status}: {stdout}\n{stderr}");
    for marker in [
        "SCRIPT_WORKER_CHECK deny: passed (child reaped)",
        "SCRIPT_WORKER_CHECK memory: passed (child reaped)",
        "SCRIPT_WORKER_CHECK cpu: terminated by resource limit (child reaped)",
        "SCRIPT_WORKER_CHECK wall: killed and reaped",
        "SCRIPT_WORKER_CHECK output: killed and reaped",
        "SCRIPT_WORKER_CHECK protocol: bounded document round trip passed",
        "SCRIPT_WORKER_CHECK bulk: multi-buffer document round trip passed",
        "SCRIPT_WORKER_CHECK input: oversized request rejected before launch",
        "SCRIPT_WORKER_SELFTEST_OK",
    ] {
        assert!(stdout.contains(marker), "missing {marker}: {stdout}");
    }
}

#[test]
fn restricted_child_executes_original_javascript_and_serializes_dom() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/owned-fixture".into(),
        html: "<html><head><title>Before</title></head><body><p id=output>Old</p><script>document.title='Worker ready';document.getElementById('output').textContent='Changed & safe';location.href='/next?q=rust';</script></body></html>".into(),
    };
    let input = serde_json::to_vec(&request).expect("serialize local request");
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply =
        serde_json::from_str(&stdout).expect("decode worker reply");
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(
        reply.navigation.as_deref(),
        Some("https://example.test/next?q=rust")
    );
    assert!(reply.html.contains("<title>Worker ready</title>"));
    assert!(reply.html.contains("Changed &amp; safe"));
}

#[test]
fn restricted_child_uses_real_symbols_before_creating_controls() {
    // This exact source previously stopped at ReferenceError: Symbol is not
    // defined. Identity/key/reflection/registry checks precede all form creation.
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-symbol-worker-baseline".into(),
        html: include_str!("fixtures/script/symbols.html").into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    let report = reply.allocations.unwrap();
    println!("Symbol fixture allocation: {report:?}");
    assert!(report.is_valid());
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none());
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Symbol-built local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(document.items.iter().any(|item| matches!(item,
        mg_deps::document::Item::Input { name, .. } if name == "q")));
    assert!(document.nodes.iter().any(|node| node.tag == "input"
        && node.attr("type") == Some("hidden")
        && node.attr("name") == Some("source")
        && node.attr("value") == Some("fixture")));
    assert!(
        document
            .nodes
            .iter()
            .any(|node| node.tag == "button" && node.attr("type") == Some("submit"))
    );
    assert!(reply.html.contains("Symbol form ready"));
}

#[test]
fn restricted_child_rejects_symbol_dom_conversion_and_recovers_in_later_script() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-symbol-conversion".into(),
        html: "<html><head><title>Symbol conversion fallback</title></head><body><p id=output>Kept text</p><script>var token=Symbol('not text');document.getElementById('output').textContent=Object(token);location.href='/incorrect';</script><script>if(typeof token==='symbol'){document.title='Recovered after Symbol error';}</script></body></html>".into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(reply.errors.len(), 1);
    assert!(reply.errors[0].contains("TypeError"), "{:?}", reply.errors);
    assert!(reply.navigation.is_none());
    let report = reply.allocations.unwrap();
    assert!(report.is_valid());
    assert!(report.first_rejected.is_none());
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Recovered after Symbol error");
    let output = document.query_selector(0, "#output").unwrap().unwrap();
    assert_eq!(
        document.nodes[document.nodes[output].children[0]].text,
        "Kept text"
    );
    assert!(document.forms.is_empty());
}

#[test]
fn restricted_child_symbol_description_storage_stays_cumulative_and_fatal() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-symbol-storage-limit".into(),
        html: "<html><head><title>Symbol storage fallback</title></head><body><p id=output>Readable Symbol fallback</p><script>var description=Array(10000).join('abcdefgh');document.getElementById('output').setAttribute('data-builder','ready');Symbol(description);document.getElementById('output').setAttribute('data-symbol','ready');try{for(var i=0;i<100;i++){Symbol(description);}document.title='Incorrect completion';}catch(error){document.title='Incorrect catch';}finally{document.title='Incorrect finally';}</script><script>document.title='Incorrect later script';location.href='/incorrect';</script></body></html>".into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(reply.errors.len(), 2);
    assert!(reply.navigation.is_none());
    let report = reply.allocations.unwrap();
    println!("Symbol storage negative allocation: {report:?}");
    assert!(report.is_valid());
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert_eq!(
        report.first_rejected.unwrap().phase,
        mg_deps::js::runtime::AllocationPhase::Runtime
    );
    let first = reply.errors[0].split_once(": ").unwrap().1;
    assert!(first.contains("JavaScript allocation budget exhausted"));
    assert_eq!(reply.errors[1].split_once(": ").unwrap().1, first);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Symbol storage fallback");
    assert!(document.forms.is_empty());
    assert!(reply.html.contains("Readable Symbol fallback"));
    // The builder and a real Symbol succeeded before the failing loop. A
    // Runtime-phase failure in Array/join alone cannot satisfy this negative.
    let output = document.query_selector(0, "#output").unwrap().unwrap();
    assert_eq!(document.nodes[output].attr("data-builder"), Some("ready"));
    assert_eq!(document.nodes[output].attr("data-symbol"), Some("ready"));
}

#[test]
fn restricted_child_retains_large_ast_and_calls_real_form_builder() {
    // Frozen before the AST change: 19,998 harmless statements followed by 19
    // DOM-building statements. Its original AST admission exceeded 4 MiB.
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-ast-worker-baseline".into(),
        html: include_str!("fixtures/script/ast.html").into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    let report = reply.allocations.unwrap();
    println!("large AST fixture allocation: {report:?}");
    assert!(report.is_valid());
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none());
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.navigation.is_none());
    // Body storage remains charged even though inline number expressions do
    // not each own another allocation. Original array/source creation still pays.
    assert!(report.phases.ast >= 20_000 * 80);
    assert!(report.phases.runtime >= 10_000 * 64 + 39_996 * 4);
    assert!(report.phases.source >= 40_530);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "AST-built local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(document.items.iter().any(|item| matches!(item,
        mg_deps::document::Item::Input { name, .. } if name == "q")));
    assert!(document.nodes.iter().any(|node| node.tag == "input"
        && node.attr("type") == Some("hidden")
        && node.attr("name") == Some("source")
        && node.attr("value") == Some("fixture")));
    assert!(reply.html.contains("AST form ready"));
}

#[test]
fn restricted_child_sparse_ast_capacity_stays_cumulative_and_fatal() {
    // Merely retaining a function with holes must pay its AST slot capacity,
    // even though the returned array is never executed or created at runtime.
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-sparse-ast-limit".into(),
        html: "<html><head><title>Original sparse fallback</title></head><body><p>Readable sparse AST fallback</p><script>try{for(var i=0;i<6;i++){Function('return ['+Array(10000).join(',')+'];');}document.title='Incorrect completion';}catch(error){document.title='Incorrect catch';}finally{document.title='Incorrect finally';}</script><script>document.title='Incorrect later script';location.href='/incorrect';</script></body></html>".into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(reply.errors.len(), 2);
    assert!(reply.navigation.is_none());
    let report = reply.allocations.unwrap();
    println!("sparse AST fixture allocation: {report:?}");
    assert!(report.is_valid());
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    let rejected = report.first_rejected.unwrap();
    assert_eq!(rejected.phase, mg_deps::js::runtime::AllocationPhase::Ast);
    assert!(rejected.requested_bytes >= 16_384 * 56);
    assert!(report.phases.ast >= 2 * 16_384 * 56);
    let first = reply.errors[0].split_once(": ").unwrap().1;
    assert!(first.contains("JavaScript allocation budget exhausted"));
    assert_eq!(reply.errors[1].split_once(": ").unwrap().1, first);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Original sparse fallback");
    assert!(document.forms.is_empty());
    assert!(reply.html.contains("Readable sparse AST fallback"));
}

#[test]
fn restricted_child_uses_labeled_control_flow_and_uri_builtins() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-language-fixture".into(),
        html: r#"<html><head><title>Before</title></head><body><p id=output>Before</p><script>
        var word='';
        outer: for(var i=0;i<3;i++) {
            for(var j=0;j<3;j++) {
                if(j===1)continue outer;
                word+=i;
            }
        }
        done: {word+=' café';break done;word='wrong';}
        var encoded=encodeURIComponent(word);
        document.getElementById('output').textContent=decodeURIComponent(encoded);
        document.title='Labels and URI ready';
        location.href='/next?q='+encoded;
        </script></body></html>"#
            .into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.html.contains("<title>Labels and URI ready</title>"));
    assert!(reply.html.contains("012 café"));
    assert_eq!(
        reply.navigation.as_deref(),
        Some("https://example.test/next?q=012%20caf%C3%A9")
    );
}

#[test]
fn restricted_child_dynamic_compilation_creates_real_form_controls() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/script-dynamic".into(),
        html: include_str!("fixtures/script/dynamic.html").into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Dynamic script-built local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(
        document
            .items
            .iter()
            .any(|item| matches!(item,mg_deps::document::Item::Input{name,..} if name=="q"))
    );
}

#[test]
fn restricted_child_regexp_execution_creates_real_form_controls() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/script-regexp".into(),
        html: include_str!("fixtures/script/regexp.html").into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Regex-built local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(
        document
            .items
            .iter()
            .any(|item| matches!(item, mg_deps::document::Item::Input { name, .. } if name == "q"))
    );
    assert!(
        reply
            .html
            .contains("captures, lastIndex, replacement created this usable form.")
    );
}

#[test]
fn restricted_child_rejects_invalid_literal_before_dom_prefix_effects() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-regexp-syntax".into(),
        html: "<html><head><title>Original title</title></head><body><p id=output>Before</p><script>document.title='Incorrect prefix';var broken=/(/;</script><script>document.getElementById('output').textContent='Later script executes';</script></body></html>".into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(reply.errors.len(), 1);
    assert!(
        reply.errors[0].contains("SyntaxError"),
        "{:?}",
        reply.errors
    );
    assert!(reply.html.contains("<title>Original title</title>"));
    assert!(reply.html.contains("Later script executes"));
}

#[test]
fn restricted_child_iteration_and_switch_create_real_form_controls() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/script-iteration".into(),
        html: include_str!("fixtures/script/iteration.html").into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Iteration-built local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(
        document
            .items
            .iter()
            .any(|item| matches!(item, mg_deps::document::Item::Input { name, .. } if name == "q"))
    );
    assert!(
        reply
            .html
            .contains("Own and inherited fields plus switch created this usable form.")
    );
    assert!(document.nodes.iter().any(|node| node.tag == "input"
        && node.attr("name") == Some("source")
        && node.attr("value") == Some("fixture")));
}

#[test]
fn restricted_child_rejects_invalid_switch_before_dom_prefix_effects() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-switch-syntax".into(),
        html: "<html><head><title>Original title</title></head><body><p id=output>Before</p><script>document.title='Incorrect prefix';switch(1){default:break;default:break;}</script><script>document.getElementById('output').textContent='Later script executes';</script></body></html>".into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(reply.errors.len(), 1);
    assert!(
        reply.errors[0].contains("SyntaxError"),
        "{:?}",
        reply.errors
    );
    assert!(reply.html.contains("<title>Original title</title>"));
    assert!(reply.html.contains("Later script executes"));
}

#[test]
fn restricted_child_deeply_grouped_factory_creates_real_form_controls() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/script-expressions".into(),
        html: include_str!("fixtures/script/expressions.html").into(),
    };
    assert!(
        request
            .html
            .contains(&format!("var create = {}function", "(".repeat(64)))
    );
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Expression-built local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(
        document
            .items
            .iter()
            .any(|item| matches!(item, mg_deps::document::Item::Input { name, .. } if name == "q"))
    );
    assert!(document.nodes.iter().any(|node| node.tag == "input"
        && node.attr("name") == Some("source")
        && node.attr("value") == Some("fixture")));
    assert!(
        reply
            .html
            .contains("64 grouping pairs created this usable form.")
    );
}

#[test]
fn restricted_child_malformed_group_rejects_prefix_and_preserves_later_script() {
    let source = format!(
        "document.title='Incorrect prefix';var broken={}1{};",
        "(".repeat(64),
        ")".repeat(63)
    );
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-expression-syntax".into(),
        html: format!(
            "<html><head><title>Original title</title></head><body><p id=output>Before</p><script>{source}</script><script>document.getElementById('output').textContent='Later script executes';</script></body></html>"
        ),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(reply.errors.len(), 1);
    assert!(
        reply.errors[0].contains("SyntaxError"),
        "{:?}",
        reply.errors
    );
    assert!(reply.html.contains("<title>Original title</title>"));
    assert!(reply.html.contains("Later script executes"));
}

#[test]
fn restricted_child_excessive_group_depth_is_fatal_before_prefix_effects() {
    let source = format!(
        "document.title='Incorrect prefix';var tooDeep={}1{};",
        "(".repeat(1024),
        ")".repeat(1024)
    );
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-expression-limit".into(),
        html: format!(
            "<html><head><title>Original title</title></head><body><p>Readable original content</p><script>{source}</script><script>document.title='Incorrect later script';</script></body></html>"
        ),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(reply.errors.len(), 2);
    assert!(
        reply
            .errors
            .iter()
            .all(|message| message.contains("parser limit")),
        "{:?}",
        reply.errors
    );
    assert!(reply.navigation.is_none());
    assert!(reply.html.contains("<title>Original title</title>"));
    assert!(reply.html.contains("Readable original content"));
}

#[test]
fn restricted_child_nested_evaluation_limits_cannot_be_caught_or_reset() {
    // Authored expression-heavy and statement-heavy recursion; these must
    // return a language resource error, not crash the worker or run finally.
    for body in [
        format!("return {}recurse();", "+ ".repeat(8)),
        format!("{}return recurse();", "if(true)".repeat(32)),
        format!("{}return recurse();", "for(var key in {a:1})".repeat(48)),
        format!(
            "{}return {}recurse();{}",
            "switch(1){case 1:".repeat(48),
            "+ ".repeat(24),
            "}".repeat(48)
        ),
    ] {
        let request = mg_deps::js_browser::Request {
            url: "https://example.test/local-evaluation-limit".into(),
            html: format!(
                "<html><head><title>Original title</title></head><body><p>Readable original content</p><script>function recurse(){{{body}}}try{{recurse();}}catch(e){{document.title='Incorrect catch';}}finally{{document.title='Incorrect finally';}}</script><script>document.title='Incorrect later script';</script></body></html>"
            ),
        };
        let input = serde_json::to_vec(&request).unwrap();
        let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
        assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
        let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
        assert!(reply.applied);
        assert_eq!(reply.scripts_executed, 0);
        assert_eq!(reply.errors.len(), 2);
        assert!(
            reply
                .errors
                .iter()
                .all(|message| message.contains("evaluation depth limit exhausted")),
            "{:?}",
            reply.errors
        );
        assert!(reply.navigation.is_none());
        assert!(reply.html.contains("<title>Original title</title>"));
        assert!(reply.html.contains("Readable original content"));
    }
}

#[test]
fn restricted_child_returns_first_allocation_failure_without_reset_or_dom_effects() {
    use mg_deps::js::runtime::AllocationPhase;
    let source = "try{while(true){Array(1000);}}catch(e){document.title='Incorrect catch';}finally{document.title='Incorrect finally';}";
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-allocation-limit".into(),
        html: format!(
            "<html><head><title>Original title</title></head><body><p>Readable original content</p><script>{source}</script><script>document.title='Incorrect later script';</script></body></html>"
        ),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(reply.errors.len(), 2);
    let report = reply.allocations.expect("executed realm has a report");
    assert!(report.is_valid());
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    let failure = report.first_rejected.expect("allocation failure retained");
    assert_eq!(failure.phase, AllocationPhase::Runtime);
    assert_eq!(failure.accepted_bytes, report.accepted_bytes);
    assert!(failure.requested_bytes > report.limit_bytes - report.accepted_bytes);
    let first = reply.errors[0].split_once(": ").unwrap().1;
    let second = reply.errors[1].split_once(": ").unwrap().1;
    assert_eq!(first, second, "later script must retain the first failure");
    assert!(first.contains("JavaScript allocation budget exhausted"));
    assert!(reply.navigation.is_none());
    assert!(reply.html.contains("<title>Original title</title>"));
    assert!(reply.html.contains("Readable original content"));
    let diagnostic = serde_json::to_string(&report).unwrap();
    assert!(diagnostic.len() < 1024);
    assert!(!diagnostic.contains("example.test"));
    assert!(!diagnostic.contains("Incorrect"));
}

#[test]
fn restricted_child_reports_successful_allocation_totals_and_pre_runtime_rejection() {
    for (url, expected_applied) in [
        ("https://example.test/local-allocation-report", true),
        ("file:///local-allocation-report", false),
    ] {
        let request = mg_deps::js_browser::Request {
            url: url.into(),
            html: "<html><body><p>Local fixture</p><script>function add(a,b){return a+b;}document.title=String(add(2,3));</script></body></html>".into(),
        };
        let input = serde_json::to_vec(&request).unwrap();
        let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
        assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
        let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
        assert_eq!(reply.applied, expected_applied);
        if expected_applied {
            let report = reply.allocations.unwrap();
            assert!(report.is_valid());
            assert!(report.first_rejected.is_none());
            assert!(report.phases.bootstrap > 0);
            assert!(report.phases.source > 0);
            assert!(report.phases.ast > 0);
            assert!(reply.errors.is_empty(), "{:?}", reply.errors);
            assert_eq!(reply.scripts_executed, 1);
            assert!(reply.html.contains("<title>5</title>"));
        } else {
            assert!(reply.allocations.is_none());
            assert!(reply.navigation.is_none());
            assert_eq!(reply.scripts_executed, 0);
        }
    }
}

#[test]
fn restricted_child_shared_large_factory_creates_real_form_inside_unchanged_budget() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/script-allocation".into(),
        html: include_str!("fixtures/script/allocation.html").into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    let report = reply.allocations.unwrap();
    assert!(report.is_valid());
    assert!(report.first_rejected.is_none());
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    // 9,999 numbers plus a return occupy the shared body slots. The former
    // two-MiB threshold described fixed node weights, not retained storage.
    assert!(report.phases.ast >= 10_000 * std::mem::size_of::<mg_deps::js::Stmt>() as u64);
    assert!(report.phases.function_code < 1024, "{report:?}");
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Shared-code local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(
        document
            .nodes
            .iter()
            .any(|node| node.tag == "input" && node.attr("name") == Some("q"))
    );
    assert!(document.nodes.iter().any(|node| node.tag == "input"
        && node.attr("name") == Some("source")
        && node.attr("value") == Some("fixture")));
    assert!(reply.html.contains("within the unchanged budget."));
}

#[test]
fn restricted_child_owned_function_source_creates_form_inside_unchanged_budget() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/script-sources".into(),
        html: include_str!("fixtures/script/sources.html").into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    let report = reply.allocations.unwrap();
    assert!(report.is_valid());
    println!("source fixture allocation: {report:?}");
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none());
    assert!(report.phases.runtime >= 10_000 * 64 + 749_925 * 2);
    assert!(report.phases.source >= 749_925);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Source-owned local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(document.nodes.iter().any(|node| node.tag == "input"
        && node.attr("type") == Some("hidden")
        && node.attr("name") == Some("source")
        && node.attr("value") == Some("fixture")));
    assert!(document.items.iter().any(|item| matches!(item,
        mg_deps::document::Item::Input { name, .. } if name == "q")));
    assert!(reply.html.contains("Owned source and real controls fit"));
}

#[test]
fn restricted_child_multiple_source_fragments_keep_real_join_and_utf8_limits() {
    let separator = " ".repeat(75);
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-multiple-source-limit".into(),
        html: format!(
            "<html><head><title>Original source title</title></head><body><p>Readable source fallback</p><script>try{{Function(Array(10000).join('{separator}'),'',\"document.title='Incorrect body';\")();location.href='/incorrect';}}catch(error){{document.title='Incorrect catch';}}finally{{document.title='Incorrect finally';}}</script><script>document.title='Incorrect later script';</script></body></html>"
        ),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(reply.errors.len(), 2);
    assert!(reply.navigation.is_none());
    let report = reply.allocations.unwrap();
    assert!(report.is_valid());
    let first = report.first_rejected.unwrap();
    assert_eq!(first.phase, mg_deps::js::runtime::AllocationPhase::Source);
    // Two parameter fragments still require a real joined buffer and comma,
    // admitted before the required ASCII-to-UTF8 source copy is rejected.
    assert_eq!(first.requested_bytes, 749_926);
    assert!(report.phases.source >= 749_926 * 2);
    assert!(report.phases.runtime >= 10_000 * 64 + 749_925 * 2);
    let diagnostic = reply.errors[0].split_once(": ").unwrap().1;
    assert!(diagnostic.contains("JavaScript allocation budget exhausted"));
    assert_eq!(reply.errors[1].split_once(": ").unwrap().1, diagnostic);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Original source title");
    assert!(document.forms.is_empty());
    assert!(reply.html.contains("Readable source fallback"));
}

#[test]
fn restricted_child_parameter_copy_creates_form_inside_unchanged_budget() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/script-bindings".into(),
        html: include_str!("fixtures/script/bindings.html").into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    let report = reply.allocations.unwrap();
    assert!(report.is_valid());
    println!("parameter fixture allocation: {report:?}");
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.first_rejected.is_none());
    // One source array plus the original joined buffer and its actual copy.
    assert!(report.phases.runtime >= 10_000 * 64 + 749_925 * 4);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Parameter-copy local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(document.nodes.iter().any(|node| node.tag == "input"
        && node.attr("type") == Some("hidden")
        && node.attr("name") == Some("source")
        && node.attr("value") == Some("fixture")));
    assert!(document.items.iter().any(|item| matches!(item,
        mg_deps::document::Item::Input { name, .. } if name == "q")));
    assert!(
        reply
            .html
            .contains("original argument, independent parameter copy and real controls")
    );
}

#[test]
fn restricted_child_parameter_real_copy_exhaustion_latches_before_body_or_handlers() {
    let separator = "0123456789".repeat(11);
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/local-parameter-limit".into(),
        html: format!(
            "<html><head><title>Original title</title></head><body><p>Readable parameter fallback</p><script>function touch(buffer){{document.title='Incorrect body';}}try{{touch(Array(10000).join('{separator}'));location.href='/incorrect';}}catch(error){{document.title='Incorrect catch';}}finally{{document.title='Incorrect finally';}}</script><script>document.title='Incorrect later script';</script></body></html>"
        ),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(reply.errors.len(), 2);
    assert!(reply.navigation.is_none());
    let report = reply.allocations.unwrap();
    assert!(report.is_valid());
    let first = report.first_rejected.unwrap();
    assert_eq!(first.phase, mg_deps::js::runtime::AllocationPhase::Runtime);
    assert_eq!(first.requested_bytes, 9_999 * 110 * 2);
    assert!(report.phases.runtime >= 10_000 * 64 + 9_999 * 110 * 2);
    let diagnostic = reply.errors[0].split_once(": ").unwrap().1;
    assert!(diagnostic.contains("JavaScript allocation budget exhausted"));
    assert_eq!(reply.errors[1].split_once(": ").unwrap().1, diagnostic);
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Original title");
    assert!(document.forms.is_empty());
    assert!(reply.html.contains("Readable parameter fallback"));
}

#[test]
fn restricted_child_prepaid_arrays_create_real_form_inside_unchanged_budget() {
    let request = mg_deps::js_browser::Request {
        url: "https://example.test/script-arrays".into(),
        html: include_str!("fixtures/script/arrays.html").into(),
    };
    let input = serde_json::to_vec(&request).unwrap();
    let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    let report = reply.allocations.unwrap();
    assert!(report.is_valid());
    assert!(report.first_rejected.is_none());
    assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
    assert!(report.phases.runtime >= 6 * 10_000 * 64, "{report:?}");
    let document = mg_deps::document::parse_with_scripting(&reply.html, &request.url, true);
    assert_eq!(document.title, "Prepaid-array local fixture");
    assert_eq!(document.forms.len(), 1);
    assert_eq!(document.forms[0].action, "https://example.test/search");
    assert!(
        document
            .nodes
            .iter()
            .any(|node| node.tag == "input" && node.attr("name") == Some("q"))
    );
    assert!(document.nodes.iter().any(|node| node.tag == "input"
        && node.attr("name") == Some("source")
        && node.attr("value") == Some("fixture")));
    assert!(
        reply
            .html
            .contains("Six independent arrays and real form controls")
    );
}

#[test]
fn restricted_child_retained_arguments_preserve_snapshot_and_fatal_array_limits() {
    for (source, succeeds) in [
        (
            r#"var original={};
        function keep(text,object){text='changed';arguments[1].seen='same';return arguments;}
        var saved=keep('\uD800Z',original);
        if(saved[0].charCodeAt(0)!==55296 || saved[0].charCodeAt(1)!==90 ||
            saved[1]!==original || original.seen!=='same' || saved.callee!==keep) {
            throw 'Unmapped arguments changed';
        }
        document.title='Retained arguments ready';"#,
            true,
        ),
        (
            r#"try{for(var index=0;index<7;index++){Array(10000);}}
        catch(error){document.title='Incorrect catch';}
        finally{document.title='Incorrect finally';}"#,
            false,
        ),
    ] {
        let request = mg_deps::js_browser::Request {
            url: "https://example.test/local-array-ownership".into(),
            html: format!(
                "<html><head><title>Original title</title></head><body><p>Readable original content</p><script>{source}</script><script>if(document.title==='Retained arguments ready'){{document.title='Later snapshot ready';}}else{{document.title='Incorrect later script';}}</script></body></html>"
            ),
        };
        let input = serde_json::to_vec(&request).unwrap();
        let (status, stdout, stderr) = run(&["--script-worker"], &input, Duration::from_secs(3));
        assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
        let reply: mg_deps::js_browser::Reply = serde_json::from_str(&stdout).unwrap();
        assert!(reply.applied);
        let report = reply.allocations.unwrap();
        assert!(report.is_valid());
        assert_eq!(report.limit_bytes, 4 * 1024 * 1024);
        assert!(reply.navigation.is_none());
        if succeeds {
            assert!(reply.errors.is_empty(), "{:?}", reply.errors);
            assert_eq!(reply.scripts_executed, 2);
            assert!(report.first_rejected.is_none());
            assert!(reply.html.contains("<title>Later snapshot ready</title>"));
        } else {
            assert_eq!(reply.scripts_executed, 0);
            assert_eq!(reply.errors.len(), 2);
            let first = reply.errors[0].split_once(": ").unwrap().1;
            assert_eq!(first, reply.errors[1].split_once(": ").unwrap().1);
            assert!(first.contains("JavaScript allocation budget exhausted"));
            assert_eq!(
                report.first_rejected.unwrap().phase,
                mg_deps::js::runtime::AllocationPhase::Runtime
            );
            assert!(reply.html.contains("<title>Original title</title>"));
            assert!(reply.html.contains("Readable original content"));
        }
    }
}
