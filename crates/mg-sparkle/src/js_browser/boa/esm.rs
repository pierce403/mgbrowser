//! Browser-hosted ECMAScript Module (`<script type="module">`) support for `BoaPageRealm`.
//!
//! Preserves standalone `BoaPageRealm::start` / `start_with_observer` / `boa::execute` classic
//! behavior (`modules_enabled == false`) while enabling full ES module parsing, relative and
//! `<script type="importmap">` specifier resolution, live namespace bindings, `import.meta.url`,
//! and dynamic `import()` in browser-hosted `BoaPageRealm::start_with_platform` sessions without
//! modifying `mg-butane` or granting worker file/network capabilities.

use super::{BoaPageRealm, Ingress, failure, modern, state, utf8};
use crate::js_browser::extract_static_module_specifiers;
use modern::ExecutionFailure;
use modern::boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsString, JsSymbol, JsValue, NativeFunction, Source,
    builtins::promise::PromiseState,
    js_string,
    module::Module,
    object::{
        FunctionObjectBuilder, JsObject, ObjectInitializer,
        builtins::{JsArray, JsPromise},
    },
    property::{Attribute, PropertyDescriptor},
};
use modern::boa_gc::{Finalize, Trace};
use std::collections::{HashMap, HashSet};

#[derive(Default, Trace, Finalize)]
pub(super) struct EsmState {
    #[unsafe_ignore_trace]
    pub enabled: bool,
    #[unsafe_ignore_trace]
    pub page_url: String,
    #[unsafe_ignore_trace]
    pub importmap: HashMap<String, String>,
    #[unsafe_ignore_trace]
    pub sources: HashMap<String, String>,
    #[unsafe_ignore_trace]
    pub evaluated: HashSet<String>,
    #[unsafe_ignore_trace]
    pub evaluating: HashSet<String>,
    pub namespaces: HashMap<String, JsObject>,
    pub eval_errors: HashMap<String, JsValue>,
}

impl EsmState {
    pub(super) fn clear(&mut self) {
        self.sources.clear();
        self.evaluated.clear();
        self.evaluating.clear();
        self.namespaces.clear();
        self.eval_errors.clear();
        self.importmap.clear();
    }
}

pub(super) fn parse_importmap_json(json_text: &str, base_url: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str::<serde_json::Value>(json_text)
    else {
        return map;
    };
    let Some(serde_json::Value::Object(imports)) = root.get("imports") else {
        return map;
    };
    let base = url::Url::parse(base_url).ok();
    for (key, val) in imports {
        if key.is_empty() || map.len() >= 256 {
            continue;
        }
        let Some(target) = val.as_str() else {
            continue;
        };
        if let Some(resolved) = resolve_url_like(target, base.as_ref()) {
            map.insert(key.clone(), resolved);
        }
    }
    map
}

fn resolve_url_like(spec: &str, base: Option<&url::Url>) -> Option<String> {
    let trimmed = spec.trim();
    if trimmed.starts_with('/') || trimmed.starts_with("./") || trimmed.starts_with("../") {
        let mut joined = base?.join(trimmed).ok()?;
        joined.set_fragment(None);
        return Some(joined.into());
    }
    let mut parsed = url::Url::parse(trimmed).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.set_fragment(None);
    Some(parsed.into())
}

pub(super) fn resolve_module_specifier(
    specifier: &str,
    referrer_url: &str,
    esm: &EsmState,
) -> Result<String, String> {
    if let Some(mapped) = esm.importmap.get(specifier) {
        return check_same_origin(mapped, &esm.page_url);
    }
    // Check prefix mappings ending with `/` (longest prefix wins).
    let mut best_prefix: Option<(&str, &str)> = None;
    for (k, v) in &esm.importmap {
        if k.ends_with('/')
            && specifier.starts_with(k.as_str())
            && v.ends_with('/')
            && best_prefix.is_none_or(|(prev, _)| k.len() > prev.len())
        {
            best_prefix = Some((k.as_str(), v.as_str()));
        }
    }
    if let Some((prefix, target_prefix)) = best_prefix {
        let rest = &specifier[prefix.len()..];
        if let Ok(base) = url::Url::parse(target_prefix)
            && let Ok(mut joined) = base.join(rest)
        {
            joined.set_fragment(None);
            return check_same_origin(joined.as_str(), &esm.page_url);
        }
    }

    let base_str = if referrer_url.is_empty() {
        esm.page_url.as_str()
    } else {
        referrer_url
    };
    let base = url::Url::parse(base_str).ok();
    let Some(resolved) = resolve_url_like(specifier, base.as_ref()) else {
        return Err(format!(
            "Failed to resolve module specifier \"{specifier}\". Relative references must start with either \"/\", \"./\", or \"../\"."
        ));
    };
    check_same_origin(&resolved, &esm.page_url)
}

fn check_same_origin(resolved_url: &str, page_url: &str) -> Result<String, String> {
    let resolved = url::Url::parse(resolved_url).map_err(|e| e.to_string())?;
    if let Ok(page) = url::Url::parse(page_url)
        && matches!(page.scheme(), "http" | "https")
        && resolved.origin() != page.origin()
    {
        return Err(format!(
            "Cross-origin module import is not allowed in same-origin module profile: {resolved_url}"
        ));
    }
    Ok(resolved_url.to_owned())
}

pub(super) fn evaluate_inline_or_root_module(
    realm: &mut BoaPageRealm,
    source_code: &str,
    module_url: &str,
    is_external: bool,
) -> Result<JsValue, ExecutionFailure> {
    if is_external && !module_url.is_empty() {
        let ns = evaluate_module_by_url(realm, module_url, Some(source_code))?;
        return Ok(ns.into());
    }
    let ns = evaluate_module_source(realm, source_code, module_url, None)?;
    Ok(ns.into())
}

fn evaluate_module_by_url(
    realm: &mut BoaPageRealm,
    canonical_url: &str,
    override_source: Option<&str>,
) -> Result<JsObject, ExecutionFailure> {
    {
        let data = realm.state.borrow();
        if let Some(err_val) = data.esm.eval_errors.get(canonical_url).cloned() {
            let js_err = JsError::from_opaque(err_val);
            return Err(ExecutionFailure {
                diagnostic: modern::safe_error(&js_err),
                exception: Some(js_err),
            });
        }
        if (data.esm.evaluated.contains(canonical_url)
            || data.esm.evaluating.contains(canonical_url))
            && let Some(ns) = data.esm.namespaces.get(canonical_url).cloned()
        {
            return Ok(ns);
        }
    }

    let source_code = match override_source {
        Some(s) => s.to_owned(),
        None => {
            let found = realm.state.borrow().esm.sources.get(canonical_url).cloned();
            let Some(src) = found else {
                return Err(type_error_failure(&format!(
                    "Failed to load module script: {canonical_url}"
                )));
            };
            src
        }
    };

    let ns_obj = create_module_namespace_shell(realm.engine.context_mut())?;
    {
        let mut data = realm.state.borrow_mut();
        data.esm
            .namespaces
            .insert(canonical_url.to_owned(), ns_obj.clone());
        data.esm.evaluating.insert(canonical_url.to_owned());
    }

    let res = evaluate_module_source(realm, &source_code, canonical_url, Some(ns_obj.clone()));
    match res {
        Ok(ns) => {
            let mut data = realm.state.borrow_mut();
            data.esm.evaluating.remove(canonical_url);
            data.esm.evaluated.insert(canonical_url.to_owned());
            Ok(ns)
        }
        Err(failure) => {
            let mut data = realm.state.borrow_mut();
            data.esm.evaluating.remove(canonical_url);
            if let Some(exc) = &failure.exception
                && let Ok(opaque) = exc.clone().into_opaque(realm.engine.context_mut())
            {
                data.esm
                    .eval_errors
                    .insert(canonical_url.to_owned(), opaque);
            }
            Err(failure)
        }
    }
}

