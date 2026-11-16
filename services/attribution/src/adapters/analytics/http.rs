use super::{dashboard, export, postgres};
use crate::{
    adapters::{
        identity::http::session_token,
        management::Authorized,
        setup::http::{SetupHttp, header, uuid},
    },
    application::{
        analytics::Query as ReportQuery,
        identity::{CompanyId, Failure, Permission, Result},
        setup::KeyScope,
    },
};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::header as headers,
    response::{IntoResponse, Response},
    routing::get,
};

pub fn router(state: SetupHttp) -> Router<crate::adapters::http::HttpState> {
    Router::new()
        .route("/v1/analytics", get(report))
        .route("/v1/analytics/export", get(report))
        .merge(dashboard::router())
        .with_state(state)
}
async fn report(State(state): State<SetupHttp>, request: Request) -> Result<Response> {
    let mut permit = None;
    let principal = if request.headers().contains_key(headers::AUTHORIZATION) {
        Authorized::Key(
            state
                .store
                .merchant(
                    &state.identity.service,
                    &state.admission,
                    request.headers(),
                    KeyScope::AnalyticsRead,
                )
                .await?,
        )
    } else {
        let token = session_token(request.headers())?;
        let company = CompanyId(uuid(header(request.headers(), "x-company-id")?)?);
        let p = state
            .identity
            .service
            .company(&token, company, Permission::Read)
            .await?;
        permit = Some(state.admission.enter(company)?);
        Authorized::User(p)
    };
    let _permit = permit;
    let query = Query::<ReportQuery>::try_from_uri(request.uri())
        .map_err(|_| Failure::Invalid)?
        .0;
    let is_export = request.uri().path().ends_with("/export");
    if is_export && (query.cursor.is_some() || query.limit.is_some()) {
        return Err(Failure::Invalid);
    }
    let filter = query.validate()?;
    let grouping = filter.group_by;
    let result = postgres::report(&state.store, &principal, filter, is_export).await?;
    if is_export {
        Ok((
            [
                (headers::CONTENT_TYPE, "text/csv; charset=utf-8"),
                (
                    headers::CONTENT_DISPOSITION,
                    "attachment; filename=attribution.csv",
                ),
            ],
            export::csv(&result, grouping)?,
        )
            .into_response())
    } else {
        Ok(Json(result).into_response())
    }
}
