use crate::{
    adapters::{
        http::{app_state::AppState, session::SessionAuthority},
        memory::{hindsight::HindsightProvider, hydradb::HydraDbProvider},
        monitoring::{CompositeMonitor, InMemoryMonitor, TracingMonitor},
        protocols::email::{EmailRenderer, EmailSender},
        smtp::LettreMailTransport,
        storage::{FileStorage, gcs::GcsFileStorage},
    },
    domain::monitoring::MonitoringService,
    entities::{memory::MemoryProviderKind, runtime_metrics::MachineIdentity},
    infra::{
        argon2_password_hasher,
        config::{AppConfig, agent_run_timeout_from_env, smtp_allow_plaintext_local_from_env},
        events::MailboxEvents,
        postgres_persistence,
    },
    services::{
        database_query_health::DatabaseQueryHealthService,
        inbound_event_worker::{InboundEventWakeups, InboundEventWorker},
        memory_coordinator::MemoryCoordinator,
        memory_provider::{ConfiguredMemoryProviders, MemoryProviderRegistry},
        memory_worker::MemoryWorker,
        outbound_dispatcher::{MailTransport, OutboundDispatcher, SmtpConfirmationSender},
        runtime_metrics::MemoryProviderActivity,
    },
    transport::{
        DeliveryComposer, InboundEventDecoderRegistry, MonitoredInboundEventInbox,
        TransportRegistry, TransportRenderers,
    },
    use_cases::{
        agent::AgentUseCases,
        approval::ApprovalUseCases,
        channel::ChannelUseCases,
        company::CompanyUseCases,
        company_invite::CompanyInviteUseCases,
        memory::MemoryUseCases,
        schedule::ScheduleUseCases,
        thread::{InboundIngestPorts, ThreadStores, ThreadUseCases},
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
    let database_query_health = Arc::new(DatabaseQueryHealthService::new(postgres_arc.clone()));
    let memory_provider_activity = MemoryProviderActivity::default();
    // One registry entry and one configured-set entry per provider this deployment carries
    // credentials for. The activity handle is shared: the runtime panel is a machine-level
    // aggregate, not a per-provider split.
    let mut memory_providers = MemoryProviderRegistry::default();
    let mut configured_memory_providers = Vec::new();
    if let Some(hydradb) = config.hydradb.as_ref() {
        let provider = HydraDbProvider::new(
            hydradb.base_url.clone(),
            hydradb.api_key.clone(),
            hydradb.fast_timeout,
            hydradb.thinking_timeout,
        )?
        .with_activity(memory_provider_activity.clone());
        memory_providers =
            memory_providers.register(MemoryProviderKind::Hydradb, Arc::new(provider));
        configured_memory_providers.push(MemoryProviderKind::Hydradb);
    }
    if let Some(hindsight) = config.hindsight.as_ref() {
        let provider = HindsightProvider::new(
            hindsight.base_url.clone(),
            hindsight.api_key.clone(),
            hindsight.fast_timeout,
            hindsight.thinking_timeout,
        )?
        .with_activity(memory_provider_activity.clone());
        memory_providers =
            memory_providers.register(MemoryProviderKind::Hindsight, Arc::new(provider));
        configured_memory_providers.push(MemoryProviderKind::Hindsight);
    }
    let configured_memory_providers =
        ConfiguredMemoryProviders::from_iter(configured_memory_providers);
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
        configured_memory_providers,
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
        ChannelUseCases::new(
            postgres_arc.clone(),
            postgres_arc.clone(),
            postgres_arc.clone(),
            config.clone(),
        )
        .with_memory_persistence(postgres_arc.clone()),
    );
    let agent_use_cases = AgentUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        if config.is_spam_scan_enabled() {
            crate::use_cases::agent::SpamScanning::Available
        } else {
            crate::use_cases::agent::SpamScanning::Unavailable
        },
    );

    // Renderers first, then the use cases that freeze parts with them, then the senders -- one of
    // which needs those use cases as its internal relay. Building the pair in that order is what
    // keeps the wiring acyclic; see `TransportRenderers`.
    let email_renderer = Arc::new(EmailRenderer::new(&config.app_domain_name));
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(email_renderer.clone())
            .map_err(|error| anyhow::anyhow!("Could not register the email renderer: {error}"))?,
    );
    let delivery_composer = DeliveryComposer::new(renderers.clone(), postgres_arc.clone());

    let approval_use_cases = Arc::new(ApprovalUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        delivery_composer.clone(),
        config.clone(),
    ));

    let thread_use_cases = Arc::new(
        ThreadUseCases::new(
            ThreadStores {
                threads: postgres_arc.clone(),
                channels: postgres_arc.clone(),
                companies: postgres_arc.clone(),
                participants: postgres_arc.clone(),
                tasks: postgres_arc.clone(),
            },
            InboundIngestPorts {
                committer: postgres_arc.clone(),
                correlation: postgres_arc.clone(),
                bindings: postgres_arc.clone(),
                standalone_deliveries: postgres_arc.clone(),
            },
            renderers.clone(),
            config.clone(),
        )
        .with_agent_run_timeout(agent_run_timeout)
        .with_mail_transport(mail_transport.clone())
        .with_agent_persistence(postgres_arc.clone())
        .with_agent_channel_provisioning(postgres_arc.clone())
        .with_approval_use_cases(approval_use_cases.clone())
        .with_monitoring(monitoring.clone())
        .with_memory(memory_coordinator),
    );

    // The generic inbox exists before any webhook uses it. Slack adds its decoder to this
    // registry in the transport phase; an empty registry is safe because no route can enqueue a
    // Slack event yet, while startup reconciliation is already supervised and exercised.
    let inbound_event_wakeups = InboundEventWakeups::new();
    let inbound_event_worker = Arc::new(
        InboundEventWorker::new(
            postgres_arc.clone(),
            postgres_arc.clone(),
            Arc::new(InboundEventDecoderRegistry::new()),
            inbound_event_wakeups.clone(),
        )
        .with_monitoring(monitoring.clone()),
    );
    let inbound_event_inbox = Arc::new(MonitoredInboundEventInbox::new(
        postgres_arc.clone(),
        monitoring.clone(),
    ));

    // The email sender reaches the internal relay before SMTP, so a channel answering another
    // channel of the same company never leaves the building.
    let transports = Arc::new(
        TransportRegistry::new()
            .register(
                // The same renderer instance the producers freeze parts with, so a queued part and
                // a re-render can never disagree about the deployment's own domain.
                email_renderer,
                Arc::new(EmailSender::new(mail_transport, thread_use_cases.clone())),
            )
            .map_err(|error| anyhow::anyhow!("Could not register the email transport: {error}"))?,
    );

    let schedule_use_cases = Arc::new(ScheduleUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
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
        inbound_event_worker,
        inbound_event_inbox,
        inbound_event_wakeups,
        dashboard_persistence: postgres_arc.clone(),
        database_query_health,
        dashboard_sse_connections: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        runtime_metrics: postgres_arc.clone(),
        runtime_identity,
        memory_provider_activity,
        sessions,
        file_storage,
        transports,
        deliveries: postgres_arc.clone(),
        delivery_queue: postgres_arc.clone(),
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