fn type_error_failure(msg: &str) -> ExecutionFailure {
    let err: JsError = JsNativeError::typ().with_message(msg.to_owned()).into();
    ExecutionFailure {
        diagnostic: modern::safe_error(&err),
        exception: Some(err),
    }
}

fn syntax_error_failure(msg: &str) -> ExecutionFailure {
    let err: JsError = JsNativeError::syntax().with_message(msg.to_owned()).into();
    ExecutionFailure {
        diagnostic: modern::safe_error(&err),
        exception: Some(err),
    }
}

fn create_module_namespace_shell(ctx: &mut Context) -> Result<JsObject, ExecutionFailure> {
    let ns = JsObject::with_null_proto();
    ns.define_property_or_throw(
        JsSymbol::to_string_tag(),
        PropertyDescriptor::builder()
            .value(js_string!("Module"))
            .writable(false)
            .enumerable(false)
            .configurable(false),
        ctx,
    )
    .map_err(|err| ExecutionFailure {
        diagnostic: modern::safe_error(&err),
        exception: Some(err),
    })?;
    Ok(ns)
}

fn evaluate_module_source(
    realm: &mut BoaPageRealm,
    source_code: &str,
    module_url: &str,
    existing_ns: Option<JsObject>,
) -> Result<JsObject, ExecutionFailure> {
    realm
        .engine
        .check()
        .map_err(|diagnostic| ExecutionFailure {
            diagnostic,
            exception: None,
        })?;

    // 1. Validate 100% genuine ECMAScript Module grammar via boa_engine::Module::parse!
    let parse_res = {
        let _ingress = Ingress::enter(realm.state.clone());
        Module::parse(
            Source::from_bytes(source_code),
            None,
            realm.engine.context_mut(),
        )
    };
    if let Err(parse_err) = parse_res {
        // Charge empty evaluation so ResourceStats source counters stay monotonic.
        let _ = realm.engine.evaluate_raw("");
        return Err(ExecutionFailure {
            diagnostic: modern::safe_error(&parse_err),
            exception: Some(parse_err),
        });
    }

    // 2. Transform module syntax and collect static dependency specifiers.
    let transformed =
        transform_esm_source(source_code).map_err(|msg| syntax_error_failure(&msg))?;

    // 3. Resolve & evaluate all static dependencies in source order.
    let mut dep_objects: Vec<JsValue> = Vec::with_capacity(transformed.dependencies.len());
    for spec in &transformed.dependencies {
        let resolved_url = {
            let data = realm.state.borrow();
            resolve_module_specifier(spec, module_url, &data.esm)
        };
        let resolved_url = match resolved_url {
            Ok(u) => u,
            Err(msg) => {
                let _ = realm.engine.evaluate_raw("");
                return Err(type_error_failure(&msg));
            }
        };
        let dep_ns = evaluate_module_by_url(realm, &resolved_url, None)?;
        dep_objects.push(dep_ns.into());
    }

    // 4. Prepare exports namespace, deps array, import.meta, and dynamic import().
    let ns_obj = match existing_ns {
        Some(ns) => ns,
        None => create_module_namespace_shell(realm.engine.context_mut())?,
    };
    let (deps_array, import_meta, dyn_import, scope_obj) = {
        let ctx = realm.engine.context_mut();
        let deps = JsArray::from_iter(dep_objects, ctx);
        let meta = JsObject::with_null_proto();
        let effective_url = if module_url.is_empty() {
            state(ctx).borrow().esm.page_url.clone()
        } else {
            module_url.to_owned()
        };
        meta.define_property_or_throw(
            js_string!("url"),
            PropertyDescriptor::builder()
                .value(JsString::from(effective_url.as_str()))
                .writable(true)
                .enumerable(true)
                .configurable(true),
            ctx,
        )
        .map_err(|err| ExecutionFailure {
            diagnostic: modern::safe_error(&err),
            exception: Some(err),
        })?;
        let dyn_fn = build_dynamic_import_fn(&effective_url, ctx);
        let scope = JsObject::with_null_proto();
        (deps, meta, dyn_fn, scope)
    };

    // 5. Evaluate the compiled strict-mode module closure via Engine::evaluate_raw + call_raw.
    let call_res = {
        let _ingress = Ingress::enter(realm.state.clone());
        let fn_val = realm.engine.evaluate_raw(&transformed.wrapper_code)?;
        let Some(fn_obj) = fn_val.as_object() else {
            return Err(syntax_error_failure("Internal module wrapper failed"));
        };
        realm.engine.call_raw(
            &fn_obj,
            &JsValue::undefined(),
            &[
                scope_obj.into(),
                ns_obj.clone().into(),
                deps_array.into(),
                import_meta.into(),
                dyn_import.into(),
            ],
        )?
    };

    // If top-level await was used, drain jobs and inspect the returned Promise.
    if transformed.has_top_level_await
        && let Some(promise_obj) = call_res.as_object()
        && let Ok(js_promise) = JsPromise::from_object(promise_obj.clone())
    {
        let _ = realm.engine.checkpoint_raw();
        if let PromiseState::Rejected(reason) = js_promise.state() {
            let js_err = JsError::from_opaque(reason);
            return Err(ExecutionFailure {
                diagnostic: modern::safe_error(&js_err),
                exception: Some(js_err),
            });
        }
    }

    Ok(ns_obj)
}

