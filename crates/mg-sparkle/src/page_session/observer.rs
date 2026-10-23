//! Bounded cooperative Window-harness observations, never a hostile-page proof.
//!
//! The wire owns no JS handles. The parent stages a delta and publishes it only
//! after its existing combined accounting/projection transaction succeeds.
use serde::{Deserialize, Serialize};
use std::fmt;
use std::mem::size_of;

pub const MAX_TESTS: usize = 256;
pub const MAX_RECORDS: usize = 257;
pub const MAX_FIELD_UNITS: usize = 4096;
pub const MAX_COPIED_UNITS: usize = 131_072;

// Serde's derived unit enums also accept externally tagged objects. The wire
// contract requires actual strings for every kind/status/fault/contract tag.
macro_rules! string_enum {
    ($ty:ident { $($variant:ident),+ $(,)? }) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct EnumVisitor;
                impl<'de> serde::de::Visitor<'de> for EnumVisitor {
                    type Value = $ty;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str("an exact observer enum string")
                    }
                    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<$ty, E> {
                        match value {
                            $(stringify!($variant) => Ok($ty::$variant),)+
                            _ => Err(E::unknown_variant(value, &[$(stringify!($variant)),+])),
                        }
                    }
                }
                d.deserialize_str(EnumVisitor)
            }
        }
    };
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum ObserverContract {
    PinnedWindowHarnessV1,
}
string_enum!(ObserverContract {
    PinnedWindowHarnessV1
});

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ObserverInit {
    #[default]
    Disabled,
    WindowHarness {
        contract: ObserverContract,
        run_id: u64,
        bootstrap_node: u32,
    },
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum ObserverExpectation {
    #[default]
    Disabled,
    WindowHarness {
        run_id: u64,
    },
}

impl ObserverInit {
    pub fn validate(self) -> Result<(), ObserverValidationError> {
        self.expectation().validate()
    }

    pub fn expectation(self) -> ObserverExpectation {
        match self {
            Self::Disabled => ObserverExpectation::Disabled,
            Self::WindowHarness { run_id, .. } => ObserverExpectation::WindowHarness { run_id },
        }
    }
}

impl ObserverExpectation {
    pub fn validate(self) -> Result<(), ObserverValidationError> {
        match self {
            Self::WindowHarness { run_id: 0 } => Err(ObserverValidationError::Configuration),
            _ => Ok(()),
        }
    }
}

/// Owned lossless units. Cloning must be an explicit, pre-admitted owner action.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Utf16(Vec<u16>);

impl Utf16 {
    pub fn from_units(units: Vec<u16>) -> Result<Self, ObserverValidationError> {
        if units.len() > MAX_FIELD_UNITS || units.capacity() > MAX_FIELD_UNITS {
            return Err(ObserverValidationError::FieldBound);
        }
        Ok(Self(units))
    }

    pub fn as_units(&self) -> &[u16] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl TryFrom<Vec<u16>> for Utf16 {
    type Error = ObserverValidationError;

    fn try_from(value: Vec<u16>) -> Result<Self, Self::Error> {
        Self::from_units(value)
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum OptionalUtf16 {
    Null,
    Undefined,
    Text { units: Utf16 },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum SubtestStatus {
    Pass,
    Fail,
    Timeout,
    NotRun,
    PreconditionFailed,
}
string_enum!(SubtestStatus {
    Pass,
    Fail,
    Timeout,
    NotRun,
    PreconditionFailed
});

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum HarnessStatus {
    Ok,
    Error,
    Timeout,
    PreconditionFailed,
}
string_enum!(HarnessStatus {
    Ok,
    Error,
    Timeout,
    PreconditionFailed
});

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestRecord {
    pub id: u16,
    pub index: u16,
    pub name: Utf16,
    pub status: SubtestStatus,
    pub message: OptionalUtf16,
    pub stack: OptionalUtf16,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessRecord {
    pub status: HarnessStatus,
    pub message: OptionalUtf16,
    pub stack: OptionalUtf16,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ObserverRecord {
    Result {
        test: TestRecord,
    },
    Completion {
        harness: HarnessRecord,
        inventory: Vec<TestRecord>,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum ObserverProgress {
    Waiting,
    Settling,
    Finished,
}
string_enum!(ObserverProgress {
    Waiting,
    Settling,
    Finished
});

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum ObserverFault {
    RejectedInit,
    InvalidConfiguration,
    MalformedCapture,
    DuplicateResult,
    DuplicateCompletion,
    ResultAfterCompletion,
    CaptureOverflow,
    StorageAdmission,
    AuthorityUnavailable,
    PrivacyLost,
    RealmTerminated,
    NavigationProposed,
    InvariantFailure,
}
string_enum!(ObserverFault {
    RejectedInit,
    InvalidConfiguration,
    MalformedCapture,
    DuplicateResult,
    DuplicateCompletion,
    ResultAfterCompletion,
    CaptureOverflow,
    StorageAdmission,
    AuthorityUnavailable,
    PrivacyLost,
    RealmTerminated,
    NavigationProposed,
    InvariantFailure
});

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ObserverReply {
    Disabled,
    WindowHarness {
        run_id: u64,
        first_ordinal: u32,
        next_ordinal: u32,
        records: Vec<ObserverRecord>,
        progress: ObserverProgress,
        execution_failed: bool,
    },
    ObservationFailed {
        run_id: u64,
        prefix_end: u32,
        fault: ObserverFault,
        execution_failed: bool,
    },
}

impl ObserverReply {
    pub fn rejected_init(expected: ObserverExpectation) -> Self {
        match expected {
            ObserverExpectation::Disabled => Self::Disabled,
            ObserverExpectation::WindowHarness { run_id } => Self::ObservationFailed {
                run_id,
                prefix_end: 0,
                fault: ObserverFault::RejectedInit,
                execution_failed: true,
            },
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ObserverValidationError {
    Configuration,
    Mode,
    Run,
    Cursor,
    FieldBound,
    CopyBound,
    RecordBound,
    Storage,
    Identity,
    DuplicateResult,
    Completion,
    Inventory,
    Reconciliation,
    Progress,
    StickyFailure,
    Frame,
    ParentFailed,
}

impl fmt::Display for ObserverValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "invalid observer configuration",
            Self::Mode => "observer mode mismatch",
            Self::Run => "observer run mismatch",
            Self::Cursor => "invalid observer cursor",
            Self::FieldBound => "observer text field exceeds bound",
            Self::CopyBound => "observer cumulative text exceeds bound",
            Self::RecordBound => "observer record count exceeds bound",
            Self::Storage => "observer storage allocation failed",
            Self::Identity => "invalid observer identity or creation index",
            Self::DuplicateResult => "duplicate observer result",
            Self::Completion => "invalid observer completion",
            Self::Inventory => "invalid observer final inventory",
            Self::Reconciliation => "observer result changed at completion",
            Self::Progress => "invalid observer progress transition",
            Self::StickyFailure => "observer failure was not preserved",
            Self::Frame => "observer contradicts outer frame",
            Self::ParentFailed => "observer parent already failed",
        })
    }
}

impl std::error::Error for ObserverValidationError {}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ObserverParentFailure {
    Validation,
    Projection,
    ActiveCharge,
    Transport,
    BrokerPolicy,
    Deadline,
    Cleanup,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NameReconciliation {
    Unchanged,
    PinnedSanitized,
    FinalOnly,
}

/// Scalar link only: original and final text remain in their actual records.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ReconciledName {
    pub id: u16,
    pub index: u16,
    pub kind: NameReconciliation,
}

/// Allocation-free comparison with the pinned harness's isolated-surrogate
/// spelling. Prefer Unchanged when both permitted observations are identical.
pub fn reconcile_name(
    raw: &[u16],
    final_units: &[u16],
) -> Result<NameReconciliation, ObserverValidationError> {
    if raw == final_units {
        return Ok(NameReconciliation::Unchanged);
    }
    let mut source = 0;
    let mut target = 0;
    while source < raw.len() {
        let unit = raw[source];
        let high = (0xd800..=0xdbff).contains(&unit);
        let low = (0xdc00..=0xdfff).contains(&unit);
        if high
            && raw
                .get(source + 1)
                .is_some_and(|u| (0xdc00..=0xdfff).contains(u))
        {
            if final_units.get(target..target + 2) != Some(&raw[source..source + 2]) {
                return Err(ObserverValidationError::Reconciliation);
            }
            source += 2;
            target += 2;
        } else if high || low {
            let hex = b"0123456789abcdef";
            let spelled = [
                b'U' as u16,
                b'+' as u16,
                hex[((unit >> 12) & 15) as usize] as u16,
                hex[((unit >> 8) & 15) as usize] as u16,
                hex[((unit >> 4) & 15) as usize] as u16,
                hex[(unit & 15) as usize] as u16,
            ];
            if final_units.get(target..target + 6) != Some(spelled.as_slice()) {
                return Err(ObserverValidationError::Reconciliation);
            }
            source += 1;
            target += 6;
        } else {
            if final_units.get(target) != Some(&unit) {
                return Err(ObserverValidationError::Reconciliation);
            }
            source += 1;
            target += 1;
        }
    }
    if target != final_units.len() {
        return Err(ObserverValidationError::Reconciliation);
    }
    Ok(NameReconciliation::PinnedSanitized)
}

impl OptionalUtf16 {
    fn text_units(&self) -> usize {
        match self {
            Self::Text { units } => units.len(),
            _ => 0,
        }
    }

    fn text_capacity(&self) -> usize {
        match self {
            Self::Text { units } => units.0.capacity(),
            _ => 0,
        }
    }
}

impl TestRecord {
    fn text_units(&self) -> usize {
        self.name.len() + self.message.text_units() + self.stack.text_units()
    }

    fn text_capacity(&self) -> usize {
        self.name.0.capacity() + self.message.text_capacity() + self.stack.text_capacity()
    }
}

impl ObserverRecord {
    fn text_units(&self) -> usize {
        match self {
            Self::Result { test } => test.text_units(),
            Self::Completion { harness, inventory } => {
                harness.message.text_units()
                    + harness.stack.text_units()
                    + inventory.iter().map(TestRecord::text_units).sum::<usize>()
            }
        }
    }

    fn heap_capacity_bytes(&self) -> usize {
        match self {
            Self::Result { test } => test.text_capacity() * size_of::<u16>(),
            Self::Completion { harness, inventory } => {
                inventory.capacity() * size_of::<TestRecord>()
                    + (harness.message.text_capacity()
                        + harness.stack.text_capacity()
                        + inventory
                            .iter()
                            .map(TestRecord::text_capacity)
                            .sum::<usize>())
                        * size_of::<u16>()
            }
        }
    }
}

impl ObserverReply {
    /// Frame-local validation; cumulative identities and inventory belong to the
    /// ledger. This also handles values constructed directly rather than decoded.
    pub fn validate_expectation(
        &self,
        expected: ObserverExpectation,
    ) -> Result<(), ObserverValidationError> {
        expected.validate()?;
        match (expected, self) {
            (ObserverExpectation::Disabled, Self::Disabled) => Ok(()),
            (
                ObserverExpectation::WindowHarness { run_id: expected },
                Self::WindowHarness {
                    run_id,
                    first_ordinal,
                    next_ordinal,
                    records,
                    ..
                },
            ) => {
                if *run_id != expected {
                    return Err(ObserverValidationError::Run);
                }
                if records.len() > MAX_RECORDS || records.capacity() > MAX_RECORDS {
                    return Err(ObserverValidationError::RecordBound);
                }
                if *first_ordinal > MAX_RECORDS as u32
                    || *next_ordinal > MAX_RECORDS as u32
                    || first_ordinal.checked_add(records.len() as u32) != Some(*next_ordinal)
                {
                    return Err(ObserverValidationError::Cursor);
                }
                for record in records {
                    match record {
                        ObserverRecord::Result { test } => validate_test(test)?,
                        ObserverRecord::Completion { harness, inventory } => {
                            if inventory.len() > MAX_TESTS || inventory.capacity() > MAX_TESTS {
                                return Err(ObserverValidationError::RecordBound);
                            }
                            validate_optional(&harness.message)?;
                            validate_optional(&harness.stack)?;
                            for test in inventory {
                                validate_test(test)?;
                            }
                        }
                    }
                }
                if records
                    .iter()
                    .map(ObserverRecord::text_units)
                    .sum::<usize>()
                    > MAX_COPIED_UNITS
                {
                    return Err(ObserverValidationError::CopyBound);
                }
                Ok(())
            }
            (
                ObserverExpectation::WindowHarness { run_id: expected },
                Self::ObservationFailed {
                    run_id, prefix_end, ..
                },
            ) => {
                if *run_id != expected {
                    return Err(ObserverValidationError::Run);
                }
                if *prefix_end > MAX_RECORDS as u32 {
                    return Err(ObserverValidationError::Cursor);
                }
                Ok(())
            }
            _ => Err(ObserverValidationError::Mode),
        }
    }
}

fn validate_optional(value: &OptionalUtf16) -> Result<(), ObserverValidationError> {
    if let OptionalUtf16::Text { units } = value {
        if units.len() > MAX_FIELD_UNITS || units.0.capacity() > MAX_FIELD_UNITS {
            return Err(ObserverValidationError::FieldBound);
        }
    }
    Ok(())
}

fn validate_test(test: &TestRecord) -> Result<(), ObserverValidationError> {
    if test.id == 0 || test.id as usize > MAX_TESTS || test.index as usize >= MAX_TESTS {
        return Err(ObserverValidationError::Identity);
    }
    if test.name.len() > MAX_FIELD_UNITS || test.name.0.capacity() > MAX_FIELD_UNITS {
        return Err(ObserverValidationError::FieldBound);
    }
    validate_optional(&test.message)?;
    validate_optional(&test.stack)
}

#[derive(Debug)]
struct IdentityState {
    id_to_index: [Option<u16>; MAX_TESTS],
    index_to_id: [Option<u16>; MAX_TESTS],
    results: [bool; MAX_TESTS],
    identities: u16,
    completion: bool,
}

impl Default for IdentityState {
    fn default() -> Self {
        Self {
            id_to_index: [None; MAX_TESTS],
            index_to_id: [None; MAX_TESTS],
            results: [false; MAX_TESTS],
            identities: 0,
            completion: false,
        }
    }
}

impl IdentityState {
    // Fixed bounded scalar scratch only, never a text/record-history clone.
    fn scratch(&self) -> Self {
        Self {
            id_to_index: self.id_to_index,
            index_to_id: self.index_to_id,
            results: self.results,
            identities: self.identities,
            completion: self.completion,
        }
    }

    fn bind(&mut self, test: &TestRecord) -> Result<(), ObserverValidationError> {
        validate_test(test)?;
        let id = test.id as usize - 1;
        let index = test.index as usize;
        if let Some(old_index) = self.id_to_index[id] {
            if old_index != test.index || self.index_to_id[index] != Some(test.id) {
                return Err(ObserverValidationError::Identity);
            }
        } else {
            if test.id != self.identities + 1 || self.index_to_id[index].is_some() {
                return Err(ObserverValidationError::Identity);
            }
            self.identities += 1;
            self.id_to_index[id] = Some(test.index);
            self.index_to_id[index] = Some(test.id);
        }
        Ok(())
    }
}

/// Bounded parent-owned accepted evidence. Neither this nor a staged transaction
/// is Clone. All retained text lives once in the moved actual wire records.
#[derive(Debug)]
pub struct ObserverLedger {
    expected: ObserverExpectation,
    cursor: u32,
    progress: Option<ObserverProgress>,
    execution_failed: bool,
    fault: Option<ObserverFault>,
    parent_failure: Option<ObserverParentFailure>,
    records: Vec<ObserverRecord>,
    reconciliations: Vec<ReconciledName>,
    identities: IdentityState,
    copied_units: usize,
    started: bool,
}

#[derive(Debug)]
pub struct ObservationView<'a> {
    pub expectation: ObserverExpectation,
    pub cursor: u32,
    pub records: &'a [ObserverRecord],
    pub progress: Option<ObserverProgress>,
    pub execution_failed: bool,
    pub fault: Option<ObserverFault>,
    pub parent_failure: Option<ObserverParentFailure>,
    pub reconciliations: &'a [ReconciledName],
    pub copied_units: usize,
    pub retained_capacity_bytes: usize,
}

#[derive(Debug)]
struct Plan {
    cursor: u32,
    progress: Option<ObserverProgress>,
    execution_failed: bool,
    fault: Option<ObserverFault>,
    identities: IdentityState,
    reconciliations: Vec<ReconciledName>,
}

/// Holds only this incoming delta plus fixed scalar scratch. Dropping it cancels
/// publication, not cumulative text-copy accounting. Its exclusive borrow makes
/// stale/cross-ledger commits impossible without any globally allocated token.
#[derive(Debug)]
pub struct StagedObservation<'a> {
    ledger: &'a mut ObserverLedger,
    incoming: ObserverReply,
    plan: Plan,
}

impl ObserverLedger {
    pub fn new(expected: ObserverExpectation) -> Result<Self, ObserverValidationError> {
        expected.validate()?;
        Ok(Self {
            expected,
            cursor: 0,
            progress: None,
            execution_failed: false,
            fault: None,
            parent_failure: None,
            records: Vec::new(),
            reconciliations: Vec::new(),
            identities: IdentityState::default(),
            copied_units: 0,
            started: false,
        })
    }

    pub fn observation(&self) -> ObservationView<'_> {
        ObservationView {
            expectation: self.expected,
            cursor: self.cursor,
            records: &self.records,
            progress: self.progress,
            execution_failed: self.execution_failed,
            fault: self.fault,
            parent_failure: self.parent_failure,
            reconciliations: &self.reconciliations,
            copied_units: self.copied_units,
            retained_capacity_bytes: self.retained_capacity_bytes(),
        }
    }

    pub fn fail_parent(&mut self, failure: ObserverParentFailure) {
        self.parent_failure.get_or_insert(failure);
    }

    fn retained_capacity_bytes(&self) -> usize {
        size_of::<Self>()
            + self.records.capacity() * size_of::<ObserverRecord>()
            + self.reconciliations.capacity() * size_of::<ReconciledName>()
            + self
                .records
                .iter()
                .map(ObserverRecord::heap_capacity_bytes)
                .sum::<usize>()
    }

    pub fn stage(
        &mut self,
        incoming: ObserverReply,
    ) -> Result<StagedObservation<'_>, ObserverValidationError> {
        match self.prepare(incoming) {
            Ok((incoming, plan)) => Ok(StagedObservation {
                ledger: self,
                incoming,
                plan,
            }),
            Err(error) => {
                self.fail_parent(ObserverParentFailure::Validation);
                Err(error)
            }
        }
    }

    fn prepare(
        &mut self,
        incoming: ObserverReply,
    ) -> Result<(ObserverReply, Plan), ObserverValidationError> {
        // Incoming strings have already actually been copied by the decoder (or
        // explicitly admitted producer); moving or abandoning them refunds none.
        let units = match &incoming {
            ObserverReply::WindowHarness { records, .. } => {
                records.iter().map(ObserverRecord::text_units).sum()
            }
            _ => 0,
        };
        self.copied_units = self
            .copied_units
            .checked_add(units)
            .ok_or(ObserverValidationError::CopyBound)?;
        if self.copied_units > MAX_COPIED_UNITS {
            return Err(ObserverValidationError::CopyBound);
        }
        if self.parent_failure.is_some() {
            return Err(ObserverValidationError::ParentFailed);
        }
        incoming.validate_expectation(self.expected)?;
        if self.progress == Some(ObserverProgress::Finished) {
            return Err(ObserverValidationError::Progress);
        }
        let mut plan = Plan {
            cursor: self.cursor,
            progress: self.progress,
            execution_failed: self.execution_failed,
            fault: self.fault,
            identities: self.identities.scratch(),
            reconciliations: Vec::new(),
        };
        match &incoming {
            ObserverReply::Disabled => {}
            ObserverReply::ObservationFailed {
                prefix_end,
                fault,
                execution_failed,
                ..
            } => {
                if *prefix_end != self.cursor {
                    return Err(ObserverValidationError::Cursor);
                }
                if self.fault.is_some_and(|old| old != *fault)
                    || (self.execution_failed && !execution_failed)
                {
                    return Err(ObserverValidationError::StickyFailure);
                }
                if *fault == ObserverFault::RejectedInit
                    && ((self.started && self.fault != Some(ObserverFault::RejectedInit))
                        || self.cursor != 0
                        || !execution_failed)
                {
                    return Err(ObserverValidationError::Frame);
                }
                plan.fault = Some(*fault);
                plan.execution_failed = *execution_failed;
            }
            ObserverReply::WindowHarness {
                first_ordinal,
                next_ordinal,
                records,
                progress,
                execution_failed,
                ..
            } => {
                if self.fault.is_some() || (self.execution_failed && !execution_failed) {
                    return Err(ObserverValidationError::StickyFailure);
                }
                if *first_ordinal != self.cursor {
                    return Err(ObserverValidationError::Cursor);
                }
                if self.progress.is_some_and(|old| *progress < old) {
                    return Err(ObserverValidationError::Progress);
                }
                for (position, record) in records.iter().enumerate() {
                    match record {
                        ObserverRecord::Result { test } => {
                            if plan.identities.completion {
                                return Err(ObserverValidationError::Completion);
                            }
                            plan.identities.bind(test)?;
                            if plan.identities.results[test.id as usize - 1] {
                                return Err(ObserverValidationError::DuplicateResult);
                            }
                            plan.identities.results[test.id as usize - 1] = true;
                        }
                        ObserverRecord::Completion { harness, inventory } => {
                            if plan.identities.completion || position + 1 != records.len() {
                                return Err(ObserverValidationError::Completion);
                            }
                            plan.reconciliations
                                .try_reserve_exact(inventory.len())
                                .map_err(|_| ObserverValidationError::Storage)?;
                            if plan.reconciliations.capacity() > MAX_TESTS {
                                return Err(ObserverValidationError::Storage);
                            }
                            let mut present = [false; MAX_TESTS];
                            for (index, final_test) in inventory.iter().enumerate() {
                                if final_test.index as usize != index {
                                    return Err(ObserverValidationError::Inventory);
                                }
                                plan.identities.bind(final_test)?;
                                let id = final_test.id as usize - 1;
                                if present[id] {
                                    return Err(ObserverValidationError::Inventory);
                                }
                                present[id] = true;
                                let prior = find_result(&self.records, final_test.id)
                                    .or_else(|| find_result(&records[..position], final_test.id));
                                let kind = if let Some(prior) = prior {
                                    if prior.index != final_test.index
                                        || prior.status != final_test.status
                                        || prior.message != final_test.message
                                        || prior.stack != final_test.stack
                                    {
                                        return Err(ObserverValidationError::Reconciliation);
                                    }
                                    reconcile_name(
                                        prior.name.as_units(),
                                        final_test.name.as_units(),
                                    )?
                                } else {
                                    if harness.status == HarnessStatus::Ok {
                                        return Err(ObserverValidationError::Inventory);
                                    }
                                    NameReconciliation::FinalOnly
                                };
                                plan.reconciliations.push(ReconciledName {
                                    id: final_test.id,
                                    index: final_test.index,
                                    kind,
                                });
                            }
                            if plan
                                .identities
                                .id_to_index
                                .iter()
                                .enumerate()
                                .any(|(id, index)| index.is_some() && !present[id])
                            {
                                return Err(ObserverValidationError::Inventory);
                            }
                            plan.identities.completion = true;
                        }
                    }
                }
                if (*progress == ObserverProgress::Waiting) == plan.identities.completion {
                    return Err(ObserverValidationError::Progress);
                }
                self.records
                    .try_reserve_exact(records.len())
                    .map_err(|_| ObserverValidationError::Storage)?;
                if self.records.capacity() > MAX_RECORDS {
                    return Err(ObserverValidationError::Storage);
                }
                self.reconciliations
                    .try_reserve_exact(plan.reconciliations.len())
                    .map_err(|_| ObserverValidationError::Storage)?;
                if self.reconciliations.capacity() > MAX_TESTS {
                    return Err(ObserverValidationError::Storage);
                }
                plan.cursor = *next_ordinal;
                plan.progress = Some(*progress);
                plan.execution_failed = *execution_failed;
            }
        }
        // The caps imply a much smaller representation, but check actual Vec
        // capacities rather than assuming len equals retained storage. This is
        // a narrowing parent metadata guard, not a worker allowance/refund.
        let temporary_bytes = size_of::<Plan>()
            + size_of::<StagedObservation<'_>>()
            + plan.reconciliations.capacity() * size_of::<ReconciledName>()
            + match &incoming {
                ObserverReply::WindowHarness { records, .. } => {
                    records.capacity() * size_of::<ObserverRecord>()
                        + records
                            .iter()
                            .map(ObserverRecord::heap_capacity_bytes)
                            .sum::<usize>()
                }
                _ => 0,
            };
        if self
            .retained_capacity_bytes()
            .checked_add(temporary_bytes)
            .is_none_or(|bytes| bytes > 4 * 1024 * 1024)
        {
            return Err(ObserverValidationError::Storage);
        }
        Ok((incoming, plan))
    }
}

fn find_result(records: &[ObserverRecord], id: u16) -> Option<&TestRecord> {
    records.iter().find_map(|record| match record {
        ObserverRecord::Result { test } if test.id == id => Some(test),
        _ => None,
    })
}

impl StagedObservation<'_> {
    /// All allocation/reservation and validation happened in stage. The caller
    /// must first complete its one combined active charge and other frame checks.
    pub fn commit(self) {
        let Self {
            ledger,
            incoming,
            plan,
        } = self;
        if let ObserverReply::WindowHarness { records, .. } = incoming {
            ledger.records.extend(records);
        }
        ledger.reconciliations.extend(plan.reconciliations);
        ledger.cursor = plan.cursor;
        ledger.progress = plan.progress;
        ledger.execution_failed = plan.execution_failed;
        ledger.fault = plan.fault;
        ledger.identities = plan.identities;
        ledger.started = true;
    }
}

// A seed-local budget covers the whole reply, including nested final rows.
// No TLS/global decoder state, generic Value tree or oversized temporary list.
mod decode {
    use super::*;
    use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
    use std::marker::PhantomData;

    #[derive(Default)]
    struct Budget {
        units: usize,
    }

    trait Decode: Sized {
        fn decode<'de, D: serde::Deserializer<'de>>(
            budget: &mut Budget,
            deserializer: D,
        ) -> Result<Self, D::Error>;
    }

    struct Seed<'a, T>(&'a mut Budget, PhantomData<T>);

    impl<'de, T: Decode> DeserializeSeed<'de> for Seed<'_, T> {
        type Value = T;

        fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<T, D::Error> {
            T::decode(self.0, d)
        }
    }

    macro_rules! scalar {
        ($($ty:ty),* $(,)?) => {$(
            impl Decode for $ty {
                fn decode<'de, D: serde::Deserializer<'de>>(
                    _: &mut Budget, d: D,
                ) -> Result<Self, D::Error> {
                    Self::deserialize(d)
                }
            }
        )*};
    }

    fn grow<T, E: de::Error>(values: &mut Vec<T>, maximum: usize) -> Result<(), E> {
        if values.len() == values.capacity() {
            let target = values.capacity().saturating_mul(2).max(8).min(maximum);
            values
                .try_reserve_exact(target - values.len())
                .map_err(E::custom)?;
            if values.capacity() > maximum {
                return Err(E::custom("observer sequence capacity exceeds bound"));
            }
        }
        Ok(())
    }

    struct List<T, const N: usize>(Vec<T>);

    impl<T: Decode, const N: usize> Decode for List<T, N> {
        fn decode<'de, D: serde::Deserializer<'de>>(
            budget: &mut Budget,
            d: D,
        ) -> Result<Self, D::Error> {
            struct ListVisitor<'a, T, const N: usize>(&'a mut Budget, PhantomData<T>);
            impl<'de, T: Decode, const N: usize> Visitor<'de> for ListVisitor<'_, T, N> {
                type Value = List<T, N>;
                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("a bounded observer sequence")
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                    let mut values = Vec::new();
                    loop {
                        if values.len() == N {
                            if a.next_element::<de::IgnoredAny>()?.is_some() {
                                return Err(de::Error::custom("observer sequence exceeds bound"));
                            }
                            break;
                        }
                        let Some(value) = a.next_element_seed(Seed::<T>(self.0, PhantomData))?
                        else {
                            break;
                        };
                        grow::<_, A::Error>(&mut values, N)?;
                        values.push(value);
                    }
                    Ok(List(values))
                }
            }
            d.deserialize_seq(ListVisitor::<T, N>(budget, PhantomData))
        }
    }

    impl Decode for Utf16 {
        fn decode<'de, D: serde::Deserializer<'de>>(
            budget: &mut Budget,
            d: D,
        ) -> Result<Self, D::Error> {
            struct UnitsVisitor<'a>(&'a mut Budget);
            impl<'de> Visitor<'de> for UnitsVisitor<'_> {
                type Value = Utf16;
                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("bounded unsigned UTF-16 units")
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Utf16, A::Error> {
                    let mut values = Vec::new();
                    while let Some(unit) = a.next_element::<u16>()? {
                        if values.len() == MAX_FIELD_UNITS || self.0.units == MAX_COPIED_UNITS {
                            return Err(de::Error::custom("observer UTF-16 copy bound exceeded"));
                        }
                        grow::<_, A::Error>(&mut values, MAX_FIELD_UNITS)?;
                        self.0.units += 1;
                        values.push(unit);
                    }
                    Ok(Utf16(values))
                }
            }
            d.deserialize_seq(UnitsVisitor(budget))
        }
    }

    macro_rules! required {
        ($name:ident) => {
            $name.ok_or_else(|| de::Error::missing_field(stringify!($name)))?
        };
    }
    macro_rules! absent {
        ($($name:ident),* $(,)?) => {$(
            if $name.is_some() {
                return Err(de::Error::custom(concat!("field forbidden for observer variant: ", stringify!($name))));
            }
        )*};
    }

    // All field names and enum tags are parsed directly into closed identifiers.
    // Order is immaterial; duplicated/unknown/missing fields are never defaults.
    macro_rules! object {
        ($ty:ty, {$($field:ident: $field_ty:ty),* $(,)?}, $finish:block) => {
            impl Decode for $ty {
                fn decode<'de, D: serde::Deserializer<'de>>(
                    budget: &mut Budget, d: D,
                ) -> Result<Self, D::Error> {
                    #[allow(non_camel_case_types)]
                    #[derive(Deserialize)]
                    #[serde(field_identifier)]
                    enum Field { $($field),* }
                    struct ObjectVisitor<'a>(&'a mut Budget);
                    impl<'de> Visitor<'de> for ObjectVisitor<'_> {
                        type Value = $ty;
                        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                            f.write_str("a strict observer object")
                        }
                        fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                            $(let mut $field: Option<$field_ty> = None;)*
                            while let Some(field) = a.next_key::<Field>()? {
                                match field {$(Field::$field => {
                                    if $field.is_some() {
                                        return Err(de::Error::duplicate_field(stringify!($field)));
                                    }
                                    $field = Some(a.next_value_seed(Seed::<$field_ty>(self.0, PhantomData))?);
                                }),*}
                            }
                            $finish
                        }
                    }
                    d.deserialize_map(ObjectVisitor(budget))
                }
            }
        };
    }

    enum TextKind {
        Null,
        Undefined,
        Text,
    }
    string_enum!(TextKind {
        Null,
        Undefined,
        Text
    });
    enum InitKind {
        Disabled,
        WindowHarness,
    }
    string_enum!(InitKind {
        Disabled,
        WindowHarness
    });
    enum RecordKind {
        Result,
        Completion,
    }
    string_enum!(RecordKind { Result, Completion });
    enum ReplyKind {
        Disabled,
        WindowHarness,
        ObservationFailed,
    }
    string_enum!(ReplyKind {
        Disabled,
        WindowHarness,
        ObservationFailed
    });
    scalar!(
        u16,
        u32,
        u64,
        bool,
        SubtestStatus,
        HarnessStatus,
        ObserverProgress,
        ObserverFault,
        ObserverContract,
        InitKind,
        TextKind,
        RecordKind,
        ReplyKind
    );

    object!(ObserverInit, {
        kind: InitKind, contract: ObserverContract, run_id: u64, bootstrap_node: u32,
    }, {
        let value = match required!(kind) {
            InitKind::Disabled => {
                absent!(contract, run_id, bootstrap_node);
                ObserverInit::Disabled
            }
            InitKind::WindowHarness => ObserverInit::WindowHarness {
                contract: required!(contract), run_id: required!(run_id),
                bootstrap_node: required!(bootstrap_node),
            },
        };
        value.validate().map_err(de::Error::custom)?;
        Ok(value)
    });

    object!(OptionalUtf16, {kind: TextKind, units: Utf16}, {
        Ok(match required!(kind) {
            TextKind::Null => { absent!(units); OptionalUtf16::Null }
            TextKind::Undefined => { absent!(units); OptionalUtf16::Undefined }
            TextKind::Text => OptionalUtf16::Text { units: required!(units) },
        })
    });
    object!(TestRecord, {
        id: u16, index: u16, name: Utf16, status: SubtestStatus,
        message: OptionalUtf16, stack: OptionalUtf16,
    }, {
        Ok(TestRecord { id: required!(id), index: required!(index), name: required!(name),
            status: required!(status), message: required!(message), stack: required!(stack) })
    });
    object!(HarnessRecord, {
        status: HarnessStatus, message: OptionalUtf16, stack: OptionalUtf16,
    }, {
        Ok(HarnessRecord { status: required!(status), message: required!(message), stack: required!(stack) })
    });
    object!(ObserverRecord, {
        kind: RecordKind, test: TestRecord, harness: HarnessRecord,
        inventory: List<TestRecord, MAX_TESTS>,
    }, {
        Ok(match required!(kind) {
            RecordKind::Result => { absent!(harness, inventory); ObserverRecord::Result { test: required!(test) } }
            RecordKind::Completion => { absent!(test); ObserverRecord::Completion {
                harness: required!(harness), inventory: required!(inventory).0,
            } }
        })
    });
    object!(ObserverReply, {
        kind: ReplyKind, run_id: u64, first_ordinal: u32, next_ordinal: u32,
        records: List<ObserverRecord, MAX_RECORDS>, progress: ObserverProgress,
        execution_failed: bool, prefix_end: u32, fault: ObserverFault,
    }, {
        Ok(match required!(kind) {
            ReplyKind::Disabled => {
                absent!(run_id, first_ordinal, next_ordinal, records, progress, execution_failed, prefix_end, fault);
                ObserverReply::Disabled
            }
            ReplyKind::WindowHarness => {
                absent!(prefix_end, fault);
                ObserverReply::WindowHarness {
                    run_id: required!(run_id), first_ordinal: required!(first_ordinal),
                    next_ordinal: required!(next_ordinal), records: required!(records).0,
                    progress: required!(progress), execution_failed: required!(execution_failed),
                }
            }
            ReplyKind::ObservationFailed => {
                absent!(first_ordinal, next_ordinal, records, progress);
                ObserverReply::ObservationFailed {
                    run_id: required!(run_id), prefix_end: required!(prefix_end),
                    fault: required!(fault), execution_failed: required!(execution_failed),
                }
            }
        })
    });

    macro_rules! deserialize {
        ($($ty:ty),* $(,)?) => {$(
            impl<'de> Deserialize<'de> for $ty {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    Self::decode(&mut Budget::default(), d)
                }
            }
        )*};
    }
    deserialize!(
        ObserverInit,
        Utf16,
        OptionalUtf16,
        TestRecord,
        HarnessRecord,
        ObserverRecord,
        ObserverReply
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected() -> ObserverExpectation {
        ObserverExpectation::WindowHarness { run_id: 9 }
    }

    fn text(units: &[u16]) -> Utf16 {
        Utf16::from_units(units.to_vec()).unwrap()
    }

    fn test(id: u16, index: u16, name: &[u16]) -> TestRecord {
        TestRecord {
            id,
            index,
            name: text(name),
            status: SubtestStatus::Pass,
            message: OptionalUtf16::Null,
            stack: OptionalUtf16::Null,
        }
    }

    fn result(id: u16, index: u16, name: &[u16]) -> ObserverRecord {
        ObserverRecord::Result {
            test: test(id, index, name),
        }
    }

    fn completion(status: HarnessStatus, inventory: Vec<TestRecord>) -> ObserverRecord {
        ObserverRecord::Completion {
            harness: HarnessRecord {
                status,
                message: OptionalUtf16::Null,
                stack: OptionalUtf16::Null,
            },
            inventory,
        }
    }

    fn delta(
        first: u32,
        records: Vec<ObserverRecord>,
        progress: ObserverProgress,
    ) -> ObserverReply {
        ObserverReply::WindowHarness {
            run_id: 9,
            first_ordinal: first,
            next_ordinal: first + records.len() as u32,
            records,
            progress,
            execution_failed: false,
        }
    }

    fn failure(prefix_end: u32, fault: ObserverFault, execution_failed: bool) -> ObserverReply {
        ObserverReply::ObservationFailed {
            run_id: 9,
            prefix_end,
            fault,
            execution_failed,
        }
    }

    #[test]
    fn init_disabled_and_enabled_are_exact_and_nonzero() {
        assert_eq!(ObserverInit::default(), ObserverInit::Disabled);
        assert_eq!(
            ObserverInit::Disabled.expectation(),
            ObserverExpectation::Disabled
        );
        let enabled = ObserverInit::WindowHarness {
            contract: ObserverContract::PinnedWindowHarnessV1,
            run_id: 9,
            bootstrap_node: 4,
        };
        assert_eq!(enabled.expectation(), expected());
        assert!(enabled.validate().is_ok());
        for malformed in [
            r#"null"#,
            r#"{}"#,
            r#"{"kind":"Disabled","run_id":9}"#,
            r#"{"kind":"WindowHarness","run_id":9,"bootstrap_node":4}"#,
            r#"{"kind":"WindowHarness","contract":"other","run_id":9,"bootstrap_node":4}"#,
        ] {
            assert!(
                serde_json::from_str::<ObserverInit>(malformed).is_err(),
                "{malformed}"
            );
        }
        assert!(ObserverLedger::new(ObserverExpectation::WindowHarness { run_id: 0 }).is_err());
        assert_eq!(
            ObserverReply::rejected_init(expected()),
            failure(0, ObserverFault::RejectedInit, true)
        );
        assert_eq!(
            ObserverReply::rejected_init(ObserverExpectation::Disabled),
            ObserverReply::Disabled
        );
    }

    #[test]
    fn closed_init_decoder_rejects_known_unit_variant_extras_and_invalid_enabled_metadata() {
        // Derived internally tagged unit variants accepted known extra fields
        // despite deny_unknown_fields. Preserve the original failing control
        // above and cover its entire metadata/null surface with this decoder.
        for malformed in [
            r#"{"kind":"Disabled","run_id":1}"#,
            r#"{"kind":"Disabled","run_id":null}"#,
            r#"{"kind":"Disabled","contract":"PinnedWindowHarnessV1"}"#,
            r#"{"kind":"Disabled","contract":null}"#,
            r#"{"kind":"Disabled","bootstrap_node":1}"#,
            r#"{"kind":"Disabled","bootstrap_node":null}"#,
            r#"{"kind":"Disabled","unknown":null}"#,
            r#"{"kind":"Disabled","kind":"Disabled"}"#,
            r#"{"run_id":1}"#,
            r#"{"kind":null}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","bootstrap_node":1}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":null,"bootstrap_node":1}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":0,"bootstrap_node":1}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":true,"bootstrap_node":1}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":-1,"bootstrap_node":1}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":1.0,"bootstrap_node":1}"#,
            r#"{"kind":"WindowHarness","contract":null,"run_id":1,"bootstrap_node":1}"#,
            r#"{"kind":"WindowHarness","contract":"unknown","run_id":1,"bootstrap_node":1}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":1}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":1,"bootstrap_node":null}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":1,"run_id":1,"bootstrap_node":1}"#,
            r#"{"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":1,"bootstrap_node":1,"extra":0}"#,
        ] {
            assert!(
                serde_json::from_str::<ObserverInit>(malformed).is_err(),
                "{malformed}"
            );
        }
        assert_eq!(
            serde_json::from_str::<ObserverInit>(r#"{"kind":"Disabled"}"#).unwrap(),
            ObserverInit::Disabled
        );
        let enabled = ObserverInit::WindowHarness {
            contract: ObserverContract::PinnedWindowHarnessV1,
            run_id: u64::MAX,
            bootstrap_node: u32::MAX,
        };
        let reordered = r#"{"bootstrap_node":4294967295,"run_id":18446744073709551615,"contract":"PinnedWindowHarnessV1","kind":"WindowHarness"}"#;
        assert_eq!(
            serde_json::from_str::<ObserverInit>(reordered).unwrap(),
            enabled
        );
        assert_eq!(
            serde_json::from_slice::<ObserverInit>(&serde_json::to_vec(&enabled).unwrap()).unwrap(),
            enabled
        );
    }

    #[test]
    fn every_scalar_and_kind_tag_is_an_actual_string_not_an_external_enum_object() {
        macro_rules! scalar {
            ($ty:ty, $tag:literal) => {
                assert!(serde_json::from_str::<$ty>(concat!("\"", $tag, "\"")).is_ok());
                for bad in [concat!("{\"", $tag, "\":null}"), "null", "true", "0", "[]"] {
                    assert!(serde_json::from_str::<$ty>(bad).is_err(), "{bad}");
                }
            };
        }
        scalar!(ObserverContract, "PinnedWindowHarnessV1");
        scalar!(SubtestStatus, "Pass");
        scalar!(HarnessStatus, "Ok");
        scalar!(ObserverProgress, "Waiting");
        scalar!(ObserverFault, "MalformedCapture");
        for bad in [
            r#"{"kind":{"Disabled":null}}"#,
            r#"{"kind":true}"#,
            r#"{"kind":"WindowHarness","contract":{"PinnedWindowHarnessV1":null},"run_id":1,"bootstrap_node":1}"#,
        ] {
            assert!(serde_json::from_str::<ObserverInit>(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<OptionalUtf16>(r#"{"kind":{"Null":null}}"#).is_err());
        assert!(serde_json::from_str::<ObserverReply>(r#"{"kind":{"Disabled":null}}"#).is_err());
        let record = serde_json::to_string(&result(1, 0, &[65])).unwrap();
        assert!(
            serde_json::from_str::<ObserverRecord>(&record.replacen(
                r#""kind":"Result""#,
                r#""kind":{"Result":null}"#,
                1
            ))
            .is_err()
        );
        let row = serde_json::to_string(&test(1, 0, &[65])).unwrap();
        assert!(
            serde_json::from_str::<TestRecord>(&row.replacen(
                r#""status":"Pass""#,
                r#""status":{"Pass":null}"#,
                1
            ))
            .is_err()
        );
    }

    #[test]
    fn wire_roundtrip_preserves_undefined_null_empty_and_surrogates() {
        let mut first = test(1, 0, &[0xd800, 0, 0xdc00]);
        first.message = OptionalUtf16::Undefined;
        first.stack = OptionalUtf16::Text { units: text(&[]) };
        let reply = delta(
            0,
            vec![ObserverRecord::Result { test: first }],
            ObserverProgress::Waiting,
        );
        let wire = serde_json::to_vec(&reply).unwrap();
        let decoded: ObserverReply = serde_json::from_slice(&wire).unwrap();
        assert_eq!(decoded, reply);
        assert_ne!(OptionalUtf16::Undefined, OptionalUtf16::Null);
        assert_ne!(
            OptionalUtf16::Text { units: text(&[]) },
            OptionalUtf16::Null
        );
    }

    #[test]
    fn strict_reply_and_nested_shapes_reject_missing_null_extra_duplicate() {
        for malformed in [
            r#"null"#,
            r#"{}"#,
            r#"{"kind":"Disabled","extra":0}"#,
            r#"{"kind":"Disabled","records":[]}"#,
            r#"{"kind":"Disabled","kind":"Disabled"}"#,
            r#"{"kind":"WindowHarness","run_id":9,"first_ordinal":0,"next_ordinal":0,"records":[],"progress":"Waiting"}"#,
            r#"{"kind":"WindowHarness","run_id":9,"first_ordinal":0,"next_ordinal":0,"records":null,"progress":"Waiting","execution_failed":false}"#,
            r#"{"kind":"ObservationFailed","run_id":9,"prefix_end":0,"fault":"unknown","execution_failed":false}"#,
        ] {
            assert!(
                serde_json::from_str::<ObserverReply>(malformed).is_err(),
                "{malformed}"
            );
        }
        for malformed in [
            r#"null"#,
            r#"{"kind":"Null","units":[]}"#,
            r#"{"kind":"Undefined","extra":0}"#,
            r#"{"kind":"Text"}"#,
            r#"{"kind":"Text","units":null}"#,
        ] {
            assert!(
                serde_json::from_str::<OptionalUtf16>(malformed).is_err(),
                "{malformed}"
            );
        }
        let source = serde_json::to_value(test(1, 0, &[65])).unwrap();
        for field in ["id", "index", "name", "status", "message", "stack"] {
            let mut missing = source.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<TestRecord>(missing).is_err(),
                "{field}"
            );
            let mut null = source.clone();
            null[field] = serde_json::Value::Null;
            assert!(
                serde_json::from_value::<TestRecord>(null).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn units_reject_nonintegers_and_exact_field_bound_plus_one() {
        for malformed in [
            "[-1]", "[65536]", "[1.0]", "[true]", "[null]", "[\"1\"]", "{}",
        ] {
            assert!(
                serde_json::from_str::<Utf16>(malformed).is_err(),
                "{malformed}"
            );
        }
        let exact = vec![65u16; MAX_FIELD_UNITS];
        let encoded = serde_json::to_vec(&exact).unwrap();
        assert_eq!(
            serde_json::from_slice::<Utf16>(&encoded).unwrap().len(),
            MAX_FIELD_UNITS
        );
        assert!(
            serde_json::from_slice::<Utf16>(
                &serde_json::to_vec(&vec![65u16; MAX_FIELD_UNITS + 1]).unwrap()
            )
            .is_err()
        );
        let mut oversized_capacity = Vec::with_capacity(MAX_FIELD_UNITS + 1);
        oversized_capacity.push(65);
        assert_eq!(
            Utf16::from_units(oversized_capacity).unwrap_err(),
            ObserverValidationError::FieldBound
        );
    }

    #[test]
    fn one_decode_budget_covers_all_nested_text_fields_and_final_rows() {
        let units = vec![65u16; MAX_FIELD_UNITS];
        let mut records = Vec::new();
        for id in 1..=32 {
            records.push(result(id, id - 1, &units));
        }
        let exact = delta(0, records, ObserverProgress::Waiting);
        assert!(
            serde_json::from_slice::<ObserverReply>(&serde_json::to_vec(&exact).unwrap()).is_ok()
        );
        let mut value = serde_json::to_value(exact).unwrap();
        value["records"][31]["test"]["message"] = serde_json::json!({"kind":"Text","units":[66]});
        assert!(
            serde_json::from_slice::<ObserverReply>(&serde_json::to_vec(&value).unwrap()).is_err()
        );
        // A final inventory is not a fresh decode budget.
        let mut rows = Vec::new();
        for id in 1..=11 {
            let mut row = test(id, id - 1, &units);
            row.message = OptionalUtf16::Text {
                units: text(&units),
            };
            row.stack = OptionalUtf16::Text {
                units: text(&units),
            };
            rows.push(row);
        }
        let final_reply = delta(
            0,
            vec![completion(HarnessStatus::Error, rows)],
            ObserverProgress::Finished,
        );
        assert!(
            serde_json::from_slice::<ObserverReply>(&serde_json::to_vec(&final_reply).unwrap())
                .is_err()
        );
    }

    #[test]
    fn sequence_limits_are_checked_in_decoder_not_only_ledger() {
        let result_value = serde_json::to_value(result(1, 0, &[])).unwrap();
        let wire = serde_json::json!({"kind":"WindowHarness","run_id":9,"first_ordinal":0,
            "next_ordinal":258,"records":vec![result_value; MAX_RECORDS + 1],
            "progress":"Waiting","execution_failed":false});
        assert!(
            serde_json::from_slice::<ObserverReply>(&serde_json::to_vec(&wire).unwrap()).is_err()
        );
        let row = serde_json::to_value(test(1, 0, &[])).unwrap();
        let wire = serde_json::json!({"kind":"Completion", "harness":{"status":"Error","message":{"kind":"Null"},"stack":{"kind":"Null"}},
            "inventory":vec![row; MAX_TESTS + 1]});
        assert!(
            serde_json::from_slice::<ObserverRecord>(&serde_json::to_vec(&wire).unwrap()).is_err()
        );
    }

    #[test]
    fn stage_is_unpublished_drop_does_not_refund_text_and_commit_is_visible() {
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        let pending = ledger
            .stage(delta(
                0,
                vec![result(1, 0, &[65])],
                ObserverProgress::Waiting,
            ))
            .unwrap();
        drop(pending);
        assert_eq!(ledger.observation().cursor, 0);
        assert!(ledger.observation().records.is_empty());
        assert_eq!(ledger.observation().copied_units, 1);
        ledger
            .stage(delta(
                0,
                vec![result(1, 0, &[65])],
                ObserverProgress::Waiting,
            ))
            .unwrap()
            .commit();
        assert_eq!(ledger.observation().cursor, 1);
        assert_eq!(ledger.observation().copied_units, 2);
        assert_eq!(ledger.observation().records.len(), 1);
    }

    #[test]
    fn out_of_order_results_bind_identity_and_dense_creation_inventory() {
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        ledger
            .stage(delta(
                0,
                vec![result(1, 1, &[66]), result(2, 0, &[65])],
                ObserverProgress::Waiting,
            ))
            .unwrap()
            .commit();
        ledger
            .stage(delta(
                2,
                vec![completion(
                    HarnessStatus::Ok,
                    vec![test(2, 0, &[65]), test(1, 1, &[66])],
                )],
                ObserverProgress::Settling,
            ))
            .unwrap()
            .commit();
        assert_eq!(ledger.observation().cursor, 3);
        assert_eq!(ledger.observation().reconciliations[0].id, 2);
        ledger
            .stage(delta(3, vec![], ObserverProgress::Finished))
            .unwrap()
            .commit();
        assert_eq!(
            ledger.observation().progress,
            Some(ObserverProgress::Finished)
        );
        assert!(
            ledger
                .stage(delta(3, vec![], ObserverProgress::Finished))
                .is_err()
        );
    }

    #[test]
    fn pinned_surrogate_vectors_compare_without_allocating_or_lossy_conversion() {
        for (raw, final_units, kind) in [
            (vec![], vec![], NameReconciliation::Unchanged),
            (vec![65], vec![65], NameReconciliation::Unchanged),
            (
                vec![0xd83d, 0xde00],
                vec![0xd83d, 0xde00],
                NameReconciliation::Unchanged,
            ),
            (
                vec![0xd800],
                "U+d800".encode_utf16().collect(),
                NameReconciliation::PinnedSanitized,
            ),
            (
                vec![0xdc00],
                "U+dc00".encode_utf16().collect(),
                NameReconciliation::PinnedSanitized,
            ),
            (
                vec![0xd800, 0xd800, 0xdc00],
                vec![85, 43, 100, 56, 48, 48, 0xd800, 0xdc00],
                NameReconciliation::PinnedSanitized,
            ),
            (
                vec![0xdfff, 0xdbff],
                "U+dfffU+dbff".encode_utf16().collect(),
                NameReconciliation::PinnedSanitized,
            ),
            (
                "U+d800".encode_utf16().collect(),
                "U+d800".encode_utf16().collect(),
                NameReconciliation::Unchanged,
            ),
        ] {
            assert_eq!(reconcile_name(&raw, &final_units).unwrap(), kind);
        }
        for bad in ["U+D800", "u+d800", "U+d800x", "", "replacement"] {
            assert!(reconcile_name(&[0xd800], &bad.encode_utf16().collect::<Vec<_>>()).is_err());
        }
    }

    #[test]
    fn sanitizer_name_collision_is_legal_and_both_actual_observations_remain() {
        let literal: Vec<u16> = "U+d800".encode_utf16().collect();
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        ledger
            .stage(delta(
                0,
                vec![
                    result(1, 0, &[0xd800]),
                    result(2, 1, &literal),
                    completion(
                        HarnessStatus::Ok,
                        vec![test(1, 0, &literal), test(2, 1, &literal)],
                    ),
                ],
                ObserverProgress::Finished,
            ))
            .unwrap()
            .commit();
        assert_eq!(ledger.observation().records.len(), 3);
        assert_eq!(
            ledger.observation().reconciliations[0].kind,
            NameReconciliation::PinnedSanitized
        );
        assert_eq!(
            ledger.observation().reconciliations[1].kind,
            NameReconciliation::Unchanged
        );
    }

    #[test]
    fn non_ok_final_only_rows_preserve_every_real_subtest_status() {
        for status in [
            SubtestStatus::Pass,
            SubtestStatus::Fail,
            SubtestStatus::Timeout,
            SubtestStatus::NotRun,
            SubtestStatus::PreconditionFailed,
        ] {
            let mut row = test(1, 0, &[65]);
            row.status = status;
            let mut ledger = ObserverLedger::new(expected()).unwrap();
            ledger
                .stage(delta(
                    0,
                    vec![completion(HarnessStatus::Timeout, vec![row])],
                    ObserverProgress::Finished,
                ))
                .unwrap()
                .commit();
            assert_eq!(
                ledger.observation().reconciliations[0].kind,
                NameReconciliation::FinalOnly
            );
        }
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        assert_eq!(
            ledger
                .stage(delta(
                    0,
                    vec![completion(HarnessStatus::Ok, vec![test(1, 0, &[65])])],
                    ObserverProgress::Finished
                ))
                .unwrap_err(),
            ObserverValidationError::Inventory
        );
    }

    #[test]
    fn zero_row_ok_is_representable_not_a_positive_verdict() {
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        ledger
            .stage(delta(
                0,
                vec![completion(HarnessStatus::Ok, vec![])],
                ObserverProgress::Finished,
            ))
            .unwrap()
            .commit();
        assert_eq!(ledger.observation().cursor, 1);
        assert!(ledger.observation().reconciliations.is_empty());
    }

    #[test]
    fn result_identity_cursor_and_inventory_rejections_are_sticky_and_unpublished() {
        for bad in [
            delta(0, vec![result(0, 0, &[65])], ObserverProgress::Waiting),
            delta(0, vec![result(2, 0, &[65])], ObserverProgress::Waiting),
            delta(0, vec![result(1, 256, &[65])], ObserverProgress::Waiting),
            delta(1, vec![result(1, 0, &[65])], ObserverProgress::Waiting),
            delta(
                0,
                vec![result(1, 0, &[65]), result(1, 0, &[65])],
                ObserverProgress::Waiting,
            ),
            delta(
                0,
                vec![result(1, 0, &[65]), result(2, 0, &[66])],
                ObserverProgress::Waiting,
            ),
            delta(
                0,
                vec![
                    result(1, 0, &[65]),
                    completion(HarnessStatus::Error, vec![]),
                ],
                ObserverProgress::Finished,
            ),
            delta(
                0,
                vec![
                    result(1, 0, &[65]),
                    completion(HarnessStatus::Ok, vec![test(1, 0, &[66])]),
                ],
                ObserverProgress::Finished,
            ),
        ] {
            let mut ledger = ObserverLedger::new(expected()).unwrap();
            assert!(ledger.stage(bad).is_err());
            assert_eq!(ledger.observation().cursor, 0);
            assert!(ledger.observation().records.is_empty());
            assert_eq!(
                ledger.observation().parent_failure,
                Some(ObserverParentFailure::Validation)
            );
            assert!(
                ledger
                    .stage(delta(0, vec![], ObserverProgress::Waiting))
                    .is_err()
            );
        }
    }

    #[test]
    fn status_optional_and_creation_identity_cannot_change_at_completion() {
        for change in 0..4 {
            let mut ledger = ObserverLedger::new(expected()).unwrap();
            ledger
                .stage(delta(
                    0,
                    vec![result(1, 0, &[65])],
                    ObserverProgress::Waiting,
                ))
                .unwrap()
                .commit();
            let mut final_row = test(1, 0, &[65]);
            match change {
                0 => final_row.status = SubtestStatus::Fail,
                1 => final_row.message = OptionalUtf16::Undefined,
                2 => final_row.stack = OptionalUtf16::Text { units: text(&[]) },
                _ => final_row.index = 1,
            }
            assert!(
                ledger
                    .stage(delta(
                        1,
                        vec![completion(HarnessStatus::Ok, vec![final_row])],
                        ObserverProgress::Finished
                    ))
                    .is_err()
            );
            assert_eq!(ledger.observation().cursor, 1);
            assert_eq!(ledger.observation().records.len(), 1);
        }
    }

    #[test]
    fn completion_is_unique_last_and_required_by_progress() {
        for bad in [
            delta(0, vec![], ObserverProgress::Settling),
            delta(0, vec![], ObserverProgress::Finished),
            delta(
                0,
                vec![completion(HarnessStatus::Ok, vec![])],
                ObserverProgress::Waiting,
            ),
            delta(
                0,
                vec![completion(HarnessStatus::Ok, vec![]), result(1, 0, &[65])],
                ObserverProgress::Finished,
            ),
            delta(
                0,
                vec![
                    completion(HarnessStatus::Ok, vec![]),
                    completion(HarnessStatus::Ok, vec![]),
                ],
                ObserverProgress::Finished,
            ),
        ] {
            let mut ledger = ObserverLedger::new(expected()).unwrap();
            assert!(ledger.stage(bad).is_err());
        }
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        ledger
            .stage(delta(
                0,
                vec![completion(HarnessStatus::Ok, vec![])],
                ObserverProgress::Settling,
            ))
            .unwrap()
            .commit();
        assert!(
            ledger
                .stage(delta(
                    1,
                    vec![result(1, 0, &[65])],
                    ObserverProgress::Settling
                ))
                .is_err()
        );
    }

    #[test]
    fn fault_preserves_exact_historical_prefix_no_payload_and_execution_failure() {
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        ledger
            .stage(delta(
                0,
                vec![result(1, 0, &[65])],
                ObserverProgress::Waiting,
            ))
            .unwrap()
            .commit();
        ledger
            .stage(failure(1, ObserverFault::CaptureOverflow, false))
            .unwrap()
            .commit();
        ledger
            .stage(failure(1, ObserverFault::CaptureOverflow, true))
            .unwrap()
            .commit();
        assert_eq!(ledger.observation().cursor, 1);
        assert_eq!(ledger.observation().records.len(), 1);
        assert!(ledger.observation().execution_failed);
        assert_eq!(
            ledger.observation().fault,
            Some(ObserverFault::CaptureOverflow)
        );
        assert!(
            ledger
                .stage(delta(1, vec![], ObserverProgress::Waiting))
                .is_err()
        );
        for bad in [
            failure(1, ObserverFault::PrivacyLost, false),
            failure(258, ObserverFault::CaptureOverflow, false),
            failure(0, ObserverFault::RejectedInit, false),
        ] {
            let mut ledger = ObserverLedger::new(expected()).unwrap();
            assert!(ledger.stage(bad).is_err());
        }
    }

    #[test]
    fn execution_failure_is_sticky_even_with_clean_delta() {
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        let mut first = delta(0, vec![], ObserverProgress::Waiting);
        if let ObserverReply::WindowHarness {
            execution_failed, ..
        } = &mut first
        {
            *execution_failed = true;
        }
        ledger.stage(first).unwrap().commit();
        assert!(
            ledger
                .stage(delta(0, vec![], ObserverProgress::Waiting))
                .is_err()
        );
        assert!(ledger.observation().execution_failed);
    }

    #[test]
    fn mode_run_and_scalar_bounds_cannot_be_bypassed_by_direct_construction() {
        let mut disabled = ObserverLedger::new(ObserverExpectation::Disabled).unwrap();
        disabled.stage(ObserverReply::Disabled).unwrap().commit();
        assert!(
            disabled
                .stage(delta(0, vec![], ObserverProgress::Waiting))
                .is_err()
        );
        let mut enabled = ObserverLedger::new(expected()).unwrap();
        assert!(enabled.stage(ObserverReply::Disabled).is_err());
        let mut wrong_run = delta(0, vec![], ObserverProgress::Waiting);
        if let ObserverReply::WindowHarness { run_id, .. } = &mut wrong_run {
            *run_id = 10;
        }
        assert_eq!(
            wrong_run.validate_expectation(expected()).unwrap_err(),
            ObserverValidationError::Run
        );
        let mut wrong_cursor = delta(0, vec![], ObserverProgress::Waiting);
        if let ObserverReply::WindowHarness { next_ordinal, .. } = &mut wrong_cursor {
            *next_ordinal = 1;
        }
        assert_eq!(
            wrong_cursor.validate_expectation(expected()).unwrap_err(),
            ObserverValidationError::Cursor
        );
    }

    #[test]
    fn exact_cumulative_copy_bound_and_plus_one_across_frames_no_refund() {
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        let units = vec![65u16; MAX_FIELD_UNITS];
        let mut records = Vec::with_capacity(32);
        for id in 1..=32 {
            records.push(result(id, id - 1, &units));
        }
        ledger
            .stage(delta(0, records, ObserverProgress::Waiting))
            .unwrap()
            .commit();
        assert_eq!(ledger.observation().copied_units, MAX_COPIED_UNITS);
        ledger
            .stage(delta(32, vec![], ObserverProgress::Waiting))
            .unwrap()
            .commit();
        assert_eq!(
            ledger
                .stage(delta(
                    32,
                    vec![result(33, 32, &[65])],
                    ObserverProgress::Waiting
                ))
                .unwrap_err(),
            ObserverValidationError::CopyBound
        );
        assert_eq!(ledger.observation().cursor, 32);
        assert_eq!(ledger.observation().copied_units, MAX_COPIED_UNITS + 1);
    }

    #[test]
    fn exact_256_results_plus_completion_fit_without_a_free_extra_callback() {
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        let mut records = Vec::with_capacity(MAX_RECORDS);
        let mut inventory = Vec::with_capacity(MAX_TESTS);
        for id in 1..=256 {
            records.push(result(id, id - 1, &[]));
            inventory.push(test(id, id - 1, &[]));
        }
        records.push(completion(HarnessStatus::Ok, inventory));
        let wire = serde_json::to_vec(&delta(0, records, ObserverProgress::Finished)).unwrap();
        let decoded: ObserverReply = serde_json::from_slice(&wire).unwrap();
        ledger.stage(decoded).unwrap().commit();
        assert_eq!(ledger.observation().cursor, MAX_RECORDS as u32);
        assert_eq!(ledger.observation().reconciliations.len(), MAX_TESTS);
        assert!(ledger.observation().retained_capacity_bytes < 4 * 1024 * 1024);
    }

    #[test]
    fn parent_failure_is_first_sticky_and_never_manufactures_harness_status() {
        let mut ledger = ObserverLedger::new(expected()).unwrap();
        ledger
            .stage(delta(
                0,
                vec![result(1, 0, &[65])],
                ObserverProgress::Waiting,
            ))
            .unwrap()
            .commit();
        ledger.fail_parent(ObserverParentFailure::ActiveCharge);
        ledger.fail_parent(ObserverParentFailure::Cleanup);
        assert_eq!(
            ledger.observation().parent_failure,
            Some(ObserverParentFailure::ActiveCharge)
        );
        assert_eq!(ledger.observation().records.len(), 1);
        assert_eq!(
            ledger.observation().progress,
            Some(ObserverProgress::Waiting)
        );
        assert!(
            ledger
                .stage(delta(1, vec![], ObserverProgress::Waiting))
                .is_err()
        );
    }
}
