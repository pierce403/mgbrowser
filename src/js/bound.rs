//! ES5-shaped bound callables. Persistent values stay in the function arena;
//! forwarding uses one bounded index stack and one prepaid argument vector.
use super::*;

pub(super) struct BoundData {
    pub(super) target: Value,
    pub(super) receiver: Value,
    pub(super) arguments: Box<[Value]>,
    pub(super) length: f64,
}
const BOUND_METADATA: usize = 160;

impl Runtime {
    pub(super) fn bind_function(
        &mut self,
        target: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        if !target.callable() {
            return Err(exception("TypeError: bind target is not callable"));
        }
        if let Value::Function(id) = &target {
            self.identity_storage(PrototypeIdentity::Function(*id))?;
        }
        if self.functions.len() >= MAX_OBJECTS {
            return Err(Fault::Fatal("JavaScript function limit exhausted".into()));
        }
        let count = args.len().saturating_sub(1);
        if count > MAX_ARRAY {
            return Err(Fault::Fatal("JavaScript argument limit exhausted".into()));
        }
        // The subset's callable lengths are numeric virtual own fields. Read
        // length, never target.name/prototype/constructor or bound receiver hooks.
        let length = match self.get(&target, "length", host)? {
            Value::Number(length) => (length - count as f64).max(0.0),
            _ => 0.0,
        };
        self.budget
            .allocate_in(AllocationPhase::FunctionCode, BOUND_METADATA)?;
        let properties = self.object(Some(self.function_prototype), None)?;
        self.budget.allocate(count.saturating_mul(64))?;
        // Public invoke pays payload ingress but not Vec slot capacity. Build
        // exact retained slots, moving owned values only after their admission.
        let mut input = args.into_iter();
        let receiver = input.next().unwrap_or(Value::Undefined);
        let mut arguments = Vec::with_capacity(count);
        arguments.extend(input);
        let bound = Rc::new(BoundData {
            target,
            receiver,
            arguments: arguments.into_boxed_slice(),
            length,
        });
        let id = self.functions.len();
        self.functions.push(Function {
            kind: FunctionKind::Bound(bound),
            environment: 0, // no bound lexical environment is created or used
            properties,
            pending_default_prototype: false,
        });
        Ok(Value::Function(id))
    }

    fn enter_bound_forward(&mut self) -> Eval<()> {
        self.budget.step()?;
        if self.budget.calls >= MAX_CALLS {
            return Err(Fault::Fatal("JavaScript call depth exhausted".into()));
        }
        self.budget.enter_evaluation(false)?;
        self.budget.calls += 1;
        Ok(())
    }

    fn check_bound_constructor(&self, target: &Value) -> Eval<()> {
        match target {
            Value::Function(_) => Ok(()),
            Value::Native(name) if name == "Symbol" => {
                Err(exception("TypeError: Symbol is not a constructor"))
            }
            Value::Native(name)
                if matches!(
                    name.as_str(),
                    "String"
                        | "Number"
                        | "Boolean"
                        | "RegExp"
                        | "Array"
                        | "Object"
                        | "Function"
                        | "Error"
                        | "TypeError"
                        | "RangeError"
                        | "ReferenceError"
                        | "URIError"
                        | "SyntaxError"
                ) =>
            {
                Ok(())
            }
            Value::Native(_) => Err(unsupported("this native constructor")),
            _ => Err(exception("TypeError: value is not a constructor")),
        }
    }

