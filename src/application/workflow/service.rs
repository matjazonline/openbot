use super::contracts::AdmissionSnapshots;
use super::{
    AdmissionResult, AdmitWorkflowRequest, CancelCommand, CancelResult, CancelWorkflowRequest,
    OwnedVersion, PreparedAdmission, WorkflowAdmission, WorkflowAuthorization, WorkflowDefinitions,
    WorkflowInspection, WorkflowOperation, WorkflowRunTransitions,
};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::workflow::{
    Binding, Context, ContextLimits, ContextReference, ExecutionId, RunId, RunMetadata,
    TriggerSource, ValidatedWorkflow, resolve,
};
use std::collections::BTreeMap;
use uuid::Uuid;

pub struct WorkflowService<D, A, T, I, H> {
    definitions: D,
    admission: A,
    transitions: T,
    inspection: I,
    authorization: H,
}

impl<D, A, T, I, H> WorkflowService<D, A, T, I, H>
where
    D: WorkflowDefinitions,
    A: WorkflowAdmission,
    T: WorkflowRunTransitions,
    I: WorkflowInspection,
    H: WorkflowAuthorization,
{
    pub fn new(
        definitions: D,
        admission: A,
        transitions: T,
        inspection: I,
        authorization: H,
    ) -> Self {
        Self {
            definitions,
            admission,
            transitions,
            inspection,
            authorization,
        }
    }

    /// Authorizes the current actor and related visibility before any run write.
    /// A definition with unresolved resource requirements is rejected with
    /// `BadRequest`; structural validation and lifecycle access do not authorize
    /// tools, resources, or execution effects.
    pub async fn admit(&self, request: AdmitWorkflowRequest) -> AppResult<AdmissionResult> {
        self.authorization
            .authorize(
                request.company_id,
                request.actor,
                request.association,
                WorkflowOperation::Admit,
            )
            .await?;
        let version = self
            .definitions
            .published_version(request.company_id, request.version_id)
            .await?
            .ok_or_else(|| AppError::NotFound("workflow version".into()))?;
        verify_version(&version, &request)?;
        if !version.definition.definition().resources.is_empty() {
            return Err(AppError::BadRequest(
                "workflow resources are not yet resolvable".into(),
            ));
        }
        let proposed_run = RunId::new(Uuid::new_v4());
        let snapshots = bounded_snapshots(&request, &version.definition, proposed_run)?;
        let command = PreparedAdmission::new(
            request,
            &version,
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

fn verify_version(version: &OwnedVersion, request: &AdmitWorkflowRequest) -> AppResult<()> {
    let definition = version.definition.definition();
    if version.company_id != request.company_id
        || request.trigger.company_id() != request.company_id
        || version.version_id != request.version_id
        || version.workflow_id != definition.workflow_id
        || version.version_id != definition.version_id
    {
        return Err(AppError::Internal(
            "workflow definition returned mismatched scope or identity".into(),
        ));
    }
    Ok(())
}

/// The definition's context-byte ceiling is aggregate across both snapshots.
/// Each snapshot also consumes its own domain work/depth budget before cloning.
fn bounded_snapshots(
    request: &AdmitWorkflowRequest,
    definition: &ValidatedWorkflow,
    run_id: RunId,
) -> AppResult<AdmissionSnapshots> {
    let outputs = BTreeMap::new();
    let context = Context {
        input: &request.input,
        params: &request.params,
        step_outputs: &outputs,
        run: RunMetadata {
            run_id,
            parent_run_id: match request.trigger.source() {
                TriggerSource::Child { parent } => Some(parent.execution().run_id()),
                _ => None,
            },
        },
    };
    let limits = definition.context_limits();
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
    let params = resolve(
        &Binding::Reference(ContextReference::parse("/params").expect("static reference")),
        &context,
        ContextLimits {
            output_bytes: remaining,
            work_nodes: limits.work_nodes,
        },
    )
    .map_err(|error| AppError::BadRequest(format!("workflow params: {error}")))?;
    Ok(AdmissionSnapshots { input, params })
}
