//! Bounded CDP routing only. Evaluation and every root live in the page worker.
use super::{App, Reply, Route, boolean, failed, frame_id, invalid, keys, string};
use crate::{runtime_automation::AutomationCompletion, scripts::SessionContinuation};
use mg_sparkle::page_session::automation::{
    self, Argument, AutomationRequest, AutomationResult, MAX_ARGUMENT_BYTES, MAX_ARGUMENTS,
    MAX_DEPTH, MAX_ENTRIES, MAX_GROUP_BYTES, MAX_ISSUED, MAX_OWNER_IDENTITIES, MAX_OWNERS,
    MAX_SOURCE_BYTES, MAX_STRING_UNITS, MAX_TREE_UNITS, Number, ObjectKind, Operation,
    PropertyDescriptor, RemoteValue, SerializedValue,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) enum Delivery {
    Reply {
        route: Route,
        id: Value,
        result: Reply,
    },
    Event {
        route: Route,
        method: &'static str,
        params: Value,
    },
}

/// This mockable seam is data-only; production delegates to the shared Browser
/// transaction slot, never another engine or a copied/renewed script session.
pub(super) trait Host {
    fn identity(&self) -> Option<(u64, u64)>;
    fn idle(&self) -> bool;
    fn dispatch(&mut self, request: AutomationRequest, token: u64) -> Result<(), String>;
    fn completion(&mut self) -> Option<AutomationCompletion>;
    fn cancel(&mut self, reason: &str);
    fn origin(&self) -> String;
    fn ensure_automation(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn can_preload(&self) -> Option<u64> {
        None
    }
}
impl Host for App {
    fn identity(&self) -> Option<(u64, u64)> {
        self.runtime_identity()
    }
    fn idle(&self) -> bool {
        self.runtime_automation_idle()
    }
    fn dispatch(&mut self, request: AutomationRequest, token: u64) -> Result<(), String> {
        self.dispatch_automation(request, token)
    }
    fn completion(&mut self) -> Option<AutomationCompletion> {
        self.take_automation_completion()
    }
    fn cancel(&mut self, reason: &str) {
        self.cancel_automation_session(reason);
    }
    fn origin(&self) -> String {
        url::Url::parse(&self.page_url)
            .map(|url| url.origin().ascii_serialization())
            .unwrap_or_else(|_| "null".into())
    }
    fn ensure_automation(&mut self) -> Result<(), String> {
        self.ensure_automation_session()
    }
    fn can_preload(&self) -> Option<u64> {
        if !self.cdp_enabled
            || self.loading
            || self.fetching
            || self.script_blocked
            || self.child_pool.source_wire_mode() != crate::scripts::SourceWireMode::Modern
            || self.child_pool.observer_expectation()
                != mg_sparkle::page_session::observer::ObserverExpectation::Disabled
            || self
                .lifecycle
                .is_some_and(|p| p.phase != crate::LifecyclePhase::LoadComplete)
        {
            None
        } else {
            Some(self.generation)
        }
    }
}

struct Owner {
    route: Route,
    id: u64,
    revoked: bool,
}
struct Context {
    id: u32,
    identity: (u64, u64),
    owners: Vec<Owner>,
    issued_owner: u64,
    isolated: Vec<(String, u32)>,
    pending: Option<Pending>,
    queued: VecDeque<QueuedOp>,
}
struct Pending {
    token: u64,
    request: AutomationRequest,
    recipient: Recipient,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ClientReplyKind {
    Runtime,
    ResolveNode,
}
enum Recipient {
    Client {
        route: Route,
        id: Value,
        owner: u64,
        exec_context: u32,
        kind: ClientReplyKind,
    },
    ClientIsolatedWorld {
        route: Route,
        id: Value,
        owner: u64,
        world_name: String,
    },
    InternalWorld {
        world_name: String,
    },
    InternalEval,
    Cleanup {
        owners: Vec<u64>,
    },
}
enum QueuedOp {
    Client {
        route: Route,
        id: Value,
        owner: u64,
        new_owner: bool,
        exec_context: u32,
        kind: ClientReplyKind,
        request: AutomationRequest,
    },
    ClientIsolatedWorld {
        route: Route,
        id: Value,
        owner: u64,
        new_owner: bool,
        world_name: String,
    },
    InternalWorld {
        owner: u64,
        world_name: String,
    },
    InternalEval {
        owner: u64,
        world_name: Option<String>,
        source: String,
    },
}

struct PreloadScript {
    id: String,
    route: Route,
    target: String,
    world_name: Option<String>,
    source: String,
}

#[derive(Default)]
pub(super) struct Broker {
    contexts: HashMap<String, Context>,
    enabled: HashMap<Route, String>,
    auto_attached: HashSet<Route>,
    node_handles: HashMap<(Route, String), (String, u64, usize)>,
    preloads: Vec<PreloadScript>,
    preloaded_generations: HashMap<(Route, String, String), u64>,
    next_preload: u64,
    next_context: u32,
    next_token: u64,
    next_exception: u32,
}

pub(super) fn recognizes(method: &str) -> bool {
    matches!(
        method,
        "Runtime.enable"
            | "Runtime.disable"
            | "Runtime.runIfWaitingForDebugger"
            | "Runtime.addBinding"
            | "Runtime.evaluate"
            | "Runtime.callFunctionOn"
            | "Runtime.getProperties"
            | "Runtime.releaseObject"
            | "Runtime.releaseObjectGroup"
    )
}

const MAX_QUEUED_OPS: usize = 64;

impl Broker {
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.contexts.is_empty()
    }

    pub(super) fn set_auto_attached(&mut self, route: &Route, enabled: bool) {
        if enabled {
            self.auto_attached.insert(route.clone());
        } else {
            self.auto_attached.remove(route);
        }
    }

    pub(super) fn has_pending_or_queued(&self, target: &str) -> bool {
        self.contexts
            .get(target)
            .is_some_and(|c| c.pending.is_some() || !c.queued.is_empty())
    }

    pub(super) fn node_for_object(
        &self,
        route: &Route,
        object_id: &str,
    ) -> Option<(String, u64, usize)> {
        self.node_handles
            .get(&(route.clone(), object_id.to_owned()))
            .cloned()
    }

    pub(super) fn add_preload_script(
        &mut self,
        route: &Route,
        target: &str,
        source: String,
        world_name: Option<String>,
        current_settled_generation: Option<u64>,
        run_immediately: bool,
    ) -> String {
        self.next_preload += 1;
        let id = format!("preload-{}", self.next_preload);
        if !run_immediately && let Some(generation) = current_settled_generation {
            self.preloaded_generations
                .insert((route.clone(), target.to_owned(), id.clone()), generation);
        }
        self.preloads.push(PreloadScript {
            id: id.clone(),
            route: route.clone(),
            target: target.to_owned(),
            world_name,
            source,
        });
        id
    }

    pub(super) fn remove_preload_script(&mut self, route: &Route, id: &str) {
        self.preloads.retain(|p| !(&p.route == route && p.id == id));
        self.preloaded_generations
            .retain(|(r, _, pid), _| !(r == route && pid == id));
    }

    fn token(&mut self) -> Result<u64, (i32, String)> {
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or_else(|| failed("Runtime transaction token space exhausted"))?;
        Ok(self.next_token)
    }

    fn route_frame_id(&self, target: &str, route: &Route) -> String {
        if self.auto_attached.contains(route) {
            target.to_owned()
        } else {
            frame_id(target)
        }
    }