fn build_dynamic_import_fn(referrer_url: &str, ctx: &mut Context) -> JsObject {
    let ref_holder = ObjectInitializer::new(ctx)
        .property(
            js_string!("ref"),
            JsString::from(referrer_url),
            Attribute::READONLY,
        )
        .build();
    let raw_fn = FunctionObjectBuilder::new(
        ctx.realm(),
        NativeFunction::from_fn_ptr(|this, args, ctx| {
            modern::check_context(ctx)?;
            let referrer = this
                .as_object()
                .and_then(|o| o.get(js_string!("ref"), ctx).ok())
                .and_then(|v| v.as_string().map(|s| s.to_std_string_lossy()))
                .unwrap_or_default();
            let spec = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            modern::check_context(ctx)?;
            let resolved = {
                let shared = state(ctx);
                let data = shared.borrow();
                resolve_module_specifier(&spec, &referrer, &data.esm)
            };
            let promise_ctor = ctx.intrinsics().constructors().promise().constructor();
            match resolved {
                Err(msg) => {
                    let err: JsError = JsNativeError::typ().with_message(msg).into();
                    let opaque = err.into_opaque(ctx)?;
                    let reject_fn = promise_ctor
                        .get(js_string!("reject"), ctx)?
                        .as_callable()
                        .ok_or_else(|| failure("Promise.reject unavailable"))?;
                    reject_fn.call(&promise_ctor.into(), &[opaque], ctx)
                }
                Ok(url) => {
                    let existing = state(ctx).borrow().esm.namespaces.get(&url).cloned();
                    if let Some(ns) = existing {
                        let resolve_fn = promise_ctor
                            .get(js_string!("resolve"), ctx)?
                            .as_callable()
                            .ok_or_else(|| failure("Promise.resolve unavailable"))?;
                        return resolve_fn.call(&promise_ctor.into(), &[ns.into()], ctx);
                    }
                    // Check if source exists in admitted module map; if not, fetch same-origin dynamically!
                    let source_opt = state(ctx).borrow().esm.sources.get(&url).cloned();
                    let Some(src) = source_opt else {
                        if let Some(fetch_fn) = ctx
                            .global_object()
                            .get(js_string!("fetch"), ctx)?
                            .as_callable()
                        {
                            let fetch_promise = fetch_fn.call(
                                &JsValue::undefined(),
                                &[JsString::from(url.as_str()).into()],
                                ctx,
                            )?;
                            if let Some(fp_obj) = fetch_promise.as_object()
                                && let Some(then_fn) =
                                    fp_obj.get(js_string!("then"), ctx)?.as_callable()
                            {
                                let step1 = FunctionObjectBuilder::new(
                                    ctx.realm(),
                                    NativeFunction::from_fn_ptr(|_, fargs, c| {
                                        modern::check_context(c)?;
                                        let Some(resp) =
                                            fargs.first().and_then(JsValue::as_object)
                                        else {
                                            return Err(JsNativeError::typ()
                                                .with_message(
                                                    "Failed to fetch dynamically imported module",
                                                )
                                                .into());
                                        };
                                        if !resp.get(js_string!("ok"), c)?.to_boolean() {
                                            return Err(JsNativeError::typ()
                                                .with_message(
                                                    "Failed to fetch dynamically imported module",
                                                )
                                                .into());
                                        }
                                        if let Some(hdrs) =
                                            resp.get(js_string!("headers"), c)?.as_object()
                                            && let Some(get_h) =
                                                hdrs.get(js_string!("get"), c)?.as_callable()
                                        {
                                            let ct = get_h.call(
                                                &hdrs.into(),
                                                &[js_string!("content-type").into()],
                                                c,
                                            )?;
                                            if !ct.is_null() && !ct.is_undefined() {
                                                let ct_s = utf8(&ct, c)?;
                                                let mime = ct_s
                                                    .split(';')
                                                    .next()
                                                    .unwrap_or("")
                                                    .trim()
                                                    .to_ascii_lowercase();
                                                if !mime.is_empty()
                                                    && !matches!(
                                                        mime.as_str(),
                                                        "text/javascript"
                                                            | "application/javascript"
                                                            | "application/x-javascript"
                                                            | "text/ecmascript"
                                                            | "application/ecmascript"
                                                    )
                                                {
                                                    return Err(JsNativeError::typ()
                                                        .with_message("Failed to fetch dynamically imported module: invalid MIME type")
                                                        .into());
                                                }
                                            }
                                        }
                                        let text_fn = resp
                                            .get(js_string!("text"), c)?
                                            .as_callable()
                                            .ok_or_else(|| failure("Response.text unavailable"))?;
                                        text_fn.call(&resp.into(), &[], c)
                                    }),
                                )
                                .build();
                                let text_promise =
                                    then_fn.call(&fp_obj.into(), &[step1.into()], ctx)?;
                                if let Some(tp_obj) = text_promise.as_object()
                                    && let Some(then2) =
                                        tp_obj.get(js_string!("then"), ctx)?.as_callable()
                                {
                                    let url_holder = ObjectInitializer::new(ctx)
                                        .property(
                                            js_string!("u"),
                                            JsString::from(url.as_str()),
                                            Attribute::READONLY,
                                        )
                                        .build();
                                    let step2_raw = FunctionObjectBuilder::new(
                                        ctx.realm(),
                                        NativeFunction::from_fn_ptr(|sthis, sargs, c| {
                                            modern::check_context(c)?;
                                            let mod_url = sthis
                                                .as_object()
                                                .and_then(|o| o.get(js_string!("u"), c).ok())
                                                .and_then(|v| {
                                                    v.as_string().map(|s| s.to_std_string_lossy())
                                                })
                                                .unwrap_or_default();
                                            let fetched_src = utf8(
                                                sargs.first().unwrap_or(&JsValue::undefined()),
                                                c,
                                            )?;
                                            modern::check_context(c)?;
                                            {
                                                let shared = state(c);
                                                let mut data = shared.borrow_mut();
                                                data.host
                                                    .charge(mod_url.len() + fetched_src.len() + 64)
                                                    .map_err(failure)?;
                                                data.esm
                                                    .sources
                                                    .insert(mod_url.clone(), fetched_src.clone());
                                            }
                                            evaluate_dynamic_source_in_ctx(&mod_url, &fetched_src, c)
                                        }),
                                    )
                                    .build();
                                    let step2 = if let Some(bind_fn) = step2_raw
                                        .get(js_string!("bind"), ctx)
                                        .ok()
                                        .and_then(|v| v.as_callable())
                                        && let Ok(bound) = bind_fn.call(
                                            &step2_raw.clone().into(),
                                            &[url_holder.into()],
                                            ctx,
                                        ) {
                                        bound
                                    } else {
                                        step2_raw.into()
                                    };
                                    return then2.call(&tp_obj.into(), &[step2], ctx);
                                }
                            }
                        }
                        let err: JsError = JsNativeError::typ()
                            .with_message(format!(
                                "Failed to fetch dynamically imported module: {url}"
                            ))
                            .into();
                        let opaque = err.into_opaque(ctx)?;
                        let reject_fn = promise_ctor
                            .get(js_string!("reject"), ctx)?
                            .as_callable()
                            .ok_or_else(|| failure("Promise.reject unavailable"))?;
                        return reject_fn.call(&promise_ctor.into(), &[opaque], ctx);
                    };
                    match evaluate_dynamic_source_in_ctx(&url, &src, ctx) {
                        Ok(ns_or_p) => {
                            let resolve_fn = promise_ctor
                                .get(js_string!("resolve"), ctx)?
                                .as_callable()
                                .ok_or_else(|| failure("Promise.resolve unavailable"))?;
                            resolve_fn.call(&promise_ctor.into(), &[ns_or_p], ctx)
                        }
                        Err(err) => {
                            let opaque = err.into_opaque(ctx)?;
                            let reject_fn = promise_ctor
                                .get(js_string!("reject"), ctx)?
                                .as_callable()
                                .ok_or_else(|| failure("Promise.reject unavailable"))?;
                            reject_fn.call(&promise_ctor.into(), &[opaque], ctx)
                        }
                    }
                }
            }
        }),
    )
    .name(js_string!("import"))
    .length(1)
    .build();
    let bind = raw_fn
        .get(js_string!("bind"), ctx)
        .ok()
        .and_then(|v| v.as_callable());
    if let Some(bind_fn) = bind
        && let Ok(bound) = bind_fn.call(&raw_fn.clone().into(), &[ref_holder.into()], ctx)
        && let Some(bound_obj) = bound.as_object()
    {
        return bound_obj;
    }
    raw_fn.into()
}

