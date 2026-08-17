use crate::{
    adapters::{
        http::app_state::AppState,
        monitoring::{CompositeMonitor, InMemoryMonitor, TracingMonitor},
        protocols::{EgressRegistry, email::EmailEgressAdapter},
    },
    domain::monitoring::MonitoringService,
    infra::{argon2_password_hasher, config::AppConfig, postgres_persistence},
    use_cases::{
        agent::AgentUseCases, approval::ApprovalUseCases, channel::ChannelUseCases,
        company::CompanyUseCases, company_invite::CompanyInviteUseCases, thread::ThreadUseCases,
        user::UserUseCases,
    },
};
use std::fs::File;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub async fn init_app_state() -> anyhow::Result<AppState> {
    let config = Arc::new(AppConfig::from_env());

    let monitoring: Arc<dyn MonitoringService> = Arc::new(CompositeMonitor::new(vec![
        Arc::new(TracingMonitor::new()),
        Arc::new(InMemoryMonitor::new()),
    ]));

    let postgres_arc = Arc::new(postgres_persistence().await?);
    let argon_hasher = argon2_password_hasher();

    let user_use_cases = UserUseCases::new(Arc::new(argon_hasher), postgres_arc.clone());
    let company_use_cases = CompanyUseCases::new(postgres_arc.clone());
    let company_invite_use_cases =
        CompanyInviteUseCases::new(postgres_arc.clone(), postgres_arc.clone());
    let channel_use_cases = Arc::new(ChannelUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        config.clone(),
    ));
    let agent_use_cases = AgentUseCases::new(postgres_arc.clone(), postgres_arc.clone());

    let approval_use_cases = Arc::new(ApprovalUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        config.clone(),
    ));

    let egress_registry =
        Arc::new(EgressRegistry::new().register(Arc::new(EmailEgressAdapter::new(config.clone()))));

    let thread_use_cases = Arc::new(
        ThreadUseCases::new(
            postgres_arc.clone(),
            postgres_arc.clone(),
            postgres_arc.clone(),
            postgres_arc.clone(),
            config.clone(),
        )
        .with_egress_registry(egress_registry)
        .with_agent_persistence(postgres_arc.clone())
        .with_approval_use_cases(approval_use_cases.clone())
        .with_monitoring(monitoring.clone()),
    );

    Ok(AppState {
        db: postgres_arc.pool().clone(),
        config,
        monitoring,
        user_use_cases: Arc::new(user_use_cases),
        company_use_cases: Arc::new(company_use_cases),
        company_invite_use_cases: Arc::new(company_invite_use_cases),
        channel_use_cases,
        agent_use_cases: Arc::new(agent_use_cases),
        thread_use_cases,
        approval_use_cases,
    })
}

pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "axum_trainer=debug,tower_http=debug".into());

    // Console (pretty logs)
    let console_layer = fmt::layer()
        .with_target(false) // don’t show target (module path)
        .with_level(true) // show log level
        .pretty(); // human-friendly, with colors

    // File (structured JSON logs)
    let file = File::create("app.log").expect("cannot create log file");
    let json_layer = fmt::layer()
        .json()
        .with_writer(file)
        .with_current_span(true)
        .with_span_list(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(json_layer)
        .try_init()
        .ok();
}
