use crate::{
    adapters::{
        http::{app_state::AppState, session::SessionAuthority},
        memory::hydradb::HydraDbProvider,
        monitoring::{CompositeMonitor, InMemoryMonitor, TracingMonitor},
        protocols::{EgressRegistry, email::EmailEgressAdapter},
        smtp::LettreMailTransport,
        storage::{FileStorage, gcs::GcsFileStorage},
    },
    domain::monitoring::MonitoringService,
    entities::runtime_metrics::MachineIdentity,
    infra::{
        argon2_password_hasher,
        config::{
            AppConfig, agent_run_timeout_from_env, smtp_allow_plaintext_local_from_env,
        },
        events::MailboxEvents,
        postgres_persistence,
    },
    services::{
        memory_coordinator::MemoryCoordinator,
        memory_provider::MemoryProviderRegistry,
        memory_worker::MemoryWorker,
        outbound_dispatcher::{MailTransport, OutboundDispatcher, SmtpConfirmationSender},
        runtime_metrics::HydraDbActivity,
    },
    use_cases::{
        agent::AgentUseCases,
        approval::ApprovalUseCases,
        channel::ChannelUseCases,
        company::CompanyUseCases,
        company_invite::CompanyInviteUseCases,
        memory::MemoryUseCases,
        schedule::ScheduleUseCases,
        thread::ThreadUseCases,
        user::{EmailConfirmation, UserUseCases},
    },
};
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub async fn init_app_state() -> anyhow::Result<AppState> {
    let config = Arc::new(AppConfig::from_env());
    let agent_run_timeout = agent_run_timeout_from_env();
    let mail_transport: Arc<dyn MailTransport> =
        LettreMailTransport::from_config(&config, smtp_allow_plaintext_local_from_env()).await?;
    let mail_dispatcher = Arc::new(OutboundDispatcher::new(
        config.clone(),
        mail_transport.clone(),
    ));

    let sessions = Arc::new(SessionAuthority::new(&config));

    let monitoring: Arc<dyn MonitoringService> = Arc::new(CompositeMonitor::new(vec![
        Arc::new(TracingMonitor::new()),
        Arc::new(InMemoryMonitor::new()),
    ]));

    // Built before anything else that could fail slowly: a bucket named in the environment but
    // unreachable through its key should stop the boot, not the first person to pick a picture.
    let file_storage = match config.gcs.as_ref() {
        Some(gcs) => Some(Arc::new(GcsFileStorage::from_config(gcs)?) as Arc<dyn FileStorage>),
        None => None,
    };

    let postgres_arc = Arc::new(postgres_persistence().await?);
    let hydradb_activity = HydraDbActivity::default();
    let mut memory_providers = MemoryProviderRegistry::default();
    if let Some(hydradb) = config.hydradb.as_ref() {
        let provider = HydraDbProvider::new(
            hydradb.base_url.clone(),
            hydradb.api_key.clone(),
            hydradb.fast_timeout,
            hydradb.thinking_timeout,
        )?
        .with_activity(hydradb_activity.clone());
        memory_providers = memory_providers.register(
            crate::entities::memory::MemoryProviderKind::Hydradb,
            Arc::new(provider),
        );
    }
    let memory_providers = Arc::new(memory_providers);
    let memory_coordinator = Arc::new(MemoryCoordinator::new(
        postgres_arc.clone(),
        memory_providers.clone(),
        monitoring.clone(),
    ));
    let memory_use_cases = Arc::new(MemoryUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        config.hydradb.is_some(),
    ));
    let memory_worker = Arc::new(MemoryWorker::new(
        postgres_arc.clone(),
        memory_providers,
        monitoring.clone(),
    ));
    let runtime_identity = MachineIdentity::from_runtime_environment();
    let argon_hasher = argon2_password_hasher();

    let user_use_cases = UserUseCases::new(Arc::new(argon_hasher), postgres_arc.clone())
        .with_email_confirmation(EmailConfirmation {
            registrations: postgres_arc.clone(),
            account_changes: postgres_arc.clone(),
            codes: Arc::new(SmtpConfirmationSender::new(mail_dispatcher.clone())),
            config: config.clone(),
        });
    let company_use_cases = CompanyUseCases::new(postgres_arc.clone());
    let company_invite_use_cases =
        CompanyInviteUseCases::new(postgres_arc.clone(), postgres_arc.clone());
    let channel_use_cases = Arc::new(
        ChannelUseCases::new(postgres_arc.clone(), postgres_arc.clone(), config.clone())
            .with_memory_persistence(postgres_arc.clone()),
    );
    let agent_use_cases = AgentUseCases::new(postgres_arc.clone(), postgres_arc.clone());

    let approval_use_cases = Arc::new(ApprovalUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        config.clone(),
    ));

    let egress_registry = Arc::new(
        EgressRegistry::new().register(Arc::new(EmailEgressAdapter::new(mail_dispatcher))),
    );

    let thread_use_cases = Arc::new(
        ThreadUseCases::new(
            postgres_arc.clone(),
            postgres_arc.clone(),
            postgres_arc.clone(),
            postgres_arc.clone(),
            config.clone(),
        )
        .with_agent_run_timeout(agent_run_timeout)
        .with_mail_transport(mail_transport)
        .with_egress_registry(egress_registry)
        .with_agent_persistence(postgres_arc.clone())
        .with_agent_channel_provisioning(postgres_arc.clone())
        .with_approval_use_cases(approval_use_cases.clone())
        .with_monitoring(monitoring.clone())
        .with_file_storage(file_storage.clone())
        .with_memory(memory_coordinator),
    );

    let schedule_use_cases = Arc::new(ScheduleUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        config.clone(),
    ));

    Ok(AppState {
        db: postgres_arc.pool().clone(),
        config,
        monitoring,
        user_use_cases: Arc::new(user_use_cases),
        company_use_cases: Arc::new(company_use_cases),
        company_invite_use_cases: Arc::new(company_invite_use_cases),
        channel_use_cases,
        schedule_use_cases,
        agent_use_cases: Arc::new(agent_use_cases),
        thread_use_cases,
        approval_use_cases,
        memory_use_cases,
        memory_worker,
        dashboard_persistence: postgres_arc.clone(),
        runtime_metrics: postgres_arc.clone(),
        runtime_identity,
        hydradb_activity,
        sessions,
        file_storage,
        events: MailboxEvents::new(),
    })
}

pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "mail_agents=info,tower_http=info".into());

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
