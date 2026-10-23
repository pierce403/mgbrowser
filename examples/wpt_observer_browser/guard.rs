//! Test-host inventory/navigation policy, not arbitrary-server confinement.
//! Chassis still performs the actual projection, ledger stage and publication.
use crate::output_config;
use mg_chassis::scripts::{ScriptRuntime, ScriptSession, SessionUpdate, SourceWireMode};
use mg_sparkle::{
    document::{self, Node},
    js_browser::{ClassicErrorVisibility, Request},
    page_session::{
        DefaultAction, LifecycleProgress, RealmState, SessionInput, SessionReply, TaskPump,
        observer::{ObserverExpectation, ObserverInit, ObserverProgress, ObserverReply},
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_TAGS: usize = 50_000;
const MAX_TAG_BYTES: usize = 8 * 1024 * 1024;
const MAX_TRANSACTIONS: usize = 64;
pub const HARNESS_SHA: &str = "d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e";
type Result<T> = std::result::Result<T, String>;

pub struct Inventory {
    pub url: String,
    pub html: String,
    pub harness: String,
    pub harness_url: String,
    pub output: Option<output_config::SourceInventory>,
    pub observer: ObserverInit,
    pub cutoff: Instant,
}

impl Inventory {
    pub fn verify(&self, request: &Request) -> Result<()> {
        if request.url != self.url || request.html.as_bytes() != self.html.as_bytes() {
            return Err("Actual document differs from frozen source inventory".into());
        }
        if let Some(output) = &self.output {
            if !matches!(self.observer, ObserverInit::WindowHarness { bootstrap_node, .. }
                if bootstrap_node as usize == output.plan.harness_node)
            {
                return Err("Configured bootstrap differs from actual output plan".into());
            }
            return output.verify(
                request,
                &self.html,
                &self.url,
                &self.harness,
                &self.harness_url,
            );
        }
        let sources = request
            .external_scripts
            .as_ref()
            .ok_or("Actual external source inventory missing")?;
        if sources.len() != 1 {
            return Err("Actual external source inventory count differs".into());
        }
        let source = &sources[0];
        if source.node != 1
            || source.source.len() != 198_291
            || source.source.as_bytes() != self.harness.as_bytes()
            || Sha256::digest(source.source.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                != HARNESS_SHA
        {
            return Err("Actual bootstrap differs from pinned complete source".into());
        }
        let provenance = source
            .provenance
            .as_ref()
            .ok_or("Actual bootstrap provenance missing")?;
        if provenance.error_visibility != ClassicErrorVisibility::SameOrigin
            || provenance.response_url != self.harness_url
        {
            return Err("Actual bootstrap response provenance differs".into());
        }
        let page = url::Url::parse(&self.url).map_err(|_| "Invalid document origin")?;
        let response =
            url::Url::parse(&provenance.response_url).map_err(|_| "Invalid source origin")?;
        if page.origin() != response.origin()
            || !response.username().is_empty()
            || response.password().is_some()
            || response.query().is_some()
            || response.fragment().is_some()
        {
            return Err("Actual source is not the exact owned origin".into());
        }
        let document = document::parse_with_scripting(&request.html, &request.url, true);
        let node = document.nodes.get(1).ok_or("Bootstrap node missing")?;
        if node.tag != "script"
            || node.attr("src") != Some("/resources/testharness.js")
            || node.attr("defer").is_some()
            || mg_sparkle::js_browser::unsupported_external_attribute(node).is_some()
            || !mg_sparkle::js_browser::classic_script_type(node.attr("type").unwrap_or(""))
            || document
                .nodes
                .iter()
                .filter(|node| node.tag == "script")
                .count()
                != 2
        {
            return Err("Actual bootstrap node/order/ordinary-source binding differs".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ChargedTrace {
    pub revision: u64,
    pub state: RealmState,
    pub lifecycle: Option<LifecycleProgress>,
    pub progress: Option<ObserverProgress>,
    pub first_ordinal: Option<u32>,
    pub next_ordinal: Option<u32>,
    pub diagnostics: usize,
    pub scripts_executed: usize,
    pub result_callbacks: usize,
    pub completion_callbacks: usize,
    pub source_admissions: Option<usize>,
    pub jobs_admitted: Option<usize>,
    pub jobs_executed: Option<usize>,
    pub pending_jobs: Option<usize>,
    pub wrapper_validation_ns: u64,
    pub manager_charge_ns: Option<u64>,
    pub manager_charge_succeeded: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub failure: Option<&'static str>,
    pub starts: usize,
    pub diagnostics: usize,
    pub diagnostics_match: bool,
    /// Raw reply metadata and charge acknowledgements, NOT accepted publication.
    pub charged_trace: Vec<ChargedTrace>,
}

pub struct HostState {
    state: Mutex<Snapshot>,
    expected_diagnostics: Vec<String>,
}
pub struct Summary {
    pub failure: Option<&'static str>,
    pub starts: usize,
    pub diagnostics: usize,
    pub diagnostics_match: bool,
    pub replies: usize,
    pub last: Option<ChargedTrace>,
}
impl HostState {
    pub fn new(expected_diagnostics: Vec<String>) -> Self {
        Self {
            state: Mutex::new(Snapshot {
                diagnostics_match: true,
                ..Snapshot::default()
            }),
            expected_diagnostics,
        }
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "Host evidence unavailable".into())
    }
    pub fn summary(&self) -> Result<Summary> {
        let state = self.state.lock().map_err(|_| "Host evidence unavailable")?;
        Ok(Summary {
            failure: state.failure,
            starts: state.starts,
            diagnostics: state.diagnostics,
            diagnostics_match: state.diagnostics_match,
            replies: state.charged_trace.len(),
            last: state.charged_trace.last().copied(),
        })
    }
    pub fn fail(&self, reason: &'static str) {
        if let Ok(mut state) = self.state.lock() {
            state.failure.get_or_insert(reason);
        }
    }
    pub fn check(&self) -> Result<()> {
        let state = self.state.lock().map_err(|_| "Host evidence unavailable")?;
        state.failure.map_or(Ok(()), |reason| Err(reason.into()))
    }
    fn note_reply(&self, reply: &SessionReply) -> Result<usize> {
        let mut state = self.state.lock().map_err(|_| "Host evidence unavailable")?;
        if state.charged_trace.len() >= MAX_TRANSACTIONS {
            return Err("Host reply trace bound exceeded".into());
        }
        for diagnostic in &reply.errors {
            let matches = self.expected_diagnostics.get(state.diagnostics) == Some(diagnostic);
            state.diagnostics_match &= matches;
            state.diagnostics = state
                .diagnostics
                .checked_add(1)
                .ok_or("Diagnostic count exhausted")?;
        }
        let (progress, first, next) = match &reply.observer {
            Some(ObserverReply::WindowHarness {
                progress,
                first_ordinal,
                next_ordinal,
                ..
            }) => (Some(*progress), Some(*first_ordinal), Some(*next_ordinal)),
            _ => (None, None, None),
        };
        let (result_callbacks, completion_callbacks) = match &reply.observer {
            Some(ObserverReply::WindowHarness { records, .. }) => (
                records
                    .iter()
                    .filter(|record| {
                        matches!(
                            record,
                            mg_sparkle::page_session::observer::ObserverRecord::Result { .. }
                        )
                    })
                    .count(),
                records
                    .iter()
                    .filter(|record| {
                        matches!(
                            record,
                            mg_sparkle::page_session::observer::ObserverRecord::Completion { .. }
                        )
                    })
                    .count(),
            ),
            _ => (0, 0),
        };
        state
            .charged_trace
            .try_reserve_exact(1)
            .map_err(|_| "Cannot reserve bounded trace")?;
        if state.charged_trace.capacity() > MAX_TRANSACTIONS {
            return Err("Host trace capacity exceeds bound".into());
        }
        let index = state.charged_trace.len();
        state.charged_trace.push(ChargedTrace {
            revision: reply.revision,
            state: reply.state,
            lifecycle: reply.lifecycle,
            progress,
            first_ordinal: first,
            next_ordinal: next,
            diagnostics: reply.errors.len(),
            scripts_executed: reply.scripts_executed,
            result_callbacks,
            completion_callbacks,
            source_admissions: reply.boa.as_ref().map(|r| r.source_admissions),
            jobs_admitted: reply.boa.as_ref().map(|r| r.jobs_admitted),
            jobs_executed: reply.boa.as_ref().map(|r| r.jobs_executed),
            pending_jobs: reply.boa.as_ref().map(|r| r.pending_jobs),
            wrapper_validation_ns: 0,
            manager_charge_ns: None,
            manager_charge_succeeded: false,
        });
        Ok(index)
    }
    fn validation(&self, index: usize, elapsed: Duration) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| "Host evidence unavailable")?;
        let row = state
            .charged_trace
            .get_mut(index)
            .ok_or("Host trace index missing")?;
        row.wrapper_validation_ns = nanos(elapsed);
        Ok(())
    }
    fn charge(&self, index: usize, elapsed: Duration, success: bool) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| "Host evidence unavailable")?;
        let row = state
            .charged_trace
            .get_mut(index)
            .ok_or("Host trace index missing")?;
        if row.manager_charge_ns.is_some() {
            return Err("Host observed a duplicate acknowledgement".into());
        }
        row.manager_charge_ns = Some(nanos(elapsed));
        row.manager_charge_succeeded = success;
        Ok(())
    }
}
fn nanos(elapsed: Duration) -> u64 {
    elapsed.as_nanos().min(u64::MAX as u128) as u64
}

pub struct InventoryScripts {
    inner: Arc<dyn ScriptRuntime>,
    inventory: Inventory,
    pub evidence: Arc<HostState>,
    generation: AtomicU64,
    closed: AtomicBool,
}
impl InventoryScripts {
    pub fn new(
        inner: Arc<dyn ScriptRuntime>,
        inventory: Inventory,
        evidence: Arc<HostState>,
    ) -> Self {
        Self {
            inner,
            inventory,
            evidence,
            generation: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }
    }
    fn admit(&self, generation: u64) -> Result<()> {
        if self.closed.load(Ordering::Acquire)
            || Instant::now() >= self.inventory.cutoff
            || generation == 0
            || self.generation.load(Ordering::Acquire) != generation
        {
            self.evidence
                .fail("Host generation/cutoff authority rejected admission");
            return Err("Host generation/cutoff authority rejected admission".into());
        }
        self.evidence.check()
    }
}

#[derive(Default)]
struct StableArenaTags {
    tags: Vec<String>,
    bytes: usize,
}
struct StagedArenaTags {
    tags: Vec<String>,
    bytes: usize,
}
impl StableArenaTags {
    fn stage(&mut self, reply: &SessionReply) -> Result<StagedArenaTags> {
        let nodes = reply
            .snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| snapshot.nodes.as_slice());
        if nodes.len() > MAX_TAGS {
            return Err("Snapshot tag node limit exceeded".into());
        }
        let appended = nodes.get(self.tags.len()..).unwrap_or_default();
        let bytes = appended
            .iter()
            .try_fold(0usize, |n, node| n.checked_add(node.tag.len()))
            .filter(|bytes| self.bytes.saturating_add(*bytes) <= MAX_TAG_BYTES)
            .ok_or("Snapshot tag byte limit exceeded")?;
        let mut tags = Vec::new();
        tags.try_reserve_exact(appended.len())
            .map_err(|_| "Cannot reserve staged tags")?;
        self.tags
            .try_reserve_exact(appended.len())
            .map_err(|_| "Cannot reserve stable tags")?;
        for node in appended {
            let mut tag = String::new();
            tag.try_reserve_exact(node.tag.len())
                .map_err(|_| "Cannot reserve tag bytes")?;
            tag.push_str(&node.tag);
            tags.push(tag);
        }
        Ok(StagedArenaTags { tags, bytes })
    }
    fn commit(&mut self, staged: StagedArenaTags) {
        self.tags.extend(staged.tags);
        self.bytes += staged.bytes;
    }
    fn validate(&self, nodes: &[Node]) -> Result<()> {
        if nodes.len() < self.tags.len()
            || self
                .tags
                .iter()
                .zip(nodes)
                .any(|(tag, node)| *tag != node.tag)
        {
            return Err("Snapshot reused or removed stable arena identities".into());
        }
        Ok(())
    }
}
enum Rejection {
    Policy,
    Infrastructure,
}

// Predicate intentionally matches wpt_render's independently reviewed policy.
fn admit_script_reply(reply: &SessionReply) -> Result<()> {
    if reply.navigation.is_some() || reply.default_action != DefaultAction::None {
        return Err("Owned observer host rejects navigation/default actions".into());
    }
    if reply.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.nodes.iter().any(|node| {
            node.tag.eq_ignore_ascii_case("meta")
                && node.attributes.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case("http-equiv")
                        && value.trim().eq_ignore_ascii_case("refresh")
                })
        })
    }) {
        return Err("Owned observer host rejects projected meta refresh".into());
    }
    Ok(())
}
fn validate_policy_rejection(
    reply: &mut SessionReply,
    page_url: &str,
    previous: Option<&StableArenaTags>,
) -> std::result::Result<(), Rejection> {
    if admit_script_reply(reply).is_ok() {
        return Ok(());
    }
    let validation = (|| {
        let snapshot = reply
            .snapshot
            .take()
            .ok_or("Denied reply omitted snapshot")?;
        if let Some(previous) = previous {
            previous.validate(&snapshot.nodes)?;
        }
        document::project_nodes(snapshot.nodes, page_url).map(|_| ())
    })();
    match validation {
        Ok(()) => Err(Rejection::Policy),
        Err(_) => Err(Rejection::Infrastructure),
    }
}
fn rejection_reason(rejection: Rejection) -> &'static str {
    match rejection {
        Rejection::Policy => "Owned host denied navigation/meta policy",
        Rejection::Infrastructure => "Denied snapshot was malformed infrastructure evidence",
    }
}

