//! Boa research adapter. No browser path links this module.
//!
//! Parent containment remains essential: these loop/stack limits are NOT the
//! original Butane cumulative work/heap contract or comprehensive interruption.
use boa_engine::{
    Context, JsData, JsError, JsNativeError, JsResult, JsValue, Module, NativeFunction, Script,
    Source, builtins::error::Error, builtins::promise::PromiseState, context::time::FixedClock,
    error::JsNativeErrorKind, js_string, module::MapModuleLoader, object::JsObject,
};
use boa_gc::{Finalize, Trace};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    path::Path,
    rc::Rc,
};

#[derive(Default)]
struct Completion {
    calls: usize,
    failed: bool,
}

struct Realm {
    context: Context,
    loader: Rc<MapModuleLoader>,
    done: Rc<RefCell<Completion>>,
    terminated: bool,
    assertion_prototype: Option<JsObject>,
}

impl Realm {
    fn new() -> JsResult<Self> {
        let loader = Rc::new(MapModuleLoader::new());
        let mut context = Context::builder()
            .clock(Rc::new(FixedClock::from_millis(1_700_000_000_000)))
            .module_loader(loader.clone())
            .can_block(false)
            .build()?;
        context
            .runtime_limits_mut()
            .set_loop_iteration_limit(1_000_000);
        context.runtime_limits_mut().set_recursion_limit(64);
        context.runtime_limits_mut().set_stack_size_limit(65_536);
        context.runtime_limits_mut().set_backtrace_limit(16);
        let done = Rc::new(RefCell::new(Completion::default()));
        context.insert_data(done.clone());
        context.register_global_builtin_callable(
            js_string!("$DONE"),
            1,
            NativeFunction::from_fn_ptr(|_, args, context| {
                let state = context
                    .get_data::<Rc<RefCell<Completion>>>()
                    .expect("host completion state");
                let mut state = state.borrow_mut();
                state.calls += 1;
                state.failed |= args.first().is_some_and(|v| !v.is_undefined()) || state.calls > 1;
                Ok(JsValue::undefined())
            }),
        )?;
        context.register_global_builtin_callable(
            js_string!("print"),
            1,
            NativeFunction::from_fn_ptr(|_, args, context| {
                // Test262's doneprintHandle.js emits primitive strings. Never invoke
                // user-controlled coercion while recording harness output.
                if let Some(text) = args.first().and_then(JsValue::as_string) {
                    let text = text.to_std_string_escaped();
                    if text == "Test262:AsyncTestComplete"
                        || text.starts_with("Test262:AsyncTestFailure:")
                    {
                        let state = context
                            .get_data::<Rc<RefCell<Completion>>>()
                            .expect("host completion state");
                        let mut state = state.borrow_mut();
                        state.calls += 1;
                        state.failed |= text != "Test262:AsyncTestComplete" || state.calls > 1;
                    }
                }
                Ok(JsValue::undefined())
            }),
        )?;
        Ok(Self {
            context,
            loader,
            done,
            terminated: false,
            assertion_prototype: None,
        })
    }

    fn script(&mut self, source: &str, parse_only: bool) -> Result<JsValue, Value> {
        if self.terminated {
            return Err(termination("runtime", "realm is fatally terminated"));
        }
        let script = Script::parse(Source::from_bytes(source), None, &mut self.context)
            .map_err(|error| self.failure(error, "parse"))?;
        // Compile before evaluation so static failures never count as runtime.
        script
            .codeblock(&mut self.context)
            .map_err(|error| self.failure(error, "early"))?;
        if parse_only {
            return Ok(JsValue::undefined());
        }
        script
            .evaluate(&mut self.context)
            .map_err(|error| self.failure(error, "runtime"))
    }

    fn jobs(&mut self) -> Result<(), Value> {
        if self.terminated {
            return Err(termination("runtime", "realm is fatally terminated"));
        }
        self.context
            .run_jobs()
            .map_err(|error| self.failure(error, "runtime"))
    }

    fn failure(&mut self, error: JsError, phase: &str) -> Value {
        if error.as_engine().is_some() {
            self.terminated = true;
        }
        error_response(
            &error,
            phase,
            Some(&self.context),
            self.assertion_prototype.as_ref(),
        )
    }

