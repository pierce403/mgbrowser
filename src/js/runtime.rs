//! Original, deliberately limited classic-script evaluator.
//!
//! Values use UTF-16 strings. `Value::as_text` is diagnostic formatting, not
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
//! modules, promises and a general event loop are not implemented.
//! Unsupported exposed builtins throw an explicit error. Math.random
//! is a deterministic research PRNG and must never be used for cryptography.

use super::{Expr, ForInBinding, Program, Stmt, SwitchCase, regexp, storage, syntax, uri};
use std::rc::Rc;
#[path = "arguments.rs"]
mod arguments;
#[path = "array.rs"]
mod array;
#[path = "array_callbacks.rs"]
mod array_callbacks;
#[path = "bound.rs"]
mod bound;
#[path = "core_intrinsics.rs"]
mod core_intrinsics;
#[path = "diagnostic.rs"]
mod diagnostic;
#[path = "error.rs"]
mod error;
#[path = "property.rs"]
mod property;
#[path = "prototype.rs"]
mod prototype;
#[path = "symbol.rs"]
mod symbol;
use bound::BoundData;
use diagnostic::{MemberContext, MemberOperation, Producer, ProducerKind};
use error::ErrorKind;
use property::{Stored, WriteAction};
use prototype::KeyRef;
pub use symbol::SymbolHandle;
use symbol::{Hint, PropertyKey};

const MAX_FUEL: u64 = 1_000_000;
const MAX_HEAP: usize = 4 * 1024 * 1024;
const MAX_OBJECTS: usize = 10_000;
const MAX_CALLS: usize = 64;
const MAX_ARRAY: usize = 10_000;
// AST and call limits cannot independently bound their product on the native
// stack. Track retained evaluation entries across function/native re-entry too.
const MAX_ACTIVE_EXPRESSIONS: usize = 128;
const MAX_EVALUATION_ENTRIES: usize = 384;

/// Exclusive logical charge sites, not allocator RSS or inclusive call stacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationPhase {
    Bootstrap,
    Source,
    Ast,
    FunctionCode,
    Runtime,
    RegexCompile,
    RegexResult,
}
impl AllocationPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Source => "source",
            Self::Ast => "ast",
            Self::FunctionCode => "function_code",
            Self::Runtime => "runtime",
            Self::RegexCompile => "regex_compile",
            Self::RegexResult => "regex_result",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationTotals {
    pub bootstrap: u64,
    pub source: u64,
    pub ast: u64,
    pub function_code: u64,
    /// Includes shared object/property storage and ordinary value copies, even
    /// when called by source conversion or regex code. Callbacks keep their own
    /// charge attribution rather than inheriting a caller's phase.
    pub runtime: u64,
    pub regex_compile: u64,
    pub regex_result: u64,
}
impl AllocationTotals {
    fn counter(&mut self, phase: AllocationPhase) -> &mut u64 {
        match phase {
            AllocationPhase::Bootstrap => &mut self.bootstrap,
            AllocationPhase::Source => &mut self.source,
            AllocationPhase::Ast => &mut self.ast,
            AllocationPhase::FunctionCode => &mut self.function_code,
            AllocationPhase::Runtime => &mut self.runtime,
            AllocationPhase::RegexCompile => &mut self.regex_compile,
            AllocationPhase::RegexResult => &mut self.regex_result,
        }
    }
    fn total(self) -> Option<u64> {
        [
            self.bootstrap,
            self.source,
            self.ast,
            self.function_code,
            self.runtime,
            self.regex_compile,
            self.regex_result,
        ]
        .into_iter()
        .try_fold(0u64, u64::checked_add)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectedAllocation {
    pub phase: AllocationPhase,
    pub accepted_bytes: u64,
    pub requested_bytes: u64,
    pub limit_bytes: u64,
}
impl RejectedAllocation {
    fn fault(self) -> Fault {
        Fault::Fatal(format!(
            "JavaScript allocation budget exhausted: phase={} accepted={} requested={} limit={}",
            self.phase.label(),
            self.accepted_bytes,
            self.requested_bytes,
            self.limit_bytes
        ))
    }
}

/// Fixed-size host diagnostics with no script text, names, URLs or event history.
/// Accepted charges are cumulative; rejection never advances these totals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationReport {
    pub limit_bytes: u64,
    pub accepted_bytes: u64,
    pub phases: AllocationTotals,
    pub first_rejected: Option<RejectedAllocation>,
}
impl AllocationReport {
    /// Check a transferred report without allocating or trusting reported caps.
    pub fn is_valid(&self) -> bool {
        self.limit_bytes == MAX_HEAP as u64
            && self.accepted_bytes <= self.limit_bytes
            && self.phases.total() == Some(self.accepted_bytes)
            && self.first_rejected.is_none_or(|rejected| {
                rejected.limit_bytes == self.limit_bytes
                    && rejected.accepted_bytes == self.accepted_bytes
                    && rejected.requested_bytes > self.limit_bytes - self.accepted_bytes
            })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u16>),
    Symbol(SymbolHandle),
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
            Self::Symbol(value) => value.display(),
            Self::Function(_) => "function () { [mgbrowser code] }".into(),
            Self::Native(name) => format!("function {name}() {{ [native code] }}"),
            Self::Object(_) | Self::Host(_) => "[object Object]".into(),
        }
    }
    /// Primitive-only DOM boundary after the runtime's marked Host ToString.
    /// Objects retain the existing diagnostic fallback for unmarked operations.
    pub fn as_dom_text(&self) -> Result<String, String> {
        if matches!(self, Self::Symbol(_)) {
            Err("TypeError: cannot convert Symbol to string".into())
        } else {
            Ok(self.as_text())
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
    /// Presence for borrowed indexed algorithms; undefined is not absence.
    /// Hosts must opt in without invoking Get or changing iteration order.
    fn has_indexed_property(&mut self, _object: &str, _index: usize) -> Result<bool, String> {
        Err("Indexed property inspection is not implemented by this host".into())
    }
    fn string_assignment(&self, _object: &str, _key: &str) -> bool {
        false
    }
    fn string_arguments(&self, _name: &str) -> &'static [usize] {
        &[]
    }
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String>;
    fn set(&mut self, object: &str, key: &str, value: Value) -> Result<(), String>;
    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String>;
}

