use super::postgres;
use crate::{
    adapters::setup::http::{SetupHttp, parse},
    application::{
        catalog::{ClickId, LinkId},
        identity::*,
        setup::{InstallationId, KeyScope},
        website::*,
    },
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;

#[derive(Clone)]
struct WebsiteHttp {
    setup: SetupHttp,
    public: Arc<Semaphore>,
}
pub fn router(setup: SetupHttp) -> Router<crate::adapters::http::HttpState> {
    Router::new()
        .route("/pixel.gif", get(observe).head(observe))
        .route("/v1/events", post(observe).options(preflight))
        .route("/v1/checkouts", post(checkout))
        .route("/sdk/1.0.0.js", get(sdk))
        .with_state(WebsiteHttp {
            setup,
            public: Arc::new(Semaphore::new(16)),
        })
}
async fn sdk() -> impl IntoResponse {
    (
        [
            ("content-type", "text/javascript; charset=utf-8"),
            ("access-control-allow-origin", "*"),
        ],
        include_str!("../../../assets/sdk-1.0.0.js"),
    )
}
fn origin(headers: &HeaderMap) -> Result<Option<String>> {
    if headers.get_all("origin").iter().count() > 1 {
        return Err(Failure::Forbidden);
    }
    headers
        .get("origin")
        .map(|h| {
            h.to_str()
                .map(str::to_owned)
                .map_err(|_| Failure::Forbidden)
        })
        .transpose()
}
fn cors(mut response: Response, origin: Option<&str>) -> Response {
    if let Some(origin) = origin
        && let Ok(value) = HeaderValue::from_str(origin)
    {
        response
            .headers_mut()
            .insert("access-control-allow-origin", value);
    }
    if origin.is_some() {
        response.headers_mut().insert(
            "access-control-expose-headers",
            HeaderValue::from_static("Retry-After"),
        );
    }
    response
        .headers_mut()
        .insert("vary", HeaderValue::from_static("Origin"));
    response
}
async fn preflight(State(state): State<WebsiteHttp>, request: Request) -> Result<Response> {
    let permit = state.public.try_acquire();
    let origin = origin(request.headers())?.ok_or(Failure::Forbidden)?;
    postgres::cors_origin(&state.setup.store, &origin).await?;
    let result: Result<Response> = (|| {
        let _permit = permit.map_err(|_| Failure::RateLimited)?;
        if request
            .headers()
            .get_all("access-control-request-method")
            .iter()
            .count()
            != 1
            || request
                .headers()
                .get("access-control-request-method")
                .and_then(|h| h.to_str().ok())
                != Some("POST")
        {
            return Err(Failure::Forbidden);
        }
        if request
            .headers()
            .get_all("access-control-request-headers")
            .iter()
            .count()
            > 1
        {
            return Err(Failure::Forbidden);
        }
        if let Some(headers) = request.headers().get("access-control-request-headers")
            && !headers
                .to_str()
                .map_err(|_| Failure::Forbidden)?
                .split(',')
                .all(|h| h.trim().eq_ignore_ascii_case("content-type"))
        {
            return Err(Failure::Forbidden);
        }
        Ok((
            StatusCode::NO_CONTENT,
            [
                ("access-control-allow-methods", "POST"),
                ("access-control-allow-headers", "Content-Type"),
            ],
        )
            .into_response())
    })();
    Ok(cors(result.into_response(), Some(&origin)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pixel {
    installation_id: InstallationId,
    event_id: Option<ObservationId>,
    observed_at: Option<String>,
    click_id: Option<ClickId>,
    link_id: Option<LinkId>,
    consent: String,
}
// GIF89a, one transparent pixel. No successful image is returned before COMMIT.
const GIF:&[u8]=b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\x21\xf9\x04\x01\x00\x00\x00\x00\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3b";
async fn observe(State(state): State<WebsiteHttp>, request: Request) -> Response {
    // Admission still precedes body reads and ingestion. An approved-origin
    // response-policy lookup is bounded by the shared HTTP admission/deadline,
    // including when the narrower public budget is occupied.
    let permit = state.public.try_acquire();
    let origin = match origin(request.headers()) {
        Ok(origin) => origin,
        Err(e) => return e.into_response(),
    };
    if let Some(origin) = &origin
        && let Err(e) = postgres::cors_origin(&state.setup.store, origin).await
    {
        return e.into_response();
    }
    let result: Result<Response> = async {
        let _permit = permit.map_err(|_| Failure::RateLimited)?;
        let pixel = request.uri().path() == "/pixel.gif";
        let head = request.method() == Method::HEAD;
        let event = if pixel {
            if request.uri().query().is_some_and(|q| q.len() > 2048) {
                return Err(Failure::TooLarge);
            }
            let q = Query::<Pixel>::try_from_uri(request.uri())
                .map_err(|_| Failure::Invalid)?
                .0;
            if q.consent != "granted" || q.event_id.is_some() != q.observed_at.is_some() {
                return Err(Failure::Invalid);
            }
            Observation {
                installation_id: q.installation_id,
                event_id: q.event_id.unwrap_or(ObservationId(Uuid::new_v4())),
                kind: ObservationKind::ImageLoad,
                click_id: q.click_id,
                link_id: q.link_id,
                observed_at: q
                    .observed_at
                    .as_deref()
                    .map(parse_time)
                    .transpose()?
                    .unwrap_or_else(Utc::now),
                consent: true,
            }
        } else {
            if request.uri().query().is_some() {
                return Err(Failure::Invalid);
            }
            parse::<Observation>(
                &to_bytes(request.into_body(), 8192)
                    .await
                    .map_err(|_| Failure::TooLarge)?,
            )?
        };
        event.validate()?;
        let company = state
            .setup
            .store
            .installation_scope(event.installation_id)
            .await?;
        let _tenant = state.setup.admission.enter(company)?;
        let ticket = state.setup.identity.service.tenant(company).await?;
        postgres::observe(
            &state.setup.store,
            company,
            &ticket,
            &event,
            origin.as_deref(),
            head,
        )
        .await?;
        if head || !pixel {
            Ok(StatusCode::NO_CONTENT.into_response())
        } else {
            Ok(([("content-type", "image/gif")], GIF).into_response())
        }
    }
    .await;
    cors(result.into_response(), origin.as_deref())
}
async fn checkout(State(state): State<WebsiteHttp>, request: Request) -> Result<Response> {
    let p = state
        .setup
        .store
        .merchant(
            &state.setup.identity.service,
            &state.setup.admission,
            request.headers(),
            KeyScope::CheckoutsWrite,
        )
        .await?;
    if request.uri().query().is_some() {
        return Err(Failure::Invalid);
    }
    let body = to_bytes(request.into_body(), 16384)
        .await
        .map_err(|_| Failure::TooLarge)?;
    Ok(Json(postgres::checkout(&state.setup.store, &p, parse(&body)?).await?).into_response())
}