fn evaluate_dynamic_source_in_ctx(url: &str, src: &str, ctx: &mut Context) -> JsResult<JsValue> {
    if let Some(existing) = state(ctx).borrow().esm.namespaces.get(url).cloned() {
        return Ok(existing.into());
    }
    Module::parse(Source::from_bytes(src), None, ctx)?;
    let transformed =
        transform_esm_source(src).map_err(|msg| JsNativeError::syntax().with_message(msg))?;
    let mut dep_vals = Vec::with_capacity(transformed.dependencies.len());
    for dep_spec in &transformed.dependencies {
        let dep_url = {
            let shared = state(ctx);
            let data = shared.borrow();
            resolve_module_specifier(dep_spec, url, &data.esm)
                .map_err(|msg| JsNativeError::typ().with_message(msg))?
        };
        if let Some(dns) = state(ctx).borrow().esm.namespaces.get(&dep_url).cloned() {
            dep_vals.push(dns.into());
        } else if let Some(dep_src) = state(ctx).borrow().esm.sources.get(&dep_url).cloned() {
            let dns_val = evaluate_dynamic_source_in_ctx(&dep_url, &dep_src, ctx)?;
            dep_vals.push(dns_val);
        } else {
            return Err(JsNativeError::typ()
                .with_message(format!("Failed to resolve dependency: {dep_url}"))
                .into());
        }
    }
    let ns = JsObject::with_null_proto();
    ns.define_property_or_throw(
        JsSymbol::to_string_tag(),
        PropertyDescriptor::builder()
            .value(js_string!("Module"))
            .writable(false)
            .enumerable(false)
            .configurable(false),
        ctx,
    )?;
    let deps_arr = JsArray::from_iter(dep_vals, ctx);
    let meta = JsObject::with_null_proto();
    meta.define_property_or_throw(
        js_string!("url"),
        PropertyDescriptor::builder()
            .value(JsString::from(url))
            .writable(true)
            .enumerable(true)
            .configurable(true),
        ctx,
    )?;
    let inner_dyn = build_dynamic_import_fn(url, ctx);
    let scope = JsObject::with_null_proto();
    let parsed = modern::boa_engine::Script::parse(
        Source::from_bytes(&transformed.wrapper_code),
        None,
        ctx,
    )?;
    let fn_val = parsed.evaluate(ctx)?;
    let fn_obj = fn_val
        .as_object()
        .ok_or_else(|| failure("Internal dynamic module wrapper failed"))?;
    let call_res = fn_obj.call(
        &JsValue::undefined(),
        &[
            scope.into(),
            ns.clone().into(),
            deps_arr.into(),
            meta.into(),
            inner_dyn.into(),
        ],
        ctx,
    )?;
    {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        data.esm.namespaces.insert(url.to_owned(), ns.clone());
        data.esm.evaluated.insert(url.to_owned());
    }
    if transformed.has_top_level_await
        && let Some(p_obj) = call_res.as_object()
        && let Some(then_fn) = p_obj.get(js_string!("then"), ctx)?.as_callable()
    {
        let ns_holder = ObjectInitializer::new(ctx)
            .property(js_string!("ns"), ns.clone(), Attribute::READONLY)
            .build();
        let ret_ns_raw = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_fn_ptr(|sthis, _, c| {
                sthis
                    .as_object()
                    .ok_or_else(|| failure("Invalid module ns holder"))?
                    .get(js_string!("ns"), c)
            }),
        )
        .build();
        if let Some(bind_fn) = ret_ns_raw
            .get(js_string!("bind"), ctx)
            .ok()
            .and_then(|v| v.as_callable())
            && let Ok(bound) = bind_fn.call(&ret_ns_raw.into(), &[ns_holder.into()], ctx)
        {
            return then_fn.call(&p_obj.into(), &[bound], ctx);
        }
    }
    Ok(ns.into())
}

// --- Lexical JS Module Scanner & Rewriter ---

