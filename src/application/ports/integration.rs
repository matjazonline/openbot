//! Channel-binding persistence and its audited write contract.
use crate::{
    app_error::{AppError, AppResult},
    entities::{
        creation::CreationProvenance,
        transport::{
            BindingAccessPolicy, BindingAccessSnapshot, BindingAuditAction, BindingAuditEvent,
            BindingChangeReason, BindingDeliveryPolicy, BindingStatus, ChannelBinding,
            ChannelBindingId, EndpointNamespace, ExternalEndpointKey, InstallationId,
            TransportKind,
        },
    },
};
use async_trait::async_trait;
use uuid::Uuid;
/// The bound on one manager-facing binding listing. Every caller-supplied page size is clamped to
/// it, so an operational view cannot ask the database for an unbounded scan.
pub const MAX_BINDING_AUDIT_EVENTS: i64 = 200;

/// Everything creating one protocol interface onto a channel sets.
#[derive(Debug, Clone)]
pub struct BindingWrite {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub installation_id: Option<InstallationId>,
    pub transport: TransportKind,
    pub namespace: EndpointNamespace,
    pub external_endpoint_key: ExternalEndpointKey,
    pub display_label: String,
    pub access_policy: BindingAccessPolicy,
    pub delivery_policy: BindingDeliveryPolicy,
    pub access_snapshot: BindingAccessSnapshot,
    pub created_by: CreationProvenance,
}

impl BindingWrite {
    /// The coherence rule the database also states as a `CHECK`, checked here so the caller gets a
    /// sentence instead of a constraint name.
    pub fn validate(&self) -> AppResult<()> {
        if self.transport.requires_installation() != self.installation_id.is_some() {
            return Err(AppError::BadRequest(format!(
                "A {} binding {} name an installation.",
                self.transport,
                if self.transport.requires_installation() {
                    "must"
                } else {
                    "cannot"
                }
            )));
        }
        Ok(())
    }
}

/// One audited binding lifecycle transition.
#[derive(Debug, Clone)]
pub struct BindingStatusChange {
    pub company_id: Uuid,
    pub binding_id: ChannelBindingId,
    pub status: BindingStatus,
    pub reason: Option<BindingChangeReason>,
    pub actor: CreationProvenance,
}

impl BindingStatusChange {
    /// The audit verb this transition is recorded under. One mapping, so the audit log cannot
    /// disagree with the status column it describes.
    pub const fn audit_action(&self) -> BindingAuditAction {
        match self.status {
            BindingStatus::Active => BindingAuditAction::Enabled,
            BindingStatus::Paused => BindingAuditAction::Paused,
            BindingStatus::Disabled => BindingAuditAction::Disabled,
            BindingStatus::Orphaned => BindingAuditAction::DriftDetected,
        }
    }

    /// A binding that stops carrying traffic says why. The database enforces the same pairing;
    /// this is where the caller finds out before the write.
    pub fn validate(&self) -> AppResult<()> {
        let needs_reason = !self.status.holds_endpoint_claim();
        if needs_reason != self.reason.is_some() {
            return Err(AppError::BadRequest(format!(
                "A binding moving to '{}' {} a reason.",
                self.status,
                if needs_reason { "requires" } else { "takes no" }
            )));
        }
        Ok(())
    }
}

/// The exact endpoint an inbound provider event arrived on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundEndpoint {
    pub transport: TransportKind,
    pub installation_id: Option<InstallationId>,
    pub namespace: EndpointNamespace,
    pub external_endpoint_key: ExternalEndpointKey,
}

/// Channel bindings and their append-only history.
///
/// Every method takes the company explicitly. That is not redundancy with the binding id: an id
/// arrives from a URL or a provider payload, and a lookup that trusts it alone is a tenancy bug
/// waiting for someone to guess a UUID.
#[async_trait]
pub trait ChannelBindingPersistence: Send + Sync {
    /// Create the binding and its `linked` audit record in one transaction.
    async fn create_binding(&self, write: BindingWrite) -> AppResult<ChannelBinding>;

    /// The interfaces this channel is currently carrying traffic on. Bindings whose installation
    /// is no longer usable are excluded here rather than being rewritten row by row.
    async fn active_bindings_for_channel(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelBinding>>;

    /// The one active binding that owns this endpoint, for routing an inbound event.
    async fn find_active_binding_by_endpoint(
        &self,
        endpoint: &InboundEndpoint,
    ) -> AppResult<Option<ChannelBinding>>;

    /// Every binding a manager may see, whatever its status.
    async fn list_bindings_for_company(&self, company_id: Uuid) -> AppResult<Vec<ChannelBinding>>;

    async fn get_binding(
        &self,
        company_id: Uuid,
        binding_id: ChannelBindingId,
    ) -> AppResult<Option<ChannelBinding>>;

    /// Apply the transition and append its audit record in one transaction.
    async fn set_binding_status(&self, change: BindingStatusChange) -> AppResult<ChannelBinding>;

    /// Newest first, bounded by [`MAX_BINDING_AUDIT_EVENTS`].
    async fn list_binding_audit_events(
        &self,
        company_id: Uuid,
        binding_id: ChannelBindingId,
        limit: i64,
    ) -> AppResult<Vec<BindingAuditEvent>>;
}
