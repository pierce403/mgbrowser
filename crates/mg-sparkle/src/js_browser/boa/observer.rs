//! Trusted, default-off capture at the pinned harness's ordinary callback seam.
//! Capture performs no JavaScript, coercion, task admission or checkpoint.
use super::*;
use crate::page_session::observer::{
    HarnessRecord, HarnessStatus, ObserverContract, ObserverFault, ObserverInit, ObserverProgress,
    ObserverRecord, ObserverReply, OptionalUtf16, SubtestStatus, TestRecord, Utf16,
};
use modern::boa_engine::builtins::function::OrdinaryFunction;
use std::mem::size_of;

const FAILURE: &str = "Window harness observer invariant or storage admission failed";
const MAX_IDENTITIES: usize = 256;
const MAX_FIELD: usize = 4096;
const MAX_TEXT: usize = 131_072;
const MEMBERSHIP_BYTES: usize = 256;

#[derive(Trace, Finalize)]
pub(super) struct Capture {
    #[unsafe_ignore_trace]
    run_id: u64,
    #[unsafe_ignore_trace]
    installed: bool,
    #[unsafe_ignore_trace]
    fault: Option<ObserverFault>,
    #[unsafe_ignore_trace]
    execution_failed: bool,
    #[unsafe_ignore_trace]
    emitted: u32,
    #[unsafe_ignore_trace]
    completion: bool,
    #[unsafe_ignore_trace]
    copied_units: usize,
    #[unsafe_ignore_trace]
    identities: Vec<Identity>,
    #[unsafe_ignore_trace]
    pending: Vec<ObserverRecord>,
    // Scalar IDs only, never a value that points back to a key.
    ids: Option<JsWeakMap>,
}

struct Identity {
    index: u16,
    result: Option<TestRecord>,
}

#[derive(Debug)]
enum CaptureError {
    Narrow(ObserverFault),
    Fatal(ObserverFault),
}
type CaptureResult<T> = Result<T, CaptureError>;
fn malformed() -> CaptureError {
    CaptureError::Narrow(ObserverFault::MalformedCapture)
}
fn invariant() -> CaptureError {
    CaptureError::Fatal(ObserverFault::InvariantFailure)
}
fn storage() -> CaptureError {
    CaptureError::Fatal(ObserverFault::StorageAdmission)
}
fn overflow() -> CaptureError {
    CaptureError::Narrow(ObserverFault::CaptureOverflow)
}

pub(super) struct PreparedReply {
    reply: ObserverReply,
    cursor: Option<u32>,
}

pub(super) fn enable(realm: &mut BoaPageRealm, init: ObserverInit) {
    let ObserverInit::WindowHarness { run_id, .. } = init else {
        return;
    };
    let charged =
        realm.state.borrow_mut().host.charge(
            size_of::<Capture>() + 2 * (size_of::<JsObject>() + size_of::<NativeFunction>()),
        );
    realm.state.borrow_mut().observer = Some(Capture {
        run_id,
        installed: false,
        fault: None,
        execution_failed: false,
        emitted: 0,
        completion: false,
        copied_units: 0,
        identities: Vec::new(),
        pending: Vec::new(),
        ids: None,
    });
    if charged.is_err() {
        fail(
            realm.engine.context_mut(),
            CaptureError::Fatal(ObserverFault::StorageAdmission),
        );
    }
}

pub(super) fn mark_execution_failed(shared: &Shared) {
    if let Some(capture) = &mut shared.borrow_mut().observer {
        capture.execution_failed = true;
    }
}

pub(super) fn bootstrap_failed(realm: &mut BoaPageRealm) {
    let fault = if realm.state.borrow().host.navigation.is_some() {
        ObserverFault::NavigationProposed
    } else if realm.engine.is_fatal() {
        ObserverFault::RealmTerminated
    } else {
        ObserverFault::InvalidConfiguration
    };
    discard(&realm.state, fault, true);
    realm.closed = true;
}

pub(super) fn navigation_proposed(shared: &Shared) {
    discard(shared, ObserverFault::NavigationProposed, true);
}

fn discard(shared: &Shared, fault: ObserverFault, execution_failed: bool) {
    let discarded = {
        let mut data = shared.borrow_mut();
        let Some(capture) = &mut data.observer else {
            return;
        };
        capture.fault.get_or_insert(fault);
        capture.execution_failed |= execution_failed;
        (
            capture.ids.take(),
            std::mem::take(&mut capture.identities),
            std::mem::take(&mut capture.pending),
        )
    };
    // No map/temporary root is dropped while State is borrowed.
    drop(discarded);
}

fn fail(context: &mut Context, error: CaptureError) {
    let (fault, fatal) = match error {
        CaptureError::Narrow(fault) => (fault, false),
        CaptureError::Fatal(fault) => (fault, true),
    };
    if let Some(shared) = context.get_data::<Shared>().cloned() {
        discard(&shared, fault, fatal || fault == ObserverFault::PrivacyLost);
    }
    if fatal {
        modern::latch_context(context, FAILURE);
    }
}

fn charge(context: &mut Context, bytes: usize) -> CaptureResult<()> {
    modern::check_context(context).map_err(|_| storage())?;
    let shared = context.get_data::<Shared>().ok_or_else(invariant)?.clone();
    let result = shared.borrow_mut().host.charge(bytes);
    result.map_err(|_| storage())
}

fn reserve<T>(context: &mut Context, values: &mut Vec<T>, extra: usize) -> CaptureResult<()> {
    let shared = context.get_data::<Shared>().ok_or_else(invariant)?.clone();
    let result = reserve_storage(&mut shared.borrow_mut().host, values, extra);
    result.map_err(|_| storage())
}

fn own_data(object: &JsObject, key: PropertyKey) -> CaptureResult<JsValue> {
    let object = object.try_borrow().map_err(|_| invariant())?;
    object
        .properties()
        .get(&key)
        .and_then(|descriptor| descriptor.value().cloned())
        .ok_or_else(malformed)
}

fn number(value: &JsValue, maximum: u16) -> CaptureResult<u16> {
    let value = value.as_number().ok_or_else(malformed)?;
    if !value.is_finite() || value < 0.0 || value > f64::from(maximum) || value.fract() != 0.0 {
        return Err(malformed());
    }
    Ok(value as u16)
}

fn ordinary(value: &JsValue) -> CaptureResult<JsObject> {
    let object = value.as_object().ok_or_else(malformed)?;
    if !object.is_ordinary() {
        return Err(malformed());
    }
    Ok(object)
}

fn array_length(value: &JsValue) -> CaptureResult<(JsObject, u16)> {
    let object = value.as_object().ok_or_else(malformed)?;
    if !object.is_array() {
        return Err(malformed());
    }
    let length = number(&own_data(&object, js_string!("length").into())?, 256)?;
    Ok((object, length))
}

fn copy_units(
    context: &mut Context,
    units: impl ExactSizeIterator<Item = u16>,
) -> CaptureResult<Utf16> {
    let count = units.len();
    if count > MAX_FIELD {
        return Err(overflow());
    }
    let shared = context.get_data::<Shared>().ok_or_else(invariant)?.clone();
    {
        let mut data = shared.borrow_mut();
        let capture = data.observer.as_mut().ok_or_else(invariant)?;
        let total = capture
            .copied_units
            .checked_add(count)
            .ok_or_else(overflow)?;
        if total > MAX_TEXT {
            return Err(overflow());
        }
        capture.copied_units = total;
    }
    let mut output = Vec::new();
    reserve(context, &mut output, count)?;
    output.extend(units);
    Utf16::from_units(output).map_err(|_| invariant())
}