    fn module(
        &mut self,
        source: &str,
        modules: Option<&serde_json::Map<String, Value>>,
        parse_only: bool,
    ) -> Result<JsValue, Value> {
        if self.terminated {
            return Err(termination("runtime", "realm is fatally terminated"));
        }
        let module = Module::parse(
            Source::from_bytes(source).with_path(Path::new("/main.mjs")),
            None,
            &mut self.context,
        )
        .map_err(|error| self.failure(error, "parse"))?;
        if parse_only {
            return Ok(JsValue::undefined());
        }
        if let Some(modules) = modules {
            for (name, text) in modules {
                let text = text
                    .as_str()
                    .ok_or_else(|| unsupported("module sources must be strings"))?;
                let name = if name.starts_with('/') {
                    name.clone()
                } else {
                    format!("/{}", name.trim_start_matches("./"))
                };
                let supplied = Module::parse(
                    Source::from_bytes(text).with_path(Path::new(&name)),
                    None,
                    &mut self.context,
                )
                .map_err(|error| self.failure(error, "resolution"))?;
                self.loader.insert(&name, supplied);
            }
        }
        self.loader.insert("/main.mjs", module.clone());
        let loaded = module.load(&mut self.context);
        self.jobs()?;
        match loaded.state() {
            PromiseState::Fulfilled(_) => {}
            PromiseState::Rejected(error) => {
                return Err(self.failure(JsError::from_opaque(error), "resolution"));
            }
            PromiseState::Pending => return Err(unsupported("module load remained pending")),
        }
        module
            .link(&mut self.context)
            .map_err(|error| self.failure(error, "resolution"))?;
        let result = module
            .evaluate(&mut self.context)
            .map_err(|error| self.failure(error, "runtime"))?;
        self.jobs()?;
        match result.state() {
            PromiseState::Fulfilled(value) => Ok(value),
            PromiseState::Rejected(error) => {
                Err(self.failure(JsError::from_opaque(error), "runtime"))
            }
            PromiseState::Pending => Err(unsupported("module evaluation remained pending")),
        }
    }
}

fn termination(phase: &str, message: &str) -> Value {
    json!({"protocol":1,"outcome":"termination","phase":phase,"error_type":"EngineLimit","message":message})
}

fn unsupported(message: &str) -> Value {
    json!({"protocol":1,"outcome":"unsupported","phase":"harness","message":message})
}

fn native_type(kind: &JsNativeErrorKind) -> &'static str {
    match kind {
        JsNativeErrorKind::Error => "Error",
        JsNativeErrorKind::Eval => "EvalError",
        JsNativeErrorKind::Type => "TypeError",
        JsNativeErrorKind::Range => "RangeError",
        JsNativeErrorKind::Reference => "ReferenceError",
        JsNativeErrorKind::Syntax => "SyntaxError",
        JsNativeErrorKind::Uri => "URIError",
        JsNativeErrorKind::Aggregate(_) => "AggregateError",
        _ => "UnknownError",
    }
}

fn error_response(
    error: &JsError,
    phase: &str,
    context: Option<&Context>,
    assertion_prototype: Option<&JsObject>,
) -> Value {
    if let Some(engine) = error.as_engine() {
        return termination(phase, &engine.to_string());
    }
    if let Some(native) = error.as_native() {
        return json!({"protocol":1,"outcome":"exception","phase":phase,"error_type":native_type(native.kind()),"message":native.message().chars().take(512).collect::<String>()});
    }
    // try_native(), into_erased(), to_string(), and reading .name/.message can
    // invoke hostile getters. Inspect only the native tag's presence and raw
    // prototype links. Modified/custom prototypes are conservatively unknown.
    let mut error_type = "ThrownValue";
    if let (Some(context), Some(object)) = (context, error.as_opaque().and_then(JsValue::as_object))
    {
        if let Some(assertion_prototype) = assertion_prototype {
            let mut prototype = object.prototype();
            for _ in 0..64 {
                let Some(current) = prototype else {
                    break;
                };
                if &current == assertion_prototype {
                    return json!({"protocol":1,"outcome":"exception","phase":phase,"error_type":"Test262Error","message":"Test262 assertion (not coerced by host)"});
                }
                prototype = current.prototype();
            }
        }
        if object.downcast_ref::<Error>().is_some() {
            let constructors = context.intrinsics().constructors();
            let known = [
                (constructors.type_error().prototype(), "TypeError"),
                (constructors.range_error().prototype(), "RangeError"),
                (constructors.reference_error().prototype(), "ReferenceError"),
                (constructors.syntax_error().prototype(), "SyntaxError"),
                (constructors.eval_error().prototype(), "EvalError"),
                (constructors.uri_error().prototype(), "URIError"),
                (constructors.aggregate_error().prototype(), "AggregateError"),
                (constructors.error().prototype(), "Error"),
            ];
            let mut prototype = object.prototype();
            for _ in 0..64 {
                let Some(current) = prototype else {
                    break;
                };
                if let Some((_, name)) = known.iter().find(|(candidate, _)| candidate == &current) {
                    error_type = name;
                    break;
                }
                prototype = current.prototype();
            }
        }
    }
    json!({"protocol":1,"outcome":"exception","phase":phase,"error_type":error_type,"message":"thrown value (not coerced by host)"})
}

