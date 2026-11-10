//! Authenticated management of company-local profiles and associated sessions.
use super::{bounds::MAX_PROJECTS, ports::LocalAgentRead, protocol::ConnectionIssue};
use crate::application::transport::bounded::BoundedVec;
use crate::{
    app_error::AppResult,
    domain::local_agents::*,
    use_cases::company::{CompanyPersistence, managed_company},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

pub const SESSION_OPERATION_PAGE: usize = 8;

#[derive(Serialize)]
pub struct RecordPage<T> {
    pub entries: Vec<T>,
    pub truncated: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalOperationView {
    pub operation: LocalAgentOperation,
    pub origin: OperationOrigin,
    pub output: Option<LocalOutput>,
    pub output_truncated: bool,
    pub observed_turn_state: Option<TurnState>,
    pub measured_usage: Option<MeasuredUsage>,
}

#[derive(Serialize)]
pub struct SessionDetails {
    pub session: LocalAgentSession,
    pub recent: RecordPage<LocalOperationView>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSummary {
    pub id: SessionId,
    pub profile_id: ProfileId,
    pub thread_id: Option<CloudroomThreadId>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineRuntime {
    pub connected: bool,
    pub last_issue: Option<ConnectionIssue>,
    pub reconciled: bool,
    pub unsettled_effects: u32,
    pub outstanding_commands: u32,
    pub oldest_outstanding_at: Option<DateTime<Utc>>,
    pub permission_waits: u32,
    pub uncertain_outcomes: u32,
    pub reconciliation_gaps: u32,
    pub backpressured: bool,
    pub last_native_success_at: Option<DateTime<Utc>>,
    pub projects: BoundedVec<CloudroomProjectId, MAX_PROJECTS>,
}

/// Request identity is selected once by the human caller, independently of retries.
pub struct StartSession {
    pub actor: Uuid,
    pub company: Uuid,
    pub request_id: OperationId,
    pub profile_id: ProfileId,
    pub conversation_id: Uuid,
    pub prompt: LocalPrompt,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionCommand {
    Send { prompt: LocalPrompt },
    Inspect,
    Stop,
    Archive,
}
pub struct ControlSession {
    pub actor: Uuid,
    pub company: Uuid,
    pub request_id: OperationId,
    pub session_id: SessionId,
    pub command: SessionCommand,
}

#[async_trait]
pub trait LocalAgentManagementStore: LocalAgentRead {
    async fn list_profiles(
        &self,
        actor: Uuid,
        company: Uuid,
        after: Option<ProfileId>,
    ) -> AppResult<RecordPage<LocalAgentProfile>>;
    async fn delete_profile(&self, actor: Uuid, company: Uuid, profile: ProfileId)
    -> AppResult<()>;
    async fn start_session(&self, request: &StartSession) -> AppResult<LocalOperationView>;
    async fn control_session(&self, request: &ControlSession) -> AppResult<LocalOperationView>;
    async fn read_session(
        &self,
        actor: Uuid,
        company: Uuid,
        session: SessionId,
    ) -> AppResult<SessionDetails>;
    async fn list_sessions(
        &self,
        actor: Uuid,
        company: Uuid,
        association: &SessionAssociation,
    ) -> AppResult<RecordPage<SessionSummary>>;
    async fn machine_runtime(
        &self,
        actor: Uuid,
        company: Uuid,
        machine: MachineId,
    ) -> AppResult<MachineRuntime>;
    async fn read_operation(
        &self,
        actor: Uuid,
        company: Uuid,
        operation: OperationId,
    ) -> AppResult<LocalOperationView>;
    async fn refresh_catalog(
        &self,
        actor: Uuid,
        company: Uuid,
        machine: MachineId,
        request: OperationId,
    ) -> AppResult<LocalAgentOperation>;
}

pub struct LocalAgentManagement {
    companies: Arc<dyn CompanyPersistence>,
    store: Arc<dyn LocalAgentManagementStore>,
}
impl LocalAgentManagement {
    pub fn new(
        companies: Arc<dyn CompanyPersistence>,
        store: Arc<dyn LocalAgentManagementStore>,
    ) -> Self {
        Self { companies, store }
    }
    pub async fn profiles(
        &self,
        actor: Uuid,
        company: Uuid,
        after: Option<ProfileId>,
    ) -> AppResult<RecordPage<LocalAgentProfile>> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.list_profiles(actor, company, after).await
    }
    pub async fn profile(
        &self,
        actor: Uuid,
        company: Uuid,
        id: ProfileId,
    ) -> AppResult<Option<LocalAgentProfile>> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.profile(company, id).await
    }
    pub async fn delete_profile(&self, actor: Uuid, company: Uuid, id: ProfileId) -> AppResult<()> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.delete_profile(actor, company, id).await
    }
    pub async fn start(&self, request: StartSession) -> AppResult<LocalOperationView> {
        managed_company(self.companies.as_ref(), request.actor, request.company).await?;
        self.store.start_session(&request).await
    }
    pub async fn control(&self, request: ControlSession) -> AppResult<LocalOperationView> {
        managed_company(self.companies.as_ref(), request.actor, request.company).await?;
        self.store.control_session(&request).await
    }
    pub async fn session(
        &self,
        actor: Uuid,
        company: Uuid,
        id: SessionId,
    ) -> AppResult<SessionDetails> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.read_session(actor, company, id).await
    }
    pub async fn sessions(
        &self,
        actor: Uuid,
        company: Uuid,
        association: &SessionAssociation,
    ) -> AppResult<RecordPage<SessionSummary>> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.list_sessions(actor, company, association).await
    }
    pub async fn runtime(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
    ) -> AppResult<MachineRuntime> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.machine_runtime(actor, company, id).await
    }
    pub async fn operation(
        &self,
        actor: Uuid,
        company: Uuid,
        id: OperationId,
    ) -> AppResult<LocalOperationView> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.read_operation(actor, company, id).await
    }
    pub async fn test_connection(
        &self,
        actor: Uuid,
        company: Uuid,
        machine: MachineId,
        request: OperationId,
    ) -> AppResult<LocalAgentOperation> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store
            .refresh_catalog(actor, company, machine, request)
            .await
    }
}
