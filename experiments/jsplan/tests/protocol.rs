//! Integration checks launch only owned, pipe-connected research children.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn request(input: &Value, inherited: bool) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mg-jsplan-probe"));
    command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if inherited {
        command.env("UNEXPECTED_RESEARCH_ENV", "test");
    }
    let mut child = Owned(command.spawn().unwrap());
    let bytes = serde_json::to_vec(input).unwrap();
    assert!(bytes.len() < 4096);
    let written = child.0.stdin.take().unwrap().write_all(&bytes);
    if !inherited {
        written.unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "owned research child exceeded deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut output = String::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .take(65537)
        .read_to_string(&mut output)
        .unwrap();
    assert!(
        output.len() <= 65536 && status.success(),
        "{status}: {output}"
    );
    serde_json::from_str(&output).unwrap()
}
fn eval(engine: &str, source: &str) -> Value {
    json!({"protocol":1,"engine":engine,"goal":"script","action":"evaluate","source":source})
}

#[test]
fn environment_and_schema_reject_before_execution() {
    assert_eq!(
        request(&eval("boa", "true"), true)["outcome"],
        "termination"
    );
    let mut malformed = eval("boa", "true");
    malformed["protocol"] = json!(2);
    assert_eq!(request(&malformed, false)["outcome"], "termination");
    malformed["protocol"] = json!(1);
    malformed["extra_capability"] = json!("network");
    assert_eq!(request(&malformed, false)["outcome"], "termination");
}

#[test]
fn explicit_engines_do_not_fall_back() {
    let source = "const value = {a: 42}; (() => value.a === 42)();";
    assert_eq!(
        request(&eval("baseline", source), false)["outcome"],
        "unsupported"
    );
    let modern = request(&eval("boa", source), false);
    assert_eq!(modern["outcome"], "ok", "{modern}");
    assert_eq!(modern["value"], true);
    let mut invalid = eval("another-engine", "true;");
    assert_eq!(request(&invalid, false)["outcome"], "termination");
    invalid["engine"] = json!("baseline");
    assert_eq!(request(&invalid, false)["value"], true);
}

#[test]
fn production_policy_denies_capabilities_in_research_child() {
    let input =
        json!({"protocol":1,"engine":"boa","goal":"script","action":"probe","probe":"isolation"});
    let reply = request(&input, false);
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["value"], true);
}

#[test]
fn parser_runtime_and_harness_phases_are_distinct() {
    let malformed = request(&eval("boa", "let = ;"), false);
    assert_eq!(malformed["phase"], "parse", "{malformed}");
    let runtime = request(
        &eval("boa", "throw new TypeError('runtime negative');"),
        false,
    );
    assert_eq!(runtime["phase"], "runtime", "{runtime}");
    assert_eq!(runtime["outcome"], "exception");
    assert_eq!(runtime["error_type"], "TypeError");
    let mut input = eval("boa", "true;");
    input["includes"] = json!([{"name":"failure.js","source":"throw new Error('harness');"}]);
    let harness = request(&input, false);
    assert_eq!(harness["phase"], "harness", "{harness}");
}

#[test]
fn baseline_reports_existing_cumulative_budget_not_boa_metrics() {
    let simple = request(&eval("baseline", "6 * 7;"), false);
    assert_eq!(simple["value"].as_f64(), Some(42.0));
    assert_eq!(
        simple["metrics"]["logical_allocation"]["limit_bytes"],
        4194304
    );
    let exhausted = request(&eval("baseline", "while (true) {}"), false);
    assert_eq!(exhausted["outcome"], "termination", "{exhausted}");
}

#[test]
fn actual_harness_assertion_uses_captured_identity_without_diagnostic_getters() {
    // Same ordinary-constructor pattern as the pinned Test262 sta.js, not a
    // native Error subclass and not a fabricated response-envelope unit test.
    let mut input = eval(
        "boa",
        "var failure = new Test262Error('assertion'); Object.defineProperty(failure, 'message', {get: function(){while(true){}}}); Object.defineProperty(failure, 'name', {get: function(){while(true){}}}); Test262Error = new Proxy({}, {get: function(){while(true){}}}); throw failure;",
    );
    input["includes"] = json!([{"name":"sta.js","source":"function Test262Error(message) { this.message = message || ''; } Test262Error.prototype.toString = function() { return 'Test262Error: ' + this.message; };"}]);
    let reply = request(&input, false);
    assert_eq!(reply["outcome"], "exception", "{reply}");
    assert_eq!(reply["phase"], "runtime", "{reply}");
    assert_eq!(reply["error_type"], "Test262Error", "{reply}");
}

#[test]
fn harness_name_does_not_authorize_accessors_or_proxy_traps() {
    for source in [
        "Object.defineProperty(globalThis, 'Test262Error', {get: function(){while(true){}}});",
        "var Test262Error = {}; Object.defineProperty(Test262Error, 'prototype', {get: function(){while(true){}}});",
        "var Test262Error = new Proxy({}, {get: function(){while(true){}}, getOwnPropertyDescriptor: function(){while(true){}}});",
    ] {
        let mut input = eval("boa", "throw {name: 'Test262Error'};");
        input["includes"] = json!([{"name":"sta.js","source":source}]);
        let reply = request(&input, false);
        assert_eq!(reply["outcome"], "exception", "{reply}");
        assert_eq!(reply["phase"], "runtime", "{reply}");
        assert_eq!(reply["error_type"], "ThrownValue", "{reply}");
    }
}