fn value_json(value: &JsValue) -> Value {
    // Tagged primitives preserve -0, non-finite numbers and lone UTF-16
    // surrogates. Objects are not coerced/serialized by a callback-capable API.
    if value.is_undefined() {
        return json!({"type":"undefined"});
    }
    if value.is_null() {
        return Value::Null;
    }
    if let Some(value) = value.as_boolean() {
        return json!(value);
    }
    if let Some(value) = value.as_number() {
        let number = if value.is_nan() {
            json!("NaN")
        } else if value == f64::INFINITY {
            json!("Infinity")
        } else if value == f64::NEG_INFINITY {
            json!("-Infinity")
        } else if value == 0.0 && value.is_sign_negative() {
            json!("-0")
        } else {
            json!(value)
        };
        return if number.is_number() {
            number
        } else {
            json!({"type":"number","value":number})
        };
    }
    if let Some(value) = value.as_string() {
        return json!({"type":"string","utf16":value.iter().collect::<Vec<_>>()});
    }
    if let Some(value) = value.as_bigint() {
        return json!({"type":"bigint","decimal":value.to_string_radix(10)});
    }
    // Identity-bearing objects and symbols are not differential-test oracles.
    // Tests of those values must assert their relationships within JavaScript.
    json!({"type": if value.is_object() { "object" } else { "unsupported-primitive" }})
}

pub fn evaluate(request: &Value) -> Value {
    let mut realm = match Realm::new() {
        Ok(realm) => realm,
        Err(error) => return error_response(&error, "harness", None, None),
    };
    if request["action"] == "probe" {
        return match probe(request["probe"].as_str().unwrap_or(""), &mut realm) {
            Ok(value) | Err(value) => value,
        };
    }
    let Some(source) = request["source"].as_str() else {
        return unsupported("missing source");
    };
    if let Some(includes) = request["includes"].as_array() {
        for include in includes {
            let Some(source) = include["source"].as_str() else {
                return unsupported("missing harness source");
            };
            if let Err(mut error) = realm.script(source, false) {
                error["phase"] = json!("harness");
                return error;
            }
            if include["name"] == "sta.js" {
                // The runner pins harness bytes. Still inspect raw own data
                // descriptors here: a caller-controlled include name must not
                // authorize getters or Proxy traps during classification setup.
                realm.assertion_prototype =
                    own_data_object(&realm.context.global_object(), "Test262Error")
                        .and_then(|constructor| own_data_object(&constructor, "prototype"));
            }
        }
    }
    let parse_only = request["action"] == "parse";
    let result = if request["goal"] == "module" {
        realm.module(source, request["modules"].as_object(), parse_only)
    } else {
        realm.script(source, parse_only)
    };
    let value = match result {
        Ok(value) => value,
        Err(error) => return error,
    };
    if request["drain_jobs"] == true || request["async"] == true {
        if let Err(error) = realm.jobs() {
            return error;
        }
    }
    let done = realm.done.borrow();
    json!({"protocol":1,"outcome":"ok","phase":if parse_only { "parse" } else { "runtime" },
        "done":if done.failed { "error" } else if done.calls == 1 { "ok" } else { "missing" },
        "value":value_json(&value)})
}

fn own_data_object(object: &JsObject, name: &str) -> Option<JsObject> {
    object
        .borrow()
        .properties()
        .get(&js_string!(name).into())?
        .value()
        .and_then(JsValue::as_object)
}

#[derive(Trace, JsData)]
struct HostNode {
    #[unsafe_ignore_trace]
    finalized: Rc<Cell<usize>>,
}

impl Finalize for HostNode {
    fn finalize(&self) {
        self.finalized.set(self.finalized.get() + 1);
    }
}

