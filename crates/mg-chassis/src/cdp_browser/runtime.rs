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
use std::collections::HashMap;

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
    pending: Option<Pending>,
}
struct Pending {
    token: u64,
    request: AutomationRequest,
    recipient: Recipient,
}
enum Recipient {
    Client { route: Route, id: Value, owner: u64 },
    Cleanup { owners: Vec<u64> },
}

#[derive(Default)]
pub(super) struct Broker {
    contexts: HashMap<String, Context>,
    enabled: HashMap<Route, String>,
    next_context: u32,
    next_token: u64,
    next_exception: u32,
}

pub(super) fn recognizes(method: &str) -> bool {
    matches!(
        method,
        "Runtime.enable"
            | "Runtime.disable"
            | "Runtime.evaluate"
            | "Runtime.callFunctionOn"
            | "Runtime.getProperties"
            | "Runtime.releaseObject"
            | "Runtime.releaseObjectGroup"
    )
}

impl Broker {
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.contexts.is_empty()
    }

    fn token(&mut self) -> Result<u64, (i32, String)> {
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or_else(|| failed("Runtime transaction token space exhausted"))?;
        Ok(self.next_token)
    }
    fn created(target: &str, context: &Context, host: &impl Host, route: &Route) -> Delivery {
        Delivery::Event {
            route: route.clone(),
            method: "Runtime.executionContextCreated",
            params: json!({"context":{"id":context.id,"origin":host.origin(),"name":"",
                "uniqueId":format!("mg-context-{}",context.id),
                "auxData":{"isDefault":true,"type":"default","frameId":frame_id(target)}}}),
        }
    }
    fn destroyed(&self, target: &str, context: &Context) -> Vec<Delivery> {
        self.enabled
            .iter()
            .filter(|(_, t)| t.as_str() == target)
            .map(|(route, _)| Delivery::Event {
                route: route.clone(),
                method: "Runtime.executionContextDestroyed",
                params: json!({"executionContextId":context.id,
                "executionContextUniqueId":format!("mg-context-{}",context.id)}),
            })
            .collect()
    }
    fn pending_error(context: &mut Context, reason: &str, out: &mut Vec<Delivery>) {
        if let Some(Pending {
            recipient: Recipient::Client { route, id, owner },
            ..
        }) = context.pending.take()
        {
            if context.owners.iter().any(|o| o.id == owner && !o.revoked) {
                out.push(Delivery::Reply {
                    route,
                    id,
                    result: Err(failed(reason)),
                });
            }
        }
    }

    /// Revocation is immediate parent authority loss. Keep roots/owner capacity
    /// until a normally charged worker ReleaseOwners completion actually lands.
    pub(super) fn revoke(&mut self, route: &Route) {
        self.enabled.remove(route);
        for context in self.contexts.values_mut() {
            for owner in &mut context.owners {
                if &owner.route == route {
                    owner.revoked = true;
                }
            }
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
        }
        self.enabled.retain(|_, target| targets.contains(target));
        out
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
                    Recipient::Client { route, id, owner } => {
                        let live = context.owners.iter().any(|o| o.id == owner && !o.revoked);
                        let result = match completion.result {
                            Err(error) => Err(failed(error)),
                            Ok(result) => {
                                let ordinary_rejection =
                                    matches!(result, AutomationResult::Rejected { .. });
                                let converted = result.validate_for(&pending.request).map_err(failed).and_then(|()| {
                                    if crate::runtime_automation::has_handles(&result)
                                        && (completion.continuation == SessionContinuation::Terminal
                                            || host.identity() != Some(context.identity))
                                    {
                                        return Err(failed("Destroyed Runtime contexts cannot publish object handles"));
                                    }
                                    let exception_id = if matches!(result, AutomationResult::Exception { .. }) {
                                        self.next_exception = self.next_exception.checked_add(1)
                                            .filter(|id| *id <= i32::MAX as u32)
                                            .ok_or_else(|| failed("Runtime exception identity space exhausted"))?;
                                        self.next_exception
                                    } else { 0 };
                                    response(context.id, owner, exception_id, result)
                                });
                                // The worker stages callback-free CDP text admission
                                // atomically. Defensive codec/protocol failure cannot
                                // leave inaccessible newly committed handles alive.
                                if converted.is_err()
                                    && !ordinary_rejection
                                    && host.identity() == Some(context.identity)
                                {
                                    host.cancel("Runtime result cannot be represented safely by this CDP codec");
                                }
                                converted
                            }
                        };
                        if live {
                            out.push(Delivery::Reply { route, id, result });
                        }
                    }
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
                    // No reliance on timing of Browser's closed flag. Trusted
                    // manager terminal status wins even if a host is inconsistent.
                    out.extend(self.destroyed(target, context));
                    return out;
                }
            }
        }
        let identity = host.identity();
        if context
            .as_ref()
            .is_some_and(|context| Some(context.identity) != identity)
        {
            let mut old = context.take().unwrap();
            Self::pending_error(
                &mut old,
                "Runtime execution context was destroyed",
                &mut out,
            );
            out.extend(self.destroyed(target, &old));
        }
        if context.is_none() {
            if let Some(identity) = identity {
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
                    pending: None,
                };
                for (route, enabled_target) in &self.enabled {
                    if enabled_target == target {
                        out.push(Self::created(target, &new, host, route));
                    }
                }
                context = Some(new);
            }
        }
        if let Some(mut context) = context {
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
                        host.cancel("Runtime revoked-owner cleanup cannot enter the existing worker allowance");
                        out.extend(self.destroyed(target, &context));
                        return out;
                    }
                }
            }
            self.contexts.insert(target.to_owned(), context);
        }
        out
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
                            out.push(Self::created(target, context, host, route));
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
                ()
            }),
            _ => self.admit(target, host, route, id.clone(), method, params),
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

    fn admit(
        &mut self,
        target: &str,
        host: &mut impl Host,
        route: &Route,
        id: Value,
        method: &str,
        params: &Value,
    ) -> Result<(), (i32, String)> {
        let context = self.contexts.get(target).ok_or_else(|| {
            failed("Runtime requires an enabled, loaded, usable page script realm")
        })?;
        if context.pending.is_some() || !host.idle() {
            return Err(failed("A page transaction or Runtime result is pending"));
        }
        let old_owner = context
            .owners
            .iter()
            .find(|owner| &owner.route == route && !owner.revoked)
            .map(|o| o.id);
        let owner = if let Some(owner) = old_owner {
            owner
        } else {
            if context.owners.len() >= MAX_OWNERS
                || context.issued_owner >= MAX_OWNER_IDENTITIES as u64
            {
                return Err(failed("Runtime route-owner limit reached for this worker"));
            }
            context
                .issued_owner
                .checked_add(1)
                .ok_or_else(|| failed("Runtime owner identity space exhausted"))?
        };
        let operation = operation(method, params, context.id, owner)?;
        let request = AutomationRequest {
            owner,
            operation,
            edits: Vec::new(),
        };
        request.validate().map_err(invalid)?;
        let token = self.token()?;
        host.dispatch(request.clone(), token).map_err(failed)?;
        let context = self.contexts.get_mut(target).unwrap();
        if old_owner.is_none() {
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
fn context_option(params: &Value, key: &str, context: u32) -> Result<(), (i32, String)> {
    if let Some(value) = params.get(key) {
        let id = value
            .as_u64()
            .filter(|id| *id > 0 && *id <= i32::MAX as u64)
            .ok_or_else(|| invalid(format!("{key} must be a positive execution context ID")))?;
        if id != u64::from(context) {
            return Err(failed("Unknown or foreign execution context"));
        }
    }
    Ok(())
}
fn source(params: &Value, key: &str) -> Result<String, (i32, String)> {
    let source = string(params, key)?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(invalid("Runtime source exceeds 16 KiB"));
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
fn object(value: &str, context: u32, owner: u64) -> Result<u64, (i32, String)> {
    // Canonical spelling prevents aliases; opaque IDs never carry authority by
    // themselves. Both route owner and context must match before child lookup.
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
                && c == context
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
    context: u32,
    owner: u64,
) -> Result<Operation, (i32, String)> {
    match method {
        "Runtime.evaluate" => {
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
            context_option(params, "contextId", context)?;
            Ok(Operation::Evaluate {
                source: source(params, "expression")?,
                return_by_value: boolean(params, "returnByValue", false)?,
                group: group(params)?,
            })
        }
        "Runtime.callFunctionOn" => {
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
            if params.get("objectId").is_some() && params.get("executionContextId").is_some() {
                return Err(invalid("Specify objectId or executionContextId, not both"));
            }
            context_option(params, "executionContextId", context)?;
            let receiver = params
                .get("objectId")
                .map(|_| object(string(params, "objectId")?, context, owner))
                .transpose()?;
            let arguments = arguments(params, context, owner)?;
            Ok(Operation::CallFunction {
                source: source(params, "functionDeclaration")?,
                receiver,
                arguments,
                return_by_value: boolean(params, "returnByValue", false)?,
                group: group(params)?,
            })
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
            Ok(Operation::GetOwnProperties {
                object: object(string(params, "objectId")?, context, owner)?,
            })
        }
        "Runtime.releaseObject" => {
            keys(params, &["objectId"])?;
            Ok(Operation::ReleaseObject {
                object: object(string(params, "objectId")?, context, owner)?,
            })
        }
        "Runtime.releaseObjectGroup" => {
            keys(params, &["objectGroup"])?;
            Ok(Operation::ReleaseGroup {
                group: group(params)?.ok_or_else(|| invalid("objectGroup is required"))?,
            })
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
    // Bounded serializer counts input bytes without cloning large strings/trees.
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
            // This explicitly narrowed exception shape omits unavailable source
            // positions and values rather than inventing either for full CDP.
            let exception = exception
                .as_ref()
                .map(|value| remote(context, owner, value))
                .transpose()?;
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
