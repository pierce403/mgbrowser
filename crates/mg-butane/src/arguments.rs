//! Empty-only arguments snapshot deferral. Nonempty incoming vectors keep their
//! existing ownership/copy path; a pending binding retains only its callee id.
use super::*;

impl Runtime {
    pub(super) fn defer_empty_arguments(&mut self, environment: usize, callee: usize) -> Eval<()> {
        if environment == 0
            || environment >= self.environments.len()
            || callee >= self.functions.len()
            || self.environments[environment]
                .bindings
                .iter()
                .any(|binding| binding.name == "arguments")
        {
            return Err(Fault::Fatal(
                "Invalid pending JavaScript arguments binding".into(),
            ));
        }
        // Keep normal local binding admission and its nondeletable attribute.
        // Calls reach this after all formals and before any body hoisting.
        self.define(environment, "arguments", Value::Undefined)?;
        self.environments[environment]
            .bindings
            .last_mut()
            .expect("newly admitted arguments binding")
            .pending_empty_arguments = Some(callee);
        Ok(())
    }

    pub(super) fn materialize_empty_arguments(
        &mut self,
        environment: usize,
        index: usize,
    ) -> Eval<()> {
        let Some(callee) = self.environments[environment].bindings[index].pending_empty_arguments
        else {
            return Ok(());
        };
        // These are the same zero-slot construction, metadata admission, brand,
        // original callee and property write used by an eager snapshot. Only
        // their timing changes. put_own supplies the existing fuel step; there
        // is no callback, extra synthetic step or property/binding bypass.
        let items = PrepaidArray::with_slots(&mut self.budget, 0, AllocationPhase::Runtime)?;
        let arguments = self.object(Some(self.object_prototype), Some(items))?;
        self.objects[arguments].arguments = true;
        self.put_own(arguments, "callee", Value::Function(callee), false)?;

        // Publish only a complete object. On failure retain the pending binding
        // and any admitted unreachable object charge, and propagate the fatal
        // error to the existing realm latch. No half-built value is observable.
        let binding = &mut self.environments[environment].bindings[index];
        binding.value = Value::Object(arguments);
        binding.pending_empty_arguments = None;
        Ok(())
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

    fn function_id(value: Value) -> usize {
        let Value::Function(id) = value else {
            panic!("function expected")
        };
        id
    }
    fn object_id(value: Value) -> usize {
        let Value::Object(id) = value else {
            panic!("object expected")
        };
        id
    }
    fn pending(runtime: &Runtime, environment: usize) -> &Binding {
        runtime.environments[environment]
            .bindings
            .iter()
            .find(|binding| binding.name == "arguments")
            .unwrap()
    }
    fn seed() -> (Runtime, usize, usize) {
        let mut runtime = Runtime::new();
        let callee = function_id(
            runtime
                .execute("(function(){return 7;});", &mut NoIo)
                .unwrap(),
        );
        let environment = runtime.environment(0, true).unwrap();
        (runtime, environment, callee)
    }
    fn deferred() -> (Runtime, usize, usize) {
        let (mut runtime, environment, callee) = seed();
        runtime.defer_empty_arguments(environment, callee).unwrap();
        (runtime, environment, callee)
    }
    fn leave_bytes(runtime: &mut Runtime, remaining: usize) {
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - remaining)
            .unwrap();
    }

    #[test]
    fn layouts_and_real_pending_binding_admission_fit_existing_allowances() {
        assert!(std::mem::size_of::<Binding>() <= 128);
        assert!(std::mem::size_of::<Environment>() <= 128);
        assert!(std::mem::size_of::<Object>() <= 128);
        let (mut runtime, environment, callee) = seed();
        eprintln!(
            "EMPTY_ARGUMENTS_LAYOUT Binding={} Environment={} Object={} Bootstrap={}",
            std::mem::size_of::<Binding>(),
            std::mem::size_of::<Environment>(),
            std::mem::size_of::<Object>(),
            runtime.allocation_report().phases.bootstrap
        );
        let before = runtime.allocation_report();
        let objects = runtime.objects.len();
        let fuel = runtime.budget.fuel;
        runtime.defer_empty_arguments(environment, callee).unwrap();
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            137
        );
        assert_eq!(runtime.objects.len(), objects);
        assert_eq!(runtime.budget.fuel, fuel);
        assert_eq!(runtime.lookup(environment, "arguments"), Some(environment));
        assert_eq!(
            pending(&runtime, environment).pending_empty_arguments,
            Some(callee)
        );
        assert_eq!(pending(&runtime, environment).value, Value::Undefined);
        assert!(!pending(&runtime, environment).deletable);
        assert!(runtime.allocation_report().is_valid());

