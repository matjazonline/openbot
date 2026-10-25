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
        config::{AppConfig, smtp_allow_plaintext_local_from_env},
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

pub async fn init_app_state() -> anyhow::Result<AppState> {
    let config = Arc::new(AppConfig::from_env());

    // The relay every company that has *not* connected a provider account of its own sends
    // through, and the one every platform notice sends through whatever its tenants have
    // configured -- a confirmation code belongs to the deployment, not to a company.
    let deployment_mail_transport: Arc<dyn MailTransport> =
        LettreMailTransport::from_config(&config, smtp_allow_plaintext_local_from_env()).await?;

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
    install_builtin_agent_library(postgres_arc.as_ref()).await?;
    let database_query_health = Arc::new(DatabaseQueryHealthService::new(postgres_arc.clone()));
    let dashboard_snapshots = Arc::new(DashboardSnapshotService::new(postgres_arc.clone()));
    let runtime_identity = MachineIdentity::process().clone();
    let argon_hasher = argon2_password_hasher();

    let user_use_cases = UserUseCases::new(Arc::new(argon_hasher), postgres_arc.clone())
        .with_email_confirmation(EmailConfirmation {
            registrations: postgres_arc.clone(),
            account_changes: postgres_arc.clone(),
            codes: Arc::new(SmtpConfirmationSender::new(
                config.clone(),
                deployment_mail_transport.clone(),
            )),
            config: config.clone(),
        });
    let company_use_cases = CompanyUseCases::new(postgres_arc.clone());
    let mcp_policy = crate::adapters::mcp::policy::EndpointPolicy::new(
        std::env::var("MCP_INTERNAL_ENDPOINTS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned),
    )?;
    let mcp_client = Arc::new(crate::adapters::mcp::HttpMcpClient::new(mcp_policy));
    let mcp_use_cases = Arc::new(crate::use_cases::mcp::McpUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        mcp_client.clone(),
    ));
    let company_resend_api_use_cases =
        Arc::new(CompanyResendApiUseCases::new(postgres_arc.clone()));
    // One resolver, shared by the three places that act as a company at Resend: the delivery
    // worker sending its mail, the decoder fetching the mail its webhook announced, and nothing
    // else. It holds the HTTP pool; the credentials stay in the database until each call.
    let resend_api_clients = Arc::new(CompanyResendApiClients::new(
        postgres_arc.clone(),
        config.resend_api.clone(),
    )?);
    let company_invite_use_cases =
        CompanyInviteUseCases::new(postgres_arc.clone(), postgres_arc.clone());
    let channel_use_cases = Arc::new(ChannelUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        config.clone(),
    ));
    // The system-prompt generator's one-shot completion; agents themselves run as workflow model
    // steps.
    let prompt_classifier: Arc<dyn TextClassifier> =
        Arc::new(crate::adapters::harness::rig::RigTextClassifier::new(
            crate::adapters::harness::rig::providers::ProviderRegistry::standard()?,
        ));

    let agent_use_cases = AgentUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
    )
    .with_prompt_classifier(prompt_classifier);
    let skill_use_cases = Arc::new(SkillUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
    ));

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
    let workflow = Arc::new(crate::infra::workflow::WorkflowEngine::new(
        (*postgres_arc).clone(),
        &config.public_base_url(),
        &config.app_domain_name,
        delivery_composer.clone(),
        mcp_client,
        monitoring.clone(),
        crate::infra::workflow::worker_slots(
            std::env::var("WORKFLOW_WORKER_SLOTS").ok().as_deref(),
            crate::infra::db::max_connections(),
        )?,
    )?);
    let notification_worker = Arc::new(NotificationWorker::new(
        postgres_arc.clone(),
        delivery_composer.clone(),
        config.clone(),
        monitoring.clone(),
    ));

    let thread_use_cases = Arc::new(
        ThreadUseCases::new(
            ThreadStores {
                threads: postgres_arc.clone(),
                channels: postgres_arc.clone(),
                companies: postgres_arc.clone(),
                participants: postgres_arc.clone(),
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
        .with_monitoring(monitoring.clone())
        .with_intent_wakeups(workflow.intent_wakeups.clone()),
    );

    // Registered unconditionally now that Resend is configured per company: any tenant may connect
    // an account at any time, and a decoder that was left out at boot would leave that company's
    // stored events unclaimed until the next deploy. A decoder for a company with no account
    // fails the event terminally rather than silently, which is the honest answer.
    let inbound_decoders = InboundEventDecoderRegistry::new()
        .register(Arc::new(ResendApiInboundDecoder::new(
            resend_api_clients.clone(),
            config.clone(),
            thread_use_cases.clone(),
            file_storage.clone(),
        )))
        .map_err(|error| {
            anyhow::anyhow!("Could not register the Resend inbound decoder: {error}")
        })?;
    let inbound_event_wakeups = InboundEventWakeups::new();
    let inbound_event_worker = Arc::new(
        InboundEventWorker::new(
            postgres_arc.clone(),
            postgres_arc.clone(),
            Arc::new(inbound_decoders),
            inbound_event_wakeups.clone(),
        )
        .with_monitoring(monitoring.clone())
        .with_intent_wakeups(workflow.intent_wakeups.clone()),
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
                Arc::new(EmailSender::new(
                    Arc::new(ResendApiCompanyTransports::new(
                        resend_api_clients.clone(),
                        deployment_mail_transport.clone(),
                    )),
                    thread_use_cases.clone(),
                )),
            )
            .map_err(|error| anyhow::anyhow!("Could not register the email transport: {error}"))?,
    );

    let schedule_use_cases = Arc::new(ScheduleUseCases::new(
        postgres_arc.clone(),
        workflow.authorization(),
        workflow.schedule_run_now(),
    ));

    Ok(AppState {
        model_usage: postgres_arc.clone(),
        run_counts: postgres_arc.clone(),
        thread_activity: postgres_arc.clone(),
        message_outcomes: postgres_arc.clone(),
        db: postgres_arc.pool().clone(),
        mcp_use_cases,
        config,
        monitoring,
        user_use_cases: Arc::new(user_use_cases),
        company_use_cases: Arc::new(company_use_cases),
        company_invite_use_cases: Arc::new(company_invite_use_cases),
        company_resend_api_use_cases,
        company_resend_api_accounts: postgres_arc.clone(),
        channel_use_cases,
        schedule_use_cases,
        agent_use_cases: Arc::new(agent_use_cases),
        skill_use_cases,
        thread_use_cases,
        inbound_event_worker,
        inbound_event_inbox,
        inbound_event_wakeups,
        dashboard_snapshots,
        attention: postgres_arc.clone(),
        notifications: postgres_arc.clone(),
        notification_worker,
        database_query_health,
        dashboard_sse_connections: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        runtime_metrics: postgres_arc.clone(),
        runtime_identity,
        sessions,
        file_storage,
        transports,
        deliveries: postgres_arc.clone(),
        delivery_queue: postgres_arc.clone(),
        events: MailboxEvents::new(),
        workflow,
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
