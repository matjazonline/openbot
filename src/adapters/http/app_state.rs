use std::sync::Arc;

use axum::extract::FromRef;
use sqlx::PgPool;

use crate::{
    adapters::{persistence::dashboard::DashboardPersistence, storage::FileStorage},
    domain::monitoring::MonitoringService,
    infra::{config::AppConfig, events::MailboxEvents},
    use_cases::{
        agent::AgentUseCases, approval::ApprovalUseCases, channel::ChannelUseCases,
        company::CompanyUseCases, company_invite::CompanyInviteUseCases,
        schedule::ScheduleUseCases, thread::ThreadUseCases, user::UserUseCases,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<AppConfig>,
    pub monitoring: Arc<dyn MonitoringService>,
    pub user_use_cases: Arc<UserUseCases>,
    pub company_use_cases: Arc<CompanyUseCases>,
    pub company_invite_use_cases: Arc<CompanyInviteUseCases>,
    pub channel_use_cases: Arc<ChannelUseCases>,
    pub schedule_use_cases: Arc<ScheduleUseCases>,
    pub agent_use_cases: Arc<AgentUseCases>,
    pub thread_use_cases: Arc<ThreadUseCases>,
    pub approval_use_cases: Arc<ApprovalUseCases>,
    /// Read-only aggregates behind `/ui/dashboard`.
    pub dashboard_persistence: Arc<dyn DashboardPersistence>,
    /// Where a picked file is stored; `None` when no bucket is configured, which is what the
    /// avatar pickers report instead of failing at upload time.
    pub file_storage: Option<Arc<dyn FileStorage>>,
    /// Committed messages, for the mailbox's live message stream.
    pub events: MailboxEvents,
}

impl FromRef<AppState> for PgPool {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.db.clone()
    }
}

impl FromRef<AppState> for Arc<AppConfig> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.config.clone()
    }
}

impl FromRef<AppState> for Arc<dyn MonitoringService> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.monitoring.clone()
    }
}

impl FromRef<AppState> for Arc<UserUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.user_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<CompanyUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.company_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<CompanyInviteUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.company_invite_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<ChannelUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.channel_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<ScheduleUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.schedule_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<AgentUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.agent_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<ThreadUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.thread_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<ApprovalUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.approval_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<dyn DashboardPersistence> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.dashboard_persistence.clone()
    }
}

impl FromRef<AppState> for Option<Arc<dyn FileStorage>> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.file_storage.clone()
    }
}

impl FromRef<AppState> for MailboxEvents {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.events.clone()
    }
}
