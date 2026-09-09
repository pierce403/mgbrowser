//! Original bounded ES5-shaped indexed callback algorithms. Presence and values
//! are observed at each visit, never snapshotted. Only the receiver length is
//! captured; no arena borrow or Host capability is retained across callbacks.
use super::array::IndexKey;
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallbackKind {
    ForEach,
    Map,
    Filter,
    Some,
    Every,
    Reduce,
    ReduceRight,
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
    fn getter(runtime: &mut Runtime, object: usize, name: &str, function: &str) {
        let function = runtime.get_global(function);
        runtime.put_own(object, name, function, false).unwrap();
        runtime.objects[object]
            .properties
            .iter_mut()
            .find(|property| property.key == name)
            .unwrap()
            .getter = true;
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
    fn callback_registration_has_measured_additive_metadata_and_unchanged_layout() {
        use std::mem::size_of;
        let mut runtime = Runtime::new();
        let added = 128 + "reduceRight".len() + "Array.reduceRight".len();
        assert_eq!(added, 156);
        assert_eq!(
            runtime.allocation_report().phases.bootstrap,
            (25_999 + added + 725) as u64
        );
        assert_eq!(size_of::<CallbackKind>(), 1);
        assert_eq!(size_of::<Fault>(), 40);
        assert_eq!(size_of::<Eval<Value>>(), 40);
        assert_eq!(size_of::<Eval<Flow>>(), 64);
        assert_eq!(size_of::<Eval<Reference>>(), 56);
        assert!(size_of::<Object>() <= 128);
        let property = runtime.objects[runtime.array_prototype]
            .properties
            .iter()
            .find(|property| property.key == "reduceRight")
            .unwrap();
        assert_eq!(property.value, Value::Native("Array.reduceRight".into()));
        assert!(!property.enumerable && property.writable && property.configurable);
        eprintln!(
            "array callbacks bootstrap={} added={added}",
            runtime.allocation_report().phases.bootstrap
        );
        assert_eq!(runtime.execute(
            "Array.prototype.reduceRight.length===1 && Array.prototype.reduceRight.name==='reduceRight' && !Array.prototype.reduceRight.hasOwnProperty('prototype');",
            &mut NoIo).unwrap(), Value::Bool(true));
    }

    #[test]
    fn real_length_getter_converts_once_before_callback_validation_and_index_gets() {
        let mut runtime = Runtime::new();
        runtime.execute(
            "var source={0:7},trace='';function lengthGetter(){if(this!==source)throw 'receiver';trace+='L';return {valueOf:function(){trace+='N';return 4294967297;}}}function visit(v,i,o){if(o!==source)throw 'object';trace+='C';return v+i;}",
            &mut NoIo).unwrap();
        let source = object(runtime.get_global("source"));
        getter(&mut runtime, source, "length", "lengthGetter");
        let result = object(
            runtime
                .execute("Array.prototype.map.call(source,visit);", &mut NoIo)
                .unwrap(),
        );
        assert_eq!(
            runtime.objects[result].array.as_ref().unwrap(),
            &vec![Some(Value::Number(7.0))]
        );
        assert_eq!(runtime.get_global("trace"), Value::text("LNC"));
        runtime.execute("trace='';", &mut NoIo).unwrap();
        let error = runtime
            .execute("Array.prototype.forEach.call(source,3);", &mut NoIo)
            .unwrap_err();
        assert!(error.contains("TypeError"));
        assert_eq!(runtime.get_global("trace"), Value::text("LN"));
        counters(&runtime);
    }

    #[test]
    fn inherited_getters_keep_original_receiver_and_live_mutation_and_own_results() {
        let mut runtime = Runtime::new();
        runtime.execute(
            "var source=[,2,,],trace='';function first(){if(this!==source)throw 'receiver';trace+='G';source[1]=8;source[2]=9;source[3]=10;return 4}function visit(v,i,o){if(o!==source)throw 'object';trace+='C'+i;return v*2}",
            &mut NoIo).unwrap();
        let prototype = runtime.object_prototype;
        getter(&mut runtime, prototype, "0", "first");
        let result = object(runtime.execute("source.map(visit);", &mut NoIo).unwrap());
        assert_eq!(
            runtime.objects[result].array.as_ref().unwrap(),
            &vec![
                Some(Value::Number(8.0)),
                Some(Value::Number(16.0)),
                Some(Value::Number(18.0))
            ]
        );
        assert_eq!(runtime.get_global("trace"), Value::text("GC0C1C2"));
        assert_eq!(
            runtime.objects[result].prototype,
            Some(PrototypeIdentity::Object(runtime.array_prototype))
        );
        counters(&runtime);
    }

    #[test]
    fn indexed_getter_fault_stops_without_callback_and_preserves_inner_context() {
        let mut runtime = Runtime::new();
        runtime.execute("var source=[,2],called=false;function first(){return null.length}function visit(){called=true}", &mut NoIo).unwrap();
        let prototype = runtime.object_prototype;
        getter(&mut runtime, prototype, "0", "first");
        let error = runtime
            .execute("source.forEach(visit);", &mut NoIo)
            .unwrap_err();
        assert!(
            error.ends_with(
                "[member operation=resolve-read base=null key=length] [producer kind=expression]"
            ),
            "{error}"
        );
        assert_eq!(runtime.get_global("called"), Value::Bool(false));
        assert_eq!(
            runtime.execute("42;", &mut NoIo).unwrap(),
            Value::Number(42.0)
        );
        counters(&runtime);
    }

    #[test]
    fn map_result_metadata_and_full_slots_are_admitted_before_indexed_getters() {
        for (remaining, accepted, requested) in [(127, 0, 128), (255, 128, 128)] {
            let mut runtime = Runtime::new();
            runtime.execute("var source=[,2],entered=false;function first(){entered=true;return 7}function visit(v){return v}", &mut NoIo).unwrap();
            let prototype = runtime.object_prototype;
            getter(&mut runtime, prototype, "0", "first");
            let source = runtime.get_global("source");
            let callback = runtime.get_global("visit");
            let objects = runtime.objects.len();
            leave_bytes(&mut runtime, remaining);
            let before = runtime.budget.allocated;
            let result = runtime.array_callback("Array.map", source, vec![callback], &mut NoIo);
            let error = runtime.finish(result).unwrap_err();
            let report = runtime.allocation_report();
            let rejected = report.first_rejected.unwrap();
            assert_eq!(rejected.phase, AllocationPhase::Runtime);
            assert_eq!(rejected.requested_bytes, requested);
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
            counters(&runtime);
        }
    }

    #[derive(Default)]
    struct InspectCall {
        calls: usize,
        pointer: usize,
        argument_count: usize,
    }
    impl Host for InspectCall {
        fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
            panic!("unexpected Get")
        }
        fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
            panic!("unexpected Set")
        }
        fn call(&mut self, name: &str, _: Value, args: Vec<Value>) -> Result<Value, String> {
            assert_eq!(name, "host.inspect");
            self.calls += 1;
            self.argument_count = args.len();
            let Value::String(units) = &args[0] else {
                panic!("expected string")
            };
            self.pointer = units.as_ptr() as usize;
            assert_eq!(units, &[0xd800, 0xdc00, 7]);
            Ok(Value::Bool(true))
        }
    }

    #[test]
    fn filter_retains_original_string_and_pays_independent_callback_copy() {
        let mut runtime = Runtime::new();
        let value = runtime.string(vec![0xd800, 0xdc00, 7]).unwrap();
        let Value::String(units) = &value else {
            unreachable!()
        };
        let pointer = units.as_ptr() as usize;
        let callback = Value::Native("host.inspect".into());
        let receiver = Value::Host("collection".into());
        let this_arg = Value::String(vec![0xdc01]);
        runtime
            .budget
            .allocate(value_bytes(&callback) + value_bytes(&receiver) + value_bytes(&this_arg))
            .unwrap();
        let before = runtime.budget.allocated;
        let mut retained = None;
        let mut host = InspectCall::default();
        assert_eq!(
            runtime
                .call_array_element(
                    &callback,
                    &this_arg,
                    &receiver,
                    value,
                    0,
                    None,
                    Some(&mut retained),
                    &mut host
                )
                .unwrap(),
            Value::Bool(true)
        );
        let Some(Value::String(units)) = retained else {
            panic!("missing retained value")
        };
        assert_eq!(units.as_ptr() as usize, pointer);
        assert_ne!(host.pointer, pointer);
        assert_eq!(host.calls, 1);
        assert_eq!(host.argument_count, 3);
        assert_eq!(
            runtime.budget.allocated - before,
            192 + 6 + "host.inspect".len() + 2 + "collection".len()
        );
        counters(&runtime);
    }

    #[test]
    fn ordinary_element_and_reduction_accumulator_transfer_without_payload_copy() {
        for reduce in [false, true] {
            let mut runtime = Runtime::new();
            let value = runtime.string(vec![0xd800, 0xdc00, 7]).unwrap();
            let Value::String(units) = &value else {
                unreachable!()
            };
            let pointer = units.as_ptr() as usize;
            let callback = Value::Native("host.inspect".into());
            runtime.budget.allocate(value_bytes(&callback)).unwrap();
            let before = runtime.budget.allocated;
            let mut host = InspectCall::default();
            let (element, accumulator) = if reduce {
                (Value::Number(4.0), Some(value))
            } else {
                (value, None)
            };
            runtime
                .call_array_element(
                    &callback,
                    &Value::Undefined,
                    &Value::Object(0),
                    element,
                    0,
                    accumulator,
                    None,
                    &mut host,
                )
                .unwrap();
            assert_eq!(host.pointer, pointer);
            assert_eq!(host.argument_count, if reduce { 4 } else { 3 });
            assert_eq!(
                runtime.budget.allocated - before,
                if reduce {
                    256 + "host.inspect".len()
                } else {
                    192 + "host.inspect".len()
                }
            );
            counters(&runtime);
        }
    }

    #[test]
    fn callback_slot_rejection_precedes_real_copies_and_host_effects() {
        for reduce in [false, true] {
            let mut runtime = Runtime::new();
            let value = runtime.string(vec![0xd800, 0xdc00, 7]).unwrap();
            let mut host = InspectCall::default();
            let count = if reduce { 4 } else { 3 };
            leave_bytes(&mut runtime, count * 64 - 1);
            let before = runtime.allocation_report();
            let mut retained = None;
            let result = runtime.call_array_element(
                &Value::Native("host.inspect".into()),
                &Value::Undefined,
                &Value::Object(0),
                value,
                0,
                if reduce { Some(Value::Undefined) } else { None },
                Some(&mut retained),
                &mut host,
            );
            let error = runtime.finish(result).unwrap_err();
            let after = runtime.allocation_report();
            assert_eq!(after.accepted_bytes, before.accepted_bytes);
            assert_eq!(
                after.first_rejected.unwrap().requested_bytes,
                (count * 64) as u64
            );
            assert!(retained.is_none());
            assert_eq!(host.calls, 0);
            assert_eq!(runtime.execute("42;", &mut NoIo).unwrap_err(), error);
            counters(&runtime);
        }
    }

    #[test]
    fn callback_reentry_and_pending_expressions_stay_bounded_on_default_stack() {
        const CHILD: &str = "MGBROWSER_ARRAY_CALLBACK_DEPTH_CHILD";
        if std::env::var_os(CHILD).is_some() {
            for method in [
                "forEach",
                "map",
                "filter",
                "some",
                "every",
                "reduce",
                "reduceRight",
            ] {
                for depth in [0, 8, 96] {
                    let mut runtime = Runtime::new();
                    let source = format!(
                        "var caught=false,finalized=false;function recurse(){{return {}[1].{method}(recurse,0);}}try{{recurse();}}catch(e){{caught=true;}}finally{{finalized=true;}}",
                        "+ ".repeat(depth)
                    );
                    let error = runtime.execute(&source, &mut NoIo).unwrap_err();
                    assert!(error.contains("depth"), "{method}/{depth}: {error}");
                    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
                    assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
                    assert_eq!(runtime.execute("42;", &mut NoIo).unwrap_err(), error);
                    counters(&runtime);
                }
            }
            return;
        }
        use std::{
            process::{Child, Command, Stdio},
            time::{Duration, Instant},
        };
        struct OwnedChild(Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = OwnedChild(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "js::runtime::array_callbacks::tests::callback_reentry_and_pending_expressions_stay_bounded_on_default_stack", "--nocapture"])
            .env(CHILD, "1").env("RUST_BACKTRACE", "0").env_remove("RUST_MIN_STACK")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "owned callback-depth child failed: {status}"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned callback-depth child deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Runtime {
    #[inline(never)]
    pub(super) fn array_callback(
        &mut self,
        name: &str,
        this: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let kind = match name {
            "Array.forEach" => CallbackKind::ForEach,
            "Array.map" => CallbackKind::Map,
            "Array.filter" => CallbackKind::Filter,
            "Array.some" => CallbackKind::Some,
            "Array.every" => CallbackKind::Every,
            "Array.reduce" => CallbackKind::Reduce,
            "Array.reduceRight" => CallbackKind::ReduceRight,
            _ => return Err(unsupported(name)),
        };
        if matches!(this, Value::Null | Value::Undefined) {
            return Err(exception(
                "TypeError: Array callback requires a non-null receiver",
            ));
        }
        let receiver = self.boxed(this)?;
        let length = self.get(&receiver, "length", host)?;
        // The existing finite ToInt32 helper computes the same modulo 2^32
        // bits. Reinterpret them as unsigned, not a clamp or apply's floor.
        let length = int32(self.number(length, host)?) as u32 as usize;
        let mut arguments = args.into_iter();
        let callback = arguments.next().unwrap_or(Value::Undefined);
        if !callback.callable() {
            return Err(exception("TypeError: Array callback is not callable"));
        }
        if length > MAX_ARRAY {
            return Err(Fault::Fatal("JavaScript array limit exhausted".into()));
        }
        let second = arguments.next();
        // No argument beyond the optional receiver/seed is copied or coerced.
        if matches!(kind, CallbackKind::Reduce | CallbackKind::ReduceRight) {
            self.array_reduce(
                kind == CallbackKind::ReduceRight,
                receiver,
                length,
                callback,
                second,
                host,
            )
        } else {
            self.array_visit(
                kind,
                receiver,
                length,
                callback,
                second.unwrap_or(Value::Undefined),
                host,
            )
        }
    }

    fn indexed_callback_value(
        &mut self,
        receiver: &Value,
        index: usize,
        host: &mut impl Host,
    ) -> Eval<Option<Value>> {
        self.budget.step()?;
        let key = IndexKey::new(index, &mut self.budget)?;
        let present = if let Value::Host(object) = receiver {
            self.budget.step()?;
            host.has_indexed_property(object, index)
                .map_err(exception)?
        } else {
            self.has_property(receiver, KeyRef::String(key.as_str()), false)?
        };
        if present {
            // Get preserves the original receiver and performs existing actual
            // payload-copy/foreign-symbol/Host-ingress admission exactly once.
            Ok(Some(self.get(receiver, key.as_str(), host)?))
        } else {
            Ok(None)
        }
    }

    #[inline(never)]
    fn array_visit(
        &mut self,
        kind: CallbackKind,
        receiver: Value,
        length: usize,
        callback: Value,
        this_arg: Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let result = if matches!(kind, CallbackKind::Map | CallbackKind::Filter) {
            let empty = PrepaidArray::with_slots(&mut self.budget, 0, AllocationPhase::Runtime)?;
            Some(self.object(Some(self.array_prototype), Some(empty))?)
        } else {
            None
        };
        let mut output = if kind == CallbackKind::Map {
            PrepaidArray::with_slots(&mut self.budget, length, AllocationPhase::Runtime)?
        } else {
            PrepaidArray::growing(AllocationPhase::Runtime)
        };
        for index in 0..length {
            let Some(value) = self.indexed_callback_value(&receiver, index, host)? else {
                if kind == CallbackKind::Map {
                    output.push_owned(&mut self.budget, None)?;
                }
                continue;
            };
            // Filter retains the paid Get result even if the callback changes
            // the source. Its independently owned callback argument is a real,
            // charged copy; rereading after the callback would be incorrect.
            let mut retained = None;
            let called = self.call_array_element(
                &callback,
                &this_arg,
                &receiver,
                value,
                index,
                None,
                if kind == CallbackKind::Filter {
                    Some(&mut retained)
                } else {
                    None
                },
                host,
            )?;
            match kind {
                CallbackKind::Map => {
                    output.push_owned(&mut self.budget, Some(called))?;
                }
                CallbackKind::Filter if called.truthy() => {
                    output.push_owned(&mut self.budget, retained)?;
                }
                CallbackKind::Some if called.truthy() => return Ok(Value::Bool(true)),
                CallbackKind::Every if !called.truthy() => return Ok(Value::Bool(false)),
                _ => {}
            }
        }
        if let Some(result) = result {
            // The result identity was admitted before indexed effects but was
            // never exposed. Adopt paid own slots without prototype Put hooks.
            self.objects[result].array = Some(output.into_values()?);
            Ok(Value::Object(result))
        } else {
            Ok(match kind {
                CallbackKind::Some => Value::Bool(false),
                CallbackKind::Every => Value::Bool(true),
                _ => Value::Undefined,
            })
        }
    }

    #[inline(never)]
    fn array_reduce(
        &mut self,
        reverse: bool,
        receiver: Value,
        length: usize,
        callback: Value,
        mut accumulator: Option<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        for offset in 0..length {
            // offset < length here, including when the captured length is 0.
            let index = if reverse { length - 1 - offset } else { offset };
            let Some(value) = self.indexed_callback_value(&receiver, index, host)? else {
                continue;
            };
            accumulator = Some(if let Some(accumulator) = accumulator {
                self.call_array_element(
                    &callback,
                    &Value::Undefined,
                    &receiver,
                    value,
                    index,
                    Some(accumulator),
                    None,
                    host,
                )?
            } else {
                value
            });
        }
        accumulator
            .ok_or_else(|| exception("TypeError: reduce of empty array without initial value"))
    }

    #[inline(never)]
    fn call_array_element(
        &mut self,
        callback: &Value,
        this_arg: &Value,
        receiver: &Value,
        value: Value,
        index: usize,
        accumulator: Option<Value>,
        retain: Option<&mut Option<Value>>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let count = if accumulator.is_some() { 4 } else { 3 };
        self.budget.allocate(count * 64)?;
        let value = if let Some(retained) = retain {
            let copied = self.copy(&value)?;
            *retained = Some(value);
            copied
        } else {
            value
        };
        let callback = self.copy(callback)?;
        let this_arg = self.copy(this_arg)?;
        let receiver = self.copy(receiver)?;
        let mut arguments = Vec::with_capacity(count);
        if let Some(accumulator) = accumulator {
            arguments.push(accumulator);
        }
        arguments.push(value);
        arguments.push(Value::Number(index as f64));
        arguments.push(receiver);
        self.call(callback, this_arg, arguments, None, host)
    }
}
