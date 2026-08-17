use std::sync::Arc;

use axum::extract::FromRef;
use sqlx::PgPool;

use crate::{
    domain::monitoring::MonitoringService,
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases, approval::ApprovalUseCases, channel::ChannelUseCases,
        company::CompanyUseCases, company_invite::CompanyInviteUseCases, thread::ThreadUseCases,
        user::UserUseCases,
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
    pub agent_use_cases: Arc<AgentUseCases>,
    pub thread_use_cases: Arc<ThreadUseCases>,
    pub approval_use_cases: Arc<ApprovalUseCases>,
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
