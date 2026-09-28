use super::{
    AdmissionResult, CancelCommand, CancelResult, ClaimRequest, ClaimedExecution, CompanyId,
    IdempotencyKey, PreparedAdmission, RunHead, binding::ConfiguredBinding,
};
use crate::application::app_error::AppResult;
use crate::domain::workflow::{RunId, TriggerRef, WorkflowBindingId};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
/// Selects the original immutable configuration for a matching company/key/binding
/// replay or matching company/binding/canonical source event, otherwise the
/// currently active binding with a selectable version. Source lookup is required
/// even when a redelivery uses a different command key or trigger UUID. Manual
/// events use their stable trigger UUID; messages use their canonical message ID,
/// schedules their occurrence, and child events their immediate execution/action.
/// Never substitute current configuration for a saved run, even after archive or
/// deactivation. None means absent/unselectable; errors propagate. The returned
/// company and logical binding must match. This read is not an activation grant:
/// WorkflowAdmission must close races with selection changes before new-run commit.
pub trait WorkflowBindings: Send + Sync {
    async fn admission_binding(
        &self,
        company_id: CompanyId,
        binding_id: WorkflowBindingId,
        key: &IdempotencyKey,
        trigger: &TriggerRef,
    ) -> AppResult<Option<Arc<ConfiguredBinding>>>;
}

#[async_trait]
/// One atomic owner for company-scoped dedup, immutable snapshots, the run,
/// first execution and first scheduling job. Exact replay returns the original
/// run even when binding revisions or proposed random IDs change; changed logical binding or
/// input, related association, or stable trigger/source under the same
/// company/key return `Conflict` without new writes. Correlation and proposed
/// random IDs are excluded from equivalence. This is a trusted internal write port;
/// the service reauthorizes the authenticated actor before every admission
/// or replay. Before committing or replaying, the adapter must validate the
/// company-scoped source against authoritative stored message/schedule occurrence/
/// parent execution/action rows, including message association and action to
/// execution to run linkage. Missing, foreign, or inconsistent records fail
/// without writes; reader errors propagate. Causal references and correlation
/// never grant access, deduplicate on their own, or fence a write. Later storage
/// commits must close authority revocation races.
/// For a new run, atomically verify that the prepared binding revision/bundle is
/// still selected and active and its version selectable; a stale selection must
/// fail without writes (Conflict error, safe for the caller to reload/retry).
/// Existing admissions are resolved first and retain their original configuration;
/// configuration revisions and parameters are not part of replay equivalence.
/// A new command key for the same canonical source and logical binding aliases
/// the original run after checking input/association equivalence and current
/// authority. Persist the alias's command equivalence atomically; never create a
/// second run or select current configuration for an already admitted source.
/// Persist the exact selected revision, resource references and complete bundle
/// with the run, never reconstruct them from a later mutable binding head.
/// New runs begin `Queued`; replay must preserve their authoritative state.
pub trait WorkflowAdmission: Send + Sync {
    /// Dedup, snapshots, run, first execution, and first job share one atomic commit.
    async fn admit(&self, command: &PreparedAdmission) -> AppResult<AdmissionResult>;
}

#[async_trait]
/// Compare-and-set cancellation atomically invalidates pending work and live
/// ownership. Committed outputs and effect receipts remain intact. Cancellation
/// does not promise rollback of an external effect already made. The port must
/// compare the expected revision against current authoritative state, retain
/// every terminal state unchanged, and preserve committed outputs and receipts.
/// This trusted internal port receives a command only after the service checks
/// current management access and the run's stored related association.
pub trait WorkflowRunTransitions: Send + Sync {
    /// CAS cancellation also invalidates pending jobs and ownership atomically.
    async fn cancel(&self, command: CancelCommand) -> AppResult<CancelResult>;
}

#[async_trait]
/// Claims at most the checked batch size with a checked positive lease,
/// excluding waiting and terminal runs as well as executions with live owners.
/// The first queued claim atomically makes its run `Running` and advances its
/// revision; later eligible running claims need not advance the state revision.
/// Each claim persists a fresh fence, expiry and receipt. Later commits require that fence and live expiry;
/// expiry/recovery accounting is introduced in phase 03.
pub trait WorkflowExecutionScheduling: Send + Sync {
    /// Exclusive claim returns a new fence and ownership receipt for later commits.
    async fn claim_ready(&self, request: ClaimRequest) -> AppResult<Vec<ClaimedExecution>>;
}

#[async_trait]
/// Reads a company-scoped authoritative run head including current state and
/// stored related association and original immutable causality for the independent visibility check.
/// `None` means absent, while
/// `Err` preserves infrastructure failure; scope is not authorization proof.
pub trait WorkflowInspection: Send + Sync {
    async fn head(&self, company_id: CompanyId, run_id: RunId) -> AppResult<Option<RunHead>>;
}
