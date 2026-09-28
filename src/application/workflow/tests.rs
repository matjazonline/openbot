use super::*;
use crate::application::app_error::{AppError, AppResult};
use crate::application::use_cases::participant::PrincipalAccessPersistence;
use crate::domain::entities::{
    channel::Channel, company_member::CompanyMembership, correlation::CorrelationId,
    message::CanonicalMessageId, participant::PrincipalAccessContext, thread::Thread,
};
use crate::domain::workflow::{
    ActionInvocationId, ActionRef, BindingRevision, ChildCause, ExecutionId, ExecutionRef,
    RunCausality, RunId, RunState, ScheduleId, ScheduleOccurrenceId, StepCausality, TriggerId,
    TriggerRef, TriggerSource, VersionId, WorkflowBindingId, WorkflowId,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::Barrier;
use uuid::Uuid;

fn company() -> CompanyId {
    CompanyId::new(Uuid::new_v4())
}
fn actor() -> WorkflowActor {
    WorkflowActor::authenticated(Uuid::from_u128(1)).unwrap()
}
fn version() -> VersionId {
    VersionId::new(Uuid::new_v4())
}
fn step_id(value: &str) -> crate::domain::workflow::StepId {
    crate::domain::workflow::StepId::parse(value).unwrap()
}
fn test_causality(company_id: CompanyId, run_id: RunId) -> RunCausality {
    RunCausality::new(
        run_id,
        TriggerRef::new(
            company_id,
            TriggerId::new(Uuid::new_v4()),
            TriggerSource::Manual,
        )
        .unwrap(),
        CorrelationId::new(),
    )
    .unwrap()
}
fn trigger_id_for_key(key: &str) -> TriggerId {
    let value = key.bytes().fold(0xcbf29ce484222325_u128, |hash, byte| {
        (hash ^ u128::from(byte)).wrapping_mul(0x100000001b3)
    });
    TriggerId::new(Uuid::from_u128(value))
}

// These fixtures reuse the version UUID for the logical binding UUID to keep
// the existing source/authorization matrix concise; production IDs are distinct types.
fn owned(
    company_id: CompanyId,
    version_id: VersionId,
    context_bytes: usize,
) -> Arc<binding::ConfiguredBinding> {
    configured(company_id, version_id, context_bytes, json!(2))
}

fn configured(
    company_id: CompanyId,
    version_id: VersionId,
    context_bytes: usize,
    params: Value,
) -> Arc<binding::ConfiguredBinding> {
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let entry = source["entry"].as_str().unwrap().to_owned();
    let step = source["steps"]
        .as_object_mut()
        .unwrap()
        .remove(&entry)
        .unwrap();
    source["steps"]["actual_entry"] = step;
    source["entry"] = json!("actual_entry");
    source["limits"]["max_context_bytes"] = json!(context_bytes);
    source["input_schema"] = json!({});
    source["parameter_schema"] = json!({});
    source["steps"]["actual_entry"]["with"]["output_schema"] = json!({"literal":{}});
    let bundle = Arc::new(
        publication::freeze(
            crate::adapters::workflow_source::decode(&source.to_string()).unwrap(),
            company_id,
            version_id,
            publication::DependencySnapshots::default(),
            vec![],
        )
        .unwrap(),
    );
    Arc::new(
        binding::ConfiguredBinding::new(
            binding::BindingConfiguration {
                id: WorkflowBindingId::new(version_id.as_uuid()),
                revision: BindingRevision::new(1).unwrap(),
                company_id,
                params,
                resources: BTreeMap::new(),
            },
            bundle,
        )
        .unwrap(),
    )
}

fn request(
    company_id: CompanyId,
    version_id: VersionId,
    key: &str,
    input: Value,
) -> AdmitWorkflowRequest {
    AdmitWorkflowRequest {
        company_id,
        actor: actor(),
        association: RelatedAssociation::Company,
        trigger: TriggerRef::new(company_id, trigger_id_for_key(key), TriggerSource::Manual)
            .unwrap(),
        correlation_id: CorrelationId::new(),
        binding_id: WorkflowBindingId::new(version_id.as_uuid()),
        idempotency_key: IdempotencyKey::parse(key).unwrap(),
        input,
    }
}

#[derive(Clone)]
struct MemoryStore {
    state: Arc<Mutex<State>>,
    admission_barrier: Option<Arc<Barrier>>,
    claim_barrier: Option<Arc<Barrier>>,
}

#[derive(Default)]
struct State {
    versions: BTreeMap<(CompanyId, VersionId), Arc<binding::ConfiguredBinding>>,
    admissions: BTreeMap<(CompanyId, IdempotencyKey), StoredAdmission>,
    runs: BTreeMap<(CompanyId, RunId), TestRun>,
    access: BTreeMap<(CompanyId, Uuid), PrincipalAccessContext>,
    channels: BTreeMap<Uuid, Channel>,
    threads: BTreeMap<Uuid, Thread>,
    source_messages: BTreeMap<(CompanyId, CanonicalMessageId), RelatedAssociation>,
    schedule_occurrences:
        BTreeMap<(CompanyId, ScheduleId, ScheduleOccurrenceId), RelatedAssociation>,
    parent_executions: BTreeMap<(CompanyId, ExecutionId), ExecutionRef>,
    parent_actions: BTreeMap<(CompanyId, ActionInvocationId), ActionRef>,
    fail_source: bool,
    fail_access: bool,
    fail_channel: bool,
    fail_thread: bool,
    jobs: Vec<TestJob>,
    fail_lookup: bool,
    fail_admit: bool,
    fail_head: bool,
    fail_cancel: bool,
    returned_version: Option<Arc<binding::ConfiguredBinding>>,
    returned_head: Option<RunHead>,
    replace_on_admit: Option<Arc<binding::ConfiguredBinding>>,
}

#[derive(Clone)]
struct StoredAdmission {
    causality: RunCausality,
    command_trigger: TriggerRef,
    version_id: VersionId,
    input: Value,
    params: Value,
    binding: Arc<binding::ConfiguredBinding>,
    association: RelatedAssociation,
}

struct TestRun {
    causality: RunCausality,
    workflow_id: WorkflowId,
    version_id: VersionId,
    revision: RunRevision,
    state: RunState,
    association: RelatedAssociation,
}
struct TestJob {
    step: StepCausality,
    claimed_until: Option<SystemTime>,
    invalidated: bool,
}
impl TestJob {
    fn company_id(&self) -> CompanyId {
        self.step.execution().company_id()
    }
    fn run_id(&self) -> RunId {
        self.step.execution().run_id()
    }
    fn step_id(&self) -> &crate::domain::workflow::StepId {
        self.step.execution().step_id()
    }
}

impl MemoryStore {
    fn new(version: Arc<binding::ConfiguredBinding>) -> Self {
        let mut state = State::default();
        state.access.insert(
            (version.company_id(), actor().user_id()),
            PrincipalAccessContext {
                principal_id: None,
                membership: CompanyMembership::Owner,
            },
        );
        state.versions.insert(
            (version.company_id(), VersionId::new(version.id().as_uuid())),
            version,
        );
        Self {
            state: Arc::new(Mutex::new(state)),
            admission_barrier: None,
            claim_barrier: None,
        }
    }
    fn with_admission_barrier(mut self) -> Self {
        self.admission_barrier = Some(Arc::new(Barrier::new(2)));
        self
    }
    fn with_claim_barrier(mut self) -> Self {
        self.claim_barrier = Some(Arc::new(Barrier::new(2)));
        self
    }
}

#[async_trait]
impl WorkflowBindings for MemoryStore {
    async fn admission_binding(
        &self,
        company_id: CompanyId,
        binding_id: WorkflowBindingId,
        key: &IdempotencyKey,
        trigger: &TriggerRef,
    ) -> AppResult<Option<Arc<binding::ConfiguredBinding>>> {
        let result = {
            let state = self.state.lock().unwrap();
            if state.fail_lookup {
                return Err(AppError::Database("lookup failed".into()));
            }
            state
                .admissions
                .get(&(company_id, key.clone()))
                .filter(|saved| saved.binding.id() == binding_id)
                .or_else(|| {
                    state.admissions.values().find(|saved| {
                        saved.binding.company_id() == company_id
                            && saved.binding.id() == binding_id
                            && same_source_event(saved.causality.trigger(), trigger)
                    })
                })
                .map(|saved| saved.binding.clone())
                .or_else(|| state.returned_version.clone())
                .or_else(|| {
                    state
                        .versions
                        .get(&(company_id, VersionId::new(binding_id.as_uuid())))
                        .cloned()
                })
        };
        if let Some(barrier) = &self.admission_barrier {
            barrier.wait().await;
        }
        Ok(result)
    }
}

#[async_trait]
impl WorkflowAdmission for MemoryStore {
    async fn admit(&self, command: &PreparedAdmission) -> AppResult<AdmissionResult> {
        let mut state = self.state.lock().unwrap();
        if state.fail_admit {
            return Err(AppError::Database("write failed".into()));
        }
        let key = (command.company_id(), command.idempotency_key().clone());
        if let Some(saved) = state.admissions.get(&key) {
            if saved.binding.id() != command.binding().id()
                || saved.input != *command.input()
                || saved.association != command.association()
                || saved.command_trigger != *command.trigger()
            {
                return Ok(AdmissionResult::Conflict);
            }
            let run_id = saved.causality.run_id();
            validate_source(&state, command)?;
            return Ok(AdmissionResult::Replayed(run_id));
        }
        if let Some(replayed) = replay_source_alias(&mut state, command)? {
            return Ok(replayed);
        }
        if let Some(next) = state.replace_on_admit.take() {
            state.versions.insert(
                (next.company_id(), VersionId::new(next.id().as_uuid())),
                next,
            );
        }
        let selected = state.versions.get(&(
            command.company_id(),
            VersionId::new(command.binding().id().as_uuid()),
        ));
        if !selected.is_some_and(|selected| Arc::ptr_eq(selected, command.binding())) {
            return Err(AppError::Conflict("binding selection changed".into()));
        }
        validate_source(&state, command)?;
        state.admissions.insert(
            key,
            StoredAdmission {
                causality: command.causality().clone(),
                command_trigger: command.trigger().clone(),
                version_id: command.version_id(),
                input: command.input().clone(),
                params: command.params().clone(),
                binding: command.binding().clone(),
                association: command.association(),
            },
        );
        state.runs.insert(
            (command.company_id(), command.proposed_run_id()),
            TestRun {
                causality: command.causality().clone(),
                workflow_id: command.workflow_id(),
                version_id: command.version_id(),
                revision: RunRevision(1),
                state: RunState::Queued,
                association: command.association(),
            },
        );
        state.jobs.push(TestJob {
            step: command.first_step().clone(),
            claimed_until: None,
            invalidated: false,
        });
        Ok(AdmissionResult::Created(command.proposed_run_id()))
    }
}

fn same_source_event(left: &TriggerRef, right: &TriggerRef) -> bool {
    left.company_id() == right.company_id()
        && left.source() == right.source()
        && (!matches!(left.source(), TriggerSource::Manual)
            || left.trigger_id() == right.trigger_id())
}

fn replay_source_alias(
    state: &mut State,
    command: &PreparedAdmission,
) -> AppResult<Option<AdmissionResult>> {
    let Some(mut saved) = state
        .admissions
        .values()
        .find(|saved| {
            saved.binding.company_id() == command.company_id()
                && saved.binding.id() == command.binding().id()
                && same_source_event(saved.causality.trigger(), command.trigger())
        })
        .cloned()
    else {
        return Ok(None);
    };
    if saved.input != *command.input() || saved.association != command.association() {
        return Ok(Some(AdmissionResult::Conflict));
    }
    validate_source(state, command)?;
    let run_id = saved.causality.run_id();
    saved.command_trigger = command.trigger().clone();
    state.admissions.insert(
        (command.company_id(), command.idempotency_key().clone()),
        saved,
    );
    Ok(Some(AdmissionResult::Replayed(run_id)))
}

#[async_trait]
impl WorkflowInspection for MemoryStore {
    async fn head(&self, company_id: CompanyId, run_id: RunId) -> AppResult<Option<RunHead>> {
        let state = self.state.lock().unwrap();
        if state.fail_head {
            return Err(AppError::Database("head failed".into()));
        }
        Ok(state.returned_head.clone().or_else(|| {
            state.runs.get(&(company_id, run_id)).map(|run| RunHead {
                causality: run.causality.clone(),
                workflow_id: run.workflow_id,
                version_id: run.version_id,
                association: run.association,
                state: run.state,
                revision: run.revision,
            })
        }))
    }
}

#[async_trait]
impl WorkflowRunTransitions for MemoryStore {
    async fn cancel(&self, command: CancelCommand) -> AppResult<CancelResult> {
        let mut state = self.state.lock().unwrap();
        if state.fail_cancel {
            return Err(AppError::Database("cancel failed".into()));
        }
        let Some(run) = state.runs.get_mut(&(command.company_id, command.run_id)) else {
            return Ok(CancelResult::NotFound);
        };
        if run.state.is_terminal() {
            return Ok(CancelResult::AlreadyTerminalOrApplied {
                revision: run.revision,
            });
        }
        if run.revision != command.expected_revision {
            return Ok(CancelResult::RevisionConflict {
                current_revision: run.revision,
            });
        }
        run.revision.0 += 1;
        run.state = run.state.cancel();
        let revision = run.revision;
        for job in &mut state.jobs {
            if job.company_id() == command.company_id && job.run_id() == command.run_id {
                job.invalidated = true;
                job.claimed_until = None;
            }
        }
        Ok(CancelResult::Applied { revision })
    }
}

#[async_trait]
impl WorkflowExecutionScheduling for MemoryStore {
    async fn claim_ready(&self, request: ClaimRequest) -> AppResult<Vec<ClaimedExecution>> {
        if let Some(barrier) = &self.claim_barrier {
            barrier.wait().await;
        }
        let mut state = self.state.lock().unwrap();
        let now = SystemTime::now();
        let mut claims = Vec::new();
        for index in 0..state.jobs.len() {
            if claims.len() >= usize::from(request.batch.get()) {
                break;
            }
            let job = &state.jobs[index];
            if job.invalidated || job.claimed_until.is_some_and(|until| until > now) {
                continue;
            }
            let key = (job.company_id(), job.run_id());
            let Some(run) = state.runs.get_mut(&key) else {
                continue;
            };
            match run.state {
                RunState::Queued => {
                    run.state = run.state.start().expect("queued run can start");
                    run.revision.0 += 1;
                }
                RunState::Running => {}
                RunState::Waiting(_)
                | RunState::Succeeded
                | RunState::Failed
                | RunState::Cancelled => continue,
            }
            let run_causality = state.runs[&key].causality.clone();
            let job = &mut state.jobs[index];
            let expiry = now + request.lease.get();
            job.claimed_until = Some(expiry);
            claims.push(ClaimedExecution::new(
                run_causality,
                job.step.clone(),
                request.worker_id,
                FenceToken::new(Uuid::new_v4()),
                OwnershipReceipt::new(Uuid::new_v4()),
                expiry,
            )?);
        }
        Ok(claims)
    }
}

fn service(
    store: &MemoryStore,
) -> WorkflowService<
    MemoryStore,
    MemoryStore,
    MemoryStore,
    MemoryStore,
    LifecycleAuthorizer<MemoryStore, MemoryStore, MemoryStore>,
> {
    WorkflowService::new(
        store.clone(),
        store.clone(),
        store.clone(),
        store.clone(),
        LifecycleAuthorizer::new(store.clone(), store.clone(), store.clone()),
    )
}

#[async_trait]
impl PrincipalAccessPersistence for MemoryStore {
    async fn access_context_for_identity(
        &self,
        _company_id: Uuid,
        _identity: &crate::domain::entities::transport::QualifiedIdentity,
    ) -> AppResult<PrincipalAccessContext> {
        Err(AppError::Internal(
            "identity access is outside workflow tests".into(),
        ))
    }
    async fn access_context_for_user(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<Option<PrincipalAccessContext>> {
        let state = self.state.lock().unwrap();
        if state.fail_access {
            return Err(AppError::Database("access failed".into()));
        }
        Ok(state
            .access
            .get(&(CompanyId::new(company_id), user_id))
            .copied())
    }
}

#[async_trait]
impl WorkflowChannelReader for MemoryStore {
    async fn workflow_channel(&self, id: RelatedChannelId) -> AppResult<Option<Channel>> {
        let state = self.state.lock().unwrap();
        if state.fail_channel {
            return Err(AppError::Database("channel failed".into()));
        }
        Ok(state.channels.get(&id.as_uuid()).cloned())
    }
}

#[async_trait]
impl WorkflowThreadReader for MemoryStore {
    async fn workflow_thread(&self, id: RelatedThreadId) -> AppResult<Option<Thread>> {
        let state = self.state.lock().unwrap();
        if state.fail_thread {
            return Err(AppError::Database("thread failed".into()));
        }
        Ok(state.threads.get(&id.as_uuid()).cloned())
    }
}

mod authorization_cases;
mod cases;
mod causality_cases;
mod source_validation;
mod state_cases;
use source_validation::validate_source;

#[path = "tests/snapshot_cases.rs"]
mod snapshot_cases;
