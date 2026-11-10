//! Transport-level test double: one fixed client credential, every other store call
//! unavailable. Storage semantics are tested against PostgreSQL, never against this.
use super::{
    contract::*,
    ports::{
        Actor, AdminCredentialRecord, Aged, ClientCredentialRecord, ClientRead, ClientWrite,
        CredentialPage, Dispatch, GenerationJob, GenerationStore, IndexCommit, IndexFailure,
        IndexJob, IndexedPart, MemoryStore, NewCredential, PackCandidates, Reaped, RecallStore,
        Removed, SearchQuery, SearchStore, Settled, Settlement, Store, Swept, TenantAction,
        TimelineQuery,
    },
};
use crate::domain::{
    access_level::AccessLevel,
    admin_policy::AdminScope,
    credential::{self, TokenKind},
    error::ServiceError,
    grant::{NamespaceGrant, Operation},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub struct StaticStore {
    pub client: Option<ClientCredentialRecord>,
    pub available: bool,
    /// Delay every credential lookup, to exercise request deadlines.
    pub delay: std::time::Duration,
}

/// A store holding one client credential for tenant `tenant-a`, namespace `ns-a`.
pub fn static_store() -> (StaticStore, String) {
    let issued = credential::issue(TokenKind::Client).unwrap();
    let record = ClientCredentialRecord {
        id: issued.id,
        tenant_id: Uuid::now_v7(),
        tenant: "tenant-a".into(),
        tenant_state: TenantState::Active,
        label: "contract".into(),
        verifier: issued.verifier.to_vec(),
        expires_at: None,
        revoked_at: None,
        grants: vec![NamespaceGrant {
            namespace_id: Uuid::now_v7(),
            namespace: "ns-a".into(),
            read_max: Some(AccessLevel::new(6).unwrap()),
            write_max: Some(AccessLevel::new(6).unwrap()),
            operations: Operation::ALL.into_iter().collect(),
        }],
    };
    (
        StaticStore {
            client: Some(record),
            available: true,
            delay: std::time::Duration::ZERO,
        },
        issued.token,
    )
}

fn down() -> ServiceError {
    ServiceError::unavailable("test store")
}

#[async_trait]
impl MemoryStore for StaticStore {
    async fn record_sources(
        &self,
        _: &ClientWrite,
        _: &SourcesRecordRequest,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(down())
    }
    async fn open_session(
        &self,
        _: &ClientWrite,
        _: &SessionOpenRequest,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(down())
    }
    async fn close_session(
        &self,
        _: &ClientWrite,
        _: &SessionCloseRequest,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(down())
    }
    async fn save_observations(
        &self,
        _: &ClientWrite,
        _: &SaveRequest,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(down())
    }
    async fn correct_observation(
        &self,
        _: &ClientWrite,
        _: &CorrectRequest,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(down())
    }
    async fn delete_observation(
        &self,
        _: &ClientWrite,
        _: &DeleteRequest,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(down())
    }
    async fn purge_namespace(
        &self,
        _: &ClientWrite,
        _: &PurgeRequest,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(down())
    }
    async fn purge_replay(
        &self,
        _: Uuid,
        _: Uuid,
        _: &str,
        _: [u8; 32],
    ) -> Result<Option<serde_json::Value>, ServiceError> {
        Err(down())
    }
    async fn fetch_observations(
        &self,
        _: &ClientRead,
        _: &[Uuid],
    ) -> Result<FetchResponse, ServiceError> {
        Err(down())
    }
}

#[async_trait]
impl RecallStore for StaticStore {
    async fn timeline(
        &self,
        _: &ClientRead,
        _: TimelineQuery,
    ) -> Result<TimelineResponse, ServiceError> {
        Err(down())
    }
    async fn pack_candidates(
        &self,
        _: &ClientRead,
        _: Option<&SearchQuery>,
        _: PackDetail,
    ) -> Result<PackCandidates, ServiceError> {
        Err(down())
    }
    async fn pack_bodies(
        &self,
        _: &ClientRead,
        _: &[(Uuid, i64)],
    ) -> Result<std::collections::HashMap<Uuid, String>, ServiceError> {
        Err(down())
    }
    async fn validate_pack(
        &self,
        _: &ClientRead,
        _: &[PackedRevision],
    ) -> Result<ContextValidateResponse, ServiceError> {
        Err(down())
    }
}

#[async_trait]
impl SearchStore for StaticStore {
    async fn search_observations(
        &self,
        _: &ClientRead,
        _: &SearchQuery,
    ) -> Result<SearchResponse, ServiceError> {
        Err(down())
    }
    async fn claim_index_jobs(
        &self,
        _: usize,
        _: std::time::Duration,
    ) -> Result<Vec<IndexJob>, ServiceError> {
        Ok(Vec::new())
    }
    async fn renew_index_leases(
        &self,
        _: &[IndexJob],
        _: std::time::Duration,
    ) -> Result<Vec<Uuid>, ServiceError> {
        Err(down())
    }
    async fn commit_index(
        &self,
        _: &IndexJob,
        _: &str,
        _: &[IndexedPart],
    ) -> Result<IndexCommit, ServiceError> {
        Err(down())
    }
    async fn fail_index(&self, _: &IndexJob, _: IndexFailure) -> Result<bool, ServiceError> {
        Err(down())
    }
    async fn reindex_failed(&self) -> Result<u64, ServiceError> {
        Err(down())
    }
    async fn adopt_embedding_model(&self, _: &str) -> Result<(), ServiceError> {
        Ok(())
    }
    async fn vector_capability(&self) -> Result<(), ServiceError> {
        Ok(())
    }
}

#[async_trait]
impl GenerationStore for StaticStore {
    async fn accept_generation(
        &self,
        _: &ClientWrite,
        _: &super::generation::PreparedGeneration,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(down())
    }
    async fn generation_status(
        &self,
        _: &ClientRead,
        _: Uuid,
    ) -> Result<GenerationStatus, ServiceError> {
        Err(down())
    }
    async fn cancel_generation(
        &self,
        _: &ClientWrite,
        _: Uuid,
    ) -> Result<serde_json::Value, ServiceError> {
        Err(down())
    }
    async fn claim_generation(
        &self,
        _: Uuid,
        _: std::time::Duration,
    ) -> Result<Option<GenerationJob>, ServiceError> {
        Err(down())
    }
    async fn dispatch_generation(
        &self,
        _: &GenerationJob,
        _: std::time::Duration,
    ) -> Result<Dispatch, ServiceError> {
        Err(down())
    }
    async fn renew_generation(
        &self,
        _: &GenerationJob,
        _: std::time::Duration,
    ) -> Result<bool, ServiceError> {
        Err(down())
    }
    async fn settle_generation(
        &self,
        _: &GenerationJob,
        _: &Settlement,
    ) -> Result<Settled, ServiceError> {
        Err(down())
    }
    async fn reap_generations(
        &self,
        _: std::time::Duration,
        _: i64,
    ) -> Result<Reaped, ServiceError> {
        Err(down())
    }
    async fn sweep_retention(&self, _: i64) -> Result<Swept, ServiceError> {
        Err(down())
    }
    async fn remove_deleted(&self, _: i64) -> Result<Removed, ServiceError> {
        Err(down())
    }
    async fn sweep_aged(&self, _: i64) -> Result<Aged, ServiceError> {
        Err(down())
    }
    async fn after_restore(&self) -> Result<u64, ServiceError> {
        Err(down())
    }
}

#[async_trait]
impl Store for StaticStore {
    async fn ping(&self) -> Result<(), ServiceError> {
        if self.available { Ok(()) } else { Err(down()) }
    }
    async fn metrics_gauges(&self) -> Result<super::metrics::Gauges, ServiceError> {
        Err(down())
    }
    async fn admin_credential(
        &self,
        _: Uuid,
    ) -> Result<Option<AdminCredentialRecord>, ServiceError> {
        Ok(None)
    }
    async fn client_credential(
        &self,
        id: Uuid,
    ) -> Result<Option<ClientCredentialRecord>, ServiceError> {
        if !self.available {
            return Err(down());
        }
        tokio::time::sleep(self.delay).await;
        Ok(self.client.clone().filter(|record| record.id == id))
    }
    async fn bootstrap_operator(&self, _: Uuid, _: &str, _: [u8; 32]) -> Result<(), ServiceError> {
        Err(down())
    }
    async fn issue_admin_credential(
        &self,
        _: Actor,
        _: Uuid,
        _: AdminScope,
        _: &str,
        _: Option<DateTime<Utc>>,
        _: [u8; 32],
    ) -> Result<(), ServiceError> {
        Err(down())
    }
    async fn revoke_admin_credential(
        &self,
        _: Actor,
        _: Uuid,
    ) -> Result<AdminCredentialView, ServiceError> {
        Err(down())
    }
    async fn create_tenant(&self, _: Actor, _: &str) -> Result<TenantView, ServiceError> {
        Err(down())
    }
    async fn change_tenant(
        &self,
        _: Actor,
        _: &str,
        _: TenantAction,
    ) -> Result<TenantView, ServiceError> {
        Err(down())
    }
    async fn set_tenant_cap(&self, _: Actor, _: &str, _: u64) -> Result<TenantView, ServiceError> {
        Err(down())
    }
    async fn create_namespace(
        &self,
        _: Actor,
        _: &str,
        _: &str,
    ) -> Result<NamespaceView, ServiceError> {
        Err(down())
    }
    async fn delete_namespace(
        &self,
        _: Actor,
        _: &str,
        _: &str,
    ) -> Result<NamespaceView, ServiceError> {
        Err(down())
    }
    async fn issue_credential(
        &self,
        _: Actor,
        _: &str,
        _: NewCredential,
    ) -> Result<(), ServiceError> {
        Err(down())
    }
    async fn list_credentials(
        &self,
        _: Actor,
        _: &str,
        _: CredentialPage<'_>,
    ) -> Result<CredentialList, ServiceError> {
        Err(down())
    }
    async fn reassign_tenants(
        &self,
        _: Actor,
        _: Uuid,
        _: Uuid,
        _: u32,
    ) -> Result<TenantReassignResponse, ServiceError> {
        Err(down())
    }
    async fn revoke_credential(
        &self,
        _: Actor,
        _: &str,
        _: Uuid,
    ) -> Result<CredentialView, ServiceError> {
        Err(down())
    }
}
