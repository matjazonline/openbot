//! Shared principal-access facts and observed identity contract.
use crate::{
    app_error::AppResult,
    entities::{
        participant::{IdentityClaimMetadata, IdentityProvenance, PrincipalAccessContext},
        transport::QualifiedIdentity,
    },
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
/// One sighting of a transport handle. Observing confers no grant; it only fixes which principal
/// every later decision about that handle will name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityObservation {
    pub identity: QualifiedIdentity,
    pub display_label: Option<String>,
    pub claim_metadata: IdentityClaimMetadata,
    pub provenance: IdentityProvenance,
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
    /// no principal in it at all. An account whose current email is unverified retains its
    /// principal for attribution and membership, but has withheld grant authority so an email
    /// allowlist cannot authorize it.
    async fn access_context_for_user(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<Option<PrincipalAccessContext>>;
}
