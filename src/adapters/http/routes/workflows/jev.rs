//! Secret-free company TypeSafe views; body values never enter tracing or Debug.
use super::*;
use crate::application::workflow::jev::*;
use axum::extract::{Path, State};
use secrecy::SecretString;
use serde::Deserialize;
use std::collections::BTreeSet;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/companies/{company_id}/typesafe-connections",
            get(list).post(create),
        )
        .route(
            "/api/companies/{company_id}/typesafe-connections/{id}",
            get(read).put(update),
        )
        .route(
            "/api/companies/{company_id}/typesafe-connections/{id}/rotate",
            post(rotate),
        )
        .layer(DefaultBodyLimit::max(16_384))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBody {
    pub enabled: bool,
    pub models: BTreeSet<JevModel>,
    pub api_key: SecretString,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBody {
    pub expected_revision: i64,
    pub enabled: bool,
    pub models: BTreeSet<JevModel>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotateBody {
    pub expected_revision: i64,
    pub api_key: SecretString,
}
pub async fn list(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> WorkflowResult<Json<Value>> {
    Ok(ok(json!(
        engine
            .jev_connections()
            .list_jev(company(id), actor(&user)?)
            .await?
    )))
}
pub async fn read(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, id)): Path<(Uuid, Uuid)>,
) -> WorkflowResult<Json<Value>> {
    Ok(ok(json!(
        engine
            .jev_connections()
            .read_jev(company(company_id), actor(&user)?, id)
            .await?
    )))
}
pub async fn create(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateBody>,
) -> WorkflowResult<(StatusCode, Json<Value>)> {
    let result = engine
        .jev_connections()
        .create_jev(
            company(id),
            actor(&user)?,
            JevConnectionWrite {
                enabled: body.enabled,
                models: body.models,
                secret: body.api_key,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, ok(json!(result))))
}
pub async fn update(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateBody>,
) -> WorkflowResult<Json<Value>> {
    let result = engine
        .jev_connections()
        .update_jev(
            company(company_id),
            actor(&user)?,
            id,
            JevConnectionUpdate {
                expected_revision: body.expected_revision,
                enabled: body.enabled,
                models: body.models,
            },
        )
        .await?;
    Ok(ok(json!(result)))
}
pub async fn rotate(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<RotateBody>,
) -> WorkflowResult<Json<Value>> {
    Ok(ok(json!(
        engine
            .jev_connections()
            .rotate_jev(
                company(company_id),
                actor(&user)?,
                id,
                body.expected_revision,
                body.api_key
            )
            .await?
    )))
}
