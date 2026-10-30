//! Typed, bounded Console protocol between isolated Boa workers and the host UI.
//! Never carries JavaScript roots across IPC; object inspection uses scalar IDs.
use serde::{Deserialize, Serialize};

pub const MAX_CONSOLE_ENTRIES_PER_TX: usize = 128;
pub const MAX_CONSOLE_TEXT_BYTES: usize = 2048;
pub const MAX_CONSOLE_WIRE_BYTES: usize = 64 * 1024;
pub const MAX_CONSOLE_SOURCE_BYTES: usize = 16 * 1024;
pub const MAX_CONSOLE_PROPERTIES: usize = 64;
pub const MAX_CONSOLE_INSPECTED_NODES: usize = 5;

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", deny_unknown_fields)]
pub enum ConsoleWireLevel {
    Log,
    Info,
    Debug,
    Warn,
    Error,
    Table,
    Dir,
    Group,
    GroupCollapsed,
    GroupEnd,
    Clear,
    InspectNode(usize),
    Copy(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleWireEntry {
    pub level: ConsoleWireLevel,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<usize>,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub group_depth: u8,
}

impl ConsoleWireEntry {
    pub fn validate(&self) -> Result<(), String> {
        if self.text.len() > MAX_CONSOLE_TEXT_BYTES * 2 {
            return Err("Console entry text exceeds byte limit".into());
        }
        if let ConsoleWireLevel::Copy(text) = &self.level
            && text.len() > MAX_CONSOLE_WIRE_BYTES
        {
            return Err("Console copy payload exceeds byte limit".into());
        }
        if self.group_depth > 32 {
            return Err("Console group depth exceeds limit".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ConsoleCommand {
    Evaluate {
        source: String,
        #[serde(default)]
        inspected_nodes: Vec<usize>,
    },
    InspectObject {
        object_id: u64,
    },
    ClearObjects,
}

impl ConsoleCommand {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Evaluate {
                source,
                inspected_nodes,
            } => {
                if source.len() > MAX_CONSOLE_SOURCE_BYTES {
                    return Err("Console source exceeds 16 KiB limit".into());
                }
                if inspected_nodes.len() > MAX_CONSOLE_INSPECTED_NODES {
                    return Err("Console inspected node history exceeds limit".into());
                }
                Ok(())
            }
            Self::InspectObject { object_id } => {
                if *object_id == 0 {
                    return Err("Console object ID must be nonzero".into());
                }
                Ok(())
            }
            Self::ClearObjects => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsoleValueKind {
    Undefined,
    Null,
    Boolean,
    Number,
    BigInt,
    String,
    Symbol,
    Function,
    Array,
    Node,
    Error,
    Object,
    Exception,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolePropertyPreview {
    pub name: String,
    pub value: String,
    pub kind: ConsoleValueKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<usize>,
    pub enumerable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ConsoleResult {
    Evaluated {
        value_kind: ConsoleValueKind,
        preview: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object_id: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node_id: Option<usize>,
    },
    Properties {
        object_id: u64,
        entries: Vec<ConsolePropertyPreview>,
    },
    Cleared,
    Rejected {
        diagnostic: String,
    },
}

impl ConsoleResult {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Evaluated { preview, .. } => {
                if preview.len() > MAX_CONSOLE_TEXT_BYTES * 2 {
                    return Err("Console evaluation preview exceeds byte limit".into());
                }
                Ok(())
            }
            Self::Properties {
                object_id, entries, ..
            } => {
                if *object_id == 0 {
                    return Err("Console properties object ID must be nonzero".into());
                }
                if entries.len() > MAX_CONSOLE_PROPERTIES + 16 {
                    return Err("Console property count exceeds limit".into());
                }
                for entry in entries {
                    if entry.name.len() > 512 || entry.value.len() > MAX_CONSOLE_TEXT_BYTES {
                        return Err("Console property preview exceeds byte limit".into());
                    }
                }
                Ok(())
            }
            Self::Cleared => Ok(()),
            Self::Rejected { diagnostic } => {
                if diagnostic.len() > MAX_CONSOLE_TEXT_BYTES {
                    return Err("Console diagnostic exceeds byte limit".into());
                }
                Ok(())
            }
        }
    }
}