impl ScriptRuntime for InventoryScripts {
    fn observer_expectation(&self) -> ObserverExpectation {
        self.inventory.observer.expectation()
    }
    fn source_wire_mode(&self) -> SourceWireMode {
        self.inner.source_wire_mode()
    }
    fn set_generation(&self, generation: u64) -> Result<()> {
        if generation == 0
            || self.closed.load(Ordering::Acquire)
            || self
                .generation
                .compare_exchange(0, generation, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            self.evidence
                .fail("Owned host permits only one navigation generation");
            return Err("Owned host permits only one navigation generation".into());
        }
        if let Err(error) = self.inner.set_generation(generation) {
            self.evidence.fail("Inner generation admission failed");
            return Err(error);
        }
        Ok(())
    }
    fn start(
        &self,
        request: Request,
        generation: u64,
    ) -> Result<(Box<dyn ScriptSession>, SessionReply)> {
        let validation_started = Instant::now();
        self.admit(generation)?;
        if let Err(error) = self.inventory.verify(&request) {
            self.evidence
                .fail("Actual source inventory validation failed");
            return Err(error);
        }
        self.admit(generation)?;
        {
            let mut state = self
                .evidence
                .state
                .lock()
                .map_err(|_| "Host evidence unavailable")?;
            if state.starts != 0 {
                return Err("Owned host refuses a repeated worker start".into());
            }
            state.starts += 1;
        }
        let source_validation = validation_started.elapsed();
        let (mut session, mut reply) = match self.inner.start(request, generation) {
            Ok(value) => value,
            Err(error) => {
                self.evidence.fail("Inner worker startup failed");
                return Err(error);
            }
        };
        let started = Instant::now();
        let trace = match self.evidence.note_reply(&reply) {
            Ok(trace) => trace,
            Err(error) => {
                let charged =
                    session.charge_active(source_validation.saturating_add(started.elapsed()));
                session.cancel();
                self.evidence.fail("Initial host trace admission failed");
                charged?;
                return Err(error);
            }
        };
        let mut tags = StableArenaTags::default();
        let mut validation = validate_policy_rejection(&mut reply, &self.inventory.url, None)
            .and_then(|()| tags.stage(&reply).map_err(|_| Rejection::Infrastructure));
        let elapsed = source_validation.saturating_add(started.elapsed());
        if self.evidence.validation(trace, elapsed).is_err() {
            validation = Err(Rejection::Infrastructure);
        }
        let staged = match validation {
            Ok(staged) => staged,
            Err(rejection) => {
                let charged = session.charge_active(elapsed);
                let recorded = self.evidence.charge(trace, elapsed, charged.is_ok());
                session.cancel();
                self.evidence.fail(rejection_reason(rejection));
                charged?;
                recorded?;
                return Err("Initial host reply policy/projection rejected".into());
            }
        };
        Ok((
            Box::new(NoNavigationSession {
                inner: session,
                pending_validation: elapsed,
                page_url: self.inventory.url.clone(),
                tags,
                staged_tags: Some(staged),
                acknowledgement_failed: false,
                evidence: self.evidence.clone(),
                pending_trace: Some(trace),
                cutoff: self.inventory.cutoff,
            }),
            reply,
        ))
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.inner.close();
    }
    fn wait_idle(&self) -> Result<()> {
        self.inner.wait_idle()
    }
}

