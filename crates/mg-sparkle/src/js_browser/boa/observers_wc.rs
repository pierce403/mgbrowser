//! DOM Observers (`MutationObserver`, `ResizeObserver`, `IntersectionObserver`),
//! Custom Elements v1 (`window.customElements`), Web Animations (`Element.animate`,
//! `getAnimations`), and `window.visualViewport`.
use super::{
    BoaPageRealm, Ingress, State, def_cap, def_fn, def_get_set_cap, def_getter_cap, def_prop,
    dom_events, dom_exception, dom_ops, enter_world_in_context, failure, geometry_style,
    link_ctor_global, modern, page_session, receiver, state, utf8, wrap,
};
use mg_butane::runtime::Value;
use modern::boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue, NativeFunction,
    job::{GenericJob, Job},
    js_string,
    object::{
        FunctionObjectBuilder, JsObject, ObjectInitializer,
        builtins::{JsArray, JsFunction, JsPromise},
    },
    property::Attribute,
};
use modern::boa_gc::{Finalize, Trace};
use std::collections::{HashMap, HashSet};

pub(super) const EL_ANIMATE: &str = "host.m5.animate";
pub(super) const EL_GET_ANIMATIONS: &str = "host.m5.elGetAnimations";
pub(super) const DOC_GET_ANIMATIONS: &str = "host.m5.docGetAnimations";

const MAX_OBSERVERS: usize = 256;
const MAX_QUEUED_MUTATION_RECORDS: usize = 2048;
const MAX_CE_DEFS: usize = 256;
const MAX_ANIMATIONS: usize = 512;

#[derive(Clone, Debug)]
struct MutationReg {
    target: usize,
    child_list: bool,
    attributes: bool,
    character_data: bool,
    subtree: bool,
    attribute_old_value: bool,
    character_data_old_value: bool,
    attribute_filter: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
enum PendingMutationKind {
    ChildList {
        added: Vec<usize>,
        removed: Vec<usize>,
        prev_sibling: Option<usize>,
        next_sibling: Option<usize>,
    },
    Attributes {
        name: String,
        old_value: Option<String>,
    },
    CharacterData {
        old_value: Option<String>,
    },
}

#[derive(Clone, Debug)]
struct PendingMutationRecord {
    target: usize,
    kind: PendingMutationKind,
}

#[derive(Trace, Finalize)]
struct MutationObserverRecord {
    object: JsObject,
    callback: JsObject,
    #[unsafe_ignore_trace]
    registrations: Vec<MutationReg>,
    #[unsafe_ignore_trace]
    records: Vec<PendingMutationRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoxSizeKey {
    bw: u64,
    bh: u64,
    cw: u64,
    ch: u64,
}

#[derive(Trace, Finalize)]
struct ResizeObserverRecord {
    object: JsObject,
    callback: JsObject,
    #[unsafe_ignore_trace]
    targets: Vec<(usize, Option<BoxSizeKey>)>,
}

#[derive(Clone, Copy, Debug)]
enum MarginValue {
    Px(f64),
    Percent(f64),
}

impl MarginValue {
    fn resolve(self, basis: f64) -> f64 {
        match self {
            Self::Px(px) => px,
            Self::Percent(pct) => basis * (pct / 100.0),
        }
    }
}

#[derive(Clone, Debug)]
struct PendingIntersectionEntry {
    time: f64,
    target: usize,
    root_bounds: (f64, f64, f64, f64),
    bounding_client_rect: (f64, f64, f64, f64),
    intersection_rect: (f64, f64, f64, f64),
    is_intersecting: bool,
    intersection_ratio: f64,
}

#[derive(Trace, Finalize)]
struct IntersectionObserverRecord {
    object: JsObject,
    callback: JsObject,
    #[unsafe_ignore_trace]
    root: Option<usize>,
    #[unsafe_ignore_trace]
    root_margin: [MarginValue; 4],
    #[unsafe_ignore_trace]
    root_margin_str: String,
    #[unsafe_ignore_trace]
    thresholds: Vec<f64>,
    #[unsafe_ignore_trace]
    targets: Vec<(usize, Option<(bool, i32)>)>,
    #[unsafe_ignore_trace]
    records: Vec<PendingIntersectionEntry>,
}

#[derive(Trace, Finalize)]
struct CustomElementDef {
    #[unsafe_ignore_trace]
    name: String,
    #[unsafe_ignore_trace]
    extends: Option<String>,
    constructor: JsObject,
    prototype: JsObject,
    #[unsafe_ignore_trace]
    observed_attributes: Vec<String>,
}

#[derive(Trace, Finalize)]
struct PendingWhenDefined {
    #[unsafe_ignore_trace]
    name: String,
    promise: JsObject,
    resolve: JsFunction,
}

#[derive(Clone, Debug)]
enum CeReaction {
    Upgrade(usize),
    Connected(usize),
    Disconnected(usize),
    AttributeChanged {
        node: usize,
        name: String,
        old_value: Option<String>,
        new_value: Option<String>,
    },
}

#[derive(Trace, Finalize)]
struct ConstructionEntry {
    #[unsafe_ignore_trace]
    node: Option<usize>,
    constructor: JsObject,
}

#[derive(Trace, Finalize)]
struct AnimationEntry {
    #[unsafe_ignore_trace]
    target: usize,
    object: JsObject,
}

#[derive(Trace, Finalize)]
struct VvListener {
    #[unsafe_ignore_trace]
    event_type: String,
    callback: JsObject,
    once: bool,
    property: bool,
}

#[derive(Default, Trace, Finalize)]
pub(super) struct ObserversWcState {
    mutation_observers: Vec<MutationObserverRecord>,
    resize_observers: Vec<ResizeObserverRecord>,
    intersection_observers: Vec<IntersectionObserverRecord>,
    #[unsafe_ignore_trace]
    mutation_job_scheduled: bool,
    #[unsafe_ignore_trace]
    layout_job_scheduled: bool,
    #[unsafe_ignore_trace]
    layout_dirty: bool,
    ce_defs: Vec<CustomElementDef>,
    pending_when_defined: Vec<PendingWhenDefined>,
    construction_stack: Vec<ConstructionEntry>,
    #[unsafe_ignore_trace]
    upgraded_nodes: HashMap<usize, usize>,
    upgraded_wrappers: HashMap<usize, JsObject>,
    #[unsafe_ignore_trace]
    pending_ce_reactions: Vec<CeReaction>,
    #[unsafe_ignore_trace]
    ce_draining: bool,
    animations: Vec<AnimationEntry>,
    animation_proto: Option<JsObject>,
    keyframe_effect_proto: Option<JsObject>,
    main_visual_viewport: Option<JsObject>,
    vv_listeners: Vec<VvListener>,
}

impl ObserversWcState {
    pub(super) fn clear(&mut self) {
        self.mutation_observers.clear();
        self.resize_observers.clear();
        self.intersection_observers.clear();
        self.mutation_job_scheduled = false;
        self.layout_job_scheduled = false;
        self.layout_dirty = false;
        self.ce_defs.clear();
        self.pending_when_defined.clear();
        self.construction_stack.clear();
        self.upgraded_nodes.clear();
        self.upgraded_wrappers.clear();
        self.pending_ce_reactions.clear();
        self.ce_draining = false;
        self.animations.clear();
        self.animation_proto = None;
        self.keyframe_effect_proto = None;
        self.main_visual_viewport = None;
        self.vv_listeners.clear();
    }
}

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        EL_ANIMATE => Some(("animate", 1)),
        EL_GET_ANIMATIONS => Some(("getAnimations", 0)),
        DOC_GET_ANIMATIONS => Some(("getAnimations", 0)),
        _ => None,
    }
}

#[inline]
pub(super) fn has_any_interest(data: &State) -> bool {
    let s = &data.observers_wc;
    !s.mutation_observers.is_empty()
        || !s.resize_observers.is_empty()
        || !s.intersection_observers.is_empty()
        || !s.ce_defs.is_empty()
}

pub(super) fn register(context: &mut Context, is_extra_realm: bool) -> Result<(), String> {
    let global = context.global_object();
    install_mutation_observer(&global, context)?;
    install_resize_observer(&global, context)?;
    install_intersection_observer(&global, context)?;
    install_custom_elements(&global, context)?;
    install_web_animations(&global, is_extra_realm, context)?;
    install_visual_viewport(&global, is_extra_realm, context)?;
    Ok(())
}

// ============================================================================
// DOM Node / Element Extraction Helpers
// ============================================================================

fn node_id_from_js(val: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(val, context)? else {
        return Err(JsNativeError::typ()
            .with_message("Expected a DOM Node")
            .into());
    };
    let shared = state(context);
    let data = shared.borrow();
    if handle == "document" {
        return Ok(0);
    }
    if let Some(sub) = handle
        .strip_prefix("subdoc:")
        .and_then(|s| s.parse::<usize>().ok())
        && sub < data.host.document.nodes.len()
    {
        return Ok(sub);
    }
    data.host.node(&handle).map_err(|_| {
        JsNativeError::typ()
            .with_message("Expected a DOM Node")
            .into()
    })
}

fn element_id_from_js(val: &JsValue, context: &mut Context) -> JsResult<usize> {
    let id = node_id_from_js(val, context)?;
    let is_el = {
        let shared = state(context);
        let data = shared.borrow();
        id != 0
            && id < data.host.document.nodes.len()
            && !data.host.document.nodes[id].tag.starts_with('#')
    };
    if !is_el {
        return Err(JsNativeError::typ()
            .with_message("Expected a DOM Element")
            .into());
    }
    Ok(id)
}

fn wrap_any_node(id: usize, context: &mut Context) -> JsResult<JsValue> {
    if id == 0 {
        return wrap("document", context);
    }
    let is_subdoc = state(context)
        .borrow()
        .host
        .document
        .nodes
        .get(id)
        .is_some_and(|n| n.tag == "#document-subroot");
    if is_subdoc {
        wrap(&format!("subdoc:{id}"), context)
    } else {
        wrap(&format!("node:{id}"), context)
    }
}

fn build_nodelist_array(ids: &[usize], context: &mut Context) -> JsResult<JsObject> {
    let mut vals = Vec::with_capacity(ids.len());
    for &id in ids {
        vals.push(wrap_any_node(id, context)?);
    }
    let arr: JsObject = JsArray::from_iter(vals, context).into();
    def_fn(
        &arr,
        "item",
        1,
        false,
        |this, args, ctx| {
            let obj = this
                .as_object()
                .ok_or_else(|| failure("NodeList.item requires an object receiver"))?;
            let idx = args.first().unwrap_or(&JsValue::undefined()).to_u32(ctx)?;
            let v = obj.get(idx, ctx)?;
            Ok(if v.is_undefined() { JsValue::null() } else { v })
        },
        context,
    )?;
    Ok(arr)
}

fn new_observer_instance(
    new_target: &JsValue,
    args: &[JsValue],
    name: &str,
    ctx: &mut Context,
) -> JsResult<(JsObject, JsObject)> {
    modern::check_context(ctx)?;
    if new_target.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message(format!("{name} constructor requires 'new'"))
            .into());
    }
    let Some(cb) = args.first().and_then(JsValue::as_callable) else {
        return Err(JsNativeError::typ()
            .with_message(format!("{name} constructor requires a callback function"))
            .into());
    };
    let inst_proto = new_target
        .as_object()
        .and_then(|o| o.get(js_string!("prototype"), ctx).ok())
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| ctx.intrinsics().constructors().object().prototype());
    Ok((JsObject::from_proto_and_data(inst_proto, ()), cb))
}

// ============================================================================
// MutationObserver
// ============================================================================

fn install_mutation_observer(global: &JsObject, context: &mut Context) -> Result<(), String> {
    let proto = JsObject::with_object_proto(context.intrinsics());
    for (k, len, f) in [
        (
            "observe",
            2usize,
            mo_observe as fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
        ),
        ("disconnect", 0, mo_disconnect),
        ("takeRecords", 0, mo_take_records),
    ] {
        def_fn(&proto, k, len, false, f, context)
            .map_err(|_| "MutationObserver method install failed")?;
    }

    let ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|new_target, args, ctx| {
            let (inst, cb) = new_observer_instance(new_target, args, "MutationObserver", ctx)?;
            {
                let shared = state(ctx);
                let mut data = shared.borrow_mut();
                if data.observers_wc.mutation_observers.len() >= MAX_OBSERVERS {
                    return Err(failure("MutationObserver limit exceeded"));
                }
                data.host.charge(256).map_err(failure)?;
                data.observers_wc
                    .mutation_observers
                    .push(MutationObserverRecord {
                        object: inst.clone(),
                        callback: cb,
                        registrations: Vec::new(),
                        records: Vec::new(),
                    });
            }
            Ok(inst.into())
        }),
    )
    .name(js_string!("MutationObserver"))
    .length(1)
    .constructor(true)
    .build()
    .into();

    link_ctor_global(global, "MutationObserver", &ctor, &proto, context)
}

fn find_mo_index(this: &JsValue, data: &State) -> JsResult<usize> {
    let Some(obj) = this.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("Illegal invocation on MutationObserver")
            .into());
    };
    data.observers_wc
        .mutation_observers
        .iter()
        .position(|r| r.object == obj)
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("Illegal invocation on MutationObserver")
                .into()
        })
}

