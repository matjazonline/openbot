//! A `401` on a namespace credential OpenBot holds as live re-verifies that credential (p06
//! gate A4-9).
//!
//! After a service restore, a credential issued after the backup does not exist there, and an
//! operator may have revoked one by hand. Either way the service answers `401` for it. Every
//! client call goes through [`ReverifyingClient`]. On a `401` it re-pends the namespace's
//! settled `present` lifecycle, and that step then checks the stored credential against the
//! service's live listing and issues a new one if it is missing (A4-8). The call itself still
//! fails: a delivery retries transiently, and recall and tools answer `memory.forbidden`.
//!
//! The re-pend is a compare-and-set on the stored credential id: concurrent `401`s for one
//! credential move the row once, and a row already re-issued, deleted or still running is left
//! alone. A search or pack across several namespaces does not say which credential was refused,
//! so each of them is re-verified; verifying a valid credential only lists it again.
use super::ports::*;
use crate::app_error::AppResult;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

#[async_trait]
pub trait CredentialReverification: Send + Sync {
    /// Re-pend the settled `present` lifecycle of the namespace that stores `credential`, if it
    /// still does. Returns whether a row moved.
    async fn credential_refused(
        &self,
        tenant: TenantExternalId,
        credential: Uuid,
    ) -> AppResult<bool>;
}

pub struct ReverifyingClient {
    inner: Arc<dyn MemoryClient>,
    store: Arc<dyn CredentialReverification>,
}

impl ReverifyingClient {
    pub fn new(inner: Arc<dyn MemoryClient>, store: Arc<dyn CredentialReverification>) -> Self {
        Self { inner, store }
    }

    async fn checked<T>(
        &self,
        credentials: &[&ClientCredential],
        answer: Result<T, MemoryServiceError>,
    ) -> Result<T, MemoryServiceError> {
        if matches!(answer, Err(MemoryServiceError::Unauthorized)) {
            for credential in credentials {
                match self
                    .store
                    .credential_refused(credential.tenant, credential.id)
                    .await
                {
                    Ok(moved) => tracing::warn!(
                        company_id = %credential.tenant.0,
                        credential_id = %credential.id,
                        reverifying = moved,
                        "The memory service refused a namespace credential; re-verifying it"
                    ),
                    // The call's own outcome stands; the next 401 tries again.
                    Err(error) => tracing::warn!(
                        %error,
                        "A refused memory credential could not be queued for re-verification"
                    ),
                }
            }
        }
        answer
    }
}

#[async_trait]
impl MemoryClient for ReverifyingClient {
    async fn save(
        &self,
        credential: &ClientCredential,
        request: &MemorySaveRequest,
    ) -> Result<MemorySaveReceipt, MemoryServiceError> {
        let answer = self.inner.save(credential, request).await;
        self.checked(&[credential], answer).await
    }

    async fn request_generation(
        &self,
        credential: &ClientCredential,
        request: &MemoryGenerationRequest,
    ) -> Result<MemoryGenerationReceipt, MemoryServiceError> {
        let answer = self.inner.request_generation(credential, request).await;
        self.checked(&[credential], answer).await
    }

    async fn search(
        &self,
        credential: &ClientCredential,
        search: &MemorySearch,
    ) -> Result<MemorySearchResult, MemoryServiceError> {
        let answer = self.inner.search(credential, search).await;
        self.checked(&[credential], answer).await
    }

    async fn search_scopes(
        &self,
        searches: &[ScopedSearch<'_>],
    ) -> Result<Vec<MemorySearchResult>, MemoryServiceError> {
        let answer = self.inner.search_scopes(searches).await;
        let credentials: Vec<_> = searches.iter().map(|search| search.credential).collect();
        self.checked(&credentials, answer).await
    }

    async fn context_pack(
        &self,
        packs: &[ScopedPack<'_>],
    ) -> Result<Vec<MemoryPack>, MemoryServiceError> {
        let answer = self.inner.context_pack(packs).await;
        let credentials: Vec<_> = packs.iter().map(|pack| pack.credential).collect();
        self.checked(&credentials, answer).await
    }

    async fn context_validate(
        &self,
        credential: &ClientCredential,
        namespace: NamespaceExternalId,
        read_ceiling: u8,
        items: &[PackedRevision],
    ) -> Result<Vec<PackItemStatus>, MemoryServiceError> {
        let answer = self
            .inner
            .context_validate(credential, namespace, read_ceiling, items)
            .await;
        self.checked(&[credential], answer).await
    }

    async fn timeline(
        &self,
        credential: &ClientCredential,
        timeline: &MemoryTimeline,
    ) -> Result<MemorySearchResult, MemoryServiceError> {
        let answer = self.inner.timeline(credential, timeline).await;
        self.checked(&[credential], answer).await
    }

    async fn fetch(
        &self,
        credential: &ClientCredential,
        namespace: NamespaceExternalId,
        read_ceiling: u8,
        ids: &[Uuid],
    ) -> Result<MemoryFetchResult, MemoryServiceError> {
        let answer = self
            .inner
            .fetch(credential, namespace, read_ceiling, ids)
            .await;
        self.checked(&[credential], answer).await
    }
}
