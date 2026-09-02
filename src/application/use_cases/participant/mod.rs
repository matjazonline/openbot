//! Company-scoped actors, and the two ports the application needs to reach them.
//!
//! A `Principal` is the stable identity every authorization and participation decision names. A
//! `ParticipantIdentity` is one transport-qualified handle pointing at such a principal, so an
//! email mailbox and a Slack user id are two rows rather than two identity models.

#[cfg(test)]
pub mod test_support;

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    adapters::protocols::email::EmailIdentity,
    app_error::{AppError, AppResult},
    entities::{
        participant::{
            IdentityClaimMetadata, IdentityProvenance, ParticipantIdentity, Principal,
            PrincipalAccessContext,
        },
        transport::{PrincipalId, QualifiedIdentity, TransportKind},
        value_objects::EmailAddress,
    },
};

/// One sighting of a transport handle. Observing confers no grant; it only fixes which principal
/// every later decision about that handle will name.
#[derive(Debug, Clone)]
pub struct IdentityObservation {
    pub identity: QualifiedIdentity,
    pub display_label: Option<String>,
    pub claim_metadata: IdentityClaimMetadata,
    pub provenance: IdentityProvenance,
}

#[derive(Debug, Clone)]
pub struct IdentityResolution {
    pub principal: Principal,
    pub identity: ParticipantIdentity,
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

    /// The usable handles these principals can be reached on over one transport, oldest first.
    async fn identities_for_principals(
        &self,
        company_id: Uuid,
        principal_ids: &[PrincipalId],
        transport: TransportKind,
    ) -> AppResult<Vec<ParticipantIdentity>>;
}

/// The actor facts channel policy needs, looked up by whichever handle the caller arrived with.
#[async_trait]
pub trait PrincipalAccessPersistence: Send + Sync {
    /// For an inbound message: who this transport handle is to the company.
    async fn access_context_for_identity(
        &self,
        company_id: Uuid,
        identity: &QualifiedIdentity,
    ) -> AppResult<PrincipalAccessContext>;

    /// For a signed-in session: who this account is to the company. `None` means the account has
    /// no principal in it at all.
    async fn access_context_for_user(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<Option<PrincipalAccessContext>>;
}

pub trait ParticipantPersistence: IdentityDirectory + PrincipalAccessPersistence {}

impl<T> ParticipantPersistence for T where T: IdentityDirectory + PrincipalAccessPersistence {}

/// Resolve one email address to the company-scoped actor every later decision will name.
///
/// The sighting is recorded first so that ingress, channel policy and thread participation all
/// agree on the principal for a mailbox they may never have seen before. Observing an address
/// grants nothing; it only makes the actor addressable.
pub async fn observe_email_access_context(
    participants: &dyn ParticipantPersistence,
    company_id: Uuid,
    address: &str,
) -> AppResult<PrincipalAccessContext> {
    let identity = EmailIdentity::parse(EmailAddress::from(address))
        .map(EmailIdentity::qualify_default)
        .map_err(|error| AppError::BadRequest(format!("Invalid email identity: {error}")))?;
    participants
        .resolve_or_create_external_identity(
            company_id,
            IdentityObservation {
                identity: identity.clone(),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::EmailIngress,
            },
        )
        .await?;
    participants
        .access_context_for_identity(company_id, &identity)
        .await
}
