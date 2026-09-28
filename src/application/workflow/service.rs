use super::contracts::AdmissionSnapshots;
use super::{
    AdmissionResult, AdmitWorkflowRequest, CancelCommand, CancelResult, CancelWorkflowRequest,
    PreparedAdmission, WorkflowAdmission, WorkflowAuthorization, WorkflowBindings,
    WorkflowInspection, WorkflowOperation, WorkflowRunTransitions, binding::ConfiguredBinding,
};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::workflow::{
    Binding, Context, ContextLimits, ContextReference, ExecutionId, RunId, RunMetadata,
    TriggerSource, resolve,
};
use std::collections::BTreeMap;
use uuid::Uuid;

pub struct WorkflowService<D, A, T, I, H> {
    bindings: D,
    admission: A,
    transitions: T,
    inspection: I,
    authorization: H,
}

impl<D, A, T, I, H> WorkflowService<D, A, T, I, H>
where
    D: WorkflowBindings,
    A: WorkflowAdmission,
    T: WorkflowRunTransitions,
    I: WorkflowInspection,
    H: WorkflowAuthorization,
{
    pub fn new(bindings: D, admission: A, transitions: T, inspection: I, authorization: H) -> Self {
        Self {
            bindings,
            admission,
            transitions,
            inspection,
            authorization,
        }
    }

    /// Authorizes every admission/replay, then captures one checked immutable binding.
    /// Resource readiness is checked on activation; effects recheck current access,
    /// revocation and secrets at use time. A frozen snapshot is never an access grant.
    pub async fn admit(&self, request: AdmitWorkflowRequest) -> AppResult<AdmissionResult> {
        self.authorization
            .authorize(
                request.company_id,
                request.actor,
                request.association,
                WorkflowOperation::Admit,
            )
            .await?;
        let binding = self
            .bindings
            .admission_binding(
                request.company_id,
                request.binding_id,
                &request.idempotency_key,
                &request.trigger,
            )
            .await?
            .ok_or_else(|| AppError::NotFound("workflow binding".into()))?;
        verify_binding(&binding, &request)?;
        let proposed_run = RunId::new(Uuid::new_v4());
        let snapshots = bounded_snapshots(&request, &binding, proposed_run)?;
        let command = PreparedAdmission::new(
            request,
            binding,
            proposed_run,
            ExecutionId::new(Uuid::new_v4()),
            snapshots,
        )?;
        self.admission.admit(&command).await
    }

    pub async fn cancel(&self, request: CancelWorkflowRequest) -> AppResult<CancelResult> {
        self.authorization
            .authorize(
                request.company_id,
                request.actor,
                super::RelatedAssociation::Company,
                WorkflowOperation::Cancel,
            )
            .await?;
        let Some(head) = self
            .inspection
            .head(request.company_id, request.run_id)
            .await?
        else {
            return Ok(CancelResult::NotFound);
        };
        if head.company_id() != request.company_id || head.run_id() != request.run_id {
            return Err(AppError::Internal(
                "workflow inspection returned mismatched run scope".into(),
            ));
        }
        self.authorization
            .authorize(
                request.company_id,
                request.actor,
                head.association,
                WorkflowOperation::Cancel,
            )
            .await?;
        self.transitions
            .cancel(CancelCommand {
                company_id: request.company_id,
                run_id: request.run_id,
                expected_revision: head.revision,
            })
            .await
    }
}

fn verify_binding(binding: &ConfiguredBinding, request: &AdmitWorkflowRequest) -> AppResult<()> {
    if binding.company_id() != request.company_id
        || request.trigger.company_id() != request.company_id
        || binding.id() != request.binding_id
    {
        return Err(AppError::Internal(
            "workflow binding returned mismatched scope or identity".into(),
        ));
    }
    Ok(())
}

/// The definition's context-byte ceiling is aggregate across both snapshots.
/// Each snapshot also consumes its own domain work/depth budget before cloning.
fn bounded_snapshots(
    request: &AdmitWorkflowRequest,
    binding: &ConfiguredBinding,
    run_id: RunId,
) -> AppResult<AdmissionSnapshots> {
    let outputs = BTreeMap::new();
    let context = Context {
        input: &request.input,
        params: binding.params(),
        step_outputs: &outputs,
        run: RunMetadata {
            run_id,
            parent_run_id: match request.trigger.source() {
                TriggerSource::Child { parent } => Some(parent.execution().run_id()),
                _ => None,
            },
        },
    };
    let limits = binding.bundle().compiled().graph().context_limits();
    let input = resolve(
        &Binding::Reference(ContextReference::parse("/input").expect("static reference")),
        &context,
        limits,
    )
    .map_err(|error| AppError::BadRequest(format!("workflow input: {error}")))?;
    let input_bytes = serde_json::to_vec(&input)
        .map_err(|error| AppError::Internal(format!("bounded input encoding: {error}")))?
        .len();
    let remaining = limits.output_bytes - input_bytes;
    resolve(
        &Binding::Reference(ContextReference::parse("/params").expect("static reference")),
        &context,
        ContextLimits {
            output_bytes: remaining,
            work_nodes: limits.work_nodes,
        },
    )
    .map_err(|error| AppError::BadRequest(format!("workflow params: {error}")))?;
    binding
        .bundle()
        .compiled()
        .validate_input(&input)
        .map_err(|error| {
            AppError::BadRequest(format!(
                "workflow input ({}): {}",
                error.code, error.message
            ))
        })?;
    Ok(AdmissionSnapshots { input })
}
