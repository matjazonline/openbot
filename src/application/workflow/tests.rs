use super::*;
use crate::application::app_error::{AppError, AppResult};
use crate::application::use_cases::participant::PrincipalAccessPersistence;
use crate::domain::entities::{
    channel::Channel, company_member::CompanyMembership, correlation::CorrelationId,
    message::CanonicalMessageId, participant::PrincipalAccessContext, thread::Thread,
};
use crate::domain::workflow::{
    ActionInvocationId, ActionRef, ChildCause, ExecutionId, ExecutionLimits, ExecutionRef, Routes,
    RunCausality, RunId, RunState, ScheduleId, ScheduleOccurrenceId, StepCausality, StepDefinition,
    TransitionTarget, TriggerId, TriggerRef, TriggerSource, TypeName, VersionId,
    WorkflowDefinition, WorkflowId, validate,
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

fn owned(company_id: CompanyId, version_id: VersionId, context_bytes: usize) -> OwnedVersion {
    let workflow_id = WorkflowId::new(Uuid::new_v4());
    let definition = WorkflowDefinition {
        format_version: 1,
        workflow_id,
        version_id,
        input_schema: None,
        parameter_schema: None,
        output_schema: None,
        resources: vec![],
        entry: step_id("actual_entry"),
        steps: BTreeMap::from([(
            step_id("actual_entry"),
            StepDefinition {
                step_type: TypeName::parse("agent.run").unwrap(),
                inputs: BTreeMap::new(),
                routes: Routes::Success(TransitionTarget::End),
                final_error: None,
            },
        )]),
        limits: ExecutionLimits {
            max_steps: 10,
            max_context_bytes: context_bytes,
        },
    };
    OwnedVersion {
        company_id,
        workflow_id,
        version_id,
        definition: validate(definition).unwrap(),
    }
}

fn request(
    company_id: CompanyId,
    version_id: VersionId,
    key: &str,
    input: Value,
    params: Value,
) -> AdmitWorkflowRequest {
    AdmitWorkflowRequest {
        company_id,
        actor: actor(),
        association: RelatedAssociation::Company,
        trigger: TriggerRef::new(company_id, trigger_id_for_key(key), TriggerSource::Manual)
            .unwrap(),
        correlation_id: CorrelationId::new(),
        version_id,
        idempotency_key: IdempotencyKey::parse(key).unwrap(),
        input,
        params,
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
    versions: BTreeMap<(CompanyId, VersionId), OwnedVersion>,
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
    returned_version: Option<OwnedVersion>,
    returned_head: Option<RunHead>,
}

struct StoredAdmission {
    causality: RunCausality,
    workflow_id: WorkflowId,
    version_id: VersionId,
    input: Value,
    params: Value,
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
    fn new(version: OwnedVersion) -> Self {
        let mut state = State::default();
        state.access.insert(
            (version.company_id, actor().user_id()),
            PrincipalAccessContext {
                principal_id: None,
                membership: CompanyMembership::Owner,
            },
        );
        state
            .versions
            .insert((version.company_id, version.version_id), version);
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
impl WorkflowDefinitions for MemoryStore {
    async fn published_version(
        &self,
        company_id: CompanyId,
        version_id: VersionId,
    ) -> AppResult<Option<OwnedVersion>> {
        let result = {
            let state = self.state.lock().unwrap();
            if state.fail_lookup {
                return Err(AppError::Database("lookup failed".into()));
            }
            state
                .returned_version
                .clone()
                .or_else(|| state.versions.get(&(company_id, version_id)).cloned())
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
            if saved.workflow_id != command.workflow_id()
                || saved.version_id != command.version_id()
                || saved.input != *command.input()
                || saved.params != *command.params()
                || saved.association != command.association()
                || saved.causality.trigger() != command.trigger()
            {
                return Ok(AdmissionResult::Conflict);
            }
            let run_id = saved.causality.run_id();
            validate_source(&state, command)?;
            return Ok(AdmissionResult::Replayed(run_id));
        }
        validate_source(&state, command)?;
        state.admissions.insert(
            key,
            StoredAdmission {
                causality: command.causality().clone(),
                workflow_id: command.workflow_id(),
                version_id: command.version_id(),
                input: command.input().clone(),
                params: command.params().clone(),
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