fn mo_observe(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let _ = {
        let shared = state(ctx);
        let data = shared.borrow();
        find_mo_index(this, &data)?
    };
    let target = node_id_from_js(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
    let opts = args.get(1).and_then(JsValue::as_object).ok_or_else(|| {
        JsNativeError::typ().with_message("MutationObserver.observe requires options")
    })?;

    let child_list_v = opts.get(js_string!("childList"), ctx)?;
    let child_list = child_list_v.to_boolean();

    let attrs_v = opts.get(js_string!("attributes"), ctx)?;
    let attrs_opt = if attrs_v.is_undefined() {
        None
    } else {
        Some(attrs_v.to_boolean())
    };

    let char_v = opts.get(js_string!("characterData"), ctx)?;
    let char_opt = if char_v.is_undefined() {
        None
    } else {
        Some(char_v.to_boolean())
    };

    let subtree = opts.get(js_string!("subtree"), ctx)?.to_boolean();

    let attr_old_v = opts.get(js_string!("attributeOldValue"), ctx)?;
    let attr_old_opt = if attr_old_v.is_undefined() {
        None
    } else {
        Some(attr_old_v.to_boolean())
    };

    let char_old_v = opts.get(js_string!("characterDataOldValue"), ctx)?;
    let char_old_opt = if char_old_v.is_undefined() {
        None
    } else {
        Some(char_old_v.to_boolean())
    };

    let filter_v = opts.get(js_string!("attributeFilter"), ctx)?;
    let attribute_filter = if filter_v.is_undefined() {
        None
    } else {
        let Some(fobj) = filter_v.as_object() else {
            return Err(JsNativeError::typ()
                .with_message("attributeFilter must be a sequence")
                .into());
        };
        let len = fobj.get(js_string!("length"), ctx)?.to_u32(ctx)?.min(256);
        let mut names = Vec::with_capacity(len as usize);
        for i in 0..len {
            let item = fobj.get(i, ctx)?;
            names.push(utf8(&item, ctx)?.to_ascii_lowercase());
            modern::check_context(ctx)?;
        }
        Some(names)
    };

    let attributes = match attrs_opt {
        Some(b) => b,
        None => attr_old_opt == Some(true) || attribute_filter.is_some(),
    };
    if !attributes && (attr_old_opt == Some(true) || attribute_filter.is_some()) {
        return Err(JsNativeError::typ()
            .with_message("Attributes option cannot be false when attributeOldValue or attributeFilter is specified")
            .into());
    }

    let character_data = match char_opt {
        Some(b) => b,
        None => char_old_opt == Some(true),
    };
    if !character_data && char_old_opt == Some(true) {
        return Err(JsNativeError::typ()
            .with_message("characterData option cannot be false when characterDataOldValue is true")
            .into());
    }

    if !child_list && !attributes && !character_data {
        return Err(JsNativeError::typ()
            .with_message(
                "MutationObserver.observe requires childList, attributes, or characterData",
            )
            .into());
    }

    let reg = MutationReg {
        target,
        child_list,
        attributes,
        character_data,
        subtree,
        attribute_old_value: attr_old_opt.unwrap_or(false),
        character_data_old_value: char_old_opt.unwrap_or(false),
        attribute_filter,
    };

    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let idx = find_mo_index(this, &data)?;
    data.host.charge(128).map_err(failure)?;
    let obs = &mut data.observers_wc.mutation_observers[idx];
    if let Some(existing) = obs.registrations.iter_mut().find(|r| r.target == target) {
        *existing = reg;
    } else {
        obs.registrations.push(reg);
    }
    Ok(JsValue::undefined())
}

fn mo_disconnect(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let idx = find_mo_index(this, &data)?;
    let obs = &mut data.observers_wc.mutation_observers[idx];
    obs.registrations.clear();
    obs.records.clear();
    Ok(JsValue::undefined())
}

fn mo_take_records(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let drained = {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        let idx = find_mo_index(this, &data)?;
        std::mem::take(&mut data.observers_wc.mutation_observers[idx].records)
    };
    build_mutation_records_js(&drained, ctx).map(Into::into)
}

fn build_mutation_records_js(
    records: &[PendingMutationRecord],
    ctx: &mut Context,
) -> JsResult<JsArray> {
    let mut out = Vec::with_capacity(records.len());
    for rec in records {
        let target_js = wrap_any_node(rec.target, ctx)?;
        let (type_str, added_ids, removed_ids, prev_id, next_id, attr_name, old_val) =
            match &rec.kind {
                PendingMutationKind::ChildList {
                    added,
                    removed,
                    prev_sibling,
                    next_sibling,
                } => (
                    "childList",
                    added.as_slice(),
                    removed.as_slice(),
                    *prev_sibling,
                    *next_sibling,
                    None,
                    None,
                ),
                PendingMutationKind::Attributes { name, old_value } => (
                    "attributes",
                    &[][..],
                    &[][..],
                    None,
                    None,
                    Some(name.as_str()),
                    old_value.as_deref(),
                ),
                PendingMutationKind::CharacterData { old_value } => (
                    "characterData",
                    &[][..],
                    &[][..],
                    None,
                    None,
                    None,
                    old_value.as_deref(),
                ),
            };
        let added_js = build_nodelist_array(added_ids, ctx)?;
        let removed_js = build_nodelist_array(removed_ids, ctx)?;
        let prev_js = match prev_id {
            Some(id) => wrap_any_node(id, ctx)?,
            None => JsValue::null(),
        };
        let next_js = match next_id {
            Some(id) => wrap_any_node(id, ctx)?,
            None => JsValue::null(),
        };
        let attr_js = match attr_name {
            Some(n) => JsString::from(n).into(),
            None => JsValue::null(),
        };
        let old_js = match old_val {
            Some(v) => JsString::from(v).into(),
            None => JsValue::null(),
        };
        let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
        let entry = ObjectInitializer::new(ctx)
            .property(js_string!("type"), JsString::from(type_str), ec)
            .property(js_string!("target"), target_js, ec)
            .property(js_string!("addedNodes"), added_js, ec)
            .property(js_string!("removedNodes"), removed_js, ec)
            .property(js_string!("previousSibling"), prev_js, ec)
            .property(js_string!("nextSibling"), next_js, ec)
            .property(js_string!("attributeName"), attr_js, ec)
            .property(js_string!("attributeNamespace"), JsValue::null(), ec)
            .property(js_string!("oldValue"), old_js, ec)
            .build();
        out.push(entry.into());
    }
    Ok(JsArray::from_iter(out, ctx))
}

// ============================================================================
// ResizeObserver
// ============================================================================

fn find_ro_mut<'a>(this: &JsValue, data: &'a mut State) -> JsResult<&'a mut ResizeObserverRecord> {
    let Some(obj) = this.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("Illegal invocation on ResizeObserver")
            .into());
    };
    data.observers_wc
        .resize_observers
        .iter_mut()
        .find(|r| r.object == obj)
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("Illegal invocation on ResizeObserver")
                .into()
        })
}

fn install_resize_observer(global: &JsObject, context: &mut Context) -> Result<(), String> {
    let proto = JsObject::with_object_proto(context.intrinsics());
    def_fn(
        &proto,
        "observe",
        1,
        false,
        |this, args, ctx| {
            modern::check_context(ctx)?;
            let target = element_id_from_js(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            {
                let shared = state(ctx);
                let mut data = shared.borrow_mut();
                let ro = find_ro_mut(this, &mut data)?;
                if let Some(existing) = ro.targets.iter_mut().find(|(id, _)| *id == target) {
                    existing.1 = None;
                } else {
                    ro.targets.push((target, None));
                }
                data.observers_wc.layout_dirty = true;
            }
            ensure_layout_observer_job(ctx)?;
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "ResizeObserver.observe failed")?;

    def_fn(
        &proto,
        "unobserve",
        1,
        false,
        |this, args, ctx| {
            modern::check_context(ctx)?;
            let target = element_id_from_js(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            let shared = state(ctx);
            let mut data = shared.borrow_mut();
            find_ro_mut(this, &mut data)?
                .targets
                .retain(|(id, _)| *id != target);
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "ResizeObserver.unobserve failed")?;

    def_fn(
        &proto,
        "disconnect",
        0,
        false,
        |this, _, ctx| {
            modern::check_context(ctx)?;
            let shared = state(ctx);
            let mut data = shared.borrow_mut();
            find_ro_mut(this, &mut data)?.targets.clear();
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "ResizeObserver.disconnect failed")?;

    let ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|new_target, args, ctx| {
            let (inst, cb) = new_observer_instance(new_target, args, "ResizeObserver", ctx)?;
            {
                let shared = state(ctx);
                let mut data = shared.borrow_mut();
                if data.observers_wc.resize_observers.len() >= MAX_OBSERVERS {
                    return Err(failure("ResizeObserver limit exceeded"));
                }
                data.host.charge(256).map_err(failure)?;
                data.observers_wc
                    .resize_observers
                    .push(ResizeObserverRecord {
                        object: inst.clone(),
                        callback: cb,
                        targets: Vec::new(),
                    });
            }
            Ok(inst.into())
        }),
    )
    .name(js_string!("ResizeObserver"))
    .length(1)
    .constructor(true)
    .build()
    .into();

    link_ctor_global(global, "ResizeObserver", &ctor, &proto, context)
}

// ============================================================================
// IntersectionObserver
// ============================================================================

fn parse_root_margin(raw: &str) -> Result<([MarginValue; 4], String), &'static str> {
    let parts: Vec<&str> = raw.split_ascii_whitespace().collect();
    if parts.is_empty() || parts.len() > 4 {
        return Err("rootMargin must have 1 to 4 pixel or percentage values");
    }
    let mut parsed = Vec::with_capacity(parts.len());
    let mut formatted = Vec::with_capacity(parts.len());
    for part in parts {
        if let Some(num_s) = part.strip_suffix("px") {
            let v: f64 = num_s
                .parse()
                .ok()
                .filter(|v: &f64| v.is_finite())
                .ok_or("Invalid pixel value in rootMargin")?;
            parsed.push(MarginValue::Px(v));
            formatted.push(format!("{v}px"));
        } else if let Some(num_s) = part.strip_suffix('%') {
            let v: f64 = num_s
                .parse()
                .ok()
                .filter(|v: &f64| v.is_finite())
                .ok_or("Invalid percent value in rootMargin")?;
            parsed.push(MarginValue::Percent(v));
            formatted.push(format!("{v}%"));
        } else {
            return Err("rootMargin values must end with 'px' or '%'");
        }
    }
    let (m4, s4) = match parsed.len() {
        1 => (
            [parsed[0], parsed[0], parsed[0], parsed[0]],
            [&formatted[0], &formatted[0], &formatted[0], &formatted[0]],
        ),
        2 => (
            [parsed[0], parsed[1], parsed[0], parsed[1]],
            [&formatted[0], &formatted[1], &formatted[0], &formatted[1]],
        ),
        3 => (
            [parsed[0], parsed[1], parsed[2], parsed[1]],
            [&formatted[0], &formatted[1], &formatted[2], &formatted[1]],
        ),
        _ => (
            [parsed[0], parsed[1], parsed[2], parsed[3]],
            [&formatted[0], &formatted[1], &formatted[2], &formatted[3]],
        ),
    };
    Ok((m4, format!("{} {} {} {}", s4[0], s4[1], s4[2], s4[3])))
}

fn install_intersection_observer(global: &JsObject, context: &mut Context) -> Result<(), String> {
    let proto = JsObject::with_object_proto(context.intrinsics());
    def_fn(
        &proto,
        "observe",
        1,
        false,
        |this, args, ctx| {
            modern::check_context(ctx)?;
            let target = element_id_from_js(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            {
                let shared = state(ctx);
                let mut data = shared.borrow_mut();
                let idx = find_io_index(this, &data)?;
                let io = &mut data.observers_wc.intersection_observers[idx];
                if !io.targets.iter().any(|(id, _)| *id == target) {
                    io.targets.push((target, None));
                }
                data.observers_wc.layout_dirty = true;
            }
            ensure_layout_observer_job(ctx)?;
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "IntersectionObserver.observe failed")?;

    def_fn(
        &proto,
        "unobserve",
        1,
        false,
        |this, args, ctx| {
            modern::check_context(ctx)?;
            let target = element_id_from_js(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            let shared = state(ctx);
            let mut data = shared.borrow_mut();
            let idx = find_io_index(this, &data)?;
            data.observers_wc.intersection_observers[idx]
                .targets
                .retain(|(id, _)| *id != target);
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "IntersectionObserver.unobserve failed")?;

    def_fn(
        &proto,
        "disconnect",
        0,
        false,
        |this, _, ctx| {
            modern::check_context(ctx)?;
            let shared = state(ctx);
            let mut data = shared.borrow_mut();
            let idx = find_io_index(this, &data)?;
            let io = &mut data.observers_wc.intersection_observers[idx];
            io.targets.clear();
            io.records.clear();
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "IntersectionObserver.disconnect failed")?;

    def_fn(
        &proto,
        "takeRecords",
        0,
        false,
        |this, _, ctx| {
            modern::check_context(ctx)?;
            let drained = {
                let shared = state(ctx);
                let mut data = shared.borrow_mut();
                let idx = find_io_index(this, &data)?;
                evaluate_intersection_observer_index(&mut data, idx);
                std::mem::take(&mut data.observers_wc.intersection_observers[idx].records)
            };
            build_io_entries_js(&drained, ctx).map(Into::into)
        },
        context,
    )
    .map_err(|_| "IntersectionObserver.takeRecords failed")?;

    for prop in ["root", "rootMargin", "thresholds"] {
        def_getter_cap(
            &proto,
            prop,
            true,
            prop,
            |this, _, &p, ctx| {
                modern::check_context(ctx)?;
                let (root_opt, margin_s, ts) = {
                    let shared = state(ctx);
                    let data = shared.borrow();
                    let io = &data.observers_wc.intersection_observers[find_io_index(this, &data)?];
                    (io.root, io.root_margin_str.clone(), io.thresholds.clone())
                };
                match p {
                    "root" => root_opt.map_or(Ok(JsValue::null()), |rid| wrap_any_node(rid, ctx)),
                    "rootMargin" => Ok(JsString::from(margin_s.as_str()).into()),
                    _ => Ok(JsArray::from_iter(ts.into_iter().map(JsValue::from), ctx).into()),
                }
            },
            context,
        )
        .map_err(|_| "IntersectionObserver getter install failed")?;
    }

    let ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|new_target, args, ctx| {
            modern::check_context(ctx)?;
            if new_target.is_undefined() {
                return Err(JsNativeError::typ()
                    .with_message("IntersectionObserver constructor requires 'new'")
                    .into());
            }
            let Some(cb) = args.first().and_then(JsValue::as_callable) else {
                return Err(JsNativeError::typ()
                    .with_message("IntersectionObserver constructor requires a callback function")
                    .into());
            };
            let mut root = None;
            let mut root_margin = [MarginValue::Px(0.0); 4];
            let mut root_margin_str = "0px 0px 0px 0px".to_owned();
            let mut thresholds = vec![0.0f64];

            if let Some(opts_val) = args.get(1)
                && !opts_val.is_undefined()
                && !opts_val.is_null()
            {
                let opts = opts_val.as_object().ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message("IntersectionObserver options must be an object")
                })?;
                let rv = opts.get(js_string!("root"), ctx)?;
                if !rv.is_undefined() && !rv.is_null() {
                    let rid = node_id_from_js(&rv, ctx)?;
                    let valid_root = {
                        let shared = state(ctx);
                        let data = shared.borrow();
                        rid == 0 || !data.host.document.nodes[rid].tag.starts_with('#')
                    };
                    if !valid_root {
                        return Err(JsNativeError::typ()
                            .with_message(
                                "IntersectionObserver root must be an Element or Document",
                            )
                            .into());
                    }
                    root = Some(rid);
                }
                let rm = opts.get(js_string!("rootMargin"), ctx)?;
                if !rm.is_undefined() {
                    let s = utf8(&rm, ctx)?;
                    modern::check_context(ctx)?;
                    let (parsed_m, norm_s) = parse_root_margin(&s)
                        .map_err(|msg| dom_exception::throw("SyntaxError", msg, ctx))?;
                    root_margin = parsed_m;
                    root_margin_str = norm_s;
                }
                let th = opts.get(js_string!("threshold"), ctx)?;
                if !th.is_undefined() {
                    let mut raw_list = Vec::new();
                    if let Some(tobj) = th.as_object() {
                        let len = tobj.get(js_string!("length"), ctx)?.to_u32(ctx)?.min(128);
                        for i in 0..len {
                            raw_list.push(tobj.get(i, ctx)?.to_number(ctx)?);
                        }
                    } else {
                        raw_list.push(th.to_number(ctx)?);
                    }
                    for &v in &raw_list {
                        if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                            return Err(JsNativeError::range()
                                .with_message("IntersectionObserver threshold must be in [0, 1]")
                                .into());
                        }
                    }
                    if raw_list.is_empty() {
                        raw_list.push(0.0);
                    }
                    raw_list.sort_by(|a, b| a.total_cmp(b));
                    raw_list.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                    thresholds = raw_list;
                }
            }

            let inst_proto = new_target
                .as_object()
                .and_then(|o| o.get(js_string!("prototype"), ctx).ok())
                .and_then(|v| v.as_object())
                .unwrap_or_else(|| ctx.intrinsics().constructors().object().prototype());
            let inst = JsObject::from_proto_and_data(inst_proto, ());
            {
                let shared = state(ctx);
                let mut data = shared.borrow_mut();
                if data.observers_wc.intersection_observers.len() >= MAX_OBSERVERS {
                    return Err(failure("IntersectionObserver limit exceeded"));
                }
                data.host.charge(256).map_err(failure)?;
                data.observers_wc
                    .intersection_observers
                    .push(IntersectionObserverRecord {
                        object: inst.clone(),
                        callback: cb,
                        root,
                        root_margin,
                        root_margin_str,
                        thresholds,
                        targets: Vec::new(),
                        records: Vec::new(),
                    });
            }
            Ok(inst.into())
        }),
    )
    .name(js_string!("IntersectionObserver"))
    .length(1)
    .constructor(true)
    .build()
    .into();

    link_ctor_global(global, "IntersectionObserver", &ctor, &proto, context)
}

fn find_io_index(this: &JsValue, data: &State) -> JsResult<usize> {
    let Some(obj) = this.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("Illegal invocation on IntersectionObserver")
            .into());
    };
    data.observers_wc
        .intersection_observers
        .iter()
        .position(|r| r.object == obj)
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("Illegal invocation on IntersectionObserver")
                .into()
        })
}

