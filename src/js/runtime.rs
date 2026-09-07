//! Original, deliberately limited classic-script evaluator.
//!
//! Values use UTF-16 strings. `Value::as_text` is a host-display coercion, not
//! user-defined ToPrimitive: it replaces lone surrogates only at the UTF-8 host
//! boundary and approximates number/function formatting. The evaluator keeps
//! string code units intact. Resource accounting is a conservative, cumulative
//! logical allocation budget, not a measurement of allocator RSS; the worker's
//! OS memory limit is a separate boundary. No garbage collector is implemented.
//! Dynamic parsing charges source conversion and fixed attempt overhead, then
//! successful ASTs; discarded partial AST/token buffers are independently bounded
//! by parser limits, not measured cumulatively as physical allocation bytes.
//!
//! This is a classic non-strict subset, not ECMAScript conformance. In particular,
//! arguments are an unmapped snapshot; descriptors/accessors, lexical declarations,
//! modules, regex, promises and a general event loop are not implemented.
//! Unsupported exposed builtins throw an explicit error. Math.random
//! is a deterministic research PRNG and must never be used for cryptography.

use super::{Expr, Program, Stmt, syntax, uri};
use std::rc::Rc;

const MAX_FUEL: u64 = 1_000_000;
const MAX_HEAP: usize = 4 * 1024 * 1024;
const MAX_OBJECTS: usize = 10_000;
const MAX_CALLS: usize = 64;
const MAX_ARRAY: usize = 10_000;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u16>),
    Object(usize),
    Function(usize),
    Native(String),
    Host(String),
}

impl Value {
    pub fn text(text: &str) -> Self {
        Self::String(text.encode_utf16().collect())
    }
    pub fn as_text(&self) -> String {
        match self {
            Self::Undefined => "undefined".into(),
            Self::Null => "null".into(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => number_text(*value),
            Self::String(value) => String::from_utf16_lossy(value),
            Self::Function(_) => "function () { [mgbrowser code] }".into(),
            Self::Native(name) => format!("function {name}() {{ [native code] }}"),
            Self::Object(_) | Self::Host(_) => "[object Object]".into(),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(v) => *v,
            Self::Number(v) => *v != 0.0 && !v.is_nan(),
            Self::String(v) => !v.is_empty(),
            _ => true,
        }
    }
    fn primitive(&self) -> bool {
        !matches!(
            self,
            Self::Object(_) | Self::Function(_) | Self::Native(_) | Self::Host(_)
        )
    }
    fn callable(&self) -> bool {
        matches!(self, Self::Function(_) | Self::Native(_))
    }
}

pub trait Host {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String>;
    fn set(&mut self, object: &str, key: &str, value: Value) -> Result<(), String>;
    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String>;
}

#[derive(Debug)]
enum Fault {
    Throw(Value),
    Fatal(String),
}
type Eval<T> = Result<T, Fault>;
fn exception(message: impl Into<String>) -> Fault {
    Fault::Throw(Value::text(&message.into()))
}
fn unsupported(what: &str) -> Fault {
    exception(format!("Unsupported JavaScript behavior: {what}"))
}

struct Budget {
    fuel: u64,
    allocated: usize,
    calls: usize,
}
impl Budget {
    fn step(&mut self) -> Eval<()> {
        self.fuel = self
            .fuel
            .checked_sub(1)
            .ok_or_else(|| Fault::Fatal("JavaScript fuel exhausted".into()))?;
        Ok(())
    }
    fn allocate(&mut self, bytes: usize) -> Eval<()> {
        let total = self.allocated.saturating_add(bytes);
        if total > MAX_HEAP {
            return Err(Fault::Fatal(
                "JavaScript allocation budget exhausted".into(),
            ));
        }
        self.allocated = total;
        Ok(())
    }
    fn copy(&mut self, value: &Value) -> Eval<Value> {
        self.allocate(value_bytes(value))?;
        Ok(value.clone())
    }
}
fn value_bytes(value: &Value) -> usize {
    match value {
        Value::String(s) => s.len().saturating_mul(2),
        Value::Native(s) | Value::Host(s) => s.len(),
        _ => 0,
    }
}

struct Property {
    key: String,
    value: Value,
    enumerable: bool,
}
struct Object {
    properties: Vec<Property>,
    array: Option<Vec<Option<Value>>>,
    prototype: Option<usize>,
    boxed: Option<Value>,
}
struct Environment {
    parent: Option<usize>,
    variable: usize,
    bindings: Vec<Binding>,
}
struct Binding {
    name: String,
    value: Value,
    deletable: bool,
}
struct Code {
    name: Option<String>,
    params: Vec<String>,
    body: Vec<Stmt>,
}
struct Function {
    code: Rc<Code>,
    environment: usize,
    properties: usize,
}
enum Reference {
    Binding(usize, String),
    Property(Value, String),
}
enum Flow {
    Normal(Option<Value>),
    Return(Value),
    Break(Option<String>, Option<Value>),
    Continue(Option<String>, Option<Value>),
}
impl Flow {
    fn update_empty(self, previous: Option<Value>) -> Self {
        match self {
            Self::Normal(None) => Self::Normal(previous),
            Self::Break(target, None) => Self::Break(target, previous),
            Self::Continue(target, None) => Self::Continue(target, previous),
            other => other,
        }
    }
}
enum LoopStep {
    Next,
    Stop,
    Abrupt(Flow),
}
fn loop_step(flow: Flow, last: &mut Option<Value>, labels: &[&str]) -> LoopStep {
    let matches = |target: &Option<String>| {
        target
            .as_ref()
            .is_none_or(|target| labels.contains(&target.as_str()))
    };
    match flow.update_empty(last.take()) {
        Flow::Normal(value) => {
            *last = value;
            LoopStep::Next
        }
        Flow::Continue(target, value) if matches(&target) => {
            *last = value;
            LoopStep::Next
        }
        Flow::Break(target, value) if matches(&target) => {
            *last = value;
            LoopStep::Stop
        }
        other => LoopStep::Abrupt(other),
    }
}

pub struct Runtime {
    objects: Vec<Object>,
    functions: Vec<Function>,
    environments: Vec<Environment>,
    native_properties: Vec<(String, usize)>,
    budget: Budget,
    fatal: Option<String>,
    random: u64,
    object_prototype: usize,
    array_prototype: usize,
    function_prototype: usize,
    string_prototype: usize,
    number_prototype: usize,
    boolean_prototype: usize,
    global_declarations: Vec<String>,
    global_setters: Vec<(String, String, String)>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn new() -> Self {
        let mut runtime = Self {
            objects: Vec::new(),
            functions: Vec::new(),
            environments: vec![Environment {
                parent: None,
                variable: 0,
                bindings: Vec::new(),
            }],
            native_properties: Vec::new(),
            budget: Budget {
                fuel: MAX_FUEL,
                allocated: 128,
                calls: 0,
            },
            fatal: None,
            random: 0x9e3779b97f4a7c15,
            object_prototype: 1,
            array_prototype: 2,
            function_prototype: 3,
            string_prototype: 4,
            number_prototype: 5,
            boolean_prototype: 6,
            global_declarations: Vec::new(),
            global_setters: Vec::new(),
        };
        // The fixed bootstrap is far below all limits and has no host capability.
        for prototype in [Some(1), None, Some(1), Some(1), Some(1), Some(1), Some(1)] {
            runtime
                .object(prototype, None)
                .expect("fixed bounded runtime bootstrap");
        }
        for (id, prefix, methods) in [
            (1, "Object", &["toString", "valueOf", "hasOwnProperty"][..]),
            (
                2,
                "Array",
                &[
                    "push", "pop", "shift", "unshift", "join", "toString", "slice", "concat",
                    "indexOf", "includes", "reverse", "forEach", "map", "filter", "some", "every",
                    "reduce",
                ][..],
            ),
            (3, "Function", &["call", "apply", "toString"][..]),
            (
                4,
                "String",
                &[
                    "charAt",
                    "charCodeAt",
                    "slice",
                    "substring",
                    "substr",
                    "indexOf",
                    "includes",
                    "startsWith",
                    "endsWith",
                    "split",
                    "concat",
                    "trim",
                    "toLowerCase",
                    "toUpperCase",
                    "toString",
                    "valueOf",
                ][..],
            ),
            (5, "Number", &["toString", "valueOf", "toFixed"][..]),
            (6, "Boolean", &["toString", "valueOf"][..]),
        ] {
            for method in methods {
                runtime
                    .put_own(
                        id,
                        method,
                        Value::Native(format!("{prefix}.{method}")),
                        false,
                    )
                    .expect("fixed bootstrap");
            }
        }
        for name in [
            "Object",
            "Array",
            "String",
            "Number",
            "Boolean",
            "Function",
            "parseInt",
            "parseFloat",
            "isNaN",
            "isFinite",
            "encodeURIComponent",
            "decodeURIComponent",
            "encodeURI",
            "decodeURI",
            "eval",
            "Error",
            "TypeError",
            "RangeError",
            "ReferenceError",
            "URIError",
            "SyntaxError",
        ] {
            runtime.set_global(name, Value::Native(name.into()));
        }
        runtime.set_global("undefined", Value::Undefined);
        runtime.set_global("NaN", Value::Number(f64::NAN));
        runtime.set_global("Infinity", Value::Number(f64::INFINITY));
        let math = runtime.object(Some(1), None).expect("fixed bootstrap");
        for name in [
            "abs", "floor", "ceil", "round", "trunc", "sqrt", "pow", "min", "max", "random", "sin",
            "cos", "tan", "log", "exp", "imul", "sign",
        ] {
            runtime
                .put_own(math, name, Value::Native(format!("Math.{name}")), false)
                .expect("fixed bootstrap");
        }
        for (name, value) in [
            ("PI", std::f64::consts::PI),
            ("E", std::f64::consts::E),
            ("LN2", std::f64::consts::LN_2),
        ] {
            runtime
                .put_own(math, name, Value::Number(value), false)
                .expect("fixed bootstrap");
        }
        runtime.set_global("Math", Value::Object(math));
        runtime.set_global("globalThis", Value::Object(0));
        runtime
            .put_own(
                runtime.function_prototype,
                "constructor",
                Value::Native("Function".into()),
                false,
            )
            .expect("fixed bootstrap");
        runtime
    }

