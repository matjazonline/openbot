use crate::{
    adapters::http::app_state::AppState,
    infra::{argon2_password_hasher, config::AppConfig, postgres_persistence},
    use_cases::{
        company::CompanyUseCases, company_invite::CompanyInviteUseCases, thread::ThreadUseCases,
        user::UserUseCases, workflow::WorkflowUseCases,
    },
};
use std::fs::File;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub async fn init_app_state() -> anyhow::Result<AppState> {
    let config = Arc::new(AppConfig::from_env());

    let postgres_arc = Arc::new(postgres_persistence().await?);
    let argon_hasher = argon2_password_hasher();

    let user_use_cases = UserUseCases::new(Arc::new(argon_hasher), postgres_arc.clone());
    let company_use_cases = CompanyUseCases::new(postgres_arc.clone());
    let company_invite_use_cases = CompanyInviteUseCases::new(postgres_arc.clone(), postgres_arc.clone());
    let workflow_use_cases = WorkflowUseCases::new(postgres_arc.clone(), postgres_arc.clone());
    let thread_use_cases = ThreadUseCases::new(
        postgres_arc.clone(),
        postgres_arc.clone(),
        postgres_arc.clone(),
        config.clone(),
    );

    Ok(AppState {
        config,
        user_use_cases: Arc::new(user_use_cases),
        company_use_cases: Arc::new(company_use_cases),
        company_invite_use_cases: Arc::new(company_invite_use_cases),
        workflow_use_cases: Arc::new(workflow_use_cases),
        thread_use_cases: Arc::new(thread_use_cases),
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