    fn created(
        &self,
        target: &str,
        exec_id: u32,
        name: &str,
        is_default: bool,
        host: &impl Host,
        route: &Route,
    ) -> Delivery {
        let fid = self.route_frame_id(target, route);
        let ctx_type = if is_default { "default" } else { "isolated" };
        Delivery::Event {
            route: route.clone(),
            method: "Runtime.executionContextCreated",
            params: json!({
                "context": {
                    "id": exec_id,
                    "origin": host.origin(),
                    "name": name,
                    "uniqueId": format!("mg-context-{exec_id}"),
                    "auxData": {
                        "isDefault": is_default,
                        "type": ctx_type,
                        "frameId": fid
                    }
                }
            }),
        }
    }

    fn destroyed(&self, target: &str, context: &Context) -> Vec<Delivery> {
        let mut out = Vec::new();
        for (route, t) in &self.enabled {
            if t.as_str() != target {
                continue;
            }
            out.push(Delivery::Event {
                route: route.clone(),
                method: "Runtime.executionContextDestroyed",
                params: json!({
                    "executionContextId": context.id,
                    "executionContextUniqueId": format!("mg-context-{}", context.id)
                }),
            });
            for (_, iso_id) in &context.isolated {
                out.push(Delivery::Event {
                    route: route.clone(),
                    method: "Runtime.executionContextDestroyed",
                    params: json!({
                        "executionContextId": iso_id,
                        "executionContextUniqueId": format!("mg-context-{iso_id}")
                    }),
                });
            }
            if self.auto_attached.contains(route) {
                out.push(Delivery::Event {
                    route: route.clone(),
                    method: "Runtime.executionContextsCleared",
                    params: json!({}),
                });
            }
        }
        out
    }

    fn pending_error(context: &mut Context, reason: &str, out: &mut Vec<Delivery>) {
        if let Some(pending) = context.pending.take() {
            match pending.recipient {
                Recipient::Client {
                    route, id, owner, ..
                }
                | Recipient::ClientIsolatedWorld {
                    route, id, owner, ..
                } => {
                    if context.owners.iter().any(|o| o.id == owner && !o.revoked) {
                        out.push(Delivery::Reply {
                            route,
                            id,
                            result: Err(failed(reason)),
                        });
                    }
                }
                Recipient::InternalWorld { .. }
                | Recipient::InternalEval
                | Recipient::Cleanup { .. } => {}
            }
        }
        while let Some(queued) = context.queued.pop_front() {
            match queued {
                QueuedOp::Client {
                    route,
                    id,
                    owner,
                    new_owner,
                    ..
                }
                | QueuedOp::ClientIsolatedWorld {
                    route,
                    id,
                    owner,
                    new_owner,
                    ..
                } => {
                    let live =
                        new_owner || context.owners.iter().any(|o| o.id == owner && !o.revoked);
                    if live {
                        out.push(Delivery::Reply {
                            route,
                            id,
                            result: Err(failed(reason)),
                        });
                    }
                }
                QueuedOp::InternalWorld { .. } | QueuedOp::InternalEval { .. } => {}
            }
        }
    }