fn evaluate_intersection_observer_index(data: &mut State, idx: usize) {
    let (root_opt, margins, thresholds, targets_snapshot) = {
        let io = &data.observers_wc.intersection_observers[idx];
        (
            io.root,
            io.root_margin,
            io.thresholds.clone(),
            io.targets.clone(),
        )
    };
    if targets_snapshot.is_empty() {
        return;
    }
    let base_root = match root_opt {
        None | Some(0) => (
            0.0,
            0.0,
            data.viewport.inner_width as f64,
            data.viewport.inner_height as f64,
        ),
        Some(rid) => geometry_style::client_rect_and_boxes(data, rid)
            .map(|(bcr, _)| bcr)
            .unwrap_or((0.0, 0.0, 0.0, 0.0)),
    };
    let mt = margins[0].resolve(base_root.3);
    let mr = margins[1].resolve(base_root.2);
    let mb = margins[2].resolve(base_root.3);
    let ml = margins[3].resolve(base_root.2);
    let rx0 = base_root.0 - ml;
    let ry0 = base_root.1 - mt;
    let rx1 = base_root.0 + base_root.2 + mr;
    let ry1 = base_root.1 + base_root.3 + mb;
    let rw = (rx1 - rx0).max(0.0);
    let rh = (ry1 - ry0).max(0.0);
    let root_bounds = (rx0, ry0, rw, rh);
    let now_ms = dom_events::elapsed_ms(data);

    let mut updated_targets = targets_snapshot;
    let mut new_entries = Vec::new();
    for (tid, last_state) in &mut updated_targets {
        let target_opt = geometry_style::client_rect_and_boxes(data, *tid).map(|(bcr, _)| bcr);
        let (bcr, irec, is_intersecting, ratio) = match target_opt {
            None => ((0.0, 0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 0.0), false, 0.0),
            Some((tx, ty, tw, th)) => {
                let ix0 = tx.max(rx0);
                let iy0 = ty.max(ry0);
                let ix1 = (tx + tw).min(rx1);
                let iy1 = (ty + th).min(ry1);
                if ix1 >= ix0 && iy1 >= iy0 && rw > 0.0 && rh > 0.0 {
                    let iw = (ix1 - ix0).max(0.0);
                    let ih = (iy1 - iy0).max(0.0);
                    let area = tw * th;
                    let (inter, rat) = if area > 0.0 {
                        let r = ((iw * ih) / area).clamp(0.0, 1.0);
                        (iw > 0.0 && ih > 0.0, r)
                    } else {
                        (true, 1.0)
                    };
                    let ir = if inter {
                        (ix0, iy0, iw, ih)
                    } else {
                        (0.0, 0.0, 0.0, 0.0)
                    };
                    ((tx, ty, tw, th), ir, inter, rat)
                } else {
                    ((tx, ty, tw, th), (0.0, 0.0, 0.0, 0.0), false, 0.0)
                }
            }
        };
        let threshold_idx: i32 = if !is_intersecting {
            -1
        } else {
            thresholds.iter().filter(|&&t| ratio + 1e-7 >= t).count() as i32
        };
        let next_state = (is_intersecting, threshold_idx);
        if *last_state != Some(next_state) {
            *last_state = Some(next_state);
            new_entries.push(PendingIntersectionEntry {
                time: now_ms,
                target: *tid,
                root_bounds,
                bounding_client_rect: bcr,
                intersection_rect: irec,
                is_intersecting,
                intersection_ratio: ratio,
            });
        }
    }
    if let Some(io) = data.observers_wc.intersection_observers.get_mut(idx) {
        io.targets = updated_targets;
        io.records.extend(new_entries);
    }
}

fn build_io_entries_js(
    entries: &[PendingIntersectionEntry],
    ctx: &mut Context,
) -> JsResult<JsArray> {
    let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let target_js = wrap(&format!("node:{}", e.target), ctx)?;
        let (rb_t, bcr_t, ir_t) = (e.root_bounds, e.bounding_client_rect, e.intersection_rect);
        let rb = geometry_style::build_dom_rect(rb_t.0, rb_t.1, rb_t.2, rb_t.3, ctx);
        let bcr = geometry_style::build_dom_rect(bcr_t.0, bcr_t.1, bcr_t.2, bcr_t.3, ctx);
        let ir = geometry_style::build_dom_rect(ir_t.0, ir_t.1, ir_t.2, ir_t.3, ctx);
        let obj = ObjectInitializer::new(ctx)
            .property(js_string!("time"), e.time, ec)
            .property(js_string!("target"), target_js, ec)
            .property(js_string!("rootBounds"), rb, ec)
            .property(js_string!("boundingClientRect"), bcr, ec)
            .property(js_string!("intersectionRect"), ir, ec)
            .property(js_string!("isIntersecting"), e.is_intersecting, ec)
            .property(js_string!("intersectionRatio"), e.intersection_ratio, ec)
            .build();
        out.push(obj.into());
    }
    Ok(JsArray::from_iter(out, ctx))
}

// ============================================================================
// Custom Elements v1 (`window.customElements`)
// ============================================================================

fn is_valid_custom_element_name(name: &str) -> bool {
    if name.len() < 2 || name.len() > 128 || !name.contains('-') {
        return false;
    }
    if matches!(
        name,
        "annotation-xml"
            | "color-profile"
            | "font-face"
            | "font-face-src"
            | "font-face-uri"
            | "font-face-format"
            | "font-face-name"
            | "missing-glyph"
    ) {
        return false;
    }
    let bytes = name.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    name.chars().all(|c| {
        c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || matches!(c, '-' | '.' | '_' | '\u{00B7}')
            || (!c.is_ascii() && !c.is_whitespace() && !c.is_control())
    })
}

fn install_custom_elements(global: &JsObject, context: &mut Context) -> Result<(), String> {
    let reg_proto = JsObject::with_object_proto(context.intrinsics());
    def_fn(&reg_proto, "define", 2, true, ce_define, context)
        .map_err(|_| "CustomElementRegistry.define failed")?;
    def_fn(
        &reg_proto,
        "get",
        1,
        true,
        |_, args, ctx| {
            modern::check_context(ctx)?;
            let name = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            modern::check_context(ctx)?;
            let found = state(ctx)
                .borrow()
                .observers_wc
                .ce_defs
                .iter()
                .find(|d| d.name == name)
                .map(|d| d.constructor.clone());
            Ok(found.map(Into::into).unwrap_or_else(JsValue::undefined))
        },
        context,
    )
    .map_err(|_| "CustomElementRegistry.get failed")?;
    def_fn(
        &reg_proto,
        "getName",
        1,
        true,
        |_, args, ctx| {
            modern::check_context(ctx)?;
            let Some(ctor) = args.first().and_then(JsValue::as_object) else {
                return Err(JsNativeError::typ()
                    .with_message("customElements.getName requires a constructor")
                    .into());
            };
            let found = state(ctx)
                .borrow()
                .observers_wc
                .ce_defs
                .iter()
                .find(|d| d.constructor == ctor)
                .map(|d| d.name.clone());
            Ok(found
                .map(|n| JsString::from(n.as_str()).into())
                .unwrap_or_else(JsValue::null))
        },
        context,
    )
    .map_err(|_| "CustomElementRegistry.getName failed")?;
    def_fn(
        &reg_proto,
        "whenDefined",
        1,
        true,
        |_, args, ctx| {
            modern::check_context(ctx)?;
            let name = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            modern::check_context(ctx)?;
            if !is_valid_custom_element_name(&name) {
                let err =
                    dom_exception::exception("SyntaxError", "Invalid custom element name", ctx)?;
                let p = JsPromise::reject(err, ctx)?;
                modern::check_context(ctx)?;
                return Ok(p.into());
            }
            if let Some(ctor) = state(ctx)
                .borrow()
                .observers_wc
                .ce_defs
                .iter()
                .find(|d| d.name == name)
                .map(|d| d.constructor.clone())
            {
                let p = JsPromise::resolve(ctor, ctx)?;
                modern::check_context(ctx)?;
                return Ok(p.into());
            }
            if let Some(existing) = state(ctx)
                .borrow()
                .observers_wc
                .pending_when_defined
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.promise.clone())
            {
                return Ok(existing.into());
            }
            let (promise, resolvers) = JsPromise::new_pending(ctx);
            modern::check_context(ctx)?;
            let p_obj: JsObject = promise.into();
            {
                let shared = state(ctx);
                let mut data = shared.borrow_mut();
                data.host.charge(128 + name.len()).map_err(failure)?;
                data.observers_wc
                    .pending_when_defined
                    .push(PendingWhenDefined {
                        name,
                        promise: p_obj.clone(),
                        resolve: resolvers.resolve,
                    });
            }
            Ok(p_obj.into())
        },
        context,
    )
    .map_err(|_| "CustomElementRegistry.whenDefined failed")?;
    def_fn(
        &reg_proto,
        "upgrade",
        1,
        true,
        |_, args, ctx| {
            modern::check_context(ctx)?;
            let root = node_id_from_js(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            let candidates = collect_unupgraded_candidates_in_subtree(root, ctx);
            for (nid, def_idx) in candidates {
                upgrade_element_with_def(nid, def_idx, false, ctx)?;
            }
            after_dom_mutation(ctx)?;
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "CustomElementRegistry.upgrade failed")?;

    let reg_ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|_, _, ctx| {
            modern::check_context(ctx)?;
            Err(failure("Illegal constructor"))
        }),
    )
    .name(js_string!("CustomElementRegistry"))
    .length(0)
    .constructor(true)
    .build()
    .into();

    link_ctor_global(
        global,
        "CustomElementRegistry",
        &reg_ctor,
        &reg_proto,
        context,
    )?;
    let registry_inst = JsObject::from_proto_and_data(reg_proto, ());
    def_prop(
        global,
        js_string!("customElements"),
        registry_inst,
        false,
        true,
        true,
        context,
    )
    .map_err(|_| "window.customElements install failed".to_owned())
}