    #[inline(never)]
    pub(super) fn forward_bound(
        &mut self,
        function: usize,
        extra: Vec<Value>,
        construct: bool,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let mut entered = 0;
        let result = (|| {
            let mut chain = [0usize; MAX_CALLS];
            let mut count = 0;
            let mut current = function;
            let mut length = extra.len();
            loop {
                self.budget.step()?;
                if count == MAX_CALLS {
                    return Err(Fault::Fatal("JavaScript call depth exhausted".into()));
                }
                // call() already counted the outer bound callable. Construction
                // enters here directly, so each of its wrappers counts here.
                if construct || count != 0 {
                    self.enter_bound_forward()?;
                    entered += 1;
                }
                let Some(Function {
                    kind: FunctionKind::Bound(bound),
                    ..
                }) = self.functions.get(current)
                else {
                    return Err(Fault::Fatal(
                        "Invalid JavaScript bound forwarding target".into(),
                    ));
                };
                length = length.saturating_add(bound.arguments.len());
                if length > MAX_ARRAY {
                    return Err(Fault::Fatal("JavaScript argument limit exhausted".into()));
                }
                chain[count] = current;
                count += 1;
                if let Value::Function(next) = &bound.target
                    && self
                        .functions
                        .get(*next)
                        .is_some_and(|function| matches!(function.kind, FunctionKind::Bound(_)))
                {
                    current = *next;
                } else {
                    break;
                }
            }
            let FunctionKind::Bound(last) = &self.functions[current].kind else {
                unreachable!()
            };
            let last = Rc::clone(last);
            if construct {
                self.check_bound_constructor(&last.target)?;
            }
            // Admit every actual output slot before allocating or copying any
            // retained payload. No repeated intermediate concatenation occurs.
            self.budget.allocate(length.saturating_mul(64))?;
            let mut arguments = Vec::with_capacity(length);
            for id in chain[..count].iter().rev() {
                let FunctionKind::Bound(bound) = &self.functions[*id].kind else {
                    unreachable!()
                };
                let bound = Rc::clone(bound);
                for value in &bound.arguments {
                    self.budget.step()?;
                    arguments.push(self.copy(value)?);
                }
            }
            arguments.extend(extra);
            let target = self.copy(&last.target)?;
            if construct {
                self.construct_value(target, arguments, host)
            } else {
                // Only the innermost receiver is actually forwarded. Outer
                // receivers are neither coerced nor copied merely to discard.
                let receiver = self.copy(&last.receiver)?;
                self.call(target, receiver, arguments, None, host)
            }
        })();
        for _ in 0..entered {
            self.budget.calls -= 1;
            self.budget.leave_evaluation(false);
        }
        result
    }