#[derive(Clone, Debug)]
struct JsToken {
    kind: TokKind,
    start: usize,
    end: usize,
    line_before: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokKind {
    Ident(String),
    StringLit(String),
    Punct(String),
    Other,
}

struct TransformedModule {
    dependencies: Vec<String>,
    wrapper_code: String,
    has_top_level_await: bool,
}

#[derive(Clone, Debug)]
enum ImportBinding {
    Namespace {
        dep_idx: usize,
        local: String,
    },
    Named {
        dep_idx: usize,
        imported: String,
        local: String,
    },
}

#[derive(Clone, Debug)]
enum ExportRecord {
    Local {
        local: String,
        exported: String,
    },
    Indirect {
        dep_idx: usize,
        imported: String,
        exported: String,
    },
    NamespaceFrom {
        dep_idx: usize,
        exported: String,
    },
    StarFrom {
        dep_idx: usize,
    },
}

fn can_precede_regex(prev: Option<&TokKind>) -> bool {
    match prev {
        None => true,
        Some(TokKind::Punct(p)) => matches!(
            p.as_str(),
            "(" | "["
                | "{"
                | ","
                | ";"
                | ":"
                | "?"
                | "="
                | "=="
                | "==="
                | "!="
                | "!=="
                | "+"
                | "-"
                | "*"
                | "**"
                | "%"
                | "&"
                | "&&"
                | "|"
                | "||"
                | "??"
                | "^"
                | "!"
                | "~"
                | "<"
                | "<="
                | "<<"
                | ">"
                | ">="
                | ">>"
                | ">>>"
                | "=>"
                | "+="
                | "-="
                | "*="
                | "/="
                | "%="
                | "&="
                | "|="
                | "^="
                | "&&="
                | "||="
                | "??="
        ),
        Some(TokKind::Ident(kw)) => matches!(
            kw.as_str(),
            "return"
                | "throw"
                | "case"
                | "delete"
                | "void"
                | "typeof"
                | "instanceof"
                | "in"
                | "of"
                | "new"
                | "yield"
                | "await"
                | "else"
                | "do"
        ),
        _ => false,
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '$' || ch == '_' || ch == '\\' || ch.is_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '$'
        || ch == '_'
        || ch == '\\'
        || ch == '\u{200C}'
        || ch == '\u{200D}'
        || ch.is_alphanumeric()
}

fn tokenize_js(src: &str) -> Vec<JsToken> {
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut tokens: Vec<JsToken> = Vec::new();
    let mut template_brace_stack: Vec<usize> = Vec::new();
    let mut brace_depth = 0usize;
    let mut saw_newline = false;

    while i < len {
        let b = bytes[i];
        if matches!(b, b' ' | b'\t' | b'\x0B' | b'\x0C') {
            i += 1;
            continue;
        }
        if b == b'\n' || b == b'\r' {
            saw_newline = true;
            i += 1;
            continue;
        }
        // Check UTF-8 line terminators U+2028 / U+2029 / BOM
        if b >= 0x80 {
            let ch = src[i..].chars().next().unwrap_or(' ');
            if matches!(ch, '\u{2028}' | '\u{2029}') {
                saw_newline = true;
                i += ch.len_utf8();
                continue;
            }
            if ch.is_whitespace() || ch == '\u{FEFF}' {
                i += ch.len_utf8();
                continue;
            }
        }
        // Hashbang at start of file
        if i == 0 && b == b'#' && bytes.get(1) == Some(&b'!') {
            while i < len && bytes[i] != b'\n' && bytes[i] != b'\r' {
                i += 1;
            }
            continue;
        }
        // Line comment
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < len && bytes[i] != b'\n' && bytes[i] != b'\r' {
                i += 1;
            }
            continue;
        }
        // Block comment
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    saw_newline = true;
                }
                i += 1;
            }
            i = (i + 2).min(len);
            continue;
        }

        let start = i;
        let line_before = saw_newline;
        saw_newline = false;

        // String literal
        if b == b'\'' || b == b'"' {
            let quote = b;
            i += 1;
            let mut val = String::new();
            while i < len {
                let ch = src[i..].chars().next().unwrap_or('\0');
                i += ch.len_utf8();
                if ch as u32 == quote as u32 {
                    break;
                }
                if ch == '\\' && i < len {
                    let esc = src[i..].chars().next().unwrap_or('\0');
                    i += esc.len_utf8();
                    match esc {
                        'n' => val.push('\n'),
                        'r' => val.push('\r'),
                        't' => val.push('\t'),
                        '\\' => val.push('\\'),
                        '\'' => val.push('\''),
                        '"' => val.push('"'),
                        _ => val.push(esc),
                    }
                } else {
                    val.push(ch);
                }
            }
            tokens.push(JsToken {
                kind: TokKind::StringLit(val),
                start,
                end: i,
                line_before,
            });
            continue;
        }

        // Template literal start
        if b == b'`' {
            i += 1;
            while i < len {
                let c = bytes[i];
                if c == b'\\' {
                    i = (i + 2).min(len);
                    continue;
                }
                if c == b'`' {
                    i += 1;
                    break;
                }
                if c == b'$' && bytes.get(i + 1) == Some(&b'{') {
                    i += 2;
                    template_brace_stack.push(brace_depth);
                    brace_depth += 1;
                    break;
                }
                i += 1;
            }
            tokens.push(JsToken {
                kind: TokKind::Other,
                start,
                end: i,
                line_before,
            });
            continue;
        }

        // Template continuation on closing `}`
        if b == b'}' && template_brace_stack.last().copied() == Some(brace_depth.saturating_sub(1))
        {
            brace_depth = brace_depth.saturating_sub(1);
            template_brace_stack.pop();
            i += 1;
            while i < len {
                let c = bytes[i];
                if c == b'\\' {
                    i = (i + 2).min(len);
                    continue;
                }
                if c == b'`' {
                    i += 1;
                    break;
                }
                if c == b'$' && bytes.get(i + 1) == Some(&b'{') {
                    i += 2;
                    template_brace_stack.push(brace_depth);
                    brace_depth += 1;
                    break;
                }
                i += 1;
            }
            tokens.push(JsToken {
                kind: TokKind::Other,
                start,
                end: i,
                line_before,
            });
            continue;
        }

        // Regex literal check
        if b == b'/' && can_precede_regex(tokens.last().map(|t| &t.kind)) {
            i += 1;
            let mut in_class = false;
            while i < len {
                let c = bytes[i];
                if c == b'\\' {
                    i = (i + 2).min(len);
                    continue;
                }
                if c == b'[' {
                    in_class = true;
                    i += 1;
                    continue;
                }
                if c == b']' {
                    in_class = false;
                    i += 1;
                    continue;
                }
                if c == b'/' && !in_class {
                    i += 1;
                    while i < len && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    break;
                }
                if c == b'\n' || c == b'\r' {
                    break;
                }
                i += 1;
            }
            tokens.push(JsToken {
                kind: TokKind::Other,
                start,
                end: i,
                line_before,
            });
            continue;
        }

        let ch = src[i..].chars().next().unwrap_or(' ');
        if is_ident_start(ch) {
            i += ch.len_utf8();
            while i < len {
                let nc = src[i..].chars().next().unwrap_or(' ');
                if is_ident_continue(nc) {
                    i += nc.len_utf8();
                } else {
                    break;
                }
            }
            tokens.push(JsToken {
                kind: TokKind::Ident(src[start..i].to_owned()),
                start,
                end: i,
                line_before,
            });
            continue;
        }

        if ch.is_ascii_digit() {
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'.' | b'_')) {
                i += 1;
            }
            tokens.push(JsToken {
                kind: TokKind::Other,
                start,
                end: i,
                line_before,
            });
            continue;
        }

        // Punctuators
        if b == b'{' {
            brace_depth += 1;
        } else if b == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
        }
        let punct = if i + 3 <= len
            && matches!(
                &src[i..i + 3],
                "===" | "!==" | ">>>" | "..." | "&&=" | "||=" | "??="
            ) {
            i += 3;
            &src[start..i]
        } else if i + 2 <= len
            && matches!(
                &src[i..i + 2],
                "=>" | "=="
                    | "!="
                    | "<="
                    | ">="
                    | "&&"
                    | "||"
                    | "??"
                    | "**"
                    | "++"
                    | "--"
                    | "+="
                    | "-="
                    | "*="
                    | "/="
                    | "%="
                    | "&="
                    | "|="
                    | "^="
                    | "<<"
                    | ">>"
                    | "?."
            )
        {
            i += 2;
            &src[start..i]
        } else {
            i += ch.len_utf8();
            &src[start..i]
        };
        tokens.push(JsToken {
            kind: TokKind::Punct(punct.to_owned()),
            start,
            end: i,
            line_before,
        });
    }
    tokens
}

fn intern_dep(deps: &mut Vec<String>, spec: &str) -> usize {
    if let Some(pos) = deps.iter().position(|s| s == spec) {
        pos
    } else {
        let idx = deps.len();
        deps.push(spec.to_owned());
        idx
    }
}

fn skip_import_attributes(tokens: &[JsToken], mut idx: usize) -> usize {
    // Optional `with { ... }` or `assert { ... }`
    if let Some(JsToken {
        kind: TokKind::Ident(kw),
        line_before: false,
        ..
    }) = tokens.get(idx)
        && matches!(kw.as_str(), "with" | "assert")
        && matches!(tokens.get(idx + 1).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == "{")
    {
        idx += 2;
        let mut depth = 1usize;
        while idx < tokens.len() && depth > 0 {
            if let TokKind::Punct(p) = &tokens[idx].kind {
                if p == "{" {
                    depth += 1;
                } else if p == "}" {
                    depth -= 1;
                }
            }
            idx += 1;
        }
    }
    if matches!(tokens.get(idx).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == ";") {
        idx += 1;
    }
    idx
}

fn export_or_ident_name(tok: &JsToken) -> Option<String> {
    match &tok.kind {
        TokKind::Ident(s) | TokKind::StringLit(s) => Some(s.clone()),
        _ => None,
    }
}

