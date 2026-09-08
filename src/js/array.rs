//! Bounded ES5-shaped concat. Array identity is distinct from indexed arguments
//! storage; result construction never routes new own slots through inherited Put.
use super::*;

impl Runtime {
    pub(super) fn is_array(&self, value: &Value) -> bool {
        matches!(value, Value::Object(id) if self.objects.get(*id)
            .is_some_and(|object| object.array.is_some() && !object.arguments))
    }

    pub(super) fn array_concat(
        &mut self,
        this: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        if matches!(this, Value::Null | Value::Undefined) {
            return Err(exception(
                "TypeError: Array.concat requires a non-null receiver",
            ));
        }
        // Only the receiver undergoes ToObject. Nonarray arguments (including
        // opaque Host handles) are values, never array-like/coercion protocols.
        let receiver = self.boxed(this)?;
        let empty = PrepaidArray::with_slots(&mut self.budget, 0, AllocationPhase::Runtime)?;
        let result = self.object(Some(self.array_prototype), Some(empty))?;
        let mut output = PrepaidArray::growing(AllocationPhase::Runtime);
        for operand in std::iter::once(receiver).chain(args) {
            self.budget.step()?;
            if self.is_array(&operand) {
                let Value::Object(id) = operand else {
                    unreachable!()
                };
                // Capture when this operand is reached, not before earlier
                // getters can mutate it. Hold no source borrow across callbacks.
                let length = self.objects[id].array.as_ref().unwrap().len();
                if length > MAX_ARRAY.saturating_sub(output.len()) {
                    return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
                }
                for index in 0..length {
                    self.budget.step()?;
                    // Resource admission precedes HasProperty/Get and therefore
                    // all source getter effects, including for missing slots.
                    output.prepare_push(&mut self.budget)?;
                    let key = IndexKey::new(index, &mut self.budget)?;
                    let value =
                        if self.has_property(&operand, KeyRef::String(key.as_str()), false)? {
                            Some(self.get(&operand, key.as_str(), host)?)
                        } else {
                            None
                        };
                    // get() paid for an actual payload read. Consume that copy;
                    // holes still occupy paid slots and advance logical length.
                    output.push_owned(&mut self.budget, value)?;
                }
            } else {
                output.prepare_push(&mut self.budget)?;
                // Runtime/native call ingress or source evaluation already owns
                // and admits this payload. No new payload copy is performed.
                output.push_owned(&mut self.budget, Some(operand))?;
            }
        }
        // The result identity/metadata was admitted before callbacks, but its
        // empty storage and private index were never exposed to the script.
        // Adopt only the complete paid vector; no Set can invoke prototype
        // setters/readonly guards. Retain trailing holes in its actual length.
        self.objects[result].array = Some(output.into_values()?);
        Ok(Value::Object(result))
    }
}

