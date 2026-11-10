//! The company memory opt-in form (U1). Every command authorizes owner/admin again under the
//! E2 locks; anyone else sees the same denial as a missing company.
use super::*;
use axum::extract::{DefaultBodyLimit, Path};
use serde::Deserialize;

/// Enumerated by the cross-origin write test, like every workspace write.
pub(super) const WRITE_ROUTES: [&str; 2] = [
    "/ui/companies/{company_id}/memory",
    "/ui/companies/{company_id}/memory/capture",
];

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(WRITE_ROUTES[0], get(show).post(command))
        .route(WRITE_ROUTES[1], post(capture))
        .layer(DefaultBodyLimit::max(4_096))
}

#[derive(Deserialize)]
pub(super) struct CaptureForm {
    pub agent: Uuid,
    pub kind: String,
    pub enabled: bool,
    pub allowance_tokens: i64,
    pub max_output_tokens: i32,
    pub expiry_hours: i32,
    pub reason: String,
    pub expected_revision: i64,
}

/// One capture policy change (E8); turning `threads` on repeats the cross-channel text.
pub(super) async fn capture(
    ui: Ui,
    Path(company): Path<Uuid>,
    axum::Form(form): axum::Form<CaptureForm>,
) -> AppResult<Html<String>> {
    use crate::application::memory::{CaptureLimits, MemoryCaptureChange, MemoryCaptureKind};
    let result = match MemoryCaptureKind::parse(&form.kind) {
        None => Err(AppError::BadRequest("Unknown capture kind".into())),
        Some(kind) => ui
            .engine
            .memory_settings()
            .set_capture(
                super::super::company(company),
                ui.actor()?,
                MemoryCaptureChange {
                    agent: form.agent,
                    kind,
                    enabled: form.enabled,
                    limits: CaptureLimits {
                        allowance_tokens: form.allowance_tokens,
                        max_output_tokens: form.max_output_tokens,
                        expiry_hours: form.expiry_hours,
                    },
                    reason: form.reason,
                    expected_revision: form.expected_revision,
                },
            )
            .await
            .map(|()| match (kind, form.enabled) {
                (MemoryCaptureKind::Threads, true) => format!(
                    "Capture settings saved. {}",
                    pages::company_memory::THREAD_CAPTURE_TEXT
                ),
                _ => "Capture settings saved.".to_owned(),
            }),
    };
    let message = match result {
        Ok(message) => message,
        Err(AppError::BadRequest(message) | AppError::Conflict(message)) => message,
        Err(AppError::NotFound(_)) => return Ok(Html(pages::company_memory::denied())),
        Err(error) => return Err(error),
    };
    match render(&ui, company, Some(&message)).await {
        Err(AppError::NotFound(_)) => Ok(Html(pages::company_memory::denied())),
        result => result,
    }
}

#[derive(Deserialize)]
pub(super) struct MemoryForm {
    pub action: String,
    pub expected_revision: i64,
}

async fn render(ui: &Ui, company: Uuid, message: Option<&str>) -> AppResult<Html<String>> {
    let view = ui
        .engine
        .memory_settings()
        .view(super::super::company(company), ui.actor()?)
        .await?;
    Ok(Html(pages::company_memory::section(
        company, &view, message,
    )))
}

pub(super) async fn show(ui: Ui, Path(company): Path<Uuid>) -> AppResult<Html<String>> {
    match render(&ui, company, None).await {
        Err(AppError::NotFound(_)) => Ok(Html(pages::company_memory::denied())),
        result => result,
    }
}

pub(super) async fn command(
    ui: Ui,
    Path(company): Path<Uuid>,
    axum::Form(form): axum::Form<MemoryForm>,
) -> AppResult<Html<String>> {
    let settings = ui.engine.memory_settings();
    let scope = super::super::company(company);
    let result = match form.action.as_str() {
        "enable" => settings
            .enable(scope, ui.actor()?, form.expected_revision)
            .await
            .map(|()| "Memory settings saved.".to_owned()),
        "disable" => settings
            .disable(scope, ui.actor()?, form.expected_revision)
            .await
            .map(|()| "Memory settings saved.".to_owned()),
        "retry" => settings
            .retry(scope, ui.actor()?, form.expected_revision)
            .await
            .map(|count| match count {
                0 => "Nothing failed; nothing to retry.".to_owned(),
                count => format!("Retrying {count} failed memory provisioning step(s)."),
            }),
        _ => Err(AppError::BadRequest("Unknown memory command".into())),
    };
    let message = match result {
        Ok(message) => message,
        Err(AppError::BadRequest(message) | AppError::Conflict(message)) => message,
        Err(AppError::NotFound(_)) => return Ok(Html(pages::company_memory::denied())),
        Err(error) => return Err(error),
    };
    match render(&ui, company, Some(&message)).await {
        Err(AppError::NotFound(_)) => Ok(Html(pages::company_memory::denied())),
        result => result,
    }
}
