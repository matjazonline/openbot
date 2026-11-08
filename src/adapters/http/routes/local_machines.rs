use crate::{
    adapters::http::{auth::AuthenticatedUser, auth_admission::AuthClientSource},
    app_error::{AppError, AppResult},
    application::{local_agents::*, transport::bounded::BoundedVec},
    domain::local_agents::{CloudroomProjectId, InstallationId, MachineId, PairingId},
};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, FromRef, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};
use uuid::Uuid;

pub fn public_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<LocalMachineUseCases>: FromRef<S>,
{
    Router::new()
        .route("/v1/local-machines/pair", post(pair))
        .route("/v1/local-machines/pair-status", get(pair_status))
        .layer(DefaultBodyLimit::max(4096))
        .layer(middleware::map_response(private_response))
}

pub fn protected_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<LocalMachineUseCases>: FromRef<S>,
{
    Router::new()
        .route(
            "/api/companies/{company}/local-machine-pairings",
            get(pairings).post(create_pairing),
        )
        .route(
            "/api/companies/{company}/local-machine-pairings/{pairing}",
            delete(cancel_pairing),
        )
        .route("/api/companies/{company}/local-machines", get(machines))
        .route(
            "/api/companies/{company}/local-machines/{machine}",
            get(machine),
        )
        .route(
            "/api/companies/{company}/local-machines/{machine}/state",
            post(change),
        )
        .route(
            "/api/companies/{company}/local-machines/{machine}/projects",
            put(projects),
        )
        .route(
            "/api/companies/{company}/local-machines/{machine}/replacement-pairing",
            post(replacement),
        )
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(middleware::map_response(private_response))
}

pub(super) async fn private_response(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page<T> {
    after: Option<T>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateChange {
    action: MachineChange,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectSelection {
    projects: BoundedVec<CloudroomProjectId, { bounds::MAX_PROJECTS }>,
}

fn source(extension: Option<Extension<AuthClientSource>>) -> IpAddr {
    extension
        .map(|Extension(source)| source.0)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

fn secret_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
    length: usize,
) -> AppResult<&'a str> {
    if headers.get_all(name).iter().count() != 1 {
        return Err(AppError::InvalidCredentials);
    }
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() == length)
        .ok_or(AppError::InvalidCredentials)
}

async fn pair(
    State(cases): State<Arc<LocalMachineUseCases>>,
    source_ip: Option<Extension<AuthClientSource>>,
    Json(claim): Json<PairingClaim>,
) -> AppResult<Json<PairingReceipt>> {
    Ok(Json(cases.pair(source(source_ip), claim).await?))
}

async fn pair_status(
    State(cases): State<Arc<LocalMachineUseCases>>,
    source_ip: Option<Extension<AuthClientSource>>,
    headers: HeaderMap,
) -> AppResult<Json<PairingReceipt>> {
    let secret = PairingClaimSecret::try_from(
        secret_header(&headers, "x-openbot-pair-claim", 64)?.to_owned(),
    )
    .map_err(|_| AppError::InvalidCredentials)?;
    let installation = InstallationId::try_from(
        secret_header(&headers, "x-openbot-installation-id", 36)?.to_owned(),
    )
    .map_err(|_| AppError::InvalidCredentials)?;
    Ok(Json(
        cases
            .pair_status(source(source_ip), installation, secret)
            .await?,
    ))
}

fn created(pairing: CreatedPairing) -> Response {
    (
        StatusCode::CREATED,
        Json(json!({"code":pairing.code.expose_secret(),"pairing":pairing.attempt})),
    )
        .into_response()
}

async fn create_pairing(
    State(cases): State<Arc<LocalMachineUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
) -> AppResult<Response> {
    Ok(created(cases.create_pairing(user.id, company, None).await?))
}

async fn replacement(
    State(cases): State<Arc<LocalMachineUseCases>>,
    user: AuthenticatedUser,
    Path((company, machine)): Path<(Uuid, MachineId)>,
) -> AppResult<Response> {
    Ok(created(
        cases
            .create_pairing(user.id, company, Some(machine))
            .await?,
    ))
}

async fn pairings(
    State(cases): State<Arc<LocalMachineUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Query(page): Query<Page<PairingId>>,
) -> AppResult<Json<Value>> {
    let items = cases.list_pairings(user.id, company, page.after).await?;
    let next = (items.len() == bounds::MAX_PAGE_RECORDS)
        .then(|| items.last().map(|item| item.id))
        .flatten();
    Ok(Json(json!({"pairings":items,"next":next})))
}

async fn cancel_pairing(
    State(cases): State<Arc<LocalMachineUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, PairingId)>,
) -> AppResult<StatusCode> {
    cases.cancel_pairing(user.id, company, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn machines(
    State(cases): State<Arc<LocalMachineUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Query(page): Query<Page<MachineId>>,
) -> AppResult<Json<Value>> {
    let items = cases.machines(user.id, company, page.after).await?;
    let next = (items.len() == bounds::MAX_PAGE_RECORDS)
        .then(|| items.last().map(|item| item.id))
        .flatten();
    Ok(Json(json!({"machines":items,"next":next})))
}

async fn machine(
    State(cases): State<Arc<LocalMachineUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, MachineId)>,
) -> AppResult<Json<Value>> {
    let details = cases.details(user.id, company, id).await?;
    Ok(Json(
        json!({"machine":details.machine,"catalog_available":details.capabilities.is_some()}),
    ))
}

async fn change(
    State(cases): State<Arc<LocalMachineUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, MachineId)>,
    Json(body): Json<StateChange>,
) -> AppResult<Json<crate::domain::local_agents::LocalMachine>> {
    Ok(Json(cases.change(user.id, company, id, body.action).await?))
}

async fn projects(
    State(cases): State<Arc<LocalMachineUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, MachineId)>,
    Json(body): Json<ProjectSelection>,
) -> AppResult<StatusCode> {
    cases
        .set_projects(user.id, company, id, &body.projects)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "local_machines_tests.rs"]
mod tests;