fn ce_define(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let name = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
    modern::check_context(ctx)?;
    if !is_valid_custom_element_name(&name) {
        return Err(dom_exception::exception(
            "SyntaxError",
            "Invalid custom element name",
            ctx,
        )?);
    }
    let Some(ctor) = args.get(1).and_then(JsValue::as_object) else {
        return Err(JsNativeError::typ()
            .with_message("customElements.define requires a constructor")
            .into());
    };
    if !ctor.is_constructor() {
        return Err(JsNativeError::typ()
            .with_message("customElements.define requires a constructor")
            .into());
    }
    let already_defined = {
        let shared = state(ctx);
        let data = shared.borrow();
        data.observers_wc
            .ce_defs
            .iter()
            .any(|d| d.name == name || d.constructor == ctor)
    };
    if already_defined {
        return Err(dom_exception::exception(
            "NotSupportedError",
            "Custom element name or constructor has already been defined",
            ctx,
        )?);
    }

    let proto_val = ctor.get(js_string!("prototype"), ctx)?;
    let Some(prototype) = proto_val.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("Custom element constructor prototype must be an object")
            .into());
    };

    let obs_attrs_val = ctor.get(js_string!("observedAttributes"), ctx)?;
    let mut observed_attributes = Vec::new();
    if !obs_attrs_val.is_undefined() && !obs_attrs_val.is_null() {
        let Some(aobj) = obs_attrs_val.as_object() else {
            return Err(JsNativeError::typ()
                .with_message("observedAttributes must be an iterable/array")
                .into());
        };
        let len = aobj.get(js_string!("length"), ctx)?.to_u32(ctx)?.min(128);
        for i in 0..len {
            let item = aobj.get(i, ctx)?;
            observed_attributes.push(utf8(&item, ctx)?.to_ascii_lowercase());
            modern::check_context(ctx)?;
        }
    }

    let mut extends = None;
    if let Some(opts) = args.get(2).and_then(JsValue::as_object) {
        let ext_v = opts.get(js_string!("extends"), ctx)?;
        if !ext_v.is_undefined() {
            extends = Some(utf8(&ext_v, ctx)?.to_ascii_lowercase());
            modern::check_context(ctx)?;
        }
    }

    let (def_idx, resolvers) = {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        if data.observers_wc.ce_defs.len() >= MAX_CE_DEFS {
            return Err(failure("Custom element definition limit exceeded"));
        }
        if data
            .observers_wc
            .ce_defs
            .iter()
            .any(|d| d.name == name || d.constructor == ctor)
        {
            drop(data);
            return Err(dom_exception::exception(
                "NotSupportedError",
                "Custom element name or constructor has already been defined",
                ctx,
            )?);
        }
        data.host.charge(256 + name.len()).map_err(failure)?;
        let idx = data.observers_wc.ce_defs.len();
        data.observers_wc.ce_defs.push(CustomElementDef {
            name: name.clone(),
            extends,
            constructor: ctor.clone(),
            prototype,
            observed_attributes,
        });
        let mut matched = Vec::new();
        let mut i = 0;
        while i < data.observers_wc.pending_when_defined.len() {
            if data.observers_wc.pending_when_defined[i].name == name {
                let removed = data.observers_wc.pending_when_defined.remove(i);
                matched.push(removed.resolve.clone());
            } else {
                i += 1;
            }
        }
        (idx, matched)
    };

    // Upgrade existing matching elements in document tree order!
    let candidates = collect_unupgraded_candidates_in_subtree(0, ctx);
    for (nid, cand_def_idx) in candidates {
        if cand_def_idx == def_idx {
            let connected = is_connected_in_ctx(nid, ctx);
            upgrade_element_with_def(nid, def_idx, connected, ctx)?;
        }
    }
    after_dom_mutation(ctx)?;

    for resolve in resolvers {
        resolve.call(&JsValue::undefined(), &[ctor.clone().into()], ctx)?;
        modern::check_context(ctx)?;
    }
    Ok(JsValue::undefined())
}

pub(super) fn construct_html_element(
    new_target: &JsValue,
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let Some(nt_obj) = new_target.as_object() else {
        return Err(failure("Illegal constructor"));
    };
    let (def_idx, tag_name, existing_node) = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let Some(def_idx) = data
            .observers_wc
            .ce_defs
            .iter()
            .position(|d| d.constructor == nt_obj)
        else {
            return Err(failure("Illegal constructor"));
        };
        let tag = data.observers_wc.ce_defs[def_idx]
            .extends
            .clone()
            .unwrap_or_else(|| data.observers_wc.ce_defs[def_idx].name.clone());
        let from_stack = match data.observers_wc.construction_stack.last_mut() {
            Some(entry) if entry.constructor == nt_obj => match entry.node.take() {
                Some(nid) => Some(Ok(nid)),
                None => Some(Err(
                    "Custom element super() already called for this upgrade",
                )),
            },
            _ => None,
        };
        (def_idx, tag, from_stack)
    };

    let node_id = match existing_node {
        Some(Ok(nid)) => nid,
        Some(Err(msg)) => {
            return Err(dom_exception::exception("InvalidStateError", msg, context)?);
        }
        None => {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let nid = data
                .host
                .new_node(tag_name.clone(), String::new())
                .map_err(failure)?;
            let def_name = data.observers_wc.ce_defs[def_idx].name.clone();
            if data.observers_wc.ce_defs[def_idx].extends.is_some() {
                data.host.attr(nid, "is", def_name).map_err(failure)?;
            }
            nid
        }
    };

    let wrapper = wrap(&format!("node:{node_id}"), context)?;
    let proto_val = nt_obj.get(js_string!("prototype"), context)?;
    if let (Some(w_obj), Some(p_obj)) = (wrapper.as_object(), proto_val.as_object()) {
        set_object_prototype(&w_obj, &p_obj, context)?;
        let shared = state(context);
        let mut data = shared.borrow_mut();
        data.observers_wc.upgraded_nodes.insert(node_id, def_idx);
        data.observers_wc
            .upgraded_wrappers
            .insert(node_id, w_obj.clone());
    } else {
        state(context)
            .borrow_mut()
            .observers_wc
            .upgraded_nodes
            .insert(node_id, def_idx);
    }
    Ok(wrapper)
}

fn set_object_prototype(obj: &JsObject, proto: &JsObject, ctx: &mut Context) -> JsResult<()> {
    let reflect = ctx.intrinsics().objects().reflect();
    if let Some(set_proto) = reflect
        .get(js_string!("setPrototypeOf"), ctx)?
        .as_callable()
    {
        set_proto.call(
            &reflect.into(),
            &[obj.clone().into(), proto.clone().into()],
            ctx,
        )?;
    }
    Ok(())
}

pub(super) fn upgraded_prototype(data: &State, node_id: usize) -> Option<JsObject> {
    let &def_idx = data.observers_wc.upgraded_nodes.get(&node_id)?;
    data.observers_wc
        .ce_defs
        .get(def_idx)
        .map(|d| d.prototype.clone())
}

fn find_matching_def_for_node(data: &State, node_id: usize) -> Option<usize> {
    if data.observers_wc.ce_defs.is_empty()
        || data.observers_wc.upgraded_nodes.contains_key(&node_id)
    {
        return None;
    }
    let node = data.host.document.nodes.get(node_id)?;
    if node.tag.starts_with('#') {
        return None;
    }
    let is_attr = node.attr("is");
    data.observers_wc
        .ce_defs
        .iter()
        .position(|d| match &d.extends {
            None => d.name == node.tag,
            Some(ext) => ext == &node.tag && is_attr == Some(d.name.as_str()),
        })
}

fn collect_unupgraded_candidates_in_subtree(root: usize, ctx: &Context) -> Vec<(usize, usize)> {
    let shared = state(ctx);
    let data = shared.borrow();
    if data.observers_wc.ce_defs.is_empty() || root >= data.host.document.nodes.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut stack = vec![root];
    for _ in 0..crate::document::MAX_NODES {
        let Some(id) = stack.pop() else { break };
        if id != 0
            && let Some(def_idx) = find_matching_def_for_node(&data, id)
        {
            out.push((id, def_idx));
        }
        if data.host.document.nodes[id].tag != "template" {
            stack.extend(data.host.document.nodes[id].children.iter().rev().copied());
        }
    }
    out
}

fn is_connected_in_ctx(id: usize, ctx: &Context) -> bool {
    let shared = state(ctx);
    let data = shared.borrow();
    dom_ops::is_connected_node(&data.host.document.nodes, id)
}

fn upgrade_element_with_def(
    node_id: usize,
    def_idx: usize,
    fire_connected: bool,
    ctx: &mut Context,
) -> JsResult<()> {
    let (ctor, proto, obs_attrs, initial_attrs) = {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        if data.observers_wc.upgraded_nodes.contains_key(&node_id) {
            return Ok(());
        }
        let Some(def) = data.observers_wc.ce_defs.get(def_idx) else {
            return Ok(());
        };
        let ctor = def.constructor.clone();
        let proto = def.prototype.clone();
        let obs_attrs = def.observed_attributes.clone();
        let attrs: Vec<(String, String)> = data.host.document.nodes[node_id]
            .attributes
            .iter()
            .filter(|(k, _)| k != crate::document::INTERNAL_CANVAS_ATTR)
            .cloned()
            .collect();
        data.observers_wc
            .construction_stack
            .push(ConstructionEntry {
                node: Some(node_id),
                constructor: ctor.clone(),
            });
        (ctor, proto, obs_attrs, attrs)
    };

    let expected_wrapper = wrap(&format!("node:{node_id}"), ctx)?;
    let construct_res = ctor.construct(&[], None, ctx);
    {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        data.observers_wc.construction_stack.pop();
    }
    let constructed = construct_res?;
    modern::check_context(ctx)?;
    if expected_wrapper.as_object().as_ref() != Some(&constructed) {
        return Err(JsNativeError::typ()
            .with_message("Custom element constructor returned a different object")
            .into());
    }
    set_object_prototype(&constructed, &proto, ctx)?;
    {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        data.observers_wc.upgraded_nodes.insert(node_id, def_idx);
        data.observers_wc
            .upgraded_wrappers
            .insert(node_id, constructed);
    }

    // Fire attributeChangedCallback for existing observed attributes
    for (aname, aval) in initial_attrs {
        if obs_attrs.iter().any(|o| o == &aname) {
            invoke_ce_attribute_changed(node_id, &aname, None, Some(&aval), ctx)?;
        }
    }
    if fire_connected && is_connected_in_ctx(node_id, ctx) {
        invoke_ce_lifecycle(node_id, "connectedCallback", ctx)?;
    }
    Ok(())
}

pub(super) fn upgrade_created_element_if_defined(
    node_id: usize,
    is_attr: Option<&str>,
    context: &mut Context,
) -> JsResult<()> {
    let def_idx = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        if let Some(is_val) = is_attr
            && !is_val.is_empty()
        {
            data.host
                .attr(node_id, "is", is_val.to_ascii_lowercase())
                .map_err(failure)?;
        }
        find_matching_def_for_node(&data, node_id)
    };
    if let Some(idx) = def_idx {
        upgrade_element_with_def(node_id, idx, false, context)?;
    }
    Ok(())
}

fn invoke_ce_lifecycle(node_id: usize, method_name: &str, ctx: &mut Context) -> JsResult<()> {
    let el = wrap(&format!("node:{node_id}"), ctx)?;
    let Some(el_obj) = el.as_object() else {
        return Ok(());
    };
    let cb_val = el_obj.get(JsString::from(method_name), ctx)?;
    modern::check_context(ctx)?;
    if let Some(cb) = cb_val.as_callable() {
        cb.call(&el, &[], ctx)?;
        modern::check_context(ctx)?;
    }
    Ok(())
}

fn invoke_ce_attribute_changed(
    node_id: usize,
    name: &str,
    old_val: Option<&str>,
    new_val: Option<&str>,
    ctx: &mut Context,
) -> JsResult<()> {
    let el = wrap(&format!("node:{node_id}"), ctx)?;
    let Some(el_obj) = el.as_object() else {
        return Ok(());
    };
    let cb_val = el_obj.get(js_string!("attributeChangedCallback"), ctx)?;
    modern::check_context(ctx)?;
    if let Some(cb) = cb_val.as_callable() {
        let old_js = old_val
            .map(|s| JsString::from(s).into())
            .unwrap_or_else(JsValue::null);
        let new_js = new_val
            .map(|s| JsString::from(s).into())
            .unwrap_or_else(JsValue::null);
        cb.call(
            &el,
            &[JsString::from(name).into(), old_js, new_js, JsValue::null()],
            ctx,
        )?;
        modern::check_context(ctx)?;
    }
    Ok(())
}