struct NoNavigationSession {
    inner: Box<dyn ScriptSession>,
    pending_validation: Duration,
    page_url: String,
    tags: StableArenaTags,
    staged_tags: Option<StagedArenaTags>,
    acknowledgement_failed: bool,
    evidence: Arc<HostState>,
    pending_trace: Option<usize>,
    cutoff: Instant,
}
impl ScriptSession for NoNavigationSession {
    fn id(&self) -> u64 {
        self.inner.id()
    }
    fn try_dispatch(&mut self, _: SessionInput, _: u64) -> Result<u64> {
        self.evidence
            .fail("Owned observer host does not admit activation commands");
        Err("Owned observer host does not admit activation commands".into())
    }
    fn try_task_pump(&mut self, pump: TaskPump, revision: u64) -> Result<u64> {
        if self.acknowledgement_failed
            || self.pending_trace.is_some()
            || Instant::now() >= self.cutoff
        {
            self.evidence.fail("Host task admission closed");
            return Err("Host task admission closed".into());
        }
        self.evidence.check()?;
        self.inner
            .try_task_pump(pump, revision)
            .inspect_err(|_| self.evidence.fail("Inner task admission failed"))
    }
    fn try_recv(&mut self) -> Option<SessionUpdate> {
        if self.acknowledgement_failed {
            return None;
        }
        let mut update = self.inner.try_recv()?;
        if let Ok(reply) = &mut update.reply {
            let started = Instant::now();
            let trace = match self.evidence.note_reply(reply) {
                Ok(trace) => trace,
                Err(error) => {
                    let charged = self.inner.charge_active(started.elapsed());
                    self.evidence.fail("Host reply trace failed");
                    self.inner.cancel();
                    self.acknowledgement_failed = true;
                    update.reply = Err(charged.err().unwrap_or(error));
                    return Some(update);
                }
            };
            let validation = validate_policy_rejection(reply, &self.page_url, Some(&self.tags))
                .and_then(|()| {
                    self.tags
                        .stage(reply)
                        .map_err(|_| Rejection::Infrastructure)
                });
            let elapsed = started.elapsed();
            if self.evidence.validation(trace, elapsed).is_err() {
                let charged = self.inner.charge_active(elapsed);
                self.evidence.fail("Host validation trace failed");
                self.inner.cancel();
                self.acknowledgement_failed = true;
                update.reply = Err(charged
                    .err()
                    .unwrap_or_else(|| "Host validation trace failed".into()));
                return Some(update);
            }
            match validation {
                Err(rejection) => {
                    let charged = self.inner.charge_active(elapsed);
                    let recorded = self.evidence.charge(trace, elapsed, charged.is_ok());
                    self.inner.cancel();
                    self.acknowledgement_failed = true;
                    self.staged_tags = None;
                    self.evidence.fail(rejection_reason(rejection));
                    update.reply = Err(charged
                        .err()
                        .or_else(|| recorded.err())
                        .unwrap_or_else(|| "Host reply policy/projection rejected".into()));
                }
                Ok(staged) => {
                    self.pending_validation = self.pending_validation.saturating_add(elapsed);
                    self.staged_tags = Some(staged);
                    self.pending_trace = Some(trace);
                }
            }
        } else {
            self.evidence.fail("Inner transport reply failed");
        }
        Some(update)
    }
    fn cancel(&self) {
        self.inner.cancel();
    }
    fn charge_active(&mut self, duration: Duration) -> Result<()> {
        if self.acknowledgement_failed {
            return Err("Host acknowledgement already failed".into());
        }
        let Some(trace) = self.pending_trace.take() else {
            self.acknowledgement_failed = true;
            self.inner.cancel();
            self.evidence.fail("No host reply awaits acknowledgement");
            return Err("No host reply awaits acknowledgement".into());
        };
        let validation = std::mem::take(&mut self.pending_validation);
        let staged = self.staged_tags.take();
        let total = duration.saturating_add(validation);
        let charged = self.inner.charge_active(total);
        let recorded = self.evidence.charge(trace, total, charged.is_ok());
        if charged.is_ok() && recorded.is_ok() {
            if let Some(staged) = staged {
                self.tags.commit(staged);
            }
            Ok(())
        } else {
            self.acknowledgement_failed = true;
            self.inner.cancel();
            self.evidence.fail("Inner combined acknowledgement failed");
            charged.and(recorded)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mg_sparkle::{
        js_browser::{ClassicSourceProvenance, ExternalScriptSource},
        page_session::{ArenaSnapshot, EventOutcome, LifecyclePhase, observer::ObserverContract},
    };

    fn inventory() -> Inventory {
        Inventory {
            url: "http://127.0.0.1:7878/owned/unit.html".into(),
            html:
                "<script src=\"/resources/testharness.js\"></script><script>const unit=1;</script>"
                    .into(),
            harness: include_str!("../../tools/wpt/upstream/resources/testharness.js").into(),
            harness_url: "http://127.0.0.1:7878/resources/testharness.js".into(),
            output: None,
            observer: ObserverInit::WindowHarness {
                contract: ObserverContract::PinnedWindowHarnessV1,
                run_id: 1,
                bootstrap_node: 1,
            },
            cutoff: Instant::now() + Duration::from_secs(20),
        }
    }
    fn request(value: &Inventory) -> Request {
        Request {
            url: value.url.clone(),
            html: value.html.clone(),
            external_scripts: Some(vec![ExternalScriptSource {
                node: 1,
                source: value.harness.clone(),
                provenance: Some(ClassicSourceProvenance {
                    response_url: value.harness_url.clone(),
                    error_visibility: ClassicErrorVisibility::SameOrigin,
                }),
            }]),
        }
    }
    fn reply() -> SessionReply {
        SessionReply {
            revision: 0,
            snapshot: Some(ArenaSnapshot {
                nodes: document::parse("<p>owned</p>", "http://127.0.0.1:7878/").nodes,
            }),
            outcome: EventOutcome::default(),
            default_action: DefaultAction::None,
            navigation: None,
            errors: vec![],
            scripts_executed: 2,
            allocations: None,
            boa: None,
            state: RealmState::Ready,
            acknowledgements: vec![],
            task_wake: None,
            lifecycle: Some(LifecycleProgress {
                phase: LifecyclePhase::Interactive,
                initial_failure: false,
            }),
            observer: Some(ObserverReply::WindowHarness {
                run_id: 1,
                first_ordinal: 0,
                next_ordinal: 0,
                records: vec![],
                progress: ObserverProgress::Waiting,
                execution_failed: false,
            }),
        }
    }
    #[derive(Default)]
    struct Stats {
        starts: usize,
        charges: Vec<Duration>,
        canceled: bool,
        generation: Vec<u64>,
        fail_generation: bool,
        fail_charge: bool,
        initial: Option<SessionReply>,
        later: Option<SessionReply>,
    }
    struct FakeRuntime(Arc<Mutex<Stats>>);
    struct FakeSession {
        shared: Arc<Mutex<Stats>>,
        awaiting: bool,
    }
    impl ScriptRuntime for FakeRuntime {
        fn set_generation(&self, generation: u64) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            state.generation.push(generation);
            if state.fail_generation {
                Err("fake generation failure".into())
            } else {
                Ok(())
            }
        }
        fn start(&self, _: Request, _: u64) -> Result<(Box<dyn ScriptSession>, SessionReply)> {
            let mut state = self.0.lock().unwrap();
            state.starts += 1;
            let reply = state.initial.take().unwrap();
            Ok((
                Box::new(FakeSession {
                    shared: self.0.clone(),
                    awaiting: true,
                }),
                reply,
            ))
        }
        fn close(&self) {}
        fn wait_idle(&self) -> Result<()> {
            Ok(())
        }
    }
    impl ScriptSession for FakeSession {
        fn id(&self) -> u64 {
            1
        }
        fn try_dispatch(&mut self, _: SessionInput, _: u64) -> Result<u64> {
            Err("no unit activations".into())
        }
        fn try_task_pump(&mut self, _: TaskPump, _: u64) -> Result<u64> {
            if self.awaiting {
                return Err("awaiting charge".into());
            }
            Ok(1)
        }
        fn try_recv(&mut self) -> Option<SessionUpdate> {
            assert!(!self.awaiting);
            let reply = self.shared.lock().unwrap().later.take()?;
            self.awaiting = true;
            Some(SessionUpdate {
                generation: 1,
                session_id: 1,
                sequence: Some(1),
                reply: Ok(reply),
            })
        }
        fn cancel(&self) {
            self.shared.lock().unwrap().canceled = true;
        }
        fn charge_active(&mut self, duration: Duration) -> Result<()> {
            assert!(self.awaiting, "duplicate actual inner acknowledgement");
            self.awaiting = false;
            let mut state = self.shared.lock().unwrap();
            state.charges.push(duration);
            if state.fail_charge {
                Err("fake charge failure".into())
            } else {
                Ok(())
            }
        }
    }
    fn runtime(initial: SessionReply) -> (InventoryScripts, Arc<Mutex<Stats>>) {
        let state = Arc::new(Mutex::new(Stats {
            initial: Some(initial),
            ..Stats::default()
        }));
        let host = InventoryScripts::new(
            Arc::new(FakeRuntime(state.clone())),
            inventory(),
            Arc::new(HostState::new(vec![])),
        );
        (host, state)
    }

    #[test]
    fn exact_actual_source_inventory_and_every_mutation_are_checked_before_inner_start() {
        let expected = inventory();
        assert!(expected.verify(&request(&expected)).is_ok());
        for change in 0..8 {
            let (host, state) = runtime(reply());
            host.set_generation(1).unwrap();
            let mut actual = request(&host.inventory);
            match change {
                0 => actual.html.push(' '),
                1 => actual.url.push_str("?changed"),
                2 => actual.external_scripts = None,
                3 => actual
                    .external_scripts
                    .as_mut()
                    .unwrap()
                    .push(ExternalScriptSource {
                        node: 9,
                        source: String::new(),
                        provenance: None,
                    }),
                4 => actual.external_scripts.as_mut().unwrap()[0]
                    .source
                    .push(' '),
                5 => actual.external_scripts.as_mut().unwrap()[0].node = 2,
                6 => actual.external_scripts.as_mut().unwrap()[0].provenance = None,
                _ => {
                    actual.external_scripts.as_mut().unwrap()[0]
                        .provenance
                        .as_mut()
                        .unwrap()
                        .error_visibility = ClassicErrorVisibility::Muted
                }
            }
            assert!(host.start(actual, 1).is_err());
            assert_eq!(state.lock().unwrap().starts, 0);
            assert!(host.evidence.check().is_err());
        }
        let expected = inventory();
        let mut actual = request(&expected);
        actual.external_scripts.as_mut().unwrap()[0]
            .provenance
            .as_mut()
            .unwrap()
            .response_url
            .push_str("?changed");
        assert!(expected.verify(&actual).is_err());
    }

    #[test]
    fn only_first_nonzero_generation_and_enabled_expectation_are_forwarded() {
        // The output suite has its own three-source inventory; old inventory
        // and its node1/two-script assertions above remain unchanged.
        let (host, state) = runtime(reply());
        assert_eq!(
            host.observer_expectation(),
            ObserverExpectation::WindowHarness { run_id: 1 }
        );
        assert!(host.set_generation(0).is_err());
        assert!(state.lock().unwrap().generation.is_empty());
        let (host, state) = runtime(reply());
        host.set_generation(1).unwrap();
        assert!(host.set_generation(2).is_err());
        assert_eq!(state.lock().unwrap().generation, vec![1]);
        assert_eq!(state.lock().unwrap().starts, 0);
        let (host, state) = runtime(reply());
        state.lock().unwrap().fail_generation = true;
        assert!(host.set_generation(1).is_err());
        assert!(host.start(request(&host.inventory), 1).is_err());
        assert_eq!(state.lock().unwrap().starts, 0);
    }

    #[test]
    fn successful_guard_forwards_one_combined_charge_and_preflight_time() {
        let (host, state) = runtime(reply());
        host.set_generation(1).unwrap();
        let (mut session, _) = host.start(request(&host.inventory), 1).unwrap();
        assert!(state.lock().unwrap().charges.is_empty());
        session.charge_active(Duration::from_millis(7)).unwrap();
        let locked = state.lock().unwrap();
        assert_eq!(locked.charges.len(), 1);
        assert!(locked.charges[0] >= Duration::from_millis(7));
        drop(locked);
        let evidence = host.evidence.snapshot().unwrap();
        assert!(evidence.charged_trace[0].wrapper_validation_ns > 0);
        assert!(evidence.charged_trace[0].manager_charge_succeeded);
        assert!(session.charge_active(Duration::ZERO).is_err());
        assert_eq!(state.lock().unwrap().charges.len(), 1);
        assert!(state.lock().unwrap().canceled);
        assert!(
            session
                .try_task_pump(TaskPump { wake_token: 1 }, 0)
                .is_err()
        );
    }

    #[test]
    fn output_inventory_checks_all_three_actual_positions_before_any_inner_start() {
        let manifest = output_config::Manifest::parse(include_bytes!(
            "../../docs/WPT_HARNESS_OUTPUT_CONTROLS.json"
        ))
        .unwrap();
        let case = &manifest.cases[0];
        for change in 0..5 {
            let (mut host, state) = runtime(reply());
            host.inventory.html = case.html_utf8.clone();
            host.inventory.url = format!("http://127.0.0.1:7878{}", case.url_path);
            let plan = case.source_plan(&host.inventory.url).unwrap();
            host.inventory.observer = ObserverInit::WindowHarness {
                contract: case.observer_contract,
                run_id: case.run_id,
                bootstrap_node: plan.harness_node as u32,
            };
            let vendor_url = "http://127.0.0.1:7878/resources/testharnessreport.js".to_string();
            let vendor =
                include_str!("../../tools/wpt/upstream/resources/testharnessreport.js").to_string();
            host.inventory.output = Some(output_config::SourceInventory {
                plan,
                body: case.body_source_utf8.clone(),
                vendor: vendor.clone(),
                vendor_url: vendor_url.clone(),
            });
            let mut actual = Request {
                url: host.inventory.url.clone(),
                html: case.html_utf8.clone(),
                external_scripts: Some(vec![
                    ExternalScriptSource {
                        node: plan.harness_node,
                        source: host.inventory.harness.clone(),
                        provenance: Some(ClassicSourceProvenance {
                            response_url: host.inventory.harness_url.clone(),
                            error_visibility: ClassicErrorVisibility::SameOrigin,
                        }),
                    },
                    ExternalScriptSource {
                        node: plan.vendor_node,
                        source: vendor,
                        provenance: Some(ClassicSourceProvenance {
                            response_url: vendor_url,
                            error_visibility: ClassicErrorVisibility::SameOrigin,
                        }),
                    },
                ]),
            };
            host.inventory.verify(&actual).unwrap();
            let sources = actual.external_scripts.as_mut().unwrap();
            match change {
                0 => sources[1].source.push(' '),
                1 => sources[0].node = 1,
                2 => sources.swap(0, 1),
                3 => {
                    sources.pop();
                }
                _ => sources[1]
                    .provenance
                    .as_mut()
                    .unwrap()
                    .response_url
                    .push_str("?wrong"),
            }
            host.set_generation(1).unwrap();
            assert!(host.start(actual, 1).is_err());
            assert_eq!(state.lock().unwrap().starts, 0);
        }
    }

    #[test]
    fn valid_navigation_and_meta_denials_charge_once_and_cancel() {
        for meta in [false, true] {
            let mut initial = reply();
            if meta {
                initial.snapshot = Some(ArenaSnapshot {
                    nodes: document::parse(
                        "<meta http-equiv=refresh content='0;url=/trap'>",
                        "http://127.0.0.1:7878/",
                    )
                    .nodes,
                });
            } else {
                initial.navigation = Some("http://127.0.0.1:7878/trap".into());
            }
            let (host, state) = runtime(initial);
            host.set_generation(1).unwrap();
            assert!(host.start(request(&host.inventory), 1).is_err());
            assert_eq!(state.lock().unwrap().charges.len(), 1);
            assert!(state.lock().unwrap().canceled);
            assert_eq!(
                host.evidence.snapshot().unwrap().failure,
                Some("Owned host denied navigation/meta policy")
            );
        }
    }

    #[test]
    fn malformed_denied_projection_is_infrastructure_not_a_policy_success() {
        let mut initial = reply();
        initial.navigation = Some("http://127.0.0.1:7878/trap".into());
        initial.snapshot.as_mut().unwrap().nodes[1].parent = usize::MAX;
        let (host, state) = runtime(initial);
        host.set_generation(1).unwrap();
        assert!(host.start(request(&host.inventory), 1).is_err());
        assert_eq!(state.lock().unwrap().charges.len(), 1);
        assert_eq!(
            host.evidence.snapshot().unwrap().failure,
            Some("Denied snapshot was malformed infrastructure evidence")
        );
    }

    #[test]
    fn reportless_rejected_initial_is_not_a_fake_projection_error() {
        let mut initial = reply();
        initial.snapshot = None;
        initial.scripts_executed = 0;
        initial.state = RealmState::Closed;
        initial.lifecycle = Some(LifecycleProgress {
            phase: LifecyclePhase::Initializing,
            initial_failure: true,
        });
        initial.observer = Some(ObserverReply::rejected_init(
            ObserverExpectation::WindowHarness { run_id: 1 },
        ));
        assert!(initial.is_rejected_initial());
        let (host, state) = runtime(initial);
        host.set_generation(1).unwrap();
        let (mut session, returned) = host.start(request(&host.inventory), 1).unwrap();
        assert!(returned.is_rejected_initial());
        session.charge_active(Duration::ZERO).unwrap();
        assert_eq!(state.lock().unwrap().charges.len(), 1);
    }

    #[test]
    fn failed_real_ack_does_not_commit_tags_and_blocks_further_work() {
        let (host, state) = runtime(reply());
        host.set_generation(1).unwrap();
        state.lock().unwrap().fail_charge = true;
        let (mut session, _) = host.start(request(&host.inventory), 1).unwrap();
        assert!(session.charge_active(Duration::ZERO).is_err());
        assert!(
            session
                .try_task_pump(TaskPump { wake_token: 1 }, 0)
                .is_err()
        );
        assert!(session.try_recv().is_none());
        assert_eq!(state.lock().unwrap().charges.len(), 1);
        assert!(state.lock().unwrap().canceled);
    }

    #[test]
    fn later_denial_checks_previously_acknowledged_tag_prefix_and_charges_once() {
        let (host, state) = runtime(reply());
        host.set_generation(1).unwrap();
        let (mut session, _) = host.start(request(&host.inventory), 1).unwrap();
        session.charge_active(Duration::ZERO).unwrap();
        let mut later = reply();
        later.revision = 1;
        later.scripts_executed = 0;
        later.navigation = Some("http://127.0.0.1:7878/trap".into());
        later.snapshot.as_mut().unwrap().nodes[1].tag = "changed-stable-tag".into();
        state.lock().unwrap().later = Some(later);
        assert!(session.try_recv().unwrap().reply.is_err());
        assert_eq!(state.lock().unwrap().charges.len(), 2);
        assert!(state.lock().unwrap().canceled);
        assert_eq!(
            host.evidence.snapshot().unwrap().failure,
            Some("Denied snapshot was malformed infrastructure evidence")
        );
    }
}