fn probe(name: &str, realm: &mut Realm) -> Result<Value, Value> {
    let value = match name {
        "host-root-reentry" => {
            let finalized = Rc::new(Cell::new(0));
            let node = JsObject::from_proto_and_data(
                None,
                HostNode {
                    finalized: finalized.clone(),
                },
            );
            node.set(js_string!("value"), 41, false, &mut realm.context)
                .map_err(|e| realm.failure(e, "runtime"))?;
            realm
                .context
                .register_global_builtin_callable(
                    js_string!("hostVisit"),
                    1,
                    NativeFunction::from_copy_closure_with_captures(
                        |_, args, node, context| {
                            boa_gc::force_collect();
                            let callback =
                                args.first().and_then(JsValue::as_callable).ok_or_else(|| {
                                    JsNativeError::typ().with_message("callback required")
                                })?;
                            callback.call(&JsValue::undefined(), &[node.clone().into()], context)
                        },
                        node.clone(),
                    ),
                )
                .map_err(|e| realm.failure(e, "harness"))?;
            realm.script("hostVisit(function(node) { return hostVisit(function(inner) { if (node !== inner) throw new Error('identity'); inner.value++; return inner.value; }); });", false)?;
            boa_gc::force_collect();
            let retained = node
                .get(js_string!("value"), &mut realm.context)
                .map_err(|e| realm.failure(e, "runtime"))?;
            require(
                retained.as_number() == Some(42.0) && finalized.get() == 0,
                "host root did not survive collection/reentry",
            )?;
            json!({"retained_value":value_json(&retained),"finalized_while_rooted":finalized.get(),"nested_callback":true})
        }
        "promise-order" => {
            realm.script("var events=['sync']; Promise.resolve().then(()=>{events.push('first'); Promise.resolve().then(()=>events.push('nested'));}); Promise.resolve().then(()=>events.push('second'));", false)?;
            let before = realm.script("events.join(',')", false)?;
            realm.jobs()?;
            let after = realm.script("events.join(',')", false)?;
            require(
                before.as_string().is_some_and(|v| v == js_string!("sync"))
                    && after
                        .as_string()
                        .is_some_and(|v| v == js_string!("sync,first,second,nested")),
                "Promise checkpoint ordering",
            )?;
            json!({"before":value_json(&before),"after":value_json(&after)})
        }
        "supplied-module" => {
            let modules =
                json!({"dep.mjs":"export let value=41; export function increment(){value++;}"});
            realm.module("import {value,increment} from './dep.mjs'; increment(); globalThis.moduleAnswer=value;", modules.as_object(), false)?;
            let result = realm.script("moduleAnswer", false)?;
            require(
                result.as_number() == Some(42.0),
                "supplied module live binding",
            )?;
            json!({"answer":value_json(&result),"loader":"host-map-only"})
        }
        "fatal-interrupt" => {
            realm
                .context
                .runtime_limits_mut()
                .set_loop_iteration_limit(32);
            realm.script("var marker=0;", false)?;
            let first = realm.script(
                "try { while(true){} } catch(e) { marker=1; } marker=2;",
                false,
            );
            let later = realm.script("marker=3;", false);
            let marker = realm
                .context
                .global_object()
                .get(js_string!("marker"), &mut realm.context)
                .map_err(|e| realm.failure(e, "runtime"))?;
            require(
                first
                    .as_ref()
                    .err()
                    .is_some_and(|v| v["outcome"] == "termination")
                    && later
                        .as_ref()
                        .err()
                        .is_some_and(|v| v["outcome"] == "termination")
                    && marker.as_number() == Some(0.0)
                    && realm.terminated,
                "fatal interruption was caught or later ingress continued",
            )?;
            json!({"first":first.err(),"later":later.err(),"marker":value_json(&marker),"adapter_latched":realm.terminated,"complete_budget_contract":false})
        }
        "gc-cycles" => {
            let finalized = Rc::new(Cell::new(0));
            let retained = JsObject::from_proto_and_data(
                None,
                HostNode {
                    finalized: finalized.clone(),
                },
            );
            retained
                .set(js_string!("value"), 42, false, &mut realm.context)
                .map_err(|e| realm.failure(e, "runtime"))?;
            let mut checkpoints = Vec::new();
            for batch in 0..10 {
                for _ in 0..1_000 {
                    let cycle = JsObject::from_proto_and_data(
                        None,
                        HostNode {
                            finalized: finalized.clone(),
                        },
                    );
                    cycle
                        .set(js_string!("self"), cycle.clone(), false, &mut realm.context)
                        .map_err(|e| realm.failure(e, "runtime"))?;
                }
                realm.context.clear_kept_objects();
                boa_gc::force_collect();
                checkpoints.push(
                    json!({"allocated_cycles":(batch+1)*1000,"finalized_cycles":finalized.get()}),
                );
            }
            let result = retained
                .get(js_string!("value"), &mut realm.context)
                .map_err(|e| realm.failure(e, "runtime"))?;
            require(
                finalized.get() == 10_000 && result.as_number() == Some(42.0),
                "unreachable cycles retained or persistent root lost",
            )?;
            json!({"checkpoints":checkpoints,"retained_value":value_json(&result),"live_heap_bytes":null,"gc_pause_time":null})
        }
        _ => return Err(unsupported("unknown probe")),
    };
    Ok(
        json!({"protocol":1,"outcome":"ok","phase":"runtime","probe":name,"value":true,"metrics":value}),
    )
}

fn require(condition: bool, message: &str) -> Result<(), Value> {
    if condition {
        Ok(())
    } else {
        Err(
            json!({"protocol":1,"outcome":"exception","phase":"harness","error_type":"ProbeAssertion","message":message}),
        )
    }
}