#[derive(Debug)]
enum Fault {
    Throw(Value),
    Member {
        value: Value,
        context: MemberContext,
    },
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
    active_expressions: usize,
    evaluation_entries: usize,
    allocations: AllocationTotals,
    first_rejected: Option<RejectedAllocation>,
    bootstrapping: bool,
}
impl Budget {
    fn enter_evaluation(&mut self, expression: bool) -> Eval<()> {
        if self.evaluation_entries >= MAX_EVALUATION_ENTRIES
            || (expression && self.active_expressions >= MAX_ACTIVE_EXPRESSIONS)
        {
            return Err(Fault::Fatal(
                "JavaScript evaluation depth limit exhausted".into(),
            ));
        }
        self.evaluation_entries += 1;
        self.active_expressions += usize::from(expression);
        Ok(())
    }
    fn leave_evaluation(&mut self, expression: bool) {
        self.evaluation_entries -= 1;
        self.active_expressions -= usize::from(expression);
    }
    fn step(&mut self) -> Eval<()> {
        self.fuel = self
            .fuel
            .checked_sub(1)
            .ok_or_else(|| Fault::Fatal("JavaScript fuel exhausted".into()))?;
        Ok(())
    }
    fn allocate(&mut self, bytes: usize) -> Eval<()> {
        self.allocate_in(AllocationPhase::Runtime, bytes)
    }
    fn allocate_in(&mut self, phase: AllocationPhase, bytes: usize) -> Eval<()> {
        if let Some(rejected) = self.first_rejected {
            return Err(rejected.fault());
        }
        let phase = if self.bootstrapping {
            AllocationPhase::Bootstrap
        } else {
            phase
        };
        let total = self.allocated.saturating_add(bytes);
        if total > MAX_HEAP {
            let rejected = RejectedAllocation {
                phase,
                accepted_bytes: self.allocated as u64,
                requested_bytes: bytes as u64,
                limit_bytes: MAX_HEAP as u64,
            };
            self.first_rejected = Some(rejected);
            return Err(rejected.fault());
        }
        self.allocated = total;
        *self.allocations.counter(phase) += bytes as u64;
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

/// Audited array producers move their owned element payloads without creating
/// new copies. Existing evaluation/read/copy/ingress accounting stays in place.
/// This non-Clone wrapper separately admits the new Option<Value> slots. It must
/// not be made from a bare vector or exempt general property transfers.
struct PrepaidArray {
    values: Vec<Option<Value>>,
    paid_slots: usize,
    phase: AllocationPhase,
    growing: bool,
}
impl PrepaidArray {
    fn with_slots(budget: &mut Budget, count: usize, phase: AllocationPhase) -> Eval<Self> {
        if count > MAX_ARRAY {
            return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
        }
        // Fixed producers pay before output-vector allocation; literals also
        // preflight here before evaluating any element expression.
        budget.allocate_in(phase, count.saturating_mul(64))?;
        Ok(Self {
            values: Vec::with_capacity(count),
            paid_slots: count,
            phase,
            growing: false,
        })
    }
    fn growing(phase: AllocationPhase) -> Self {
        Self {
            values: Vec::new(),
            paid_slots: 0,
            phase,
            growing: true,
        }
    }
    // Concat must reserve a slot before a source getter can run. Other producers
    // retain their existing push-time admission through the same helper.
    fn prepare_push(&mut self, budget: &mut Budget) -> Eval<()> {
        if self.values.len() >= MAX_ARRAY {
            return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
        }
        if self.values.len() == self.paid_slots {
            if !self.growing {
                return Err(Fault::Fatal(
                    "Invalid prepaid JavaScript array admission".into(),
                ));
            }
            let target = self.paid_slots.saturating_mul(2).max(1).min(MAX_ARRAY);
            budget.allocate_in(self.phase, (target - self.paid_slots).saturating_mul(64))?;
            // Prepay geometric capacity before growing, avoiding per-element
            // reallocations. Unused credits remain cumulative; allocator RSS
            // and rounding are not what this logical budget measures.
            self.values.reserve_exact(target - self.values.len());
            self.paid_slots = target;
        }
        Ok(())
    }
    fn push_owned(&mut self, budget: &mut Budget, value: Option<Value>) -> Eval<()> {
        self.prepare_push(budget)?;
        self.values.push(value);
        Ok(())
    }
    fn len(&self) -> usize {
        self.values.len()
    }
    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    fn into_values(self) -> Eval<Vec<Option<Value>>> {
        if self.values.len() > self.paid_slots
            || (!self.growing && self.values.len() != self.paid_slots)
            || self.values.capacity() < self.paid_slots
            || self.paid_slots > MAX_ARRAY
        {
            return Err(Fault::Fatal(
                "Invalid prepaid JavaScript array admission".into(),
            ));
        }
        Ok(self.values)
    }
}

struct Property {
    key: PropertyKey,
    value: Stored,
    enumerable: bool,
    writable: bool,
    configurable: bool,
    getter: bool,
}
struct Object {
    properties: Vec<Property>,
    array: Option<Vec<Option<Value>>>,
    prototype: Option<PrototypeIdentity>,
    boxed: Option<Value>,
    regexp: Option<Rc<regexp::Regex>>,
    error: Option<ErrorKind>,
    // Snapshot storage is indexed, but an arguments object is not an Array.
    arguments: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrototypeIdentity {
    Object(usize),
    Function(usize),
    Native(usize),
}
type EnumerationOwner = PrototypeIdentity;
struct EnumerationEntry {
    owner: EnumerationOwner,
    key: String,
    enumerable: bool,
}
fn enumeration_equal(budget: &mut Budget, left: &str, right: &str) -> Eval<bool> {
    budget.step()?;
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.bytes().zip(right.bytes()) {
        budget.step()?;
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}
fn enumeration_add(
    budget: &mut Budget,
    entries: &mut Vec<EnumerationEntry>,
    owner: EnumerationOwner,
    key: &str,
    enumerable: bool,
) -> Eval<()> {
    budget.step()?;
    for entry in entries.iter() {
        if enumeration_equal(budget, &entry.key, key)? {
            return Ok(());
        }
    }
    budget.allocate(64usize.saturating_add(key.len()))?;
    entries.push(EnumerationEntry {
        owner,
        key: key.into(),
        enumerable,
    });
    Ok(())
}
fn native_virtual_names(name: &str) -> &'static [&'static str] {
    match name {
        "Object" => &[
            "name",
            "length",
            "prototype",
            "keys",
            "create",
            "getPrototypeOf",
            "getOwnPropertyNames",
            "getOwnPropertySymbols",
        ],
        "Array" => &["name", "length", "prototype", "isArray"],
        "String" => &["name", "length", "prototype", "fromCharCode"],
        "Number" => &[
            "name",
            "length",
            "prototype",
            "isNaN",
            "isFinite",
            "isInteger",
        ],
        "Function" | "Boolean" | "RegExp" => &["name", "length", "prototype"],
        "Symbol" => &[
            "name",
            "length",
            "prototype",
            "for",
            "keyFor",
            "toPrimitive",
            "toStringTag",
        ],
        _ => &["name", "length"],
    }
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
    // Only an empty actual argument list can have this private pending value.
    // The ordinary binding exists immediately; no public Value or Vec is kept.
    pending_empty_arguments: Option<usize>,
}
struct Code {
    name: Option<String>,
    params: Rc<[String]>,
    body: Rc<[Stmt]>,
}
struct Function {
    kind: FunctionKind,
    environment: usize,
    properties: usize,
    // The real own property is admitted immediately. Only its default object
    // and constructor backlink wait for an actual value read.
    pending_default_prototype: bool,
}
enum FunctionKind {
    Ordinary(Rc<Code>),
    Bound(Rc<BoundData>),
}
impl Function {
    #[cfg(test)]
    fn ordinary_code(&self) -> &Rc<Code> {
        match &self.kind {
            FunctionKind::Ordinary(code) => code,
            FunctionKind::Bound(_) => panic!("ordinary function fixture expected"),
        }
    }
}
enum Reference {
    Binding(usize, String),
    Property(Value, PropertyKey),
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
    native_deleted: Vec<(String, String)>,
    budget: Budget,
    fatal: Option<String>,
    random: u64,
    object_prototype: usize,
    array_prototype: usize,
    function_prototype: usize,
    string_prototype: usize,
    number_prototype: usize,
    boolean_prototype: usize,
    regexp_prototype: usize,
    symbol_prototype: usize,
    error_prototypes: [usize; 6],
    symbols: Vec<SymbolHandle>,
    symbol_registry: Vec<SymbolHandle>,
    to_primitive: Option<SymbolHandle>,
    to_string_tag: Option<SymbolHandle>,
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
            native_deleted: Vec::new(),
            budget: Budget {
                fuel: MAX_FUEL,
                allocated: 128,
                calls: 0,
                active_expressions: 0,
                evaluation_entries: 0,
                allocations: AllocationTotals {
                    bootstrap: 128,
                    ..AllocationTotals::default()
                },
                first_rejected: None,
                bootstrapping: true,
            },
            fatal: None,
            random: 0x9e3779b97f4a7c15,
            object_prototype: 1,
            array_prototype: 2,
            function_prototype: 3,
            string_prototype: 4,
            number_prototype: 5,
            boolean_prototype: 6,
            regexp_prototype: 7,
            symbol_prototype: 8,
            error_prototypes: [0; 6],
            symbols: Vec::new(),
            symbol_registry: Vec::new(),
            to_primitive: None,
            to_string_tag: None,
            global_declarations: Vec::new(),
            global_setters: Vec::new(),
        };
        // The fixed bootstrap is far below all limits and has no host capability.
        for prototype in [
            Some(1),
            None,
            Some(1),
            Some(1),
            Some(1),
            Some(1),
            Some(1),
            Some(1),
            Some(1),
        ] {
            runtime
                .object(prototype, None)
                .expect("fixed bounded runtime bootstrap");
        }
        let empty_array =
            PrepaidArray::with_slots(&mut runtime.budget, 0, AllocationPhase::Runtime)
                .expect("fixed empty Array prototype");
        runtime.objects[runtime.array_prototype].array = Some(
            empty_array
                .into_values()
                .expect("prepaid empty Array prototype"),
        );
        runtime
            .core_intrinsics_bootstrap()
            .expect("fixed core intrinsic prototypes");
        for (id, prefix, methods) in [
            (1, "Object", &["toString", "valueOf", "hasOwnProperty"][..]),
            (
                2,
                "Array",
                &[
                    "push",
                    "pop",
                    "shift",
                    "unshift",
                    "join",
                    "toString",
                    "slice",
                    "concat",
                    "indexOf",
                    "includes",
                    "reverse",
                    "forEach",
                    "map",
                    "filter",
                    "some",
                    "every",
                    "reduce",
                    "reduceRight",
                ][..],
            ),
            (3, "Function", &["call", "apply", "bind", "toString"][..]),
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
                    "match",
                    "search",
                    "replace",
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
            (7, "RegExp", &["exec", "test", "toString"][..]),
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
        let empty = runtime.compile_regexp(&[], "").expect("fixed empty regex");
        runtime
            .init_regexp(7, empty)
            .expect("fixed regex prototype");
        runtime
            .put_own(7, "constructor", Value::Native("RegExp".into()), false)
            .expect("fixed regex constructor");
        for name in [
            "Object",
            "Array",
            "String",
            "Number",
            "Boolean",
            "Function",
            "RegExp",
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
        runtime.symbol_bootstrap().expect("fixed symbol bootstrap");
        runtime.error_bootstrap().expect("fixed error bootstrap");
        runtime.budget.bootstrapping = false;
        runtime
    }

    pub fn allocation_report(&self) -> AllocationReport {
        AllocationReport {
            limit_bytes: MAX_HEAP as u64,
            accepted_bytes: self.budget.allocated as u64,
            phases: self.budget.allocations,
            first_rejected: self.budget.first_rejected,
        }
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
    /// Host coordinators may stop later callbacks without inspecting error text.
    pub fn is_fatal(&self) -> bool {
        self.fatal.is_some()
    }
    /// Host inspection returns a copy; it does not execute JS coercions/getters.
    pub fn get_global(&self, name: &str) -> Value {
        self.objects[0]
            .properties
            .iter()
            .find(|p| p.key == name)
            .map(|p| p.value.raw().clone())
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
    /// Install an explicit host-backed global accessor. This does not expose a
    /// general accessor-definition API to scripts or change ordinary globals.
    pub fn set_global_accessor(&mut self, name: &str, object: &str, key: &str, getter: Value) {
        if self.fatal.is_some() {
            return;
        }
        if !getter.callable() {
            self.fatal = Some("Host global getter must be callable".into());
            return;
        }
        self.set_global_setter(name, object, key);
        if self.fatal.is_some() {
            return;
        }
        let result = self.put_own(0, name, getter, true);
        if let Err(error) = result {
            self.fatal = Some(fault_text(error));
            return;
        }
        let property = self.objects[0]
            .properties
            .iter_mut()
            .find(|p| p.key == name)
            .unwrap();
        property.getter = true;
        property.writable = true; // The paired host setter handles writes.
        property.configurable = false;
    }
    /// Invoke a retained callback without an uncharged clone in the host. The
    /// callback's actual payload copy is admitted once, before copying it;
    /// receiver/arguments retain the normal public-ingress policy.
    pub fn invoke_retained(
        &mut self,
        callee: &Value,
        this: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, String> {
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        let result = (|| {
            self.admit(callee)?;
            self.admit(&this)?;
            for argument in &args {
                self.admit(argument)?;
            }
            let callee = self.budget.copy(callee)?;
            self.budget.allocate(
                value_bytes(&this).saturating_add(args.iter().map(value_bytes).sum::<usize>()),
            )?;
            self.call(callee, this, args, None, host)
        })();
        self.finish(result)
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
            self.admit(&callee)?;
            self.admit(&this)?;
            for argument in &args {
                self.admit(argument)?;
            }
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
            self.budget.allocate_in(
                AllocationPhase::Source,
                128usize.saturating_add(source.len()),
            )?;
            let program = self.parse_result(syntax::parse(source))?;
            self.budget
                .allocate_in(AllocationPhase::Ast, storage::program_bytes(&program))?;
            let Program(statements) = program;
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
            Err(Fault::Member { context, .. }) => Err(context.format()),
            Err(Fault::Throw(value)) => {
                if let Value::Object(id) = &value
                    && let Some(kind) = self.objects.get(*id).and_then(|object| object.error)
                {
                    return Err(self.error_diagnostic(*id, kind));
                }
                let text = if let Value::Object(id) = &value {
                    self.objects
                        .get(*id)
                        .and_then(|object| {
                            let name = object.properties.iter().find(|p| p.key == "name")?;
                            let message = object.properties.iter().find(|p| p.key == "message")?;
                            Some(format!(
                                "{}: {}",
                                name.diagnostic_text(),
                                message.diagnostic_text()
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
        let kind = ErrorKind::from_name(name)
            .ok_or_else(|| Fault::Fatal("Invalid intrinsic JavaScript error family".into()))?;
        let object = self.error_instance(kind)?;
        let message = self.text(message)?;
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
        self.budget.allocate_in(AllocationPhase::Source, length)?;
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
        self.budget.allocate_in(AllocationPhase::Source, 128)?;
        let program = self.parse_result(syntax::parse(&source))?;
        self.budget
            .allocate_in(AllocationPhase::Ast, storage::program_bytes(&program))?;
        let Program(statements) = program;
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
        self.budget
            .allocate_in(AllocationPhase::Source, args.len().saturating_mul(32))?;
        let mut parts = Vec::with_capacity(args.len());
        // Complete every ToString before any parameter/body grammar validation.
        for argument in args {
            parts.push(self.units_in(argument, host, AllocationPhase::Source)?);
        }
        let body = parts.pop().unwrap_or_default();
        let parameters = self.function_parameter_source(parts)?;
        let parameters = self.utf8_source(&parameters)?;
        let body = self.utf8_source(&body)?;
        self.budget.allocate_in(AllocationPhase::Source, 128)?;
        let parsed = self.parse_result(syntax::parse_function(&parameters, &body))?;
        self.budget
            .allocate_in(AllocationPhase::Ast, storage::expression_bytes(&parsed))?;
        let Expr::Function { name, params, body } = parsed else {
            return Err(Fault::Fatal(
                "Invalid dynamic function parser result".into(),
            ));
        };
        // "anonymous" is display metadata, not a self-name lexical binding.
        self.function(name.as_ref(), &params, &body, 0, false)
    }

    // Called only after all Function arguments were converted and the body was
    // removed. A sole owned fragment needs no join/copy; UTF-8 conversion and
    // grammar/resource admission still happen in dynamic_function afterward.
    fn function_parameter_source(&mut self, mut parts: Vec<Vec<u16>>) -> Eval<Vec<u16>> {
        if parts.len() == 1 {
            // Retain first-failure latching without an accepted payload charge.
            self.budget.allocate_in(AllocationPhase::Source, 0)?;
            return Ok(parts.pop().expect("one parameter fragment"));
        }
        let length = parts
            .iter()
            .map(Vec::len)
            .fold(parts.len().saturating_sub(1), usize::saturating_add);
        self.budget
            .allocate_in(AllocationPhase::Source, length.saturating_mul(2))?;
        let mut parameters = Vec::with_capacity(length);
        for (index, part) in parts.into_iter().enumerate() {
            if index != 0 {
                parameters.push(b',' as u16);
            }
            parameters.extend(part);
        }
        Ok(parameters)
    }

    fn object(&mut self, prototype: Option<usize>, array: Option<PrepaidArray>) -> Eval<usize> {
        self.object_with_prototype(prototype.map(PrototypeIdentity::Object), array)
    }
    fn object_with_prototype(
        &mut self,
        prototype: Option<PrototypeIdentity>,
        array: Option<PrepaidArray>,
    ) -> Eval<usize> {
        if self.objects.len() >= MAX_OBJECTS {
            return Err(Fault::Fatal("JavaScript object limit exhausted".into()));
        }
        let array = array.map(PrepaidArray::into_values).transpose()?;
        // The consumed builder admits every array slot exactly once. Metadata
        // remains Runtime storage even when a regex operation produced it.
        self.budget.allocate(128)?;
        let id = self.objects.len();
        self.objects.push(Object {
            properties: Vec::new(),
            array,
            prototype,
            boxed: None,
            regexp: None,
            error: None,
            arguments: false,
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
        self.admit(value)?;
        self.budget.copy(value)
    }

    fn put_own(&mut self, object: usize, key: &str, value: Value, enumerable: bool) -> Eval<()> {
        self.budget.step()?;
        self.admit(&value)?;
        let entry = self
            .objects
            .get_mut(object)
            .ok_or_else(|| exception("TypeError: unknown object"))?;
        if let Some(property) = entry
            .properties
            .iter_mut()
            .find(|property| property.key == key)
        {
            if !property.writable {
                return Ok(());
            }
            self.budget.allocate(value_bytes(&value))?;
            property.value = Stored::Inline(value);
        } else {
            self.budget.allocate(value_bytes(&value))?;
            self.budget.allocate(128 + key.len())?;
            entry.properties.push(Property {
                key: PropertyKey::String(key.into()),
                value: Stored::Inline(value),
                enumerable,
                writable: true,
                configurable: true,
                getter: false,
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
        if let Some(Value::String(units)) = &entry.boxed {
            if key == "length" {
                return Ok(Some(Value::Number(units.len() as f64)));
            }
            if let Some(index) = array_index(key)
                && let Some(unit) = units.get(index)
            {
                self.budget.allocate(2)?;
                return Ok(Some(Value::String(vec![*unit])));
            }
        }
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
            .map(|property| self.budget.copy(property.read_value()?))
            .transpose()
    }
    fn enumeration_owner(&mut self, value: &Value) -> Eval<EnumerationOwner> {
        self.object_identity(value)
    }
    fn native_properties_id(&mut self, name: &str) -> Eval<Option<usize>> {
        for (existing, id) in &self.native_properties {
            if enumeration_equal(&mut self.budget, existing, name)? {
                return Ok(Some(*id));
            }
        }
        Ok(None)
    }
    fn native_property_deleted(&mut self, name: &str, key: &str) -> Eval<bool> {
        for (owner, property) in &self.native_deleted {
            if enumeration_equal(&mut self.budget, owner, name)?
                && enumeration_equal(&mut self.budget, property, key)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn enumeration_parent(&mut self, owner: EnumerationOwner) -> Eval<Option<EnumerationOwner>> {
        self.identity_parent(owner)
    }
    fn enumeration_object_keys(
        &mut self,
        id: usize,
        owner: EnumerationOwner,
        entries: &mut Vec<EnumerationEntry>,
    ) -> Eval<()> {
        let object = &self.objects[id];
        if let Some(Value::String(units)) = &object.boxed {
            for index in 0..units.len() {
                self.budget.step()?;
                self.budget.allocate(20)?;
                enumeration_add(&mut self.budget, entries, owner, &index.to_string(), true)?;
            }
            enumeration_add(&mut self.budget, entries, owner, "length", false)?;
        }
        if let Some(array) = &object.array {
            for (index, item) in array.iter().enumerate() {
                self.budget.step()?;
                if item.is_some() {
                    self.budget.allocate(20)?;
                    enumeration_add(&mut self.budget, entries, owner, &index.to_string(), true)?;
                }
            }
            enumeration_add(&mut self.budget, entries, owner, "length", false)?;
        }
        for property in &object.properties {
            self.budget.step()?;
            let Some(key) = property.key.string() else {
                continue;
            };
            enumeration_add(&mut self.budget, entries, owner, key, property.enumerable)?;
        }
        Ok(())
    }
    fn enumeration_own_keys(
        &mut self,
        owner: EnumerationOwner,
        _root: &Value,
        entries: &mut Vec<EnumerationEntry>,
    ) -> Eval<()> {
        self.budget.step()?;
        match owner {
            EnumerationOwner::Object(id) => self.enumeration_object_keys(id, owner, entries),
            EnumerationOwner::Function(id) => {
                enumeration_add(&mut self.budget, entries, owner, "length", false)?;
                if matches!(self.functions[id].kind, FunctionKind::Bound(_)) {
                    enumeration_add(&mut self.budget, entries, owner, "caller", false)?;
                    enumeration_add(&mut self.budget, entries, owner, "arguments", false)?;
                } else {
                    enumeration_add(&mut self.budget, entries, owner, "name", false)?;
                }
                self.enumeration_object_keys(self.functions[id].properties, owner, entries)
            }
            EnumerationOwner::Native(native) => {
                self.enumeration_object_keys(self.native_properties[native].1, owner, entries)?;
                for key in native_virtual_names(&self.native_properties[native].0) {
                    if self.native_identity_deleted(native, key)? {
                        continue;
                    }
                    enumeration_add(&mut self.budget, entries, owner, key, false)?;
                }
                Ok(())
            }
        }
    }
    fn enumeration_descriptor(
        &mut self,
        owner: EnumerationOwner,
        _root: &Value,
        key: &str,
    ) -> Eval<Option<bool>> {
        Ok(self
            .own_descriptor(owner, KeyRef::String(key))?
            .map(|d| d.enumerable))
    }
    fn enumeration_snapshot(&mut self, root: &Value) -> Eval<Vec<EnumerationEntry>> {
        // Deterministic research policy: snapshot first-visible owner/key pairs,
        // including nonenumerable shadows, before entering the body. New names
        // are excluded. Recheck the same visible owner and enumerability before
        // visiting; deletion/new shadowing skips it. Delete+readd on the same
        // owner before visitation may visit the replacement property.
        let mut entries = Vec::new();
        let mut current = Some(self.enumeration_owner(root)?);
        for _ in 0..MAX_CALLS {
            let Some(owner) = current else {
                return Ok(entries);
            };
            self.enumeration_own_keys(owner, root, &mut entries)?;
            current = self.enumeration_parent(owner)?;
        }
        if current.is_none() {
            Ok(entries)
        } else {
            Err(Fault::Fatal(
                "JavaScript enumeration prototype depth limit exhausted".into(),
            ))
        }
    }
    fn enumeration_visible(
        &mut self,
        root: &Value,
        key: &str,
    ) -> Eval<Option<(EnumerationOwner, bool)>> {
        let mut current = Some(self.enumeration_owner(root)?);
        for _ in 0..MAX_CALLS {
            let Some(owner) = current else {
                return Ok(None);
            };
            if let Some(enumerable) = self.enumeration_descriptor(owner, root, key)? {
                return Ok(Some((owner, enumerable)));
            }
            current = self.enumeration_parent(owner)?;
        }
        if current.is_none() {
            Ok(None)
        } else {
            Err(Fault::Fatal(
                "JavaScript enumeration prototype depth limit exhausted".into(),
            ))
        }
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
        let Some(index) = self.environments[environment]
            .bindings
            .iter()
            .position(|binding| binding.name == name)
        else {
            return self.budget.copy(&Value::Undefined);
        };
        self.materialize_empty_arguments(environment, index)?;
        self.budget
            .copy(&self.environments[environment].bindings[index].value)
    }
    fn binding_value(
        &mut self,
        environment: usize,
        name: &str,
        host: &mut impl Host,
    ) -> Eval<Value> {
        if environment == 0
            && self.objects[0]
                .properties
                .iter()
                .any(|p| p.key == name && p.getter)
        {
            return self.get(&Value::Object(0), name, host);
        }
        self.binding(environment, name)
    }
    fn define(&mut self, environment: usize, name: &str, value: Value) -> Eval<()> {
        if environment == 0 {
            return self.put_own(0, name, value, true);
        }
        self.budget.allocate(value_bytes(&value))?;
        self.store_local_binding(environment, name, value)
    }
    fn bind_parameter_copy(&mut self, environment: usize, name: &str, value: &Value) -> Eval<()> {
        if environment == 0 || environment >= self.environments.len() {
            return Err(Fault::Fatal(
                "Invalid local JavaScript binding environment".into(),
            ));
        }
        // Formals require an independent copy; admission precedes that clone.
        // Moving the paid copy into its local binding creates no extra payload.
        let value = self.copy(value)?;
        self.store_local_binding(environment, name, value)
    }
    // Private local storage only. The two callers retain distinct payload
    // policies: ordinary define charges its transfer; formals make a paid copy.
    // Do not route catches, host ingress or general assignments around define.
    fn store_local_binding(&mut self, environment: usize, name: &str, value: Value) -> Eval<()> {
        if environment == 0 || environment >= self.environments.len() {
            return Err(Fault::Fatal(
                "Invalid local JavaScript binding environment".into(),
            ));
        }
        let bindings = &mut self.environments[environment].bindings;
        if let Some(previous) = bindings.iter_mut().find(|binding| binding.name == name) {
            previous.value = value;
            previous.pending_empty_arguments = None;
        } else {
            self.budget.allocate(128 + name.len())?;
            bindings.push(Binding {
                name: name.into(),
                value,
                deletable: false,
                pending_empty_arguments: None,
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
        params: &Rc<[String]>,
        body: &Rc<[Stmt]>,
        environment: usize,
        self_named: bool,
    ) -> Eval<Value> {
        if self.functions.len() >= MAX_OBJECTS {
            return Err(Fault::Fatal("JavaScript function limit exhausted".into()));
        }
        // Each parse already paid for the immutable slices. A closure owns only
        // fresh instance metadata and its display name, not another code copy.
        let bytes = 128usize.saturating_add(name.map_or(0, String::len));
        self.budget
            .allocate_in(AllocationPhase::FunctionCode, bytes)?;
        let environment = if self_named && name.is_some() {
            self.environment(environment, false)?
        } else {
            environment
        };
        let properties = self.object(Some(self.function_prototype), None)?;
        let id = self.functions.len();
        self.functions.push(Function {
            kind: FunctionKind::Ordinary(Rc::new(Code {
                name: name.cloned(),
                params: Rc::clone(params),
                body: Rc::clone(body),
            })),
            environment,
            properties,
            pending_default_prototype: false,
        });
        self.put_own(properties, "prototype", Value::Undefined, false)?;
        self.functions[id].pending_default_prototype = true;
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
                Stmt::ForIn { binding, body, .. } => {
                    if let ForInBinding::Var { name, .. } = binding.as_ref() {
                        self.declare(environment, name, deletable)?;
                    }
                    self.hoist(
                        std::slice::from_ref(body.as_ref()),
                        environment,
                        lexical,
                        deletable,
                    )?;
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        self.budget.step()?;
                        self.hoist(&case.body, environment, lexical, deletable)?;
                    }
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
    #[inline(never)]
    fn statement(
        &mut self,
        statement: &Stmt,
        labels: &[&str],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Flow> {
        self.budget.enter_evaluation(false)?;
        let result = self.statement_inner(statement, labels, environment, this, host);
        self.budget.leave_evaluation(false);
        result
    }
    fn statement_inner(
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
            Stmt::Var(bindings) => self.variable_statement(bindings, environment, this, host),
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
                self.label_statement(name, body, labels, environment, this, host)
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
            Stmt::ForIn {
                binding,
                object,
                body,
            } => self.for_in(binding, object, body, labels, environment, this, host),
            Stmt::Switch {
                discriminant,
                cases,
            } => self.switch_statement(discriminant, cases, labels, environment, this, host),
            Stmt::Try { .. } => self.try_statement(statement, environment, this, host),
        }
    }

    // Keep bulky branch temporaries outside ordinary dispatch: every retained
    // statement otherwise reserves their combined debug-build stack space.
    fn variable_statement(
        &mut self,
        bindings: &[(String, Option<Expr>)],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Flow> {
        for (name, expr) in bindings {
            if let Some(expr) = expr {
                let value = self.expression(expr, environment, this, host)?;
                let target = self.lookup(environment, name).unwrap_or(0);
                self.write_reference(Reference::Binding(target, name.clone()), value, host)?;
            }
        }
        Ok(Flow::Normal(None))
    }
    fn label_statement(
        &mut self,
        name: &str,
        body: &Stmt,
        labels: &[&str],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Flow> {
        self.budget
            .allocate((labels.len() + 1).saturating_mul(std::mem::size_of::<&str>()))?;
        let mut nested = labels.to_vec();
        nested.push(name);
        match self.statement(body, &nested, environment, this, host)? {
            Flow::Break(Some(target), value) if target == name => Ok(Flow::Normal(value)),
            other => Ok(other),
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
                (body, test.as_deref(), update.as_deref(), false)
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

    fn for_in(
        &mut self,
        binding: &ForInBinding,
        object: &Expr,
        body: &Stmt,
        labels: &[&str],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Flow> {
        let object = self.for_in_object(binding, object, environment, this, host)?;
        if matches!(object, Value::Null | Value::Undefined) {
            return Ok(Flow::Normal(None));
        }
        let entries = self.enumeration_snapshot(&object)?;
        let mut last = None;
        for entry in entries {
            self.budget.step()?;
            if !entry.enumerable
                || self.enumeration_visible(&object, &entry.key)? != Some((entry.owner, true))
            {
                continue;
            }
            self.for_in_assignment(binding, &entry.key, environment, this, host)?;
            match loop_step(
                self.statement(body, &[], environment, this, host)?,
                &mut last,
                labels,
            ) {
                LoopStep::Stop => break,
                LoopStep::Next => {}
                LoopStep::Abrupt(flow) => return Ok(flow),
            }
        }
        Ok(Flow::Normal(last))
    }

    // Setup and per-key assignment may evaluate user code, but their temporary
    // frames must end before retaining this loop around recursive body calls.
    #[inline(never)]
    fn for_in_object(
        &mut self,
        binding: &ForInBinding,
        object: &Expr,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        if let ForInBinding::Var {
            name,
            init: Some(init),
        } = binding
        {
            let value = self.expression(init, environment, this, host)?;
            let target = self.lookup(environment, name).unwrap_or(0);
            self.budget.allocate(name.len())?;
            self.write_reference(Reference::Binding(target, name.clone()), value, host)?;
        }
        let object = self.expression(object, environment, this, host)?;
        if matches!(object, Value::Null | Value::Undefined) {
            return Ok(object);
        }
        if matches!(object, Value::Host(_)) {
            return Err(unsupported("for-in enumeration of host objects"));
        }
        self.boxed(object)
    }

    #[inline(never)]
    fn for_in_assignment(
        &mut self,
        binding: &ForInBinding,
        key: &str,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<()> {
        let reference = match binding {
            ForInBinding::Var { name, .. } => {
                self.budget.allocate(name.len())?;
                Reference::Binding(self.lookup(environment, name).unwrap_or(0), name.clone())
            }
            ForInBinding::Reference(expr) => {
                self.reference(expr, MemberOperation::ForInTarget, environment, this, host)?
            }
        };
        let key = self.text(key)?;
        self.write_reference(reference, key, host)
    }
    fn switch_statement(
        &mut self,
        discriminant: &Expr,
        cases: &[SwitchCase],
        labels: &[&str],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Flow> {
        // Selector temporaries must unwind before recursive clause execution.
        let Some(selected) = self.switch_selection(discriminant, cases, environment, this, host)?
        else {
            return Ok(Flow::Normal(None));
        };
        let mut last = None;
        for case in &cases[selected..] {
            self.budget.step()?;
            match self
                .statements(&case.body, environment, this, host)?
                .update_empty(last.take())
            {
                Flow::Normal(value) => last = value,
                Flow::Break(target, value)
                    if target
                        .as_ref()
                        .is_none_or(|target| labels.contains(&target.as_str())) =>
                {
                    return Ok(Flow::Normal(value));
                }
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal(last))
    }

    #[inline(never)]
    fn switch_selection(
        &mut self,
        discriminant: &Expr,
        cases: &[SwitchCase],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Option<usize>> {
        let value = self.expression(discriminant, environment, this, host)?;
        let mut default = None;
        for (index, case) in cases.iter().enumerate() {
            self.budget.step()?;
            if let Some(test) = &case.test {
                let selector = self.expression(test, environment, this, host)?;
                if strict_equal(&value, &selector) {
                    return Ok(Some(index));
                }
            } else {
                default = Some(index);
            }
        }
        Ok(default)
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
        if let Some((name, catch)) = catch {
            result = match result {
                Err(Fault::Throw(value) | Fault::Member { value, .. }) => {
                    // The binding gets only the original page-visible value.
                    // A later explicit throw cannot inherit this fault's context.
                    let environment = self.environment(environment, false)?;
                    self.define(environment, name, value)?;
                    self.statement(catch, &[], environment, this, host)
                }
                other => other,
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
        operation: MemberOperation,
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
                let mut producer = Producer::simple(ProducerKind::Expression);
                let object = self.member_base(object, environment, this, host, &mut producer)?;
                let property = self.expression(property, environment, this, host)?;
                // Evaluate the key expression, but reject an invalid base before
                // ToPropertyKey can call user code on the resulting key value.
                if matches!(object, Value::Null | Value::Undefined) {
                    return Err(diagnostic::member_fault_observed(
                        operation, &object, &property, producer,
                    ));
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
                self.binding_value(*environment, name, host)
            }
            Reference::Property(object, key) => self.get_key(object, key, host),
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
            Reference::Property(object, key) => self.set_key(object, &key, value, host),
        }
    }
    #[inline(never)]
    fn expression(
        &mut self,
        expr: &Expr,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        self.budget.enter_evaluation(true)?;
        let result = self.expression_inner(expr, environment, this, host, None);
        self.budget.leave_evaluation(true);
        result
    }
    // Identical expression entry/exit accounting, with an observation local to
    // one enclosing member base. Nested evaluation never receives this sink.
    #[inline(never)]
    fn member_base(
        &mut self,
        expr: &Expr,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
        producer: &mut Producer,
    ) -> Eval<Value> {
        self.budget.enter_evaluation(true)?;
        let result = self.expression_inner(expr, environment, this, host, Some(producer));
        self.budget.leave_evaluation(true);
        result
    }
    fn expression_inner(
        &mut self,
        expr: &Expr,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
        producer: Option<&mut Producer>,
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
            Expr::RegExp { pattern, flags } => {
                let regex = self.compile_regexp(pattern, flags)?;
                self.regexp_object(regex)
            }
            Expr::Ident(name) => {
                let value = match self.lookup(environment, name) {
                    Some(environment) => self.binding_value(environment, name, host)?,
                    None => {
                        return Err(exception(format!("ReferenceError: {name} is not defined")));
                    }
                };
                if let Some(producer) = producer {
                    *producer = Producer::simple(ProducerKind::Binding);
                }
                Ok(value)
            }
            Expr::This => self.copy(this),
            Expr::Array(items) => self.array_expression(items, environment, this, host),
            Expr::Object(properties) => self.object_expression(properties, environment, this, host),
            Expr::Function { name, params, body } => {
                self.function(name.as_ref(), params, body, environment, true)
            }
            Expr::Member { .. } => {
                let reference =
                    self.reference(expr, MemberOperation::Read, environment, this, host)?;
                if let Some(producer) = producer {
                    let Reference::Property(object, key) = &reference else {
                        unreachable!("member expression resolves a property")
                    };
                    let mut kind = ProducerKind::PresentProperty;
                    let value = self.get_key_observed(object, key, host, Some(&mut kind))?;
                    *producer = Producer::property(kind, key);
                    Ok(value)
                } else {
                    self.read_reference(&reference, host)
                }
            }
            Expr::Unary { op, expr } => self.unary_expression(op, expr, environment, this, host),
            Expr::Binary { op, left, right } => {
                self.binary_expression(op, left, right, environment, this, host)
            }
            Expr::Assign { op, left, right } => {
                self.assignment_expression(op, left, right, environment, this, host)
            }
            Expr::Update { op, expr, prefix } => {
                self.update_expression(op, expr, *prefix, environment, this, host)
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
                self.call_expression(callee, args, environment, this, host, producer)
            }
            Expr::New { callee, args } => {
                self.new_expression(callee, args, environment, this, host)
            }
        }
    }

    #[inline(never)]
    fn array_expression(
        &mut self,
        items: &[Option<Expr>],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let mut values =
            PrepaidArray::with_slots(&mut self.budget, items.len(), AllocationPhase::Runtime)?;
        for item in items {
            // Evaluated payloads are owned and paid; None preserves a hole.
            let value = item
                .as_ref()
                .map(|expr| self.expression(expr, environment, this, host))
                .transpose()?;
            values.push_owned(&mut self.budget, value)?;
        }
        Ok(Value::Object(
            self.object(Some(self.array_prototype), Some(values))?,
        ))
    }

    #[inline(never)]
    fn object_expression(
        &mut self,
        properties: &[(String, Expr)],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
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

    #[inline(never)]
    fn binary_expression(
        &mut self,
        op: &str,
        left: &Expr,
        right: &Expr,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
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

    #[inline(never)]
    fn update_expression(
        &mut self,
        op: &str,
        expr: &Expr,
        prefix: bool,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let reference =
            self.reference(expr, MemberOperation::UpdateTarget, environment, this, host)?;
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
        Ok(Value::Number(if prefix { new } else { old }))
    }

    // Keep unrelated expression temporaries out of every retained AST frame.
    #[inline(never)]
    fn unary_expression(
        &mut self,
        op: &str,
        expr: &Expr,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        if op == "typeof"
            && let Expr::Ident(name) = expr
            && self.lookup(environment, name).is_none()
        {
            return self.text("undefined");
        }
        if op == "delete" {
            return match expr {
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
                    let reference = self.reference(
                        expr,
                        MemberOperation::DeleteTarget,
                        environment,
                        this,
                        host,
                    )?;
                    if let Reference::Property(object, key) = reference {
                        self.delete_key(object, &key)
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
        match op {
            "!" => Ok(Value::Bool(!value.truthy())),
            "void" => Ok(Value::Undefined),
            "typeof" => self.text(match value {
                Value::Undefined => "undefined",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Symbol(_) => "symbol",
                Value::Function(_) | Value::Native(_) => "function",
                _ => "object",
            }),
            "+" => Ok(Value::Number(self.number(value, host)?)),
            "-" => Ok(Value::Number(-self.number(value, host)?)),
            "~" => Ok(Value::Number((!int32(self.number(value, host)?)) as f64)),
            _ => Err(unsupported(op)),
        }
    }

    // Keep unrelated expression temporaries out of every retained AST frame.
    #[inline(never)]
    fn assignment_expression(
        &mut self,
        op: &str,
        left: &Expr,
        right: &Expr,
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let operation = if op == "=" {
            MemberOperation::WriteTarget
        } else {
            MemberOperation::CompoundTarget
        };
        let reference = self.reference(left, operation, environment, this, host)?;
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
        let value = if matches!(op, "=" | "&&=" | "||=" | "??=") {
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

    // Keep unrelated expression temporaries out of every retained AST frame.
    #[inline(never)]
    fn call_expression(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
        producer: Option<&mut Producer>,
    ) -> Eval<Value> {
        let eval_reference = matches!(callee, Expr::Ident(name) if name == "eval");
        let (callee, mut receiver) = if matches!(callee, Expr::Member { .. }) {
            let reference =
                self.reference(callee, MemberOperation::CallTarget, environment, this, host)?;
            let callee = self.read_reference(&reference, host)?;
            let receiver = if let Reference::Property(value, _) = reference {
                value
            } else {
                unreachable!()
            };
            (callee, receiver)
        } else {
            // These supported non-member references supply undefined. Ordinary
            // non-strict functions substitute the global object in call();
            // native receivers must not be normalized before their dispatch.
            (
                self.expression(callee, environment, this, host)?,
                Value::Undefined,
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
        let kind = match &callee {
            Value::Native(name) if name.starts_with("host.") => ProducerKind::HostCall,
            Value::Native(_) => ProducerKind::NativeCall,
            Value::Function(id)
                if self
                    .functions
                    .get(*id)
                    .is_some_and(|function| matches!(function.kind, FunctionKind::Bound(_))) =>
            {
                ProducerKind::BoundCall
            }
            Value::Function(_) => ProducerKind::UserCall,
            _ => ProducerKind::Expression,
        };
        let value = self.call(callee, receiver, args, direct_eval, host)?;
        if let Some(producer) = producer {
            *producer = Producer::simple(kind);
        }
        Ok(value)
    }

    // Keep unrelated expression temporaries out of every retained AST frame.
    #[inline(never)]
    fn new_expression(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        environment: usize,
        this: &Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let callee = self.expression(callee, environment, this, host)?;
        let args = self.arguments(args, environment, this, host)?;
        self.construct_value(callee, args, host)
    }

    #[inline(never)]
    fn construct_value(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        if let Value::Function(id) = &callee
            && self
                .functions
                .get(*id)
                .is_some_and(|function| matches!(function.kind, FunctionKind::Bound(_)))
        {
            return self.forward_bound(*id, args, true, host);
        }
        if let Value::Native(name) = &callee {
            if name == "Symbol" {
                return Err(exception("TypeError: Symbol is not a constructor"));
            }
            if name == "String" {
                let value = args.into_iter().next().unwrap_or(Value::text(""));
                let units = self.units(value, host)?;
                return self.boxed(Value::String(units));
            }
            if name == "Number" {
                // Argument evaluation has already completed. Consume the first
                // owned operand, converting before allocating the instance.
                let number = match args.into_iter().next() {
                    Some(value) => self.number(value, host)?,
                    None => 0.0,
                };
                return self.boxed(Value::Number(number));
            }
            if name == "Boolean" {
                let boolean = args.first().is_some_and(Value::truthy);
                return self.boxed(Value::Bool(boolean));
            }
            if name == "RegExp" {
                return self.call(
                    Value::Native("RegExp.new".into()),
                    Value::Undefined,
                    args,
                    None,
                    host,
                );
            }
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
        let prototype = self.get(&callee, "prototype", host)?;
        let prototype = if prototype.primitive() {
            PrototypeIdentity::Object(self.object_prototype)
        } else {
            self.object_identity(&prototype)?
        };
        let object = Value::Object(self.object_with_prototype(Some(prototype), None)?);
        let receiver = self.copy(&object)?;
        let result = self.call(callee, receiver, args, None, host)?;
        Ok(if result.primitive() { object } else { result })
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
        self.get_observed(value, key, host, None)
    }
    fn get_observed(
        &mut self,
        value: &Value,
        key: &str,
        host: &mut impl Host,
        observation: Option<&mut ProducerKind>,
    ) -> Eval<Value> {
        self.budget.step()?;
        if let Value::Host(object) = value {
            let value = host.get(object, key).map_err(exception)?;
            self.admit(&value)?;
            self.budget.allocate(value_bytes(&value))?;
            ProducerKind::HostGet.record(observation);
            return Ok(value);
        }
        if let Value::String(units) = value {
            if key == "length" {
                ProducerKind::PresentProperty.record(observation);
                return Ok(Value::Number(units.len() as f64));
            }
            if let Some(index) = array_index(key) {
                return match units.get(index) {
                    Some(unit) => {
                        let value = self.string(vec![*unit])?;
                        ProducerKind::PresentProperty.record(observation);
                        Ok(value)
                    }
                    None => {
                        // The existing primitive-string index fast path does
                        // not traverse prototypes, so it cannot establish absence.
                        ProducerKind::Expression.record(observation);
                        Ok(Value::Undefined)
                    }
                };
            }
        }
        self.read_property_observed(value, KeyRef::String(key), host, observation)
    }
    fn set(&mut self, object: Value, key: &str, value: Value, host: &mut impl Host) -> Eval<()> {
        self.budget.step()?;
        match object {
            Value::Host(object) => {
                let value = if host.string_assignment(&object, key) {
                    Value::String(self.units(value, host)?)
                } else {
                    value
                };
                host.set(&object, key, value).map_err(exception)
            }
            Value::Object(id) => {
                match self
                    .property_write_action(PrototypeIdentity::Object(id), KeyRef::String(key))?
                {
                    WriteAction::Ignore => return Ok(()),
                    WriteAction::Setter { object, index } => {
                        return self.invoke_property_setter(
                            object,
                            index,
                            &Value::Object(id),
                            value,
                            host,
                        );
                    }
                    WriteAction::Own => {}
                }
                if id == 0
                    && let Some((_, object, property)) =
                        self.global_setters.iter().find(|(name, _, _)| name == key)
                {
                    let object = object.clone();
                    let property = property.clone();
                    let value = if host.string_assignment(&object, &property) {
                        Value::String(self.units(value, host)?)
                    } else {
                        value
                    };
                    return host.set(&object, &property, value).map_err(exception);
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
                let function = self
                    .functions
                    .get(id)
                    .ok_or_else(|| exception("TypeError: unknown function"))?;
                if key == "length"
                    || key == "name" && matches!(function.kind, FunctionKind::Ordinary(_))
                {
                    return Ok(());
                }
                let object = function.properties;
                match self
                    .property_write_action(PrototypeIdentity::Function(id), KeyRef::String(key))?
                {
                    WriteAction::Ignore => return Ok(()),
                    WriteAction::Setter { object, index } => {
                        return self.invoke_property_setter(
                            object,
                            index,
                            &Value::Function(id),
                            value,
                            host,
                        );
                    }
                    WriteAction::Own => {}
                }
                self.put_own(object, key, value, true)?;
                if key == "prototype" {
                    // Readonly/no-op and failed paid writes return above. An
                    // inherited child shadow takes the ordinary-object path.
                    self.functions[id].pending_default_prototype = false;
                }
                Ok(())
            }
            Value::Native(name) => {
                let virtual_key = native_virtual_names(&name).contains(&key);
                if matches!(key, "name" | "length") || (key == "prototype" && virtual_key) {
                    return Ok(());
                }
                let enumerable = !virtual_key || self.native_property_deleted(&name, key)?;
                let native = self.native_identity(&name)?;
                match self
                    .property_write_action(PrototypeIdentity::Native(native), KeyRef::String(key))?
                {
                    WriteAction::Ignore => return Ok(()),
                    WriteAction::Setter { object, index } => {
                        return self.invoke_property_setter(
                            object,
                            index,
                            &Value::Native(name),
                            value,
                            host,
                        );
                    }
                    WriteAction::Own => {}
                }
                let object = self.native_properties[native].1;
                self.put_own(object, key, value, enumerable)
            }
            Value::Null | Value::Undefined => Err(exception(
                "TypeError: property assignment on null or undefined",
            )),
            _ => Ok(()), // Non-strict assignment to transient primitive wrappers.
        }
    }
    fn delete(&mut self, object: Value, key: &str) -> Eval<Value> {
        if let Value::Function(id) = &object {
            let function = self
                .functions
                .get(*id)
                .ok_or_else(|| exception("TypeError: unknown function"))?;
            let protected = match function.kind {
                FunctionKind::Ordinary(_) => matches!(key, "name" | "length" | "prototype"),
                FunctionKind::Bound(_) => matches!(key, "length" | "caller" | "arguments"),
            };
            if protected {
                return Ok(Value::Bool(false));
            }
        }
        if let Value::Native(name) = &object {
            if let Some(id) = self.native_properties_id(name)? {
                if self.objects[id]
                    .properties
                    .iter()
                    .any(|p| p.key == key && !p.configurable)
                {
                    return Ok(Value::Bool(false));
                }
            }
            let virtual_key = native_virtual_names(name).contains(&key);
            if matches!(key, "name" | "length") || (key == "prototype" && virtual_key) {
                return Ok(Value::Bool(false));
            }
            if virtual_key && !self.native_property_deleted(name, key)? {
                self.budget
                    .allocate(64usize.saturating_add(name.len()).saturating_add(key.len()))?;
                self.native_deleted.push((name.clone(), key.into()));
            }
            if let Some(id) = self.native_properties_id(name)? {
                let properties = &mut self.objects[id].properties;
                for index in 0..properties.len() {
                    if let Some(name) = properties[index].key.string()
                        && enumeration_equal(&mut self.budget, name, key)?
                    {
                        properties.remove(index);
                        break;
                    }
                }
            }
            return Ok(Value::Bool(true));
        }
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
        if let Some(Value::String(units)) = &object.boxed
            && (key == "length" || array_index(key).is_some_and(|index| index < units.len()))
        {
            return Ok(Value::Bool(false));
        }
        if object.regexp.is_some()
            && matches!(
                key,
                "source" | "global" | "ignoreCase" | "multiline" | "lastIndex"
            )
        {
            return Ok(Value::Bool(false));
        }
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
        if object
            .properties
            .iter()
            .any(|p| p.key == key && !p.configurable)
        {
            return Ok(Value::Bool(false));
        }
        object.properties.retain(|property| property.key != key);
        Ok(Value::Bool(true))
    }
    fn property_key(&mut self, value: Value, host: &mut impl Host) -> Eval<PropertyKey> {
        let value = self.primitive(value, Hint::String, host)?;
        match value {
            Value::Symbol(symbol) => Ok(PropertyKey::Symbol(symbol)),
            Value::String(units) => String::from_utf16(&units)
                .map(PropertyKey::String)
                .map_err(|_| unsupported("lone-surrogate property keys")),
            other => Ok(PropertyKey::String(other.as_text())),
        }
    }
    fn primitive(&mut self, value: Value, hint: Hint, host: &mut impl Host) -> Eval<Value> {
        if value.primitive() {
            return Ok(value);
        }
        if !matches!(value, Value::Host(_)) {
            let key = PropertyKey::Symbol(self.to_primitive.as_ref().unwrap().clone());
            let method = self.get_key(&value, &key, host)?;
            if !matches!(method, Value::Undefined | Value::Null) {
                if !method.callable() {
                    return Err(exception("TypeError: Symbol.toPrimitive is not callable"));
                }
                let receiver = self.copy(&value)?;
                let hint_value = self.text(match hint {
                    Hint::Default => "default",
                    Hint::String => "string",
                    Hint::Number => "number",
                })?;
                // This hook creates its own one-slot argument vector; unlike
                // ordinary source calls, no arguments() producer prepaid it.
                self.budget.allocate(64)?;
                let result = self.call(method, receiver, vec![hint_value], None, host)?;
                if result.primitive() {
                    return Ok(result);
                }
                return Err(exception(
                    "TypeError: Symbol.toPrimitive must return a primitive",
                ));
            }
        }
        for key in if matches!(hint, Hint::String) {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
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
        primitive_number(&self.primitive(value, Hint::Number, host)?)
    }
    fn units(&mut self, value: Value, host: &mut impl Host) -> Eval<Vec<u16>> {
        self.units_in(value, host, AllocationPhase::Runtime)
    }
    fn units_in(
        &mut self,
        value: Value,
        host: &mut impl Host,
        phase: AllocationPhase,
    ) -> Eval<Vec<u16>> {
        let value = self.primitive(value, Hint::String, host)?;
        match value {
            Value::Symbol(_) => Err(exception("TypeError: cannot convert Symbol to string")),
            Value::String(units) => Ok(units),
            other => {
                let text = other.as_text();
                self.budget
                    .allocate_in(phase, text.len().saturating_mul(2))?;
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
                if right.primitive() {
                    return Err(exception("TypeError: right side of in is not an object"));
                }
                let key = self.property_key(left, host)?;
                return Ok(Value::Bool(self.has_key(&right, &key, false)?));
            }
            "instanceof" => {
                if !right.callable() {
                    return Err(exception(
                        "TypeError: right side of instanceof is not callable",
                    ));
                }
                let right = self.bound_instance_target(right)?;
                if left.primitive() {
                    return Ok(Value::Bool(false));
                }
                let prototype = self.get(&right, "prototype", host)?;
                if prototype.primitive() {
                    return Err(exception(
                        "TypeError: constructor prototype is not an object",
                    ));
                }
                let target = self.object_identity(&prototype)?;
                let left = self.object_identity(&left)?;
                let mut current = self.identity_parent(left)?;
                for _ in 0..MAX_CALLS {
                    let Some(id) = current else {
                        return Ok(Value::Bool(false));
                    };
                    if id == target {
                        return Ok(Value::Bool(true));
                    }
                    current = self.identity_parent(id)?;
                }
                return Err(Fault::Fatal(
                    "JavaScript prototype depth limit exhausted".into(),
                ));
            }
            _ => {}
        }
        let hint = if op == "+" {
            Hint::Default
        } else {
            Hint::Number
        };
        let left = self.primitive(left, hint, host)?;
        // Numeric-only operators finish converting the left operand before
        // invoking right-side coercion hooks. Addition and comparisons first
        // obtain both primitives, as required by their distinct algorithms.
        let left = if matches!(op, "+" | "<" | ">" | "<=" | ">=") {
            left
        } else {
            Value::Number(primitive_number(&left)?)
        };
        let right = self.primitive(right, hint, host)?;
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
                _ => primitive_number(&left)?.partial_cmp(&primitive_number(&right)?),
            };
            return Ok(Value::Bool(order.is_some_and(|order| match op {
                "<" => order.is_lt(),
                ">" => order.is_gt(),
                "<=" => !order.is_gt(),
                _ => !order.is_lt(),
            })));
        }
        let (a, b) = (primitive_number(&left)?, primitive_number(&right)?);
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
                Ok(primitive_number(&left)? == primitive_number(&right)?)
            }
            (Value::Bool(value), _) => {
                self.equal(Value::Number(if *value { 1.0 } else { 0.0 }), right, host)
            }
            (_, Value::Bool(value)) => {
                self.equal(left, Value::Number(if *value { 1.0 } else { 0.0 }), host)
            }
            _ if !left.primitive()
                && matches!(
                    right,
                    Value::Number(_) | Value::String(_) | Value::Symbol(_)
                ) =>
            {
                let left = self.primitive(left, Hint::Default, host)?;
                self.equal(left, right, host)
            }
            _ if !right.primitive()
                && matches!(left, Value::Number(_) | Value::String(_) | Value::Symbol(_)) =>
            {
                let right = self.primitive(right, Hint::Default, host)?;
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
            Value::Symbol(_) => self.symbol_prototype,
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
        self.budget.enter_evaluation(false)?;
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
                let mut args = args;
                for index in host.string_arguments(&name) {
                    if let Some(argument) = args.get_mut(*index) {
                        let value = std::mem::replace(argument, Value::Undefined);
                        *argument = Value::String(self.units(value, host)?);
                    }
                }
                let value = host.call(&name, this, args).map_err(exception)?;
                self.admit(&value)?;
                self.budget.allocate(value_bytes(&value))?;
                Ok(value)
            }
            Value::Native(name) => self.native(&name, this, args, host),
            Value::Function(id) => {
                let function = self
                    .functions
                    .get(id)
                    .ok_or_else(|| exception("TypeError: unknown function"))?;
                let code = match &function.kind {
                    FunctionKind::Ordinary(code) => Rc::clone(code),
                    FunctionKind::Bound(_) => return self.forward_bound(id, args, false, host),
                };
                let parent = function.environment;
                let environment = self.environment(parent, true)?;
                let this = if matches!(this, Value::Undefined | Value::Null) {
                    Value::Object(0)
                } else {
                    self.boxed(this)?
                };
                for (index, param) in code.params.iter().enumerate() {
                    self.bind_parameter_copy(
                        environment,
                        param,
                        args.get(index).unwrap_or(&Value::Undefined),
                    )?;
                }
                if !code.params.iter().any(|name| name == "arguments") {
                    if args.is_empty() {
                        self.defer_empty_arguments(environment, id)?;
                    } else {
                        // Parameter copies/bindings above remain independent. The
                        // incoming Vec<Value> cannot be assumed to reuse storage as
                        // Vec<Option<Value>>: pay these new slots, then move payloads.
                        let mut items = PrepaidArray::with_slots(
                            &mut self.budget,
                            args.len(),
                            AllocationPhase::Runtime,
                        )?;
                        for arg in args {
                            items.push_owned(&mut self.budget, Some(arg))?;
                        }
                        let arguments = self.object(Some(self.object_prototype), Some(items))?;
                        self.objects[arguments].arguments = true;
                        self.put_own(arguments, "callee", Value::Function(id), false)?;
                        self.define(environment, "arguments", Value::Object(arguments))?;
                    }
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
        self.budget.leave_evaluation(false);
        result
    }

    fn native(
        &mut self,
        name: &str,
        this: Value,
        mut args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        if name == "Symbol" || name.starts_with("Symbol.") {
            return self.symbol_native(name, this, args, host);
        }
        // Forwarding branches own their arguments and do not use this copy.
        // Copy only for branches that actually consume `first`; argument
        // expression evaluation and user coercion order remain unchanged.
        let needs_first = matches!(
            name,
            "encodeURI"
                | "encodeURIComponent"
                | "decodeURI"
                | "decodeURIComponent"
                | "RegExp.exec"
                | "RegExp.test"
                | "String"
                | "Number"
                | "Boolean"
                | "Object"
                | "Array.isArray"
                | "Function.apply"
                | "Object.hasOwnProperty"
                | "Object.keys"
                | "Object.getOwnPropertyNames"
                | "Object.getOwnPropertySymbols"
                | "Object.create"
                | "Object.getPrototypeOf"
                | "Number.isNaN"
                | "Number.isFinite"
                | "Number.isInteger"
                | "isNaN"
                | "isFinite"
                | "parseInt"
                | "parseFloat"
                | "Error"
                | "TypeError"
                | "RangeError"
                | "ReferenceError"
                | "URIError"
                | "SyntaxError"
                | "Number.toString"
                | "Number.toFixed"
        ) || (name == "Array"
            && args.len() == 1
            && matches!(args.first(), Some(Value::Number(_))))
            || (name.starts_with("Math.")
                && !matches!(name, "Math.random" | "Math.min" | "Math.max"));
        let first = if needs_first {
            self.copy(args.first().unwrap_or(&Value::Undefined))?
        } else {
            Value::Undefined
        };
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
            "Function.bind" => self.bind_function(this, args, host),
            "RegExp" | "RegExp.new" => self.regexp_constructor(args, name == "RegExp.new", host),
            "RegExp.exec" | "RegExp.test" | "RegExp.toString" => {
                self.regexp_method(name, this, first, host)
            }
            "String" => {
                if args.is_empty() {
                    self.text("")
                } else if matches!(first, Value::Symbol(_)) {
                    self.string_symbol(first)
                } else {
                    let units = self.units(first, host)?;
                    // ToString either transfers a paid buffer or charges its
                    // conversion before allocating one.
                    Ok(Value::String(units))
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
                    let length = primitive_number(&first)?;
                    if !length.is_finite() || length < 0.0 || length.fract() != 0.0 {
                        return Err(exception("RangeError: invalid array length"));
                    }
                    if length > MAX_ARRAY as f64 {
                        return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
                    }
                    let mut array = PrepaidArray::with_slots(
                        &mut self.budget,
                        length as usize,
                        AllocationPhase::Runtime,
                    )?;
                    for _ in 0..length as usize {
                        array.push_owned(&mut self.budget, None)?;
                    }
                    array
                } else {
                    // Arguments already own their paid payloads, but this is a
                    // distinct array-slot vector, not the incoming call vector.
                    let mut array = PrepaidArray::with_slots(
                        &mut self.budget,
                        args.len(),
                        AllocationPhase::Runtime,
                    )?;
                    for arg in args {
                        array.push_owned(&mut self.budget, Some(arg))?;
                    }
                    array
                };
                Ok(Value::Object(
                    self.object(Some(self.array_prototype), Some(array))?,
                ))
            }
            "Array.isArray" => Ok(Value::Bool(self.is_array(&first))),
            "Array.concat" => self.array_concat(this, args, host),
            "Array.forEach" | "Array.map" | "Array.filter" | "Array.some" | "Array.every"
            | "Array.reduce" | "Array.reduceRight" => self.array_callback(name, this, args, host),
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
            "Object.toString" => self.object_tag(this, host),
            "Object.hasOwnProperty" => {
                let key = self.property_key(first, host)?;
                match &this {
                    Value::Object(_) | Value::Function(_) | Value::Native(_) => {
                        Ok(Value::Bool(self.has_key(&this, &key, true)?))
                    }
                    Value::Host(_) => return Err(unsupported("hasOwnProperty on host objects")),
                    _ => Ok(Value::Bool(false)),
                }
            }
            "Object.getOwnPropertySymbols" => self.own_symbols(first),
            "Object.keys" | "Object.getOwnPropertyNames" => {
                let owner = match &first {
                    Value::Object(_) | Value::Function(_) | Value::Native(_) => {
                        self.enumeration_owner(&first)?
                    }
                    _ => return Err(unsupported("Object keys on primitive/host values")),
                };
                let mut keys = Vec::new();
                self.enumeration_own_keys(owner, &first, &mut keys)?;
                let mut values = PrepaidArray::growing(AllocationPhase::Runtime);
                for entry in keys {
                    self.budget.step()?;
                    if entry.enumerable || name.ends_with("Names") {
                        let key = self.text(&entry.key)?;
                        values.push_owned(&mut self.budget, Some(key))?;
                    }
                }
                Ok(Value::Object(
                    self.object(Some(self.array_prototype), Some(values))?,
                ))
            }
            "Object.create" => {
                if first.primitive() && !matches!(first, Value::Null) {
                    return Err(exception("TypeError: prototype must be an object or null"));
                }
                let prototype = match first {
                    Value::Null => None,
                    Value::Object(_) | Value::Function(_) | Value::Native(_) => {
                        Some(self.object_identity(&first)?)
                    }
                    Value::Host(_) => return Err(unsupported("host prototype identities")),
                    _ => return Err(exception("TypeError: prototype must be an object or null")),
                };
                let object = self.object_with_prototype(prototype, None)?;
                match args.into_iter().nth(1) {
                    Some(map) if !matches!(map, Value::Undefined) => {
                        self.create_described_object(object, map, host)
                    }
                    _ => Ok(Value::Object(object)),
                }
            }
            "Object.getPrototypeOf" => {
                let owner = self.object_identity(&first)?;
                match self.identity_parent(owner)? {
                    Some(prototype) => self.identity_value(prototype),
                    None => Ok(Value::Null),
                }
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
                let kind = ErrorKind::from_name(name).expect("matched intrinsic error family");
                let object = self.error_instance(kind)?;
                if !matches!(first, Value::Undefined) {
                    let units = self.units(first, host)?;
                    self.put_own(object, "message", Value::String(units), false)?;
                }
                Ok(Value::Object(object))
            }
            "Error.toString" => self.error_to_string(this, host),
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
                    return self.text(&format!(
                        "{:.*}",
                        digits as usize,
                        primitive_number(&value)?
                    ));
                }
                self.text(&value.as_text())
            }
            _ => Err(unsupported(name)),
        }
    }
    fn regexp_result<T>(&mut self, result: Result<T, regexp::Error>) -> Eval<T> {
        match result {
            Ok(value) => Ok(value),
            Err(regexp::Error::Syntax(message)) => {
                Err(Fault::Throw(self.error_object("SyntaxError", &message)?))
            }
            Err(regexp::Error::Limit(message)) => Err(Fault::Fatal(message)),
        }
    }
    fn compile_regexp(&mut self, pattern: &[u16], flags: &str) -> Eval<Rc<regexp::Regex>> {
        // Charge every attempt, including catchable syntax failures. Compiler
        // workspaces also have independent structural limits in regexp.rs.
        let reserved = pattern.len().saturating_mul(128).saturating_add(1024);
        self.budget
            .allocate_in(AllocationPhase::RegexCompile, reserved)?;
        let compiled = self.regexp_result(regexp::Regex::compile(pattern, flags))?;
        self.budget.allocate_in(
            AllocationPhase::RegexCompile,
            compiled.estimated_bytes().saturating_sub(reserved),
        )?;
        Ok(Rc::new(compiled))
    }
    fn regexp_source(&mut self, pattern: &[u16]) -> Eval<Value> {
        self.budget.allocate_in(
            AllocationPhase::RegexResult,
            pattern.len().saturating_mul(12).saturating_add(8),
        )?;
        let mut source = Vec::new();
        if pattern.is_empty() {
            source.extend("(?:)".encode_utf16());
        }
        let mut escaped = false;
        for &unit in pattern {
            // Constructor patterns may contain a backslash followed by a raw
            // line terminator. Replace that identity escape, rather than adding
            // a second backslash and changing the pattern when source is reused.
            if escaped && matches!(unit, 10 | 13 | 0x2028 | 0x2029) {
                source.pop();
            }
            match unit {
                10 => source.extend("\\n".encode_utf16()),
                13 => source.extend("\\r".encode_utf16()),
                0x2028 => source.extend("\\u2028".encode_utf16()),
                0x2029 => source.extend("\\u2029".encode_utf16()),
                47 if !escaped => source.extend("\\/".encode_utf16()),
                _ => source.push(unit),
            }
            escaped = unit == 92 && !escaped;
        }
        Ok(Value::String(source))
    }
    fn init_regexp(&mut self, id: usize, regex: Rc<regexp::Regex>) -> Eval<()> {
        let source = self.regexp_source(regex.pattern())?;
        self.put_own(id, "source", source, false)?;
        self.put_own(id, "global", Value::Bool(regex.global()), false)?;
        self.put_own(id, "ignoreCase", Value::Bool(regex.ignore_case()), false)?;
        self.put_own(id, "multiline", Value::Bool(regex.multiline()), false)?;
        self.put_own(id, "lastIndex", Value::Number(0.0), false)?;
        self.objects[id].regexp = Some(regex);
        Ok(())
    }
    fn regexp_object(&mut self, regex: Rc<regexp::Regex>) -> Eval<Value> {
        let id = self.object(Some(self.regexp_prototype), None)?;
        self.init_regexp(id, regex)?;
        Ok(Value::Object(id))
    }
    fn as_regexp(&self, value: &Value) -> Option<(usize, Rc<regexp::Regex>)> {
        if let Value::Object(id) = value {
            self.objects
                .get(*id)?
                .regexp
                .as_ref()
                .map(|regex| (*id, Rc::clone(regex)))
        } else {
            None
        }
    }
    fn regexp_constructor(
        &mut self,
        args: Vec<Value>,
        construct: bool,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let pattern = self.copy(args.first().unwrap_or(&Value::Undefined))?;
        let flags = self.copy(args.get(1).unwrap_or(&Value::Undefined))?;
        if let Some((_, regex)) = self.as_regexp(&pattern) {
            if !matches!(flags, Value::Undefined) {
                return Err(Fault::Throw(self.error_object(
                    "TypeError",
                    "RegExp flags must be undefined when cloning a RegExp",
                )?));
            }
            return if construct {
                self.regexp_object(regex)
            } else {
                Ok(pattern)
            };
        }
        let pattern = if matches!(pattern, Value::Undefined) {
            Vec::new()
        } else {
            self.units(pattern, host)?
        };
        let flags = if matches!(flags, Value::Undefined) {
            Vec::new()
        } else {
            self.units(flags, host)?
        };
        self.budget.allocate(flags.len().saturating_mul(3))?;
        let flags = match String::from_utf16(&flags) {
            Ok(flags) => flags,
            Err(_) => {
                return Err(Fault::Throw(
                    self.error_object("SyntaxError", "Invalid RegExp flags")?,
                ));
            }
        };
        let regex = self.compile_regexp(&pattern, &flags)?;
        self.regexp_object(regex)
    }
    fn regexp_find(
        &mut self,
        regex: &regexp::Regex,
        input: &[u16],
        start: usize,
    ) -> Eval<Option<regexp::Match>> {
        // The compiler bounds capture count and matching workspaces; returned
        // capture storage is charged before the matcher can allocate it.
        self.budget.allocate_in(
            AllocationPhase::RegexResult,
            regex.pattern().len().saturating_add(1).saturating_mul(32),
        )?;
        let result = regex.find(input, start, &mut self.budget.fuel);
        self.regexp_result(result)
    }
    fn regexp_exec(
        &mut self,
        id: usize,
        regex: &regexp::Regex,
        input: &[u16],
        host: &mut impl Host,
    ) -> Eval<Option<regexp::Match>> {
        let last = self.own(id, "lastIndex")?.unwrap_or(Value::Undefined);
        let last = self.number(last, host)?;
        let last = if last.is_nan() { 0.0 } else { last.trunc() };
        let start = if regex.global() { last } else { 0.0 };
        let matched = if start < 0.0 || start > input.len() as f64 {
            None
        } else {
            self.regexp_find(regex, input, start as usize)?
        };
        if let Some(found) = &matched {
            if regex.global() {
                self.put_own(id, "lastIndex", Value::Number(found.end as f64), false)?;
            }
        } else {
            self.put_own(id, "lastIndex", Value::Number(0.0), false)?;
        }
        Ok(matched)
    }
    fn slice_value(
        &mut self,
        input: &[u16],
        start: usize,
        end: usize,
        phase: AllocationPhase,
    ) -> Eval<Value> {
        self.budget
            .allocate_in(phase, end.saturating_sub(start).saturating_mul(2))?;
        Ok(Value::String(input[start..end].to_vec()))
    }
    fn match_array(&mut self, input: &[u16], found: regexp::Match) -> Eval<Value> {
        let mut values = PrepaidArray::with_slots(
            &mut self.budget,
            found.captures.len(),
            AllocationPhase::RegexResult,
        )?;
        for capture in found.captures {
            let value = match capture {
                Some((start, end)) => {
                    self.slice_value(input, start, end, AllocationPhase::RegexResult)?
                }
                None => Value::Undefined,
            };
            // slice_value paid the new capture buffer; missing captures are
            // present undefined values, not array holes.
            values.push_owned(&mut self.budget, Some(value))?;
        }
        let id = self.object(Some(self.array_prototype), Some(values))?;
        self.put_own(id, "index", Value::Number(found.start as f64), true)?;
        let original = self.slice_value(input, 0, input.len(), AllocationPhase::RegexResult)?;
        self.put_own(id, "input", original, true)?;
        Ok(Value::Object(id))
    }
    fn regexp_method(
        &mut self,
        name: &str,
        this: Value,
        first: Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let Some((id, regex)) = self.as_regexp(&this) else {
            return Err(Fault::Throw(
                self.error_object("TypeError", "Incompatible RegExp receiver")?,
            ));
        };
        if name == "RegExp.toString" {
            let Value::String(source) = self.regexp_source(regex.pattern())? else {
                unreachable!()
            };
            let mut output = Vec::new();
            self.append_units(&mut output, &[47], AllocationPhase::RegexResult)?;
            self.append_units(&mut output, &source, AllocationPhase::RegexResult)?;
            self.append_units(&mut output, &[47], AllocationPhase::RegexResult)?;
            for flag in regex.flags().encode_utf16() {
                self.append_units(&mut output, &[flag], AllocationPhase::RegexResult)?;
            }
            return Ok(Value::String(output));
        }
        let input = self.units(first, host)?;
        let matched = self.regexp_exec(id, &regex, &input, host)?;
        if name == "RegExp.test" {
            return Ok(Value::Bool(matched.is_some()));
        }
        match matched {
            Some(found) => self.match_array(&input, found),
            None => Ok(Value::Null),
        }
    }
    fn string_regexp_match(
        &mut self,
        name: &str,
        input: &[u16],
        pattern: Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let value = if self.as_regexp(&pattern).is_some() {
            pattern
        } else {
            self.regexp_constructor(vec![pattern], true, host)?
        };
        let (id, regex) = self.as_regexp(&value).expect("new RegExp state");
        if name == "String.search" {
            return Ok(Value::Number(
                self.regexp_find(&regex, input, 0)?
                    .map_or(-1.0, |found| found.start as f64),
            ));
        }
        if !regex.global() {
            return match self.regexp_exec(id, &regex, input, host)? {
                Some(found) => self.match_array(input, found),
                None => Ok(Value::Null),
            };
        }
        self.put_own(id, "lastIndex", Value::Number(0.0), false)?;
        let mut values = PrepaidArray::growing(AllocationPhase::RegexResult);
        while let Some(found) = self.regexp_exec(id, &regex, input, host)? {
            let value =
                self.slice_value(input, found.start, found.end, AllocationPhase::RegexResult)?;
            values.push_owned(&mut self.budget, Some(value))?;
            if found.start == found.end {
                self.put_own(
                    id,
                    "lastIndex",
                    Value::Number((found.end + 1) as f64),
                    false,
                )?;
            }
        }
        if values.is_empty() {
            Ok(Value::Null)
        } else {
            Ok(Value::Object(
                self.object(Some(self.array_prototype), Some(values))?,
            ))
        }
    }
    fn append_units(
        &mut self,
        output: &mut Vec<u16>,
        units: &[u16],
        phase: AllocationPhase,
    ) -> Eval<()> {
        self.budget
            .allocate_in(phase, units.len().saturating_mul(2))?;
        output.extend_from_slice(units);
        Ok(())
    }
    fn plain_find(
        &mut self,
        input: &[u16],
        search: &[u16],
        start: usize,
    ) -> Eval<Option<(usize, usize)>> {
        if search.len() > input.len() {
            return Ok(None);
        }
        for position in start..=input.len() - search.len() {
            self.budget.step()?;
            let mut same = true;
            for (left, right) in input[position..position + search.len()].iter().zip(search) {
                self.budget.step()?;
                if left != right {
                    same = false;
                    break;
                }
            }
            if same {
                return Ok(Some((position, position + search.len())));
            }
        }
        Ok(None)
    }
    fn replacement_text(
        &mut self,
        output: &mut Vec<u16>,
        replacement: &[u16],
        input: &[u16],
        found: &regexp::Match,
        phase: AllocationPhase,
    ) -> Eval<()> {
        let mut index = 0;
        while index < replacement.len() {
            self.budget.step()?;
            let unit = replacement[index];
            if unit != 36 || index + 1 == replacement.len() {
                self.append_units(output, &[unit], phase)?;
                index += 1;
                continue;
            }
            let next = replacement[index + 1];
            let span = match next {
                36 => {
                    self.append_units(output, &[36], phase)?;
                    index += 2;
                    continue;
                }
                38 => Some((found.start, found.end)),
                96 => Some((0, found.start)),
                39 => Some((found.end, input.len())),
                48..=57 => {
                    let first = (next - 48) as usize;
                    let second = replacement
                        .get(index + 2)
                        .filter(|&&u| (48..=57).contains(&u));
                    let two = second.map(|&u| first * 10 + (u - 48) as usize);
                    let (capture, used) = if two.is_some_and(|n| n > 0 && n < found.captures.len())
                    {
                        (two.unwrap(), 3)
                    } else if first > 0 && first < found.captures.len() {
                        (first, 2)
                    } else {
                        self.append_units(output, &[36], phase)?;
                        index += 1;
                        continue;
                    };
                    if let Some((start, end)) = found.captures[capture] {
                        self.append_units(output, &input[start..end], phase)?;
                    }
                    index += used;
                    continue;
                }
                _ => {
                    self.append_units(output, &[36], phase)?;
                    index += 1;
                    continue;
                }
            };
            if let Some((start, end)) = span {
                self.append_units(output, &input[start..end], phase)?;
            }
            index += 2;
        }
        Ok(())
    }
    fn string_replace(
        &mut self,
        input: &[u16],
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let search = self.copy(args.first().unwrap_or(&Value::Undefined))?;
        let replacement = self.copy(args.get(1).unwrap_or(&Value::Undefined))?;
        let regex = self.as_regexp(&search);
        let phase = if regex.is_some() {
            AllocationPhase::RegexResult
        } else {
            AllocationPhase::Runtime
        };
        let plain = if regex.is_none() {
            Some(self.units(search, host)?)
        } else {
            None
        };
        // Find before invoking callbacks: callbacks may mutate the RegExp or
        // recurse, but cannot invalidate matcher borrows or alter this match set.
        let mut matches = Vec::new();
        if let Some((id, regex)) = regex {
            if regex.global() {
                self.put_own(id, "lastIndex", Value::Number(0.0), false)?;
            }
            while let Some(found) = self.regexp_exec(id, &regex, input, host)? {
                if matches.len() >= MAX_ARRAY {
                    return Err(Fault::Fatal(
                        "JavaScript match result limit exhausted".into(),
                    ));
                }
                self.budget
                    .allocate_in(phase, 64 + found.captures.len().saturating_mul(32))?;
                let empty = found.start == found.end;
                let next = found.end + 1;
                matches.push(found);
                if !regex.global() {
                    break;
                }
                if empty {
                    self.put_own(id, "lastIndex", Value::Number(next as f64), false)?;
                }
            }
        } else if let Some((start, end)) =
            self.plain_find(input, plain.as_ref().expect("plain search"), 0)?
        {
            self.budget.allocate(96)?;
            matches.push(regexp::Match {
                start,
                end,
                captures: vec![Some((start, end))],
            });
        }
        // ES5.1 describes replacement coercion after searching. Its side
        // effects, like callback side effects, survive the completed search.
        let replacement_string = if replacement.callable() {
            None
        } else {
            let value = self.copy(&replacement)?;
            Some(self.units(value, host)?)
        };
        let mut output = Vec::new();
        let mut previous = 0;
        for found in matches {
            self.append_units(&mut output, &input[previous..found.start], phase)?;
            if let Some(replacement) = &replacement_string {
                self.replacement_text(&mut output, replacement, input, &found, phase)?;
            } else {
                let count = found.captures.len().saturating_add(2);
                if count > MAX_ARRAY {
                    return Err(Fault::Fatal("JavaScript argument limit exhausted".into()));
                }
                self.budget.allocate_in(phase, count.saturating_mul(64))?;
                let mut callback_args = Vec::with_capacity(count);
                for capture in &found.captures {
                    callback_args.push(match capture {
                        Some((start, end)) => self.slice_value(input, *start, *end, phase)?,
                        None => Value::Undefined,
                    });
                }
                callback_args.push(Value::Number(found.start as f64));
                callback_args.push(self.slice_value(input, 0, input.len(), phase)?);
                let callback = self.copy(&replacement)?;
                let value = self.call(callback, Value::Undefined, callback_args, None, host)?;
                let units = self.units(value, host)?;
                self.append_units(&mut output, &units, phase)?;
            }
            previous = found.end;
        }
        self.append_units(&mut output, &input[previous..], phase)?;
        Ok(Value::String(output))
    }
    fn split_find(
        &mut self,
        regex: Option<&regexp::Regex>,
        plain: &[u16],
        input: &[u16],
        start: usize,
    ) -> Eval<Option<regexp::Match>> {
        if let Some(regex) = regex {
            return self.regexp_find(regex, input, start);
        }
        match self.plain_find(input, plain, start)? {
            Some((start, end)) => {
                self.budget.allocate(64)?;
                Ok(Some(regexp::Match {
                    start,
                    end,
                    captures: vec![Some((start, end))],
                }))
            }
            None => Ok(None),
        }
    }
    fn string_split(
        &mut self,
        input: &[u16],
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let separator = self.copy(args.first().unwrap_or(&Value::Undefined))?;
        let limit = self.copy(args.get(1).unwrap_or(&Value::Undefined))?;
        let limit = if matches!(limit, Value::Undefined) {
            u32::MAX
        } else {
            int32(self.number(limit, host)?) as u32
        };
        let regex = self.as_regexp(&separator).map(|(_, regex)| regex);
        let phase = if regex.is_some() {
            AllocationPhase::RegexResult
        } else {
            AllocationPhase::Runtime
        };
        let undefined = matches!(separator, Value::Undefined);
        // Separator coercion precedes the zero-limit return in ES5.1.
        let plain = if regex.is_none() {
            self.units(separator, host)?
        } else {
            Vec::new()
        };
        let mut values = PrepaidArray::growing(phase);
        if limit != 0 {
            if undefined {
                let value = self.slice_value(input, 0, input.len(), phase)?;
                values.push_owned(&mut self.budget, Some(value))?;
            } else if input.is_empty() {
                if self
                    .split_find(regex.as_deref(), &plain, input, 0)?
                    .is_none()
                {
                    let value = self.slice_value(input, 0, 0, phase)?;
                    values.push_owned(&mut self.budget, Some(value))?;
                }
            } else {
                let mut previous = 0;
                let mut start = 0;
                while start < input.len() && values.len() < limit as usize {
                    let Some(found) = self.split_find(regex.as_deref(), &plain, input, start)?
                    else {
                        break;
                    };
                    // Empty matches at the end cannot terminate a separator.
                    if found.start == input.len() {
                        break;
                    }
                    if found.end == previous {
                        start = found.start + 1;
                        continue;
                    }
                    let value = self.slice_value(input, previous, found.start, phase)?;
                    values.push_owned(&mut self.budget, Some(value))?;
                    previous = found.end;
                    if values.len() == limit as usize {
                        break;
                    }
                    for capture in found.captures.into_iter().skip(1) {
                        let value = match capture {
                            Some((start, end)) => self.slice_value(input, start, end, phase)?,
                            None => Value::Undefined,
                        };
                        values.push_owned(&mut self.budget, Some(value))?;
                        if values.len() == limit as usize {
                            break;
                        }
                    }
                    start = previous;
                }
                if values.len() < limit as usize {
                    let value = self.slice_value(input, previous, input.len(), phase)?;
                    values.push_owned(&mut self.budget, Some(value))?;
                }
            }
        }
        Ok(Value::Object(
            self.object(Some(self.array_prototype), Some(values))?,
        ))
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
        let needs_first = matches!(
            name,
            "String.match"
                | "String.search"
                | "String.charAt"
                | "String.charCodeAt"
                | "String.slice"
                | "String.substring"
                | "String.substr"
                | "String.indexOf"
                | "String.includes"
                | "String.startsWith"
                | "String.endsWith"
        );
        let first = if needs_first {
            self.copy(args.first().unwrap_or(&Value::Undefined))?
        } else {
            Value::Undefined
        };
        match name {
            "String.toString" | "String.valueOf" => Ok(Value::String(units)),
            "String.match" | "String.search" => self.string_regexp_match(name, &units, first, host),
            "String.replace" => self.string_replace(&units, args, host),
            "String.split" => self.string_split(&units, args, host),
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
                Ok(Value::String(units[start..end].to_vec()))
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
                // The receiver buffer was paid; each appended segment was
                // charged immediately before extending it above.
                Ok(Value::String(result))
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
                        Err(error) => {
                            self.budget.allocate(2)?;
                            result.push(error.unpaired_surrogate());
                        }
                    }
                }
                Ok(Value::String(result))
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
                // Separators and element payloads were prepaid before append.
                Ok(Value::String(result))
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
                let mut values = PrepaidArray::with_slots(
                    &mut self.budget,
                    end - start,
                    AllocationPhase::Runtime,
                )?;
                for index in start..end {
                    // own() still charges genuine payload reads; adopt each
                    // owned copy and preserve the existing hole behavior.
                    let value = self.own(id, &index.to_string())?;
                    values.push_owned(&mut self.budget, value)?;
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
        Fault::Member { context, .. } => context.format(),
    }
}
fn array_index(key: &str) -> Option<usize> {
    // Canonical uint32 keys without formatting/allocation during descriptor scans.
    if key.is_empty()
        || key.len() > 10
        || (key.len() > 1 && key.starts_with('0'))
        || !key.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let index = key.parse::<u32>().ok()?;
    (index != u32::MAX).then_some(index as usize)
}
fn strict_equal(left: &Value, right: &Value) -> bool {
    left == right
}
fn js_space(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}
fn primitive_number(value: &Value) -> Eval<f64> {
    if matches!(value, Value::Symbol(_)) {
        return Err(exception("TypeError: cannot convert Symbol to number"));
    }
    Ok(primitive_number_without_symbol(value))
}
fn primitive_number_without_symbol(value: &Value) -> f64 {
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sole_function_parameter_fragment_moves_pointer_capacity_and_units() {
        for units in [
            vec![],
            vec![b'a' as u16, b',' as u16, b'b' as u16],
            vec![0xd800],
        ] {
            let mut runtime = Runtime::new();
            let mut fragment = Vec::with_capacity(16);
            fragment.extend(&units);
            let pointer = fragment.as_ptr();
            let capacity = fragment.capacity();
            let before = runtime.allocation_report();
            let parameters = runtime.function_parameter_source(vec![fragment]).unwrap();
            assert_eq!(parameters.as_ptr(), pointer);
            assert_eq!(parameters.capacity(), capacity);
            assert_eq!(parameters, units);
            assert_eq!(runtime.allocation_report(), before);
            if units == [0xd800] {
                // Ownership transfer must not sanitize invalid source. The
                // existing UTF-8 boundary still rejects this unchanged unit.
                assert!(matches!(
                    runtime.utf8_source(&parameters),
                    Err(Fault::Throw(_))
                ));
                assert_eq!(
                    runtime.allocation_report().phases.source,
                    before.phases.source
                );
            }
            assert!(runtime.allocation_report().is_valid());
        }
    }

    #[test]
    fn zero_and_multiple_function_parameter_fragments_keep_join_charges() {
        let mut runtime = Runtime::new();
        let before = runtime.allocation_report();
        assert!(
            runtime
                .function_parameter_source(vec![])
                .unwrap()
                .is_empty()
        );
        assert_eq!(runtime.allocation_report(), before);
        let first = vec![b'a' as u16];
        let second = vec![b'b' as u16];
        let first_pointer = first.as_ptr();
        let second_pointer = second.as_ptr();
        let parameters = runtime
            .function_parameter_source(vec![first, second])
            .unwrap();
        assert_eq!(parameters, vec![b'a' as u16, b',' as u16, b'b' as u16]);
        assert_ne!(parameters.as_ptr(), first_pointer);
        assert_ne!(parameters.as_ptr(), second_pointer);
        assert_eq!(
            runtime.allocation_report().phases.source - before.phases.source,
            6
        );
        let before = runtime.allocation_report();
        assert_eq!(
            runtime
                .function_parameter_source(vec![vec![], vec![], vec![]])
                .unwrap(),
            vec![b',' as u16, b',' as u16]
        );
        assert_eq!(
            runtime.allocation_report().phases.source - before.phases.source,
            4
        );
        assert_eq!(
            runtime.allocation_report().phases.runtime,
            before.phases.runtime
        );
        assert!(runtime.allocation_report().is_valid());
    }

    #[test]
    fn function_parameter_join_and_utf8_preflight_preserve_first_failure() {
        let mut runtime = Runtime::new();
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - 5)
            .unwrap();
        let before = runtime.allocation_report();
        assert!(
            runtime
                .function_parameter_source(vec![vec![b'a' as u16], vec![b'b' as u16]])
                .is_err()
        );
        let rejected = runtime.allocation_report();
        assert_eq!(rejected.accepted_bytes, before.accepted_bytes);
        assert_eq!(rejected.first_rejected.unwrap().requested_bytes, 6);
        assert_eq!(
            rejected.first_rejected.unwrap().phase,
            AllocationPhase::Source
        );
        for parts in [vec![], vec![vec![]], vec![vec![], vec![]]] {
            assert!(runtime.function_parameter_source(parts).is_err());
            assert_eq!(runtime.allocation_report(), rejected);
        }
        assert!(rejected.is_valid());

        let mut runtime = Runtime::new();
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - 2)
            .unwrap();
        let before = runtime.allocation_report();
        let parameters = runtime
            .function_parameter_source(vec![vec![b'a' as u16; 3]])
            .unwrap();
        assert_eq!(runtime.allocation_report(), before);
        assert!(matches!(
            runtime.utf8_source(&parameters),
            Err(Fault::Fatal(_))
        ));
        let report = runtime.allocation_report();
        assert_eq!(report.accepted_bytes, before.accepted_bytes);
        assert_eq!(report.first_rejected.unwrap().requested_bytes, 3);
        assert_eq!(
            report.first_rejected.unwrap().phase,
            AllocationPhase::Source
        );
        assert!(report.is_valid());
    }

    #[test]
    fn formal_binding_copies_payload_once_and_preserves_local_metadata() {
        let mut runtime = Runtime::new();
        let environment = runtime.environment(0, true).unwrap();
        let input = Value::String(vec![0xd800, b'a' as u16, 0xdc00]);
        let Value::String(original) = &input else {
            unreachable!()
        };
        let before = runtime.allocation_report();
        runtime
            .bind_parameter_copy(environment, "param", &input)
            .unwrap();
        let after = runtime.allocation_report();
        assert_eq!(after.phases.runtime - before.phases.runtime, 6 + 128 + 5);
        let binding = &runtime.environments[environment].bindings[0];
        let Value::String(copy) = &binding.value else {
            panic!("parameter string missing");
        };
        assert_eq!(copy, original);
        assert_ne!(copy.as_ptr(), original.as_ptr());
        assert!(!binding.deletable);
        // The low-level move keeps its buffer; metadata applies only to new
        // names, and replacing a duplicate preserves the prior attributes.
        runtime.environments[environment].bindings[0].deletable = true;
        let before = runtime.allocation_report();
        runtime
            .bind_parameter_copy(environment, "param", &input)
            .unwrap();
        let bindings = &runtime.environments[environment].bindings;
        assert_eq!(bindings.len(), 1);
        assert!(bindings[0].deletable);
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            6
        );
        let value = runtime.copy(&input).unwrap();
        let Value::String(units) = &value else {
            unreachable!()
        };
        let pointer = units.as_ptr();
        let before = runtime.allocation_report();
        runtime
            .store_local_binding(environment, "param", value)
            .unwrap();
        let Value::String(stored) = &runtime.environments[environment].bindings[0].value else {
            unreachable!()
        };
        assert_eq!(stored.as_ptr(), pointer);
        assert_eq!(runtime.allocation_report(), before);
        assert!(before.is_valid());
    }

    #[test]
    fn formal_binding_rejections_preflight_copy_then_metadata_without_insertion() {
        for remaining in [5, 6 + 128 + 5 - 1] {
            let mut runtime = Runtime::new();
            let environment = runtime.environment(0, true).unwrap();
            let input = Value::String(vec![0xd800, b'a' as u16, 0xdc00]);
            runtime
                .budget
                .allocate(MAX_HEAP - runtime.budget.allocated - remaining)
                .unwrap();
            let before = runtime.allocation_report();
            assert!(
                runtime
                    .bind_parameter_copy(environment, "param", &input)
                    .is_err()
            );
            assert!(runtime.environments[environment].bindings.is_empty());
            let after = runtime.allocation_report();
            let rejected = after.first_rejected.unwrap();
            assert_eq!(rejected.phase, AllocationPhase::Runtime);
            if remaining == 5 {
                // Budget::copy rejects before clone; no accepted payload cost.
                assert_eq!(after.accepted_bytes, before.accepted_bytes);
                assert_eq!(rejected.requested_bytes, 6);
            } else {
                // The real copy was paid, but binding metadata did not fit;
                // no partial name/value entry may be installed.
                assert_eq!(after.accepted_bytes, before.accepted_bytes + 6);
                assert_eq!(rejected.requested_bytes, 128 + 5);
            }
            assert_eq!(input, Value::String(vec![0xd800, b'a' as u16, 0xdc00]));
            assert!(
                runtime
                    .bind_parameter_copy(environment, "later", &Value::Undefined)
                    .is_err()
            );
            assert_eq!(runtime.allocation_report(), after);
            assert!(after.is_valid());
        }
    }

    #[test]
    fn formal_binding_storage_rejects_nonlocal_use_and_does_not_exempt_define() {
        let mut runtime = Runtime::new();
        let input = Value::text("owned");
        for environment in [0, runtime.environments.len()] {
            let before = runtime.allocation_report();
            assert!(
                runtime
                    .bind_parameter_copy(environment, "invalid", &input)
                    .is_err()
            );
            assert!(
                runtime
                    .store_local_binding(environment, "invalid", Value::Undefined)
                    .is_err()
            );
            assert_eq!(runtime.allocation_report(), before);
            assert_eq!(runtime.get_global("invalid"), Value::Undefined);
        }
        let environment = runtime.environment(0, true).unwrap();
        let before = runtime.allocation_report();
        runtime.define(environment, "ordinary", input).unwrap();
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            10 + 128 + 8
        );
        let before = runtime.allocation_report();
        runtime
            .define(environment, "ordinary", Value::text("copy"))
            .unwrap();
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            8
        );
        assert!(runtime.allocation_report().is_valid());
    }

    #[test]
    fn prepaid_array_adoption_moves_slots_and_payloads_with_exclusive_charges() {
        let mut runtime = Runtime::new();
        let payload = runtime.text("owned payload").unwrap();
        let Value::String(units) = &payload else {
            unreachable!()
        };
        let payload_pointer = units.as_ptr();
        let before = runtime.allocation_report();
        let mut builder =
            PrepaidArray::with_slots(&mut runtime.budget, 3, AllocationPhase::RegexResult).unwrap();
        let slots_pointer = builder.values.as_ptr();
        builder
            .push_owned(&mut runtime.budget, Some(payload))
            .unwrap();
        builder.push_owned(&mut runtime.budget, None).unwrap();
        builder
            .push_owned(&mut runtime.budget, Some(Value::Undefined))
            .unwrap();
        let id = runtime
            .object(Some(runtime.array_prototype), Some(builder))
            .unwrap();
        let slots = runtime.objects[id].array.as_ref().unwrap();
        assert_eq!(slots.as_ptr(), slots_pointer);
        let Some(Value::String(units)) = &slots[0] else {
            panic!("owned string missing");
        };
        assert_eq!(units.as_ptr(), payload_pointer);
        assert!(slots[1].is_none());
        assert_eq!(slots[2], Some(Value::Undefined));
        let after = runtime.allocation_report();
        assert_eq!(
            after.phases.regex_result - before.phases.regex_result,
            3 * 64
        );
        assert_eq!(after.phases.runtime - before.phases.runtime, 128);
        assert_eq!(after.accepted_bytes - before.accepted_bytes, 3 * 64 + 128);
        assert!(after.is_valid());
    }

    #[test]
    fn prepaid_array_geometric_growth_retains_unused_credits_and_caps_work() {
        for (length, paid_slots, grows) in [(3, 4, 3), (MAX_ARRAY, MAX_ARRAY, 15)] {
            let mut runtime = Runtime::new();
            let before = runtime.allocation_report();
            let mut builder = PrepaidArray::growing(AllocationPhase::RegexResult);
            let mut growths = 0;
            for _ in 0..length {
                let previous = builder.paid_slots;
                builder.push_owned(&mut runtime.budget, None).unwrap();
                growths += usize::from(builder.paid_slots != previous);
                assert!(builder.len() <= builder.paid_slots);
                assert!(builder.paid_slots <= MAX_ARRAY);
                assert!(builder.values.capacity() >= builder.paid_slots);
            }
            assert_eq!(growths, grows);
            assert_eq!(builder.paid_slots, paid_slots);
            if length == MAX_ARRAY {
                let full = runtime.allocation_report();
                assert!(builder.push_owned(&mut runtime.budget, None).is_err());
                assert_eq!(builder.len(), MAX_ARRAY);
                assert_eq!(runtime.allocation_report(), full);
            }
            let id = runtime.object(None, Some(builder)).unwrap();
            assert_eq!(runtime.objects[id].array.as_ref().unwrap().len(), length);
            let after = runtime.allocation_report();
            // The three-element result keeps its unused fourth credit. The
            // 10k result grows only fifteen times, never on every append.
            assert_eq!(
                after.phases.regex_result - before.phases.regex_result,
                (paid_slots * 64) as u64
            );
            assert_eq!(after.phases.runtime - before.phases.runtime, 128);
            assert!(after.is_valid());
        }
        let mut runtime = Runtime::new();
        let mut builder = PrepaidArray::growing(AllocationPhase::RegexResult);
        for _ in 0..2 {
            builder.push_owned(&mut runtime.budget, None).unwrap();
        }
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - 127)
            .unwrap();
        let capacity = builder.values.capacity();
        let before = runtime.allocation_report();
        assert!(builder.push_owned(&mut runtime.budget, None).is_err());
        assert_eq!(builder.len(), 2);
        assert_eq!(builder.paid_slots, 2);
        assert_eq!(builder.values.capacity(), capacity);
        let after = runtime.allocation_report();
        assert_eq!(after.accepted_bytes, before.accepted_bytes);
        assert_eq!(after.first_rejected.unwrap().requested_bytes, 128);
        assert_eq!(
            after.first_rejected.unwrap().phase,
            AllocationPhase::RegexResult
        );
        assert!(after.is_valid());
    }

    #[test]
    fn prepaid_array_rejection_precedes_storage_growth_and_keeps_first_failure() {
        let mut runtime = Runtime::new();
        let before = runtime.allocation_report();
        assert!(matches!(
            PrepaidArray::with_slots(&mut runtime.budget, MAX_ARRAY + 1, AllocationPhase::Runtime),
            Err(Fault::Fatal(_))
        ));
        assert_eq!(runtime.allocation_report(), before);
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - 63)
            .unwrap();
        let before = runtime.allocation_report();
        let objects_before = runtime.objects.len();
        let mut builder = PrepaidArray::growing(AllocationPhase::RegexResult);
        assert!(matches!(
            builder.push_owned(&mut runtime.budget, Some(Value::Undefined)),
            Err(Fault::Fatal(_))
        ));
        assert_eq!(builder.values.len(), 0);
        assert_eq!(builder.values.capacity(), 0);
        assert_eq!(builder.paid_slots, 0);
        let rejected = runtime.allocation_report();
        assert_eq!(rejected.accepted_bytes, before.accepted_bytes);
        assert_eq!(rejected.phases, before.phases);
        assert_eq!(
            rejected.first_rejected.unwrap().phase,
            AllocationPhase::RegexResult
        );
        assert_eq!(rejected.first_rejected.unwrap().requested_bytes, 64);
        assert!(runtime.object(None, Some(builder)).is_err());
        assert_eq!(runtime.objects.len(), objects_before);
        assert_eq!(runtime.allocation_report(), rejected);
        assert!(rejected.is_valid());
    }

    #[test]
    fn prepaid_array_admission_requires_full_builder_and_metadata_budget() {
        let mut runtime = Runtime::new();
        for count in [0, 1] {
            let mut builder =
                PrepaidArray::with_slots(&mut runtime.budget, count, AllocationPhase::Runtime)
                    .unwrap();
            for _ in 0..count {
                builder.push_owned(&mut runtime.budget, None).unwrap();
            }
            let before = runtime.allocation_report();
            assert!(builder.push_owned(&mut runtime.budget, None).is_err());
            assert_eq!(builder.len(), count);
            assert_eq!(builder.paid_slots, count);
            assert_eq!(runtime.allocation_report(), before);
            runtime.object(None, Some(builder)).unwrap();
        }
        let builder =
            PrepaidArray::with_slots(&mut runtime.budget, 1, AllocationPhase::Runtime).unwrap();
        let before = runtime.allocation_report();
        let objects_before = runtime.objects.len();
        assert!(runtime.object(None, Some(builder)).is_err());
        assert_eq!(runtime.objects.len(), objects_before);
        assert_eq!(runtime.allocation_report(), before);
        let mut builder =
            PrepaidArray::with_slots(&mut runtime.budget, 1, AllocationPhase::Runtime).unwrap();
        builder.push_owned(&mut runtime.budget, None).unwrap();
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - 127)
            .unwrap();
        assert!(runtime.object(None, Some(builder)).is_err());
        assert_eq!(runtime.objects.len(), objects_before);
        let report = runtime.allocation_report();
        assert_eq!(report.first_rejected.unwrap().requested_bytes, 128);
        assert_eq!(
            report.first_rejected.unwrap().phase,
            AllocationPhase::Runtime
        );
        assert!(report.is_valid());
    }

    #[test]
    fn arguments_snapshot_moves_original_payload_after_independent_parameter_copy() {
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        let callee = runtime
            .execute(
                "function capture(value){return arguments;}capture;",
                &mut host,
            )
            .unwrap();
        let input = vec![0xd800, b'a' as u16, 0xdc00];
        let pointer = input.as_ptr();
        let Value::Object(snapshot) = runtime
            .invoke(
                callee,
                Value::Undefined,
                vec![Value::String(input)],
                &mut host,
            )
            .unwrap()
        else {
            panic!("arguments snapshot missing");
        };
        let Some(Value::String(snapshot_value)) =
            &runtime.objects[snapshot].array.as_ref().unwrap()[0]
        else {
            panic!("snapshot payload missing");
        };
        assert_eq!(snapshot_value.as_ptr(), pointer);
        let parameter = runtime
            .environments
            .last()
            .unwrap()
            .bindings
            .iter()
            .find(|binding| binding.name == "value")
            .unwrap();
        let Value::String(parameter_value) = &parameter.value else {
            panic!("parameter payload missing");
        };
        assert_ne!(parameter_value.as_ptr(), pointer);
        assert_eq!(parameter_value, snapshot_value);
        assert!(runtime.allocation_report().is_valid());
    }

    #[test]
    fn function_instances_share_code_slices_not_closure_state() {
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        runtime
            .execute(
                "function make(x){return function inner(y){return x+y;};}var a=make(4);var b=make(9);",
                &mut host,
            )
            .unwrap();
        let Value::Function(a) = runtime.get_global("a") else {
            panic!("expected first closure");
        };
        let Value::Function(b) = runtime.get_global("b") else {
            panic!("expected second closure");
        };
        assert_ne!(a, b);
        let first = &runtime.functions[a];
        let second = &runtime.functions[b];
        assert!(!Rc::ptr_eq(first.ordinary_code(), second.ordinary_code()));
        assert!(Rc::ptr_eq(
            &first.ordinary_code().body,
            &second.ordinary_code().body
        ));
        assert!(Rc::ptr_eq(
            &first.ordinary_code().params,
            &second.ordinary_code().params
        ));
        assert_ne!(first.environment, second.environment);
        assert_ne!(first.properties, second.properties);
        let before = runtime.allocation_report();
        assert_eq!(
            runtime
                .execute(
                    "a(1)===5 && b(1)===10 && a.prototype!==b.prototype && typeof inner==='undefined';",
                    &mut host,
                )
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            runtime.allocation_report().phases.function_code,
            before.phases.function_code
        );
        let make = runtime.get_global("make");
        runtime
            .invoke(make, Value::Undefined, vec![Value::Number(12.0)], &mut host)
            .unwrap();
        assert_eq!(
            runtime.allocation_report().phases.function_code - before.phases.function_code,
            128 + "inner".len() as u64
        );
        assert!(runtime.allocation_report().is_valid());
    }

    #[test]
    fn prepaid_case_output_charges_unpaired_surrogates_before_append() {
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        let before = runtime.allocation_report();
        let input = vec![0xd800, b'A' as u16, 0xdc00];
        let name = "String.toLowerCase";
        assert_eq!(
            runtime
                .invoke(
                    Value::Native(name.into()),
                    Value::String(input),
                    vec![],
                    &mut host,
                )
                .unwrap(),
            Value::String(vec![0xd800, b'a' as u16, 0xdc00])
        );
        // Host ingress pays six input bytes; all three output units each pay
        // two bytes, including the two unpaired units. No final move recharge.
        let after = runtime.allocation_report();
        assert_eq!(
            after.phases.runtime - before.phases.runtime,
            name.len() as u64 + 6 + 6
        );
        assert!(after.is_valid());
    }

    #[test]
    fn ignored_native_arguments_do_not_copy_and_coercions_remain_ordered() {
        let run = |argument| {
            let mut runtime = Runtime::new();
            let before = runtime.allocation_report();
            assert_eq!(
                runtime
                    .invoke(
                        Value::Native("String.valueOf".into()),
                        Value::text("receiver"),
                        vec![argument],
                        &mut TestHost::default(),
                    )
                    .unwrap(),
                Value::text("receiver")
            );
            runtime.allocation_report().phases.runtime - before.phases.runtime
        };
        assert_eq!(
            run(Value::String(vec![97; 1024])) - run(Value::Undefined),
            2048
        );
        yes(
            "var order='';var receiver={toString:function(){order+='r';return 'A';}};var first={toString:function(){order+='a';return 'B';}};var second={toString:function(){order+='b';return 'C';}};String.prototype.concat.call(receiver,first,second)==='ABC' && order==='rab';",
        );
    }

    #[test]
    fn allocation_phase_counters_and_first_rejection_are_exact() {
        let mut runtime = Runtime::new();
        let initial = runtime.allocation_report();
        assert!(initial.is_valid());
        assert_eq!(initial.accepted_bytes, initial.phases.bootstrap);
        assert!(initial.first_rejected.is_none());
        for phase in [
            AllocationPhase::Source,
            AllocationPhase::Ast,
            AllocationPhase::FunctionCode,
            AllocationPhase::Runtime,
            AllocationPhase::RegexCompile,
            AllocationPhase::RegexResult,
        ] {
            let before = runtime.allocation_report();
            runtime.budget.allocate_in(phase, 37).unwrap();
            let after = runtime.allocation_report();
            let mut expected = before;
            expected.accepted_bytes += 37;
            *expected.phases.counter(phase) += 37;
            assert_eq!(after, expected);
            assert!(after.is_valid());
        }
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated)
            .unwrap();
        let accepted = runtime.allocation_report();
        assert_eq!(accepted.accepted_bytes, accepted.limit_bytes);
        let error = fault_text(
            runtime
                .budget
                .allocate_in(AllocationPhase::RegexResult, 1)
                .unwrap_err(),
        );
        let rejected = runtime.allocation_report();
        assert!(rejected.is_valid());
        assert_eq!(rejected.phases, accepted.phases);
        assert_eq!(rejected.accepted_bytes, accepted.accepted_bytes);
        assert_eq!(
            rejected.first_rejected,
            Some(RejectedAllocation {
                phase: AllocationPhase::RegexResult,
                accepted_bytes: MAX_HEAP as u64,
                requested_bytes: 1,
                limit_bytes: MAX_HEAP as u64,
            })
        );
        assert_eq!(
            fault_text(
                runtime
                    .budget
                    .allocate_in(AllocationPhase::Source, usize::MAX)
                    .unwrap_err()
            ),
            error
        );
        assert_eq!(runtime.allocation_report(), rejected);
    }

    #[test]
    fn allocation_report_validation_checks_caps_overflow_and_rejection() {
        let report = Runtime::new().allocation_report();
        assert!(report.is_valid());
        let mut invalid = report;
        invalid.limit_bytes += 1;
        assert!(!invalid.is_valid());
        invalid = report;
        invalid.phases.source = u64::MAX;
        assert!(!invalid.is_valid());
        invalid = report;
        invalid.accepted_bytes += 1;
        assert!(!invalid.is_valid());
        invalid = report;
        invalid.first_rejected = Some(RejectedAllocation {
            phase: AllocationPhase::Runtime,
            accepted_bytes: report.accepted_bytes,
            requested_bytes: report.limit_bytes - report.accepted_bytes,
            limit_bytes: report.limit_bytes,
        });
        assert!(!invalid.is_valid());
        invalid.first_rejected.as_mut().unwrap().requested_bytes += 1;
        assert!(invalid.is_valid());
        invalid.first_rejected.as_mut().unwrap().accepted_bytes += 1;
        assert!(!invalid.is_valid());
    }

    #[test]
    fn allocation_output_phases_do_not_leak_into_callbacks_or_plain_strings() {
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        runtime
            .execute("'a-b'.replace('a','x');'a-b'.split('-');", &mut host)
            .unwrap();
        let plain = runtime.allocation_report();
        assert!(plain.is_valid());
        assert_eq!(plain.phases.regex_compile, 0);
        assert_eq!(plain.phases.regex_result, 0);
        runtime
            .execute(
                "'aa'.replace(/a/g,function(){return Function('return 7;')();});",
                &mut host,
            )
            .unwrap();
        let regex = runtime.allocation_report();
        assert!(regex.is_valid());
        assert!(regex.phases.regex_compile > 0 && regex.phases.regex_result > 0);
        assert!(regex.phases.source > plain.phases.source);
        assert!(regex.phases.function_code > plain.phases.function_code);
        assert!(regex.phases.runtime > plain.phases.runtime);
    }

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
    fn enumeration_and_boxed_string_descriptors_agree() {
        yes(
            "var s=Object('ab');s[0]='wrong';s.length=8;!(delete s[0]) && !(delete s.length) && s[0]==='a' && s.length===2 && s.hasOwnProperty('0') && Object.keys(s).join(',')==='0,1' && Object.getOwnPropertyNames(s).join(',')==='0,1,length';",
        );
        yes(
            "var s=Object.create(Object('ab'));s[0]='wrong';s.length=8;var keys='';for(var k in s){keys+=k;}s[0]==='a' && s.length===2 && !s.hasOwnProperty('0') && !s.hasOwnProperty('length') && keys==='01';",
        );
        yes(
            "var s=Object.create({length:3,0:'old'});s.length=4;s[0]='new';s.length===4 && s[0]==='new' && s.hasOwnProperty('length');",
        );
        yes("var caught=false;try{Object.keys('ab');}catch(e){caught=true;}caught;");
    }

    #[test]
    fn enumeration_builtin_metadata_preserves_overwrite_delete_and_readd_attributes() {
        yes(
            "Function.prototype.prototype=7;var seen='';for(var k in parseInt){seen+=k;}seen==='prototype' && parseInt.prototype===7 && !parseInt.hasOwnProperty('prototype');",
        );
        yes(
            "Function.prototype.isArray=1;Array.isArray=function(){return 2;};var seen='';for(var k in Array){seen+=k;}seen==='' && Object.keys(Array).length===0 && Array.isArray()===2;",
        );
        yes(
            "Function.prototype.isArray=1;delete Array.isArray;var seen='';for(var k in Array){seen+=k;}seen==='isArray' && Array.isArray===1 && Object.keys(Array).length===0;",
        );
        yes(
            "delete Array.isArray;Array.isArray=3;Object.keys(Array).join(',')==='isArray' && Array.isArray===3;",
        );
        yes(
            "Array.extra=1;Array.later=2;var seen='';for(var k in Array){seen+=k;delete Array.later;}seen==='extra' && Array.later===undefined;",
        );
        yes(
            "function f(){}f.length=7;f.name='wrong';!(delete f.length) && !(delete f.name) && !(delete f.prototype) && f.length===0 && f.name==='f' && f.hasOwnProperty('length') && Object.keys(f).length===0;",
        );
        yes(
            "Array.length=99;Array.name='wrong';!(delete Array.length) && !(delete Array.name) && Array.length===1 && Array.name==='Array' && Array.hasOwnProperty('length');",
        );
    }

    #[test]
    fn enumeration_snapshot_rechecks_original_owner_not_newly_revealed_or_shadowing_owner() {
        yes(
            "var o=Object.create({b:2});o.a=1;o.b=3;var seen='';for(var k in o){seen+=k;if(k==='a'){delete o.b;}}seen==='a';",
        );
        yes(
            "var o=Object.create({b:2});o.a=1;var seen='';for(var k in o){seen+=k;if(k==='a'){o.b=3;}}seen==='a';",
        );
        yes(
            "var o={a:1,b:2};var seen='';for(var k in o){seen+=k;if(k==='a'){delete o.b;o.b=3;}}seen==='ab';",
        );
    }

    #[test]
    fn enumeration_limits_are_fatal_before_partial_completion_and_remain_latched() {
        for (setup, operation, expected) in [
            (
                "var o={};for(var i=0;i<64;i++){o=Object.create(o);}",
                "for(var k in o){visited=true;}",
                "prototype depth limit",
            ),
            (
                "var o=Object.create(null);o[String.fromCharCode(65,66,67)]=1;",
                "while(true){for(var k in o){visited=true;}}",
                "exhausted",
            ),
            (
                "var o=Object('a');for(var i=0;i<64;i++){o=Object.create(o);}",
                "o[0]='wrong';",
                "prototype depth limit",
            ),
        ] {
            let mut runtime = Runtime::new();
            let mut host = TestHost::default();
            runtime.execute(setup, &mut host).unwrap();
            let source = format!(
                "var visited=false,caught=false,finalized=false;try{{{operation}}}catch(e){{caught=true;}}finally{{finalized=true;}}"
            );
            let error = runtime.execute(&source, &mut host).unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert_eq!(runtime.get_global("caught"), Value::Bool(false));
            assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
            if expected.contains("prototype") {
                assert_eq!(runtime.get_global("visited"), Value::Bool(false));
            }
            assert_eq!(runtime.execute("1", &mut host).unwrap_err(), error);
        }
    }

    #[test]
    fn recursive_iteration_and_switch_reach_configured_call_limit_on_default_stack() {
        for source in [
            "function f(){for(var k in {a:1}){return f();}}f();",
            "function f(){switch(1){case 1:return f();}}f();",
        ] {
            let mut runtime = Runtime::new();
            let error = runtime
                .execute(source, &mut TestHost::default())
                .unwrap_err();
            assert!(error.contains("call depth exhausted"), "{error}");
        }
    }

    #[test]
    fn regexp_string_operations_use_es5_intrinsics_and_preserve_callback_effects() {
        yes(
            "var r=/a/g;var s='aa'.replace(r,{toString:function(){r.lastIndex=17;return 'x';}});s==='xx' && r.lastIndex===17;",
        );
        // ES5.1 15.5.4.10 step 6 uses the standard builtin, not rx.exec.
        yes(
            "var r=/a/g;r.exec=function(){throw 'overridden';};'aba'.match(r).length===2 && 'aba'.replace(r,'x')==='xbx' && 'a'.search(r)===0 && 'aba'.split(r).length===3;",
        );
        yes(
            "var r=/a/g;var n=0;var s='aa'.replace(r,function(){n++;r.lastIndex=19;return n;});s==='12' && n===2 && r.lastIndex===19;",
        );
        yes(
            "var r=/a/g;r.lastIndex={valueOf:function(){throw 'must not coerce';}};'ba'.search(r)===1 && 'a,b'.split(r).length===2;",
        );
        yes("var r=/(?=b)/g;'ab'.match(r).length===1 && 'ab'.replace(r,'-')==='a-b';");
    }

    #[test]
    fn regexp_source_and_property_guards_are_observable() {
        let error = Runtime::new().execute("var child=/a/;for(var i=0;i<64;i++){child=Object.create(child);}child.source='wrong';", &mut TestHost::default()).unwrap_err();
        assert!(error.contains("prototype depth limit"), "{error}");
        yes(
            r"RegExp('/').source==='\\/' && RegExp('\n').source==='\\n' && RegExp('\u2028').source==='\\u2028';",
        );
        yes(r"/\//.source==='\\/' && RegExp(RegExp('/').source).test('/');");
        for line in [10, 13, 0x2028, 0x2029] {
            yes(&format!(
                "var input=String.fromCharCode({line});var r=RegExp(String.fromCharCode(92,{line}));r.test(input) && RegExp(r.source).test(input);"
            ));
            yes(&format!(
                "var input=String.fromCharCode(92,{line});var r=RegExp(String.fromCharCode(92,92,{line}));r.test(input) && RegExp(r.source).test(input);"
            ));
        }
        yes(
            "var r=/a/g;var child=Object.create(r);child.source='b';child.global=false;child.source==='a' && child.global && !child.hasOwnProperty('source');",
        );
        yes(
            "var p=RegExp.prototype;RegExp.prototype={};!(delete RegExp.prototype) && RegExp.prototype===p && p.source==='(?:)' && p.test('');",
        );
        yes(
            "var caught=false;try{RegExp.prototype.exec.call(Object.create(/a/),'a');}catch(e){caught=e.name==='TypeError';}caught;",
        );
    }

    #[test]
    fn regexp_replacement_and_split_preserve_code_units_and_coercion_order() {
        yes("'a'.replace(/(a)/,'$01-$10-$0-$99')==='a-a0-$0-$99';");
        yes("var a='b'.split(/(a)?b/);a.length===3 && a[0]==='' && a[1]===undefined && a[2]==='';");
        yes(
            "var a='😀'.split('');a.length===2 && a[0].charCodeAt(0)===55357 && a[1].charCodeAt(0)===56832;",
        );
        yes(
            "var order='';var a='abc'.split({toString:function(){order+='s';return ',';}},{valueOf:function(){order+='l';return 0;}});order==='ls' && a.length===0;",
        );
        yes("'abc'.split('',4294967297).length===1 && 'abc'.split('',-1).length===3;");
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
        yes("var mapped=[].map(function(){});Array.isArray(mapped)&&mapped.length===0;");
        let error = Runtime::new()
            .execute("[].unshift(1)", &mut TestHost::default())
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
        runtime
            .budget
            .allocate(MAX_HEAP - 1 - runtime.budget.allocated)
            .unwrap();
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

    #[test]
    fn evaluation_depth_guards_cover_pending_statements_and_expressions() {
        const CHILD: &str = "MGBROWSER_RUNTIME_DEPTH_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let cases = [
                ("unary", format!("return {}recurse();", "+ ".repeat(96))),
                ("if", format!("{}return recurse();", "if(true)".repeat(96))),
                (
                    "while",
                    format!("{}return recurse();", "while(true)".repeat(96)),
                ),
                ("for", format!("{}return recurse();", "for(;;)".repeat(48))),
                (
                    "for-in",
                    format!("{}return recurse();", "for(var k in {a:1})".repeat(48)),
                ),
                (
                    "blocks",
                    format!("{}return recurse();{}", "{".repeat(60), "}".repeat(60)),
                ),
                (
                    "try",
                    format!(
                        "{}return recurse();{}",
                        "try{".repeat(48),
                        "}finally{}".repeat(48)
                    ),
                ),
                (
                    "switch",
                    format!(
                        "{}return recurse();{}",
                        "switch(1){case 1:".repeat(48),
                        "}".repeat(48)
                    ),
                ),
                (
                    "mixed",
                    format!(
                        "{}return {}recurse();{}",
                        "if(true){".repeat(24),
                        "+ ".repeat(32),
                        "}".repeat(24)
                    ),
                ),
                (
                    "mixed-switch",
                    format!(
                        "{}return {}recurse();{}",
                        "switch(1){case 1:".repeat(48),
                        "+ ".repeat(24),
                        "}".repeat(48)
                    ),
                ),
                (
                    "mixed-if",
                    format!(
                        "{}return {}recurse();",
                        "if(true)".repeat(96),
                        "+ ".repeat(24)
                    ),
                ),
            ];
            for (name, body) in cases {
                eprintln!("Local retained-evaluation probe: {name}");
                let source = format!(
                    "var caught=false,finalized=false;function recurse(){{{body}}}try{{recurse();}}catch(e){{caught=true;}}finally{{finalized=true;}}"
                );
                let mut runtime = Runtime::new();
                let mut host = TestHost::default();
                let error = runtime.execute(&source, &mut host).unwrap_err();
                assert!(
                    error.contains("evaluation depth limit exhausted"),
                    "{name}: {error}"
                );
                assert_eq!(runtime.get_global("caught"), Value::Bool(false));
                assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
                assert_eq!(runtime.budget.active_expressions, 0);
                assert_eq!(runtime.budget.evaluation_entries, 0);
                assert_eq!(runtime.budget.calls, 0);
                assert_eq!(runtime.execute("42", &mut host).unwrap_err(), error);
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
            return;
        }
        // A Rust stack overflow aborts, not unwinds. Isolate the default-stack
        // regression so a future failure cannot abort unrelated test groups.
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "js::runtime::tests::evaluation_depth_guards_cover_pending_statements_and_expressions", "--nocapture"])
            .env(CHILD, "1")
            .env_remove("RUST_MIN_STACK")
            .stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "Owned evaluation-depth child exceeded deadline: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "Owned default-stack evaluation test failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn evaluation_entry_counters_unwind_after_catchable_errors_and_success() {
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        for source in [
            "function f(){throw 7;}try{f();}catch(e){e;}",
            "function f(){return missing;}try{f();}catch(e){1;}",
            "function f(){return 42;}f();",
        ] {
            runtime.execute(source, &mut host).unwrap();
            assert_eq!(runtime.budget.active_expressions, 0);
            assert_eq!(runtime.budget.evaluation_entries, 0);
            assert_eq!(runtime.budget.calls, 0);
        }
    }

    #[test]
    fn retained_host_invocation_preserves_normal_copy_costs_and_preflights_failures() {
        let mut normal = Runtime::new();
        let mut retained = Runtime::new();
        let mut first = TestHost::default();
        let mut second = TestHost::default();
        let callback = Value::Native("Math.abs".into());
        let a = normal
            .invoke(
                callback.clone(),
                Value::Undefined,
                vec![Value::Number(-42.)],
                &mut first,
            )
            .unwrap();
        let b = retained
            .invoke_retained(
                &callback,
                Value::Undefined,
                vec![Value::Number(-42.)],
                &mut second,
            )
            .unwrap();
        assert!(matches!((a, b), (Value::Number(42.), Value::Number(42.))));
        assert_eq!(normal.allocation_report(), retained.allocation_report());
        assert_eq!(normal.budget.fuel, retained.budget.fuel);
        let remaining = MAX_HEAP - retained.budget.allocated - 3;
        retained.budget.allocate(remaining).unwrap();
        let before_fuel = retained.budget.fuel;
        assert!(
            retained
                .invoke_retained(&callback, Value::Undefined, vec![], &mut second)
                .is_err()
        );
        assert!(retained.is_fatal());
        assert_eq!(
            retained.budget.fuel, before_fuel,
            "callback copy must reject before entering it"
        );
        assert_eq!(
            retained
                .allocation_report()
                .first_rejected
                .unwrap()
                .requested_bytes,
            8
        );
        let report = retained.allocation_report();
        assert!(retained.execute("throw 'later'", &mut second).is_err());
        assert_eq!(retained.allocation_report(), report);
        assert_eq!(retained.budget.calls, 0);
        assert_eq!(retained.budget.evaluation_entries, 0);
    }

    #[test]
    fn host_accessor_registration_keeps_default_runtime_bootstrap_unchanged() {
        let mut runtime = Runtime::new();
        let mut host = TestHost::default();
        let before = runtime.allocation_report();
        runtime.set_global_accessor(
            "setting",
            "target",
            "setting",
            Value::Native("host.get.setting".into()),
        );
        assert!(!runtime.is_fatal());
        let after = runtime.allocation_report();
        assert_eq!(before.phases.bootstrap, 25_999 + 156 + 725); // real reduceRight property
        assert_eq!(after.phases.bootstrap, before.phases.bootstrap);
        assert!(after.phases.runtime > before.phases.runtime);
        let property = runtime.objects[0]
            .properties
            .iter()
            .find(|p| p.key == "setting")
            .unwrap();
        assert!(property.getter && property.writable && !property.configurable);
        assert!(
            runtime
                .execute("function setting(){}", &mut host)
                .unwrap_err()
                .contains("conflicts with host global")
        );
        assert!(
            !runtime.is_fatal(),
            "ordinary declaration error must stay recoverable"
        );
    }
}
