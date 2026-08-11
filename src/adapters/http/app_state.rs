use std::sync::Arc;

use axum::extract::FromRef;

use crate::{
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases, approval::ApprovalUseCases, company::CompanyUseCases,
        company_invite::CompanyInviteUseCases, thread::ThreadUseCases, user::UserUseCases,
        workflow::WorkflowUseCases,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub user_use_cases: Arc<UserUseCases>,
    pub company_use_cases: Arc<CompanyUseCases>,
    pub company_invite_use_cases: Arc<CompanyInviteUseCases>,
    pub workflow_use_cases: Arc<WorkflowUseCases>,
    pub agent_use_cases: Arc<AgentUseCases>,
    pub thread_use_cases: Arc<ThreadUseCases>,
    pub approval_use_cases: Arc<ApprovalUseCases>,
}

impl FromRef<AppState> for Arc<AppConfig> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.config.clone()
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

impl FromRef<AppState> for Arc<WorkflowUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.workflow_use_cases.clone()
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