// ============================================================================
// Mutation & Custom Element Recording Hooks (Borrow-Safe)
// ============================================================================

pub(super) fn record_child_list_mutation(
    data: &mut State,
    parent: usize,
    added: &[usize],
    removed: &[usize],
    prev_sibling: Option<usize>,
    next_sibling: Option<usize>,
    parent_was_connected: bool,
) {
    if added.is_empty() && removed.is_empty() {
        return;
    }
    if !data.observers_wc.resize_observers.is_empty()
        || !data.observers_wc.intersection_observers.is_empty()
    {
        data.observers_wc.layout_dirty = true;
    }

    // 1. Record MutationRecords for observers watching `parent` or subtree ancestors
    if !data.observers_wc.mutation_observers.is_empty() {
        let chain = inclusive_ancestors(&data.host.document.nodes, parent);
        for obs in &mut data.observers_wc.mutation_observers {
            if obs.records.len() >= MAX_QUEUED_MUTATION_RECORDS {
                continue;
            }
            let matches = obs.registrations.iter().any(|r| {
                r.child_list && (r.target == parent || (r.subtree && chain.contains(&r.target)))
            });
            if matches {
                obs.records.push(PendingMutationRecord {
                    target: parent,
                    kind: PendingMutationKind::ChildList {
                        added: added.to_vec(),
                        removed: removed.to_vec(),
                        prev_sibling,
                        next_sibling,
                    },
                });
            }
        }
    }

    // 2. Record Custom Element reactions (`Disconnected`, `Upgrade`, `Connected`)
    if !data.observers_wc.ce_defs.is_empty() {
        if parent_was_connected && !removed.is_empty() {
            for &rem_root in removed {
                let mut stack = vec![rem_root];
                for _ in 0..crate::document::MAX_NODES {
                    let Some(nid) = stack.pop() else { break };
                    if data.observers_wc.upgraded_nodes.contains_key(&nid) {
                        data.observers_wc
                            .pending_ce_reactions
                            .push(CeReaction::Disconnected(nid));
                    }
                    if data.host.document.nodes[nid].tag != "template" {
                        stack.extend(data.host.document.nodes[nid].children.iter().rev().copied());
                    }
                }
            }
        }
        let parent_now_connected = dom_ops::is_connected_node(&data.host.document.nodes, parent);
        if parent_now_connected && !added.is_empty() {
            for &add_root in added {
                let mut stack = vec![add_root];
                for _ in 0..crate::document::MAX_NODES {
                    let Some(nid) = stack.pop() else { break };
                    if data.observers_wc.upgraded_nodes.contains_key(&nid) {
                        data.observers_wc
                            .pending_ce_reactions
                            .push(CeReaction::Connected(nid));
                    } else if find_matching_def_for_node(data, nid).is_some() {
                        data.observers_wc
                            .pending_ce_reactions
                            .push(CeReaction::Upgrade(nid));
                    }
                    if data.host.document.nodes[nid].tag != "template" {
                        stack.extend(data.host.document.nodes[nid].children.iter().rev().copied());
                    }
                }
            }
        }
    }
}

pub(super) fn record_attribute_mutation(
    data: &mut State,
    node_id: usize,
    attr_name: &str,
    old_value: Option<String>,
    new_value: Option<String>,
) {
    if attr_name == crate::document::INTERNAL_CANVAS_ATTR {
        return;
    }
    let lower = attr_name.to_ascii_lowercase();
    if !data.observers_wc.resize_observers.is_empty()
        || !data.observers_wc.intersection_observers.is_empty()
    {
        data.observers_wc.layout_dirty = true;
    }

    if !data.observers_wc.mutation_observers.is_empty() {
        let chain = inclusive_ancestors(&data.host.document.nodes, node_id);
        for obs in &mut data.observers_wc.mutation_observers {
            if obs.records.len() >= MAX_QUEUED_MUTATION_RECORDS {
                continue;
            }
            let mut matched = false;
            let mut wants_old = false;
            for r in &obs.registrations {
                if !r.attributes {
                    continue;
                }
                if r.target != node_id && !(r.subtree && chain.contains(&r.target)) {
                    continue;
                }
                if let Some(filter) = &r.attribute_filter
                    && !filter.iter().any(|f| f == &lower)
                {
                    continue;
                }
                matched = true;
                if r.attribute_old_value {
                    wants_old = true;
                }
            }
            if matched {
                obs.records.push(PendingMutationRecord {
                    target: node_id,
                    kind: PendingMutationKind::Attributes {
                        name: lower.clone(),
                        old_value: if wants_old { old_value.clone() } else { None },
                    },
                });
            }
        }
    }

    if let Some(&def_idx) = data.observers_wc.upgraded_nodes.get(&node_id)
        && let Some(def) = data.observers_wc.ce_defs.get(def_idx)
        && def.observed_attributes.iter().any(|a| a == &lower)
    {
        data.observers_wc
            .pending_ce_reactions
            .push(CeReaction::AttributeChanged {
                node: node_id,
                name: lower,
                old_value,
                new_value,
            });
    }
}

pub(super) fn record_character_data_mutation(data: &mut State, node_id: usize, old_value: String) {
    if !data.observers_wc.resize_observers.is_empty()
        || !data.observers_wc.intersection_observers.is_empty()
    {
        data.observers_wc.layout_dirty = true;
    }
    if data.observers_wc.mutation_observers.is_empty() {
        return;
    }
    let chain = inclusive_ancestors(&data.host.document.nodes, node_id);
    for obs in &mut data.observers_wc.mutation_observers {
        if obs.records.len() >= MAX_QUEUED_MUTATION_RECORDS {
            continue;
        }
        let mut matched = false;
        let mut wants_old = false;
        for r in &obs.registrations {
            if !r.character_data {
                continue;
            }
            if r.target != node_id && !(r.subtree && chain.contains(&r.target)) {
                continue;
            }
            matched = true;
            if r.character_data_old_value {
                wants_old = true;
            }
        }
        if matched {
            obs.records.push(PendingMutationRecord {
                target: node_id,
                kind: PendingMutationKind::CharacterData {
                    old_value: if wants_old {
                        Some(old_value.clone())
                    } else {
                        None
                    },
                },
            });
        }
    }
}

fn inclusive_ancestors(nodes: &[crate::document::Node], mut id: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for _ in 0..=crate::document::MAX_DEPTH {
        if id >= nodes.len() {
            break;
        }
        out.push(id);
        let p = nodes[id].parent;
        if id == 0 || p == id {
            break;
        }
        id = p;
    }
    out
}

// ============================================================================
// Post-Mutation CEReactions & Microtask Scheduling
// ============================================================================

pub(super) fn after_dom_mutation(context: &mut Context) -> JsResult<()> {
    let (has_ce, need_mo_job, need_layout_job) = {
        let shared = state(context);
        let data = shared.borrow();
        let s = &data.observers_wc;
        let has_ce = !s.ce_draining && !s.pending_ce_reactions.is_empty();
        let need_mo =
            !s.mutation_job_scheduled && s.mutation_observers.iter().any(|o| !o.records.is_empty());
        let need_lo = !s.layout_job_scheduled
            && s.layout_dirty
            && (!s.resize_observers.is_empty() || !s.intersection_observers.is_empty());
        (has_ce, need_mo, need_lo)
    };

    if has_ce {
        drain_ce_reactions(context)?;
    }

    // Re-check observer flags in case CE reactions mutated DOM!
    let (need_mo_after, need_lo_after) = {
        let shared = state(context);
        let data = shared.borrow();
        let s = &data.observers_wc;
        let need_mo = need_mo_job
            || (!s.mutation_job_scheduled
                && s.mutation_observers.iter().any(|o| !o.records.is_empty()));
        let need_lo = need_layout_job
            || (!s.layout_job_scheduled
                && s.layout_dirty
                && (!s.resize_observers.is_empty() || !s.intersection_observers.is_empty()));
        (need_mo, need_lo)
    };

    if need_mo_after {
        ensure_mutation_observer_job(context)?;
    }
    if need_lo_after {
        ensure_layout_observer_job(context)?;
    }
    Ok(())
}

fn drain_ce_reactions(ctx: &mut Context) -> JsResult<()> {
    {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        if data.observers_wc.ce_draining {
            return Ok(());
        }
        data.observers_wc.ce_draining = true;
    }
    let res: JsResult<()> = (|| {
        for _ in 0..512 {
            let batch = {
                let shared = state(ctx);
                let mut data = shared.borrow_mut();
                if data.observers_wc.pending_ce_reactions.is_empty() {
                    break;
                }
                std::mem::take(&mut data.observers_wc.pending_ce_reactions)
            };
            for reaction in batch {
                match reaction {
                    CeReaction::Upgrade(nid) => {
                        let def_opt = {
                            let shared = state(ctx);
                            let data = shared.borrow();
                            find_matching_def_for_node(&data, nid)
                        };
                        if let Some(def_idx) = def_opt {
                            let connected = is_connected_in_ctx(nid, ctx);
                            upgrade_element_with_def(nid, def_idx, connected, ctx)?;
                        }
                    }
                    CeReaction::Connected(nid) => {
                        if is_connected_in_ctx(nid, ctx) {
                            invoke_ce_lifecycle(nid, "connectedCallback", ctx)?;
                        }
                    }
                    CeReaction::Disconnected(nid) => {
                        invoke_ce_lifecycle(nid, "disconnectedCallback", ctx)?;
                    }
                    CeReaction::AttributeChanged {
                        node,
                        name,
                        old_value,
                        new_value,
                    } => {
                        invoke_ce_attribute_changed(
                            node,
                            &name,
                            old_value.as_deref(),
                            new_value.as_deref(),
                            ctx,
                        )?;
                    }
                }
            }
        }
        Ok(())
    })();
    state(ctx).borrow_mut().observers_wc.ce_draining = false;
    res
}

fn ensure_mutation_observer_job(ctx: &mut Context) -> JsResult<()> {
    {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        if data.observers_wc.mutation_job_scheduled {
            return Ok(());
        }
        data.observers_wc.mutation_job_scheduled = true;
    }
    let realm = ctx.realm().clone();
    let job = GenericJob::new(flush_mutation_observers, realm);
    ctx.enqueue_job(Job::GenericJob(job));
    modern::check_context(ctx)?;
    Ok(())
}

fn flush_mutation_observers(ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    state(ctx).borrow_mut().observers_wc.mutation_job_scheduled = false;
    for _ in 0..64 {
        let deliveries: Vec<(JsObject, JsObject, Vec<PendingMutationRecord>)> = {
            let shared = state(ctx);
            let mut data = shared.borrow_mut();
            data.observers_wc
                .mutation_observers
                .iter_mut()
                .filter(|o| !o.records.is_empty())
                .map(|o| {
                    (
                        o.object.clone(),
                        o.callback.clone(),
                        std::mem::take(&mut o.records),
                    )
                })
                .collect()
        };
        if deliveries.is_empty() {
            break;
        }
        for (obs_obj, cb, recs) in deliveries {
            let arr = build_mutation_records_js(&recs, ctx)?;
            let obs_val: JsValue = obs_obj.into();
            cb.call(&obs_val.clone(), &[arr.into(), obs_val], ctx)?;
            modern::check_context(ctx)?;
        }
    }
    Ok(JsValue::undefined())
}

fn ensure_layout_observer_job(ctx: &mut Context) -> JsResult<()> {
    {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        if data.observers_wc.layout_job_scheduled {
            return Ok(());
        }
        data.observers_wc.layout_job_scheduled = true;
    }
    let realm = ctx.realm().clone();
    let job = GenericJob::new(flush_layout_observers, realm);
    ctx.enqueue_job(Job::GenericJob(job));
    modern::check_context(ctx)?;
    Ok(())
}

