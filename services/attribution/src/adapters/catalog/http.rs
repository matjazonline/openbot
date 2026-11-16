use super::postgres;
use crate::adapters::management::Authorized;
use crate::{
    adapters::{
        identity::http::{csrf, session_token},
        setup::http::{SetupHttp, header, parse, retry, uuid},
    },
    application::{
        catalog::*,
        identity::*,
        setup::{KeyScope, ListQuery},
    },
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
struct CatalogHttp {
    setup: SetupHttp,
    public: Arc<Semaphore>,
}
pub fn router(setup: SetupHttp) -> Router<crate::adapters::http::HttpState> {
    Router::new()
        .route("/v1/campaigns", get(manage).post(manage))
        .route("/v1/campaigns/{id}", get(manage).patch(manage))
        .route("/v1/ads", get(manage).post(manage))
        .route("/v1/ads/{id}", get(manage).patch(manage))
        .route("/v1/links", get(manage).post(manage))
        .route("/v1/links/{id}", get(manage).patch(manage))
        // Explicit HEAD avoids Axum's implicit GET handler creating a click.
        .route("/r/{code}", get(redirect).head(redirect))
        .with_state(CatalogHttp {
            setup,
            public: Arc::new(Semaphore::new(16)),
        })
}
async fn manage(State(state): State<CatalogHttp>, request: Request) -> Result<Response> {
    let state = &state.setup;
    let read = request.method() == Method::GET || request.method() == Method::HEAD;
    let path = request.uri().path().to_owned();
    let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
    let resource = match parts.get(1).copied() {
        Some("campaigns") => Resource::Campaigns,
        Some("ads") => Resource::Ads,
        Some("links") => Resource::Links,
        _ => return Err(Failure::NotFound),
    };
    let id = parts.get(2).map(|v| uuid(v)).transpose()?;
    // Both authority classes authenticate before any request body is consumed.
    let mut permit = None;
    let principal = if request.headers().contains_key("authorization") {
        if resource != Resource::Links {
            return Err(Failure::Forbidden);
        }
        Authorized::Key(
            state
                .store
                .merchant(
                    &state.identity.service,
                    &state.admission,
                    request.headers(),
                    if read {
                        KeyScope::LinksRead
                    } else {
                        KeyScope::LinksWrite
                    },
                )
                .await?,
        )
    } else {
        let token = session_token(request.headers())?;
        let company = CompanyId(uuid(header(request.headers(), "x-company-id")?)?);
        let p = state
            .identity
            .service
            .company(
                &token,
                company,
                if read {
                    Permission::Read
                } else {
                    Permission::ManageLinks
                },
            )
            .await?;
        permit = Some(state.admission.enter(company)?);
        if !read {
            verify_csrf(
                &p.principal.session.csrf,
                csrf(&state.identity.config, request.headers())?,
            )?;
        }
        Authorized::User(p)
    };
    let _permit = permit;
    if read {
        let query = Query::<ListQuery>::try_from_uri(request.uri())
            .map_err(|_| Failure::Invalid)?
            .0;
        let value = if let Some(id) = id {
            if query.limit.is_some() || query.cursor.is_some() || query.store_id.is_some() {
                return Err(Failure::Invalid);
            }
            postgres::read(&state.store, &principal, resource, id).await?
        } else {
            postgres::list(&state.store, &principal, resource, query).await?
        };
        return Ok(Json(value).into_response());
    }
    let retry = retry(request.headers())?;
    if request.uri().query().is_some() {
        return Err(Failure::Invalid);
    }
    let method = request.method().clone();
    let bytes = to_bytes(request.into_body(), 16384)
        .await
        .map_err(|_| Failure::TooLarge)?;
    let mutation = match (resource, id, method) {
        (Resource::Campaigns, None, Method::POST) => Mutation::CreateCampaign(parse(&bytes)?),
        (Resource::Campaigns, Some(id), Method::PATCH) => {
            Mutation::UpdateCampaign(CampaignId(id), parse(&bytes)?)
        }
        (Resource::Ads, None, Method::POST) => Mutation::CreateAd(parse(&bytes)?),
        (Resource::Ads, Some(id), Method::PATCH) => Mutation::UpdateAd(AdId(id), parse(&bytes)?),
        (Resource::Links, None, Method::POST) => Mutation::CreateLink(parse(&bytes)?),
        (Resource::Links, Some(id), Method::PATCH) => {
            Mutation::UpdateLink(LinkId(id), parse(&bytes)?)
        }
        _ => return Err(Failure::NotFound),
    };
    let (created, value) = postgres::mutate(&state.store, &principal, mutation, retry).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(value),
    )
        .into_response())
}
fn signal<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|h| std::str::from_utf8(&h.as_bytes()[..h.as_bytes().len().min(512)]).ok())
        .unwrap_or("")
}
async fn redirect(
    State(state): State<CatalogHttp>,
    Path(code): Path<String>,
    request: Request,
) -> Result<Response> {
    if code.len() != 24
        || !code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Failure::NotFound);
    }
    let _public = state
        .public
        .try_acquire()
        .map_err(|_| Failure::RateLimited)?;
    let scope = postgres::redirect_scope(&state.setup.store, &code).await?;
    let _tenant = state.setup.admission.enter(scope.company)?;
    let ticket = state.setup.identity.service.tenant(scope.company).await?;
    let flags = classify(
        signal(request.headers(), "user-agent"),
        signal(request.headers(), "purpose"),
        signal(request.headers(), "sec-purpose"),
    );
    match postgres::redirect(
        &state.setup.store,
        &scope,
        &ticket,
        &code,
        flags,
        request.method() == Method::HEAD,
    )
    .await?
    {
        Some(location) => Ok((StatusCode::FOUND, [("location", location)]).into_response()),
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}
