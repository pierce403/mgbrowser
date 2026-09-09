//! Evaluate a local expression without browser or platform dependencies.
use mg_butane::runtime::{Host, Runtime, Value};

struct NoIo;
impl Host for NoIo {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        Err(format!("Host read unavailable: {object}.{key}"))
    }
    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        Err(format!("Host write unavailable: {object}.{key}"))
    }
    fn call(&mut self, name: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        Err(format!("Host call unavailable: {name}"))
    }
}

fn main() -> Result<(), String> {
    let source = std::env::args().nth(1).unwrap_or_else(|| "6 * 7;".into());
    let value = Runtime::new().execute(&source, &mut NoIo)?;
    println!("{}", value.as_text());
    Ok(())
}