fn string(context: &mut Context, value: &JsValue) -> CaptureResult<Utf16> {
    let value = value.as_string().ok_or_else(malformed)?;
    copy_units(context, value.iter())
}

fn optional(context: &mut Context, value: &JsValue) -> CaptureResult<OptionalUtf16> {
    if value.is_null() {
        return Ok(OptionalUtf16::Null);
    }
    if value.is_undefined() {
        return Ok(OptionalUtf16::Undefined);
    }
    Ok(OptionalUtf16::Text {
        units: string(context, value)?,
    })
}

fn copy_optional(context: &mut Context, value: &OptionalUtf16) -> CaptureResult<OptionalUtf16> {
    Ok(match value {
        OptionalUtf16::Null => OptionalUtf16::Null,
        OptionalUtf16::Undefined => OptionalUtf16::Undefined,
        OptionalUtf16::Text { units } => OptionalUtf16::Text {
            units: copy_units(context, units.as_units().iter().copied())?,
        },
    })
}

fn copy_record(context: &mut Context, value: &TestRecord) -> CaptureResult<TestRecord> {
    Ok(TestRecord {
        id: value.id,
        index: value.index,
        status: value.status,
        name: copy_units(context, value.name.as_units().iter().copied())?,
        message: copy_optional(context, &value.message)?,
        stack: copy_optional(context, &value.stack)?,
    })
}

fn equal_optional(a: &OptionalUtf16, b: &OptionalUtf16) -> bool {
    match (a, b) {
        (OptionalUtf16::Null, OptionalUtf16::Null)
        | (OptionalUtf16::Undefined, OptionalUtf16::Undefined) => true,
        (OptionalUtf16::Text { units: a }, OptionalUtf16::Text { units: b }) => {
            a.as_units() == b.as_units()
        }
        _ => false,
    }
}

// No allocation: at most 4096 input/output units, pinned lower-case HTML-free spelling.
fn reconciled_name(raw: &[u16], final_name: &[u16]) -> bool {
    crate::page_session::observer::reconcile_name(raw, final_name).is_ok()
}

fn capture_guard(context: &mut Context) -> CaptureResult<bool> {
    if modern::context_dynamic_compilation_seen(context).is_none() {
        return Err(CaptureError::Fatal(ObserverFault::AuthorityUnavailable));
    }
    modern::check_context(context).map_err(|_| storage())?;
    let shared = context.get_data::<Shared>().ok_or_else(invariant)?.clone();
    match rejection_exposure(context) {
        Err(()) => return Err(CaptureError::Fatal(ObserverFault::AuthorityUnavailable)),
        Ok(false) => return Err(CaptureError::Narrow(ObserverFault::PrivacyLost)),
        Ok(true) => {}
    }
    let data = shared.borrow();
    let capture = data.observer.as_ref().ok_or_else(invariant)?;
    if capture.fault.is_some() {
        return Ok(false);
    }
    if !capture.installed {
        return Err(CaptureError::Narrow(ObserverFault::InvalidConfiguration));
    }
    Ok(true)
}

fn identity(context: &mut Context, object: &JsObject, index: u16) -> CaptureResult<u16> {
    let shared = context.get_data::<Shared>().ok_or_else(invariant)?.clone();
    let map = shared
        .borrow()
        .observer
        .as_ref()
        .ok_or_else(invariant)?
        .ids
        .clone();
    let map = match map {
        Some(map) => map,
        None => {
            charge(context, size_of::<JsWeakMap>())?;
            let map = JsWeakMap::new(context);
            shared
                .borrow_mut()
                .observer
                .as_mut()
                .ok_or_else(invariant)?
                .ids = Some(map.clone());
            map
        }
    };
    let id = map.get(object, context).map_err(|_| invariant())?;
    if !id.is_undefined() {
        let id = number(&id, 256).map_err(|_| invariant())?;
        let data = shared.borrow();
        let capture = data.observer.as_ref().ok_or_else(invariant)?;
        if id == 0
            || capture
                .identities
                .get(id as usize - 1)
                .is_none_or(|row| row.index != index)
        {
            return Err(invariant());
        }
        return Ok(id);
    }
    let mut data = shared.borrow_mut();
    let State { host, observer, .. } = &mut *data;
    let capture = observer.as_mut().ok_or_else(invariant)?;
    if capture.identities.len() >= MAX_IDENTITIES {
        return Err(overflow());
    }
    if capture.identities.iter().any(|row| row.index == index) {
        return Err(malformed());
    }
    reserve_storage(host, &mut capture.identities, 1).map_err(|_| storage())?;
    host.charge(MEMBERSHIP_BYTES).map_err(|_| storage())?;
    let id = u16::try_from(capture.identities.len() + 1).map_err(|_| invariant())?;
    capture.identities.push(Identity {
        index,
        result: None,
    });
    drop(data);
    map.set(object, id.into(), context)
        .map_err(|_| invariant())?;
    Ok(id)
}

fn test_record(context: &mut Context, object: &JsObject) -> CaptureResult<TestRecord> {
    let index = number(&own_data(object, js_string!("index").into())?, 255)?;
    let status = match number(&own_data(object, js_string!("status").into())?, 4)? {
        0 => SubtestStatus::Pass,
        1 => SubtestStatus::Fail,
        2 => SubtestStatus::Timeout,
        3 => SubtestStatus::NotRun,
        4 => SubtestStatus::PreconditionFailed,
        _ => unreachable!(),
    };
    let name = own_data(object, js_string!("name").into())?;
    let message = own_data(object, js_string!("message").into())?;
    let stack = own_data(object, js_string!("stack").into())?;
    let id = identity(context, object, index)?;
    Ok(TestRecord {
        id,
        index,
        status,
        name: string(context, &name)?,
        message: optional(context, &message)?,
        stack: optional(context, &stack)?,
    })
}

fn append(context: &mut Context, record: ObserverRecord) -> CaptureResult<()> {
    let shared = context.get_data::<Shared>().ok_or_else(invariant)?.clone();
    let mut data = shared.borrow_mut();
    let State { host, observer, .. } = &mut *data;
    let capture = observer.as_mut().ok_or_else(invariant)?;
    let next = usize::try_from(capture.emitted)
        .map_err(|_| invariant())?
        .checked_add(capture.pending.len())
        .and_then(|n| n.checked_add(1))
        .ok_or_else(overflow)?;
    if next > 257 {
        return Err(overflow());
    }
    reserve_storage(host, &mut capture.pending, 1).map_err(|_| storage())?;
    capture.pending.push(record);
    Ok(())
}

fn result_capture(context: &mut Context, args: &[JsValue]) -> CaptureResult<()> {
    let shared = context.get_data::<Shared>().ok_or_else(invariant)?.clone();
    if shared
        .borrow()
        .observer
        .as_ref()
        .ok_or_else(invariant)?
        .completion
    {
        return Err(CaptureError::Narrow(ObserverFault::ResultAfterCompletion));
    }
    let object = ordinary(args.first().ok_or_else(malformed)?)?;
    let record = test_record(context, &object)?;
    if shared
        .borrow()
        .observer
        .as_ref()
        .ok_or_else(invariant)?
        .identities[record.id as usize - 1]
        .result
        .is_some()
    {
        return Err(CaptureError::Narrow(ObserverFault::DuplicateResult));
    }
    let remembered = copy_record(context, &record)?;
    shared
        .borrow_mut()
        .observer
        .as_mut()
        .ok_or_else(invariant)?
        .identities[record.id as usize - 1]
        .result = Some(remembered);
    append(context, ObserverRecord::Result { test: record })
}