fn flush_layout_observers(ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        data.observers_wc.layout_job_scheduled = false;
        data.observers_wc.layout_dirty = false;
    }

    // 1. Evaluate ResizeObservers
    type ResizeDeliveryItem = (usize, (f64, f64, f64, f64), (f64, f64), f64);
    let ro_deliveries: Vec<(JsObject, JsObject, Vec<ResizeDeliveryItem>)> = {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        let dpr = data.viewport.device_pixel_ratio as f64;
        let ro_count = data.observers_wc.resize_observers.len();
        let mut out = Vec::new();
        for idx in 0..ro_count {
            let mut targets = std::mem::take(&mut data.observers_wc.resize_observers[idx].targets);
            let mut changed = Vec::new();
            for (tid, last_key) in &mut targets {
                let (border_box, content_rect) =
                    geometry_style::client_rect_and_boxes(&mut data, *tid)
                        .unwrap_or(((0.0, 0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 0.0)));
                let key = BoxSizeKey {
                    bw: border_box.2.to_bits(),
                    bh: border_box.3.to_bits(),
                    cw: content_rect.2.to_bits(),
                    ch: content_rect.3.to_bits(),
                };
                if *last_key != Some(key) {
                    *last_key = Some(key);
                    changed.push((*tid, content_rect, (border_box.2, border_box.3), dpr));
                }
            }
            let ro = &mut data.observers_wc.resize_observers[idx];
            ro.targets = targets;
            if !changed.is_empty() {
                out.push((ro.object.clone(), ro.callback.clone(), changed));
            }
        }
        out
    };

    for (obs_obj, cb, items) in ro_deliveries {
        let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
        let box_arr = |inl: f64, blk: f64, ctx: &mut Context| {
            let o = ObjectInitializer::new(ctx)
                .property(js_string!("inlineSize"), inl, ec)
                .property(js_string!("blockSize"), blk, ec)
                .build();
            JsArray::from_iter([o.into()], ctx)
        };
        let mut js_entries = Vec::with_capacity(items.len());
        for (tid, crect, (bw, bh), dpr) in items {
            let target_js = wrap(&format!("node:{tid}"), ctx)?;
            let cr_obj = geometry_style::build_dom_rect(crect.0, crect.1, crect.2, crect.3, ctx);
            let bsize_arr = box_arr(bw, bh, ctx);
            let csize_arr = box_arr(crect.2, crect.3, ctx);
            let dp_arr = box_arr((crect.2 * dpr).round(), (crect.3 * dpr).round(), ctx);
            let entry = ObjectInitializer::new(ctx)
                .property(js_string!("target"), target_js, ec)
                .property(js_string!("contentRect"), cr_obj, ec)
                .property(js_string!("borderBoxSize"), bsize_arr, ec)
                .property(js_string!("contentBoxSize"), csize_arr, ec)
                .property(js_string!("devicePixelContentBoxSize"), dp_arr, ec)
                .build();
            js_entries.push(entry.into());
        }
        let arr = JsArray::from_iter(js_entries, ctx);
        let obs_val: JsValue = obs_obj.into();
        cb.call(&obs_val.clone(), &[arr.into(), obs_val], ctx)?;
        modern::check_context(ctx)?;
    }

    // 2. Evaluate IntersectionObservers
    let io_deliveries: Vec<(JsObject, JsObject, Vec<PendingIntersectionEntry>)> = {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        let io_count = data.observers_wc.intersection_observers.len();
        let mut out = Vec::new();
        for idx in 0..io_count {
            evaluate_intersection_observer_index(&mut data, idx);
            let io = &mut data.observers_wc.intersection_observers[idx];
            if !io.records.is_empty() {
                out.push((
                    io.object.clone(),
                    io.callback.clone(),
                    std::mem::take(&mut io.records),
                ));
            }
        }
        out
    };

    for (obs_obj, cb, recs) in io_deliveries {
        let arr = build_io_entries_js(&recs, ctx)?;
        let obs_val: JsValue = obs_obj.into();
        cb.call(&obs_val.clone(), &[arr.into(), obs_val], ctx)?;
        modern::check_context(ctx)?;
    }

    Ok(JsValue::undefined())
}

// ============================================================================
// Web Animations API (`Element.prototype.animate` + `getAnimations`)
// ============================================================================

fn install_web_animations(
    global: &JsObject,
    is_extra_realm: bool,
    context: &mut Context,
) -> Result<(), String> {
    let anim_proto = JsObject::with_object_proto(context.intrinsics());
    let effect_proto = JsObject::with_object_proto(context.intrinsics());

    for (mname, fn_ptr) in [
        (
            "play",
            anim_play as fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
        ),
        ("pause", anim_pause),
        ("finish", anim_finish),
        ("cancel", anim_cancel),
        ("reverse", anim_reverse),
        ("updatePlaybackRate", anim_update_rate),
        ("commitStyles", anim_commit_styles),
        ("persist", anim_persist),
        ("addEventListener", anim_add_listener),
        ("removeEventListener", anim_remove_listener),
        ("dispatchEvent", anim_dispatch_event),
    ] {
        def_fn(&anim_proto, mname, 0, false, fn_ptr, context)
            .map_err(|_| "Animation method install failed")?;
    }

    let anim_ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|new_target, args, ctx| {
            modern::check_context(ctx)?;
            if new_target.is_undefined() {
                return Err(JsNativeError::typ()
                    .with_message("Animation constructor requires 'new'")
                    .into());
            }
            let effect = args.first().cloned().unwrap_or(JsValue::null());
            build_animation_instance(None, effect, String::new(), 0.0, "none", Vec::new(), ctx)
        }),
    )
    .name(js_string!("Animation"))
    .length(0)
    .constructor(true)
    .build()
    .into();

    let effect_ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|new_target, args, ctx| {
            modern::check_context(ctx)?;
            if new_target.is_undefined() {
                return Err(JsNativeError::typ()
                    .with_message("KeyframeEffect constructor requires 'new'")
                    .into());
            }
            let target = args.first().cloned().unwrap_or(JsValue::null());
            let kf_val = args.get(1).cloned().unwrap_or(JsValue::null());
            let opts_val = args.get(2).cloned().unwrap_or(JsValue::undefined());
            let (kf_arr, _) = parse_keyframes(&kf_val, ctx)?;
            let timing = parse_animation_options(&opts_val, ctx)?;
            build_keyframe_effect(target, kf_arr, &timing, ctx).map(Into::into)
        }),
    )
    .name(js_string!("KeyframeEffect"))
    .length(2)
    .constructor(true)
    .build()
    .into();

    link_ctor_global(global, "Animation", &anim_ctor, &anim_proto, context)?;
    link_ctor_global(
        global,
        "KeyframeEffect",
        &effect_ctor,
        &effect_proto,
        context,
    )?;

    if !is_extra_realm {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        data.observers_wc.animation_proto = Some(anim_proto);
        data.observers_wc.keyframe_effect_proto = Some(effect_proto);
    }
    Ok(())
}

struct ParsedTiming {
    id: String,
    duration: f64,
    delay: f64,
    end_delay: f64,
    iterations: f64,
    direction: String,
    easing: String,
    fill: String,
}

fn parse_animation_options(val: &JsValue, ctx: &mut Context) -> JsResult<ParsedTiming> {
    let mut t = ParsedTiming {
        id: String::new(),
        duration: 0.0,
        delay: 0.0,
        end_delay: 0.0,
        iterations: 1.0,
        direction: "normal".into(),
        easing: "linear".into(),
        fill: "none".into(),
    };
    if val.is_undefined() || val.is_null() {
        return Ok(t);
    }
    if let Some(obj) = val.as_object() {
        let dur_v = obj.get(js_string!("duration"), ctx)?;
        if !dur_v.is_undefined() {
            if let Some(s) = dur_v.as_string()
                && s.to_std_string_escaped() == "auto"
            {
                t.duration = 0.0;
            } else {
                let d = dur_v.to_number(ctx)?;
                if !d.is_finite() || d < 0.0 {
                    return Err(JsNativeError::typ()
                        .with_message("Animation duration must be non-negative")
                        .into());
                }
                t.duration = d;
            }
        }
        let delay_v = obj.get(js_string!("delay"), ctx)?;
        if !delay_v.is_undefined() {
            let d = delay_v.to_number(ctx)?;
            if d.is_finite() {
                t.delay = d;
            }
        }
        let ed_v = obj.get(js_string!("endDelay"), ctx)?;
        if !ed_v.is_undefined() {
            let d = ed_v.to_number(ctx)?;
            if d.is_finite() {
                t.end_delay = d;
            }
        }
        let it_v = obj.get(js_string!("iterations"), ctx)?;
        if !it_v.is_undefined() {
            let it = it_v.to_number(ctx)?;
            if it.is_nan() || it < 0.0 {
                return Err(JsNativeError::typ()
                    .with_message("Animation iterations must be non-negative")
                    .into());
            }
            t.iterations = it;
        }
        for (prop, dst) in [
            ("id", &mut t.id),
            ("direction", &mut t.direction),
            ("easing", &mut t.easing),
            ("fill", &mut t.fill),
        ] {
            let pv = obj.get(JsString::from(prop), ctx)?;
            if !pv.is_undefined() {
                *dst = utf8(&pv, ctx)?;
                modern::check_context(ctx)?;
            }
        }
    } else {
        let d = val.to_number(ctx)?;
        if !d.is_finite() || d < 0.0 {
            return Err(JsNativeError::typ()
                .with_message("Animation duration must be non-negative")
                .into());
        }
        t.duration = d;
    }
    Ok(t)
}

type KeyframePairs = Vec<Vec<(String, String)>>;

fn parse_keyframes(kf_val: &JsValue, ctx: &mut Context) -> JsResult<(JsArray, KeyframePairs)> {
    if kf_val.is_undefined() || kf_val.is_null() {
        return Ok((JsArray::new(ctx)?, Vec::new()));
    }
    let Some(obj) = kf_val.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("Keyframes must be an object or array")
            .into());
    };
    let len_val = obj.get(js_string!("length"), ctx)?;
    let mut frames_decls: Vec<Vec<(String, String)>> = Vec::new();
    let mut js_frames: Vec<JsValue> = Vec::new();

    if !len_val.is_undefined() {
        let len = len_val.to_u32(ctx)?.min(256);
        for i in 0..len {
            let frame_v = obj.get(i, ctx)?;
            let Some(fobj) = frame_v.as_object() else {
                continue;
            };
            let keys = fobj.own_property_keys(ctx)?;
            let mut decls = Vec::new();
            let frame_copy = JsObject::with_object_proto(ctx.intrinsics());
            for k in keys {
                let Some(kstr) = super::property_name(&k) else {
                    continue;
                };
                let pv = fobj.get(k, ctx)?;
                frame_copy.set(JsString::from(kstr.as_str()), pv.clone(), false, ctx)?;
                if matches!(kstr.as_str(), "offset" | "easing" | "composite") {
                    continue;
                }
                let css_prop = super::super::css_property_name(&kstr);
                let val_str = utf8(&pv, ctx)?;
                modern::check_context(ctx)?;
                decls.push((css_prop, val_str));
            }
            frames_decls.push(decls);
            js_frames.push(frame_copy.into());
        }
    } else {
        // Property-indexed keyframes: `{ opacity: [0, 1], transform: ['scale(0)', 'scale(1)'] }`
        let keys = obj.own_property_keys(ctx)?;
        let mut prop_series: Vec<(String, String, Vec<String>)> = Vec::new();
        let mut max_len = 0usize;
        for k in keys {
            let Some(kstr) = super::property_name(&k) else {
                continue;
            };
            if matches!(kstr.as_str(), "offset" | "easing" | "composite") {
                continue;
            }
            let pv = obj.get(k, ctx)?;
            let mut vals = Vec::new();
            if let Some(pobj) = pv.as_object() {
                let plen_v = pobj.get(js_string!("length"), ctx)?;
                if !plen_v.is_undefined() {
                    let plen = plen_v.to_u32(ctx)?.min(256) as usize;
                    for i in 0..plen {
                        let iv = pobj.get(i as u32, ctx)?;
                        vals.push(utf8(&iv, ctx)?);
                        modern::check_context(ctx)?;
                    }
                } else {
                    vals.push(utf8(&pv, ctx)?);
                    modern::check_context(ctx)?;
                }
            } else {
                vals.push(utf8(&pv, ctx)?);
                modern::check_context(ctx)?;
            }
            max_len = max_len.max(vals.len());
            prop_series.push((kstr.clone(), super::super::css_property_name(&kstr), vals));
        }
        for idx in 0..max_len {
            let mut decls = Vec::new();
            let frame_copy = JsObject::with_object_proto(ctx.intrinsics());
            for (camel, css_prop, vals) in &prop_series {
                if let Some(v) = vals.get(idx).or_else(|| vals.last()) {
                    frame_copy.set(
                        JsString::from(camel.as_str()),
                        JsString::from(v.as_str()),
                        false,
                        ctx,
                    )?;
                    decls.push((css_prop.clone(), v.clone()));
                }
            }
            frames_decls.push(decls);
            js_frames.push(frame_copy.into());
        }
    }
    Ok((JsArray::from_iter(js_frames, ctx), frames_decls))
}