// The unchanged 10,000-element cap bounds indices to four decimal digits. Keep a
// fifth byte for a checked boundary, no heap key allocation or uncharged clone.
pub(super) struct IndexKey {
    bytes: [u8; 5],
    start: usize,
}
impl IndexKey {
    pub(super) fn new(mut index: usize, budget: &mut Budget) -> Eval<Self> {
        if index >= MAX_ARRAY || MAX_ARRAY > 100_000 {
            return Err(Fault::Fatal(
                "JavaScript array index limit exhausted".into(),
            ));
        }
        let mut key = Self {
            bytes: [0; 5],
            start: 5,
        };
        loop {
            budget.step()?;
            key.start -= 1;
            key.bytes[key.start] = b'0' + (index % 10) as u8;
            index /= 10;
            if index == 0 {
                break;
            }
        }
        Ok(key)
    }
    pub(super) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[self.start..]).expect("decimal ASCII index")
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

    fn object_id(value: Value) -> usize {
        let Value::Object(id) = value else {
            panic!("expected object")
        };
        id
    }

    fn array(runtime: &mut Runtime, values: Vec<Option<Value>>) -> usize {
        let mut paid =
            PrepaidArray::with_slots(&mut runtime.budget, values.len(), AllocationPhase::Runtime)
                .unwrap();
        for value in values {
            if let Some(value) = &value {
                runtime.budget.allocate(value_bytes(value)).unwrap();
            }
            paid.push_owned(&mut runtime.budget, value).unwrap();
        }
        runtime
            .object(Some(runtime.array_prototype), Some(paid))
            .unwrap()
    }

    // Existing internal descriptors only: no new public accessor capability.
    fn getter(runtime: &mut Runtime, object: usize, key: &str, function: Value) {
        runtime.put_own(object, key, function, false).unwrap();
        runtime.objects[object]
            .properties
            .iter_mut()
            .find(|property| property.key == key)
            .unwrap()
            .getter = true;
    }

    fn inherited_getter(runtime: &mut Runtime, key: &str, name: &str) {
        let function = runtime.get_global(name);
        getter(runtime, runtime.object_prototype, key, function);
    }

    fn leave_bytes(runtime: &mut Runtime, remaining: usize) {
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - remaining)
            .unwrap();
    }

    #[test]
    fn indexed_storage_brand_and_metadata_layout_remain_bounded() {
        let mut runtime = Runtime::new();
        assert!(std::mem::size_of::<Object>() <= 128);
        assert!(std::mem::size_of::<Property>() <= 128);
        eprintln!(
            "concat layout Object={} Property={} bootstrap={}",
            std::mem::size_of::<Object>(),
            std::mem::size_of::<Property>(),
            runtime.allocation_report().phases.bootstrap
        );
        assert!(runtime.is_array(&Value::Object(runtime.array_prototype)));
        assert!(
            runtime.objects[runtime.array_prototype]
                .array
                .as_ref()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            runtime.objects[runtime.array_prototype].prototype,
            Some(PrototypeIdentity::Object(runtime.object_prototype))
        );
        let arguments = object_id(
            runtime
                .execute(
                    "function snapshot(x){return arguments} snapshot(7);",
                    &mut NoIo,
                )
                .unwrap(),
        );
        assert!(runtime.objects[arguments].arguments);
        assert_eq!(
            runtime.objects[arguments].array.as_ref().unwrap(),
            &vec![Some(Value::Number(7.0))]
        );
        assert!(!runtime.is_array(&Value::Object(arguments)));
        assert_eq!(
            runtime.objects[arguments].prototype,
            Some(PrototypeIdentity::Object(runtime.object_prototype))
        );
        assert!(runtime.allocation_report().is_valid());
    }

    #[test]
    fn decimal_keys_use_bounded_stack_storage_and_per_digit_fuel() {
        let mut runtime = Runtime::new();
        for (index, expected) in [
            (0, "0"),
            (9, "9"),
            (10, "10"),
            (99, "99"),
            (999, "999"),
            (9999, "9999"),
        ] {
            let fuel = runtime.budget.fuel;
            let report = runtime.allocation_report();
            let key = IndexKey::new(index, &mut runtime.budget).unwrap();
            assert_eq!(key.as_str(), expected);
            assert_eq!(fuel - runtime.budget.fuel, expected.len() as u64);
            assert_eq!(runtime.allocation_report(), report);
        }
        assert!(matches!(
            IndexKey::new(MAX_ARRAY, &mut runtime.budget),
            Err(Fault::Fatal(_))
        ));
    }

    #[test]
    fn getters_capture_each_operand_length_when_encountered_and_keep_receiver() {
        let mut runtime = Runtime::new();
        runtime
            .execute(
                "var source=Array(2), later=[3], trace='';
            function first(){if(this!==source)throw 'receiver';trace+='a';
                source[1]='added';source.length=4;source[3]='ignored';later[1]=4;return 1}",
                &mut NoIo,
            )
            .unwrap();
        inherited_getter(&mut runtime, "0", "first");
        let result = object_id(runtime.execute("source.concat(later);", &mut NoIo).unwrap());
        assert_eq!(
            runtime.objects[result].array.as_ref().unwrap(),
            &vec![
                Some(Value::Number(1.0)),
                Some(Value::text("added")),
                Some(Value::Number(3.0)),
                Some(Value::Number(4.0)),
            ]
        );
        assert_eq!(runtime.get_global("trace"), Value::text("a"));
        let source = object_id(runtime.get_global("source"));
        assert_eq!(runtime.objects[source].array.as_ref().unwrap().len(), 4);

        let mut runtime = Runtime::new();
        runtime
            .execute(
                "var source=[,2,3];function first(){source.length=1;return 1}",
                &mut NoIo,
            )
            .unwrap();
        inherited_getter(&mut runtime, "0", "first");
        let result = object_id(runtime.execute("source.concat([9]);", &mut NoIo).unwrap());
        assert_eq!(
            runtime.objects[result].array.as_ref().unwrap(),
            &vec![
                Some(Value::Number(1.0)),
                None,
                None,
                Some(Value::Number(9.0)),
            ]
        );
    }

    #[test]
    fn ascending_has_and_get_observe_future_additions_and_deletions() {
        let mut runtime = Runtime::new();
        runtime
            .execute(
                "var source=[,,3], trace='';function first(){trace+='a';
            source[1]='created';delete source[2];return 1}",
                &mut NoIo,
            )
            .unwrap();
        inherited_getter(&mut runtime, "0", "first");
        let result = object_id(runtime.execute("source.concat();", &mut NoIo).unwrap());
        assert_eq!(
            runtime.objects[result].array.as_ref().unwrap(),
            &vec![Some(Value::Number(1.0)), Some(Value::text("created")), None,]
        );
        assert_eq!(runtime.get_global("trace"), Value::text("a"));
    }

    #[test]
    fn abrupt_getter_stops_later_operands_without_poisoning_recovery() {
        let mut runtime = Runtime::new();
        runtime
            .execute(
                "var source=Array(1), later=Array(2), trace='';
            function first(){trace+='a';throw 37}
            function second(){trace+='b';return 2}",
                &mut NoIo,
            )
            .unwrap();
        inherited_getter(&mut runtime, "0", "first");
        inherited_getter(&mut runtime, "1", "second");
        let error = runtime
            .execute("source.concat(later);", &mut NoIo)
            .unwrap_err();
        assert_eq!(error, "Uncaught JavaScript exception: 37");
        assert_eq!(runtime.get_global("trace"), Value::text("a"));
        assert!(runtime.allocation_report().first_rejected.is_none());
        assert_eq!(
            runtime.execute("42;", &mut NoIo).unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn metadata_initial_slot_and_geometric_growth_precede_getters() {
        for (remaining, prefix, accepted, request) in [
            (127, false, 0, 128),
            (191, false, 128, 64),
            (383, true, 256, 128),
        ] {
            let mut runtime = Runtime::new();
            runtime
                .execute(
                    "var entered=false;function first(){entered=true;return 9}",
                    &mut NoIo,
                )
                .unwrap();
            inherited_getter(&mut runtime, "0", "first");
            let source = array(&mut runtime, vec![None]);
            let receiver = if prefix {
                array(
                    &mut runtime,
                    vec![Some(Value::Number(1.0)), Some(Value::Number(2.0))],
                )
            } else {
                source
            };
            let args = if prefix {
                vec![Value::Object(source)]
            } else {
                vec![]
            };
            let objects = runtime.objects.len();
            leave_bytes(&mut runtime, remaining);
            let before = runtime.budget.allocated;
            let outcome = runtime.array_concat(Value::Object(receiver), args, &mut NoIo);
            let error = runtime.finish(outcome).unwrap_err();
            assert!(error.contains("allocation budget"), "{error}");
            let report = runtime.allocation_report();
            let rejection = report.first_rejected.unwrap();
            assert_eq!(rejection.phase, AllocationPhase::Runtime);
            assert_eq!(rejection.requested_bytes, request);
            assert_eq!(report.accepted_bytes, (before + accepted) as u64);
            assert_eq!(runtime.get_global("entered"), Value::Bool(false));
            assert_eq!(runtime.objects.len(), objects + usize::from(accepted != 0));
            if accepted != 0 {
                assert!(
                    runtime
                        .objects
                        .last()
                        .unwrap()
                        .array
                        .as_ref()
                        .unwrap()
                        .is_empty()
                );
            }
            assert_eq!(
                runtime.execute("entered=true;", &mut NoIo).unwrap_err(),
                error
            );
            assert_eq!(runtime.allocation_report(), report);
        }

        let mut runtime = Runtime::new();
        runtime
            .execute(
                "var entered=false;function first(){entered=true;return 9}",
                &mut NoIo,
            )
            .unwrap();
        inherited_getter(&mut runtime, "0", "first");
        let source = array(&mut runtime, vec![None]);
        while runtime.objects.len() < MAX_OBJECTS {
            runtime.object(None, None).unwrap();
        }
        let before = runtime.allocation_report();
        let outcome = runtime.array_concat(Value::Object(source), vec![], &mut NoIo);
        assert!(
            runtime
                .finish(outcome)
                .unwrap_err()
                .contains("object limit")
        );
        assert_eq!(runtime.get_global("entered"), Value::Bool(false));
        assert_eq!(runtime.allocation_report(), before);
    }

    #[test]
    fn true_array_depth_and_operand_cap_fail_before_unreachable_getters() {
        let mut runtime = Runtime::new();
        runtime
            .execute(
                "var entered=false;function first(){entered=true;return 9}",
                &mut NoIo,
            )
            .unwrap();
        inherited_getter(&mut runtime, "0", "first");
        let mut parent = runtime.object_prototype;
        for _ in 0..MAX_CALLS {
            parent = runtime.object(Some(parent), None).unwrap();
        }
        let source = array(&mut runtime, vec![None]);
        // Public prototypes remain immutable; this tests an internal true-array chain.
        runtime.objects[source].prototype = Some(PrototypeIdentity::Object(parent));
        let outcome = runtime.array_concat(Value::Object(source), vec![], &mut NoIo);
        let error = runtime.finish(outcome).unwrap_err();
        assert!(error.contains("prototype depth"), "{error}");
        assert_eq!(runtime.get_global("entered"), Value::Bool(false));
        assert!(runtime.allocation_report().first_rejected.is_none());
        assert_eq!(
            runtime.execute("entered=true;", &mut NoIo).unwrap_err(),
            error
        );

        let mut runtime = Runtime::new();
        runtime
            .execute(
                "var entered=false;function first(){entered=true;return 9}",
                &mut NoIo,
            )
            .unwrap();
        inherited_getter(&mut runtime, "0", "first");
        let full = array(&mut runtime, vec![Some(Value::Number(0.0)); MAX_ARRAY]);
        let later = array(&mut runtime, vec![None]);
        let outcome =
            runtime.array_concat(Value::Object(full), vec![Value::Object(later)], &mut NoIo);
        assert!(runtime.finish(outcome).unwrap_err().contains("array limit"));
        assert_eq!(runtime.get_global("entered"), Value::Bool(false));
    }

    #[test]
    fn owned_nonarray_payload_moves_but_true_array_get_makes_a_paid_copy() {
        let mut runtime = Runtime::new();
        let receiver = array(&mut runtime, vec![]);
        let units = vec![0xd800, 0, b'x' as u16];
        let pointer = units.as_ptr();
        let before = runtime.allocation_report();
        let result = object_id(
            runtime
                .invoke(
                    Value::Native("Array.concat".into()),
                    Value::Object(receiver),
                    vec![Value::String(units)],
                    &mut NoIo,
                )
                .unwrap(),
        );
        let Some(Value::String(result_units)) = &runtime.objects[result].array.as_ref().unwrap()[0]
        else {
            panic!("string")
        };
        assert_eq!(result_units.as_ptr(), pointer);
        assert_eq!(result_units, &vec![0xd800, 0, b'x' as u16]);
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            ("Array.concat".len() + 6 + 64 + 128) as u64
        );

        let mut runtime = Runtime::new();
        let source = array(
            &mut runtime,
            vec![Some(Value::String(vec![0xd800, 0, 120]))],
        );
        let Some(Value::String(units)) = &runtime.objects[source].array.as_ref().unwrap()[0] else {
            panic!("string")
        };
        let pointer = units.as_ptr();
        let before = runtime.allocation_report();
        let result = object_id(
            runtime
                .array_concat(Value::Object(source), vec![], &mut NoIo)
                .unwrap(),
        );
        let Some(Value::String(units)) = &runtime.objects[result].array.as_ref().unwrap()[0] else {
            panic!("string")
        };
        assert_ne!(units.as_ptr(), pointer);
        assert_eq!(units, &vec![0xd800, 0, 120]);
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            128 + 64 + 6
        );
    }

    #[test]
    fn result_own_slots_bypass_inherited_readonly_getter_put_guards() {
        let mut runtime = Runtime::new();
        let source = array(&mut runtime, vec![Some(Value::Number(5.0))]);
        let prototype = runtime.object_prototype;
        getter(
            &mut runtime,
            prototype,
            "0",
            Value::Native("host.must_not_run".into()),
        );
        runtime.objects[prototype]
            .properties
            .iter_mut()
            .find(|p| p.key == "0")
            .unwrap()
            .writable = false;
        let result = object_id(
            runtime
                .array_concat(Value::Object(source), vec![], &mut NoIo)
                .unwrap(),
        );
        assert_eq!(
            runtime.objects[result].array.as_ref().unwrap(),
            &vec![Some(Value::Number(5.0))]
        );
        assert_eq!(
            runtime.get(&Value::Object(result), "0", &mut NoIo).unwrap(),
            Value::Number(5.0)
        );
        assert_eq!(
            runtime.objects[source].array.as_ref().unwrap(),
            &vec![Some(Value::Number(5.0))]
        );
    }

    #[test]
    fn inherited_getter_reentry_is_fatal_and_unwinds_on_default_stack() {
        const CHILD: &str = "MGBROWSER_CONCAT_GETTER_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let mut runtime = Runtime::new();
            runtime.execute("var source=Array(2),caught=false,finished=false,later=false;
                function first(){try{return source.concat()}catch(e){caught=true}finally{finished=true}}
                function second(){later=true;return 2}", &mut NoIo).unwrap();
            inherited_getter(&mut runtime, "0", "first");
            inherited_getter(&mut runtime, "1", "second");
            let error = runtime.execute("source.concat();", &mut NoIo).unwrap_err();
            assert!(error.contains("depth"), "{error}");
            for name in ["caught", "finished", "later"] {
                assert_eq!(runtime.get_global(name), Value::Bool(false));
            }
            assert_eq!(runtime.budget.calls, 0);
            assert_eq!(runtime.budget.active_expressions, 0);
            assert_eq!(runtime.budget.evaluation_entries, 0);
            let report = runtime.allocation_report();
            assert_eq!(
                runtime.execute("later=true;", &mut NoIo).unwrap_err(),
                error
            );
            assert_eq!(runtime.allocation_report(), report);
            return;
        }
        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = OwnedChild(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "js::runtime::array::tests::inherited_getter_reentry_is_fatal_and_unwinds_on_default_stack"])
            .env(CHILD, "1").env_remove("RUST_MIN_STACK")
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()).spawn().unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "default-stack concat child failed: {status}"
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "concat child deadline exceeded"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
