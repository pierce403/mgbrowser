//! Current evaluator as an explicit comparison, never a fallback for Boa.
use mg_butane::{
    runtime::{Host, Runtime, Value},
    syntax,
};
use serde_json::{Value as Json, json};

struct NoIo;
impl Host for NoIo {
    fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
        Err("Host reads unsupported".into())
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        Err("Host writes unsupported".into())
    }
    fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        Err("Host calls unsupported".into())
    }
}
fn error(outcome: &str, phase: &str, message: &str, runtime: &Runtime) -> Json {
    // The old API returns untyped diagnostic strings: do not invent an error
    // class from their prefix to claim typed Test262 negative-test successes.
    json!({"protocol":1,"outcome":outcome,"phase":phase,
        "message":message.chars().take(512).collect::<String>(),
        "metrics":{"logical_allocation":runtime.allocation_report()}})
}
pub fn evaluate(request: &Json) -> Json {
    let mut runtime = Runtime::new();
    if request["goal"] == "module" || request["async"] == true || request["action"] == "probe" {
        return error(
            "unsupported",
            "harness",
            "Baseline lacks modules/async/modern host probes",
            &runtime,
        );
    }
    let mut host = NoIo;
    for include in request["includes"].as_array().into_iter().flatten() {
        if let Err(message) = runtime.execute(include["source"].as_str().unwrap_or(""), &mut host) {
            return error(
                if runtime.is_fatal() {
                    "termination"
                } else {
                    "unsupported"
                },
                "harness",
                &message,
                &runtime,
            );
        }
    }
    let source = request["source"].as_str().unwrap_or("");
    if let Err(message) = syntax::parse(source) {
        return error(
            if syntax::is_limit_error(&message) {
                "termination"
            } else {
                "unsupported"
            },
            "parse",
            &message,
            &runtime,
        );
    }
    if request["action"] == "parse" {
        return json!({"protocol":1,"outcome":"ok","phase":"parse"});
    }
    match runtime.execute(source, &mut host) {
        Ok(value) => {
            let value = match value {
                Value::Bool(v) => json!(v),
                Value::Null => Json::Null,
                Value::Number(v) if v.is_finite() && !(v == 0.0 && v.is_sign_negative()) => {
                    json!(v)
                }
                Value::Number(v) => {
                    json!({"number":if v.is_nan() {"NaN"} else if v == 0.0 {"-0"} else if v.is_sign_negative() {"-Infinity"} else {"Infinity"}})
                }
                Value::String(v) => json!({"utf16":v}),
                Value::Undefined => json!({"type":"undefined"}),
                _ => json!({"type":"opaque","comparison":"unsupported"}),
            };
            json!({"protocol":1,"outcome":"ok","phase":"runtime","value":value,
                "metrics":{"logical_allocation":runtime.allocation_report()}})
        }
        Err(message) => error(
            if runtime.is_fatal() {
                "termination"
            } else {
                "exception"
            },
            "runtime",
            &message,
            &runtime,
        ),
    }
}