fn build_keyframe_effect(
    target: JsValue,
    kf_arr: JsArray,
    timing: &ParsedTiming,
    ctx: &mut Context,
) -> JsResult<JsObject> {
    let proto = ctx
        .global_object()
        .get(js_string!("KeyframeEffect"), ctx)
        .ok()
        .and_then(|v| v.as_object())
        .and_then(|o| o.get(js_string!("prototype"), ctx).ok())
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| ctx.intrinsics().constructors().object().prototype());
    let eff = JsObject::from_proto_and_data(proto, ());
    eff.set(js_string!("target"), target, false, ctx)?;
    eff.set(js_string!("__mg_kf"), kf_arr, false, ctx)?;
    let active_dur = if timing.iterations.is_finite() {
        timing.duration * timing.iterations
    } else {
        timing.duration
    };
    let timing_obj = ObjectInitializer::new(ctx)
        .property(js_string!("duration"), timing.duration, Attribute::all())
        .property(js_string!("delay"), timing.delay, Attribute::all())
        .property(js_string!("endDelay"), timing.end_delay, Attribute::all())
        .property(
            js_string!("iterations"),
            timing.iterations,
            Attribute::all(),
        )
        .property(
            js_string!("direction"),
            JsString::from(timing.direction.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("easing"),
            JsString::from(timing.easing.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("fill"),
            JsString::from(timing.fill.as_str()),
            Attribute::all(),
        )
        .property(js_string!("activeDuration"), active_dur, Attribute::all())
        .property(
            js_string!("endTime"),
            timing.delay + active_dur + timing.end_delay,
            Attribute::all(),
        )
        .build();
    eff.set(js_string!("__mg_timing"), timing_obj, false, ctx)?;

    for mname in ["getKeyframes", "getTiming", "getComputedTiming"] {
        def_cap(
            &eff,
            mname,
            0,
            false,
            mname == "getKeyframes",
            |this, _, &is_kf, c| {
                let Some(o) = this.as_object() else {
                    return Ok(JsValue::undefined());
                };
                o.get(
                    if is_kf {
                        js_string!("__mg_kf")
                    } else {
                        js_string!("__mg_timing")
                    },
                    c,
                )
            },
            ctx,
        )?;
    }
    Ok(eff)
}

fn build_animation_instance(
    target_id: Option<usize>,
    effect: JsValue,
    id: String,
    active_duration: f64,
    fill: &str,
    terminal_decls: Vec<(String, String)>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let proto = ctx
        .global_object()
        .get(js_string!("Animation"), ctx)
        .ok()
        .and_then(|v| v.as_object())
        .and_then(|o| o.get(js_string!("prototype"), ctx).ok())
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| ctx.intrinsics().constructors().object().prototype());
    let anim = JsObject::from_proto_and_data(proto, ());
    let finished_p = JsPromise::resolve(anim.clone(), ctx)?;
    let ready_p = JsPromise::resolve(anim.clone(), ctx)?;
    modern::check_context(ctx)?;
    let fire_finish_fn = FunctionObjectBuilder::new(
        ctx.realm(),
        NativeFunction::from_fn_ptr(|_, args, c| {
            modern::check_context(c)?;
            if let Some(a) = args.first().and_then(JsValue::as_object) {
                let ps = utf8(&a.get(js_string!("playState"), c)?, c)?;
                if ps == "finished" {
                    dispatch_anim_event(&a, "finish", c)?;
                }
            }
            Ok(JsValue::undefined())
        }),
    )
    .build();
    let _ = finished_p.then(Some(fire_finish_fn), None, ctx)?;

    // Save pre-animation inline style if fill applies terminal styles
    let mut prev_style: Option<Option<String>> = None;
    let fill_retains = matches!(fill, "forwards" | "both");
    if let Some(tid) = target_id
        && fill_retains
        && !terminal_decls.is_empty()
    {
        prev_style = Some(apply_style_decls_to_node(tid, &terminal_decls, ctx)?);
    }

    let decls_arr = JsArray::new(ctx)?;
    for (p, v) in &terminal_decls {
        let pair = JsArray::from_iter(
            [
                JsString::from(p.as_str()).into(),
                JsString::from(v.as_str()).into(),
            ],
            ctx,
        );
        decls_arr.push(pair, ctx)?;
    }

    anim.set(js_string!("id"), JsString::from(id.as_str()), false, ctx)?;
    anim.set(js_string!("effect"), effect, false, ctx)?;
    anim.set(js_string!("playState"), js_string!("finished"), false, ctx)?;
    anim.set(js_string!("replaceState"), js_string!("active"), false, ctx)?;
    anim.set(js_string!("pending"), false, false, ctx)?;
    anim.set(js_string!("currentTime"), active_duration, false, ctx)?;
    anim.set(js_string!("startTime"), 0.0, false, ctx)?;
    anim.set(js_string!("playbackRate"), 1.0, false, ctx)?;
    anim.set(js_string!("finished"), finished_p, false, ctx)?;
    anim.set(js_string!("ready"), ready_p, false, ctx)?;
    anim.set(js_string!("onfinish"), JsValue::null(), false, ctx)?;
    anim.set(js_string!("oncancel"), JsValue::null(), false, ctx)?;
    anim.set(js_string!("onremove"), JsValue::null(), false, ctx)?;
    anim.set(js_string!("__mg_dur"), active_duration, false, ctx)?;
    anim.set(js_string!("__mg_fill"), fill_retains, false, ctx)?;
    anim.set(js_string!("__mg_persisted"), false, false, ctx)?;
    anim.set(js_string!("__mg_committed"), false, false, ctx)?;
    anim.set(js_string!("__mg_decls"), decls_arr, false, ctx)?;
    anim.set(js_string!("__mg_listeners"), JsArray::new(ctx)?, false, ctx)?;
    if let Some(tid) = target_id {
        anim.set(js_string!("__mg_tid"), tid as f64, false, ctx)?;
    }
    if let Some(prev_opt) = prev_style {
        anim.set(
            js_string!("__mg_prev_style"),
            prev_opt
                .map(|s| JsString::from(s.as_str()).into())
                .unwrap_or_else(JsValue::null),
            false,
            ctx,
        )?;
        anim.set(js_string!("__mg_has_prev"), true, false, ctx)?;
    }

    if let Some(tid) = target_id {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        if data.observers_wc.animations.len() < MAX_ANIMATIONS {
            data.host.charge(256).map_err(failure)?;
            data.observers_wc.animations.push(AnimationEntry {
                target: tid,
                object: anim.clone(),
            });
        }
    }
    Ok(anim.into())
}

fn apply_style_decls_to_node(
    tid: usize,
    decls: &[(String, String)],
    ctx: &mut Context,
) -> JsResult<Option<String>> {
    let old_style = {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        if tid >= data.host.document.nodes.len() {
            return Ok(None);
        }
        let base = data.host.base_url();
        let prev = data.host.document.nodes[tid]
            .attr("style")
            .map(str::to_owned);
        let mut current = prev.clone().unwrap_or_default();
        for (prop, val) in decls {
            if let Ok(crate::style::declarations::Mutation::Replace(next)) =
                crate::style::declarations::set_declared_value(&current, prop, val, "", &base)
            {
                current = next;
            }
        }
        let new_clone = current.clone();
        data.host.attr(tid, "style", current).map_err(failure)?;
        data.dom_epoch = data.dom_epoch.wrapping_add(1);
        record_attribute_mutation(&mut data, tid, "style", prev.clone(), Some(new_clone));
        prev
    };
    after_dom_mutation(ctx)?;
    Ok(old_style)
}

pub(super) fn call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    match name {
        EL_ANIMATE => {
            let tid = element_id_from_js(this, context)?;
            let kf_val = args.first().cloned().unwrap_or(JsValue::null());
            let opts_val = args.get(1).cloned().unwrap_or(JsValue::undefined());
            let (kf_arr, frames_decls) = parse_keyframes(&kf_val, context)?;
            let timing = parse_animation_options(&opts_val, context)?;
            let reversed = matches!(timing.direction.as_str(), "reverse" | "alternate-reverse");
            let terminal_decls = if reversed {
                frames_decls.first().cloned().unwrap_or_default()
            } else {
                frames_decls.last().cloned().unwrap_or_default()
            };
            let active_dur = if timing.iterations.is_finite() {
                timing.duration * timing.iterations
            } else {
                timing.duration
            };
            let eff = build_keyframe_effect(this.clone(), kf_arr, &timing, context)?;
            build_animation_instance(
                Some(tid),
                eff.into(),
                timing.id,
                active_dur,
                &timing.fill,
                terminal_decls,
                context,
            )
        }
        EL_GET_ANIMATIONS => {
            let tid = element_id_from_js(this, context)?;
            let include_subtree = if let Some(opts) = args.first().and_then(JsValue::as_object) {
                opts.get(js_string!("subtree"), context)?.to_boolean()
            } else {
                false
            };
            collect_animations_for_scope(Some((tid, include_subtree)), context)
        }
        DOC_GET_ANIMATIONS => {
            let doc_id = node_id_from_js(this, context)?;
            let is_doc = doc_id == 0
                || state(context)
                    .borrow()
                    .host
                    .document
                    .nodes
                    .get(doc_id)
                    .is_some_and(|n| n.tag == "#document-subroot");
            if !is_doc {
                return Err(JsNativeError::typ()
                    .with_message("Document.getAnimations requires a Document receiver")
                    .into());
            }
            collect_animations_for_scope(Some((doc_id, true)), context)
        }
        _ => Err(failure("Unknown animation method")),
    }
}

fn collect_animations_for_scope(
    scope: Option<(usize, bool)>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let candidates: Vec<JsObject> = {
        let shared = state(ctx);
        let data = shared.borrow();
        let allowed_nodes: Option<HashSet<usize>> = scope.map(|(root, subtree)| {
            if !subtree {
                let mut set = HashSet::new();
                set.insert(root);
                set
            } else {
                let mut set = HashSet::new();
                let mut stack = vec![root];
                while let Some(id) = stack.pop() {
                    set.insert(id);
                    stack.extend(data.host.document.nodes[id].children.iter().copied());
                }
                set
            }
        });
        data.observers_wc
            .animations
            .iter()
            .filter(|e| allowed_nodes.as_ref().is_none_or(|s| s.contains(&e.target)))
            .map(|e| e.object.clone())
            .collect()
    };
    let mut active = Vec::new();
    for anim in candidates {
        let ps = anim.get(js_string!("playState"), ctx)?;
        let ps_str = utf8(&ps, ctx)?;
        let fill = anim.get(js_string!("__mg_fill"), ctx)?.to_boolean();
        let persisted = anim.get(js_string!("__mg_persisted"), ctx)?.to_boolean();
        if ps_str == "running"
            || ps_str == "paused"
            || (ps_str == "finished" && (fill || persisted))
        {
            active.push(anim.into());
        }
    }
    Ok(JsArray::from_iter(active, ctx).into())
}

fn anim_obj(this: &JsValue) -> JsResult<JsObject> {
    this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Animation method requires an Animation receiver")
            .into()
    })
}

fn anim_play(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    let dur = anim.get(js_string!("__mg_dur"), ctx)?.to_number(ctx)?;
    anim.set(js_string!("playState"), js_string!("finished"), false, ctx)?;
    anim.set(js_string!("currentTime"), dur, false, ctx)?;
    anim.set(js_string!("startTime"), 0.0, false, ctx)?;
    Ok(JsValue::undefined())
}

fn anim_pause(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    anim.set(js_string!("playState"), js_string!("paused"), false, ctx)?;
    Ok(JsValue::undefined())
}

fn anim_finish(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    let dur = anim.get(js_string!("__mg_dur"), ctx)?.to_number(ctx)?;
    anim.set(js_string!("playState"), js_string!("finished"), false, ctx)?;
    anim.set(js_string!("currentTime"), dur, false, ctx)?;
    dispatch_anim_event(&anim, "finish", ctx)?;
    Ok(JsValue::undefined())
}

fn anim_cancel(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    let prev_ps = utf8(&anim.get(js_string!("playState"), ctx)?, ctx)?;
    anim.set(js_string!("playState"), js_string!("idle"), false, ctx)?;
    anim.set(js_string!("currentTime"), JsValue::null(), false, ctx)?;
    anim.set(js_string!("startTime"), JsValue::null(), false, ctx)?;
    let committed = anim.get(js_string!("__mg_committed"), ctx)?.to_boolean();
    let has_prev = anim.get(js_string!("__mg_has_prev"), ctx)?.to_boolean();
    let tid_v = anim.get(js_string!("__mg_tid"), ctx)?;
    if !committed
        && has_prev
        && let Some(tid_num) = tid_v.as_number()
    {
        let tid = tid_num as usize;
        let prev_v = anim.get(js_string!("__mg_prev_style"), ctx)?;
        let prev_s = if prev_v.is_null() || prev_v.is_undefined() {
            None
        } else {
            Some(utf8(&prev_v, ctx)?)
        };
        {
            let shared = state(ctx);
            let mut data = shared.borrow_mut();
            if tid < data.host.document.nodes.len() {
                let cur_old = data.host.document.nodes[tid]
                    .attr("style")
                    .map(str::to_owned);
                match &prev_s {
                    Some(s) => {
                        data.host.attr(tid, "style", s.clone()).map_err(failure)?;
                    }
                    None => {
                        data.host.document.nodes[tid]
                            .attributes
                            .retain(|(k, _)| k != "style");
                    }
                }
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
                record_attribute_mutation(&mut data, tid, "style", cur_old, prev_s);
            }
        }
        after_dom_mutation(ctx)?;
    }
    if prev_ps != "idle" {
        dispatch_anim_event(&anim, "cancel", ctx)?;
    }
    Ok(JsValue::undefined())
}

fn anim_reverse(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    let rate = anim.get(js_string!("playbackRate"), ctx)?.to_number(ctx)?;
    anim.set(js_string!("playbackRate"), -rate, false, ctx)?;
    Ok(JsValue::undefined())
}

fn anim_update_rate(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    let rate = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_number(ctx)?;
    if rate.is_finite() {
        anim.set(js_string!("playbackRate"), rate, false, ctx)?;
    }
    Ok(JsValue::undefined())
}

fn anim_commit_styles(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    let tid_v = anim.get(js_string!("__mg_tid"), ctx)?;
    let Some(tid_num) = tid_v.as_number() else {
        return Ok(JsValue::undefined());
    };
    let tid = tid_num as usize;
    let decls_v = anim.get(js_string!("__mg_decls"), ctx)?;
    let mut decls = Vec::new();
    if let Some(dobj) = decls_v.as_object() {
        let len = dobj.get(js_string!("length"), ctx)?.to_u32(ctx)?;
        for i in 0..len {
            if let Some(pair) = dobj.get(i, ctx)?.as_object() {
                let p = utf8(&pair.get(0u32, ctx)?, ctx)?;
                let v = utf8(&pair.get(1u32, ctx)?, ctx)?;
                decls.push((p, v));
            }
        }
    }
    if !decls.is_empty() {
        apply_style_decls_to_node(tid, &decls, ctx)?;
    }
    anim.set(js_string!("__mg_committed"), true, false, ctx)?;
    Ok(JsValue::undefined())
}

fn anim_persist(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    anim.set(
        js_string!("replaceState"),
        js_string!("persisted"),
        false,
        ctx,
    )?;
    anim.set(js_string!("__mg_persisted"), true, false, ctx)?;
    Ok(JsValue::undefined())
}

fn anim_add_listener(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    let etype = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
    let Some(cb) = args.get(1).and_then(JsValue::as_callable) else {
        return Ok(JsValue::undefined());
    };
    if let Some(list) = anim.get(js_string!("__mg_listeners"), ctx)?.as_object() {
        let arr = JsArray::from_object(list)?;
        let item = JsArray::from_iter([JsString::from(etype.as_str()).into(), cb.into()], ctx);
        arr.push(item, ctx)?;
    }
    Ok(JsValue::undefined())
}

