//! Browser forms over the same company-scoped, revision-checked TypeSafe port as the API.
use super::*;
use crate::application::workflow::jev::{JevConnectionUpdate, JevConnectionWrite, JevModel};
use axum::extract::{DefaultBodyLimit, Path};
use secrecy::SecretString;
use serde::Deserialize;
use std::collections::BTreeSet;

pub(super) const WRITE_ROUTES: [&str; 3] = [
    "/ui/companies/{company_id}/typesafe-connections",
    "/ui/companies/{company_id}/typesafe-connections/{id}",
    "/ui/companies/{company_id}/typesafe-connections/{id}/rotate",
];

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(WRITE_ROUTES[0], get(list).post(create))
        .route(WRITE_ROUTES[1], post(update))
        .route(WRITE_ROUTES[2], post(rotate))
        .layer(DefaultBodyLimit::max(16_384))
}

// No Debug: a submitted key must never enter tracing or a rendered form.
#[derive(Deserialize)]
pub(super) struct ConnectionForm {
    pub models: String,
    pub enabled: Option<String>,
    pub expected_revision: Option<i64>,
    pub api_key: Option<SecretString>,
}

fn models(form: &ConnectionForm) -> AppResult<BTreeSet<JevModel>> {
    form.models
        .split(',')
        .map(|model| JevModel::parse(model.trim()))
        .collect()
}

pub(super) async fn list(ui: Ui, Path(company): Path<Uuid>) -> AppResult<Html<String>> {
    match render(&ui, company, None).await {
        Err(AppError::NotFound(_)) => Ok(Html(r#"<section id="typesafe-connections"><p role="status">TypeSafe connections require company owner or admin permission.</p></section>"#.into())),
        result => result,
    }
}

async fn render(ui: &Ui, company: Uuid, message: Option<&str>) -> AppResult<Html<String>> {
    let connections = ui
        .engine
        .jev_connections()
        .list_jev(super::super::company(company), ui.actor()?)
        .await?;
    Ok(Html(pages::company_typesafe::connections(
        company,
        &connections,
        message,
    )))
}

async fn answer(ui: &Ui, company: Uuid, result: AppResult<()>) -> AppResult<Html<String>> {
    let message = match result {
        Ok(()) => "TypeSafe connection saved.".to_owned(),
        Err(AppError::BadRequest(message)) => message,
        Err(AppError::Conflict(_)) => {
            "This connection changed. Review its current revision and retry.".into()
        }
        Err(AppError::NotFound(_)) => "Connection unavailable or permission denied.".into(),
        Err(error) => return Err(error),
    };
    render(ui, company, Some(&message)).await
}

pub(super) async fn create(
    ui: Ui,
    Path(company): Path<Uuid>,
    axum::Form(form): axum::Form<ConnectionForm>,
) -> AppResult<Html<String>> {
    let result = async {
        ui.engine
            .jev_connections()
            .create_jev(
                super::super::company(company),
                ui.actor()?,
                JevConnectionWrite {
                    models: models(&form)?,
                    enabled: form.enabled.is_some(),
                    secret: form
                        .api_key
                        .ok_or_else(|| AppError::BadRequest("Enter a TypeSafe API key".into()))?,
                },
            )
            .await?;
        Ok(())
    }
    .await;
    answer(&ui, company, result).await
}

pub(super) async fn update(
    ui: Ui,
    Path((company, id)): Path<(Uuid, Uuid)>,
    axum::Form(form): axum::Form<ConnectionForm>,
) -> AppResult<Html<String>> {
    let result = async {
        ui.engine
            .jev_connections()
            .update_jev(
                super::super::company(company),
                ui.actor()?,
                id,
                JevConnectionUpdate {
                    models: models(&form)?,
                    enabled: form.enabled.is_some(),
                    expected_revision: form.expected_revision.ok_or_else(|| {
                        AppError::BadRequest("Missing connection revision".into())
                    })?,
                },
            )
            .await?;
        Ok(())
    }
    .await;
    answer(&ui, company, result).await
}

#[derive(Deserialize)]
pub(super) struct RotateForm {
    pub expected_revision: i64,
    pub api_key: SecretString,
}

pub(super) async fn rotate(
    ui: Ui,
    Path((company, id)): Path<(Uuid, Uuid)>,
    axum::Form(form): axum::Form<RotateForm>,
) -> AppResult<Html<String>> {
    let result = ui
        .engine
        .jev_connections()
        .rotate_jev(
            super::super::company(company),
            ui.actor()?,
            id,
            form.expected_revision,
            form.api_key,
        )
        .await
        .map(|_| ());
    answer(&ui, company, result).await
}
