use super::*;
use crate::application::app_error::{AppError, AppResult};
use crate::application::workflow::{
    RelatedAssociation, WorkflowAuthorization, WorkflowOperation, binding::ResourceDirectory,
};
use crate::domain::workflow::RuntimeResourceId;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// A snapshot or exact replay, both requiring current resource access. Refusals such
/// as revision conflict are durably recorded by snapshot without attaching evidence.
pub enum ReconciliationPreparation {
    Snapshot(Box<ReconciliationSnapshot>),
    Recorded {
        action: Box<super::super::FrozenAction>,
        result: ReconciliationResult,
    },
}

#[async_trait]
pub trait ActionReconciliation: Send + Sync {
    /// Called only after owner/admin preflight. Read the actual scoped association;
    /// never accept an association supplied by the command caller.
    async fn association(&self, command: &ReconcileActionCommand) -> AppResult<RelatedAssociation>;
    /// First bounded transaction: company/principal before run, actual association,
    /// execution/job and command-actor resource locks. Reauthorize exact replay too.
    /// Restore frozen bundle/resource/subject/marker independently of dispatch loader;
    /// detect129 entries/siblings as BoundExceeded, never truncate. No provider I/O.
    async fn snapshot(
        &self,
        command: &ReconcileActionCommand,
    ) -> AppResult<ReconciliationPreparation>;
    /// Second bounded transaction repeats current actor and resource authorization,
    /// immutable operation/marker/ALL entries/revision/registration and DB-clock bounds.
    /// Changed snapshot refuses with no grant. Evidence/receipt/conflict/reopen/audit
    /// commit atomically; generated final revision precedes immutable command receipt.
    /// Only waiting/reconciliation existing jobs may reopen, never terminal runs.
    async fn settle(
        &self,
        command: &ReconcileActionCommand,
        snapshot: &ReconciliationSnapshot,
        evidence: VerifiedEvidence,
    ) -> AppResult<ReconciliationResult>;
}

pub struct ActionReconciliationService<P, A, R> {
    persistence: P,
    authorization: A,
    resources: R,
    verifier: Option<Arc<dyn ActionEvidenceVerifier>>,
}
impl<P, A, R> ActionReconciliationService<P, A, R> {
    /// Install the approved verifier only at trusted host composition. Command
    /// callers can name a registration, but cannot supply or replace its verifier.
    pub fn new(
        persistence: P,
        authorization: A,
        resources: R,
        verifier: Option<Arc<dyn ActionEvidenceVerifier>>,
    ) -> Self {
        Self {
            persistence,
            authorization,
            resources,
            verifier,
        }
    }
}
impl<P: ActionReconciliation, A: WorkflowAuthorization, R: ResourceDirectory>
    ActionReconciliationService<P, A, R>
{
    pub async fn reconcile(
        &self,
        command: &ReconcileActionCommand,
        cancellation: &CancellationToken,
        operation_budget: Duration,
    ) -> AppResult<ReconciliationResult> {
        command.request_digest()?;
        if operation_budget.is_zero() {
            return Err(AppError::Timeout("Workflow reconciliation budget".into()));
        }
        // Box this orchestration seam so provider futures do not inflate its callers.
        let operation = Box::pin(self.reconcile_on(command, cancellation));
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(AppError::Conflict("Workflow reconciliation cancelled".into())),
            result = tokio::time::timeout(operation_budget.min(Duration::from_secs(5)), operation) => {
                result.map_err(|_| AppError::Timeout("Workflow reconciliation verification".into()))?
            }
        }
    }

    async fn reconcile_on(
        &self,
        command: &ReconcileActionCommand,
        cancellation: &CancellationToken,
    ) -> AppResult<ReconciliationResult> {
        self.authorization
            .authorize(
                command.scope.company,
                command.actor,
                RelatedAssociation::Company,
                WorkflowOperation::Reconcile,
            )
            .await?;
        let association = self.persistence.association(command).await?;
        self.authorization
            .authorize(
                command.scope.company,
                command.actor,
                association,
                WorkflowOperation::Reconcile,
            )
            .await?;
        let prepared = self.persistence.snapshot(command).await?;
        let (action, snapshot) = match &prepared {
            ReconciliationPreparation::Snapshot(snapshot) => {
                (&snapshot.action, Some(snapshot.as_ref()))
            }
            ReconciliationPreparation::Recorded { action, .. } => (action.as_ref(), None),
        };
        let clock_anchor = Instant::now();
        if action.scope() != command.scope
            || action.argument_digest() != &command.subject.argument_digest
        {
            return Err(invalid());
        }
        let resource = match action.request().target {
            super::super::ActionTarget::Connection { resource, .. }
            | super::super::ActionTarget::Local { resource, .. } => resource,
        };
        let current = self
            .resources
            .inspect(
                command.scope.company,
                command.actor,
                RuntimeResourceId::new(resource),
            )
            .await?
            .ok_or_else(|| AppError::NotFound("workflow".into()))?;
        super::super::authorization::validate_resource(action.request(), &current)?;
        let Some(snapshot) = snapshot else {
            let ReconciliationPreparation::Recorded { result, .. } = prepared else {
                unreachable!()
            };
            return Ok(result);
        };
        snapshot.validate(command)?;
        if snapshot.revision != command.expected_revision {
            return Err(invalid());
        }
        let evidence = match &command.input {
            EvidenceInput::UnknownNote { .. } => VerifiedEvidence::unknown(command, snapshot)?,
            EvidenceInput::VerifiedReference {
                registration,
                reference,
            } => {
                let verifier = self.verifier.as_deref().ok_or_else(invalid)?;
                if verifier.registration().id() != registration
                    || !verifier.registration().matches(snapshot)?
                {
                    return Err(invalid());
                }
                let attestation =
                    Box::pin(verifier.verify(snapshot, reference, cancellation)).await?;
                // The trusted observation may include snapshot transaction/return
                // latency omitted by this monotonic anchor. Settlement independently
                // rejects either timestamp ahead of its actual DB clock.
                let elapsed =
                    chrono::Duration::from_std(clock_anchor.elapsed()).map_err(|_| invalid())?;
                let verified_at = (snapshot.database_now + elapsed).max(attestation.observed_at);
                VerifiedEvidence::issue(
                    command,
                    snapshot,
                    verifier.registration(),
                    attestation,
                    verified_at,
                )?
            }
        };
        self.persistence.settle(command, snapshot, evidence).await
    }
}