fn anim_remove_listener(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    let etype = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
    let Some(cb) = args.get(1).and_then(JsValue::as_callable) else {
        return Ok(JsValue::undefined());
    };
    if let Some(list) = anim.get(js_string!("__mg_listeners"), ctx)?.as_object() {
        let len = list.get(js_string!("length"), ctx)?.to_u32(ctx)?;
        let next = JsArray::new(ctx)?;
        for i in 0..len {
            let entry = list.get(i, ctx)?;
            if let Some(eobj) = entry.as_object() {
                let t = utf8(&eobj.get(0u32, ctx)?, ctx)?;
                let c = eobj.get(1u32, ctx)?;
                if t == etype && c.as_object().as_ref() == Some(&cb) {
                    continue;
                }
            }
            next.push(entry, ctx)?;
        }
        anim.set(js_string!("__mg_listeners"), next, false, ctx)?;
    }
    Ok(JsValue::undefined())
}

fn anim_dispatch_event(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let anim = anim_obj(this)?;
    let Some(ev) = args.first().and_then(JsValue::as_object) else {
        return Err(JsNativeError::typ()
            .with_message("dispatchEvent requires an Event")
            .into());
    };
    let etype = utf8(&ev.get(js_string!("type"), ctx)?, ctx)?;
    dispatch_anim_event_obj(&anim, &etype, ev.into(), ctx)?;
    Ok(true.into())
}

fn dispatch_anim_event(anim: &JsObject, etype: &str, ctx: &mut Context) -> JsResult<()> {
    let ev = build_synthetic_event(etype, anim.clone().into(), ctx)?;
    if let Some(ev_obj) = ev.as_object() {
        let cur_time = anim.get(js_string!("currentTime"), ctx)?;
        ev_obj.set(js_string!("currentTime"), cur_time.clone(), false, ctx)?;
        ev_obj.set(js_string!("timelineTime"), cur_time, false, ctx)?;
    }
    dispatch_anim_event_obj(anim, etype, ev, ctx)
}

fn dispatch_anim_event_obj(
    anim: &JsObject,
    etype: &str,
    ev: JsValue,
    ctx: &mut Context,
) -> JsResult<()> {
    let handler_prop = format!("on{etype}");
    let on_fn = anim.get(JsString::from(handler_prop.as_str()), ctx)?;
    let this_v: JsValue = anim.clone().into();
    if let Some(cb) = on_fn.as_callable() {
        cb.call(&this_v, std::slice::from_ref(&ev), ctx)?;
        modern::check_context(ctx)?;
    }
    if let Some(list) = anim.get(js_string!("__mg_listeners"), ctx)?.as_object() {
        let len = list.get(js_string!("length"), ctx)?.to_u32(ctx)?;
        let mut to_call = Vec::new();
        for i in 0..len {
            if let Some(eobj) = list.get(i, ctx)?.as_object() {
                let t = utf8(&eobj.get(0u32, ctx)?, ctx)?;
                if t == etype
                    && let Some(cb) = eobj.get(1u32, ctx)?.as_callable()
                {
                    to_call.push(cb);
                }
            }
        }
        for cb in to_call {
            cb.call(&this_v, std::slice::from_ref(&ev), ctx)?;
            modern::check_context(ctx)?;
        }
    }
    Ok(())
}

// ============================================================================
// `window.visualViewport` (`VisualViewport`)
// ============================================================================

fn install_visual_viewport(
    global: &JsObject,
    is_extra_realm: bool,
    context: &mut Context,
) -> Result<(), String> {
    let proto = JsObject::with_object_proto(context.intrinsics());

    for prop in [
        "offsetLeft",
        "offsetTop",
        "pageLeft",
        "pageTop",
        "width",
        "height",
        "scale",
    ] {
        def_getter_cap(
            &proto,
            prop,
            true,
            prop,
            |_, _, &p, ctx| {
                modern::check_context(ctx)?;
                let vp = state(ctx).borrow().viewport;
                let v: f64 = match p {
                    "width" => vp.inner_width as f64,
                    "height" => vp.inner_height as f64,
                    "pageLeft" => vp.scroll_x as f64,
                    "pageTop" => vp.scroll_y as f64,
                    "scale" => 1.0,
                    _ => 0.0,
                };
                Ok(v.into())
            },
            context,
        )
        .map_err(|_| "VisualViewport getter install failed")?;
    }

    for (hprop, etype) in [("onresize", "resize"), ("onscroll", "scroll")] {
        def_get_set_cap(
            &proto,
            hprop,
            true,
            etype,
            |_, _, &ev_t, ctx| {
                modern::check_context(ctx)?;
                let cb = state(ctx)
                    .borrow()
                    .observers_wc
                    .vv_listeners
                    .iter()
                    .find(|l| l.property && l.event_type == ev_t)
                    .map(|l| l.callback.clone());
                Ok(cb.map(Into::into).unwrap_or_else(JsValue::null))
            },
            |_, args, &ev_t, ctx| {
                modern::check_context(ctx)?;
                let cb_opt = args.first().and_then(JsValue::as_callable);
                let shared = state(ctx);
                let mut data = shared.borrow_mut();
                data.observers_wc
                    .vv_listeners
                    .retain(|l| !(l.property && l.event_type == ev_t));
                if let Some(cb) = cb_opt {
                    data.host.charge(128).map_err(failure)?;
                    data.observers_wc.vv_listeners.push(VvListener {
                        event_type: ev_t.to_owned(),
                        callback: cb,
                        once: false,
                        property: true,
                    });
                }
                Ok(JsValue::undefined())
            },
            context,
        )
        .map_err(|_| "VisualViewport event handler install failed")?;
    }

    def_fn(
        &proto,
        "addEventListener",
        2,
        false,
        |_, args, ctx| {
            modern::check_context(ctx)?;
            let etype = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            modern::check_context(ctx)?;
            let Some(cb) = args.get(1).and_then(JsValue::as_callable) else {
                return Ok(JsValue::undefined());
            };
            let once = if let Some(opts) = args.get(2).and_then(JsValue::as_object) {
                opts.get(js_string!("once"), ctx)?.to_boolean()
            } else {
                false
            };
            let shared = state(ctx);
            let mut data = shared.borrow_mut();
            if data.observers_wc.vv_listeners.len() >= 256 {
                return Err(failure("VisualViewport listener limit exceeded"));
            }
            if !data
                .observers_wc
                .vv_listeners
                .iter()
                .any(|l| !l.property && l.event_type == etype && l.callback == cb)
            {
                data.host.charge(128).map_err(failure)?;
                data.observers_wc.vv_listeners.push(VvListener {
                    event_type: etype,
                    callback: cb,
                    once,
                    property: false,
                });
            }
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "VisualViewport.addEventListener failed")?;

    def_fn(
        &proto,
        "removeEventListener",
        2,
        false,
        |_, args, ctx| {
            modern::check_context(ctx)?;
            let etype = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            modern::check_context(ctx)?;
            let Some(cb) = args.get(1).and_then(JsValue::as_callable) else {
                return Ok(JsValue::undefined());
            };
            state(ctx)
                .borrow_mut()
                .observers_wc
                .vv_listeners
                .retain(|l| !(!l.property && l.event_type == etype && l.callback == cb));
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "VisualViewport.removeEventListener failed")?;

    def_fn(
        &proto,
        "dispatchEvent",
        1,
        false,
        |this, args, ctx| {
            modern::check_context(ctx)?;
            let Some(ev) = args.first().and_then(JsValue::as_object) else {
                return Err(JsNativeError::typ()
                    .with_message("dispatchEvent requires an Event")
                    .into());
            };
            let etype = utf8(&ev.get(js_string!("type"), ctx)?, ctx)?;
            modern::check_context(ctx)?;
            dispatch_vv_event_with_obj(this.clone(), &etype, ev.into(), ctx)?;
            Ok(true.into())
        },
        context,
    )
    .map_err(|_| "VisualViewport.dispatchEvent failed")?;

    let ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|_, _, ctx| {
            modern::check_context(ctx)?;
            Err(failure("Illegal constructor"))
        }),
    )
    .name(js_string!("VisualViewport"))
    .length(0)
    .constructor(true)
    .build()
    .into();

    link_ctor_global(global, "VisualViewport", &ctor, &proto, context)?;

    let vv_inst = JsObject::from_proto_and_data(proto, ());
    if !is_extra_realm {
        state(context)
            .borrow_mut()
            .observers_wc
            .main_visual_viewport = Some(vv_inst.clone());
    }
    def_prop(
        global,
        js_string!("visualViewport"),
        vv_inst,
        false,
        true,
        true,
        context,
    )
    .map_err(|_| "window.visualViewport install failed".to_owned())
}

fn dispatch_vv_event_if_observed(etype: &str, ctx: &mut Context) -> JsResult<()> {
    let vv_opt = {
        let shared = state(ctx);
        let data = shared.borrow();
        if !data
            .observers_wc
            .vv_listeners
            .iter()
            .any(|l| l.event_type == etype)
        {
            return Ok(());
        }
        data.observers_wc.main_visual_viewport.clone()
    };
    let Some(vv_obj) = vv_opt else {
        return Ok(());
    };
    let target_v: JsValue = vv_obj.into();
    let ev = build_synthetic_event(etype, target_v.clone(), ctx)?;
    dispatch_vv_event_with_obj(target_v, etype, ev, ctx)
}

fn dispatch_vv_event_with_obj(
    target_v: JsValue,
    etype: &str,
    ev: JsValue,
    ctx: &mut Context,
) -> JsResult<()> {
    let callbacks: Vec<JsObject> = {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        let mut out = Vec::new();
        let mut i = 0;
        while i < data.observers_wc.vv_listeners.len() {
            if data.observers_wc.vv_listeners[i].event_type == etype {
                out.push(data.observers_wc.vv_listeners[i].callback.clone());
                if data.observers_wc.vv_listeners[i].once {
                    data.observers_wc.vv_listeners.remove(i);
                    continue;
                }
            }
            i += 1;
        }
        out
    };
    for cb in callbacks {
        cb.call(&target_v, std::slice::from_ref(&ev), ctx)?;
        modern::check_context(ctx)?;
    }
    Ok(())
}

pub(super) fn notify_scroll_changed(is_window_scroll: bool, context: &mut Context) -> JsResult<()> {
    if is_window_scroll {
        dispatch_vv_event_if_observed("scroll", context)?;
    }
    let need_lo = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        if !data.observers_wc.intersection_observers.is_empty() {
            data.observers_wc.layout_dirty = true;
            !data.observers_wc.layout_job_scheduled
        } else {
            false
        }
    };
    if need_lo {
        ensure_layout_observer_job(context)?;
    }
    Ok(())
}

pub(super) fn on_viewport_sync(
    realm: &mut BoaPageRealm,
    old_vp: page_session::platform::ViewportState,
    new_vp: page_session::platform::ViewportState,
    errors: &mut Vec<String>,
) {
    let size_changed =
        old_vp.inner_width != new_vp.inner_width || old_vp.inner_height != new_vp.inner_height;
    let scroll_changed = (old_vp.scroll_x - new_vp.scroll_x).abs() > 1e-4
        || (old_vp.scroll_y - new_vp.scroll_y).abs() > 1e-4;
    if !size_changed && !scroll_changed {
        return;
    }

    let (fire_vv_resize, fire_vv_scroll, run_layout_observers) = {
        let mut data = realm.state.borrow_mut();
        let s = &mut data.observers_wc;
        let vv_r = size_changed && s.vv_listeners.iter().any(|l| l.event_type == "resize");
        let vv_s = scroll_changed && s.vv_listeners.iter().any(|l| l.event_type == "scroll");
        let has_ro = size_changed && !s.resize_observers.is_empty();
        let has_io = !s.intersection_observers.is_empty();
        if has_ro || has_io {
            s.layout_dirty = true;
        }
        (vv_r, vv_s, has_ro || has_io)
    };

    if !fire_vv_resize && !fire_vv_scroll && !run_layout_observers {
        return;
    }

    let prev_world = enter_world_in_context(0, realm.engine.context_mut());
    {
        let _ingress = Ingress::enter(realm.state.clone());
        let ctx = realm.engine.context_mut();
        if fire_vv_resize && let Err(e) = dispatch_vv_event_if_observed("resize", ctx) {
            let _ = e;
        }
        if fire_vv_scroll && let Err(e) = dispatch_vv_event_if_observed("scroll", ctx) {
            let _ = e;
        }
        if run_layout_observers && let Err(e) = ensure_layout_observer_job(ctx) {
            let _ = e;
        }
    }
    enter_world_in_context(prev_world, realm.engine.context_mut());
    realm.checkpoint(errors);
}

// ============================================================================
// Synthetic Event Helper
// ============================================================================

fn build_synthetic_event(etype: &str, target: JsValue, ctx: &mut Context) -> JsResult<JsValue> {
    let ts = dom_events::elapsed_ms(&state(ctx).borrow());
    let noop = FunctionObjectBuilder::new(
        ctx.realm(),
        NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
    )
    .build();
    let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
    let wc = Attribute::WRITABLE | Attribute::CONFIGURABLE;
    let mut ev = ObjectInitializer::new(ctx);
    ev.property(js_string!("type"), JsString::from(etype), ec)
        .property(js_string!("target"), target.clone(), ec)
        .property(js_string!("currentTarget"), target, ec)
        .property(js_string!("isTrusted"), true, ec)
        .property(js_string!("timeStamp"), ts, ec);
    for b in ["bubbles", "cancelable", "defaultPrevented"] {
        ev.property(JsString::from(b), false, ec);
    }
    for m in [
        "preventDefault",
        "stopPropagation",
        "stopImmediatePropagation",
    ] {
        ev.property(JsString::from(m), noop.clone(), wc);
    }
    Ok(ev.build().into())
}
