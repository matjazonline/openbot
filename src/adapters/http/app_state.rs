use std::sync::Arc;

use axum::extract::FromRef;
use sqlx::PgPool;

use crate::{
    adapters::{
        http::{auth_admission::AuthAdmission, session::SessionAuthority},
        storage::FileStorage,
    },
    application::{attention::AttentionUseCases, notification::NotificationPersistence},
    domain::monitoring::MonitoringService,
    entities::runtime_metrics::MachineIdentity,
    infra::{config::AppConfig, events::MailboxEvents},
    services::{
        dashboard_snapshot::DashboardSnapshotService,
        database_query_health::DatabaseQueryHealthService,
        inbound_event_worker::InboundEventWakeups, runtime_metrics::RuntimeMetricPersistence,
    },
    transport::{InboundEventInbox, TransportRegistry},
    use_cases::{
        agent::AgentUseCases,
        channel::ChannelUseCases,
        company::CompanyUseCases,
        company_invite::CompanyInviteUseCases,
        company_resend_api::{CompanyResendApiAccounts, CompanyResendApiUseCases},
        delivery::DeliveryReader,
        schedule::ScheduleUseCases,
        skill::SkillUseCases,
        thread::ThreadUseCases,
        user::UserUseCases,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub knowledge: Arc<crate::application::knowledge::KnowledgeLibrary>,
    pub auth_admission: Arc<AuthAdmission>,
    /// Workflow commands and read models, without worker handlers or spawn capability.
    pub workflow: Arc<crate::infra::workflow::WorkflowServices>,
    pub model_usage: Arc<dyn crate::services::model_usage::ModelUsageStore>,
    pub run_counts: Arc<dyn crate::application::run_counts::RunCountsReader>,
    pub thread_activity: Arc<dyn crate::application::thread_activity::ThreadActivityReader>,
    pub message_outcomes: Arc<dyn crate::application::message_outcomes::MessageOutcomeReader>,
    pub db: PgPool,
    pub config: Arc<AppConfig>,
    pub monitoring: Arc<dyn MonitoringService>,
    pub user_use_cases: Arc<UserUseCases>,
    pub mcp_use_cases: Arc<crate::use_cases::mcp::McpUseCases>,
    pub company_use_cases: Arc<CompanyUseCases>,
    pub company_invite_use_cases: Arc<CompanyInviteUseCases>,
    /// One company's Resend account, as its owner configures it.
    pub company_resend_api_use_cases: Arc<CompanyResendApiUseCases>,
    /// The same integrations as the runtime reads them: credentials by company and by webhook
    /// token, with no way back to a write.
    pub company_resend_api_accounts: Arc<dyn CompanyResendApiAccounts>,
    pub channel_use_cases: Arc<ChannelUseCases>,
    pub schedule_use_cases: Arc<ScheduleUseCases>,
    pub agent_use_cases: Arc<AgentUseCases>,
    pub onboarding_use_cases: Arc<crate::use_cases::onboarding::OnboardingUseCases>,
    pub skill_use_cases: Arc<SkillUseCases>,
    pub thread_use_cases: Arc<ThreadUseCases>,
    /// Producer-only view used by future fast-ack webhook routes.
    pub inbound_event_inbox: Arc<dyn InboundEventInbox>,
    /// A latency hint after durable storage; polling remains authoritative.
    pub inbound_event_wakeups: InboundEventWakeups,
    /// Read-only aggregates behind `/ui/dashboard`, one reading per view per tick for every tab.
    /// Handlers reach the aggregates only through this, so none can bypass the cache.
    pub dashboard_snapshots: Arc<DashboardSnapshotService>,
    /// Derived human work and durable business summaries; source commands remain authoritative.
    pub attention_use_cases: Arc<AttentionUseCases>,
    /// Recipient-facing projection over the same operational sources as `attention`.
    pub notifications: Arc<dyn NotificationPersistence>,
    /// Shared across tabs so the operator query-statistics cache is process-wide.
    pub database_query_health: Arc<DatabaseQueryHealthService>,
    /// Current dashboard streams on this process; the stream guard updates it on disconnect too.
    pub dashboard_sse_connections: Arc<std::sync::atomic::AtomicU64>,
    /// Deployment-wide runtime history; handlers must authorize operators before reading it.
    pub runtime_metrics: Arc<dyn RuntimeMetricPersistence>,
    pub runtime_identity: MachineIdentity,
    /// Issues and verifies sessions; the only thing that decides who a request is.
    pub sessions: Arc<SessionAuthority>,
    /// Where a picked file is stored; `None` when no bucket is configured, which is what the
    /// avatar pickers report instead of failing at upload time.
    pub file_storage: Option<Arc<dyn FileStorage>>,
    /// Which transports this deployment can render for and send through.
    ///
    /// The integrations page reports which transports this deployment has registered.
    pub transports: Arc<TransportRegistry>,
    /// Reads the delivery queue for `/ui/deliveries`.
    pub deliveries: Arc<dyn DeliveryReader>,
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

impl FromRef<AppState> for Arc<crate::infra::workflow::WorkflowServices> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.workflow.clone()
    }
}

/// "Create assistant channel" (p08 D2.4), over the workflow engine's provisioning.
impl FromRef<AppState> for Arc<dyn crate::application::workflow::provisioning::AssistantChannels> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.workflow.assistant_channels()
    }
}

impl FromRef<AppState> for Arc<dyn MonitoringService> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.monitoring.clone()
    }
}

impl FromRef<AppState> for Arc<dyn InboundEventInbox> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.inbound_event_inbox.clone()
    }
}

impl FromRef<AppState> for InboundEventWakeups {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.inbound_event_wakeups.clone()
    }
}

impl FromRef<AppState> for Arc<AttentionUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.attention_use_cases.clone()
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

impl FromRef<AppState> for Arc<SkillUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.skill_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<ThreadUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.thread_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<dyn crate::application::thread_activity::ThreadActivityReader> {
    fn from_ref(state: &AppState) -> Self {
        state.thread_activity.clone()
    }
}

impl FromRef<AppState> for Arc<dyn crate::application::message_outcomes::MessageOutcomeReader> {
    fn from_ref(state: &AppState) -> Self {
        state.message_outcomes.clone()
    }
}

impl FromRef<AppState> for Arc<dyn NotificationPersistence> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.notifications.clone()
    }
}

impl FromRef<AppState> for Arc<SessionAuthority> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.sessions.clone()
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

impl axum::extract::FromRef<AppState> for Arc<crate::use_cases::mcp::McpUseCases> {
    fn from_ref(state: &AppState) -> Self {
        state.mcp_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<dyn crate::services::model_usage::ModelUsageStore> {
    fn from_ref(state: &AppState) -> Self {
        state.model_usage.clone()
    }
}

impl FromRef<AppState> for Arc<crate::use_cases::onboarding::OnboardingUseCases> {
    fn from_ref(state: &AppState) -> Self {
        state.onboarding_use_cases.clone()
    }
}