fn collect_pattern_identifiers(tokens: &[JsToken], mut idx: usize) -> (Vec<String>, usize) {
    let mut names = Vec::new();
    let Some(tok) = tokens.get(idx) else {
        return (names, idx);
    };
    match &tok.kind {
        TokKind::Ident(id) => {
            names.push(id.clone());
            idx += 1;
        }
        TokKind::Punct(p) if p == "{" => {
            idx += 1;
            while idx < tokens.len() {
                if matches!(&tokens[idx].kind, TokKind::Punct(p) if p == "}") {
                    idx += 1;
                    break;
                }
                if matches!(&tokens[idx].kind, TokKind::Punct(p) if p == ",") {
                    idx += 1;
                    continue;
                }
                if matches!(&tokens[idx].kind, TokKind::Punct(p) if p == "...") {
                    idx += 1;
                    let (sub, next) = collect_pattern_identifiers(tokens, idx);
                    names.extend(sub);
                    idx = next;
                    continue;
                }
                // Either `prop: target [= default]` or `shorthand [= default]` or `[computed]: target`
                if matches!(&tokens[idx].kind, TokKind::Punct(p) if p == "[") {
                    idx = skip_balanced(tokens, idx, "[", "]");
                    if matches!(tokens.get(idx).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == ":")
                    {
                        idx += 1;
                        let (sub, next) = collect_pattern_identifiers(tokens, idx);
                        names.extend(sub);
                        idx = skip_initializer(tokens, next);
                    }
                    continue;
                }
                if matches!(tokens.get(idx + 1).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == ":")
                {
                    idx += 2;
                    let (sub, next) = collect_pattern_identifiers(tokens, idx);
                    names.extend(sub);
                    idx = skip_initializer(tokens, next);
                } else if let TokKind::Ident(id) = &tokens[idx].kind {
                    names.push(id.clone());
                    idx = skip_initializer(tokens, idx + 1);
                } else {
                    idx += 1;
                }
            }
        }
        TokKind::Punct(p) if p == "[" => {
            idx += 1;
            while idx < tokens.len() {
                if matches!(&tokens[idx].kind, TokKind::Punct(p) if p == "]") {
                    idx += 1;
                    break;
                }
                if matches!(&tokens[idx].kind, TokKind::Punct(p) if p == ",") {
                    idx += 1;
                    continue;
                }
                if matches!(&tokens[idx].kind, TokKind::Punct(p) if p == "...") {
                    idx += 1;
                }
                let (sub, next) = collect_pattern_identifiers(tokens, idx);
                names.extend(sub);
                idx = skip_initializer(tokens, next);
            }
        }
        _ => {}
    }
    (names, idx)
}

fn skip_balanced(tokens: &[JsToken], mut idx: usize, open: &str, close: &str) -> usize {
    let mut depth = 0usize;
    while idx < tokens.len() {
        if let TokKind::Punct(p) = &tokens[idx].kind {
            if p == open {
                depth += 1;
            } else if p == close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return idx + 1;
                }
            }
        }
        idx += 1;
    }
    idx
}

fn skip_initializer(tokens: &[JsToken], mut idx: usize) -> usize {
    if !matches!(tokens.get(idx).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == "=") {
        return idx;
    }
    idx += 1;
    let mut depth = 0usize;
    while idx < tokens.len() {
        if let TokKind::Punct(p) = &tokens[idx].kind {
            match p.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                "," | ";" if depth == 0 => break,
                _ => {}
            }
        }
        idx += 1;
    }
    idx
}

fn skip_statement_end(tokens: &[JsToken], mut idx: usize) -> usize {
    let mut depth = 0usize;
    while idx < tokens.len() {
        if depth == 0 && idx > 0 && tokens[idx].line_before {
            let prev = &tokens[idx - 1].kind;
            let cur = &tokens[idx].kind;
            let continues = matches!(
                prev,
                TokKind::Punct(p)
                    if matches!(
                        p.as_str(),
                        "=" | "+" | "-" | "*" | "/" | "%" | "&&" | "||" | "??" | "." | "?."
                            | "," | "(" | "[" | "{" | "?" | ":" | "=>"
                    )
            ) || matches!(
                cur,
                TokKind::Punct(p) if matches!(p.as_str(), "." | "?." | ")" | "]" | "}")
            );
            if !continues {
                break;
            }
        }
        if let TokKind::Punct(p) = &tokens[idx].kind {
            match p.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => {
                    depth = depth.saturating_sub(1);
                }
                ";" if depth == 0 => return idx + 1,
                _ => {}
            }
        }
        idx += 1;
    }
    idx
}