    pub fn set_global(&mut self, name: &str, value: Value) {
        if self.fatal.is_some() {
            return;
        }
        if let Err(error) = self.put_own(0, name, value, true) {
            self.fatal = Some(fault_text(error));
        }
    }
    pub fn global_object(&self) -> Value {
        Value::Object(0)
    }
    /// Host inspection returns a copy; it does not execute JS coercions/getters.
    pub fn get_global(&self, name: &str) -> Value {
        self.objects[0]
            .properties
            .iter()
            .find(|p| p.key == name)
            .map(|p| p.value.clone())
            .unwrap_or(Value::Undefined)
    }
    /// Configure a non-configurable host-backed global setter without hardcoding
    /// DOM interfaces. Host initialization through `set_global` bypasses it.
    pub fn set_global_setter(&mut self, name: &str, object: &str, key: &str) {
        if self.fatal.is_some() {
            return;
        }
        if let Err(error) = self
            .budget
            .allocate(128 + name.len() + object.len() + key.len())
        {
            self.fatal = Some(fault_text(error));
            return;
        }
        self.global_setters
            .retain(|(existing, _, _)| existing != name);
        self.global_setters
            .push((name.into(), object.into(), key.into()));
    }
    pub fn invoke(
        &mut self,
        callee: Value,
        this: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, String> {
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        let result = (|| {
            self.budget.allocate(
                value_bytes(&callee)
                    .saturating_add(value_bytes(&this))
                    .saturating_add(args.iter().map(value_bytes).sum::<usize>()),
            )?;
            self.call(callee, this, args, None, host)
        })();
        self.finish(result)
    }

    pub fn execute(&mut self, source: &str, host: &mut impl Host) -> Result<Value, String> {
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        let result = (|| {
            self.budget
                .allocate(128usize.saturating_add(source.len()))?;
            let Program(statements) = self.parse_result(syntax::parse(source))?;
            self.budget.allocate(
                statements
                    .iter()
                    .map(statement_bytes)
                    .fold(0usize, usize::saturating_add),
            )?;
            self.hoist(&statements, 0, 0, false)?;
            match self.statements(&statements, 0, &Value::Object(0), host)? {
                Flow::Normal(value) => Ok(value.unwrap_or(Value::Undefined)),
                _ => Err(exception("SyntaxError: invalid top-level control flow")),
            }
        })();
        self.finish(result)
    }
    fn finish(&mut self, result: Eval<Value>) -> Result<Value, String> {
        match result {
            Ok(value) => Ok(value),
            Err(Fault::Fatal(message)) => {
                self.fatal = Some(message.clone());
                Err(message)
            }
            Err(Fault::Throw(value)) => {
                let text = if let Value::Object(id) = &value {
                    self.objects
                        .get(*id)
                        .and_then(|object| {
                            let name = object.properties.iter().find(|p| p.key == "name")?;
                            let message = object.properties.iter().find(|p| p.key == "message")?;
                            Some(format!(
                                "{}: {}",
                                name.value.as_text(),
                                message.value.as_text()
                            ))
                        })
                        .unwrap_or_else(|| value.as_text())
                } else {
                    value.as_text()
                };
                Err(format!("Uncaught JavaScript exception: {text}"))
            }
        }
    }

    fn uri_result<T>(&mut self, result: Result<T, String>) -> Eval<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) if error == uri::LIMIT_ERROR => Err(Fault::Fatal(error)),
            Err(error) => {
                let value = self.error_object(
                    "URIError",
                    error.strip_prefix("URIError: ").unwrap_or(&error),
                )?;
                Err(Fault::Throw(value))
            }
        }
    }

    fn error_object(&mut self, name: &str, message: &str) -> Eval<Value> {
        let object = self.object(Some(self.object_prototype), None)?;
        let name = self.text(name)?;
        let message = self.text(message)?;
        self.put_own(object, "name", name, false)?;
        self.put_own(object, "message", message, false)?;
        Ok(Value::Object(object))
    }

    fn parse_result<T>(&mut self, result: Result<T, String>) -> Eval<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) if syntax::is_limit_error(&error) => Err(Fault::Fatal(format!(
                "JavaScript parser limit exhausted: {error}"
            ))),
            Err(error) => Err(Fault::Throw(self.error_object("SyntaxError", &error)?)),
        }
    }

    /// The source boundary deliberately rejects unpaired UTF-16; it never
    /// replaces a code unit and then parses a different program successfully.
    fn utf8_source(&mut self, units: &[u16]) -> Eval<String> {
        let mut length = 0usize;
        for character in char::decode_utf16(units.iter().copied()) {
            let Ok(character) = character else {
                return Err(Fault::Throw(self.error_object(
                    "SyntaxError",
                    "Unpaired UTF-16 source code unit is unsupported",
                )?));
            };
            length = length.saturating_add(character.len_utf8());
        }
        self.budget.allocate(length)?;
        String::from_utf16(units).map_err(|_| exception("SyntaxError: invalid UTF-16 source"))
    }

    fn eval_code(
        &mut self,
        input: Value,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let Value::String(units) = input else {
            return Ok(input);
        };
        let source = self.utf8_source(&units)?;
        self.budget.allocate(128)?;
        let Program(statements) = self.parse_result(syntax::parse(&source))?;
        self.budget.allocate(
            statements
                .iter()
                .map(statement_bytes)
                .fold(0usize, usize::saturating_add),
        )?;
        let variable = self.environments[environment].variable;
        self.hoist(&statements, variable, environment, true)?;
        match self.statements(&statements, environment, this, host)? {
            Flow::Normal(value) => Ok(value.unwrap_or(Value::Undefined)),
            _ => Err(Fault::Throw(
                self.error_object("SyntaxError", "Invalid eval control flow")?,
            )),
        }
    }

    fn dynamic_function(&mut self, args: Vec<Value>, host: &mut impl Host) -> Eval<Value> {
        self.budget.allocate(args.len().saturating_mul(32))?;
        let mut parts = Vec::with_capacity(args.len());
        // Complete every ToString before any parameter/body grammar validation.
        for argument in args {
            parts.push(self.units(argument, host)?);
        }
        let body = parts.pop().unwrap_or_default();
        let length = parts
            .iter()
            .map(Vec::len)
            .fold(parts.len().saturating_sub(1), usize::saturating_add);
        self.budget.allocate(length.saturating_mul(2))?;
        let mut parameters = Vec::with_capacity(length);
        for (index, part) in parts.into_iter().enumerate() {
            if index != 0 {
                parameters.push(b',' as u16);
            }
            parameters.extend(part);
        }
        let parameters = self.utf8_source(&parameters)?;
        let body = self.utf8_source(&body)?;
        self.budget.allocate(128)?;
        let parsed = self.parse_result(syntax::parse_function(&parameters, &body))?;
        self.budget.allocate(expression_bytes(&parsed))?;
        let Expr::Function { name, params, body } = parsed else {
            return Err(Fault::Fatal(
                "Invalid dynamic function parser result".into(),
            ));
        };
        // "anonymous" is display metadata, not a self-name lexical binding.
        self.function(name.as_ref(), &params, &body, 0, false)
    }

    fn object(
        &mut self,
        prototype: Option<usize>,
        array: Option<Vec<Option<Value>>>,
    ) -> Eval<usize> {
        if self.objects.len() >= MAX_OBJECTS {
            return Err(Fault::Fatal("JavaScript object limit exhausted".into()));
        }
        if array.as_ref().is_some_and(|items| items.len() > MAX_ARRAY) {
            return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
        }
        self.budget.allocate(
            128 + array
                .as_ref()
                .map_or(0, |items| items.len().saturating_mul(64)),
        )?;
        let id = self.objects.len();
        self.objects.push(Object {
            properties: Vec::new(),
            array,
            prototype,
            boxed: None,
        });
        Ok(id)
    }
    fn environment(&mut self, parent: usize, variable_scope: bool) -> Eval<usize> {
        if self.environments.len() >= MAX_OBJECTS {
            return Err(Fault::Fatal(
                "JavaScript environment limit exhausted".into(),
            ));
        }
        self.budget.allocate(128)?;
        let id = self.environments.len();
        self.environments.push(Environment {
            parent: Some(parent),
            variable: if variable_scope {
                id
            } else {
                self.environments[parent].variable
            },
            bindings: Vec::new(),
        });
        Ok(id)
    }
    fn string(&mut self, value: Vec<u16>) -> Eval<Value> {
        self.budget.allocate(value.len().saturating_mul(2))?;
        Ok(Value::String(value))
    }
    fn text(&mut self, value: &str) -> Eval<Value> {
        self.budget.allocate(value.len().saturating_mul(2))?;
        Ok(Value::text(value))
    }
    fn copy(&mut self, value: &Value) -> Eval<Value> {
        self.budget.copy(value)
    }

    fn put_own(&mut self, object: usize, key: &str, value: Value, enumerable: bool) -> Eval<()> {
        self.budget.step()?;
        let entry = self
            .objects
            .get_mut(object)
            .ok_or_else(|| exception("TypeError: unknown object"))?;
        self.budget.allocate(value_bytes(&value))?;
        if let Some(property) = entry
            .properties
            .iter_mut()
            .find(|property| property.key == key)
        {
            property.value = value;
        } else {
            self.budget.allocate(128 + key.len())?;
            entry.properties.push(Property {
                key: key.into(),
                value,
                enumerable,
            });
        }
        Ok(())
    }
    fn own(&mut self, object: usize, key: &str) -> Eval<Option<Value>> {
        self.budget.step()?;
        let entry = self
            .objects
            .get(object)
            .ok_or_else(|| exception("TypeError: unknown object"))?;
        if let Some(array) = &entry.array {
            if key == "length" {
                return Ok(Some(Value::Number(array.len() as f64)));
            }
            if let Some(index) = array_index(key) {
                return array
                    .get(index)
                    .and_then(Option::as_ref)
                    .map(|value| self.budget.copy(value))
                    .transpose();
            }
        }
        entry
            .properties
            .iter()
            .find(|property| property.key == key)
            .map(|property| self.budget.copy(&property.value))
            .transpose()
    }
    fn lookup(&self, mut environment: usize, name: &str) -> Option<usize> {
        loop {
            if environment == 0 {
                return self.objects[0]
                    .properties
                    .iter()
                    .any(|p| p.key == name)
                    .then_some(0);
            }
            let current = &self.environments[environment];
            if current.bindings.iter().any(|binding| binding.name == name) {
                return Some(environment);
            }
            environment = current.parent.unwrap_or(0);
        }
    }
    fn binding(&mut self, environment: usize, name: &str) -> Eval<Value> {
        if environment == 0 {
            return Ok(self.own(0, name)?.unwrap_or(Value::Undefined));
        }
        let value = self.environments[environment]
            .bindings
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| &binding.value)
            .unwrap_or(&Value::Undefined);
        self.budget.copy(value)
    }
    fn define(&mut self, environment: usize, name: &str, value: Value) -> Eval<()> {
        if environment == 0 {
            return self.put_own(0, name, value, true);
        }
        self.budget.allocate(value_bytes(&value))?;
        let bindings = &mut self.environments[environment].bindings;
        if let Some(previous) = bindings.iter_mut().find(|binding| binding.name == name) {
            previous.value = value;
        } else {
            self.budget.allocate(128 + name.len())?;
            bindings.push(Binding {
                name: name.into(),
                value,
                deletable: false,
            });
        }
        Ok(())
    }
    fn declare(&mut self, environment: usize, name: &str, deletable: bool) -> Eval<()> {
        let exists = if environment == 0 {
            self.objects[0].properties.iter().any(|p| p.key == name)
        } else {
            self.environments[environment]
                .bindings
                .iter()
                .any(|binding| binding.name == name)
        };
        if !exists {
            self.define(environment, name, Value::Undefined)?;
            if environment != 0 {
                self.environments[environment]
                    .bindings
                    .last_mut()
                    .unwrap()
                    .deletable = deletable;
            }
        }
        if environment == 0
            && !exists
            && !deletable
            && !self.global_declarations.iter().any(|key| key == name)
        {
            self.budget.allocate(32 + name.len())?;
            self.global_declarations.push(name.into());
        }
        Ok(())
    }

    fn function(
        &mut self,
        name: Option<&String>,
        params: &[String],
        body: &[Stmt],
        environment: usize,
        self_named: bool,
    ) -> Eval<Value> {
        if self.functions.len() >= MAX_OBJECTS {
            return Err(Fault::Fatal("JavaScript function limit exhausted".into()));
        }
        let bytes = body
            .iter()
            .map(statement_bytes)
            .fold(128usize, usize::saturating_add)
            .saturating_add(params.iter().map(|p| p.len() + 32).sum::<usize>());
        self.budget.allocate(bytes)?;
        let environment = if self_named && name.is_some() {
            self.environment(environment, false)?
        } else {
            environment
        };
        let properties = self.object(Some(self.function_prototype), None)?;
        let prototype = self.object(Some(self.object_prototype), None)?;
        let id = self.functions.len();
        self.functions.push(Function {
            code: Rc::new(Code {
                name: name.cloned(),
                params: params.to_vec(),
                body: body.to_vec(),
            }),
            environment,
            properties,
        });
        self.put_own(properties, "prototype", Value::Object(prototype), false)?;
        self.put_own(prototype, "constructor", Value::Function(id), false)?;
        if self_named && let Some(name) = name {
            self.define(environment, name, Value::Function(id))?;
        }
        Ok(Value::Function(id))
    }
    fn hoist(
        &mut self,
        statements: &[Stmt],
        environment: usize,
        lexical: usize,
        deletable: bool,
    ) -> Eval<()> {
        for statement in statements {
            self.budget.step()?;
            match statement {
                Stmt::Var(bindings) => {
                    for (name, _) in bindings {
                        self.declare(environment, name, deletable)?;
                    }
                }
                Stmt::Function { name, params, body } => {
                    if environment == 0 && self.global_setters.iter().any(|(key, _, _)| key == name)
                    {
                        return Err(exception(
                            "SyntaxError: function declaration conflicts with host global",
                        ));
                    }
                    self.declare(environment, name, deletable)?;
                    // Function declarations may make an existing configurable
                    // global permanent; ordinary var declarations must not.
                    if environment == 0
                        && !deletable
                        && !self.global_declarations.iter().any(|key| key == name)
                    {
                        self.budget.allocate(32 + name.len())?;
                        self.global_declarations.push(name.clone());
                    }
                    let function = self.function(Some(name), params, body, lexical, false)?;
                    self.define(environment, name, function)?;
                }
                Stmt::Block(body) => self.hoist(body, environment, lexical, deletable)?,
                Stmt::Label { body, .. } => {
                    self.hoist(
                        std::slice::from_ref(body.as_ref()),
                        environment,
                        lexical,
                        deletable,
                    )?;
                }
                Stmt::If {
                    consequent,
                    alternate,
                    ..
                } => {
                    self.hoist(
                        std::slice::from_ref(consequent.as_ref()),
                        environment,
                        lexical,
                        deletable,
                    )?;
                    if let Some(alternate) = alternate {
                        self.hoist(
                            std::slice::from_ref(alternate.as_ref()),
                            environment,
                            lexical,
                            deletable,
                        )?;
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => self.hoist(
                    std::slice::from_ref(body.as_ref()),
                    environment,
                    lexical,
                    deletable,
                )?,
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        self.hoist(
                            std::slice::from_ref(init.as_ref()),
                            environment,
                            lexical,
                            deletable,
                        )?;
                    }
                    self.hoist(
                        std::slice::from_ref(body.as_ref()),
                        environment,
                        lexical,
                        deletable,
                    )?;
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    self.hoist(
                        std::slice::from_ref(body.as_ref()),
                        environment,
                        lexical,
                        deletable,
                    )?;
                    if let Some((_, catch)) = catch {
                        self.hoist(
                            std::slice::from_ref(catch.as_ref()),
                            environment,
                            lexical,
                            deletable,
                        )?;
                    }
                    if let Some(finally) = finally {
                        self.hoist(
                            std::slice::from_ref(finally.as_ref()),
                            environment,
                            lexical,
                            deletable,
                        )?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn statements(
        &mut self,
        statements: &[Stmt],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Flow> {
        let mut last = None;
        for statement in statements {
            match self.statement(statement, &[], environment, this, host)? {
                Flow::Normal(Some(value)) => last = Some(value),
                Flow::Normal(None) => {}
                other => return Ok(other.update_empty(last)),
            }
        }
        Ok(Flow::Normal(last))
    }
    fn statement(
        &mut self,
        statement: &Stmt,
        labels: &[&str],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Flow> {
        self.budget.step()?;
        match statement {
            Stmt::Empty | Stmt::Function { .. } => Ok(Flow::Normal(None)),
            Stmt::Expr(expr) => Ok(Flow::Normal(Some(self.expression(
                expr,
                environment,
                this,
                host,
            )?))),
            Stmt::Block(body) => self.statements(body, environment, this, host),
            Stmt::Var(bindings) => {
                for (name, expr) in bindings {
                    if let Some(expr) = expr {
                        let value = self.expression(expr, environment, this, host)?;
                        let target = self.lookup(environment, name).unwrap_or(0);
                        self.write_reference(
                            Reference::Binding(target, name.clone()),
                            value,
                            host,
                        )?;
                    }
                }
                Ok(Flow::Normal(None))
            }
            Stmt::Return(expr) => Ok(Flow::Return(match expr {
                Some(expr) => self.expression(expr, environment, this, host)?,
                None => Value::Undefined,
            })),
            Stmt::Break(target) | Stmt::Continue(target) => {
                self.budget
                    .allocate(target.as_ref().map_or(0, String::len))?;
                Ok(if matches!(statement, Stmt::Break(_)) {
                    Flow::Break(target.clone(), None)
                } else {
                    Flow::Continue(target.clone(), None)
                })
            }
            Stmt::Label { name, body } => {
                self.budget
                    .allocate((labels.len() + 1).saturating_mul(std::mem::size_of::<&str>()))?;
                let mut nested = labels.to_vec();
                nested.push(name.as_str());
                match self.statement(body, &nested, environment, this, host)? {
                    Flow::Break(Some(target), value) if target == *name => Ok(Flow::Normal(value)),
                    other => Ok(other),
                }
            }
            Stmt::Throw(expr) => Err(Fault::Throw(self.expression(
                expr,
                environment,
                this,
                host,
            )?)),
            Stmt::If {
                test,
                consequent,
                alternate,
            } => {
                if self.expression(test, environment, this, host)?.truthy() {
                    self.statement(consequent, &[], environment, this, host)
                } else if let Some(alternate) = alternate {
                    self.statement(alternate, &[], environment, this, host)
                } else {
                    Ok(Flow::Normal(None))
                }
            }
            Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => {
                self.iteration(statement, labels, environment, this, host)
            }
            Stmt::Try { .. } => self.try_statement(statement, environment, this, host),
        }
    }

    // Keep loop and try frames separate from ordinary dispatch: recursive calls
    // must reach the configured call-depth error without large debug-build frames.
    fn iteration(
        &mut self,
        statement: &Stmt,
        labels: &[&str],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Flow> {
        let (body, test, update, test_after_body) = match statement {
            Stmt::While { test, body } => (body, Some(test), None, false),
            Stmt::DoWhile { test, body } => (body, Some(test), None, true),
            Stmt::For {
                init,
                test,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.statement(init, &[], environment, this, host)?;
                }
                (body, test.as_ref(), update.as_ref(), false)
            }
            _ => unreachable!("iteration dispatch only accepts loops"),
        };
        let mut last = None;
        loop {
            if matches!(statement, Stmt::For { .. }) {
                self.budget.step()?;
            }
            if !test_after_body
                && let Some(test) = test
                && !self.expression(test, environment, this, host)?.truthy()
            {
                break;
            }
            let flow = self.statement(body, &[], environment, this, host)?;
            match loop_step(flow, &mut last, labels) {
                LoopStep::Stop => break,
                LoopStep::Next => {}
                LoopStep::Abrupt(flow) => return Ok(flow),
            }
            if let Some(update) = update {
                self.expression(update, environment, this, host)?;
            }
            if test_after_body
                && let Some(test) = test
                && !self.expression(test, environment, this, host)?.truthy()
            {
                break;
            }
        }
        Ok(Flow::Normal(last))
    }

    fn try_statement(
        &mut self,
        statement: &Stmt,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Flow> {
        let Stmt::Try {
            body,
            catch,
            finally,
        } = statement
        else {
            unreachable!("try dispatch only accepts try statements")
        };
        let mut result = self.statement(body, &[], environment, this, host);
        if matches!(result, Err(Fault::Fatal(_))) {
            return result;
        }
        if let Err(Fault::Throw(value)) = result {
            result = if let Some((name, catch)) = catch {
                let environment = self.environment(environment, false)?;
                self.define(environment, name, value)?;
                self.statement(catch, &[], environment, this, host)
            } else {
                Err(Fault::Throw(value))
            };
        }
        if matches!(result, Err(Fault::Fatal(_))) {
            return result;
        }
        if let Some(finally) = finally {
            match self.statement(finally, &[], environment, this, host)? {
                Flow::Normal(_) => {}
                other => return Ok(other),
            }
        }
        result
    }

    fn reference(
        &mut self,
        expr: &Expr,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Reference> {
        match expr {
            Expr::Ident(name) => Ok(Reference::Binding(
                self.lookup(environment, name).unwrap_or(0),
                name.clone(),
            )),
            Expr::Member { object, property } => {
                let object = self.expression(object, environment, this, host)?;
                let property = self.expression(property, environment, this, host)?;
                // Evaluate the key expression, but reject an invalid base before
                // ToPropertyKey can call user code on the resulting key value.
                if matches!(object, Value::Null | Value::Undefined) {
                    return Err(exception("TypeError: property access on null or undefined"));
                }
                let key = self.property_key(property, host)?;
                Ok(Reference::Property(object, key))
            }
            _ => Err(exception("ReferenceError: invalid assignment target")),
        }
    }
    fn read_reference(&mut self, reference: &Reference, host: &mut impl Host) -> Eval<Value> {
        match reference {
            Reference::Binding(environment, name) => {
                if self.lookup(*environment, name).is_none() {
                    return Err(exception(format!("ReferenceError: {name} is not defined")));
                }
                self.binding(*environment, name)
            }
            Reference::Property(object, key) => self.get(object, key, host),
        }
    }
    fn write_reference(
        &mut self,
        reference: Reference,
        value: Value,
        host: &mut impl Host,
    ) -> Eval<()> {
        match reference {
            Reference::Binding(0, name) => self.set(Value::Object(0), &name, value, host),
            Reference::Binding(environment, name) => self.define(environment, &name, value),
            Reference::Property(object, key) => self.set(object, &key, value, host),
        }
    }
    fn expression(
        &mut self,
        expr: &Expr,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        self.budget.step()?;
        match expr {
            Expr::Undefined => Ok(Value::Undefined),
            Expr::Null => Ok(Value::Null),
            Expr::Bool(v) => Ok(Value::Bool(*v)),
            Expr::Number(v) => Ok(Value::Number(*v)),
            Expr::String(value) => {
                self.budget.allocate(value.len().saturating_mul(2))?;
                Ok(Value::String(value.clone()))
            }
            Expr::Ident(name) => match self.lookup(environment, name) {
                Some(environment) => self.binding(environment, name),
                None => Err(exception(format!("ReferenceError: {name} is not defined"))),
            },
            Expr::This => self.copy(this),
            Expr::Array(items) => {
                if items.len() > MAX_ARRAY {
                    return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
                }
                self.budget.allocate(items.len().saturating_mul(64))?;
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(
                        item.as_ref()
                            .map(|expr| self.expression(expr, environment, this, host))
                            .transpose()?,
                    );
                }
                Ok(Value::Object(
                    self.object(Some(self.array_prototype), Some(values))?,
                ))
            }
            Expr::Object(properties) => {
                let object = self.object(Some(self.object_prototype), None)?;
                for (key, expr) in properties {
                    if key == "__proto__" {
                        return Err(unsupported("object-literal __proto__ setters"));
                    }
                    let value = self.expression(expr, environment, this, host)?;
                    self.put_own(object, key, value, true)?;
                }
                Ok(Value::Object(object))
            }
            Expr::Function { name, params, body } => {
                self.function(name.as_ref(), params, body, environment, true)
            }
            Expr::Member { .. } => {
                let reference = self.reference(expr, environment, this, host)?;
                self.read_reference(&reference, host)
            }
            Expr::Unary { op, expr } => {
                if op == "typeof"
                    && let Expr::Ident(name) = expr.as_ref()
                    && self.lookup(environment, name).is_none()
                {
                    return self.text("undefined");
                }
                if op == "delete" {
                    return match expr.as_ref() {
                        Expr::Ident(name) => match self.lookup(environment, name) {
                            None => Ok(Value::Bool(true)),
                            Some(0) => self.delete(Value::Object(0), name),
                            Some(environment) => {
                                let bindings = &mut self.environments[environment].bindings;
                                let index = bindings
                                    .iter()
                                    .position(|binding| binding.name == *name)
                                    .unwrap();
                                if bindings[index].deletable {
                                    bindings.remove(index);
                                    Ok(Value::Bool(true))
                                } else {
                                    Ok(Value::Bool(false))
                                }
                            }
                        },
                        Expr::Member { .. } => {
                            let reference = self.reference(expr, environment, this, host)?;
                            if let Reference::Property(object, key) = reference {
                                self.delete(object, &key)
                            } else {
                                unreachable!()
                            }
                        }
                        _ => {
                            self.expression(expr, environment, this, host)?;
                            Ok(Value::Bool(true))
                        }
                    };
                }
                let value = self.expression(expr, environment, this, host)?;
                match op.as_str() {
                    "!" => Ok(Value::Bool(!value.truthy())),
                    "void" => Ok(Value::Undefined),
                    "typeof" => self.text(match value {
                        Value::Undefined => "undefined",
                        Value::Bool(_) => "boolean",
                        Value::Number(_) => "number",
                        Value::String(_) => "string",
                        Value::Function(_) | Value::Native(_) => "function",
                        _ => "object",
                    }),
                    "+" => Ok(Value::Number(self.number(value, host)?)),
                    "-" => Ok(Value::Number(-self.number(value, host)?)),
                    "~" => Ok(Value::Number((!int32(self.number(value, host)?)) as f64)),
                    _ => Err(unsupported(op)),
                }
            }
            Expr::Binary { op, left, right } => {
                let left = self.expression(left, environment, this, host)?;
                if op == "&&" {
                    return if left.truthy() {
                        self.expression(right, environment, this, host)
                    } else {
                        Ok(left)
                    };
                }
                if op == "||" {
                    return if left.truthy() {
                        Ok(left)
                    } else {
                        self.expression(right, environment, this, host)
                    };
                }
                if op == "??" {
                    return if matches!(left, Value::Null | Value::Undefined) {
                        self.expression(right, environment, this, host)
                    } else {
                        Ok(left)
                    };
                }
                let right = self.expression(right, environment, this, host)?;
                self.binary(op, left, right, host)
            }
            Expr::Assign { op, left, right } => {
                let reference = self.reference(left, environment, this, host)?;
                let old = if op != "=" {
                    Some(self.read_reference(&reference, host)?)
                } else {
                    None
                };
                if let Some(value) = &old {
                    if op == "&&=" && !value.truthy()
                        || op == "||=" && value.truthy()
                        || op == "??=" && !matches!(value, Value::Null | Value::Undefined)
                    {
                        return self.copy(value);
                    }
                }
                let right = self.expression(right, environment, this, host)?;
                let value = if matches!(op.as_str(), "=" | "&&=" | "||=" | "??=") {
                    right
                } else {
                    self.binary(
                        op.strip_suffix('=').ok_or_else(|| unsupported(op))?,
                        old.unwrap(),
                        right,
                        host,
                    )?
                };
                let result = self.copy(&value)?;
                self.write_reference(reference, value, host)?;
                Ok(result)
            }
            Expr::Update { op, expr, prefix } => {
                let reference = self.reference(expr, environment, this, host)?;
                let old = self.read_reference(&reference, host)?;
                let old = self.number(old, host)?;
                let new = if op == "++" {
                    old + 1.0
                } else if op == "--" {
                    old - 1.0
                } else {
                    return Err(unsupported(op));
                };
                self.write_reference(reference, Value::Number(new), host)?;
                Ok(Value::Number(if *prefix { new } else { old }))
            }
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                if self.expression(test, environment, this, host)?.truthy() {
                    self.expression(consequent, environment, this, host)
                } else {
                    self.expression(alternate, environment, this, host)
                }
            }
            Expr::Sequence(expressions) => {
                let mut result = Value::Undefined;
                for expression in expressions {
                    result = self.expression(expression, environment, this, host)?;
                }
                Ok(result)
            }
            Expr::Call { callee, args } => {
                let eval_reference = matches!(callee.as_ref(), Expr::Ident(name) if name == "eval");
                let (callee, mut receiver) = if matches!(callee.as_ref(), Expr::Member { .. }) {
                    let reference = self.reference(callee, environment, this, host)?;
                    let callee = self.read_reference(&reference, host)?;
                    let receiver = if let Reference::Property(value, _) = reference {
                        value
                    } else {
                        unreachable!()
                    };
                    (callee, receiver)
                } else {
                    (
                        self.expression(callee, environment, this, host)?,
                        Value::Object(0),
                    )
                };
                let direct_eval =
                    if eval_reference && matches!(&callee, Value::Native(name) if name == "eval") {
                        receiver = self.copy(this)?;
                        Some(environment)
                    } else {
                        None
                    };
                let args = self.arguments(args, environment, this, host)?;
                self.call(callee, receiver, args, direct_eval, host)
            }
            Expr::New { callee, args } => {
                let callee = self.expression(callee, environment, this, host)?;
                let args = self.arguments(args, environment, this, host)?;
                if let Value::Native(name) = &callee {
                    if matches!(
                        name.as_str(),
                        "Array"
                            | "Object"
                            | "Function"
                            | "Error"
                            | "TypeError"
                            | "RangeError"
                            | "ReferenceError"
                            | "URIError"
                            | "SyntaxError"
                    ) {
                        return self.call(callee, Value::Undefined, args, None, host);
                    }
                    return Err(unsupported("this native constructor"));
                }
                if !matches!(callee, Value::Function(_)) {
                    return Err(exception("TypeError: value is not a constructor"));
                }
                let prototype = match self.get(&callee, "prototype", host)? {
                    Value::Object(id) => id,
                    _ => self.object_prototype,
                };
                let object = Value::Object(self.object(Some(prototype), None)?);
                let receiver = self.copy(&object)?;
                let result = self.call(callee, receiver, args, None, host)?;
                Ok(if result.primitive() { object } else { result })
            }
        }
    }
    fn arguments(
        &mut self,
        args: &[Expr],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Vec<Value>> {
        if args.len() > MAX_ARRAY {
            return Err(Fault::Fatal("JavaScript argument limit exhausted".into()));
        }
        self.budget.allocate(args.len().saturating_mul(64))?;
        args.iter()
            .map(|arg| self.expression(arg, environment, this, host))
            .collect()
    }

    fn get(&mut self, value: &Value, key: &str, host: &mut impl Host) -> Eval<Value> {
        self.budget.step()?;
        let prototype = match value {
            Value::Undefined | Value::Null => {
                return Err(exception("TypeError: property access on null or undefined"));
            }
            Value::Host(object) => {
                let value = host.get(object, key).map_err(exception)?;
                self.budget.allocate(value_bytes(&value))?;
                return Ok(value);
            }
            Value::Object(id) => {
                if let Some(value) = self.own(*id, key)? {
                    return Ok(value);
                }
                if let Some(Value::String(units)) = &self.objects[*id].boxed {
                    if key == "length" {
                        return Ok(Value::Number(units.len() as f64));
                    }
                    if let Some(index) = array_index(key)
                        && let Some(unit) = units.get(index).copied()
                    {
                        return self.string(vec![unit]);
                    }
                }
                self.objects[*id].prototype
            }
            Value::Function(id) => {
                let function = self
                    .functions
                    .get(*id)
                    .ok_or_else(|| exception("TypeError: unknown function"))?;
                if key == "length" {
                    return Ok(Value::Number(function.code.params.len() as f64));
                }
                if key == "name" {
                    let name = function.code.name.clone().unwrap_or_default();
                    return self.text(&name);
                }
                Some(function.properties)
            }
            Value::String(units) => {
                if key == "length" {
                    return Ok(Value::Number(units.len() as f64));
                }
                if let Some(index) = array_index(key) {
                    return match units.get(index) {
                        Some(unit) => self.string(vec![*unit]),
                        None => Ok(Value::Undefined),
                    };
                }
                Some(self.string_prototype)
            }
            Value::Number(_) => Some(self.number_prototype),
            Value::Bool(_) => Some(self.boolean_prototype),
            Value::Native(name) => {
                if let Some((_, object)) = self
                    .native_properties
                    .iter()
                    .find(|(existing, _)| existing == name)
                {
                    let object = *object;
                    if let Some(value) = self.own(object, key)? {
                        return Ok(value);
                    }
                }
                if key == "name" {
                    return self.text(name.rsplit('.').next().unwrap_or(name));
                }
                if key == "prototype" {
                    return Ok(match name.as_str() {
                        "Object" => Value::Object(self.object_prototype),
                        "Array" => Value::Object(self.array_prototype),
                        "Function" => Value::Object(self.function_prototype),
                        "String" => Value::Object(self.string_prototype),
                        "Number" => Value::Object(self.number_prototype),
                        "Boolean" => Value::Object(self.boolean_prototype),
                        _ => Value::Undefined,
                    });
                }
                if matches!(
                    (name.as_str(), key),
                    ("Array", "isArray")
                        | ("String", "fromCharCode")
                        | (
                            "Object",
                            "keys" | "create" | "getPrototypeOf" | "getOwnPropertyNames"
                        )
                        | ("Number", "isNaN" | "isFinite" | "isInteger")
                ) {
                    return Ok(Value::Native(format!("{name}.{key}")));
                }
                if key == "length" {
                    return Ok(Value::Number(if name == "parseInt" { 2.0 } else { 1.0 }));
                }
                Some(self.function_prototype)
            }
        };
        let mut current = prototype;
        for _ in 0..MAX_CALLS {
            let Some(id) = current else {
                return Ok(Value::Undefined);
            };
            if let Some(value) = self.own(id, key)? {
                return Ok(value);
            }
            current = self.objects[id].prototype;
        }
        Err(Fault::Fatal(
            "JavaScript prototype depth limit exhausted".into(),
        ))
    }
    fn set(&mut self, object: Value, key: &str, value: Value, host: &mut impl Host) -> Eval<()> {
        self.budget.step()?;
        match object {
            Value::Host(object) => host.set(&object, key, value).map_err(exception),
            Value::Object(id) => {
                if id == 0
                    && let Some((_, object, property)) =
                        self.global_setters.iter().find(|(name, _, _)| name == key)
                {
                    return host.set(object, property, value).map_err(exception);
                }
                if id == 0 && matches!(key, "undefined" | "NaN" | "Infinity") {
                    return Ok(());
                }
                let array = self
                    .objects
                    .get(id)
                    .ok_or_else(|| exception("TypeError: unknown object"))?
                    .array
                    .is_some();
                if array && key == "length" {
                    let length = self.number(value, host)?;
                    if !length.is_finite()
                        || length < 0.0
                        || length.fract() != 0.0
                        || length > u32::MAX as f64
                    {
                        return Err(exception("RangeError: invalid array length"));
                    }
                    if length > MAX_ARRAY as f64 {
                        return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
                    }
                    let array = self.objects[id].array.as_mut().unwrap();
                    if length as usize > array.len() {
                        self.budget
                            .allocate((length as usize - array.len()).saturating_mul(64))?;
                    }
                    array.resize_with(length as usize, || None);
                    return Ok(());
                }
                if array && let Some(index) = array_index(key) {
                    if index >= MAX_ARRAY {
                        return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
                    }
                    let array = self.objects[id].array.as_mut().unwrap();
                    self.budget.allocate(value_bytes(&value))?;
                    if index >= array.len() {
                        self.budget
                            .allocate((index + 1 - array.len()).saturating_mul(64))?;
                        array.resize_with(index + 1, || None);
                    }
                    array[index] = Some(value);
                    return Ok(());
                }
                self.put_own(id, key, value, true)
            }
            Value::Function(id) => {
                if matches!(key, "length" | "name") {
                    return Ok(());
                }
                let object = self
                    .functions
                    .get(id)
                    .ok_or_else(|| exception("TypeError: unknown function"))?
                    .properties;
                self.put_own(object, key, value, true)
            }
            Value::Native(name) => {
                let object = if let Some((_, id)) = self
                    .native_properties
                    .iter()
                    .find(|(existing, _)| *existing == name)
                {
                    *id
                } else {
                    let id = self.object(Some(self.function_prototype), None)?;
                    self.budget.allocate(64 + name.len())?;
                    self.native_properties.push((name, id));
                    id
                };
                self.put_own(object, key, value, true)
            }
            Value::Null | Value::Undefined => Err(exception(
                "TypeError: property assignment on null or undefined",
            )),
            _ => Ok(()), // Non-strict assignment to transient primitive wrappers.
        }
    }
    fn delete(&mut self, object: Value, key: &str) -> Eval<Value> {
        let id = match object {
            Value::Object(id) => id,
            Value::Function(id) => {
                self.functions
                    .get(id)
                    .ok_or_else(|| exception("TypeError: unknown function"))?
                    .properties
            }
            Value::Host(_) => return Err(unsupported("deleting host properties")),
            Value::Null | Value::Undefined => {
                return Err(exception("TypeError: delete on null or undefined"));
            }
            _ => return Ok(Value::Bool(true)),
        };
        if id == 0
            && (self.global_declarations.iter().any(|name| name == key)
                || self.global_setters.iter().any(|(name, _, _)| name == key)
                || matches!(key, "undefined" | "NaN" | "Infinity"))
        {
            return Ok(Value::Bool(false));
        }
        let object = self
            .objects
            .get_mut(id)
            .ok_or_else(|| exception("TypeError: unknown object"))?;
        if let Some(array) = &mut object.array {
            if key == "length" {
                return Ok(Value::Bool(false));
            }
            if let Some(index) = array_index(key) {
                if let Some(item) = array.get_mut(index) {
                    *item = None;
                }
                return Ok(Value::Bool(true));
            }
        }
        object.properties.retain(|property| property.key != key);
        Ok(Value::Bool(true))
    }
    fn property_key(&mut self, value: Value, host: &mut impl Host) -> Eval<String> {
        let value = self.primitive(value, false, host)?;
        match value {
            Value::String(units) => {
                String::from_utf16(&units).map_err(|_| unsupported("lone-surrogate property keys"))
            }
            other => Ok(other.as_text()),
        }
    }
    fn primitive(&mut self, value: Value, number_hint: bool, host: &mut impl Host) -> Eval<Value> {
        if value.primitive() {
            return Ok(value);
        }
        for key in if number_hint {
            ["valueOf", "toString"]
        } else {
            ["toString", "valueOf"]
        } {
            let method = self.get(&value, key, host)?;
            if method.callable() {
                let receiver = self.copy(&value)?;
                let result = self.call(method, receiver, Vec::new(), None, host)?;
                if result.primitive() {
                    return Ok(result);
                }
            }
        }
        Err(exception(
            "TypeError: object cannot be converted to a primitive",
        ))
    }
    fn number(&mut self, value: Value, host: &mut impl Host) -> Eval<f64> {
        Ok(primitive_number(&self.primitive(value, true, host)?))
    }
    fn units(&mut self, value: Value, host: &mut impl Host) -> Eval<Vec<u16>> {
        let value = self.primitive(value, false, host)?;
        match value {
            Value::String(units) => Ok(units),
            other => {
                let text = other.as_text();
                self.budget.allocate(text.len().saturating_mul(2))?;
                Ok(text.encode_utf16().collect())
            }
        }
    }
    fn binary(&mut self, op: &str, left: Value, right: Value, host: &mut impl Host) -> Eval<Value> {
        self.budget.step()?;
        match op {
            "===" => return Ok(Value::Bool(strict_equal(&left, &right))),
            "!==" => return Ok(Value::Bool(!strict_equal(&left, &right))),
            "==" | "!=" => {
                let equal = self.equal(left, right, host)?;
                return Ok(Value::Bool(if op == "==" { equal } else { !equal }));
            }
            "in" => {
                let key = self.property_key(left, host)?;
                let mut object = match right {
                    Value::Object(id) => Some(id),
                    Value::Function(id) => Some(self.functions[id].properties),
                    Value::Host(_) => return Err(unsupported("in on host objects")),
                    _ => return Err(exception("TypeError: right side of in is not an object")),
                };
                for _ in 0..MAX_CALLS {
                    let Some(id) = object else {
                        return Ok(Value::Bool(false));
                    };
                    if self.own(id, &key)?.is_some() {
                        return Ok(Value::Bool(true));
                    }
                    object = self.objects[id].prototype;
                }
                return Err(Fault::Fatal(
                    "JavaScript prototype depth limit exhausted".into(),
                ));
            }
            "instanceof" => {
                if !right.callable() {
                    return Err(exception(
                        "TypeError: right side of instanceof is not callable",
                    ));
                }
                let Value::Object(target) = self.get(&right, "prototype", host)? else {
                    return Err(exception(
                        "TypeError: constructor prototype is not an object",
                    ));
                };
                let mut current = match left {
                    Value::Object(id) => self.objects[id].prototype,
                    Value::Function(id) => Some(self.functions[id].properties),
                    _ => None,
                };
                for _ in 0..MAX_CALLS {
                    let Some(id) = current else {
                        return Ok(Value::Bool(false));
                    };
                    if id == target {
                        return Ok(Value::Bool(true));
                    }
                    current = self.objects[id].prototype;
                }
                return Err(Fault::Fatal(
                    "JavaScript prototype depth limit exhausted".into(),
                ));
            }
            _ => {}
        }
        let left = self.primitive(left, true, host)?;
        let right = self.primitive(right, true, host)?;
        if op == "+" && (matches!(left, Value::String(_)) || matches!(right, Value::String(_))) {
            let mut a = self.units(left, host)?;
            let b = self.units(right, host)?;
            self.budget
                .allocate(a.len().saturating_add(b.len()).saturating_mul(2))?;
            a.extend(b);
            return Ok(Value::String(a));
        }
        if matches!(op, "<" | ">" | "<=" | ">=") {
            let order = match (&left, &right) {
                (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
                _ => primitive_number(&left).partial_cmp(&primitive_number(&right)),
            };
            return Ok(Value::Bool(order.is_some_and(|order| match op {
                "<" => order.is_lt(),
                ">" => order.is_gt(),
                "<=" => !order.is_gt(),
                _ => !order.is_lt(),
            })));
        }
        let (a, b) = (primitive_number(&left), primitive_number(&right));
        Ok(Value::Number(match op {
            "+" => a + b,
            "-" => a - b,
            "*" => a * b,
            "/" => a / b,
            "%" => a % b,
            "**" => a.powf(b),
            "&" => (int32(a) & int32(b)) as f64,
            "|" => (int32(a) | int32(b)) as f64,
            "^" => (int32(a) ^ int32(b)) as f64,
            "<<" => int32(a).wrapping_shl((int32(b) as u32) & 31) as f64,
            ">>" => (int32(a) >> ((int32(b) as u32) & 31)) as f64,
            ">>>" => ((int32(a) as u32) >> ((int32(b) as u32) & 31)) as f64,
            _ => return Err(unsupported(op)),
        }))
    }
    fn equal(&mut self, left: Value, right: Value, host: &mut impl Host) -> Eval<bool> {
        if strict_equal(&left, &right) {
            return Ok(true);
        }
        if matches!(
            (&left, &right),
            (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null)
        ) {
            return Ok(true);
        }
        match (&left, &right) {
            (Value::Number(_), Value::String(_)) | (Value::String(_), Value::Number(_)) => {
                Ok(primitive_number(&left) == primitive_number(&right))
            }
            (Value::Bool(value), _) => {
                self.equal(Value::Number(if *value { 1.0 } else { 0.0 }), right, host)
            }
            (_, Value::Bool(value)) => {
                self.equal(left, Value::Number(if *value { 1.0 } else { 0.0 }), host)
            }
            _ if !left.primitive() && matches!(right, Value::Number(_) | Value::String(_)) => {
                let left = self.primitive(left, true, host)?;
                self.equal(left, right, host)
            }
            _ if !right.primitive() && matches!(left, Value::Number(_) | Value::String(_)) => {
                let right = self.primitive(right, true, host)?;
                self.equal(left, right, host)
            }
            _ => Ok(false),
        }
    }
    fn boxed(&mut self, value: Value) -> Eval<Value> {
        let prototype = match value {
            Value::String(_) => self.string_prototype,
            Value::Number(_) => self.number_prototype,
            Value::Bool(_) => self.boolean_prototype,
            _ => return Ok(value),
        };
        let object = self.object(Some(prototype), None)?;
        self.budget.allocate(value_bytes(&value))?;
        self.objects[object].boxed = Some(value);
        Ok(Value::Object(object))
    }
    fn unboxed(&mut self, value: Value) -> Eval<Value> {
        if let Value::Object(id) = &value
            && let Some(primitive) = self
                .objects
                .get(*id)
                .and_then(|object| object.boxed.as_ref())
        {
            return self.budget.copy(primitive);
        }
        Ok(value)
    }
    fn call(
        &mut self,
        callee: Value,
        this: Value,
        args: Vec<Value>,
        direct_eval: Option<usize>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        self.budget.step()?;
        if self.budget.calls >= MAX_CALLS {
            return Err(Fault::Fatal("JavaScript call depth exhausted".into()));
        }
        if args.len() > MAX_ARRAY {
            return Err(Fault::Fatal("JavaScript argument limit exhausted".into()));
        }
        self.budget.calls += 1;
        let result = (|| match callee {
            Value::Native(name) if name == "eval" => {
                let environment = direct_eval.unwrap_or(0);
                let this = if direct_eval.is_some() {
                    this
                } else {
                    Value::Object(0)
                };
                self.eval_code(
                    args.into_iter().next().unwrap_or(Value::Undefined),
                    environment,
                    &this,
                    host,
                )
            }
            Value::Native(name) if name.starts_with("host.") => {
                let value = host.call(&name, this, args).map_err(exception)?;
                self.budget.allocate(value_bytes(&value))?;
                Ok(value)
            }
            Value::Native(name) => self.native(&name, this, args, host),
            Value::Function(id) => {
                let function = self
                    .functions
                    .get(id)
                    .ok_or_else(|| exception("TypeError: unknown function"))?;
                let code = function.code.clone();
                let parent = function.environment;
                let environment = self.environment(parent, true)?;
                let this = if matches!(this, Value::Undefined | Value::Null) {
                    Value::Object(0)
                } else {
                    self.boxed(this)?
                };
                for (index, param) in code.params.iter().enumerate() {
                    let argument = self.copy(args.get(index).unwrap_or(&Value::Undefined))?;
                    self.define(environment, param, argument)?;
                }
                if !code.params.iter().any(|name| name == "arguments") {
                    let mut items = Vec::with_capacity(args.len());
                    self.budget.allocate(args.len().saturating_mul(64))?;
                    for arg in &args {
                        items.push(Some(self.copy(arg)?));
                    }
                    let arguments = self.object(Some(self.array_prototype), Some(items))?;
                    self.put_own(arguments, "callee", Value::Function(id), false)?;
                    self.define(environment, "arguments", Value::Object(arguments))?;
                }
                self.hoist(&code.body, environment, environment, false)?;
                match self.statements(&code.body, environment, &this, host)? {
                    Flow::Return(value) => Ok(value),
                    Flow::Normal(_) => Ok(Value::Undefined),
                    _ => Err(exception("SyntaxError: invalid function control flow")),
                }
            }
            _ => Err(exception("TypeError: value is not callable")),
        })();
        self.budget.calls -= 1;
        result
    }

    fn native(
        &mut self,
        name: &str,
        this: Value,
        mut args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let first = self.copy(args.first().unwrap_or(&Value::Undefined))?;
        match name {
            "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent" => {
                let input = self.units(first, host)?;
                let component = name.ends_with("Component");
                let output = if name.starts_with("encode") {
                    let length = self.uri_result(uri::encoded_len(&input, component))?;
                    self.budget.allocate(length.saturating_mul(2))?;
                    self.uri_result(uri::encode(&input, component))?
                } else {
                    if input.len() > uri::MAX_UNITS {
                        return Err(Fault::Fatal(uri::LIMIT_ERROR.into()));
                    }
                    self.budget.allocate(input.len().saturating_mul(2))?;
                    self.uri_result(uri::decode(&input, component))?
                };
                // Output allocation was charged before the helper allocated it.
                Ok(Value::String(output))
            }
            "Function" => self.dynamic_function(args, host),
            "String" => {
                if args.is_empty() {
                    self.text("")
                } else {
                    let units = self.units(first, host)?;
                    self.string(units)
                }
            }
            "Number" => Ok(Value::Number(if args.is_empty() {
                0.0
            } else {
                self.number(first, host)?
            })),
            "Boolean" => Ok(Value::Bool(first.truthy())),
            "Object" => {
                if matches!(first, Value::Null | Value::Undefined) {
                    Ok(Value::Object(
                        self.object(Some(self.object_prototype), None)?,
                    ))
                } else {
                    self.boxed(first)
                }
            }
            "Array" => {
                let array = if args.len() == 1 && matches!(first, Value::Number(_)) {
                    let length = primitive_number(&first);
                    if !length.is_finite() || length < 0.0 || length.fract() != 0.0 {
                        return Err(exception("RangeError: invalid array length"));
                    }
                    if length > MAX_ARRAY as f64 {
                        return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
                    }
                    self.budget.allocate((length as usize).saturating_mul(64))?;
                    (0..length as usize).map(|_| None).collect()
                } else {
                    args.into_iter().map(Some).collect()
                };
                Ok(Value::Object(
                    self.object(Some(self.array_prototype), Some(array))?,
                ))
            }
            "Array.isArray" => Ok(Value::Bool(
                matches!(first, Value::Object(id) if self.objects.get(id).is_some_and(|object| object.array.is_some())),
            )),
            "Function.call" => {
                let receiver = if args.is_empty() {
                    Value::Undefined
                } else {
                    args.remove(0)
                };
                self.call(this, receiver, args, None, host)
            }
            "Function.apply" => {
                let list = self.copy(args.get(1).unwrap_or(&Value::Undefined))?;
                let mut values = Vec::new();
                if !matches!(list, Value::Null | Value::Undefined) {
                    if list.primitive() {
                        return Err(exception("TypeError: apply arguments must be array-like"));
                    }
                    let length = self.get(&list, "length", host)?;
                    let length = self.number(length, host)?.max(0.0).floor();
                    if !length.is_finite() || length > MAX_ARRAY as f64 {
                        return Err(Fault::Fatal("JavaScript argument limit exhausted".into()));
                    }
                    self.budget.allocate((length as usize).saturating_mul(64))?;
                    for index in 0..length as usize {
                        values.push(self.get(&list, &index.to_string(), host)?);
                    }
                }
                self.call(this, first, values, None, host)
            }
            "Function.toString" => {
                if !this.callable() {
                    return Err(exception("TypeError: not a function"));
                }
                self.text(&this.as_text())
            }
            "Object.valueOf" => {
                if matches!(this, Value::Null | Value::Undefined) {
                    Err(exception("TypeError: valueOf on null or undefined"))
                } else {
                    self.boxed(this)
                }
            }
            "Object.toString" => self.text(match this {
                Value::Undefined => "[object Undefined]",
                Value::Null => "[object Null]",
                Value::Object(id) if self.objects.get(id).is_some_and(|o| o.array.is_some()) => {
                    "[object Array]"
                }
                Value::Function(_) | Value::Native(_) => "[object Function]",
                Value::String(_) => "[object String]",
                Value::Number(_) => "[object Number]",
                Value::Bool(_) => "[object Boolean]",
                _ => "[object Object]",
            }),
            "Object.hasOwnProperty" => {
                let key = self.property_key(first, host)?;
                let id = match this {
                    Value::Object(id) => id,
                    Value::Function(id) => self.functions[id].properties,
                    Value::Host(_) => return Err(unsupported("hasOwnProperty on host objects")),
                    _ => return Ok(Value::Bool(false)),
                };
                Ok(Value::Bool(self.own(id, &key)?.is_some()))
            }
            "Object.keys" | "Object.getOwnPropertyNames" => {
                let id = match first {
                    Value::Object(id) => id,
                    Value::Function(id) => self.functions[id].properties,
                    _ => return Err(unsupported("Object keys on primitive/host values")),
                };
                let object = &self.objects[id];
                let mut keys = Vec::new();
                if let Some(array) = &object.array {
                    for (index, value) in array.iter().enumerate() {
                        if value.is_some() {
                            keys.push(index.to_string());
                        }
                    }
                    if name.ends_with("Names") {
                        keys.push("length".into());
                    }
                }
                for property in &object.properties {
                    if property.enumerable || name.ends_with("Names") {
                        keys.push(property.key.clone());
                    }
                }
                let mut values = Vec::new();
                for key in keys {
                    values.push(Some(self.text(&key)?));
                }
                Ok(Value::Object(
                    self.object(Some(self.array_prototype), Some(values))?,
                ))
            }
            "Object.create" => {
                if args.len() > 1 && !matches!(args[1], Value::Undefined) {
                    return Err(unsupported("Object.create property descriptors"));
                }
                let prototype = match first {
                    Value::Null => None,
                    Value::Object(id) => Some(id),
                    _ => return Err(exception("TypeError: prototype must be an object or null")),
                };
                Ok(Value::Object(self.object(prototype, None)?))
            }
            "Object.getPrototypeOf" => {
                let id = match first {
                    Value::Object(id) => id,
                    Value::Function(id) => self.functions[id].properties,
                    _ => return Err(unsupported("getPrototypeOf on primitive/host values")),
                };
                Ok(self.objects[id]
                    .prototype
                    .map(Value::Object)
                    .unwrap_or(Value::Null))
            }
            "String.fromCharCode" => {
                let mut units = Vec::new();
                for value in args {
                    units.push(int32(self.number(value, host)?) as u16);
                }
                self.string(units)
            }
            "Number.isNaN" => Ok(Value::Bool(
                matches!(first, Value::Number(value) if value.is_nan()),
            )),
            "Number.isFinite" => Ok(Value::Bool(
                matches!(first, Value::Number(value) if value.is_finite()),
            )),
            "Number.isInteger" => Ok(Value::Bool(
                matches!(first, Value::Number(value) if value.is_finite() && value.fract() == 0.0),
            )),
            "isNaN" => Ok(Value::Bool(self.number(first, host)?.is_nan())),
            "isFinite" => Ok(Value::Bool(self.number(first, host)?.is_finite())),
            "parseInt" => {
                let units = self.units(first, host)?;
                let text = String::from_utf16_lossy(&units);
                let radix = self.copy(args.get(1).unwrap_or(&Value::Undefined))?;
                let radix = int32(self.number(radix, host)?);
                Ok(Value::Number(parse_integer(
                    &text,
                    radix,
                    &mut self.budget,
                )?))
            }
            "parseFloat" => {
                let units = self.units(first, host)?;
                let text = String::from_utf16_lossy(&units);
                Ok(Value::Number(parse_float(&text, &mut self.budget)?))
            }
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "URIError"
            | "SyntaxError" => {
                let object = self.object(Some(self.object_prototype), None)?;
                let message = if args.is_empty() {
                    self.text("")?
                } else {
                    let units = self.units(first, host)?;
                    self.string(units)?
                };
                let label = self.text(name)?;
                self.put_own(object, "message", message, false)?;
                self.put_own(object, "name", label, false)?;
                Ok(Value::Object(object))
            }
            _ if name.starts_with("String.") => self.string_method(name, this, args, host),
            _ if name.starts_with("Array.") => self.array_method(name, this, args, host),
            _ if name.starts_with("Math.") => {
                if name == "Math.random" {
                    self.random ^= self.random << 13;
                    self.random ^= self.random >> 7;
                    self.random ^= self.random << 17;
                    return Ok(Value::Number(
                        (self.random >> 11) as f64 / (1u64 << 53) as f64,
                    ));
                }
                if matches!(name, "Math.min" | "Math.max") {
                    let mut result = if name == "Math.min" {
                        f64::INFINITY
                    } else {
                        f64::NEG_INFINITY
                    };
                    for value in args {
                        self.budget.step()?;
                        let value = self.number(value, host)?;
                        if value.is_nan() {
                            return Ok(Value::Number(f64::NAN));
                        }
                        result = if name == "Math.min" {
                            result.min(value)
                        } else {
                            result.max(value)
                        };
                    }
                    return Ok(Value::Number(result));
                }
                let a = self.number(first, host)?;
                let b = if matches!(name, "Math.pow" | "Math.imul") {
                    let second = self.copy(args.get(1).unwrap_or(&Value::Undefined))?;
                    self.number(second, host)?
                } else {
                    f64::NAN
                };
                Ok(Value::Number(match name {
                    "Math.abs" => a.abs(),
                    "Math.floor" => a.floor(),
                    "Math.ceil" => a.ceil(),
                    "Math.trunc" => a.trunc(),
                    "Math.round" => {
                        if a >= -0.5 && a < 0.0 {
                            -0.0
                        } else {
                            (a + 0.5).floor()
                        }
                    }
                    "Math.sqrt" => a.sqrt(),
                    "Math.pow" => a.powf(b),
                    "Math.sin" => a.sin(),
                    "Math.cos" => a.cos(),
                    "Math.tan" => a.tan(),
                    "Math.log" => a.ln(),
                    "Math.exp" => a.exp(),
                    "Math.imul" => int32(a).wrapping_mul(int32(b)) as f64,
                    "Math.sign" => {
                        if a == 0.0 || a.is_nan() {
                            a
                        } else {
                            a.signum()
                        }
                    }
                    _ => return Err(unsupported(name)),
                }))
            }
            "Number.valueOf" | "Number.toString" | "Number.toFixed" | "Boolean.valueOf"
            | "Boolean.toString" => {
                let value = self.unboxed(this)?;
                if name.starts_with("Number.") && !matches!(value, Value::Number(_))
                    || name.starts_with("Boolean.") && !matches!(value, Value::Bool(_))
                {
                    return Err(exception("TypeError: incompatible primitive receiver"));
                }
                if name.ends_with("valueOf") {
                    return Ok(value);
                }
                if name == "Number.toString"
                    && !args.is_empty()
                    && !matches!(first, Value::Undefined)
                {
                    let radix = self.copy(&first)?;
                    if self.number(radix, host)? != 10.0 {
                        return Err(unsupported("non-decimal Number.toString"));
                    }
                }
                if name == "Number.toFixed" {
                    let mut digits = if args.is_empty() {
                        0.0
                    } else {
                        self.number(first, host)?.trunc()
                    };
                    if digits.is_nan() {
                        digits = 0.0;
                    }
                    if !(0.0..=100.0).contains(&digits) {
                        return Err(exception("RangeError: invalid fraction digits"));
                    }
                    return self.text(&format!("{:.*}", digits as usize, primitive_number(&value)));
                }
                self.text(&value.as_text())
            }
            _ => Err(unsupported(name)),
        }
    }
    fn string_method(
        &mut self,
        name: &str,
        this: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let this = self.unboxed(this)?;
        if matches!(name, "String.toString" | "String.valueOf") && !matches!(this, Value::String(_))
        {
            return Err(exception("TypeError: incompatible string receiver"));
        }
        if matches!(this, Value::Undefined | Value::Null) {
            return Err(exception("TypeError: string method on null or undefined"));
        }
        let units = self.units(this, host)?;
        let length = units.len();
        let first = self.copy(args.first().unwrap_or(&Value::Undefined))?;
        match name {
            "String.toString" | "String.valueOf" => self.string(units),
            "String.charAt" | "String.charCodeAt" => {
                let number = if args.is_empty() {
                    0.0
                } else {
                    self.number(first, host)?
                };
                let index = if number.is_nan() { 0.0 } else { number.trunc() };
                if index < 0.0 || index >= length as f64 {
                    return if name.ends_with("CodeAt") {
                        Ok(Value::Number(f64::NAN))
                    } else {
                        self.text("")
                    };
                }
                let unit = units[index as usize];
                if name.ends_with("CodeAt") {
                    Ok(Value::Number(unit as f64))
                } else {
                    self.string(vec![unit])
                }
            }
            "String.slice" | "String.substring" | "String.substr" => {
                let start = if args.is_empty() {
                    0.0
                } else {
                    self.number(first, host)?
                };
                let second = self.copy(args.get(1).unwrap_or(&Value::Undefined))?;
                let end = if matches!(second, Value::Undefined) {
                    length as f64
                } else {
                    self.number(second, host)?
                };
                let (mut start, mut end) = if name == "String.substring" {
                    (positive_index(start, length), positive_index(end, length))
                } else if name == "String.substr" {
                    let start = relative_index(start, length);
                    (
                        start,
                        start
                            .saturating_add(positive_index(end, length))
                            .min(length),
                    )
                } else {
                    (relative_index(start, length), relative_index(end, length))
                };
                if name == "String.substring" && start > end {
                    std::mem::swap(&mut start, &mut end);
                }
                if end < start {
                    end = start;
                }
                self.budget.allocate((end - start).saturating_mul(2))?;
                self.string(units[start..end].to_vec())
            }
            "String.indexOf" | "String.includes" | "String.startsWith" | "String.endsWith" => {
                let search = self.units(first, host)?;
                let offset = self.copy(args.get(1).unwrap_or(&Value::Undefined))?;
                let offset = if matches!(offset, Value::Undefined) {
                    if name == "String.endsWith" { length } else { 0 }
                } else {
                    positive_index(self.number(offset, host)?, length)
                };
                let found = if name == "String.endsWith" {
                    offset
                        .checked_sub(search.len())
                        .filter(|&start| units[start..offset] == search)
                        .map(|start| start as i64)
                        .unwrap_or(-1)
                } else if name == "String.startsWith" {
                    if units[offset..].starts_with(&search) {
                        offset as i64
                    } else {
                        -1
                    }
                } else {
                    let mut found = -1;
                    for start in offset..=length {
                        self.budget.step()?;
                        if units[start..].starts_with(&search) {
                            found = start as i64;
                            break;
                        }
                    }
                    found
                };
                Ok(if name == "String.indexOf" {
                    Value::Number(found as f64)
                } else {
                    Value::Bool(found >= 0)
                })
            }
            "String.concat" => {
                let mut result = units;
                for value in args {
                    let more = self.units(value, host)?;
                    self.budget.allocate(more.len().saturating_mul(2))?;
                    result.extend(more);
                }
                self.string(result)
            }
            "String.trim" => {
                let start = units
                    .iter()
                    .position(|u| !char::from_u32(*u as u32).is_some_and(js_space))
                    .unwrap_or(length);
                let end = units
                    .iter()
                    .rposition(|u| !char::from_u32(*u as u32).is_some_and(js_space))
                    .map_or(start, |i| i + 1);
                self.string(units[start..end].to_vec())
            }
            "String.toLowerCase" | "String.toUpperCase" => {
                let mut result = Vec::new();
                for character in char::decode_utf16(units) {
                    self.budget.step()?;
                    match character {
                        Ok(character) => {
                            let converted = if name.ends_with("LowerCase") {
                                character.to_lowercase().collect::<String>()
                            } else {
                                character.to_uppercase().collect::<String>()
                            };
                            self.budget.allocate(converted.len().saturating_mul(2))?;
                            result.extend(converted.encode_utf16());
                        }
                        Err(error) => result.push(error.unpaired_surrogate()),
                    }
                }
                self.string(result)
            }
            _ => Err(unsupported(name)),
        }
    }
    fn array_method(
        &mut self,
        name: &str,
        this: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let Value::Object(id) = this else {
            return Err(unsupported("generic Array methods on non-array receivers"));
        };
        let length = self
            .objects
            .get(id)
            .and_then(|o| o.array.as_ref())
            .ok_or_else(|| exception("TypeError: receiver is not an array"))?
            .len();
        match name {
            "Array.push" => {
                for (offset, value) in args.into_iter().enumerate() {
                    self.set(
                        Value::Object(id),
                        &(length + offset).to_string(),
                        value,
                        host,
                    )?;
                }
                Ok(Value::Number(
                    self.objects[id].array.as_ref().unwrap().len() as f64,
                ))
            }
            "Array.pop" => Ok(self.objects[id]
                .array
                .as_mut()
                .unwrap()
                .pop()
                .flatten()
                .unwrap_or(Value::Undefined)),
            "Array.shift" => {
                if length == 0 {
                    Ok(Value::Undefined)
                } else {
                    Ok(self.objects[id]
                        .array
                        .as_mut()
                        .unwrap()
                        .remove(0)
                        .unwrap_or(Value::Undefined))
                }
            }
            "Array.reverse" => {
                self.objects[id].array.as_mut().unwrap().reverse();
                Ok(Value::Object(id))
            }
            "Array.join" | "Array.toString" => {
                let separator = if name == "Array.toString"
                    || args.is_empty()
                    || matches!(args[0], Value::Undefined)
                {
                    vec![44]
                } else {
                    let value = self.copy(&args[0])?;
                    self.units(value, host)?
                };
                let mut result = Vec::new();
                for index in 0..length {
                    self.budget.step()?;
                    if index > 0 {
                        self.budget.allocate(separator.len().saturating_mul(2))?;
                        result.extend_from_slice(&separator);
                    }
                    let value = self.get(&Value::Object(id), &index.to_string(), host)?;
                    if !matches!(value, Value::Undefined | Value::Null) {
                        let units = self.units(value, host)?;
                        self.budget.allocate(units.len().saturating_mul(2))?;
                        result.extend(units);
                    }
                }
                self.string(result)
            }
            "Array.slice" => {
                let start = self.copy(args.first().unwrap_or(&Value::Number(0.0)))?;
                let start = relative_index(self.number(start, host)?, length);
                let end = self.copy(args.get(1).unwrap_or(&Value::Undefined))?;
                let end = if matches!(end, Value::Undefined) {
                    length
                } else {
                    relative_index(self.number(end, host)?, length)
                }
                .max(start);
                let mut values = Vec::new();
                for index in start..end {
                    values.push(self.own(id, &index.to_string())?);
                }
                Ok(Value::Object(
                    self.object(Some(self.array_prototype), Some(values))?,
                ))
            }
            "Array.indexOf" | "Array.includes" => {
                let search = self.copy(args.first().unwrap_or(&Value::Undefined))?;
                let start = self.copy(args.get(1).unwrap_or(&Value::Number(0.0)))?;
                let start = relative_index(self.number(start, host)?, length);
                for index in start..length {
                    self.budget.step()?;
                    let value = self.own(id, &index.to_string())?;
                    let equal = if let Some(value) = value {
                        strict_equal(&value, &search)
                            || name == "Array.includes"
                                && matches!((&value, &search), (Value::Number(a),Value::Number(b)) if a.is_nan() && b.is_nan())
                    } else {
                        name == "Array.includes" && matches!(search, Value::Undefined)
                    };
                    if equal {
                        return Ok(if name == "Array.indexOf" {
                            Value::Number(index as f64)
                        } else {
                            Value::Bool(true)
                        });
                    }
                }
                Ok(if name == "Array.indexOf" {
                    Value::Number(-1.0)
                } else {
                    Value::Bool(false)
                })
            }
            _ => Err(unsupported(name)),
        }
    }
}

fn fault_text(error: Fault) -> String {
    match error {
        Fault::Fatal(message) => message,
        Fault::Throw(value) => format!("Uncaught JavaScript exception: {}", value.as_text()),
    }
}
fn array_index(key: &str) -> Option<usize> {
    let index = key.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index as usize)
}
fn strict_equal(left: &Value, right: &Value) -> bool {
    left == right
}
fn js_space(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}
fn primitive_number(value: &Value) -> f64 {
    match value {
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Bool(value) => {
            if *value {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(value) => *value,
        Value::String(units) => {
            let text = String::from_utf16_lossy(units);
            let text = text.trim_matches(js_space);
            if text.is_empty() {
                return 0.0;
            }
            if matches!(text, "Infinity" | "+Infinity") {
                return f64::INFINITY;
            }
            if text == "-Infinity" {
                return f64::NEG_INFINITY;
            }
            for (prefix, radix) in [
                ("0x", 16),
                ("0X", 16),
                ("0b", 2),
                ("0B", 2),
                ("0o", 8),
                ("0O", 8),
            ] {
                if let Some(digits) = text.strip_prefix(prefix) {
                    return u64::from_str_radix(digits, radix)
                        .map(|n| n as f64)
                        .unwrap_or(f64::NAN);
                }
            }
            if text.contains("inf") || text.contains("NaN") {
                f64::NAN
            } else {
                text.parse().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}
fn int32(value: f64) -> i32 {
    if !value.is_finite() || value == 0.0 {
        0
    } else {
        value.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
    }
}
fn number_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value == f64::INFINITY {
        "Infinity".into()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".into()
    } else if value == 0.0 {
        "0".into()
    } else {
        value.to_string()
    }
}
fn positive_index(value: f64, length: usize) -> usize {
    if value.is_nan() || value <= 0.0 {
        0
    } else {
        value.trunc().min(length as f64) as usize
    }
}
fn relative_index(value: f64, length: usize) -> usize {
    if value < 0.0 {
        (length as f64 + value.trunc()).max(0.0) as usize
    } else {
        positive_index(value, length)
    }
}
fn parse_float(text: &str, budget: &mut Budget) -> Eval<f64> {
    let text = text.trim_start_matches(js_space);
    let bytes = text.as_bytes();
    let mut end = usize::from(bytes.first().is_some_and(|b| matches!(b, b'+' | b'-')));
    if text[end..].starts_with("Infinity") {
        return Ok(if bytes.first() == Some(&b'-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    let mut digits = 0;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        budget.step()?;
        digits += 1;
        end += 1;
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            budget.step()?;
            digits += 1;
            end += 1;
        }
    }
    if digits == 0 {
        return Ok(f64::NAN);
    }
    if bytes.get(end).is_some_and(|b| matches!(b, b'e' | b'E')) {
        let exponent = end;
        end += 1;
        if bytes.get(end).is_some_and(|b| matches!(b, b'+' | b'-')) {
            end += 1;
        }
        let start = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            budget.step()?;
            end += 1;
        }
        if end == start {
            end = exponent;
        }
    }
    Ok(text[..end].parse().unwrap_or(f64::NAN))
}
fn parse_integer(text: &str, mut radix: i32, budget: &mut Budget) -> Eval<f64> {
    if radix != 0 && !(2..=36).contains(&radix) {
        return Ok(f64::NAN);
    }
    let mut text = text.trim_start_matches(js_space);
    let negative = text.starts_with('-');
    if text.starts_with(['-', '+']) {
        text = &text[1..];
    }
    if (radix == 0 || radix == 16) && (text.starts_with("0x") || text.starts_with("0X")) {
        radix = 16;
        text = &text[2..];
    }
    if radix == 0 {
        radix = 10;
    }
    let mut found = false;
    let mut result = 0.0;
    for character in text.chars() {
        budget.step()?;
        let Some(digit) = character.to_digit(radix as u32) else {
            break;
        };
        found = true;
        result = result * radix as f64 + digit as f64;
    }
    Ok(if !found {
        f64::NAN
    } else if negative {
        -result
    } else {
        result
    })
}
fn statement_bytes(statement: &Stmt) -> usize {
    let add = |a: usize, b: usize| a.saturating_add(b);
    128usize.saturating_add(match statement {
        Stmt::Empty => 0,
        Stmt::Break(target) | Stmt::Continue(target) => target.as_ref().map_or(0, String::len),
        Stmt::Label { name, body } => name.len().saturating_add(statement_bytes(body)),
        Stmt::Expr(expr) | Stmt::Throw(expr) => expression_bytes(expr),
        Stmt::Return(expr) => expr.as_ref().map_or(0, expression_bytes),
        Stmt::Var(bindings) => bindings
            .iter()
            .map(|(name, expr)| {
                64usize
                    .saturating_add(name.len())
                    .saturating_add(expr.as_ref().map_or(0, expression_bytes))
            })
            .fold(0, add),
        Stmt::Function { name, params, body } => name
            .len()
            .saturating_add(params.iter().map(|name| name.len() + 32).sum::<usize>())
            .saturating_add(body.iter().map(statement_bytes).fold(0, add)),
        Stmt::Block(body) => body.iter().map(statement_bytes).fold(0, add),
        Stmt::If {
            test,
            consequent,
            alternate,
        } => expression_bytes(test)
            .saturating_add(statement_bytes(consequent))
            .saturating_add(alternate.as_ref().map_or(0, |s| statement_bytes(s))),
        Stmt::While { test, body } | Stmt::DoWhile { body, test } => {
            expression_bytes(test).saturating_add(statement_bytes(body))
        }
        Stmt::For {
            init,
            test,
            update,
            body,
        } => init
            .as_ref()
            .map_or(0, |s| statement_bytes(s))
            .saturating_add(test.as_ref().map_or(0, expression_bytes))
            .saturating_add(update.as_ref().map_or(0, expression_bytes))
            .saturating_add(statement_bytes(body)),
        Stmt::Try {
            body,
            catch,
            finally,
        } => statement_bytes(body)
            .saturating_add(
                catch
                    .as_ref()
                    .map_or(0, |(name, s)| name.len() + statement_bytes(s)),
            )
            .saturating_add(finally.as_ref().map_or(0, |s| statement_bytes(s))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestHost {
        writes: Vec<(String, String, Value)>,
        calls: Vec<String>,
    }
    impl Host for TestHost {
        fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
            match (object, key) {
                ("fixture", "answer") => Ok(Value::Number(42.0)),
                ("fixture", "method") => Ok(Value::Native("host.fixture.method".into())),
                _ => Ok(Value::Undefined),
            }
        }
        fn set(&mut self, object: &str, key: &str, value: Value) -> Result<(), String> {
            self.writes.push((object.into(), key.into(), value));
            Ok(())
        }
        fn call(&mut self, name: &str, _this: Value, args: Vec<Value>) -> Result<Value, String> {
            self.calls.push(name.into());
            Ok(args.into_iter().next().unwrap_or(Value::Undefined))
        }
    }
    fn evaluate(source: &str) -> Value {
        Runtime::new()
            .execute(source, &mut TestHost::default())
            .unwrap()
    }
    fn yes(source: &str) {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }

    #[test]
    fn expressions_coercion_and_evaluation_order() {
        yes(
            "1 + 2 * 3 === 7 && '2' + 3 === '23' && '4' - 1 === 3 && null == undefined && null !== undefined",
        );
        yes(
            "var n=0; false && n++; true || n++; var a=[3]; function index(){n++;return 0;} a[index()] += 4; n===1 && a[0]===7",
        );
        yes(
            "var n=0; var a={valueOf:function(){n++;return 3;}}; Math.abs(a, a)===3 && n===1 && Math.min(a, 4)===3 && n===2",
        );
        yes("typeof neverDeclared === 'undefined'");
        yes("var n=0;try{null[{toString:function(){n++;return 'x';}}];}catch(e){}n===0");
        yes("var n=0;try{null[(n=1)];}catch(e){}n===1");
        assert!(
            Runtime::new()
                .execute("missing += 1", &mut TestHost::default())
                .unwrap_err()
                .contains("ReferenceError")
        );
        yes("(7.2).toFixed(undefined)==='7' && (delete absent) && !(delete undefined)");
        yes("temporary=1; (delete temporary) && typeof temporary==='undefined';");
        yes(
            "parseFloat('  -1.25e2x')===-125 && parseFloat('1e+')===1 && parseFloat('.5x')===0.5 && isNaN(parseFloat('inf')) && parseFloat('0x10')===0",
        );
    }

    #[test]
    fn hoisting_closures_and_loop_completion() {
        yes("before()===7; function before(){return 7;} before()===7");
        yes(
            "function outer(){var n=1; return function(){return ++n;};} var f=outer(); f()===2 && f()===3",
        );
        yes("var before=x; var x=3; before===undefined && x===3");
        yes(
            "var sum=0; for(var i=0;i<8;i++){if(i===2)continue;if(i===5)break;sum+=i;} do {sum++;} while(false); while(sum<10){sum++;} sum===10",
        );
        yes("(function named(n){return n ? n*named(n-1) : 1;})(5)===120");
    }

    #[test]
    fn this_calls_construction_and_prototypes() {
        yes(
            "var o={n:3,f:function(v){return this.n+v;}}; o.f(2)===5 && o.f.call({n:8},1)===9 && o.f.apply({n:7},[2])===9",
        );
        yes(
            "function Item(n){this.n=n;} Item.prototype.read=function(){return this.n;};var a=new Item(9);a.read()===9 && a instanceof Item && 'read' in a && !a.hasOwnProperty('read')",
        );
        yes(
            "var f=function(){return this;}; f()===this && f.call(null)===this && f.call(3).valueOf()===3",
        );
    }

    #[test]
    fn strings_preserve_utf16_and_array_holes() {
        yes(
            "var s='😀'; s.length===2 && s.charCodeAt(0)===55357 && s.charAt(1).charCodeAt(0)===56832 && s.slice(1).length===1",
        );
        yes("'abc'.substring(2,0)==='ab' && 'abc'.slice(-2)==='bc' && 'abc'.indexOf('b')===1");
        yes(
            "var a=[1,,3]; a.length===3 && !(1 in a) && a.join('-')==='1--3' && a.push(4)===4 && a.pop()===4",
        );
        assert_eq!(evaluate("'\\ud800'"), Value::String(vec![0xd800]));
    }

    #[test]
    fn catch_finally_and_function_returns() {
        yes("var x=0;try{throw 3;}catch(e){x=e;}finally{x++;}x===4");
        yes("function f(){try{return 1;}finally{return 2;}}f()===2");
        yes("var caught=false;try{null.x;}catch(e){caught=true;}caught");
        let error = Runtime::new()
            .execute("[].map(function(){})", &mut TestHost::default())
            .unwrap_err();
        assert!(error.contains("Unsupported"));
    }

    #[test]
    fn labels_preserve_completion_values_and_hoisted_variables() {
        assert_eq!(
            evaluate("outer: { 17; break outer; 99; }"),
            Value::Number(17.0)
        );
        assert_eq!(
            evaluate("outer: inner: for(var i=0;i<3;i++){42;continue outer;}"),
            Value::Number(42.0)
        );
        assert_eq!(
            evaluate("outer: while(true){inner:while(true){23;break outer;}}"),
            Value::Number(23.0)
        );
        yes("var old=hidden; block:{var hidden=3;break block;}old===undefined && hidden===3");
        yes(
            "var n=0;outer:inner:for(var i=0;i<3;i++){try{continue outer;}finally{n++;}}n===3 && i===3",
        );
    }

    #[test]
    fn dynamic_functions_have_global_scope_and_anonymous_display_names() {
        yes("var anonymous=7;var f=Function('return anonymous;');f.name==='anonymous' && f()===7");
        yes("(function(){}).constructor('return 13;')()===13");
        yes(
            "function outer(){var hidden=4;return Function('return typeof hidden;')();}outer()==='undefined'",
        );
        yes(
            "var code='var n=4;'; var wrapper=Object(code);eval(wrapper)===wrapper && typeof n==='undefined'",
        );
    }

    #[test]
    fn eval_binding_creation_and_intrinsic_identity_are_preserved() {
        yes("eval('var created=1;'); (delete created) && typeof created==='undefined'");
        yes("var permanent=1;eval('var permanent=2');!(delete permanent) && permanent===2");
        yes(
            "function run(){var local=1;eval('var created=2;');return (delete created) && !(delete local) && typeof created==='undefined';}run()",
        );
        yes(
            "var kept=eval;function f(){var x=7;return eval((eval=function(){return 9;},'x'));}f()===7 && eval('ignored')===9",
        );
        yes(
            "function f(){try{throw 4;}catch(e){eval('var made=7;function read(){return e;}');}return made+read();}f()===11",
        );
        yes(
            "function outer(){var x=1;function inner(){eval('var x=2');return x;}return inner()===2 && x===1;}outer()",
        );
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        runtime
            .execute("eval('var created=1;');", &mut host)
            .unwrap();
        assert_eq!(
            runtime
                .execute("var created; delete created;", &mut host)
                .unwrap(),
            Value::Bool(true)
        );
        runtime
            .execute("eval('var created=1;');", &mut host)
            .unwrap();
        assert_eq!(
            runtime
                .execute("function created(){} delete created;", &mut host)
                .unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn dynamic_sources_never_replace_invalid_utf16_and_syntax_errors_are_objects() {
        yes(
            "var caught=false;try{eval('\\ud800');}catch(e){caught=e.name==='SyntaxError';}caught && eval('2+3')===5",
        );
        yes(
            "var caught=false;try{Function('return \\ud800;');}catch(e){caught=e.name==='SyntaxError';}caught",
        );
        yes(
            "var order='';try{Function({toString:function(){order+='p';return '\\ud800';}},{toString:function(){order+='b';return 'return 1;';}});}catch(e){order+=e.name;}order==='pbSyntaxError'",
        );
        yes("eval(\"'\\\\ud800'\").charCodeAt(0)===55296");
        yes(
            "var caught=false;try{eval(\"'use strict';1\");}catch(e){caught=e.name==='SyntaxError';}caught",
        );
    }

    #[test]
    fn dynamic_parser_and_nested_eval_limits_remain_fatal_and_latched() {
        for source in [
            "var caught=false;var code='eval(code)';try{eval(code);}catch(e){caught=true;}finally{caught=true;}",
            "var caught=false;try{Function('return arguments.callee();')();}catch(e){caught=true;}finally{caught=true;}",
        ] {
            let mut runtime = Runtime::new();
            let mut host = TestHost::default();
            let error = runtime.execute(source, &mut host).unwrap_err();
            assert!(error.contains("call depth exhausted"), "{error}");
            assert_eq!(runtime.get_global("caught"), Value::Bool(false));
            assert_eq!(runtime.execute("1", &mut host).unwrap_err(), error);
        }
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        runtime.set_global(
            "tooDeep",
            Value::text(&format!("{}1{}", "(".repeat(140), ")".repeat(140))),
        );
        let error = runtime
            .execute(
                "var caught=false;try{eval(tooDeep);}catch(e){caught=true;}",
                &mut host,
            )
            .unwrap_err();
        assert!(error.contains("parser limit exhausted"), "{error}");
        assert_eq!(runtime.get_global("caught"), Value::Bool(false));
        assert_eq!(runtime.execute("1", &mut host).unwrap_err(), error);
    }

    #[test]
    fn uri_builtins_use_real_to_string_and_catchable_uri_errors() {
        yes(
            "var n=0;var o={toString:function(){n++;return 'a b/😀';}};encodeURIComponent(o)==='a%20b%2F%F0%9F%98%80' && n===1",
        );
        yes(
            "var order='';var o={toString:function(){order+='s';return {};},valueOf:function(){order+='v';return 'a b';}};encodeURI(o)==='a%20b' && order==='sv'",
        );
        yes(
            "var order='';decodeURIComponent({toString:function(){order+='s';return '%41';}},order+='arg')==='A' && order==='args'",
        );
        yes(
            "encodeURI('a/b?x=1#t')==='a/b?x=1#t' && decodeURI('%2f%3f%41')==='%2f%3fA' && decodeURIComponent('%2f%3f%41')==='/?A'",
        );
        yes(
            "encodeURIComponent()==='undefined' && decodeURIComponent(null)==='null' && decodeURIComponent('a+b')==='a+b'",
        );
        yes(
            "var caught=false;try{encodeURI('\\ud800');}catch(e){caught=e.name==='URIError' && e.message==='malformed URI sequence';}caught",
        );
        yes(
            "var caught=false;try{decodeURIComponent('%ED%A0%80');}catch(e){caught=e.name==='URIError';}caught",
        );
        yes(
            "var caught=false;try{encodeURI({toString:function(){throw 7;}});}catch(e){caught=e===7;}caught",
        );
        assert!(
            Runtime::new()
                .execute("decodeURIComponent('%')", &mut TestHost::default())
                .unwrap_err()
                .contains("URIError: malformed URI sequence")
        );
        assert_eq!(
            evaluate("decodeURIComponent('\\ud800')"),
            Value::String(vec![0xd800])
        );
    }

    #[test]
    fn uri_work_respects_cumulative_budget_and_latches_exhaustion() {
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        let input = Value::String(vec![b' ' as u16; uri::MAX_UNITS / 3 + 1]);
        runtime.set_global("input", input);
        let error = runtime
            .execute(
                "try{encodeURIComponent(input);}catch(e){'recovered';}",
                &mut host,
            )
            .unwrap_err();
        assert!(error.contains("limit exhausted"), "{error}");
        assert_eq!(runtime.execute("1", &mut host).unwrap_err(), error);

        let mut runtime = Runtime::new();
        runtime
            .execute("encodeURIComponent('a b')", &mut host)
            .unwrap();
        let before = runtime.budget.allocated;
        runtime
            .execute("encodeURIComponent('a b')", &mut host)
            .unwrap();
        assert!(runtime.budget.allocated > before);
        runtime.budget.allocated = MAX_HEAP - 1;
        let error = runtime
            .execute("encodeURIComponent('a b')", &mut host)
            .unwrap_err();
        assert!(error.contains("allocation budget exhausted"), "{error}");
        assert_eq!(
            runtime
                .invoke(
                    Value::Native("encodeURI".into()),
                    Value::Undefined,
                    vec![Value::text("x")],
                    &mut host
                )
                .unwrap_err(),
            error
        );
    }

    #[test]
    fn global_alias_host_capabilities_and_retained_callbacks() {
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        runtime.set_global("window", runtime.global_object());
        runtime.set_global("globalThis", runtime.global_object());
        runtime.set_global("fixture", Value::Host("fixture".into()));
        runtime.set_global("destination", Value::Host("destination".into()));
        runtime.set_global_setter("destination", "destination", "href");
        assert_eq!(runtime.execute("var n=1;window.n++;globalThis.n===2 && window===globalThis && fixture.answer===42 && fixture.method(5)===5", &mut host).unwrap(), Value::Bool(true));
        runtime.execute("destination='a'; window.destination='b';var destination='c';fixture.answer=7; var callback=function(v){n+=v;return n;};", &mut host).unwrap();
        assert_eq!(
            runtime.get_global("destination"),
            Value::Host("destination".into())
        );
        assert_eq!(
            runtime
                .execute(
                    "!(delete destination) && !(delete window.destination)",
                    &mut host
                )
                .unwrap(),
            Value::Bool(true)
        );
        assert!(
            runtime
                .execute("function destination(){}", &mut host)
                .unwrap_err()
                .contains("conflicts with host global")
        );
        assert_eq!(
            runtime.get_global("destination"),
            Value::Host("destination".into())
        );
        assert_eq!(host.calls, vec!["host.fixture.method"]);
        assert_eq!(host.writes.len(), 4);
        assert_eq!(
            host.writes[2],
            ("destination".into(), "href".into(), Value::text("c"))
        );
        assert_eq!(
            runtime
                .invoke(
                    runtime.get_global("callback"),
                    Value::Undefined,
                    vec![Value::Number(3.0)],
                    &mut host
                )
                .unwrap(),
            Value::Number(5.0)
        );
        runtime.set_global("notHost", Value::Native("unrecognized.function".into()));
        assert!(
            runtime
                .execute("notHost()", &mut host)
                .unwrap_err()
                .contains("Unsupported")
        );
        assert_eq!(host.calls.len(), 1);
    }

    #[test]
    fn fuel_call_and_allocation_failures_are_uncatchable_and_latched() {
        for (source, expected) in [
            (
                "var recovered=false;try{while(true){}}catch(e){recovered=true;}finally{recovered=true;}",
                "fuel exhausted",
            ),
            (
                "function f(){try{return f();}catch(e){return 1;}finally{return 2;}}f();",
                "call depth exhausted",
            ),
            (
                "var s='a';try{while(true){s=s+s;}}catch(e){s='recovered';}",
                "allocation budget exhausted",
            ),
            ("try{Array(10001);}catch(e){true;}", "array limit exhausted"),
        ] {
            let mut runtime = Runtime::new();
            let mut host = TestHost::default();
            let error = runtime.execute(source, &mut host).unwrap_err();
            assert!(error.contains(expected), "{error}: {source}");
            assert_eq!(runtime.execute("1", &mut host).unwrap_err(), error);
            assert_eq!(
                runtime
                    .invoke(
                        Value::Native("Number".into()),
                        Value::Undefined,
                        vec![],
                        &mut host
                    )
                    .unwrap_err(),
                error
            );
        }
    }
}
fn expression_bytes(expr: &Expr) -> usize {
    let add = |a: usize, b: usize| a.saturating_add(b);
    96usize.saturating_add(match expr {
        Expr::String(value) => value.len().saturating_mul(2),
        Expr::Ident(name) => name.len(),
        Expr::Array(items) => items
            .iter()
            .map(|expr| expr.as_ref().map_or(0, expression_bytes))
            .fold(0, add),
        Expr::Object(properties) => properties
            .iter()
            .map(|(name, expr)| name.len().saturating_add(expression_bytes(expr)))
            .fold(0, add),
        Expr::Function { name, params, body } => name
            .as_ref()
            .map_or(0, String::len)
            .saturating_add(params.iter().map(String::len).sum::<usize>())
            .saturating_add(body.iter().map(statement_bytes).fold(0, add)),
        Expr::Unary { expr, .. } | Expr::Update { expr, .. } => expression_bytes(expr),
        Expr::Binary { left, right, .. }
        | Expr::Assign { left, right, .. }
        | Expr::Member {
            object: left,
            property: right,
        } => expression_bytes(left).saturating_add(expression_bytes(right)),
        Expr::Conditional {
            test,
            consequent,
            alternate,
        } => expression_bytes(test)
            .saturating_add(expression_bytes(consequent))
            .saturating_add(expression_bytes(alternate)),
        Expr::Sequence(exprs) => exprs.iter().map(expression_bytes).fold(0, add),
        Expr::Call { callee, args } | Expr::New { callee, args } => {
            expression_bytes(callee).saturating_add(args.iter().map(expression_bytes).fold(0, add))
        }
        _ => 0,
    })
}
