//! Typed, bounded interaction between the parent and one retained page realm.
//! Transport identities and authority are supplied by the parent, never script.
use crate::document::Node;
use mg_butane::runtime::AllocationReport;
use serde::{Deserialize, Serialize};

pub const MAX_EDITS: usize = 128;
pub const MAX_EDIT_BYTES: usize = 8191;
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
pub const MAX_EVENTS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlEdit {
    pub node: usize,
    pub version: u64,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlAck {
    pub node: usize,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionInput {
    pub kind: InputKind,
    pub edits: Vec<ControlEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum InputKind {
    Click {
        target: usize,
    },
    Submit {
        form: usize,
        submitter: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum DefaultAction {
    None,
    FollowLink {
        node: usize,
    },
    SubmitForm {
        form: usize,
        submitter: Option<usize>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RealmState {
    Ready,
    Fatal,
    Closed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventOutcome {
    pub click_canceled: Option<bool>,
    pub submit_canceled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArenaSnapshot {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionReply {
    pub revision: u64,
    pub snapshot: Option<ArenaSnapshot>,
    pub outcome: EventOutcome,
    pub default_action: DefaultAction,
    pub navigation: Option<String>,
    pub errors: Vec<String>,
    pub scripts_executed: usize,
    pub allocations: Option<AllocationReport>,
    pub state: RealmState,
    pub acknowledgements: Vec<ControlAck>,
}