    pub(super) fn bound_instance_target(&mut self, mut target: Value) -> Eval<Value> {
        let mut traversed = 0;
        loop {
            let Value::Function(id) = &target else {
                return Ok(target);
            };
            let function = self
                .functions
                .get(*id)
                .ok_or_else(|| exception("TypeError: unknown function"))?;
            let FunctionKind::Bound(bound) = &function.kind else {
                return Ok(target);
            };
            self.budget.step()?;
            if traversed == MAX_CALLS {
                return Err(Fault::Fatal(
                    "JavaScript bound instance depth limit exhausted".into(),
                ));
            }
            let bound = Rc::clone(bound);
            target = self.copy(&bound.target)?;
            traversed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoIo;
    impl Host for NoIo {
        fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
            panic!("unexpected host get")
        }
        fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
            panic!("unexpected host set")
        }
        fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
            panic!("unexpected host call")
        }
    }

    fn id(value: Value) -> usize {
        let Value::Function(id) = value else {
            panic!("function expected")
        };
        id
    }
    fn ordinary_target(runtime: &mut Runtime) -> Value {
        runtime
            .execute("(function(a,b){return a;});", &mut NoIo)
            .unwrap()
    }
    fn bound(runtime: &Runtime, id: usize) -> &BoundData {
        let FunctionKind::Bound(data) = &runtime.functions[id].kind else {
            panic!("bound function expected")
        };
        data
    }
    fn leave(runtime: &mut Runtime, remaining: usize) {
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - remaining)
            .unwrap();
    }
    fn counters(runtime: &Runtime) {
        assert_eq!(
            (
                runtime.budget.calls,
                runtime.budget.active_expressions,
                runtime.budget.evaluation_entries
            ),
            (0, 0, 0)
        );
    }

    #[test]
    fn bound_layout_and_real_bootstrap_property_fit_separate_allowances() {
        use std::mem::size_of;
        let runtime = Runtime::new();
        eprintln!(
            "BOUND_LAYOUT Function={} Code={} BoundData={} Object={} Property={} Value={} Bootstrap={}",
            size_of::<Function>(),
            size_of::<Code>(),
            size_of::<BoundData>(),
            size_of::<Object>(),
            size_of::<Property>(),
            size_of::<Value>(),
            runtime.allocation_report().phases.bootstrap
        );
        let controls_and_allowance = 4 * size_of::<usize>();
        assert!(size_of::<Function>() + size_of::<Code>() + controls_and_allowance <= 128);
        assert!(
            size_of::<Function>() + size_of::<BoundData>() + controls_and_allowance
                <= BOUND_METADATA
        );
        assert!(size_of::<Value>() + 16 <= 64); // one prefix slot also covers its block allowance
        let empty: Box<[Value]> = Vec::new().into_boxed_slice();
        assert!(empty.is_empty()); // zero slots own no element allocation
        assert!(size_of::<Object>() <= 128);
        assert_eq!(
            runtime.allocation_report().phases.bootstrap,
            25_854 + 128 + "bind".len() as u64 + "Function.bind".len() as u64 + 156 + 725
        );
        let property = runtime.objects[runtime.function_prototype]
            .properties
            .iter()
            .find(|property| property.key == "bind")
            .unwrap();
        assert_eq!(property.value, Value::Native("Function.bind".into()));
        assert!(!property.enumerable && property.writable && property.configurable);
    }

    #[test]
    fn binding_moves_paid_payloads_but_rebuilds_exact_prefix_storage() {
        let mut runtime = Runtime::new();
        runtime.native_identity("host.echo").unwrap();
        let native = String::from("host.echo");
        let native_pointer = native.as_ptr();
        let receiver = vec![0xd800, 7];
        let receiver_pointer = receiver.as_ptr();
        let prefix = vec![0xdc00, 9];
        let prefix_pointer = prefix.as_ptr();
        let mut input = Vec::with_capacity(4096);
        input.push(Value::String(receiver));
        input.push(Value::String(prefix));
        let input_pointer = input.as_ptr();
        let before = runtime.allocation_report();
        let function = id(runtime
            .invoke(
                Value::Native("Function.bind".into()),
                Value::Native(native),
                input,
                &mut NoIo,
            )
            .unwrap());
        let data = bound(&runtime, function);
        let Value::Native(native) = &data.target else {
            panic!("native target expected")
        };
        let Value::String(receiver) = &data.receiver else {
            panic!("string receiver expected")
        };
        let Value::String(prefix) = &data.arguments[0] else {
            panic!("string prefix expected")
        };
        assert_eq!(native.as_ptr(), native_pointer);
        assert_eq!(receiver.as_ptr(), receiver_pointer);
        assert_eq!(prefix.as_ptr(), prefix_pointer);
        assert_eq!(receiver, &[0xd800, 7]);
        assert_eq!(prefix, &[0xdc00, 9]);
        assert_eq!(data.arguments.len(), 1);
        assert_ne!(data.arguments.as_ptr(), input_pointer);
        assert_eq!(
            runtime.allocation_report().phases.function_code - before.phases.function_code,
            BOUND_METADATA as u64
        );
        assert!(
            runtime.objects[runtime.functions[function].properties]
                .properties
                .is_empty()
        );
        assert_eq!(runtime.functions[function].environment, 0);
        assert!(!runtime.functions[function].pending_default_prototype);

        let mut runtime = Runtime::new();
        let target = ordinary_target(&mut runtime);
        for args in [
            vec![],
            vec![Value::Null],
            vec![Value::Null, Value::Number(7.0)],
        ] {
            let prefix = args.len().saturating_sub(1);
            let before = runtime.allocation_report();
            let function = id(runtime
                .bind_function(target.clone(), args, &mut NoIo)
                .unwrap());
            let after = runtime.allocation_report();
            assert_eq!(
                after.phases.runtime - before.phases.runtime,
                (128 + 64 * prefix) as u64
            );
            assert_eq!(
                after.phases.function_code - before.phases.function_code,
                160
            );
            assert_eq!(bound(&runtime, function).arguments.len(), prefix);
        }
    }

    #[test]
    fn binding_validation_and_numeric_length_do_not_observe_unrelated_getters() {
        let mut runtime = Runtime::new();
        let object = runtime.object(None, None).unwrap();
        runtime
            .put_own(
                object,
                "length",
                Value::Native("host.must_not_run".into()),
                false,
            )
            .unwrap();
        runtime.objects[object].properties[0].getter = true;
        let before = runtime.allocation_report();
        assert!(
            matches!(runtime.bind_function(Value::Object(object), vec![], &mut NoIo), Err(Fault::Throw(value)) if value.as_text().contains("not callable"))
        );
        assert_eq!(runtime.allocation_report(), before);

        let function = id(ordinary_target(&mut runtime));
        let properties = runtime.functions[function].properties;
        for name in ["name", "length"] {
            runtime
                .put_own(
                    properties,
                    name,
                    Value::Native("host.must_not_run".into()),
                    false,
                )
                .unwrap();
            runtime.objects[properties]
                .properties
                .last_mut()
                .unwrap()
                .getter = true;
        }
        // Even a poisoned unpublished default is not read during binding.
        runtime.objects[properties].properties[0].getter = true;
        let bound = id(runtime
            .bind_function(
                Value::Function(function),
                vec![Value::Null, Value::Number(1.0)],
                &mut NoIo,
            )
            .unwrap());
        assert_eq!(
            runtime
                .get(&Value::Function(bound), "length", &mut NoIo)
                .unwrap(),
            Value::Number(1.0)
        );
        assert!(runtime.functions[function].pending_default_prototype);
        assert_eq!(
            runtime
                .get(&Value::Function(bound), "name", &mut NoIo)
                .unwrap(),
            Value::Undefined
        );
        assert_eq!(
            runtime
                .get(&Value::Function(bound), "prototype", &mut NoIo)
                .unwrap(),
            Value::Undefined
        );
    }

    #[test]
    fn binding_metadata_bag_and_prefix_preflights_never_publish_partial_function() {
        for (remaining, accepted, phase, request, orphan) in [
            (159, 0, AllocationPhase::FunctionCode, 160, false),
            (287, 160, AllocationPhase::Runtime, 128, false),
            (351, 288, AllocationPhase::Runtime, 64, true),
        ] {
            let mut runtime = Runtime::new();
            let target = ordinary_target(&mut runtime);
            runtime.set_global("earlier", Value::Bool(true));
            leave(&mut runtime, remaining);
            let before = runtime.allocation_report();
            let functions = runtime.functions.len();
            let objects = runtime.objects.len();
            let outcome = runtime.bind_function(
                target.clone(),
                vec![Value::Null, Value::Number(1.0)],
                &mut NoIo,
            );
            let error = runtime.finish(outcome).unwrap_err();
            let report = runtime.allocation_report();
            assert!(report.is_valid());
            assert_eq!(report.accepted_bytes - before.accepted_bytes, accepted);
            assert_eq!(report.first_rejected.unwrap().phase, phase);
            assert_eq!(report.first_rejected.unwrap().requested_bytes, request);
            assert_eq!(runtime.functions.len(), functions);
            assert_eq!(runtime.objects.len(), objects + usize::from(orphan));
            if orphan {
                assert!(runtime.objects.last().unwrap().properties.is_empty());
            }
            assert_eq!(
                runtime.execute("earlier=false;", &mut NoIo).unwrap_err(),
                error
            );
            assert_eq!(
                runtime
                    .invoke(target, Value::Undefined, vec![], &mut NoIo)
                    .unwrap_err(),
                error
            );
            runtime.set_global("earlier", Value::Bool(false));
            assert_eq!(runtime.get_global("earlier"), Value::Bool(true));
            assert_eq!(runtime.allocation_report(), report);
            counters(&runtime);
        }
    }

    #[test]
    fn restricted_inherited_access_throws_but_nearer_own_fields_shadow_it() {
        let mut runtime = Runtime::new();
        let target = ordinary_target(&mut runtime);
        let function = id(runtime.bind_function(target, vec![], &mut NoIo).unwrap());
        let child = runtime
            .object_with_prototype(Some(PrototypeIdentity::Function(function)), None)
            .unwrap();
        for key in ["caller", "arguments"] {
            assert!(
                runtime
                    .has_property(&Value::Object(child), KeyRef::String(key), false)
                    .unwrap()
            );
            assert!(
                matches!(runtime.get(&Value::Object(child), key, &mut NoIo), Err(Fault::Throw(value)) if value.as_text().contains("TypeError"))
            );
            assert!(
                matches!(runtime.set(Value::Object(child), key, Value::Number(1.0), &mut NoIo), Err(Fault::Throw(value)) if value.as_text().contains("TypeError"))
            );
            assert!(runtime.objects[child].properties.is_empty());
            // Private own-field setup exercises shadowing without adding a
            // public accessor/descriptor or mutable-prototype facility.
            runtime
                .put_own(child, key, Value::Number(2.0), true)
                .unwrap();
            runtime
                .set(Value::Object(child), key, Value::Number(3.0), &mut NoIo)
                .unwrap();
            assert_eq!(
                runtime.get(&Value::Object(child), key, &mut NoIo).unwrap(),
                Value::Number(3.0)
            );
            assert_eq!(
                runtime.delete(Value::Object(child), key).unwrap(),
                Value::Bool(true)
            );
            assert_eq!(
                runtime.delete(Value::Function(function), key).unwrap(),
                Value::Bool(false)
            );
        }
        assert_eq!(
            runtime.execute("42;", &mut NoIo).unwrap(),
            Value::Number(42.0)
        );
        counters(&runtime);
    }

    struct Echo {
        calls: usize,
        receiver: Value,
        args: Vec<Value>,
        name_pointer: usize,
    }
    impl Echo {
        fn new() -> Self {
            Self {
                calls: 0,
                receiver: Value::Undefined,
                args: vec![],
                name_pointer: 0,
            }
        }
    }
    impl Host for Echo {
        fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
            panic!("unexpected host get")
        }
        fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
            panic!("unexpected host set")
        }
        fn call(&mut self, name: &str, receiver: Value, args: Vec<Value>) -> Result<Value, String> {
            assert_eq!(name, "host.echo");
            self.calls += 1;
            self.name_pointer = name.as_ptr() as usize;
            self.receiver = receiver;
            self.args = args;
            Ok(Value::Number(42.0))
        }
    }

    #[test]
    fn flattened_forwarding_copies_retained_payloads_once_and_moves_extra_values() {
        let mut runtime = Runtime::new();
        let inner = runtime
            .invoke(
                Value::Native("Function.bind".into()),
                Value::Native("host.echo".into()),
                vec![Value::text("this"), Value::text("inner")],
                &mut NoIo,
            )
            .unwrap();
        let inner_id = id(inner.clone());
        let outer = runtime
            .invoke(
                Value::Native("Function.bind".into()),
                inner,
                vec![Value::String(vec![120; 100_000]), Value::text("outer")],
                &mut NoIo,
            )
            .unwrap();
        let Value::String(retained_prefix) = &bound(&runtime, inner_id).arguments[0] else {
            panic!("string expected")
        };
        let prefix_pointer = retained_prefix.as_ptr();
        let Value::Native(native) = &bound(&runtime, inner_id).target else {
            panic!("native expected")
        };
        let name_pointer = native.as_ptr() as usize;
        let extra = vec![0xd800, 120];
        let extra_pointer = extra.as_ptr();
        let before = runtime.allocation_report();
        let mut echo = Echo::new();
        assert_eq!(
            runtime
                .invoke(
                    outer,
                    Value::Undefined,
                    vec![Value::String(extra)],
                    &mut echo
                )
                .unwrap(),
            Value::Number(42.0)
        );
        assert_eq!(echo.calls, 1);
        assert_eq!(echo.receiver, Value::text("this"));
        assert_eq!(
            echo.args,
            [
                Value::text("inner"),
                Value::text("outer"),
                Value::String(vec![0xd800, 120])
            ]
        );
        let Value::String(copied) = &echo.args[0] else {
            panic!("string expected")
        };
        let Value::String(moved) = &echo.args[2] else {
            panic!("string expected")
        };
        assert_ne!(copied.as_ptr(), prefix_pointer);
        assert_eq!(moved.as_ptr(), extra_pointer);
        assert_ne!(echo.name_pointer, name_pointer);
        let after = runtime.allocation_report();
        assert_eq!(
            after.phases.runtime - before.phases.runtime,
            3 * 64 + 2 * (4 + 5 + 5 + 2) + "host.echo".len() as u64
        );
        assert_eq!(after.phases.function_code, before.phases.function_code);
        assert_eq!(after.phases.ast, before.phases.ast);
        assert_eq!(after.phases.source, before.phases.source);
        counters(&runtime);
    }

    #[test]
    fn forwarding_slot_and_real_copy_failures_precede_target_effects_and_latch() {
        for (remaining, accepted, requested) in [(63, 0, 64), (83, 64, 20)] {
            let mut runtime = Runtime::new();
            let function = runtime
                .invoke(
                    Value::Native("Function.bind".into()),
                    Value::Native("host.echo".into()),
                    vec![Value::Null, Value::text("0123456789")],
                    &mut NoIo,
                )
                .unwrap();
            leave(&mut runtime, remaining);
            let before = runtime.allocation_report();
            let mut echo = Echo::new();
            let error = runtime
                .invoke(function.clone(), Value::Undefined, vec![], &mut echo)
                .unwrap_err();
            let report = runtime.allocation_report();
            assert_eq!(echo.calls, 0);
            assert_eq!(report.accepted_bytes - before.accepted_bytes, accepted);
            assert_eq!(report.first_rejected.unwrap().requested_bytes, requested);
            assert_eq!(
                runtime
                    .invoke(function, Value::Undefined, vec![], &mut echo)
                    .unwrap_err(),
                error
            );
            assert_eq!(runtime.execute("42;", &mut NoIo).unwrap_err(), error);
            assert_eq!(runtime.allocation_report(), report);
            counters(&runtime);
        }
    }

    #[test]
    fn bound_chain_entries_unwind_after_success_throw_fatal_and_reentry() {
        for (depth, succeeds) in [(63, true), (64, false)] {
            let mut runtime = Runtime::new();
            let mut function = ordinary_target(&mut runtime);
            for _ in 0..depth {
                function = runtime.bind_function(function, vec![], &mut NoIo).unwrap();
            }
            let outcome = runtime.invoke(function, Value::Undefined, vec![], &mut NoIo);
            if succeeds {
                assert_eq!(outcome.unwrap(), Value::Undefined);
            } else {
                let error = outcome.unwrap_err();
                assert_eq!(error, "JavaScript call depth exhausted");
                assert_eq!(runtime.execute("42;", &mut NoIo).unwrap_err(), error);
            }
            counters(&runtime);
        }
        let mut runtime = Runtime::new();
        assert_eq!(runtime.execute("function f(){throw 9;}var b=f.bind(null).bind(null);var caught=false;try{b();}catch(e){caught=e===9;}caught;", &mut NoIo).unwrap(), Value::Bool(true));
        counters(&runtime);
        assert_eq!(
            runtime.execute("42;", &mut NoIo).unwrap(),
            Value::Number(42.0)
        );
        let mut runtime = Runtime::new();
        let error = runtime.execute("var caught=false,finished=false;function f(){return b();}var b=f.bind(null);try{b();}catch(e){caught=true;}finally{finished=true;}", &mut NoIo).unwrap_err();
        assert_eq!(error, "JavaScript call depth exhausted");
        assert_eq!(runtime.get_global("caught"), Value::Bool(false));
        assert_eq!(runtime.get_global("finished"), Value::Bool(false));
        counters(&runtime);
    }

    #[test]
    fn construction_checks_discovery_limits_then_eligibility_before_payloads() {
        let mut runtime = Runtime::new();
        let function = id(runtime
            .invoke(
                Value::Native("Function.bind".into()),
                Value::Native("Symbol".into()),
                vec![Value::Null, Value::String(vec![120; 1000])],
                &mut NoIo,
            )
            .unwrap());
        let before = runtime.allocation_report();
        let outcome = runtime.construct_value(Value::Function(function), vec![], &mut NoIo);
        assert!(
            matches!(outcome, Err(Fault::Throw(value)) if value.as_text().contains("not a constructor"))
        );
        assert_eq!(runtime.allocation_report(), before);
        counters(&runtime);
        // Discovery's bounded count precedes terminal eligibility; this is an
        // explicit resource policy for oversized, otherwise-invalid requests.
        let mut arguments = vec![Value::Null];
        arguments.extend((0..MAX_ARRAY - 1).map(|_| Value::Number(0.0)));
        let outer = id(runtime
            .bind_function(Value::Function(function), arguments, &mut NoIo)
            .unwrap());
        let before = runtime.allocation_report();
        let outcome =
            runtime.construct_value(Value::Function(outer), vec![Value::Number(0.0)], &mut NoIo);
        assert!(
            matches!(outcome, Err(Fault::Fatal(message)) if message == "JavaScript argument limit exhausted")
        );
        assert_eq!(runtime.allocation_report(), before);
        counters(&runtime);
    }

    #[test]
    fn bound_function_and_object_caps_preflight_without_unpublished_rows() {
        let mut runtime = Runtime::new();
        let target = ordinary_target(&mut runtime);
        while runtime.objects.len() < MAX_OBJECTS {
            runtime.object(None, None).unwrap();
        }
        let before = runtime.allocation_report();
        let functions = runtime.functions.len();
        let outcome = runtime.bind_function(target, vec![], &mut NoIo);
        assert!(matches!(outcome, Err(Fault::Fatal(message)) if message.contains("object limit")));
        assert_eq!(runtime.functions.len(), functions);
        assert_eq!(
            runtime.allocation_report().phases.function_code - before.phases.function_code,
            160
        );

        let mut runtime = Runtime::new();
        let target = ordinary_target(&mut runtime);
        let function = id(runtime
            .bind_function(target.clone(), vec![], &mut NoIo)
            .unwrap());
        // Isolate the otherwise earlier object cap using unreachable, paid
        // metadata rows. No synthetic row is ever called or returned to script.
        while runtime.functions.len() < MAX_OBJECTS {
            runtime
                .budget
                .allocate_in(AllocationPhase::FunctionCode, BOUND_METADATA)
                .unwrap();
            let FunctionKind::Bound(data) = &runtime.functions[function].kind else {
                unreachable!()
            };
            runtime.functions.push(Function {
                kind: FunctionKind::Bound(Rc::clone(data)),
                environment: 0,
                properties: runtime.functions[function].properties,
                pending_default_prototype: false,
            });
        }
        let before = runtime.allocation_report();
        assert!(
            matches!(runtime.bind_function(target, vec![], &mut NoIo), Err(Fault::Fatal(message)) if message.contains("function limit"))
        );
        assert_eq!(runtime.allocation_report(), before);
    }
}
