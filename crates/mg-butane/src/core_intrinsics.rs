//! Fixed intrinsic prototype state. Payloads occupy existing object slots;
//! constructor backlinks are ordinary, individually admitted properties.
use super::*;

impl Runtime {
    pub(super) fn core_intrinsics_bootstrap(&mut self) -> Eval<()> {
        self.objects[self.string_prototype].boxed = Some(Value::String(Vec::new()));
        self.objects[self.number_prototype].boxed = Some(Value::Number(0.0));
        self.objects[self.boolean_prototype].boxed = Some(Value::Bool(false));
        for (prototype, name) in [
            (self.object_prototype, "Object"),
            (self.array_prototype, "Array"),
            (self.string_prototype, "String"),
            (self.number_prototype, "Number"),
            (self.boolean_prototype, "Boolean"),
        ] {
            self.put_own(prototype, "constructor", Value::Native(name.into()), false)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoIo;
    impl Host for NoIo {
        fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
            panic!("unexpected Host Get")
        }
        fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
            panic!("unexpected Host Set")
        }
        fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
            panic!("unexpected Host Call")
        }
    }

    fn object(value: Value) -> usize {
        let Value::Object(id) = value else {
            panic!("expected object")
        };
        id
    }
    fn leave_bytes(runtime: &mut Runtime, remaining: usize) {
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - remaining)
            .unwrap();
    }
    fn counters(runtime: &Runtime) {
        assert_eq!(runtime.budget.calls, 0);
        assert_eq!(runtime.budget.active_expressions, 0);
        assert_eq!(runtime.budget.evaluation_entries, 0);
    }

    #[test]
    fn intrinsic_payloads_and_backlinks_use_existing_layout_and_real_properties() {
        use std::mem::size_of;
        let runtime = Runtime::new();
        assert_eq!(runtime.allocation_report().phases.bootstrap, 26_155 + 725);
        assert_eq!(size_of::<Object>(), 112);
        assert!(size_of::<Property>() <= 128);
        assert_eq!(size_of::<Fault>(), 40);
        assert_eq!(size_of::<Eval<Value>>(), 40);
        assert_eq!(size_of::<Eval<Flow>>(), 64);
        assert_eq!(size_of::<Eval<Reference>>(), 56);
        assert_eq!(runtime.objects[runtime.object_prototype].prototype, None);
        assert!(
            runtime.objects[runtime.array_prototype]
                .array
                .as_ref()
                .unwrap()
                .is_empty()
        );
        assert!(runtime.objects[runtime.function_prototype].boxed.is_none());
        assert!(!Value::Object(runtime.function_prototype).callable());
        for (id, name) in [
            (runtime.object_prototype, "Object"),
            (runtime.array_prototype, "Array"),
            (runtime.string_prototype, "String"),
            (runtime.number_prototype, "Number"),
            (runtime.boolean_prototype, "Boolean"),
        ] {
            let properties: Vec<_> = runtime.objects[id]
                .properties
                .iter()
                .filter(|property| property.key == "constructor")
                .collect();
            assert_eq!(properties.len(), 1);
            let property = properties[0];
            assert_eq!(property.value, Value::Native(name.into()));
            assert!(property.writable && property.configurable);
            assert!(!property.enumerable && !property.getter);
        }
        let Some(Value::String(units)) = &runtime.objects[runtime.string_prototype].boxed else {
            panic!("String prototype must have its own string payload")
        };
        assert!(units.is_empty());
        assert_eq!(units.capacity(), 0);
        let Some(Value::Number(number)) = runtime.objects[runtime.number_prototype].boxed else {
            panic!("Number prototype must have its own number payload")
        };
        assert_eq!(number.to_bits(), 0.0f64.to_bits());
        assert_eq!(
            runtime.objects[runtime.boolean_prototype].boxed,
            Some(Value::Bool(false))
        );
    }

    #[test]
    fn each_backlink_is_individually_charged_and_consumes_a_real_put_step() {
        let mut runtime = Runtime::new();
        let ids = [
            runtime.object_prototype,
            runtime.array_prototype,
            runtime.string_prototype,
            runtime.number_prototype,
            runtime.boolean_prototype,
        ];
        // Remove only the five entries in this private reconstruction. Admission
        // credits are cumulative: removal does not refund their original cost.
        for id in ids {
            runtime.objects[id]
                .properties
                .retain(|property| property.key != "constructor");
        }
        let objects = runtime.objects.len();
        let before = runtime.budget.allocated;
        let fuel = runtime.budget.fuel;
        runtime.core_intrinsics_bootstrap().unwrap();
        let expected: usize = ["Object", "Array", "String", "Number", "Boolean"]
            .iter()
            .map(|name| 128 + "constructor".len() + name.len())
            .sum();
        assert_eq!(expected, 725);
        assert_eq!(runtime.budget.allocated - before, expected);
        assert_eq!(fuel - runtime.budget.fuel, 5);
        assert_eq!(runtime.objects.len(), objects);
    }

    #[test]
    fn inherited_primitive_payloads_do_not_brand_children_or_run_conversion_hooks() {
        let mut runtime = Runtime::new();
        for (prototype, method, expected) in [
            (runtime.string_prototype, "String.valueOf", Value::text("")),
            (
                runtime.number_prototype,
                "Number.valueOf",
                Value::Number(0.0),
            ),
            (
                runtime.boolean_prototype,
                "Boolean.valueOf",
                Value::Bool(false),
            ),
        ] {
            assert_eq!(
                runtime
                    .native(method, Value::Object(prototype), vec![], &mut NoIo)
                    .unwrap(),
                expected
            );
            let child = runtime.object(Some(prototype), None).unwrap();
            runtime
                .put_own(
                    child,
                    "valueOf",
                    Value::Native("host.forbidden".into()),
                    true,
                )
                .unwrap();
            runtime
                .put_own(
                    child,
                    "toString",
                    Value::Native("host.forbidden".into()),
                    true,
                )
                .unwrap();
            let tag = PropertyKey::Symbol(runtime.to_string_tag.as_ref().unwrap().clone());
            runtime
                .set_key(Value::Object(child), &tag, Value::text("Number"), &mut NoIo)
                .unwrap();
            assert!(runtime.objects[child].boxed.is_none());
            let before = runtime.budget.allocated;
            assert!(
                matches!(runtime.native(method, Value::Object(child), vec![], &mut NoIo),
                Err(Fault::Throw(value)) if value.as_text().contains("TypeError"))
            );
            assert_eq!(runtime.budget.allocated, before);
        }
        counters(&runtime);
    }

    #[test]
    fn scalar_construction_consumes_owned_operands_without_second_payload_copy() {
        for name in ["Number", "Boolean"] {
            for count in [0, 4_096] {
                let mut runtime = Runtime::new();
                let value = runtime.string(vec![b'1' as u16; count]).unwrap();
                let extra = runtime.string(vec![0xd800, 0xdc00, 0xdc01]).unwrap();
                let objects = runtime.objects.len();
                let before = runtime.budget.allocated;
                let made = object(
                    runtime
                        .construct_value(Value::Native(name.into()), vec![value, extra], &mut NoIo)
                        .unwrap(),
                );
                assert_eq!(runtime.budget.allocated - before, 128);
                assert_eq!(runtime.objects.len(), objects + 1);
                let prototype = if name == "Number" {
                    runtime.number_prototype
                } else {
                    runtime.boolean_prototype
                };
                assert_eq!(
                    runtime.objects[made].prototype,
                    Some(PrototypeIdentity::Object(prototype))
                );
                assert!(runtime.objects[made].properties.is_empty());
                if name == "Boolean" {
                    assert_eq!(runtime.objects[made].boxed, Some(Value::Bool(count != 0)));
                } else {
                    assert!(matches!(
                        runtime.objects[made].boxed,
                        Some(Value::Number(_))
                    ));
                }
                counters(&runtime);
            }
        }
    }

    struct Convert {
        original: usize,
        trace: String,
        fail: bool,
    }
    impl Host for Convert {
        fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
            panic!("unexpected Host Get")
        }
        fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
            panic!("unexpected Host Set")
        }
        fn call(&mut self, name: &str, receiver: Value, args: Vec<Value>) -> Result<Value, String> {
            assert_eq!(receiver, Value::Object(self.original));
            assert!(args.is_empty());
            match name {
                "host.getter" => {
                    self.trace.push('G');
                    Ok(Value::Native("host.number".into()))
                }
                "host.number" => {
                    self.trace.push('N');
                    if self.fail {
                        Err("conversion failed".into())
                    } else {
                        Ok(Value::Number(-0.0))
                    }
                }
                _ => panic!("unexpected Host Call"),
            }
        }
    }
    fn getter_operand(runtime: &mut Runtime) -> (Value, Convert) {
        let owner = runtime
            .object(Some(runtime.object_prototype), None)
            .unwrap();
        let original = runtime.object(Some(owner), None).unwrap();
        runtime
            .put_own(owner, "valueOf", Value::Native("host.getter".into()), false)
            .unwrap();
        runtime.objects[owner]
            .properties
            .iter_mut()
            .find(|property| property.key == "valueOf")
            .unwrap()
            .getter = true;
        (
            Value::Object(original),
            Convert {
                original,
                trace: String::new(),
                fail: false,
            },
        )
    }

    #[test]
    fn real_inherited_getter_and_number_conversion_run_once_with_original_receiver() {
        let mut runtime = Runtime::new();
        let (operand, mut host) = getter_operand(&mut runtime);
        let objects = runtime.objects.len();
        let before = runtime.budget.allocated;
        let made = object(
            runtime
                .construct_value(Value::Native("Number".into()), vec![operand], &mut host)
                .unwrap(),
        );
        assert_eq!(host.trace, "GN");
        assert_eq!(runtime.objects.len(), objects + 1);
        assert_eq!(
            runtime.budget.allocated - before,
            "host.getter".len() + "host.number".len() + 128
        );
        let Some(Value::Number(number)) = runtime.objects[made].boxed else {
            panic!("number payload")
        };
        assert_eq!(number.to_bits(), (-0.0f64).to_bits());
        let before = runtime.budget.allocated;
        let made = object(
            runtime
                .construct_value(
                    Value::Native("Boolean".into()),
                    vec![Value::Object(host.original)],
                    &mut host,
                )
                .unwrap(),
        );
        assert_eq!(host.trace, "GN");
        assert_eq!(runtime.objects[made].boxed, Some(Value::Bool(true)));
        assert_eq!(runtime.budget.allocated - before, 128);
        counters(&runtime);
    }

    #[test]
    fn conversion_error_precedes_instance_allocation_and_remains_catchable() {
        let mut runtime = Runtime::new();
        let (operand, mut host) = getter_operand(&mut runtime);
        host.fail = true;
        let objects = runtime.objects.len();
        let before = runtime.budget.allocated;
        let result =
            runtime.construct_value(Value::Native("Number".into()), vec![operand], &mut host);
        assert!(
            matches!(&result, Err(Fault::Throw(value)) if value.as_text() == "conversion failed")
        );
        assert!(
            runtime
                .finish(result)
                .unwrap_err()
                .contains("conversion failed")
        );
        assert_eq!(host.trace, "GN");
        assert_eq!(runtime.objects.len(), objects);
        assert_eq!(
            runtime.budget.allocated - before,
            "host.getter".len() + "host.number".len()
        );
        assert!(runtime.allocation_report().first_rejected.is_none());
        assert_eq!(
            runtime.execute("42;", &mut NoIo).unwrap(),
            Value::Number(42.0)
        );
        counters(&runtime);
    }

    #[test]
    fn metadata_preflight_happens_after_conversion_and_before_instance_publication() {
        for getter in [false, true] {
            let mut runtime = Runtime::new();
            let (operand, mut host) = getter_operand(&mut runtime);
            let extra = if getter {
                "host.getter".len() + "host.number".len()
            } else {
                0
            };
            let operand = if getter { operand } else { Value::Number(7.0) };
            let objects = runtime.objects.len();
            leave_bytes(&mut runtime, extra + 127);
            let before = runtime.budget.allocated;
            let result =
                runtime.construct_value(Value::Native("Number".into()), vec![operand], &mut host);
            let error = runtime.finish(result).unwrap_err();
            assert_eq!(host.trace, if getter { "GN" } else { "" });
            assert_eq!(runtime.objects.len(), objects);
            let report = runtime.allocation_report();
            assert_eq!(report.accepted_bytes, (before + extra) as u64);
            let rejected = report.first_rejected.as_ref().unwrap();
            assert_eq!(rejected.phase, AllocationPhase::Runtime);
            assert_eq!(rejected.requested_bytes, 128);
            assert_eq!(
                runtime.execute("throw 'later';", &mut NoIo).unwrap_err(),
                error
            );
            assert_eq!(runtime.allocation_report(), report);
            counters(&runtime);
        }
        let mut runtime = Runtime::new();
        let objects = runtime.objects.len();
        leave_bytes(&mut runtime, 127);
        let result = runtime.construct_value(Value::Native("Boolean".into()), vec![], &mut NoIo);
        assert!(matches!(result, Err(Fault::Fatal(_))));
        assert_eq!(runtime.objects.len(), objects);
        assert_eq!(
            runtime
                .allocation_report()
                .first_rejected
                .unwrap()
                .requested_bytes,
            128
        );
    }

    #[test]
    fn bound_scalar_construction_keeps_prefix_copy_and_ignores_bound_receiver() {
        for name in ["Number", "Boolean"] {
            let mut runtime = Runtime::new();
            let bound = runtime
                .invoke(
                    Value::Native("Function.bind".into()),
                    Value::Native(name.into()),
                    vec![Value::String(vec![0xd800; 4_096]), Value::text("7")],
                    &mut NoIo,
                )
                .unwrap();
            let extra = runtime.string(vec![0xdc00; 2_048]).unwrap();
            let before = runtime.budget.allocated;
            let made = object(
                runtime
                    .construct_value(bound, vec![extra], &mut NoIo)
                    .unwrap(),
            );
            assert_eq!(
                runtime.budget.allocated - before,
                2 * 64 + 2 + name.len() + 128
            );
            assert_eq!(
                runtime.objects[made].boxed,
                Some(if name == "Number" {
                    Value::Number(7.0)
                } else {
                    Value::Bool(true)
                })
            );
            counters(&runtime);
        }
    }
}