        let (mut runtime, environment, callee) = seed();
        leave_bytes(&mut runtime, 136);
        let objects = runtime.objects.len();
        let outcome = runtime.defer_empty_arguments(environment, callee);
        assert!(matches!(outcome, Err(Fault::Fatal(_))));
        assert!(runtime.environments[environment].bindings.is_empty());
        assert_eq!(runtime.objects.len(), objects);
        assert_eq!(
            runtime
                .allocation_report()
                .first_rejected
                .unwrap()
                .requested_bytes,
            137
        );
    }

    #[test]
    fn empty_call_and_first_read_pay_only_actual_storage_once() {
        let mut runtime = Runtime::new();
        let callee = function_id(
            runtime
                .execute("(function(){return 7;});", &mut NoIo)
                .unwrap(),
        );
        let before = runtime.allocation_report();
        let objects = runtime.objects.len();
        assert_eq!(
            runtime
                .invoke(Value::Function(callee), Value::Undefined, vec![], &mut NoIo)
                .unwrap(),
            Value::Number(7.0)
        );
        let environment = runtime.environments.len() - 1;
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            265
        );
        assert_eq!(runtime.objects.len(), objects);
        assert_eq!(
            pending(&runtime, environment).pending_empty_arguments,
            Some(callee)
        );
        let before_read = runtime.allocation_report();
        let fuel = runtime.budget.fuel;
        let snapshot = object_id(runtime.binding(environment, "arguments").unwrap());
        assert_eq!(
            runtime.allocation_report().phases.runtime - before_read.phases.runtime,
            262
        );
        assert_eq!(fuel - runtime.budget.fuel, 1); // existing callee put_own only
        assert_eq!(runtime.objects.len(), objects + 1);
        assert!(runtime.objects[snapshot].arguments);
        assert!(runtime.objects[snapshot].array.as_ref().unwrap().is_empty());
        assert_eq!(
            runtime.objects[snapshot].prototype,
            Some(PrototypeIdentity::Object(runtime.object_prototype))
        );
        assert_eq!(
            runtime.objects[snapshot]
                .properties
                .iter()
                .find(|p| p.key == "callee")
                .unwrap()
                .value,
            Value::Function(callee)
        );
        assert_eq!(pending(&runtime, environment).pending_empty_arguments, None);
        let after = runtime.allocation_report();
        let fuel = runtime.budget.fuel;
        assert_eq!(
            runtime.binding(environment, "arguments").unwrap(),
            Value::Object(snapshot)
        );
        assert_eq!(runtime.allocation_report(), after);
        assert_eq!(runtime.budget.fuel, fuel);

        // A single explicit undefined is nonempty, even though its payload costs zero.
        let before = runtime.allocation_report();
        let objects = runtime.objects.len();
        runtime
            .invoke(
                Value::Function(callee),
                Value::Undefined,
                vec![Value::Undefined],
                &mut NoIo,
            )
            .unwrap();
        let environment = runtime.environments.len() - 1;
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            591
        );
        assert_eq!(runtime.objects.len(), objects + 1);
        assert_eq!(pending(&runtime, environment).pending_empty_arguments, None);
        let snapshot = object_id(pending(&runtime, environment).value.clone());
        assert_eq!(
            runtime.objects[snapshot].array.as_ref().unwrap(),
            &vec![Some(Value::Undefined)]
        );
    }

    #[test]
    fn replacement_cancels_pending_but_declare_and_delete_only_inspect_metadata() {
        let (mut runtime, environment, callee) = deferred();
        let before = runtime.allocation_report();
        runtime.declare(environment, "arguments", true).unwrap();
        let deleted = runtime
            .expression(
                &Expr::Unary {
                    op: "delete".into(),
                    expr: Box::new(Expr::Ident("arguments".into())),
                },
                environment,
                &Value::Undefined,
                &mut NoIo,
            )
            .unwrap();
        assert_eq!(deleted, Value::Bool(false));
        assert_eq!(
            pending(&runtime, environment).pending_empty_arguments,
            Some(callee)
        );
        assert!(!pending(&runtime, environment).deletable);
        assert_eq!(runtime.allocation_report(), before);
        for value in [
            Value::Undefined,
            Value::Number(9.0),
            Value::String(vec![0xd800, 120]),
        ] {
            let (mut runtime, environment, _) = deferred();
            let before = runtime.allocation_report();
            let objects = runtime.objects.len();
            let bytes = value_bytes(&value);
            runtime
                .define(environment, "arguments", value.clone())
                .unwrap();
            assert_eq!(runtime.objects.len(), objects);
            assert_eq!(pending(&runtime, environment).pending_empty_arguments, None);
            assert_eq!(pending(&runtime, environment).value, value);
            assert!(!pending(&runtime, environment).deletable);
            assert_eq!(
                runtime.allocation_report().phases.runtime - before.phases.runtime,
                bytes as u64
            );
        }
        // A failed ordinary payload admission must not cancel the pending value.
        let (mut runtime, environment, callee) = deferred();
        leave_bytes(&mut runtime, 1);
        assert!(matches!(
            runtime.define(environment, "arguments", Value::text("x")),
            Err(Fault::Fatal(_))
        ));
        assert_eq!(
            pending(&runtime, environment).pending_empty_arguments,
            Some(callee)
        );
        assert_eq!(pending(&runtime, environment).value, Value::Undefined);
    }

    #[test]
    fn first_read_heap_failure_retains_pending_and_never_publishes_partial_object() {
        for (remaining, accepted, request) in [(127, 0, 128), (128, 128, 134), (261, 128, 134)] {
            let (mut runtime, environment, callee) = deferred();
            runtime.set_global("earlier", Value::Bool(true));
            leave_bytes(&mut runtime, remaining);
            let before = runtime.budget.allocated;
            let objects = runtime.objects.len();
            let outcome = runtime.binding(environment, "arguments");
            let error = runtime.finish(outcome).unwrap_err();
            assert!(error.contains("allocation budget"), "{error}");
            let report = runtime.allocation_report();
            assert_eq!(report.accepted_bytes, (before + accepted) as u64);
            assert_eq!(report.first_rejected.unwrap().requested_bytes, request);
            assert_eq!(
                report.first_rejected.unwrap().phase,
                AllocationPhase::Runtime
            );
            assert_eq!(
                pending(&runtime, environment).pending_empty_arguments,
                Some(callee)
            );
            assert_eq!(pending(&runtime, environment).value, Value::Undefined);
            assert_eq!(runtime.objects.len(), objects + usize::from(accepted != 0));
            if accepted != 0 {
                let orphan = runtime.objects.last().unwrap();
                assert!(orphan.arguments);
                assert!(orphan.array.as_ref().unwrap().is_empty());
                assert!(orphan.properties.is_empty());
            }
            assert_eq!(runtime.get_global("earlier"), Value::Bool(true));
            assert_eq!(
                runtime.execute("earlier=false;", &mut NoIo).unwrap_err(),
                error
            );
            assert_eq!(
                runtime
                    .invoke(Value::Function(callee), Value::Undefined, vec![], &mut NoIo)
                    .unwrap_err(),
                error
            );
            runtime.set_global("earlier", Value::Bool(false));
            assert_eq!(runtime.get_global("earlier"), Value::Bool(true));
            assert_eq!(runtime.allocation_report(), report);
            assert_eq!(runtime.budget.calls, 0);
            assert_eq!(runtime.budget.active_expressions, 0);
            assert_eq!(runtime.budget.evaluation_entries, 0);
        }
    }

    #[test]
    fn object_and_real_property_write_fuel_failures_preserve_pending_state() {
        let (mut runtime, environment, callee) = deferred();
        while runtime.objects.len() < MAX_OBJECTS {
            runtime.object(None, None).unwrap();
        }
        let before = runtime.allocation_report();
        let outcome = runtime.binding(environment, "arguments");
        assert!(
            runtime
                .finish(outcome)
                .unwrap_err()
                .contains("object limit")
        );
        assert_eq!(runtime.allocation_report(), before);
        assert_eq!(
            pending(&runtime, environment).pending_empty_arguments,
            Some(callee)
        );
        assert_eq!(pending(&runtime, environment).value, Value::Undefined);

        let (mut runtime, environment, callee) = deferred();
        runtime.budget.fuel = 0;
        let before = runtime.allocation_report();
        let objects = runtime.objects.len();
        let outcome = runtime.binding(environment, "arguments");
        let error = runtime.finish(outcome).unwrap_err();
        assert_eq!(error, "JavaScript fuel exhausted");
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            128
        );
        assert!(runtime.allocation_report().first_rejected.is_none());
        assert_eq!(runtime.objects.len(), objects + 1);
        assert!(runtime.objects.last().unwrap().properties.is_empty());
        assert_eq!(
            pending(&runtime, environment).pending_empty_arguments,
            Some(callee)
        );
        assert_eq!(pending(&runtime, environment).value, Value::Undefined);
        let after = runtime.allocation_report();
        assert_eq!(runtime.execute("42;", &mut NoIo).unwrap_err(), error);
        assert_eq!(runtime.allocation_report(), after);
    }

    #[test]
    fn materialization_creates_own_callee_without_inherited_getter_or_put_callbacks() {
        let (mut runtime, environment, callee) = deferred();
        let prototype = runtime.object_prototype;
        runtime
            .put_own(
                prototype,
                "callee",
                Value::Native("host.must_not_run".into()),
                false,
            )
            .unwrap();
        let property = runtime.objects[prototype]
            .properties
            .iter_mut()
            .find(|p| p.key == "callee")
            .unwrap();
        property.getter = true;
        property.writable = false;
        let snapshot = runtime.binding(environment, "arguments").unwrap();
        assert_eq!(
            runtime.get(&snapshot, "callee", &mut NoIo).unwrap(),
            Value::Function(callee)
        );
        let object = &runtime.objects[object_id(snapshot)];
        let own = object
            .properties
            .iter()
            .find(|p| p.key == "callee")
            .unwrap();
        assert!(!own.enumerable);
        assert!(own.writable && own.configurable && !own.getter);
    }

    #[test]
    fn retained_environment_keeps_original_callee_and_nested_calls_get_distinct_bindings() {
        let mut runtime = Runtime::new();
        let outer = function_id(
            runtime
                .execute("(function(){return function(){return 7;};});", &mut NoIo)
                .unwrap(),
        );
        let inner = function_id(
            runtime
                .invoke(Value::Function(outer), Value::Undefined, vec![], &mut NoIo)
                .unwrap(),
        );
        let outer_environment = runtime.functions[inner].environment;
        assert_eq!(
            pending(&runtime, outer_environment).pending_empty_arguments,
            Some(outer)
        );
        // Grow the arenas after the original call has returned; the pending
        // identity is an arena id, not a borrowed call frame or code pointer.
        for _ in 0..8 {
            runtime
                .execute("(function(){return 11;});", &mut NoIo)
                .unwrap();
            runtime.environment(0, true).unwrap();
        }
        let outer_snapshot = runtime.binding(outer_environment, "arguments").unwrap();
        assert_eq!(
            runtime.get(&outer_snapshot, "callee", &mut NoIo).unwrap(),
            Value::Function(outer)
        );
        assert_eq!(
            runtime
                .invoke(Value::Function(inner), Value::Undefined, vec![], &mut NoIo)
                .unwrap(),
            Value::Number(7.0)
        );
        let inner_environment = runtime.environments.len() - 1;
        assert_ne!(inner_environment, outer_environment);
        assert_eq!(
            pending(&runtime, inner_environment).pending_empty_arguments,
            Some(inner)
        );
        let inner_snapshot = runtime.binding(inner_environment, "arguments").unwrap();
        assert_ne!(inner_snapshot, outer_snapshot);
        assert_eq!(
            runtime.get(&inner_snapshot, "callee", &mut NoIo).unwrap(),
            Value::Function(inner)
        );
        assert_eq!(
            runtime.binding(outer_environment, "arguments").unwrap(),
            outer_snapshot
        );
        assert!(runtime.allocation_report().is_valid());
    }
}