fn completion_capture(context: &mut Context, args: &[JsValue]) -> CaptureResult<()> {
    let shared = context.get_data::<Shared>().ok_or_else(invariant)?.clone();
    if shared
        .borrow()
        .observer
        .as_ref()
        .ok_or_else(invariant)?
        .completion
    {
        return Err(CaptureError::Narrow(ObserverFault::DuplicateCompletion));
    }
    let (array, length) = array_length(args.first().ok_or_else(malformed)?)?;
    let status = ordinary(args.get(1).ok_or_else(malformed)?)?;
    let (_, debug_length) = array_length(args.get(2).ok_or_else(malformed)?)?;
    if debug_length != 0 {
        return Err(malformed());
    }
    let status_code = number(&own_data(&status, js_string!("status").into())?, 3)?;
    let message = own_data(&status, js_string!("message").into())?;
    let stack = own_data(&status, js_string!("stack").into())?;
    let harness = HarnessRecord {
        status: match status_code {
            0 => HarnessStatus::Ok,
            1 => HarnessStatus::Error,
            2 => HarnessStatus::Timeout,
            3 => HarnessStatus::PreconditionFailed,
            _ => unreachable!(),
        },
        message: optional(context, &message)?,
        stack: optional(context, &stack)?,
    };
    let mut inventory = Vec::new();
    reserve(context, &mut inventory, length as usize)?;
    for index in 0..length {
        let value = own_data(&array, PropertyKey::from(u32::from(index)))?;
        let object = ordinary(&value)?;
        let row = test_record(context, &object)?;
        if row.index != index {
            return Err(malformed());
        }
        {
            let data = shared.borrow();
            let capture = data.observer.as_ref().ok_or_else(invariant)?;
            match &capture.identities[row.id as usize - 1].result {
                Some(old)
                    if old.index == row.index
                        && old.status == row.status
                        && equal_optional(&old.message, &row.message)
                        && equal_optional(&old.stack, &row.stack)
                        && reconciled_name(old.name.as_units(), row.name.as_units()) => {}
                None if harness.status != HarnessStatus::Ok => {}
                _ => return Err(malformed()),
            }
        }
        inventory.push(row);
    }
    {
        let data = shared.borrow();
        let capture = data.observer.as_ref().ok_or_else(invariant)?;
        if capture.identities.len() != inventory.len() {
            return Err(malformed());
        }
    }
    append(context, ObserverRecord::Completion { harness, inventory })?;
    shared
        .borrow_mut()
        .observer
        .as_mut()
        .ok_or_else(invariant)?
        .completion = true;
    Ok(())
}

fn callback(args: &[JsValue], context: &mut Context, complete: bool) -> JsResult<JsValue> {
    let result = capture_guard(context).and_then(|active| {
        if !active {
            return Ok(());
        }
        if complete {
            completion_capture(context, args)
        } else {
            result_capture(context, args)
        }
    });
    if let Err(error) = result {
        fail(context, error);
    }
    modern::check_context(context)?;
    Ok(JsValue::undefined())
}
fn result_callback(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    callback(args, context, false)
}
fn completion_callback(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    callback(args, context, true)
}

fn stop_install(realm: &mut BoaPageRealm, fault: ObserverFault) -> bool {
    // Preserve the first capture fault. Pending permits and native payload roots
    // are dropped outside State borrows; this is not rollback of executed JS.
    discard(&realm.state, fault, true);
    realm.closed = true;
    realm.clear_work();
    let _ = realm.engine.check();
    false
}

fn install_ready(realm: &mut BoaPageRealm, errors: &mut Vec<String>) -> bool {
    if realm.closed || realm.engine.is_fatal() {
        return stop_install(realm, ObserverFault::RealmTerminated);
    }
    if modern::context_dynamic_compilation_seen(realm.engine.context()).is_none()
        || realm.engine.context().get_data::<Shared>().is_none()
    {
        fail(
            realm.engine.context_mut(),
            CaptureError::Fatal(ObserverFault::AuthorityUnavailable),
        );
        return stop_install(realm, ObserverFault::AuthorityUnavailable);
    }
    if let Err(error) = realm.engine.check() {
        if errors.len() < 64 {
            errors.push(bounded(&error.to_string(), 512));
        }
        return stop_install(realm, ObserverFault::RealmTerminated);
    }
    let (fault, installed, invalid_depth, navigation) = {
        let data = realm.state.borrow();
        let Some(capture) = data.observer.as_ref() else {
            drop(data);
            fail(realm.engine.context_mut(), invariant());
            return stop_install(realm, ObserverFault::InvariantFailure);
        };
        (
            capture.fault,
            capture.installed,
            data.js_depth != 0 || data.checkpointing || data.reporting_error,
            data.host.navigation.is_some(),
        )
    };
    if let Some(fault) = fault {
        return stop_install(realm, fault);
    }
    if invalid_depth {
        fail(realm.engine.context_mut(), invariant());
        return stop_install(realm, ObserverFault::InvariantFailure);
    }
    if installed {
        return stop_install(realm, ObserverFault::InvalidConfiguration);
    }
    if navigation {
        return stop_install(realm, ObserverFault::NavigationProposed);
    }
    match rejection_exposure(realm.engine.context()) {
        Ok(true) => {}
        Ok(false) => return stop_install(realm, ObserverFault::PrivacyLost),
        Err(()) => {
            fail(
                realm.engine.context_mut(),
                CaptureError::Fatal(ObserverFault::AuthorityUnavailable),
            );
            return stop_install(realm, ObserverFault::AuthorityUnavailable);
        }
    }
    if !errors.is_empty() {
        return stop_install(realm, ObserverFault::InvalidConfiguration);
    }
    true
}

