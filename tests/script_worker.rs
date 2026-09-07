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
