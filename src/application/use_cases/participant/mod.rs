//! Company-scoped actors, and the two ports the application needs to reach them.
//!
//! A `Principal` is the stable identity every authorization and participation decision names. A
//! `PrincipalIdentity` is one transport-qualified handle pointing at such a principal, so an
//! email mailbox and a Slack user id are two rows rather than two identity models.

#[cfg(test)]
pub mod test_support;

use crate::application::ports::participant::{IdentityObservation, PrincipalAccessPersistence};

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        participant::{
            IdentityClaimMetadata, IdentityProvenance, Principal, PrincipalAccessContext,
            PrincipalIdentity,
        },
        transport::QualifiedIdentity,
    },
};

#[derive(Debug, Clone)]
pub struct IdentityResolution {
    pub principal: Principal,
    pub identity: PrincipalIdentity,
}

/// Company-scoped qualified-identity resolution.
///
/// Every method here feeds either an authorization decision or a delivery address, so none of
/// them has a default implementation: an adapter or test double must state what it does rather
/// than silently answering "nobody".
#[async_trait]
pub trait IdentityDirectory: Send + Sync {
    /// Resolve the qualified identity to its principal, creating an external principal for a
    /// handle seen for the first time. Concurrent callers observing the same identity must end up
    /// with one identity row and one principal.
    async fn resolve_or_create_external_identity(
        &self,
        company_id: Uuid,
        observation: IdentityObservation,
    ) -> AppResult<IdentityResolution>;
}

pub trait ParticipantPersistence: IdentityDirectory + PrincipalAccessPersistence {}

impl<T> ParticipantPersistence for T where T: IdentityDirectory + PrincipalAccessPersistence {}

/// Resolve one email address to the company-scoped actor every later decision will name.
///
/// The sighting is recorded first so that ingress, channel policy and thread participation all
/// agree on the principal for a mailbox they may never have seen before. Observing an address
/// grants nothing; it only makes the actor addressable.
pub async fn observe_access_context(
    participants: &dyn ParticipantPersistence,
    company_id: Uuid,
    identity: &QualifiedIdentity,
    provenance: IdentityProvenance,
) -> AppResult<PrincipalAccessContext> {
    participants
        .resolve_or_create_external_identity(
            company_id,
            IdentityObservation {
                identity: identity.clone(),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance,
            },
        )
        .await?;
    participants
        .access_context_for_identity(company_id, identity)
        .await
}