pub(super) fn install(
    realm: &mut BoaPageRealm,
    errors: &mut Vec<String>,
    contract: ObserverContract,
) -> bool {
    if realm.state.borrow().observer.is_none() {
        return true;
    }
    if !install_ready(realm, errors) {
        return false;
    }
    let preparation = (|| {
        let global = realm.engine.context().global_object();
        let result = own_data(&global, js_string!("add_result_callback").into())?
            .as_object()
            .ok_or_else(malformed)?;
        let complete = own_data(&global, js_string!("add_completion_callback").into())?
            .as_object()
            .ok_or_else(malformed)?;
        // V1 deliberately never reads setup, even if it is a hostile getter.
        let setup = match contract {
            ObserverContract::PinnedWindowHarnessV1 => None,
            ObserverContract::PinnedWindowHarnessOutputDisabledV1 => Some(
                own_data(&global, js_string!("setup").into())?
                    .as_object()
                    .ok_or_else(malformed)?,
            ),
        };
        if !result.is::<OrdinaryFunction>()
            || !complete.is::<OrdinaryFunction>()
            || setup
                .as_ref()
                .is_some_and(|setup| !setup.is::<OrdinaryFunction>())
        {
            return Err(malformed());
        }
        Ok((global, result, complete, setup))
    })();
    let (global, result, complete, setup) = match preparation {
        Ok(values) => values,
        Err(error) => {
            fail(realm.engine.context_mut(), error);
            return stop_install(realm, ObserverFault::InvalidConfiguration);
        }
    };
    let result_native = FunctionObjectBuilder::new(
        realm.engine.context().realm(),
        NativeFunction::from_fn_ptr(result_callback),
    )
    .build();
    if !install_ready(realm, errors) {
        return false;
    }
    let complete_native = FunctionObjectBuilder::new(
        realm.engine.context().realm(),
        NativeFunction::from_fn_ptr(completion_callback),
    )
    .build();
    if !install_ready(realm, errors) {
        return false;
    }
    for (registration, native) in [(result, result_native), (complete, complete_native)] {
        if let Err(error) =
            realm.call_function(&registration, &global.clone().into(), &[native.into()])
        {
            realm.execution_failure("Observer registration", error, errors);
        }
        if !install_ready(realm, errors) {
            return false;
        }
    }
    if let Some(setup) = setup {
        // Fresh intrinsic ordinary object; no global Object lookup, setter,
        // source evaluation or caller-supplied property bag. Actual Boa storage
        // is admitted by the existing System policy, not a new native pool.
        let properties = JsObject::with_object_proto(realm.engine.context().intrinsics());
        if !install_ready(realm, errors) {
            return false;
        }
        let created = properties.create_data_property_or_throw(
            js_string!("output"),
            false,
            realm.engine.context_mut(),
        );
        if !created.is_ok_and(|created| created) {
            fail(realm.engine.context_mut(), invariant());
            return stop_install(realm, ObserverFault::InvariantFailure);
        }
        if !install_ready(realm, errors) {
            return false;
        }
        if let Err(error) =
            realm.call_function(&setup, &global.clone().into(), &[properties.into()])
        {
            realm.execution_failure("Observer configuration", error, errors);
        }
        if !install_ready(realm, errors) {
            return false;
        }
    }
    realm.checkpoint(errors);
    // A queued native capture can fail without throwing. Check the actual
    // sticky fault AFTER the outer checkpoint, before installed or next source.
    if !install_ready(realm, errors) {
        return false;
    }
    realm
        .state
        .borrow_mut()
        .observer
        .as_mut()
        .unwrap()
        .installed = true;
    true
}

fn unsettled(shared: &Shared) -> CaptureResult<bool> {
    let data = shared.borrow();
    // Canceled IDs can survive in a partially live batch until normal pruning.
    // Every actually queued nonempty batch remains an obligation; only native
    // outstanding weak membership is excluded from task settlement.
    for (task_index, task) in data.dom_tasks.iter().enumerate() {
        if let DomTaskKind::Batch(ids) = &task.kind {
            for (index, id) in ids.iter().enumerate() {
                if *id == 0 || *id > data.next_candidate || ids[..index].contains(id)
                    || data.dom_tasks[..task_index].iter().any(|previous| {
                        matches!(&previous.kind, DomTaskKind::Batch(previous) if previous.contains(id))
                    })
                {
                    return Err(invariant());
                }
            }
        }
    }
    for (index, candidate) in data.rejections.iter().enumerate() {
        if data.rejections[..index]
            .iter()
            .any(|old| old.id == candidate.id)
        {
            return Err(invariant());
        }
        if candidate.phase == CandidatePhase::Frozen {
            let count = data
                .dom_tasks
                .iter()
                .filter(|task| match &task.kind {
                    DomTaskKind::Batch(ids) => {
                        ids.iter().filter(|id| **id == candidate.id).count() == 1
                    }
                    _ => false,
                })
                .count();
            if count != 1 {
                return Err(invariant());
            }
        }
    }
    Ok(!data.rejections.is_empty()
        || data.dom_tasks.iter().any(|task| match &task.kind {
            DomTaskKind::Handled { .. } => true,
            DomTaskKind::Batch(ids) => !ids.is_empty(),
            _ => false,
        }))
}

pub(super) fn prepare_reply(realm: &mut BoaPageRealm, reply: &mut SessionReply) -> PreparedReply {
    if realm.state.borrow().observer.is_none() {
        return PreparedReply {
            reply: ObserverReply::Disabled,
            cursor: None,
        };
    }
    if realm.closed || realm.engine.is_fatal() || reply.state != RealmState::Ready {
        discard(&realm.state, ObserverFault::RealmTerminated, true);
    } else if reply.navigation.is_some() || realm.state.borrow().host.navigation.is_some() {
        discard(&realm.state, ObserverFault::NavigationProposed, true);
    }
    match rejection_exposure(realm.engine.context()) {
        Ok(false) => discard(&realm.state, ObserverFault::PrivacyLost, true),
        Err(()) => fail(
            realm.engine.context_mut(),
            CaptureError::Fatal(ObserverFault::AuthorityUnavailable),
        ),
        Ok(true) => {}
    }
    let clean_boundary = {
        let data = realm.state.borrow();
        data.js_depth == 0 && !data.checkpointing && !data.reporting_error
    };
    if !clean_boundary {
        fail(realm.engine.context_mut(), invariant());
    }
    if realm.state.borrow().rejection_observation_gap {
        mark_execution_failed(&realm.state);
    }
    let settling = match unsettled(&realm.state) {
        Ok(value) => value,
        Err(error) => {
            fail(realm.engine.context_mut(), error);
            false
        }
    };
    let valid_publication = {
        let data = realm.state.borrow();
        let capture = data.observer.as_ref().unwrap();
        let next = u32::try_from(capture.pending.len())
            .ok()
            .and_then(|length| capture.emitted.checked_add(length));
        next.is_some_and(|next| next <= 257)
            && !(capture.completion && settling && reply.task_wake.is_none())
    };
    if !valid_publication {
        fail(realm.engine.context_mut(), invariant());
    }
    let mut data = realm.state.borrow_mut();
    let capture = data.observer.as_mut().unwrap();
    if !capture.installed && capture.fault.is_none() {
        capture.fault = Some(ObserverFault::InvalidConfiguration);
    }
    if let Some(fault) = capture.fault {
        return PreparedReply {
            reply: ObserverReply::ObservationFailed {
                run_id: capture.run_id,
                prefix_end: capture.emitted,
                fault,
                execution_failed: capture.execution_failed,
            },
            cursor: None,
        };
    }
    let first_ordinal = capture.emitted;
    // Validated above, before borrowing State for the nonallocating move.
    let next_ordinal = first_ordinal
        .checked_add(capture.pending.len() as u32)
        .unwrap();
    let progress = if !capture.completion {
        ObserverProgress::Waiting
    } else if settling {
        ObserverProgress::Settling
    } else {
        ObserverProgress::Finished
    };
    PreparedReply {
        reply: ObserverReply::WindowHarness {
            run_id: capture.run_id,
            first_ordinal,
            next_ordinal,
            records: std::mem::take(&mut capture.pending),
            progress,
            execution_failed: capture.execution_failed,
        },
        cursor: Some(next_ordinal),
    }
}