    fn record_node_handles(
        &mut self,
        route: &Route,
        target: &str,
        generation: u64,
        exec_context: u32,
        owner: u64,
        result: &AutomationResult,
    ) {
        let mut record = |val: &RemoteValue| {
            if let RemoteValue::Object {
                object,
                kind: ObjectKind::Node { node },
            } = val
            {
                let oid = object_id(exec_context, owner, *object);
                self.node_handles
                    .insert((route.clone(), oid), (target.to_owned(), generation, *node));
            }
        };
        match result {
            AutomationResult::Value(v) => record(v),
            AutomationResult::Exception {
                exception: Some(v), ..
            } => record(v),
            AutomationResult::Properties(props) => {
                for p in props {
                    match &p.descriptor {
                        PropertyDescriptor::Data { value, .. } => record(value),
                        PropertyDescriptor::Accessor { get, set } => {
                            if let Some(g) = get {
                                record(g);
                            }
                            if let Some(s) = set {
                                record(s);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Revocation is immediate parent authority loss. Keep roots/owner capacity
    /// until a normally charged worker ReleaseOwners completion actually lands.
    pub(super) fn revoke(&mut self, route: &Route) {
        self.enabled.remove(route);
        self.auto_attached.remove(route);
        self.node_handles.retain(|(r, _), _| r != route);
        self.preloads.retain(|p| &p.route != route);
        self.preloaded_generations.retain(|(r, _, _), _| r != route);
        for context in self.contexts.values_mut() {
            for owner in &mut context.owners {
                if &owner.route == route {
                    owner.revoked = true;
                }
            }
            context.queued.retain(|q| match q {
                QueuedOp::Client { route: r, .. }
                | QueuedOp::ClientIsolatedWorld { route: r, .. } => r != route,
                QueuedOp::InternalWorld { .. } | QueuedOp::InternalEval { .. } => true,
            });
        }
    }

    pub(super) fn retain_targets(&mut self, targets: &[String]) -> Vec<Delivery> {
        let closed: Vec<_> = self
            .contexts
            .keys()
            .filter(|target| !targets.contains(target))
            .cloned()
            .collect();
        let mut out = Vec::new();
        for target in closed {
            if let Some(mut context) = self.contexts.remove(&target) {
                Self::pending_error(&mut context, "Runtime target closed", &mut out);
                out.extend(self.destroyed(&target, &context));
            }
            self.node_handles.retain(|_, (t, _, _)| t != &target);
            self.preloads.retain(|p| p.target != target);
            self.preloaded_generations
                .retain(|(_, t, _), _| t != &target);
        }
        self.enabled.retain(|_, target| targets.contains(target));
        out
    }

    fn ensure_isolated_world_registered(
        &mut self,
        target: &str,
        context: &mut Context,
        world_name: &str,
        host: &impl Host,
        out: &mut Vec<Delivery>,
    ) -> Option<u32> {
        if let Some((_, existing)) = context.isolated.iter().find(|(w, _)| w == world_name) {
            return Some(*existing);
        }
        let id = self
            .next_context
            .checked_add(1)
            .filter(|id| *id <= i32::MAX as u32)?;
        self.next_context = id;
        context.isolated.push((world_name.to_owned(), id));
        for (route, enabled_target) in &self.enabled {
            if enabled_target == target {
                out.push(self.created(target, id, world_name, false, host, route));
            }
        }
        Some(id)
    }

    pub(super) fn poll(&mut self, target: &str, host: &mut impl Host) -> Vec<Delivery> {
        let mut out = Vec::new();
        let mut context = self.contexts.remove(target);
        // Consume the parent-validated/charged completion even if its route was
        // revoked. Crucially this precedes sampling an identity destroyed by the
        // final sequence63 transaction: Ready is not evidence of a live worker.
        if let Some(completion) = host.completion() {
            let matched = context
                .as_ref()
                .and_then(|c| c.pending.as_ref())
                .is_some_and(|pending| pending.token == completion.token);
            if !matched {
                host.cancel("Runtime completion token mismatch");
                if let Some(ref mut context) = context {
                    Self::pending_error(context, "Runtime completion token mismatch", &mut out);
                }
            } else if let Some(ref mut context) = context {
                let pending = context.pending.take().unwrap();
                match pending.recipient {
                    Recipient::Client {
                        route,
                        id,
                        owner,
                        exec_context,
                        kind,
                    } => {
                        let live = context.owners.iter().any(|o| o.id == owner && !o.revoked);
                        let result = match completion.result {
                            Err(error) => Err(failed(error)),
                            Ok(result) => {
                                let ordinary_rejection =
                                    matches!(result, AutomationResult::Rejected { .. });
                                let converted = result
                                    .validate_for(&pending.request)
                                    .map_err(failed)
                                    .and_then(|()| {
                                        if crate::runtime_automation::has_handles(&result)
                                            && (completion.continuation
                                                == SessionContinuation::Terminal
                                                || host.identity() != Some(context.identity))
                                        {
                                            return Err(failed(
                                                "Destroyed Runtime contexts cannot publish object handles",
                                            ));
                                        }
                                        let exception_id =
                                            if matches!(result, AutomationResult::Exception { .. })
                                            {
                                                self.next_exception = self
                                                    .next_exception
                                                    .checked_add(1)
                                                    .filter(|id| *id <= i32::MAX as u32)
                                                    .ok_or_else(|| {
                                                        failed(
                                                            "Runtime exception identity space exhausted",
                                                        )
                                                    })?;
                                                self.next_exception
                                            } else {
                                                0
                                            };
                                        if live {
                                            self.record_node_handles(
                                                &route,
                                                target,
                                                context.identity.0,
                                                exec_context,
                                                owner,
                                                &result,
                                            );
                                            if let Operation::ReleaseObject { object } =
                                                &pending.request.operation
                                            {
                                                let oid =
                                                    object_id(exec_context, owner, *object);
                                                self.node_handles.remove(&(route.clone(), oid));
                                            }
                                        }
                                        let formatted =
                                            response(exec_context, owner, exception_id, result)?;
                                        if kind == ClientReplyKind::ResolveNode {
                                            if let Some(obj) = formatted.get("result") {
                                                Ok(json!({ "object": obj }))
                                            } else {
                                                Err(failed("Failed to resolve DOM node"))
                                            }
                                        } else {
                                            Ok(formatted)
                                        }
                                    });
                                if converted.is_err()
                                    && !ordinary_rejection
                                    && host.identity() == Some(context.identity)
                                {
                                    host.cancel(
                                        "Runtime result cannot be represented safely by this CDP codec",
                                    );
                                }
                                converted
                            }
                        };
                        if live {
                            out.push(Delivery::Reply { route, id, result });
                        }
                    }
                    Recipient::ClientIsolatedWorld {
                        route,
                        id,
                        owner,
                        world_name,
                    } => {
                        let live = context.owners.iter().any(|o| o.id == owner && !o.revoked);
                        let reply_res = match completion.result {
                            Err(err) => Err(failed(err)),
                            Ok(AutomationResult::Rejected { diagnostic }) => {
                                Err(failed(diagnostic))
                            }
                            Ok(AutomationResult::Released | AutomationResult::Value(_)) => {
                                if completion.continuation == SessionContinuation::Terminal
                                    || host.identity() != Some(context.identity)
                                {
                                    Err(failed("Runtime context was destroyed"))
                                } else if let Some(iso_id) = self.ensure_isolated_world_registered(
                                    target,
                                    context,
                                    &world_name,
                                    host,
                                    &mut out,
                                ) {
                                    Ok(json!({ "executionContextId": iso_id }))
                                } else {
                                    Err(failed("Execution context ID space exhausted"))
                                }
                            }
                            Ok(_) => Err(failed("Unexpected CreateIsolatedWorld worker result")),
                        };
                        if live {
                            out.push(Delivery::Reply {
                                route,
                                id,
                                result: reply_res,
                            });
                        }
                    }
                    Recipient::InternalWorld { world_name } => {
                        if let Ok(AutomationResult::Released | AutomationResult::Value(_)) =
                            completion.result
                            && completion.continuation != SessionContinuation::Terminal
                            && host.identity() == Some(context.identity)
                        {
                            let _ = self.ensure_isolated_world_registered(
                                target,
                                context,
                                &world_name,
                                host,
                                &mut out,
                            );
                        }
                    }
                    Recipient::InternalEval => {}
                    Recipient::Cleanup { owners } => {
                        let accepted = matches!(completion.result, Ok(AutomationResult::Released));
                        if accepted {
                            context.owners.retain(|owner| !owners.contains(&owner.id));
                        } else if host.identity() == Some(context.identity) {
                            host.cancel("Runtime revoked-owner cleanup was not accepted");
                        }
                    }
                }
                if completion.continuation == SessionContinuation::Terminal {
                    self.node_handles.retain(|_, (t, _, _)| t != target);
                    out.extend(self.destroyed(target, context));
                    return out;
                }
            }
        }
        let mut identity = host.identity();
        if identity.is_none()
            && host.can_preload().is_some()
            && self
                .enabled
                .iter()
                .any(|(r, t)| t == target && self.auto_attached.contains(r))
            && host.ensure_automation().is_ok()
        {
            identity = host.identity();
        }
        if context
            .as_ref()
            .is_some_and(|context| Some(context.identity) != identity)
        {
            let mut old = context.take().unwrap();
            self.node_handles.retain(|_, (t, _, _)| t != target);
            Self::pending_error(
                &mut old,
                "Runtime execution context was destroyed",
                &mut out,
            );
            out.extend(self.destroyed(target, &old));
        }
        if context.is_none()
            && let Some(identity) = identity
        {
            let Some(id) = self
                .next_context
                .checked_add(1)
                .filter(|id| *id <= i32::MAX as u32)
            else {
                return out;
            };
            self.next_context = id;
            let new = Context {
                id,
                identity,
                owners: Vec::new(),
                issued_owner: 0,
                isolated: Vec::new(),
                pending: None,
                queued: VecDeque::new(),
            };
            for (route, enabled_target) in &self.enabled {
                if enabled_target == target {
                    out.push(self.created(target, new.id, "", true, host, route));
                }
            }
            context = Some(new);
        }
        if let Some(mut context) = context {
            // Enqueue preload isolated worlds and scripts for newly settled generations.
            let generation = context.identity.0;
            let matching_preloads: Vec<_> = self
                .preloads
                .iter()
                .filter(|p| {
                    p.target == target
                        && self.preloaded_generations.get(&(
                            p.route.clone(),
                            p.target.clone(),
                            p.id.clone(),
                        )) != Some(&generation)
                })
                .map(|p| {
                    (
                        p.route.clone(),
                        p.id.clone(),
                        p.world_name.clone(),
                        p.source.clone(),
                    )
                })
                .collect();
            for (p_route, p_id, world_name, source) in matching_preloads {
                self.preloaded_generations
                    .insert((p_route.clone(), target.to_owned(), p_id), generation);
                let Some(owner) = Self::get_or_alloc_owner(&mut context, &p_route).ok() else {
                    continue;
                };
                if let Some(ref w) = world_name
                    && !context.isolated.iter().any(|(ew, _)| ew == w)
                    && !context.queued.iter().any(
                        |q| matches!(q, QueuedOp::InternalWorld { world_name: qw, .. } if qw == w),
                    )
                    && context.queued.len() < MAX_QUEUED_OPS
                {
                    context.queued.push_back(QueuedOp::InternalWorld {
                        owner,
                        world_name: w.clone(),
                    });
                }
                if !source.trim().is_empty() && context.queued.len() < MAX_QUEUED_OPS {
                    context.queued.push_back(QueuedOp::InternalEval {
                        owner,
                        world_name,
                        source,
                    });
                }
            }

            if context.pending.is_none() && host.idle() {
                let owners: Vec<_> = context
                    .owners
                    .iter()
                    .filter(|owner| owner.revoked)
                    .map(|o| o.id)
                    .collect();
                if !owners.is_empty() {
                    let request = AutomationRequest {
                        owner: 0,
                        operation: Operation::ReleaseOwners {
                            owners: owners.clone(),
                        },
                        edits: Vec::new(),
                    };
                    let admitted = self.token().and_then(|token| {
                        request.validate().map_err(failed)?;
                        host.dispatch(request.clone(), token).map_err(failed)?;
                        context.pending = Some(Pending {
                            token,
                            request,
                            recipient: Recipient::Cleanup { owners },
                        });
                        Ok(())
                    });
                    if admitted.is_err() {
                        host.cancel(
                            "Runtime revoked-owner cleanup cannot enter the existing worker allowance",
                        );
                        self.node_handles.retain(|_, (t, _, _)| t != target);
                        out.extend(self.destroyed(target, &context));
                        return out;
                    }
                } else {
                    while context.pending.is_none() && host.idle() {
                        let Some(queued) = context.queued.pop_front() else {
                            break;
                        };
                        match queued {
                            QueuedOp::Client {
                                route,
                                id,
                                owner,
                                new_owner,
                                exec_context,
                                kind,
                                request,
                            } => {
                                if !new_owner
                                    && !context.owners.iter().any(|o| o.id == owner && !o.revoked)
                                {
                                    continue;
                                }
                                let dispatched = self.token().and_then(|token| {
                                    host.dispatch(request.clone(), token).map_err(failed)?;
                                    if new_owner {
                                        context.issued_owner = owner;
                                        context.owners.push(Owner {
                                            route: route.clone(),
                                            id: owner,
                                            revoked: false,
                                        });
                                    }
                                    context.pending = Some(Pending {
                                        token,
                                        request,
                                        recipient: Recipient::Client {
                                            route: route.clone(),
                                            id: id.clone(),
                                            owner,
                                            exec_context,
                                            kind,
                                        },
                                    });
                                    Ok(())
                                });
                                if let Err(err) = dispatched {
                                    out.push(Delivery::Reply {
                                        route,
                                        id,
                                        result: Err(err),
                                    });
                                }
                            }
                            QueuedOp::ClientIsolatedWorld {
                                route,
                                id,
                                owner,
                                new_owner,
                                world_name,
                            } => {
                                if !new_owner
                                    && !context.owners.iter().any(|o| o.id == owner && !o.revoked)
                                {
                                    continue;
                                }
                                if let Some((_, existing_id)) =
                                    context.isolated.iter().find(|(w, _)| w == &world_name)
                                {
                                    out.push(Delivery::Reply {
                                        route,
                                        id,
                                        result: Ok(json!({ "executionContextId": existing_id })),
                                    });
                                    continue;
                                }
                                let request = AutomationRequest {
                                    owner,
                                    operation: Operation::CreateIsolatedWorld {
                                        world: world_name.clone(),
                                    },
                                    edits: Vec::new(),
                                };
                                let dispatched = self.token().and_then(|token| {
                                    request.validate().map_err(invalid)?;
                                    host.dispatch(request.clone(), token).map_err(failed)?;
                                    if new_owner {
                                        context.issued_owner = owner;
                                        context.owners.push(Owner {
                                            route: route.clone(),
                                            id: owner,
                                            revoked: false,
                                        });
                                    }
                                    context.pending = Some(Pending {
                                        token,
                                        request,
                                        recipient: Recipient::ClientIsolatedWorld {
                                            route: route.clone(),
                                            id: id.clone(),
                                            owner,
                                            world_name,
                                        },
                                    });
                                    Ok(())
                                });
                                if let Err(err) = dispatched {
                                    out.push(Delivery::Reply {
                                        route,
                                        id,
                                        result: Err(err),
                                    });
                                }
                            }
                            QueuedOp::InternalWorld { owner, world_name } => {
                                if context.isolated.iter().any(|(w, _)| w == &world_name) {
                                    continue;
                                }
                                let request = AutomationRequest {
                                    owner,
                                    operation: Operation::CreateIsolatedWorld {
                                        world: world_name.clone(),
                                    },
                                    edits: Vec::new(),
                                };
                                if let Ok(token) = self.token()
                                    && request.validate().is_ok()
                                    && host.dispatch(request.clone(), token).is_ok()
                                {
                                    context.pending = Some(Pending {
                                        token,
                                        request,
                                        recipient: Recipient::InternalWorld { world_name },
                                    });
                                }
                            }
                            QueuedOp::InternalEval {
                                owner,
                                world_name,
                                source,
                            } => {
                                let request = AutomationRequest {
                                    owner,
                                    operation: Operation::Evaluate {
                                        source,
                                        return_by_value: true,
                                        group: None,
                                        world: world_name,
                                        await_promise: false,
                                        user_gesture: false,
                                    },
                                    edits: Vec::new(),
                                };
                                if let Ok(token) = self.token()
                                    && request.validate().is_ok()
                                    && host.dispatch(request.clone(), token).is_ok()
                                {
                                    context.pending = Some(Pending {
                                        token,
                                        request,
                                        recipient: Recipient::InternalEval,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            self.contexts.insert(target.to_owned(), context);
        }
        out
    }

    fn get_or_alloc_owner(context: &mut Context, route: &Route) -> Result<u64, (i32, String)> {
        if let Some(existing) = context
            .owners
            .iter()
            .find(|o| &o.route == route && !o.revoked)
            .map(|o| o.id)
        {
            return Ok(existing);
        }
        if context.owners.len() >= MAX_OWNERS || context.issued_owner >= MAX_OWNER_IDENTITIES as u64
        {
            return Err(failed("Runtime route-owner limit reached for this worker"));
        }
        let owner = context
            .issued_owner
            .checked_add(1)
            .ok_or_else(|| failed("Runtime owner identity space exhausted"))?;
        context.issued_owner = owner;
        context.owners.push(Owner {
            route: route.clone(),
            id: owner,
            revoked: false,
        });
        Ok(owner)
    }

    pub(super) fn command(
        &mut self,
        target: &str,
        host: &mut impl Host,
        route: &Route,
        id: Value,
        method: &str,
        params: &Value,
    ) -> Vec<Delivery> {
        let mut out = self.poll(target, host);
        let result = match method {
            "Runtime.enable" | "Runtime.disable" => keys(params, &[]).map(|()| {
                if method == "Runtime.enable" {
                    let newly_enabled = self
                        .enabled
                        .insert(route.clone(), target.to_owned())
                        .is_none();
                    out.push(Delivery::Reply {
                        route: route.clone(),
                        id: id.clone(),
                        result: Ok(json!({})),
                    });
                    if newly_enabled {
                        if let Some(context) = self.contexts.get(target) {
                            out.push(self.created(target, context.id, "", true, host, route));
                            for (w, iso_id) in &context.isolated {
                                out.push(self.created(target, *iso_id, w, false, host, route));
                            }
                        } else if self.auto_attached.contains(route) && host.can_preload().is_some()
                        {
                            out.extend(self.poll(target, host));
                        }
                    }
                } else {
                    self.enabled.remove(route);
                    out.push(Delivery::Reply {
                        route: route.clone(),
                        id: id.clone(),
                        result: Ok(json!({})),
                    });
                }
            }),
            "Runtime.runIfWaitingForDebugger" => keys(params, &[]).map(|()| {
                out.push(Delivery::Reply {
                    route: route.clone(),
                    id: id.clone(),
                    result: Ok(json!({})),
                });
            }),
            "Runtime.addBinding" => keys(
                params,
                &["name", "executionContextId", "executionContextName"],
            )
            .and_then(|()| {
                let _ = string(params, "name")?;
                out.push(Delivery::Reply {
                    route: route.clone(),
                    id: id.clone(),
                    result: Ok(json!({})),
                });
                Ok(())
            }),
            _ => self.admit(target, host, route, id.clone(), method, params, &mut out),
        };
        if let Err(error) = result {
            out.push(Delivery::Reply {
                route: route.clone(),
                id,
                result: Err(error),
            });
        }
        out
    }

    pub(super) fn create_isolated_world(
        &mut self,
        target: &str,
        host: &mut impl Host,
        route: &Route,
        id: Value,
        world_name: String,
    ) -> Vec<Delivery> {
        let mut out = self.poll(target, host);
        if self.contexts.get(target).is_none() && host.can_preload().is_some() {
            if let Err(err) = host.ensure_automation() {
                out.push(Delivery::Reply {
                    route: route.clone(),
                    id,
                    result: Err(failed(err)),
                });
                return out;
            }
            out.extend(self.poll(target, host));
        }
        let res = (|| -> Result<(), (i32, String)> {
            if world_name.is_empty() || world_name.len() > 256 {
                return Err(invalid("worldName must be 1..=256 bytes"));
            }
            let context = self
                .contexts
                .get_mut(target)
                .ok_or_else(|| failed("Isolated world requires a loaded page"))?;
            if let Some((_, existing_id)) = context.isolated.iter().find(|(w, _)| w == &world_name)
            {
                out.push(Delivery::Reply {
                    route: route.clone(),
                    id: id.clone(),
                    result: Ok(json!({ "executionContextId": existing_id })),
                });
                return Ok(());
            }
            let (owner, new_owner) = Self::candidate_owner(context, route)?;
            if context.pending.is_some() || !context.queued.is_empty() || !host.idle() {
                if context.queued.len() >= MAX_QUEUED_OPS {
                    return Err(failed("Runtime automation queue is full"));
                }
                if new_owner {
                    context.issued_owner = owner;
                    context.owners.push(Owner {
                        route: route.clone(),
                        id: owner,
                        revoked: false,
                    });
                }
                context.queued.push_back(QueuedOp::ClientIsolatedWorld {
                    route: route.clone(),
                    id: id.clone(),
                    owner,
                    new_owner: false,
                    world_name,
                });
                return Ok(());
            }
            let request = AutomationRequest {
                owner,
                operation: Operation::CreateIsolatedWorld {
                    world: world_name.clone(),
                },
                edits: Vec::new(),
            };
            request.validate().map_err(invalid)?;
            let token = self.token()?;
            host.dispatch(request.clone(), token).map_err(failed)?;
            let context = self.contexts.get_mut(target).unwrap();
            if new_owner {
                context.issued_owner = owner;
                context.owners.push(Owner {
                    route: route.clone(),
                    id: owner,
                    revoked: false,
                });
            }
            context.pending = Some(Pending {
                token,
                request,
                recipient: Recipient::ClientIsolatedWorld {
                    route: route.clone(),
                    id: id.clone(),
                    owner,
                    world_name,
                },
            });
            Ok(())
        })();
        if let Err(err) = res {
            out.push(Delivery::Reply {
                route: route.clone(),
                id,
                result: Err(err),
            });
        }
        out
    }

    pub(super) fn resolve_node(
        &mut self,
        target: &str,
        host: &mut impl Host,
        route: &Route,
        id: Value,
        node: usize,
        context_id_param: Option<u32>,
        object_group: Option<String>,
    ) -> Vec<Delivery> {
        let mut out = self.poll(target, host);
        if self.contexts.get(target).is_none()
            && self.auto_attached.contains(route)
            && host.can_preload().is_some()
            && host.ensure_automation().is_ok()
        {
            out.extend(self.poll(target, host));
        }
        let auto = self.auto_attached.contains(route);
        let res = (|| -> Result<(), (i32, String)> {
            if let Some(ref g) = object_group
                && g.len() > MAX_GROUP_BYTES
            {
                return Err(invalid("Runtime object group exceeds 64 bytes"));
            }
            let context = self.contexts.get(target).ok_or_else(|| {
                failed("DOM.resolveNode requires an enabled, loaded, usable page script realm")
            })?;
            let (exec_context, world) = match context_id_param {
                None => (context.id, None),
                Some(cid) if cid == context.id => (context.id, None),
                Some(cid) => context
                    .isolated
                    .iter()
                    .find(|(_, id)| *id == cid)
                    .map(|(w, id)| (*id, Some(w.clone())))
                    .ok_or_else(|| failed("Unknown or foreign execution context"))?,
            };
            let (owner, new_owner) = Self::candidate_owner(context, route)?;
            let request = AutomationRequest {
                owner,
                operation: Operation::ResolveNode {
                    node,
                    group: object_group,
                    world,
                },
                edits: Vec::new(),
            };
            request.validate().map_err(invalid)?;
            if context.pending.is_some() || !context.queued.is_empty() || !host.idle() {
                if !auto {
                    return Err(failed("A page transaction or Runtime result is pending"));
                }
                let context = self.contexts.get_mut(target).unwrap();
                if context.queued.len() >= MAX_QUEUED_OPS {
                    return Err(failed("Runtime automation queue is full"));
                }
                if new_owner {
                    context.issued_owner = owner;
                    context.owners.push(Owner {
                        route: route.clone(),
                        id: owner,
                        revoked: false,
                    });
                }
                context.queued.push_back(QueuedOp::Client {
                    route: route.clone(),
                    id: id.clone(),
                    owner,
                    new_owner: false,
                    exec_context,
                    kind: ClientReplyKind::ResolveNode,
                    request,
                });
                return Ok(());
            }
            let token = self.token()?;
            host.dispatch(request.clone(), token).map_err(failed)?;
            let context = self.contexts.get_mut(target).unwrap();
            if new_owner {
                context.issued_owner = owner;
                context.owners.push(Owner {
                    route: route.clone(),
                    id: owner,
                    revoked: false,
                });
            }
            context.pending = Some(Pending {
                token,
                request,
                recipient: Recipient::Client {
                    route: route.clone(),
                    id: id.clone(),
                    owner,
                    exec_context,
                    kind: ClientReplyKind::ResolveNode,
                },
            });
            Ok(())
        })();
        if let Err(err) = res {
            out.push(Delivery::Reply {
                route: route.clone(),
                id,
                result: Err(err),
            });
        }
        out
    }

    fn candidate_owner(context: &Context, route: &Route) -> Result<(u64, bool), (i32, String)> {
        if let Some(owner) = context
            .owners
            .iter()
            .find(|owner| &owner.route == route && !owner.revoked)
            .map(|o| o.id)
        {
            return Ok((owner, false));
        }
        if context.owners.len() >= MAX_OWNERS || context.issued_owner >= MAX_OWNER_IDENTITIES as u64
        {
            return Err(failed("Runtime route-owner limit reached for this worker"));
        }
        let owner = context
            .issued_owner
            .checked_add(1)
            .ok_or_else(|| failed("Runtime owner identity space exhausted"))?;
        Ok((owner, true))
    }

    #[allow(clippy::too_many_arguments)]
    fn admit(
        &mut self,
        target: &str,
        host: &mut impl Host,
        route: &Route,
        id: Value,
        method: &str,
        params: &Value,
        out: &mut Vec<Delivery>,
    ) -> Result<(), (i32, String)> {
        let auto = self.auto_attached.contains(route);
        if self.contexts.get(target).is_none()
            && auto
            && host.can_preload().is_some()
            && host.ensure_automation().is_ok()
        {
            out.extend(self.poll(target, host));
        }
        let context = self.contexts.get(target).ok_or_else(|| {
            failed("Runtime requires an enabled, loaded, usable page script realm")
        })?;
        let busy = context.pending.is_some() || !context.queued.is_empty() || !host.idle();
        if busy && !auto {
            return Err(failed("A page transaction or Runtime result is pending"));
        }
        let (owner, new_owner) = Self::candidate_owner(context, route)?;
        let (exec_context, operation) = operation(method, params, context, owner, auto)?;
        let request = AutomationRequest {
            owner,
            operation,
            edits: Vec::new(),
        };
        request.validate().map_err(invalid)?;
        if busy {
            let context = self.contexts.get_mut(target).unwrap();
            if context.queued.len() >= MAX_QUEUED_OPS {
                return Err(failed("Runtime automation queue is full"));
            }
            if new_owner {
                context.issued_owner = owner;
                context.owners.push(Owner {
                    route: route.clone(),
                    id: owner,
                    revoked: false,
                });
            }
            context.queued.push_back(QueuedOp::Client {
                route: route.clone(),
                id,
                owner,
                new_owner: false,
                exec_context,
                kind: ClientReplyKind::Runtime,
                request,
            });
            return Ok(());
        }
        let token = self.token()?;
        host.dispatch(request.clone(), token).map_err(failed)?;
        let context = self.contexts.get_mut(target).unwrap();
        if new_owner {
            context.issued_owner = owner;
            context.owners.push(Owner {
                route: route.clone(),
                id: owner,
                revoked: false,
            });
        }
        context.pending = Some(Pending {
            token,
            request,
            recipient: Recipient::Client {
                route: route.clone(),
                id,
                owner,
                exec_context,
                kind: ClientReplyKind::Runtime,
            },
        });
        Ok(())
    }
}

fn false_options(params: &Value, options: &[&str]) -> Result<(), (i32, String)> {
    for option in options {
        if boolean(params, option, false)? {
            return Err(invalid(format!("{option}:true is not implemented")));
        }
    }
    Ok(())
}
fn resolve_context_param(
    params: &Value,
    key: &str,
    unique_key: Option<&str>,
    context: &Context,
) -> Result<(u32, Option<String>), (i32, String)> {
    if let Some(ukey) = unique_key
        && params.get(ukey).is_some()
    {
        if params.get(key).is_some() {
            return Err(invalid(format!("Specify {key} or {ukey}, not both")));
        }
        let unique = string(params, ukey)?;
        let cid = unique
            .strip_prefix("mg-context-")
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| failed("Unknown or foreign execution context"))?;
        if cid == context.id {
            return Ok((context.id, None));
        }
        if let Some((w, id)) = context.isolated.iter().find(|(_, id)| *id == cid) {
            return Ok((*id, Some(w.clone())));
        }
        return Err(failed("Unknown or foreign execution context"));
    }
    if let Some(value) = params.get(key) {
        let id = value
            .as_u64()
            .filter(|id| *id > 0 && *id <= i32::MAX as u64)
            .ok_or_else(|| invalid(format!("{key} must be a positive execution context ID")))?
            as u32;
        if id == context.id {
            return Ok((context.id, None));
        }
        if let Some((w, iso_id)) = context.isolated.iter().find(|(_, iso_id)| *iso_id == id) {
            return Ok((*iso_id, Some(w.clone())));
        }
        return Err(failed("Unknown or foreign execution context"));
    }
    Ok((context.id, None))
}
fn source(params: &Value, key: &str) -> Result<String, (i32, String)> {
    let source = string(params, key)?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(invalid("Runtime source exceeds 384 KiB"));
    }
    Ok(source.to_owned())
}
fn group(params: &Value) -> Result<Option<String>, (i32, String)> {
    params
        .get("objectGroup")
        .map(|_| {
            let group = string(params, "objectGroup")?;
            if group.len() > MAX_GROUP_BYTES {
                return Err(invalid("Runtime object group exceeds 64 bytes"));
            }
            Ok(group.to_owned())
        })
        .transpose()
}
fn object_id(context: u32, owner: u64, object: u64) -> String {
    format!("mg:{context}:{owner}:{object}")
}
fn parse_object_any_world(
    value: &str,
    context: &Context,
    owner: u64,
) -> Result<(u32, Option<String>, u64), (i32, String)> {
    let mut fields = value.split(':');
    let parsed = (
        fields.next(),
        fields.next().and_then(|v| v.parse::<u32>().ok()),
        fields.next().and_then(|v| v.parse::<u64>().ok()),
        fields.next().and_then(|v| v.parse::<u64>().ok()),
    );
    match parsed {
        (Some("mg"), Some(c), Some(o), Some(id))
            if fields.next().is_none()
                && o == owner
                && id > 0
                && id <= MAX_ISSUED as u64
                && value == object_id(c, o, id) =>
        {
            if c == context.id {
                return Ok((c, None, id));
            }
            if let Some((w, _)) = context.isolated.iter().find(|(_, iso_id)| *iso_id == c) {
                return Ok((c, Some(w.clone()), id));
            }
            Err(failed("Invalid, stale or foreign Runtime objectId"))
        }
        _ => Err(failed("Invalid, stale or foreign Runtime objectId")),
    }
}
fn object(value: &str, exec_context: u32, owner: u64) -> Result<u64, (i32, String)> {
    let mut fields = value.split(':');
    let parsed = (
        fields.next(),
        fields.next().and_then(|v| v.parse::<u32>().ok()),
        fields.next().and_then(|v| v.parse::<u64>().ok()),
        fields.next().and_then(|v| v.parse::<u64>().ok()),
    );
    match parsed {
        (Some("mg"), Some(c), Some(o), Some(id))
            if fields.next().is_none()
                && c == exec_context
                && o == owner
                && id > 0
                && id <= MAX_ISSUED as u64
                && value == object_id(c, o, id) =>
        {
            Ok(id)
        }
        _ => Err(failed("Invalid, stale or foreign Runtime objectId")),
    }
}
fn operation(
    method: &str,
    params: &Value,
    context: &Context,
    owner: u64,
    auto_attached: bool,
) -> Result<(u32, Operation), (i32, String)> {
    match method {
        "Runtime.evaluate" => {
            if auto_attached {
                keys(
                    params,
                    &[
                        "expression",
                        "contextId",
                        "uniqueContextId",
                        "objectGroup",
                        "returnByValue",
                        "awaitPromise",
                        "userGesture",
                        "silent",
                        "generatePreview",
                        "includeCommandLineAPI",
                        "throwOnSideEffect",
                        "replMode",
                        "allowUnsafeEvalBlockedByCSP",
                    ],
                )?;
                for opt in [
                    "silent",
                    "generatePreview",
                    "includeCommandLineAPI",
                    "allowUnsafeEvalBlockedByCSP",
                ] {
                    let _ = boolean(params, opt, false)?;
                }
                false_options(params, &["throwOnSideEffect", "replMode"])?;
            } else {
                keys(
                    params,
                    &[
                        "expression",
                        "contextId",
                        "objectGroup",
                        "returnByValue",
                        "awaitPromise",
                        "userGesture",
                        "silent",
                        "generatePreview",
                        "includeCommandLineAPI",
                        "throwOnSideEffect",
                        "replMode",
                    ],
                )?;
                false_options(
                    params,
                    &[
                        "awaitPromise",
                        "userGesture",
                        "silent",
                        "generatePreview",
                        "includeCommandLineAPI",
                        "throwOnSideEffect",
                        "replMode",
                    ],
                )?;
            }
            let (exec_context, world) = resolve_context_param(
                params,
                "contextId",
                auto_attached.then_some("uniqueContextId"),
                context,
            )?;
            let await_promise = if auto_attached {
                boolean(params, "awaitPromise", false)?
            } else {
                false
            };
            let user_gesture = if auto_attached {
                boolean(params, "userGesture", false)?
            } else {
                false
            };
            Ok((
                exec_context,
                Operation::Evaluate {
                    source: source(params, "expression")?,
                    return_by_value: boolean(params, "returnByValue", false)?,
                    group: group(params)?,
                    world,
                    await_promise,
                    user_gesture,
                },
            ))
        }
        "Runtime.callFunctionOn" => {
            if auto_attached {
                keys(
                    params,
                    &[
                        "functionDeclaration",
                        "objectId",
                        "executionContextId",
                        "uniqueContextId",
                        "arguments",
                        "objectGroup",
                        "returnByValue",
                        "awaitPromise",
                        "userGesture",
                        "silent",
                        "generatePreview",
                        "throwOnSideEffect",
                    ],
                )?;
                for opt in ["silent", "generatePreview"] {
                    let _ = boolean(params, opt, false)?;
                }
                false_options(params, &["throwOnSideEffect"])?;
            } else {
                keys(
                    params,
                    &[
                        "functionDeclaration",
                        "objectId",
                        "executionContextId",
                        "arguments",
                        "objectGroup",
                        "returnByValue",
                        "awaitPromise",
                        "userGesture",
                        "silent",
                        "generatePreview",
                        "throwOnSideEffect",
                    ],
                )?;
                false_options(
                    params,
                    &[
                        "awaitPromise",
                        "userGesture",
                        "silent",
                        "generatePreview",
                        "throwOnSideEffect",
                    ],
                )?;
            }
            let has_ctx = params.get("executionContextId").is_some()
                || (auto_attached && params.get("uniqueContextId").is_some());
            if params.get("objectId").is_some() && has_ctx {
                return Err(invalid("Specify objectId or executionContextId, not both"));
            }
            let (exec_context, world, receiver) = if params.get("objectId").is_some() {
                let (c, w, recv) =
                    parse_object_any_world(string(params, "objectId")?, context, owner)?;
                (c, w, Some(recv))
            } else {
                let (c, w) = resolve_context_param(
                    params,
                    "executionContextId",
                    auto_attached.then_some("uniqueContextId"),
                    context,
                )?;
                (c, w, None)
            };
            let arguments = arguments(params, exec_context, owner)?;
            let await_promise = if auto_attached {
                boolean(params, "awaitPromise", false)?
            } else {
                false
            };
            let user_gesture = if auto_attached {
                boolean(params, "userGesture", false)?
            } else {
                false
            };
            Ok((
                exec_context,
                Operation::CallFunction {
                    source: source(params, "functionDeclaration")?,
                    receiver,
                    arguments,
                    return_by_value: boolean(params, "returnByValue", false)?,
                    group: group(params)?,
                    world,
                    await_promise,
                    user_gesture,
                },
            ))
        }
        "Runtime.getProperties" => {
            keys(
                params,
                &[
                    "objectId",
                    "ownProperties",
                    "accessorPropertiesOnly",
                    "generatePreview",
                    "nonIndexedPropertiesOnly",
                ],
            )?;
            if auto_attached {
                let _ = boolean(params, "ownProperties", true)?;
                let _ = boolean(params, "accessorPropertiesOnly", false)?;
                let _ = boolean(params, "generatePreview", false)?;
                let _ = boolean(params, "nonIndexedPropertiesOnly", false)?;
            } else {
                if !boolean(params, "ownProperties", false)? {
                    return Err(invalid("Only ownProperties:true inspection is implemented"));
                }
                false_options(
                    params,
                    &[
                        "accessorPropertiesOnly",
                        "generatePreview",
                        "nonIndexedPropertiesOnly",
                    ],
                )?;
            }
            let (exec_context, _, obj) =
                parse_object_any_world(string(params, "objectId")?, context, owner)?;
            Ok((exec_context, Operation::GetOwnProperties { object: obj }))
        }
        "Runtime.releaseObject" => {
            keys(params, &["objectId"])?;
            let (exec_context, _, obj) =
                parse_object_any_world(string(params, "objectId")?, context, owner)?;
            Ok((exec_context, Operation::ReleaseObject { object: obj }))
        }
        "Runtime.releaseObjectGroup" => {
            keys(params, &["objectGroup"])?;
            Ok((
                context.id,
                Operation::ReleaseGroup {
                    group: group(params)?.ok_or_else(|| invalid("objectGroup is required"))?,
                },
            ))
        }
        _ => Err((-32601, format!("{method} is not implemented by mgbrowser"))),
    }
}

#[derive(Default)]
struct TreeBudget {
    entries: usize,
    units: usize,
}
impl TreeBudget {
    fn text(&mut self, text: &str) -> Result<Vec<u16>, (i32, String)> {
        let len = text.encode_utf16().count();
        self.units = self
            .units
            .checked_add(len)
            .ok_or_else(|| invalid("Runtime text budget exceeded"))?;
        if len > MAX_STRING_UNITS || self.units > MAX_TREE_UNITS {
            return Err(invalid("Runtime string/tree storage exceeded"));
        }
        Ok(text.encode_utf16().collect())
    }
    fn value(&mut self, value: &Value, depth: usize) -> Result<SerializedValue, (i32, String)> {
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or_else(|| invalid("Runtime tree size exceeded"))?;
        if depth > MAX_DEPTH || self.entries > MAX_ENTRIES {
            return Err(invalid("Runtime tree size/depth exceeded"));
        }
        match value {
            Value::Null => Ok(SerializedValue::Null),
            Value::Bool(value) => Ok(SerializedValue::Boolean(*value)),
            Value::Number(value) => {
                let value = value
                    .as_f64()
                    .ok_or_else(|| invalid("Runtime number is not representable"))?;
                if !value.is_finite() || (value == 0. && value.is_sign_negative()) {
                    return Err(invalid(
                        "Compound nonfinite/negative-zero values are not implemented",
                    ));
                }
                Ok(SerializedValue::FiniteNumber(value))
            }
            Value::String(value) => Ok(SerializedValue::String(self.text(value)?)),
            Value::Array(values) => {
                if values.len() > MAX_ENTRIES.saturating_sub(self.entries) {
                    return Err(invalid("Runtime tree size exceeded"));
                }
                values
                    .iter()
                    .map(|v| self.value(v, depth + 1))
                    .collect::<Result<Vec<_>, _>>()
                    .map(SerializedValue::Array)
            }
            Value::Object(values) => {
                if values.len() > MAX_ENTRIES.saturating_sub(self.entries) {
                    return Err(invalid("Runtime tree size exceeded"));
                }
                let mut result = Vec::with_capacity(values.len());
                for (key, value) in values {
                    result.push((self.text(key)?, self.value(value, depth + 1)?));
                }
                Ok(SerializedValue::Object(result))
            }
        }
    }
}
fn arguments(params: &Value, context: u32, owner: u64) -> Result<Vec<Argument>, (i32, String)> {
    let Some(arguments) = params.get("arguments") else {
        return Ok(Vec::new());
    };
    let arguments = arguments
        .as_array()
        .ok_or_else(|| invalid("arguments must be an array"))?;
    if arguments.len() > MAX_ARGUMENTS {
        return Err(invalid("Runtime argument count exceeds 32"));
    }
    automation::encoded_size(arguments, MAX_ARGUMENT_BYTES).map_err(invalid)?;
    let mut budget = TreeBudget::default();
    let mut result = Vec::with_capacity(arguments.len());
    for argument in arguments {
        keys(argument, &["value", "unserializableValue", "objectId"])?;
        let count = ["value", "unserializableValue", "objectId"]
            .iter()
            .filter(|key| argument.get(**key).is_some())
            .count();
        if count > 1 {
            return Err(invalid("Call argument forms are mutually exclusive"));
        }
        let value = if let Some(value) = argument.get("value") {
            if let Some(number) = value.as_f64().filter(|_| value.is_number()) {
                Argument::Number(Number::from_f64(number))
            } else {
                Argument::Value(budget.value(value, 0)?)
            }
        } else if argument.get("objectId").is_some() {
            Argument::Object(object(string(argument, "objectId")?, context, owner)?)
        } else if argument.get("unserializableValue").is_some() {
            let value = string(argument, "unserializableValue")?;
            match value {
                "NaN" => Argument::Number(Number::NaN),
                "Infinity" => Argument::Number(Number::PositiveInfinity),
                "-Infinity" => Argument::Number(Number::NegativeInfinity),
                "-0" => Argument::Number(Number::NegativeZero),
                _ => {
                    let value = value
                        .strip_suffix('n')
                        .ok_or_else(|| invalid("Unsupported unserializableValue"))?;
                    automation::validate_bigint(value).map_err(invalid)?;
                    Argument::BigInt(value.to_owned())
                }
            }
        } else {
            Argument::Undefined
        };
        result.push(value);
    }
    Ok(result)
}

fn text(units: &[u16]) -> Result<String, (i32, String)> {
    String::from_utf16(units)
        .map_err(|_| failed("CDP cannot represent an unpaired UTF-16 surrogate"))
}
fn tree(value: &SerializedValue) -> Result<Value, (i32, String)> {
    match value {
        SerializedValue::Null => Ok(Value::Null),
        SerializedValue::Boolean(v) => Ok(json!(v)),
        SerializedValue::FiniteNumber(v) => {
            if !v.is_finite() || (*v == 0. && v.is_sign_negative()) {
                return Err(failed("Invalid Runtime compound number"));
            }
            Ok(json!(v))
        }
        SerializedValue::String(v) => Ok(json!(text(v)?)),
        SerializedValue::Array(values) => values
            .iter()
            .map(tree)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        SerializedValue::Object(values) => {
            let mut result = serde_json::Map::new();
            for (key, value) in values {
                if result.insert(text(key)?, tree(value)?).is_some() {
                    return Err(failed("Duplicate Runtime by-value property"));
                }
            }
            Ok(Value::Object(result))
        }
    }
}
fn remote(context: u32, owner: u64, value: &RemoteValue) -> Reply {
    Ok(match value {
        RemoteValue::Undefined => json!({"type":"undefined"}),
        RemoteValue::Null => json!({"type":"object","subtype":"null","value":null}),
        RemoteValue::Boolean(value) => json!({"type":"boolean","value":value}),
        RemoteValue::Number(number) => {
            number.validate().map_err(failed)?;
            match number {
                Number::Finite(value) => json!({"type":"number","value":value}),
                _ => json!({"type":"number","unserializableValue":match number {
                    Number::NaN => "NaN", Number::PositiveInfinity => "Infinity",
                    Number::NegativeInfinity => "-Infinity", Number::NegativeZero => "-0", _ => unreachable!(),
                }}),
            }
        }
        RemoteValue::String(value) => json!({"type":"string","value":text(value)?}),
        RemoteValue::BigInt(value) => {
            automation::validate_bigint(value).map_err(failed)?;
            json!({"type":"bigint","unserializableValue":format!("{value}n")})
        }
        RemoteValue::Object { object, kind } => {
            if *object == 0 || *object > MAX_ISSUED as u64 {
                return Err(failed("Invalid worker object ID"));
            }
            let mut result = json!({"type":if *kind == ObjectKind::Function {"function"} else {"object"},
                "objectId":object_id(context,owner,*object)});
            if let Some(subtype) = match kind {
                ObjectKind::Array => Some("array"),
                ObjectKind::Promise => Some("promise"),
                ObjectKind::Proxy => Some("proxy"),
                ObjectKind::Error => Some("error"),
                ObjectKind::Node { .. } => Some("node"),
                _ => None,
            } {
                result["subtype"] = json!(subtype);
            }
            result
        }
        RemoteValue::ByValue(value) => {
            value.validate().map_err(failed)?;
            let value = tree(value)?;
            let mut result = json!({"type":match value { Value::Bool(_) => "boolean", Value::Number(_) => "number", Value::String(_) => "string", _ => "object" },"value":value});
            if result["value"].is_array() {
                result["subtype"] = json!("array");
            }
            if result["value"].is_null() {
                result["subtype"] = json!("null");
            }
            result
        }
    })
}
fn response(context: u32, owner: u64, exception_id: u32, result: AutomationResult) -> Reply {
    match result {
        AutomationResult::Value(value) => Ok(json!({"result":remote(context,owner,&value)?})),
        AutomationResult::Exception {
            diagnostic,
            exception,
        } => {
            let mut exception = exception
                .as_ref()
                .map(|value| remote(context, owner, value))
                .transpose()?;
            if let Some(ex) = exception.as_mut()
                && ex.get("objectId").is_some()
            {
                ex["description"] = json!(diagnostic);
            }
            let mut details =
                json!({"exceptionId":exception_id,"text":diagnostic,"executionContextId":context});
            if let Some(exception) = &exception {
                details["exception"] = exception.clone();
            }
            let mut response = json!({"exceptionDetails":details});
            if let Some(exception) = exception {
                response["result"] = exception;
            }
            Ok(response)
        }
        AutomationResult::Properties(properties) => {
            let mut result = Vec::with_capacity(properties.len());
            for property in properties {
                let mut value = json!({"name":text(&property.name)?,"enumerable":property.enumerable,
                    "configurable":property.configurable,"isOwn":true});
                match property.descriptor {
                    PropertyDescriptor::Data {
                        value: data,
                        writable,
                    } => {
                        value["value"] = remote(context, owner, &data)?;
                        value["writable"] = json!(writable);
                    }
                    PropertyDescriptor::Accessor { get, set } => {
                        if let Some(get) = get {
                            value["get"] = remote(context, owner, &get)?;
                        }
                        if let Some(set) = set {
                            value["set"] = remote(context, owner, &set)?;
                        }
                    }
                }
                result.push(value);
            }
            Ok(json!({"result":result}))
        }
        AutomationResult::Released => Ok(json!({})),
        AutomationResult::Rejected { diagnostic } => Err(failed(diagnostic)),
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
