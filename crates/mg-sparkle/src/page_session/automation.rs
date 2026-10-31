//! Strict, data-only modern Runtime transactions. No JavaScript roots cross IPC.
use super::{ControlEdit, MAX_EDIT_BYTES, MAX_EDITS};
use serde::{Deserialize, Serialize};

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

pub const MAX_SOURCE_BYTES: usize = 384 * 1024;
pub const MAX_OPERATION_BYTES: usize = 448 * 1024;
pub const MAX_AUTOMATION_FRAME_BYTES: usize = 512 * 1024;
pub const MAX_WORLD_BYTES: usize = 128;
pub const MAX_EDITS_BYTES: usize = 24 * 1024;
pub const MAX_ARGUMENT_BYTES: usize = 64 * 1024;
pub const MAX_RESULT_BYTES: usize = 128 * 1024;
pub const MAX_STRING_UNITS: usize = 4096;
pub const MAX_ROOTS: usize = 256;
pub const MAX_ISSUED: usize = 512;
pub const MAX_RESULT_ROOTS: usize = 32;
pub const MAX_PROPERTIES: usize = 64;
pub const MAX_OWNERS: usize = 32;
pub const MAX_OWNER_IDENTITIES: usize = 64;
pub const MAX_GROUPS: usize = 32;
pub const MAX_GROUP_BYTES: usize = 64;
pub const MAX_ARGUMENTS: usize = 32;
pub const MAX_DEPTH: usize = 64;
pub const MAX_ENTRIES: usize = 512;
pub const MAX_TREE_UNITS: usize = 16_384;
pub const MAX_DIAGNOSTIC_BYTES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRequest {
    pub owner: u64,
    pub operation: Operation,
    pub edits: Vec<ControlEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Operation {
    Evaluate {
        source: String,
        return_by_value: bool,
        #[serde(deserialize_with = "required_option")]
        group: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        await_promise: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        user_gesture: bool,
    },
    CallFunction {
        source: String,
        #[serde(deserialize_with = "required_option")]
        receiver: Option<u64>,
        arguments: Vec<Argument>,
        return_by_value: bool,
        #[serde(deserialize_with = "required_option")]
        group: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        await_promise: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        user_gesture: bool,
    },
    CreateIsolatedWorld {
        world: String,
    },
    ResolveNode {
        node: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        world: Option<String>,
        #[serde(deserialize_with = "required_option")]
        group: Option<String>,
    },
    GetOwnProperties {
        object: u64,
    },
    ReleaseObject {
        object: u64,
    },
    ReleaseGroup {
        group: String,
    },
    ReleaseOwners {
        owners: Vec<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Argument {
    Undefined,
    Number(Number),
    BigInt(String),
    Value(SerializedValue),
    Object(u64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Number {
    Finite(f64),
    NaN,
    PositiveInfinity,
    NegativeInfinity,
    NegativeZero,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SerializedValue {
    Null,
    Boolean(bool),
    FiniteNumber(f64),
    String(Vec<u16>),
    Array(Vec<SerializedValue>),
    Object(Vec<(Vec<u16>, SerializedValue)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectKind {
    Object,
    Array,
    Function,
    Promise,
    Proxy,
    Error,
    Node { node: usize },
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RemoteValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(Number),
    String(Vec<u16>),
    BigInt(String),
    Object { object: u64, kind: ObjectKind },
    ByValue(SerializedValue),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnProperty {
    pub name: Vec<u16>,
    pub enumerable: bool,
    pub configurable: bool,
    pub descriptor: PropertyDescriptor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PropertyDescriptor {
    Data {
        value: RemoteValue,
        writable: bool,
    },
    Accessor {
        #[serde(deserialize_with = "required_option")]
        get: Option<RemoteValue>,
        #[serde(deserialize_with = "required_option")]
        set: Option<RemoteValue>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AutomationResult {
    Value(RemoteValue),
    Exception {
        diagnostic: String,
        #[serde(deserialize_with = "required_option")]
        exception: Option<RemoteValue>,
    },
    Properties(Vec<OwnProperty>),
    Released,
    Rejected {
        diagnostic: String,
    },
}

/// Count actual JSON bytes without allocating a second serialized copy.
pub fn encoded_size<T: Serialize + ?Sized>(value: &T, limit: usize) -> Result<usize, String> {
    struct Counter {
        used: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.used = self
                .used
                .checked_add(bytes.len())
                .filter(|n| *n <= self.limit)
                .ok_or_else(|| std::io::Error::other("Automation encoded limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { used: 0, limit };
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| "Automation encoded limit exceeded".to_string())?;
    Ok(counter.used)
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn units(value: &[u16]) -> Result<(), String> {
    require(
        value.len() <= MAX_STRING_UNITS,
        "Automation string limit exceeded",
    )
}
fn object_id(id: u64) -> bool {
    id != 0 && id <= MAX_ISSUED as u64
}
fn group(value: Option<&str>) -> Result<(), String> {
    require(
        value.is_none_or(|s| s.len() <= MAX_GROUP_BYTES),
        "Automation group limit exceeded",
    )
}
impl Number {
    pub fn validate(&self) -> Result<(), String> {
        if let Self::Finite(value) = self {
            require(
                value.is_finite() && !(*value == 0.0 && value.is_sign_negative()),
                "Noncanonical finite number",
            )
        } else {
            Ok(())
        }
    }
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Finite(v) => *v,
            Self::NaN => f64::NAN,
            Self::PositiveInfinity => f64::INFINITY,
            Self::NegativeInfinity => f64::NEG_INFINITY,
            Self::NegativeZero => -0.0,
        }
    }
    pub fn from_f64(value: f64) -> Self {
        if value.is_nan() {
            Self::NaN
        } else if value == f64::INFINITY {
            Self::PositiveInfinity
        } else if value == f64::NEG_INFINITY {
            Self::NegativeInfinity
        } else if value == 0.0 && value.is_sign_negative() {
            Self::NegativeZero
        } else {
            Self::Finite(value)
        }
    }
}

/// Allocation-free decimal admission, including the actual 4096-bit magnitude.
pub fn validate_bigint(value: &str) -> Result<(), String> {
    let digits = value.strip_prefix('-').unwrap_or(value);
    require(
        !digits.is_empty()
            && digits.len() <= 1234
            && digits.bytes().all(|b| b.is_ascii_digit())
            && (digits.len() == 1 || !digits.starts_with('0'))
            && value != "-0",
        "Invalid BigInt decimal",
    )?;
    let mut limbs = [0u32; 128];
    for digit in digits.bytes() {
        let mut carry = u64::from(digit - b'0');
        for limb in &mut limbs {
            let next = u64::from(*limb) * 10 + carry;
            *limb = next as u32;
            carry = next >> 32;
        }
        require(carry == 0, "Automation BigInt limit exceeded")?;
    }
    Ok(())
}

#[derive(Default)]
struct TreeBudget {
    entries: usize,
    units: usize,
}
impl TreeBudget {
    fn text(&mut self, text: &[u16]) -> Result<(), String> {
        units(text)?;
        self.units = self
            .units
            .checked_add(text.len())
            .ok_or("Automation tree overflow")?;
        require(
            self.units <= MAX_TREE_UNITS,
            "Automation tree string limit exceeded",
        )
    }
    fn value(&mut self, value: &SerializedValue, depth: usize) -> Result<(), String> {
        self.entries += 1;
        require(
            depth <= MAX_DEPTH && self.entries <= MAX_ENTRIES,
            "Automation tree limit exceeded",
        )?;
        match value {
            SerializedValue::Null | SerializedValue::Boolean(_) => Ok(()),
            SerializedValue::FiniteNumber(v) => Number::Finite(*v).validate(),
            SerializedValue::String(s) => self.text(s),
            SerializedValue::Array(values) => {
                require(
                    values.len() <= MAX_ENTRIES - self.entries,
                    "Automation tree entry limit exceeded",
                )?;
                for value in values {
                    self.value(value, depth + 1)?;
                }
                Ok(())
            }
            SerializedValue::Object(values) => {
                require(
                    values.len() <= MAX_ENTRIES - self.entries,
                    "Automation tree entry limit exceeded",
                )?;
                for (i, (key, value)) in values.iter().enumerate() {
                    require(
                        !values[..i].iter().any(|(old, _)| old == key),
                        "Duplicate automation property",
                    )?;
                    self.text(key)?;
                    self.value(value, depth + 1)?;
                }
                Ok(())
            }
        }
    }
}
impl SerializedValue {
    pub fn validate(&self) -> Result<(), String> {
        TreeBudget::default().value(self, 0)
    }
}
impl AutomationRequest {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.edits.len() <= MAX_EDITS,
            "Automation edit count exceeded",
        )?;
        for (i, edit) in self.edits.iter().enumerate() {
            require(
                edit.value.len() <= MAX_EDIT_BYTES
                    && !self.edits[..i].iter().any(|old| old.node == edit.node),
                "Invalid automation control edit",
            )?;
        }
        encoded_size(&self.edits, MAX_EDITS_BYTES)?;
        require(
            (self.owner == 0) == matches!(self.operation, Operation::ReleaseOwners { .. }),
            "Invalid automation owner authority",
        )?;
        match &self.operation {
            Operation::Evaluate {
                source,
                group: g,
                world: w,
                ..
            } => {
                require(
                    source.len() <= MAX_SOURCE_BYTES,
                    "Automation source limit exceeded",
                )?;
                group(g.as_deref())?;
                require(
                    w.as_deref()
                        .is_none_or(|s| !s.is_empty() && s.len() <= MAX_WORLD_BYTES),
                    "Invalid automation world name",
                )?;
            }
            Operation::CallFunction {
                source,
                receiver,
                arguments,
                group: g,
                world: w,
                ..
            } => {
                require(
                    source.len() <= MAX_SOURCE_BYTES,
                    "Automation source limit exceeded",
                )?;
                group(g.as_deref())?;
                require(
                    w.as_deref()
                        .is_none_or(|s| !s.is_empty() && s.len() <= MAX_WORLD_BYTES),
                    "Invalid automation world name",
                )?;
                require(
                    receiver.is_none_or(object_id) && arguments.len() <= MAX_ARGUMENTS,
                    "Invalid automation call arguments",
                )?;
                let mut tree = TreeBudget::default();
                for argument in arguments {
                    match argument {
                        Argument::Undefined => {}
                        Argument::Number(n) => n.validate()?,
                        Argument::BigInt(n) => validate_bigint(n)?,
                        Argument::Object(id) => {
                            require(object_id(*id), "Invalid automation object")?
                        }
                        Argument::Value(v) => tree.value(v, 0)?,
                    }
                }
                encoded_size(arguments, MAX_ARGUMENT_BYTES)?;
            }
            Operation::CreateIsolatedWorld { world: w } => {
                require(
                    !w.is_empty() && w.len() <= MAX_WORLD_BYTES,
                    "Invalid automation world name",
                )?;
            }
            Operation::ResolveNode {
                node,
                world: w,
                group: g,
            } => {
                require(*node < 50_000, "Invalid automation DOM node")?;
                require(
                    w.as_deref()
                        .is_none_or(|s| !s.is_empty() && s.len() <= MAX_WORLD_BYTES),
                    "Invalid automation world name",
                )?;
                group(g.as_deref())?;
            }
            Operation::GetOwnProperties { object } | Operation::ReleaseObject { object } => {
                require(object_id(*object), "Invalid automation object")?
            }
            Operation::ReleaseGroup { group: g } => group(Some(g))?,
            Operation::ReleaseOwners { owners } => {
                require(
                    self.edits.is_empty() && !owners.is_empty() && owners.len() <= MAX_OWNERS,
                    "Invalid owner cleanup",
                )?;
                for (i, id) in owners.iter().enumerate() {
                    require(
                        *id != 0 && !owners[..i].contains(id),
                        "Invalid owner cleanup identity",
                    )?;
                }
            }
        }
        if matches!(
            self.operation,
            Operation::ReleaseObject { .. } | Operation::ReleaseGroup { .. }
        ) {
            require(self.edits.is_empty(), "Cleanup cannot carry control edits")?;
        }
        encoded_size(&self.operation, MAX_OPERATION_BYTES)?;
        encoded_size(self, MAX_AUTOMATION_FRAME_BYTES)?;
        Ok(())
    }
}
impl RemoteValue {
    fn validate_with(&self, roots: &mut usize) -> Result<(), String> {
        match self {
            Self::Undefined | Self::Null | Self::Boolean(_) => Ok(()),
            Self::Number(n) => n.validate(),
            Self::String(s) => units(s),
            Self::BigInt(n) => validate_bigint(n),
            Self::ByValue(v) => v.validate(),
            Self::Object { object, kind } => {
                if let ObjectKind::Node { node } = kind {
                    require(*node < 50_000, "Invalid automation DOM node")?;
                }
                *roots += 1;
                require(
                    object_id(*object) && *roots <= MAX_RESULT_ROOTS,
                    "Automation result root limit exceeded",
                )
            }
        }
    }
}
impl AutomationResult {
    pub fn validate(&self) -> Result<(), String> {
        let mut roots = 0;
        match self {
            Self::Value(v) => v.validate_with(&mut roots)?,
            Self::Exception {
                diagnostic,
                exception,
            } => {
                require(
                    diagnostic.len() <= MAX_DIAGNOSTIC_BYTES,
                    "Automation diagnostic limit exceeded",
                )?;
                if let Some(value) = exception {
                    value.validate_with(&mut roots)?;
                }
            }
            Self::Rejected { diagnostic } => require(
                diagnostic.len() <= MAX_DIAGNOSTIC_BYTES,
                "Automation diagnostic limit exceeded",
            )?,
            Self::Released => {}
            Self::Properties(properties) => {
                require(
                    properties.len() <= MAX_PROPERTIES,
                    "Automation descriptor limit exceeded",
                )?;
                for (i, p) in properties.iter().enumerate() {
                    units(&p.name)?;
                    require(
                        !properties[..i].iter().any(|old| old.name == p.name),
                        "Duplicate automation descriptor",
                    )?;
                    match &p.descriptor {
                        PropertyDescriptor::Data { value, .. } => {
                            require(
                                !matches!(value, RemoteValue::ByValue(_)),
                                "Own descriptors require remote values",
                            )?;
                            value.validate_with(&mut roots)?
                        }
                        PropertyDescriptor::Accessor { get, set } => {
                            for value in [get, set].into_iter().flatten() {
                                require(
                                    matches!(
                                        value,
                                        RemoteValue::Object {
                                            kind: ObjectKind::Function | ObjectKind::Proxy,
                                            ..
                                        }
                                    ),
                                    "Invalid automation accessor",
                                )?;
                                value.validate_with(&mut roots)?;
                            }
                        }
                    }
                }
            }
        }
        encoded_size(self, MAX_RESULT_BYTES)?;
        Ok(())
    }
    pub fn validate_for(&self, request: &AutomationRequest) -> Result<(), String> {
        request.validate()?;
        self.validate()?;
        let valid = match (&request.operation, self) {
            (_, Self::Rejected { .. }) => true,
            (
                Operation::Evaluate {
                    return_by_value, ..
                }
                | Operation::CallFunction {
                    return_by_value, ..
                },
                Self::Value(value),
            ) => {
                if *return_by_value {
                    !matches!(value, RemoteValue::Object { .. })
                } else {
                    !matches!(value, RemoteValue::ByValue(_))
                }
            }
            (
                Operation::Evaluate { .. } | Operation::CallFunction { .. },
                Self::Exception { .. },
            ) => true,
            (Operation::GetOwnProperties { .. }, Self::Properties(_)) => true,
            (Operation::CreateIsolatedWorld { .. }, Self::Released) => true,
            (
                Operation::ResolveNode { .. },
                Self::Value(
                    RemoteValue::Object {
                        kind: ObjectKind::Node { .. },
                        ..
                    }
                    | RemoteValue::Null,
                ),
            ) => true,
            (
                Operation::ReleaseObject { .. }
                | Operation::ReleaseGroup { .. }
                | Operation::ReleaseOwners { .. },
                Self::Released,
            ) => true,
            _ => false,
        };
        require(valid, "Automation operation/result mismatch")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(operation: Operation) -> AutomationRequest {
        AutomationRequest {
            owner: 1,
            operation,
            edits: vec![],
        }
    }
    #[test]
    fn source_owner_encoding_and_result_correspondence() {
        let mut r = request(Operation::Evaluate {
            source: "1".repeat(MAX_SOURCE_BYTES),
            return_by_value: false,
            group: None,
            world: None,
            await_promise: false,
            user_gesture: false,
        });
        assert!(r.validate().is_ok());
        if let Operation::Evaluate { source, .. } = &mut r.operation {
            source.push('1');
        }
        assert!(r.validate().is_err());
        r = request(Operation::Evaluate {
            source: "\0".repeat(80_000),
            return_by_value: false,
            group: None,
            world: None,
            await_promise: false,
            user_gesture: false,
        });
        assert!(r.validate().is_err(), "encoded operation counts escaping");
        r = request(Operation::GetOwnProperties { object: 1 });
        assert!(AutomationResult::Released.validate_for(&r).is_err());
        assert!(
            AutomationResult::Properties(vec![])
                .validate_for(&r)
                .is_ok()
        );
        r.owner = 0;
        assert!(r.validate().is_err());
        r.operation = Operation::ReleaseOwners { owners: vec![1, 1] };
        assert!(r.validate().is_err());
        r.operation = Operation::ReleaseOwners { owners: vec![1, 2] };
        assert!(r.validate().is_ok());
    }
    #[test]
    fn primitives_are_canonical_and_tree_bounds_are_aggregate() {
        assert!(Number::Finite(-0.0).validate().is_err());
        assert!(Number::Finite(f64::INFINITY).validate().is_err());
        assert!(Number::NegativeZero.validate().is_ok());
        for bad in ["", "+1", "01", "-0", "1n", " 1", "1.0"] {
            assert!(validate_bigint(bad).is_err());
        }
        assert!(validate_bigint("-123456789012345678901234567890").is_ok());
        assert!(
            validate_bigint(&"9".repeat(1234)).is_err(),
            "4096 bits, not decimal length only"
        );
        let too_many = SerializedValue::Array(vec![SerializedValue::Null; MAX_ENTRIES]);
        assert!(too_many.validate().is_err(), "root also counts");
        let duplicate = SerializedValue::Object(vec![
            (vec![65], SerializedValue::Null),
            (vec![65], SerializedValue::Null),
        ]);
        assert!(duplicate.validate().is_err());
        assert!(
            SerializedValue::String(vec![0xd800]).validate().is_ok(),
            "worker codec is lossless"
        );
    }
    #[test]
    fn descriptors_are_complete_strict_and_bounded() {
        let invalid = serde_json::json!({"Evaluate":{"source":"1","return_by_value":false,"group":null,"extra":true}});
        assert!(serde_json::from_value::<Operation>(invalid).is_err());
        assert!(
            serde_json::from_value::<Operation>(
                serde_json::json!({"Evaluate":{"source":"1","return_by_value":false}})
            )
            .is_err(),
            "missing group is not explicit null"
        );
        assert!(
            serde_json::from_value::<AutomationResult>(
                serde_json::json!({"Exception":{"diagnostic":"caught"}})
            )
            .is_err(),
            "missing exception is not explicit null"
        );
        let r = request(Operation::GetOwnProperties { object: 1 });
        let p = OwnProperty {
            name: vec![65],
            enumerable: true,
            configurable: true,
            descriptor: PropertyDescriptor::Data {
                value: RemoteValue::Null,
                writable: true,
            },
        };
        assert!(
            AutomationResult::Properties(vec![p.clone(), p])
                .validate_for(&r)
                .is_err(),
            "duplicate keys"
        );
        let bad = AutomationResult::Exception {
            diagnostic: "x".repeat(513),
            exception: None,
        };
        assert!(bad.validate().is_err());
        let bad_get = OwnProperty {
            name: vec![],
            enumerable: false,
            configurable: false,
            descriptor: PropertyDescriptor::Accessor {
                get: Some(RemoteValue::Boolean(true)),
                set: None,
            },
        };
        assert!(
            AutomationResult::Properties(vec![bad_get])
                .validate()
                .is_err()
        );
    }

    #[test]
    fn exact_handle_domain_and_return_modes_reject_forged_data() {
        let r = request(Operation::GetOwnProperties { object: 513 });
        assert!(r.validate().is_err());
        assert!(
            AutomationResult::Value(RemoteValue::Object {
                object: 513,
                kind: ObjectKind::Object
            })
            .validate()
            .is_err()
        );
        let r = request(Operation::Evaluate {
            source: "({})".into(),
            return_by_value: false,
            group: None,
            world: None,
            await_promise: false,
            user_gesture: false,
        });
        assert!(
            AutomationResult::Value(RemoteValue::ByValue(SerializedValue::Object(vec![])))
                .validate_for(&r)
                .is_err()
        );
        let r = request(Operation::Evaluate {
            source: "({})".into(),
            return_by_value: true,
            group: None,
            world: None,
            await_promise: false,
            user_gesture: false,
        });
        assert!(
            AutomationResult::Value(RemoteValue::Object {
                object: 1,
                kind: ObjectKind::Object
            })
            .validate_for(&r)
            .is_err()
        );
        assert!(
            SerializedValue::FiniteNumber(-0.0).validate().is_err(),
            "compound negative zero is explicitly unsupported, not normalized"
        );
        assert!(validate_bigint(&"1".repeat(1234)).is_err());
        assert!(validate_bigint(&format!("1{}", "0".repeat(1233))).is_ok());
    }
}