pub(super) fn commit_reply(realm: &mut BoaPageRealm, prepared: PreparedReply) -> ObserverReply {
    if realm.state.borrow().observer.is_none() {
        return prepared.reply;
    }
    if realm.closed || realm.engine.is_fatal() {
        discard(&realm.state, ObserverFault::RealmTerminated, true);
    }
    match rejection_exposure(realm.engine.context()) {
        Ok(false) => discard(&realm.state, ObserverFault::PrivacyLost, true),
        Err(()) => fail(
            realm.engine.context_mut(),
            CaptureError::Fatal(ObserverFault::AuthorityUnavailable),
        ),
        Ok(true) => {}
    }
    let valid = {
        let data = realm.state.borrow();
        let capture = data.observer.as_ref().unwrap();
        data.js_depth == 0
            && !data.checkpointing
            && !data.reporting_error
            && match &prepared.reply {
                ObserverReply::WindowHarness { first_ordinal, .. } => {
                    *first_ordinal == capture.emitted
                }
                _ => true,
            }
    };
    if !valid {
        fail(realm.engine.context_mut(), invariant());
    }
    {
        let data = realm.state.borrow();
        let capture = data.observer.as_ref().unwrap();
        if let Some(fault) = capture.fault {
            // Dropping prepared moves no cursor and releases its text; no new
            // allocation is needed to replace it with this scalar envelope.
            return ObserverReply::ObservationFailed {
                run_id: capture.run_id,
                prefix_end: capture.emitted,
                fault,
                execution_failed: capture.execution_failed,
            };
        }
    }
    if let Some(cursor) = prepared.cursor {
        if let Some(capture) = realm.state.borrow_mut().observer.as_mut() {
            capture.emitted = cursor;
        }
    }
    prepared.reply
}

pub(super) fn terminal_roots(shared: &Shared) {
    discard(shared, ObserverFault::RealmTerminated, true);
}