fn transform_esm_source(src: &str) -> Result<TransformedModule, String> {
    let tokens = tokenize_js(src);
    let mut dependencies: Vec<String> = Vec::new();
    let mut imports: Vec<ImportBinding> = Vec::new();
    let mut exports: Vec<ExportRecord> = Vec::new();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut has_top_level_await = false;

    let mut depth = 0usize;
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = &tokens[i];
        // Check `import.meta` and `import(...)` at ANY nesting depth!
        if let TokKind::Ident(id) = &tok.kind
            && id == "import"
        {
            let prev_is_prop = i > 0
                && matches!(
                    &tokens[i - 1].kind,
                    TokKind::Punct(p) if p == "." || p == "?."
                );
            if !prev_is_prop {
                if matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == ".")
                    && matches!(tokens.get(i + 2).map(|t| &t.kind), Some(TokKind::Ident(m)) if m == "meta")
                {
                    edits.push((tok.start, tokens[i + 2].end, "__mg_import_meta".to_owned()));
                    i += 3;
                    continue;
                }
                if matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == "(")
                {
                    edits.push((tok.start, tok.end, "__mg_dynamic_import".to_owned()));
                    i += 1;
                    continue;
                }
            }
        }

        if depth == 0
            && let TokKind::Ident(id) = &tok.kind
        {
            let prev_is_prop = i > 0
                && matches!(
                    &tokens[i - 1].kind,
                    TokKind::Punct(p) if p == "." || p == "?."
                );
            if !prev_is_prop && id == "await" {
                has_top_level_await = true;
            }
            if !prev_is_prop && id == "import" {
                let stmt_start = tok.start;
                let mut cur = i + 1;
                // `import "spec"`
                if let Some(JsToken {
                    kind: TokKind::StringLit(spec),
                    ..
                }) = tokens.get(cur)
                {
                    let _ = intern_dep(&mut dependencies, spec);
                    cur = skip_import_attributes(&tokens, cur + 1);
                    let stmt_end = tokens[cur - 1].end;
                    edits.push((stmt_start, stmt_end, String::new()));
                    i = cur;
                    continue;
                }

                let mut pending_bindings: Vec<(Option<String>, String)> = Vec::new(); // (imported_or_none_for_ns, local)
                // Optional default import `import def`
                if let Some(JsToken {
                    kind: TokKind::Ident(def_local),
                    ..
                }) = tokens.get(cur)
                    && def_local != "from"
                {
                    pending_bindings.push((Some("default".into()), def_local.clone()));
                    cur += 1;
                    if matches!(tokens.get(cur).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == ",")
                    {
                        cur += 1;
                    }
                }

                // Optional `* as ns` or `{ ... }`
                if matches!(tokens.get(cur).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == "*") {
                    cur += 1; // `*`
                    cur += 1; // `as`
                    if let Some(JsToken {
                        kind: TokKind::Ident(ns_local),
                        ..
                    }) = tokens.get(cur)
                    {
                        pending_bindings.push((None, ns_local.clone()));
                        cur += 1;
                    }
                } else if matches!(tokens.get(cur).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == "{")
                {
                    cur += 1;
                    while cur < tokens.len() {
                        if matches!(&tokens[cur].kind, TokKind::Punct(p) if p == "}") {
                            cur += 1;
                            break;
                        }
                        if matches!(&tokens[cur].kind, TokKind::Punct(p) if p == ",") {
                            cur += 1;
                            continue;
                        }
                        let Some(imported) = export_or_ident_name(&tokens[cur]) else {
                            cur += 1;
                            continue;
                        };
                        cur += 1;
                        let local = if matches!(
                            tokens.get(cur).map(|t| &t.kind),
                            Some(TokKind::Ident(as_kw)) if as_kw == "as"
                        ) {
                            cur += 1;
                            let l = export_or_ident_name(&tokens[cur]).unwrap_or_default();
                            cur += 1;
                            l
                        } else {
                            imported.clone()
                        };
                        pending_bindings.push((Some(imported), local));
                    }
                }

                // Expect `from "spec"`
                if matches!(tokens.get(cur).map(|t| &t.kind), Some(TokKind::Ident(f)) if f == "from")
                    && let Some(JsToken {
                        kind: TokKind::StringLit(spec),
                        ..
                    }) = tokens.get(cur + 1)
                {
                    let dep_idx = intern_dep(&mut dependencies, spec);
                    for (imp_opt, local) in pending_bindings {
                        match imp_opt {
                            None => imports.push(ImportBinding::Namespace { dep_idx, local }),
                            Some(imported) => imports.push(ImportBinding::Named {
                                dep_idx,
                                imported,
                                local,
                            }),
                        }
                    }
                    cur = skip_import_attributes(&tokens, cur + 2);
                    let stmt_end = tokens[cur - 1].end;
                    edits.push((stmt_start, stmt_end, String::new()));
                    i = cur;
                    continue;
                }
            }

            if !prev_is_prop && id == "export" {
                let stmt_start = tok.start;
                let cur = i + 1;
                match tokens.get(cur).map(|t| &t.kind) {
                    // `export * from "spec"` or `export * as ns from "spec"`
                    Some(TokKind::Punct(p)) if p == "*" => {
                        let mut c = cur + 1;
                        let mut ns_export: Option<String> = None;
                        if matches!(tokens.get(c).map(|t| &t.kind), Some(TokKind::Ident(k)) if k == "as")
                        {
                            c += 1;
                            ns_export = tokens.get(c).and_then(export_or_ident_name);
                            c += 1;
                        }
                        if matches!(tokens.get(c).map(|t| &t.kind), Some(TokKind::Ident(k)) if k == "from")
                            && let Some(JsToken {
                                kind: TokKind::StringLit(spec),
                                ..
                            }) = tokens.get(c + 1)
                        {
                            let dep_idx = intern_dep(&mut dependencies, spec);
                            if let Some(exported) = ns_export {
                                exports.push(ExportRecord::NamespaceFrom { dep_idx, exported });
                            } else {
                                exports.push(ExportRecord::StarFrom { dep_idx });
                            }
                            c = skip_import_attributes(&tokens, c + 2);
                            edits.push((stmt_start, tokens[c - 1].end, String::new()));
                            i = c;
                            continue;
                        }
                    }
                    // `export { ... } [from "spec"]`
                    Some(TokKind::Punct(p)) if p == "{" => {
                        let mut c = cur + 1;
                        let mut items: Vec<(String, String)> = Vec::new();
                        while c < tokens.len() {
                            if matches!(&tokens[c].kind, TokKind::Punct(p) if p == "}") {
                                c += 1;
                                break;
                            }
                            if matches!(&tokens[c].kind, TokKind::Punct(p) if p == ",") {
                                c += 1;
                                continue;
                            }
                            let Some(first) = export_or_ident_name(&tokens[c]) else {
                                c += 1;
                                continue;
                            };
                            c += 1;
                            let second = if matches!(
                                tokens.get(c).map(|t| &t.kind),
                                Some(TokKind::Ident(k)) if k == "as"
                            ) {
                                c += 1;
                                let s = export_or_ident_name(&tokens[c]).unwrap_or_default();
                                c += 1;
                                s
                            } else {
                                first.clone()
                            };
                            items.push((first, second));
                        }
                        if matches!(tokens.get(c).map(|t| &t.kind), Some(TokKind::Ident(k)) if k == "from")
                            && let Some(JsToken {
                                kind: TokKind::StringLit(spec),
                                ..
                            }) = tokens.get(c + 1)
                        {
                            let dep_idx = intern_dep(&mut dependencies, spec);
                            for (imported, exported) in items {
                                exports.push(ExportRecord::Indirect {
                                    dep_idx,
                                    imported,
                                    exported,
                                });
                            }
                            c = skip_import_attributes(&tokens, c + 2);
                        } else {
                            for (local, exported) in items {
                                exports.push(ExportRecord::Local { local, exported });
                            }
                            if matches!(tokens.get(c).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == ";")
                            {
                                c += 1;
                            }
                        }
                        edits.push((stmt_start, tokens[c - 1].end, String::new()));
                        i = c;
                        continue;
                    }
                    // `export default ...`
                    Some(TokKind::Ident(kw)) if kw == "default" => {
                        let def_end = tokens[cur].end;
                        let after = cur + 1;
                        // Check `export default [async] function [*] [Name]` or `export default class [Name]`
                        let mut f_idx = after;
                        if matches!(tokens.get(f_idx).map(|t| &t.kind), Some(TokKind::Ident(k)) if k == "async")
                            && matches!(tokens.get(f_idx + 1).map(|t| &t.kind), Some(TokKind::Ident(k)) if k == "function" && !tokens[f_idx + 1].line_before)
                        {
                            f_idx += 1;
                        }
                        if matches!(tokens.get(f_idx).map(|t| &t.kind), Some(TokKind::Ident(k)) if k == "function")
                        {
                            let mut name_idx = f_idx + 1;
                            if matches!(tokens.get(name_idx).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == "*")
                            {
                                name_idx += 1;
                            }
                            if let Some(JsToken {
                                kind: TokKind::Ident(fn_name),
                                ..
                            }) = tokens.get(name_idx)
                            {
                                exports.push(ExportRecord::Local {
                                    local: fn_name.clone(),
                                    exported: "default".into(),
                                });
                                edits.push((stmt_start, def_end, String::new()));
                            } else {
                                exports.push(ExportRecord::Local {
                                    local: "__mg_default".into(),
                                    exported: "default".into(),
                                });
                                edits.push((stmt_start, def_end, "const __mg_default = ".into()));
                            }
                            i = after;
                            continue;
                        }
                        if matches!(tokens.get(after).map(|t| &t.kind), Some(TokKind::Ident(k)) if k == "class")
                        {
                            if let Some(JsToken {
                                kind: TokKind::Ident(cls_name),
                                ..
                            }) = tokens.get(after + 1)
                                && cls_name != "extends"
                            {
                                exports.push(ExportRecord::Local {
                                    local: cls_name.clone(),
                                    exported: "default".into(),
                                });
                                edits.push((stmt_start, def_end, String::new()));
                            } else {
                                exports.push(ExportRecord::Local {
                                    local: "__mg_default".into(),
                                    exported: "default".into(),
                                });
                                edits.push((stmt_start, def_end, "const __mg_default = ".into()));
                            }
                            i = after;
                            continue;
                        }
                        exports.push(ExportRecord::Local {
                            local: "__mg_default".into(),
                            exported: "default".into(),
                        });
                        edits.push((stmt_start, def_end, "const __mg_default = ".into()));
                        i = after;
                        continue;
                    }
                    // `export var / let / const ...`
                    Some(TokKind::Ident(kw)) if matches!(kw.as_str(), "var" | "let" | "const") => {
                        edits.push((stmt_start, tok.end, String::new()));
                        let mut c = cur + 1;
                        let stmt_end_tok = skip_statement_end(&tokens, c);
                        while c < stmt_end_tok {
                            let (bound, next) = collect_pattern_identifiers(&tokens, c);
                            for b_name in bound {
                                exports.push(ExportRecord::Local {
                                    local: b_name.clone(),
                                    exported: b_name,
                                });
                            }
                            c = skip_initializer(&tokens, next);
                            if matches!(tokens.get(c).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == ",")
                            {
                                c += 1;
                            } else {
                                break;
                            }
                        }
                        i = cur + 1;
                        continue;
                    }
                    // `export [async] function [*] Name` or `export class Name`
                    Some(TokKind::Ident(kw))
                        if matches!(kw.as_str(), "function" | "async" | "class") =>
                    {
                        edits.push((stmt_start, tok.end, String::new()));
                        let mut c = cur;
                        if kw == "async" {
                            c += 1;
                        }
                        c += 1; // skip `function` or `class`
                        if matches!(tokens.get(c).map(|t| &t.kind), Some(TokKind::Punct(p)) if p == "*")
                        {
                            c += 1;
                        }
                        if let Some(JsToken {
                            kind: TokKind::Ident(decl_name),
                            ..
                        }) = tokens.get(c)
                        {
                            exports.push(ExportRecord::Local {
                                local: decl_name.clone(),
                                exported: decl_name.clone(),
                            });
                        }
                        i = cur;
                        continue;
                    }
                    _ => {}
                }
            }
        }

        if let TokKind::Punct(p) = &tok.kind {
            match p.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        i += 1;
    }

    // Apply edits cleanly from left to right.
    edits.sort_by_key(|e| e.0);
    let mut body = String::with_capacity(src.len() + 256);
    let mut cursor = 0usize;
    for (s, e, rep) in edits {
        if s >= cursor && e <= src.len() && s <= e {
            body.push_str(&src[cursor..s]);
            body.push_str(&rep);
            cursor = e;
        }
    }
    body.push_str(&src[cursor..]);

    // Build scope bindings for imports and live property getters for exports.
    let mut preamble_outer = String::new();
    for imp in &imports {
        match imp {
            ImportBinding::Namespace { dep_idx, local } => {
                let local_lit = serde_json::to_string(local).unwrap_or_default();
                preamble_outer.push_str(&format!(
                    "Object.defineProperty(__mg_scope, {local_lit}, {{ enumerable: true, configurable: false, get: function() {{ return __mg_deps[{dep_idx}]; }}, set: function() {{ throw new TypeError(\"Assignment to constant variable.\"); }} }});\n"
                ));
            }
            ImportBinding::Named {
                dep_idx,
                imported,
                local,
            } => {
                let imp_lit = serde_json::to_string(imported).unwrap_or_default();
                let local_lit = serde_json::to_string(local).unwrap_or_default();
                preamble_outer.push_str(&format!(
                    "if (!({imp_lit} in __mg_deps[{dep_idx}])) {{ throw new SyntaxError(\"The requested module does not provide an export named \" + {imp_lit}); }}\nObject.defineProperty(__mg_scope, {local_lit}, {{ enumerable: true, configurable: false, get: function() {{ return __mg_deps[{dep_idx}][{imp_lit}]; }}, set: function() {{ throw new TypeError(\"Assignment to constant variable.\"); }} }});\n"
                ));
            }
        }
    }

    let mut export_defs = String::new();
    for exp in &exports {
        match exp {
            ExportRecord::Local { local, exported } => {
                let exp_lit = serde_json::to_string(exported).unwrap_or_default();
                export_defs.push_str(&format!(
                    "Object.defineProperty(__mg_exports, {exp_lit}, {{ enumerable: true, configurable: false, get: function() {{ return {local}; }} }});\n"
                ));
            }
            ExportRecord::Indirect {
                dep_idx,
                imported,
                exported,
            } => {
                let imp_lit = serde_json::to_string(imported).unwrap_or_default();
                let exp_lit = serde_json::to_string(exported).unwrap_or_default();
                export_defs.push_str(&format!(
                    "if (!({imp_lit} in __mg_deps[{dep_idx}])) {{ throw new SyntaxError(\"The requested module does not provide an export named \" + {imp_lit}); }}\nObject.defineProperty(__mg_exports, {exp_lit}, {{ enumerable: true, configurable: false, get: function() {{ return __mg_deps[{dep_idx}][{imp_lit}]; }} }});\n"
                ));
            }
            ExportRecord::NamespaceFrom { dep_idx, exported } => {
                let exp_lit = serde_json::to_string(exported).unwrap_or_default();
                export_defs.push_str(&format!(
                    "Object.defineProperty(__mg_exports, {exp_lit}, {{ enumerable: true, configurable: false, get: function() {{ return __mg_deps[{dep_idx}]; }} }});\n"
                ));
            }
            ExportRecord::StarFrom { dep_idx } => {
                export_defs.push_str(&format!(
                    "Object.keys(__mg_deps[{dep_idx}]).forEach(function(__mg_k) {{ if (__mg_k !== \"default\" && !Object.prototype.hasOwnProperty.call(__mg_exports, __mg_k)) {{ Object.defineProperty(__mg_exports, __mg_k, {{ enumerable: true, configurable: false, get: function() {{ return __mg_deps[{dep_idx}][__mg_k]; }} }}); }} }});\n"
                ));
            }
        }
    }

    let fn_kw = if has_top_level_await {
        "async function()"
    } else {
        "function()"
    };

    let wrapper_code = format!(
        "(function(__mg_scope, __mg_exports, __mg_deps, __mg_import_meta, __mg_dynamic_import) {{\n{preamble_outer}with (__mg_scope) {{\nreturn ({fn_kw} {{\n\"use strict\";\n{export_defs}{body}\n}}).call(undefined);\n}}\n}})"
    );

    // Keep unused warning quiet on helper re-export check.
    let _ = extract_static_module_specifiers;

    Ok(TransformedModule {
        dependencies,
        wrapper_code,
        has_top_level_await,
    })
}
