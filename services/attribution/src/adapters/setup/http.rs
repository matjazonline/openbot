use super::{admission::Admission, dns::DomainDns, postgres::SetupPostgres};
use crate::{
    adapters::identity::http::{IdentityHttp, csrf, session_token},
    application::{identity::*, setup::*},
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Query, Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use uuid::Uuid;

#[derive(Clone)]
pub struct SetupHttp {
    pub store: SetupPostgres,
    pub identity: IdentityHttp,
    pub dns: DomainDns,
    pub admission: Admission,
}
impl SetupHttp {
    pub fn production(pool: sqlx::PgPool, identity: IdentityHttp) -> anyhow::Result<Self> {
        Ok(Self {
            store: SetupPostgres {
                pool,
                cipher: identity.config.cipher.clone(),
            },
            identity,
            dns: DomainDns::production()?,
            admission: Admission::default(),
        })
    }
}
pub fn router(state: SetupHttp) -> Router<crate::adapters::http::HttpState> {
    Router::new()
        .route("/v1/stores", get(manage).post(manage))
        .route("/v1/stores/{id}", get(manage).patch(manage))
        .route("/v1/domains", get(manage).post(manage))
        .route("/v1/domains/{id}/verify", post(manage))
        .route("/v1/domains/{id}/revoke", post(manage))
        .route("/v1/installations", get(manage).post(manage))
        .route("/v1/installations/{id}", get(manage).patch(manage))
        .route("/v1/credentials", get(manage).post(manage))
        .route("/v1/credentials/{id}/rotate", post(manage))
        .route("/v1/credentials/{id}/revoke", post(manage))
        .with_state(state)
}
pub(crate) fn header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str> {
    if headers.get_all(name).iter().count() != 1 {
        return Err(Failure::Invalid);
    }
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or(Failure::Invalid)
}
pub(crate) fn uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .ok()
        .filter(|id| !id.is_nil())
        .ok_or(Failure::Invalid)
}
pub(crate) fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| Failure::Invalid)
}
fn no_body(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(Failure::Invalid)
    }
}
pub(crate) fn retry(headers: &HeaderMap) -> Result<Retry> {
    let raw_time = header(headers, "idempotency-created-at")?;
    if raw_time.len() > 40 {
        return Err(Failure::Invalid);
    }
    let retry = Retry {
        key: uuid(header(headers, "idempotency-key")?)?,
        created_at: DateTime::parse_from_rfc3339(raw_time)
            .map_err(|_| Failure::Invalid)?
            .with_timezone(&Utc),
    };
    retry.validate(Utc::now())?;
    Ok(retry)
}
async fn manage(State(state): State<SetupHttp>, request: Request) -> Result<Response> {
    let token = session_token(request.headers())?;
    let company = CompanyId(uuid(header(request.headers(), "x-company-id")?)?);
    let read = request.method() == Method::GET;
    let permission = if read {
        Permission::Read
    } else {
        Permission::ManageIntegrations
    };
    let principal = state
        .identity
        .service
        .company(&token, company, permission)
        .await?;
    let _permit = state.admission.enter(company)?;
    let path = request.uri().path().to_owned();
    let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
    let resource = match parts.get(1).copied() {
        Some("stores") => Resource::Stores,
        Some("domains") => Resource::Domains,
        Some("installations") => Resource::Installations,
        Some("credentials") => Resource::Credentials,
        _ => return Err(Failure::NotFound),
    };
    let id = parts.get(2).map(|s| uuid(s)).transpose()?;
    if read {
        let query = Query::<ListQuery>::try_from_uri(request.uri())
            .map_err(|_| Failure::Invalid)?
            .0;
        let body = if let Some(id) = id {
            if query.limit.is_some() || query.cursor.is_some() || query.store_id.is_some() {
                return Err(Failure::Invalid);
            }
            state.store.read(&principal, resource, id).await?
        } else {
            state.store.list(&principal, resource, query).await?
        };
        return Ok(Json(body).into_response());
    }
    verify_csrf(
        &principal.principal.session.csrf,
        csrf(&state.identity.config, request.headers())?,
    )?;
    let retry = retry(request.headers())?;
    if request.uri().query().is_some() {
        return Err(Failure::Invalid);
    }
    let method = request.method().clone();
    let body = to_bytes(request.into_body(), 16384)
        .await
        .map_err(|_| Failure::TooLarge)?;
    let mutation = match (resource, id, method, parts.get(3).copied()) {
        (Resource::Stores, None, Method::POST, None) => Mutation::CreateStore(parse(&body)?),
        (Resource::Stores, Some(id), Method::PATCH, None) => {
            Mutation::UpdateStore(StoreId(id), parse(&body)?)
        }
        (Resource::Domains, None, Method::POST, None) => Mutation::CreateDomain(parse(&body)?),
        (Resource::Domains, Some(id), Method::POST, Some("verify")) => {
            no_body(&body)?;
            Mutation::VerifyDomain(DomainId(id))
        }
        (Resource::Domains, Some(id), Method::POST, Some("revoke")) => {
            no_body(&body)?;
            Mutation::RevokeDomain(DomainId(id))
        }
        (Resource::Installations, None, Method::POST, None) => {
            Mutation::CreateInstallation(parse(&body)?)
        }
        (Resource::Installations, Some(id), Method::PATCH, None) => {
            Mutation::UpdateInstallation(InstallationId(id), parse(&body)?)
        }
        (Resource::Credentials, None, Method::POST, None) => {
            Mutation::CreateCredential(parse(&body)?)
        }
        (Resource::Credentials, Some(id), Method::POST, Some("revoke")) => {
            no_body(&body)?;
            Mutation::RevokeCredential(CredentialId(id))
        }
        (Resource::Credentials, Some(id), Method::POST, Some("rotate")) => {
            Mutation::RotateCredential(CredentialId(id), parse(&body)?)
        }
        _ => return Err(Failure::NotFound),
    };
    let result = state
        .store
        .mutate(&principal, mutation, retry, &state.dns)
        .await?;
    Ok((
        if result.created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(result.body),
    )
        .into_response())
}