#[cfg(test)]
mod output_profile_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page_session::observer::ObserverContract;

    fn realm() -> BoaPageRealm {
        // Private extraction tests are post-load binding controls, not a fake
        // harness or evidence that the public control inventory completed.
        let (realm, mut reply) = BoaPageRealm::start(Request {
            url: "https://observer.example.test/private".into(),
            html: "<p>owned</p>".into(),
            external_scripts: None,
        });
        let mut realm = realm.unwrap();
        for _ in 0..2 {
            let wake = reply.task_wake.unwrap();
            assert_eq!(wake.delay_ms, 0);
            reply = realm.pump_tasks(TaskPump {
                wake_token: wake.token,
            });
        }
        assert_eq!(reply.lifecycle.unwrap().phase, LifecyclePhase::LoadComplete);
        let stats = realm.engine.stats();
        assert_eq!(
            (stats.jobs_admitted, stats.jobs_executed, stats.pending_jobs),
            (2, 2, 0)
        );
        enable(
            &mut realm,
            ObserverInit::WindowHarness {
                contract: ObserverContract::PinnedWindowHarnessV1,
                run_id: 7,
                bootstrap_node: 1,
            },
        );
        realm
            .state
            .borrow_mut()
            .observer
            .as_mut()
            .unwrap()
            .installed = true;
        realm
    }
    fn evaluate(realm: &mut BoaPageRealm, source: &str) -> JsValue {
        let _ingress = Ingress::enter(realm.state.clone());
        realm.engine.evaluate_raw(source).unwrap()
    }
    fn capture(realm: &mut BoaPageRealm, source: &str) {
        let value = evaluate(realm, source);
        result_callback(&JsValue::undefined(), &[value], realm.engine.context_mut()).unwrap();
    }
    fn fault(realm: &BoaPageRealm) -> Option<ObserverFault> {
        realm.state.borrow().observer.as_ref().unwrap().fault
    }

    #[test]
    fn own_data_extraction_rejects_getters_and_proxy_without_any_trap_or_coercion() {
        for source in [
            "globalThis.hits=0;({index:0,status:0,get name(){hits++;return 'bad'},message:null,stack:null})",
            "globalThis.hits=0;new Proxy({index:0,status:0,name:'bad',message:null,stack:null},{getOwnPropertyDescriptor(){hits++;throw 1},get(){hits++;throw 2}})",
            "globalThis.hits=0;({index:0,status:0,name:{toString(){hits++;return 'bad'}},message:null,stack:null})",
        ] {
            let mut realm = realm();
            capture(&mut realm, source);
            assert_eq!(fault(&realm), Some(ObserverFault::MalformedCapture));
            assert_eq!(evaluate(&mut realm, "hits").as_number(), Some(0.0));
            assert!(!realm.engine.is_fatal());
            let data = realm.state.borrow();
            let capture = data.observer.as_ref().unwrap();
            assert!(
                capture.pending.is_empty()
                    && capture.identities.is_empty()
                    && capture.ids.is_none()
            );
        }
    }

    #[test]
    fn optional_undefined_is_real_data_and_not_a_missing_descriptor() {
        let mut realm = realm();
        capture(
            &mut realm,
            "({index:0,status:0,name:'owned',message:undefined,stack:null})",
        );
        assert!(fault(&realm).is_none());
        {
            let data = realm.state.borrow();
            let ObserverRecord::Result { test } = &data.observer.as_ref().unwrap().pending[0]
            else {
                panic!()
            };
            assert!(matches!(test.message, OptionalUtf16::Undefined));
            assert!(matches!(test.stack, OptionalUtf16::Null));
        }
        let mut missing = self::realm();
        capture(&mut missing, "({index:0,status:0,name:'owned',stack:null})");
        assert_eq!(fault(&missing), Some(ObserverFault::MalformedCapture));
    }

    #[test]
    fn duplicate_result_discards_only_unemitted_batch_and_preserves_previous_prefix() {
        let mut realm = realm();
        let value = evaluate(
            &mut realm,
            "globalThis.t={index:0,status:0,name:'one',message:null,stack:null};t",
        );
        result_callback(
            &JsValue::undefined(),
            &[value.clone()],
            realm.engine.context_mut(),
        )
        .unwrap();
        let mut reply = BoaPageRealm::empty_reply();
        reply.state = RealmState::Ready;
        let prepared = prepare_reply(&mut realm, &mut reply);
        let first = commit_reply(&mut realm, prepared);
        assert!(matches!(
            first,
            ObserverReply::WindowHarness {
                first_ordinal: 0,
                next_ordinal: 1,
                ..
            }
        ));
        result_callback(&JsValue::undefined(), &[value], realm.engine.context_mut()).unwrap();
        assert_eq!(fault(&realm), Some(ObserverFault::DuplicateResult));
        let prepared = prepare_reply(&mut realm, &mut reply);
        assert!(matches!(
            commit_reply(&mut realm, prepared),
            ObserverReply::ObservationFailed {
                prefix_end: 1,
                fault: ObserverFault::DuplicateResult,
                ..
            }
        ));
    }

    #[test]
    fn exact_field_and_cumulative_copy_caps_do_not_truncate_or_refund() {
        let mut realm = realm();
        capture(
            &mut realm,
            "({index:0,status:0,name:'x'.repeat(4096),message:null,stack:null})",
        );
        assert!(fault(&realm).is_none());
        assert_eq!(
            realm.state.borrow().observer.as_ref().unwrap().copied_units,
            8192
        );
        let mut large = self::realm();
        capture(
            &mut large,
            "({index:0,status:0,name:'x'.repeat(4097),message:null,stack:null})",
        );
        assert_eq!(fault(&large), Some(ObserverFault::CaptureOverflow));
        assert!(!large.engine.is_fatal());
        let mut total = self::realm();
        total
            .state
            .borrow_mut()
            .observer
            .as_mut()
            .unwrap()
            .copied_units = MAX_TEXT - 2;
        capture(
            &mut total,
            "({index:0,status:0,name:'x',message:null,stack:null})",
        );
        assert!(fault(&total).is_none());
        assert_eq!(
            total.state.borrow().observer.as_ref().unwrap().copied_units,
            MAX_TEXT
        );
        capture(
            &mut total,
            "({index:1,status:0,name:'x',message:null,stack:null})",
        );
        assert_eq!(fault(&total), Some(ObserverFault::CaptureOverflow));
        assert_eq!(
            total.state.borrow().observer.as_ref().unwrap().copied_units,
            MAX_TEXT
        );
    }

    #[test]
    fn existing_host_admission_failure_is_fatal_and_clears_binding_payloads() {
        let mut realm = realm();
        let value = evaluate(
            &mut realm,
            "({index:0,status:0,name:'owned',message:null,stack:null})",
        );
        realm.state.borrow_mut().host.allocated = super::super::super::MAX_DOM_BYTES;
        assert!(
            result_callback(&JsValue::undefined(), &[value], realm.engine.context_mut()).is_err()
        );
        assert!(realm.engine.is_fatal());
        assert_eq!(fault(&realm), Some(ObserverFault::StorageAdmission));
        let data = realm.state.borrow();
        let capture = data.observer.as_ref().unwrap();
        assert!(
            capture.pending.is_empty() && capture.identities.is_empty() && capture.ids.is_none()
        );
        assert_eq!(realm.engine.stats().pending_jobs, 0);
    }

    #[test]
    fn native_map_scalar_corruption_is_fatal_not_catchable_capture_failure() {
        let mut realm = realm();
        let value = evaluate(
            &mut realm,
            "({index:0,status:0,name:'owned',message:null,stack:null})",
        );
        result_callback(
            &JsValue::undefined(),
            &[value.clone()],
            realm.engine.context_mut(),
        )
        .unwrap();
        let map = realm
            .state
            .borrow()
            .observer
            .as_ref()
            .unwrap()
            .ids
            .clone()
            .unwrap();
        map.set(
            &value.as_object().unwrap(),
            257.into(),
            realm.engine.context_mut(),
        )
        .unwrap();
        assert!(
            result_callback(&JsValue::undefined(), &[value], realm.engine.context_mut()).is_err()
        );
        assert!(realm.engine.is_fatal());
        assert_eq!(fault(&realm), Some(ObserverFault::InvariantFailure));
    }

    #[test]
    fn native_identity_membership_does_not_strongly_retain_test_object() {
        let mut realm = realm();
        capture(
            &mut realm,
            "(()=>{let t={index:0,status:0,name:'owned',message:null,stack:null};globalThis.weak=new WeakRef(t);return t})()",
        );
        realm.checkpoint(&mut Vec::new());
        modern::boa_gc::force_collect();
        assert_eq!(
            evaluate(&mut realm, "weak.deref()===undefined").as_boolean(),
            Some(true)
        );
        assert_eq!(
            realm
                .state
                .borrow()
                .observer
                .as_ref()
                .unwrap()
                .identities
                .len(),
            1
        );
    }

    #[test]
    fn dynamic_floor_loss_discards_pending_text_and_identity_roots_stickily() {
        let mut realm = realm();
        capture(
            &mut realm,
            "({index:0,status:0,name:'private',message:null,stack:null})",
        );
        evaluate(&mut realm, "eval('1')");
        let mut reply = BoaPageRealm::empty_reply();
        reply.state = RealmState::Ready;
        let prepared = prepare_reply(&mut realm, &mut reply);
        assert!(matches!(
            commit_reply(&mut realm, prepared),
            ObserverReply::ObservationFailed {
                fault: ObserverFault::PrivacyLost,
                prefix_end: 0,
                execution_failed: true,
                ..
            }
        ));
        let data = realm.state.borrow();
        let capture = data.observer.as_ref().unwrap();
        assert!(
            capture.pending.is_empty() && capture.identities.is_empty() && capture.ids.is_none()
        );
    }

    #[test]
    fn terminal_cleanup_drops_observer_roots_without_claiming_vm_teardown() {
        let mut realm = realm();
        capture(
            &mut realm,
            "({index:0,status:0,name:'owned',message:null,stack:null})",
        );
        realm.closed = true;
        realm.clear_work();
        assert_eq!(fault(&realm), Some(ObserverFault::RealmTerminated));
        let data = realm.state.borrow();
        let capture = data.observer.as_ref().unwrap();
        assert!(
            capture.pending.is_empty() && capture.identities.is_empty() && capture.ids.is_none()
        );
        assert_eq!(realm.engine.stats().pending_jobs, 0);
    }

    #[test]
    fn prepared_payload_cannot_survive_terminal_privacy_or_cursor_failure_at_commit() {
        for change in 0..3 {
            let mut realm = realm();
            capture(
                &mut realm,
                "({index:0,status:0,name:'private',message:null,stack:null})",
            );
            let mut reply = BoaPageRealm::empty_reply();
            reply.state = RealmState::Ready;
            let prepared = prepare_reply(&mut realm, &mut reply);
            match change {
                0 => {
                    realm.closed = true;
                    realm.clear_work();
                }
                1 => {
                    evaluate(&mut realm, "eval('1')");
                }
                _ => realm.state.borrow_mut().observer.as_mut().unwrap().emitted = 1,
            }
            let value = commit_reply(&mut realm, prepared);
            assert!(matches!(value, ObserverReply::ObservationFailed { .. }));
            assert_eq!(
                realm.state.borrow().observer.as_ref().unwrap().emitted,
                usize::from(change == 2) as u32
            );
            if change == 2 {
                assert!(realm.engine.is_fatal());
            }
            let data = realm.state.borrow();
            let capture = data.observer.as_ref().unwrap();
            assert!(
                capture.pending.is_empty()
                    && capture.identities.is_empty()
                    && capture.ids.is_none()
            );
        }
    }

    #[test]
    fn queued_nonempty_batch_is_obligation_even_when_canceled_ids_remain() {
        let realm = realm();
        {
            let mut data = realm.state.borrow_mut();
            data.next_candidate = 1;
            data.dom_tasks.push(DomTask {
                handle: 1,
                deadline: Instant::now(),
                kind: DomTaskKind::Batch(vec![1]),
            });
        }
        assert!(unsettled(&realm.state).unwrap());
        {
            let mut data = realm.state.borrow_mut();
            let DomTaskKind::Batch(ids) = &mut data.dom_tasks[0].kind else {
                panic!()
            };
            ids.push(1);
        }
        assert!(unsettled(&realm.state).is_err());
        realm.state.borrow_mut().dom_tasks.clear();
        assert!(!unsettled(&realm.state).unwrap());
    }

    #[test]
    fn invalid_ordinal_and_settling_without_wake_latch_fatal_before_publication() {
        for missing_wake in [false, true] {
            let mut realm = realm();
            capture(
                &mut realm,
                "({index:0,status:0,name:'owned',message:null,stack:null})",
            );
            {
                let mut data = realm.state.borrow_mut();
                if missing_wake {
                    data.observer.as_mut().unwrap().completion = true;
                    data.next_candidate = 1;
                    data.dom_tasks.push(DomTask {
                        handle: 1,
                        deadline: Instant::now(),
                        kind: DomTaskKind::Batch(vec![1]),
                    });
                } else {
                    data.observer.as_mut().unwrap().emitted = u32::MAX;
                }
            }
            let mut reply = BoaPageRealm::empty_reply();
            reply.state = RealmState::Ready;
            realm.finish_observer(&mut reply);
            assert_eq!(reply.state, RealmState::Fatal);
            assert_eq!(fault(&realm), Some(ObserverFault::InvariantFailure));
            assert!(matches!(
                reply.observer,
                Some(ObserverReply::ObservationFailed {
                    fault: ObserverFault::InvariantFailure,
                    ..
                })
            ));
            let data = realm.state.borrow();
            let capture = data.observer.as_ref().unwrap();
            assert!(
                capture.pending.is_empty()
                    && capture.identities.is_empty()
                    && capture.ids.is_none()
            );
            assert!(data.dom_tasks.is_empty());
            assert_eq!(realm.engine.stats().pending_jobs, 0);
        }
    }

    #[test]
    fn outside_ingress_fatal_cancels_pending_vm_jobs_and_preserves_first_failure() {
        let mut realm = realm();
        evaluate(
            &mut realm,
            "globalThis.reporterRan=false;Promise.resolve().then(()=>{reporterRan=true})",
        );
        assert_eq!(realm.engine.stats().pending_jobs, 1);
        realm.state.borrow_mut().observer.as_mut().unwrap().emitted = u32::MAX;
        let mut reply = BoaPageRealm::empty_reply();
        reply.state = RealmState::Ready;
        realm.finish_observer(&mut reply);
        assert_eq!(reply.state, RealmState::Fatal);
        assert_eq!(realm.engine.stats().pending_jobs, 0);
        let first = realm.engine.stats().fatal_reason.unwrap();
        realm
            .engine
            .latch_failure("must not replace the first failure");
        assert_eq!(
            realm.engine.stats().fatal_reason.as_deref(),
            Some(first.as_str())
        );
        assert_eq!(
            own_data(
                &realm.engine.context().global_object(),
                js_string!("reporterRan").into()
            )
            .unwrap()
            .as_boolean(),
            Some(false)
        );
        assert!(realm.engine.evaluate_raw("reporterRan=true").is_err());
    }

    fn result_value(realm: &mut BoaPageRealm, value: JsValue) {
        result_callback(&JsValue::undefined(), &[value], realm.engine.context_mut()).unwrap();
    }
    fn complete_value(realm: &mut BoaPageRealm, values: JsValue, status: u8, debug: JsValue) {
        let harness = evaluate(
            realm,
            &format!("({{status:{status},message:null,stack:undefined}})"),
        );
        completion_callback(
            &JsValue::undefined(),
            &[values, harness, debug],
            realm.engine.context_mut(),
        )
        .unwrap();
    }
    fn rows(realm: &mut BoaPageRealm, count: usize, units: usize) -> JsValue {
        evaluate(
            realm,
            &format!(
                "globalThis.rows=[];for(let i=0;i<{count};i++)rows.push({{index:i,status:0,name:'x'.repeat({units}),message:null,stack:null}});rows"
            ),
        )
    }
    fn capture_rows(realm: &mut BoaPageRealm, values: &JsValue) {
        let (array, length) = array_length(values).unwrap();
        for index in 0..length {
            result_value(
                realm,
                own_data(&array, PropertyKey::from(u32::from(index))).unwrap(),
            );
        }
    }

    #[test]
    fn actual_256_identities_and_257_ordinals_are_inclusive_without_new_work() {
        use crate::page_session::observer::{ObserverExpectation, ObserverLedger};
        let mut realm = realm();
        let values = rows(&mut realm, 256, 1);
        capture_rows(&mut realm, &values);
        assert!(fault(&realm).is_none());
        let debug = evaluate(&mut realm, "[]");
        complete_value(&mut realm, values, 0, debug);
        assert!(fault(&realm).is_none());
        let mut reply = BoaPageRealm::empty_reply();
        realm.snapshot(&mut reply);
        let expected = ObserverExpectation::WindowHarness { run_id: 7 };
        reply.validate_observer_frame(expected).unwrap();
        let Some(ObserverReply::WindowHarness {
            records,
            next_ordinal,
            ..
        }) = &reply.observer
        else {
            panic!()
        };
        assert_eq!(*next_ordinal, 257);
        assert_eq!((records.len(), records.capacity()), (257, 257));
        let mut ledger = ObserverLedger::new(expected).unwrap();
        ledger
            .stage(reply.observer.take().unwrap())
            .unwrap()
            .commit();
        assert_eq!(ledger.observation().records.len(), 257);
        assert_eq!(ledger.observation().reconciliations.len(), 256);
        let stats = realm.engine.stats();
        assert_eq!(
            (stats.jobs_admitted, stats.jobs_executed, stats.pending_jobs),
            (2, 2, 0)
        );

        let mut overflow = self::realm();
        let values = rows(&mut overflow, 256, 1);
        capture_rows(&mut overflow, &values);
        assert!(fault(&overflow).is_none());
        capture(
            &mut overflow,
            "({index:255,status:0,name:'257th distinct object',message:null,stack:null})",
        );
        assert_eq!(fault(&overflow), Some(ObserverFault::CaptureOverflow));
        assert!(!overflow.engine.is_fatal());
        let data = overflow.state.borrow();
        let capture = data.observer.as_ref().unwrap();
        assert!(
            capture.ids.is_none() && capture.pending.is_empty() && capture.identities.is_empty()
        );
    }

    #[test]
    fn actual_copied_units_reach_131072_and_next_unit_fails_without_credit() {
        let mut realm = realm();
        // 128 names *512 units *2 real retained copies (canonical and Result).
        // A final inventory would need more credit; this is not a fabricated
        // complete-file positive at a budget that cannot fit its final copies.
        let values = rows(&mut realm, 128, 512);
        capture_rows(&mut realm, &values);
        assert!(fault(&realm).is_none());
        assert_eq!(
            realm.state.borrow().observer.as_ref().unwrap().copied_units,
            131072
        );
        assert!(realm.state.borrow().host.allocated < super::super::super::MAX_DOM_BYTES);
        capture(
            &mut realm,
            "({index:128,status:0,name:'x',message:null,stack:null})",
        );
        assert_eq!(fault(&realm), Some(ObserverFault::CaptureOverflow));
        assert_eq!(
            realm.state.borrow().observer.as_ref().unwrap().copied_units,
            131072
        );
        assert!(!realm.engine.is_fatal());
        let stats = realm.engine.stats();
        assert_eq!(
            (stats.jobs_admitted, stats.jobs_executed, stats.pending_jobs),
            (2, 2, 0)
        );
    }

    #[test]
    fn registration_reads_only_own_data_ordinary_functions_and_partial_install_closes() {
        for source in [
            "globalThis.hits=0;Object.defineProperty(globalThis,'add_result_callback',{get(){hits++;throw 1},configurable:true});globalThis.add_completion_callback=function(){hits++}",
            "globalThis.hits=0;globalThis.add_result_callback=function(){hits++};Object.defineProperty(globalThis,'add_completion_callback',{get(){hits++;throw 1},configurable:true})",
            "globalThis.hits=0;globalThis.add_result_callback=new Proxy(function(){hits++},{apply(){hits++;throw 1}});globalThis.add_completion_callback=function(){hits++}",
            "globalThis.hits=0;globalThis.add_result_callback=(function(){hits++}).bind(null);globalThis.add_completion_callback=function(){hits++}",
        ] {
            let mut realm = realm();
            realm
                .state
                .borrow_mut()
                .observer
                .as_mut()
                .unwrap()
                .installed = false;
            evaluate(&mut realm, source);
            assert!(!install(
                &mut realm,
                &mut Vec::new(),
                ObserverContract::PinnedWindowHarnessV1,
            ));
            assert!(realm.closed && fault(&realm).is_some());
            assert_eq!(
                own_data(
                    &realm.engine.context().global_object(),
                    js_string!("hits").into()
                )
                .unwrap()
                .as_number(),
                Some(0.0)
            );
            assert_eq!(realm.engine.stats().pending_jobs, 0);
        }
        let mut realm = realm();
        realm
            .state
            .borrow_mut()
            .observer
            .as_mut()
            .unwrap()
            .installed = false;
        evaluate(
            &mut realm,
            "globalThis.hits=0;globalThis.late=false;globalThis.add_result_callback=function(cb){hits++;Promise.resolve().then(()=>{late=true})};globalThis.add_completion_callback=function(){hits++;throw new Error('owned partial install')}",
        );
        let mut errors = Vec::new();
        assert!(!install(
            &mut realm,
            &mut errors,
            ObserverContract::PinnedWindowHarnessV1,
        ));
        assert_eq!(fault(&realm), Some(ObserverFault::InvalidConfiguration));
        assert!(!errors.is_empty());
        let mut reply = BoaPageRealm::empty_reply();
        reply.errors = errors;
        realm.snapshot(&mut reply);
        assert_eq!(reply.state, RealmState::Closed);
        assert_eq!(realm.engine.stats().pending_jobs, 0);
        assert_eq!(
            own_data(
                &realm.engine.context().global_object(),
                js_string!("hits").into()
            )
            .unwrap()
            .as_number(),
            Some(2.0)
        );
        assert_eq!(
            own_data(
                &realm.engine.context().global_object(),
                js_string!("late").into()
            )
            .unwrap()
            .as_boolean(),
            Some(false)
        );
    }

    #[test]
    fn duplicate_completion_and_result_after_completion_are_distinct_sticky_faults() {
        for duplicate in [false, true] {
            let mut realm = realm();
            let values = rows(&mut realm, 1, 1);
            capture_rows(&mut realm, &values);
            let debug = evaluate(&mut realm, "[]");
            complete_value(&mut realm, values.clone(), 0, debug);
            assert!(fault(&realm).is_none());
            if duplicate {
                let debug = evaluate(&mut realm, "[]");
                complete_value(&mut realm, values, 0, debug);
            } else {
                let (array, _) = array_length(&values).unwrap();
                result_value(
                    &mut realm,
                    own_data(&array, PropertyKey::from(0u32)).unwrap(),
                );
            }
            assert_eq!(
                fault(&realm),
                Some(if duplicate {
                    ObserverFault::DuplicateCompletion
                } else {
                    ObserverFault::ResultAfterCompletion
                })
            );
            let data = realm.state.borrow();
            let capture = data.observer.as_ref().unwrap();
            assert!(
                capture.pending.is_empty()
                    && capture.identities.is_empty()
                    && capture.ids.is_none()
            );
            assert_eq!(capture.emitted, 0);
            assert!(!realm.engine.is_fatal());
        }
    }

    #[test]
    fn nominal_ok_requires_results_but_non_ok_preserves_every_real_final_only_status() {
        use crate::page_session::observer::{
            NameReconciliation, ObserverExpectation, ObserverLedger,
        };
        let mut ok = realm();
        let values = rows(&mut ok, 1, 1);
        let debug = evaluate(&mut ok, "[]");
        complete_value(&mut ok, values, 0, debug);
        assert_eq!(fault(&ok), Some(ObserverFault::MalformedCapture));
        for (status, expected) in [
            SubtestStatus::Pass,
            SubtestStatus::Fail,
            SubtestStatus::Timeout,
            SubtestStatus::NotRun,
            SubtestStatus::PreconditionFailed,
        ]
        .into_iter()
        .enumerate()
        {
            let mut realm = realm();
            let values = evaluate(
                &mut realm,
                &format!(
                    "[{{index:0,status:{status},name:'final only',message:null,stack:undefined}}]"
                ),
            );
            let debug = evaluate(&mut realm, "[]");
            complete_value(&mut realm, values, 1, debug);
            assert!(fault(&realm).is_none());
            let mut reply = BoaPageRealm::empty_reply();
            realm.snapshot(&mut reply);
            let mut ledger =
                ObserverLedger::new(ObserverExpectation::WindowHarness { run_id: 7 }).unwrap();
            ledger
                .stage(reply.observer.take().unwrap())
                .unwrap()
                .commit();
            let view = ledger.observation();
            assert_eq!(view.records.len(), 1);
            let ObserverRecord::Completion { harness, inventory } = &view.records[0] else {
                panic!()
            };
            assert_eq!(harness.status, HarnessStatus::Error);
            assert_eq!(inventory[0].status, expected);
            assert!(matches!(inventory[0].stack, OptionalUtf16::Undefined));
            assert_eq!(view.reconciliations[0].kind, NameReconciliation::FinalOnly);
            assert!(!view.execution_failed);
        }
    }

    #[test]
    fn completion_array_holes_accessors_proxies_and_nonempty_debug_do_not_run_getters() {
        for source in [
            "globalThis.hits=0;var a=[];a.length=1;Object.setPrototypeOf(a,{get 0(){hits++;throw 1}});a",
            "globalThis.hits=0;var a=[];Object.defineProperty(a,'0',{get(){hits++;throw 1}});a",
            "globalThis.hits=0;new Proxy([],{get(){hits++;throw 1},getOwnPropertyDescriptor(){hits++;throw 2}})",
        ] {
            let mut realm = realm();
            let values = evaluate(&mut realm, source);
            let debug = evaluate(&mut realm, "[]");
            complete_value(&mut realm, values, 1, debug);
            assert_eq!(fault(&realm), Some(ObserverFault::MalformedCapture));
            assert_eq!(
                own_data(
                    &realm.engine.context().global_object(),
                    js_string!("hits").into()
                )
                .unwrap()
                .as_number(),
                Some(0.0)
            );
        }
        let mut realm = realm();
        let values = evaluate(
            &mut realm,
            "globalThis.hits=0;[{index:0,status:0,get name(){hits++;throw 1},message:null,stack:null}]",
        );
        let debug = evaluate(&mut realm, "[1]");
        complete_value(&mut realm, values, 1, debug);
        assert_eq!(fault(&realm), Some(ObserverFault::MalformedCapture));
        assert_eq!(
            own_data(
                &realm.engine.context().global_object(),
                js_string!("hits").into()
            )
            .unwrap()
            .as_number(),
            Some(0.0)
        );
    }
    #[test]
    fn pinned_sanitizer_preserves_pairs_replaces_only_isolated_units_and_collisions() {
        for (raw, expected) in [
            (vec![], vec![]),
            (vec![65], vec![65]),
            (vec![0xd83d, 0xde00], vec![0xd83d, 0xde00]),
            (vec![0xd800], "U+d800".encode_utf16().collect()),
            (vec![0xdc00], "U+dc00".encode_utf16().collect()),
            (
                vec![0xd800, 0xd800, 0xdc00],
                [
                    "U+d800".encode_utf16().collect::<Vec<_>>(),
                    vec![0xd800, 0xdc00],
                ]
                .concat(),
            ),
            (
                vec![0xdfff, 0xdbff],
                "U+dfffU+dbff".encode_utf16().collect(),
            ),
            (
                "U+d800".encode_utf16().collect(),
                "U+d800".encode_utf16().collect(),
            ),
        ] {
            assert!(reconciled_name(&raw, &expected));
            assert!(reconciled_name(&raw, &raw));
            let mut wrong = expected;
            wrong.push(0);
            assert!(!reconciled_name(&raw, &wrong));
        }
    }
}
