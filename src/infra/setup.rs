use crate::{
    adapters::{
        http::{app_state::AppState, session::SessionAuthority},
        monitoring::{CompositeMonitor, InMemoryMonitor, TracingMonitor},
        protocols::email::{EmailRenderer, EmailSender, MailTransport, SmtpConfirmationSender},
        resend_api::{
            CompanyResendApiClients, ResendApiCompanyTransports, ResendApiInboundDecoder,
        },
        smtp::LettreMailTransport,
        storage::{FileStorage, gcs::GcsFileStorage},
    },
    domain::monitoring::MonitoringService,
    entities::runtime_metrics::MachineIdentity,
    infra::{
        argon2_password_hasher,
        config::{
            AppConfig, allow_unverified_email_without_smtp_from_env,
            smtp_allow_plaintext_local_from_env,
        },
        events::MailboxEvents,
        postgres_persistence,
    },
    services::{
        dashboard_snapshot::DashboardSnapshotService,
        database_query_health::DatabaseQueryHealthService,
        harness::TextClassifier,
        inbound_event_worker::{InboundEventWakeups, InboundEventWorker},
        notification_worker::NotificationWorker,
    },
    transport::{
        DeliveryComposer, InboundEventDecoderRegistry, MonitoredInboundEventInbox,
        TransportRegistry, TransportRenderers,
    },
    use_cases::{
        agent::AgentUseCases,
        builtin_agent_library::install_builtin_agent_library,
        channel::ChannelUseCases,
        company::CompanyUseCases,
        company_invite::CompanyInviteUseCases,
        company_resend_api::CompanyResendApiUseCases,
        schedule::ScheduleUseCases,
        skill::SkillUseCases,
        thread::{InboundIngestPorts, ThreadStores, ThreadUseCases},
        user::{EmailConfirmation, UserUseCases},
    },
};
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

mod accounts;
mod clients;
mod foundation;
mod inbound;
mod mail;
mod workers;
#[cfg(test)]
pub(crate) use accounts::user_use_cases;
pub use workers::ApplicationWorkers;

pub struct Application {
    pub http: AppState,
    pub workers: ApplicationWorkers,
}

pub async fn init_app_state() -> anyhow::Result<Application> {
    let foundation = foundation::build(Arc::new(AppConfig::from_env())).await?;
    let clients = clients::build(&foundation)?;
    let mail = mail::build(&foundation)?;
    let workflow = mail::workflow(&foundation, &clients, &mail)?;
    let threads = mail::threads(&foundation, &mail, &workflow);
    let transports = mail::transports(&foundation, &clients, &mail, threads.clone())?;
    let inbound = inbound::build(&foundation, &clients, threads.clone(), &workflow)?;
    let cases = accounts::build(&foundation, &workflow.services())?;
    let workers = workers::build(
        &foundation,
        workers::Components {
            workflow: workflow.clone(),
            threads: threads.clone(),
            composer: mail.composer,
            transports: transports.clone(),
            inbound: inbound.worker,
        },
    )?;
    let http = http_state(
        foundation,
        HttpComponents {
            clients,
            cases,
            threads,
            workflow: workflow.services(),
            transports,
            inbox: inbound.inbox,
            wakeups: inbound.wakeups,
        },
    )?;
    Ok(Application { http, workers })
}

struct HttpComponents {
    clients: clients::Clients,
    cases: accounts::UseCases,
    threads: Arc<ThreadUseCases>,
    workflow: Arc<crate::infra::workflow::WorkflowServices>,
    transports: Arc<TransportRegistry>,
    inbox: Arc<MonitoredInboundEventInbox>,
    wakeups: InboundEventWakeups,
}
fn http_state(f: foundation::Foundation, c: HttpComponents) -> anyhow::Result<AppState> {
    let p = f.persistence;
    let (integrations, integration_bindings) = super::integrations::gateway(&p, &f.config)?;
    Ok(AppState {
        integrations,
        integration_bindings,
        knowledge: f.knowledge,
        auth_admission: Arc::new(crate::adapters::http::auth_admission::AuthAdmission::from_env()?),
        workflow: c.workflow,
        model_usage: p.clone(),
        run_counts: p.clone(),
        thread_activity: p.clone(),
        message_outcomes: p.clone(),
        db: p.pool().clone(),
        config: f.config,
        monitoring: f.monitoring,
        user_use_cases: c.cases.user,
        mcp_use_cases: c.clients.mcp_use_cases,
        company_use_cases: c.cases.company,
        company_invite_use_cases: c.cases.invites,
        company_resend_api_use_cases: c.cases.resend,
        company_resend_api_accounts: p.clone(),
        channel_use_cases: c.cases.channel,
        schedule_use_cases: c.cases.schedules,
        agent_use_cases: c.cases.agent,
        onboarding_use_cases: c.cases.onboarding,
        skill_use_cases: c.cases.skill,
        thread_use_cases: c.threads,
        inbound_event_inbox: c.inbox,
        inbound_event_wakeups: c.wakeups,
        dashboard_snapshots: Arc::new(DashboardSnapshotService::new(p.clone())),
        attention_use_cases: c.cases.attention,
        notifications: p.clone(),
        database_query_health: Arc::new(DatabaseQueryHealthService::new(p.clone())),
        dashboard_sse_connections: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        runtime_metrics: p.clone(),
        runtime_identity: MachineIdentity::process().clone(),
        sessions: f.sessions,
        file_storage: f.storage,
        transports: c.transports,
        deliveries: p,
        events: MailboxEvents::new(),
    })
}

const DEFAULT_LOG_FILTER: &str =
    "open_bots=info,tower_http=info,monitoring::counter=info,monitoring::gauge=info";

pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| DEFAULT_LOG_FILTER.into());

    // Console (pretty logs)
    let console_layer = fmt::layer()
        .with_target(false) // don’t show target (module path)
        .with_level(true) // show log level
        .pretty(); // human-friendly, with colors

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .try_init()
        .ok();
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
