use std::sync::Arc;

use axum::extract::FromRef;
use sqlx::PgPool;

use crate::{
    adapters::{
        http::session::SessionAuthority, persistence::dashboard::DashboardPersistence,
        storage::FileStorage,
    },
    domain::monitoring::MonitoringService,
    entities::runtime_metrics::MachineIdentity,
    infra::{config::AppConfig, events::MailboxEvents},
    services::{
        database_query_health::DatabaseQueryHealthService,
        inbound_event_worker::{InboundEventWakeups, InboundEventWorker},
        memory_worker::MemoryWorker,
        runtime_metrics::{MemoryProviderActivity, RuntimeMetricPersistence},
    },
    transport::{DeliveryQueue, InboundEventInbox, TransportRegistry},
    use_cases::{
        agent::AgentUseCases, approval::ApprovalUseCases, channel::ChannelUseCases,
        company::CompanyUseCases, company_invite::CompanyInviteUseCases, delivery::DeliveryReader,
        memory::MemoryUseCases, schedule::ScheduleUseCases, thread::ThreadUseCases,
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
    pub schedule_use_cases: Arc<ScheduleUseCases>,
    pub agent_use_cases: Arc<AgentUseCases>,
    pub thread_use_cases: Arc<ThreadUseCases>,
    pub approval_use_cases: Arc<ApprovalUseCases>,
    pub memory_use_cases: Arc<MemoryUseCases>,
    pub memory_worker: Arc<MemoryWorker>,
    /// Owns claims on authenticated provider events. Routes only store and wake it.
    pub inbound_event_worker: Arc<InboundEventWorker>,
    /// Producer-only view used by future fast-ack webhook routes.
    pub inbound_event_inbox: Arc<dyn InboundEventInbox>,
    /// A latency hint after durable storage; polling remains authoritative.
    pub inbound_event_wakeups: InboundEventWakeups,
    /// Read-only aggregates behind `/ui/dashboard`.
    pub dashboard_persistence: Arc<dyn DashboardPersistence>,
    /// Shared across tabs so the operator query-statistics cache is process-wide.
    pub database_query_health: Arc<DatabaseQueryHealthService>,
    /// Current dashboard streams on this process; the stream guard updates it on disconnect too.
    pub dashboard_sse_connections: Arc<std::sync::atomic::AtomicU64>,
    /// Deployment-wide runtime history; handlers must authorize operators before reading it.
    pub runtime_metrics: Arc<dyn RuntimeMetricPersistence>,
    pub runtime_identity: MachineIdentity,
    /// Tally the memory provider writes into and the runtime sampler drains each ten seconds.
    pub memory_provider_activity: MemoryProviderActivity,
    /// Issues and verifies sessions; the only thing that decides who a request is.
    pub sessions: Arc<SessionAuthority>,
    /// Where a picked file is stored; `None` when no bucket is configured, which is what the
    /// avatar pickers report instead of failing at upload time.
    pub file_storage: Option<Arc<dyn FileStorage>>,
    /// Which transports this deployment can render for and send through.
    ///
    /// Carried on the state because `main` builds the delivery worker from it, and because a
    /// deployment's registered transports are what the integrations page reports as available.
    pub transports: Arc<TransportRegistry>,
    /// Reads the delivery queue for `/ui/deliveries` and the task board's delivery column.
    pub deliveries: Arc<dyn DeliveryReader>,
    /// Claims and fences the same queue. Not used by any handler -- `main` builds the delivery
    /// worker from it, the way it builds the memory worker from `memory_worker`.
    pub delivery_queue: Arc<dyn DeliveryQueue>,
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

impl FromRef<AppState> for Arc<MemoryUseCases> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.memory_use_cases.clone()
    }
}

impl FromRef<AppState> for Arc<dyn DashboardPersistence> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.dashboard_persistence.clone()
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
