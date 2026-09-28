use super::*;
use async_trait::async_trait;

/// These records share the same owner as WorkflowDraftCopies and WorkflowBindings.
/// Phase03 implements durable transactions, not a second authoring/admission store.
#[async_trait]
pub trait WorkflowDefinitions: Send + Sync {
    async fn draft(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
    ) -> AppResult<Option<DraftState>>;
    async fn publication(
        &self,
        company: CompanyId,
        key: &IdempotencyKey,
    ) -> AppResult<Option<PublishedCommand>>;
    /// Only selectable (non-archived) versions. History/replay uses its own immutable records.
    async fn selectable_version(
        &self,
        company: CompanyId,
        version: VersionId,
    ) -> AppResult<Option<Arc<PublishedBundle>>>;
    /// Insert or CAS the whole draft/archive state and audit atomically. Check current
    /// actor management rights, scoped identity, exact expected revision and non-archive
    /// state under the same lock. Save cannot unarchive. Archive cannot delete history.
    async fn save_draft(&self, command: PreparedDraft) -> AppResult<()>;
    /// Recheck current management rights even on replay. In one transaction first
    /// resolve company/key: equivalent request returns the stored original bundle,
    /// otherwise Conflict. New publication checks live draft revision/non-archive,
    /// scoped unique version identity and captured dependencies' current authorization;
    /// persist source, bundle, key and audit together. Never overwrite published content.
    async fn publish(&self, command: PreparedPublication) -> AppResult<Arc<PublishedBundle>>;
}

/// Trusted dependency capture, shared by validation and publication. Inspect only
/// bounded declared dependencies and enforce current company access; snapshots are
/// data, never permission. Return no credentials. Infrastructure errors propagate.
#[async_trait]
pub trait PublicationDirectory: Send + Sync {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies>;
}

#[async_trait]
pub trait BindingLifecycle: Send + Sync {
    async fn binding(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<BindingState>>;
    /// One transaction: current company/association/resource authorization, insert
    /// or exact lifecycle CAS, immutable configuration history, selected version
    /// still selectable and (if active) current readiness. Retain fixed association.
    /// Compare-and-set must include activity changes, preventing activate/deactivate
    /// ABA races. Reconfiguration preserves activity only after readiness succeeds.
    /// Deactivation is permitted after archive or revocation to stop admission;
    /// it retains prior configuration and never mutates/cancels an admitted run.
    /// Admission's atomic owner locks/checks this same state and archival owner.
    /// Failure leaves all state/history/audit unchanged; no silent defaults.
    async fn save_binding(&self, command: PreparedBinding) -> AppResult<()>;
}
