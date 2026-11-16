use super::{
    postgres,
    reads::{self, ListQuery, Resource},
};
use crate::{
    adapters::{
        identity::http::{csrf, session_token},
        setup::http::{SetupHttp, header, parse, retry, uuid},
    },
    application::{commerce::*, identity::*, setup::KeyScope},
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Query, Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};

#[derive(Clone)]
pub struct CommerceHttp {
    pub setup: SetupHttp,
    pub policy: CommercePolicy,
}
pub fn router(state: CommerceHttp) -> Router<crate::adapters::http::HttpState> {
    Router::new()
        .route("/v1/conversions", post(ingest))
        .route("/v1/orders", get(manage))
        .route("/v1/orders/{id}", get(manage))
        .route("/v1/orders/{id}/refunds", get(manage))
        .route("/v1/merchant-events", get(manage))
        .route("/v1/merchant-events/{id}/retry", post(manage))
        .with_state(state)
}
async fn ingest(State(state): State<CommerceHttp>, request: Request) -> Result<Response> {
    let principal = state
        .setup
        .store
        .merchant(
            &state.setup.identity.service,
            &state.setup.admission,
            request.headers(),
            KeyScope::ConversionsWrite,
        )
        .await?;
    if request.uri().query().is_some() {
        return Err(Failure::Invalid);
    }
    let body = to_bytes(request.into_body(), 262144)
        .await
        .map_err(|_| Failure::TooLarge)?;
    let result =
        postgres::ingest(&state.setup.store, &principal, state.policy, parse(&body)?).await?;
    let status = if result.state == EventState::Pending {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(result)).into_response())
}
async fn manage(State(state): State<CommerceHttp>, request: Request) -> Result<Response> {
    let token = session_token(request.headers())?;
    let company = CompanyId(uuid(header(request.headers(), "x-company-id")?)?);
    let read = request.method() == Method::GET;
    let p = state
        .setup
        .identity
        .service
        .company(
            &token,
            company,
            if read {
                Permission::Read
            } else {
                Permission::ManageIntegrations
            },
        )
        .await?;
    let _permit = state.setup.admission.enter(company)?;
    let path = request.uri().path().to_owned();
    let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
    let id = parts.get(2).map(|s| uuid(s)).transpose()?;
    if read {
        let q = Query::<ListQuery>::try_from_uri(request.uri())
            .map_err(|_| Failure::Invalid)?
            .0;
        let value = match (parts.get(1).copied(), id, parts.get(3).copied()) {
            (Some("orders"), None, None) => {
                reads::list(&state.setup.store, &p, Resource::Orders, None, q).await?
            }
            (Some("orders"), Some(id), None) => {
                if request.uri().query().is_some() {
                    return Err(Failure::Invalid);
                }
                reads::read_order(&state.setup.store, &p, OrderId(id)).await?
            }
            (Some("orders"), Some(id), Some("refunds")) => {
                reads::list(
                    &state.setup.store,
                    &p,
                    Resource::Refunds,
                    Some(OrderId(id)),
                    q,
                )
                .await?
            }
            (Some("merchant-events"), None, None) => {
                reads::list(&state.setup.store, &p, Resource::Events, None, q).await?
            }
            _ => return Err(Failure::NotFound),
        };
        return Ok(Json(value).into_response());
    }
    verify_csrf(
        &p.principal.session.csrf,
        csrf(&state.setup.identity.config, request.headers())?,
    )?;
    let retry = retry(request.headers())?;
    if request.uri().query().is_some() {
        return Err(Failure::Invalid);
    }
    let body = to_bytes(request.into_body(), 16384)
        .await
        .map_err(|_| Failure::TooLarge)?;
    if !body.is_empty() {
        return Err(Failure::Invalid);
    }
    let value = reads::retry_event(
        &state.setup.store,
        &p,
        EventId(id.ok_or(Failure::Invalid)?),
        retry,
    )
    .await?;
    Ok(Json(value).into_response())
}
