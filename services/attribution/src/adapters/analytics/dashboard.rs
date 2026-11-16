use super::{export, postgres, views};
use crate::{
    adapters::{
        catalog,
        commerce::reads,
        identity::http::session_token,
        management::Authorized,
        setup::http::{SetupHttp, uuid},
    },
    application::{
        analytics::{Filter, Grouping, Query as ReportQuery, midnight},
        commerce::{Currency, EventState, OrderId, ProductId},
        identity::*,
        setup,
    },
};
use axum::{
    Router,
    extract::{Query, Request, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<SetupHttp> {
    Router::new()
        .route("/reports", get(report))
        .route("/reports/export", get(report))
        .route("/reports/orders", get(ledger))
        .route("/reports/orders/{id}", get(ledger))
        .route("/reports/events", get(ledger))
        .route(
            "/assets/reports.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../../assets/reports.js"),
                )
            }),
        )
        .route(
            "/assets/reports.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../../../assets/reports.css"),
                )
            }),
        )
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Form {
    pub company_id: String,
    pub start_date: String,
    pub end_date: String,
    pub timezone: String,
    pub store_id: String,
    pub ad_id: String,
    pub campaign_id: String,
    pub advertised_product_id: String,
    pub sold_product_id: String,
    pub currency: String,
    pub group_by: String,
    pub cursor: Option<String>,
}
fn optional<T>(raw: &str, parse: impl FnOnce(&str) -> Result<T>) -> Result<Option<T>> {
    if raw.is_empty() {
        Ok(None)
    } else {
        parse(raw).map(Some)
    }
}
fn enumeration<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(raw.into())).map_err(|_| Failure::Invalid)
}
impl Form {
    fn filter(&mut self) -> Result<Filter> {
        if self.timezone.is_empty() {
            self.timezone = "UTC".into();
        }
        let zone = self
            .timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| Failure::Invalid)?;
        let today = Utc::now().with_timezone(&zone).date_naive();
        if self.start_date.is_empty() {
            self.start_date = (today - Duration::days(29)).to_string();
        }
        if self.end_date.is_empty() {
            self.end_date = (today + Duration::days(1)).to_string();
        }
        ReportQuery {
            start: midnight(&self.start_date, &self.timezone)?.to_rfc3339(),
            end: midnight(&self.end_date, &self.timezone)?.to_rfc3339(),
            timezone: self.timezone.clone(),
            store_id: optional(&self.store_id, |v| uuid(v).map(setup::StoreId))?,
            ad_id: optional(&self.ad_id, |v| {
                uuid(v).map(crate::application::catalog::AdId)
            })?,
            campaign_id: optional(&self.campaign_id, |v| {
                uuid(v).map(crate::application::catalog::CampaignId)
            })?,
            advertised_product_id: optional(&self.advertised_product_id, |v| {
                ProductId::try_from(v.to_owned()).map_err(|_| Failure::Invalid)
            })?,
            sold_product_id: optional(&self.sold_product_id, |v| {
                ProductId::try_from(v.to_owned()).map_err(|_| Failure::Invalid)
            })?,
            currency: optional::<Currency>(&self.currency, enumeration)?,
            group_by: optional::<Grouping>(&self.group_by, enumeration)?,
            limit: None,
            cursor: self.cursor.clone(),
        }
        .validate()
    }
}
async fn report(State(state): State<SetupHttp>, request: Request) -> Response {
    match report_inner(state, request).await {
        Ok(r) => r,
        Err(e) => error(e),
    }
}
async fn report_inner(state: SetupHttp, request: Request) -> Result<Response> {
    let token = session_token(request.headers())?;
    let mut form = Query::<Form>::try_from_uri(request.uri())
        .map_err(|_| Failure::Invalid)?
        .0;
    let company = CompanyId(uuid(&form.company_id)?);
    let p = state
        .identity
        .service
        .company(&token, company, Permission::Read)
        .await?;
    let _permit = state.admission.enter(company)?;
    let f = form.filter()?;
    let is_export = request.uri().path() == "/reports/export";
    if is_export && form.cursor.is_some() {
        return Err(Failure::Invalid);
    }
    let grouping = f.group_by;
    let principal = Authorized::User(p);
    let result = postgres::report(&state.store, &principal, f, is_export).await?;
    if is_export {
        return Ok((
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=attribution.csv",
                ),
            ],
            export::csv(&result, grouping)?,
        )
            .into_response());
    }
    let Authorized::User(p) = &principal else {
        return Err(Failure::Unauthorized);
    };
    let list = setup::ListQuery {
        limit: Some(100),
        cursor: None,
        store_id: None,
    };
    let stores = state.store.list(p, setup::Resource::Stores, list).await?;
    let choices = setup::ListQuery {
        limit: Some(100),
        cursor: None,
        store_id: optional(&form.store_id, |v| uuid(v).map(setup::StoreId))?,
    };
    let ads = catalog::postgres::list(
        &state.store,
        &principal,
        crate::application::catalog::Resource::Ads,
        setup::ListQuery {
            limit: choices.limit,
            cursor: None,
            store_id: choices.store_id,
        },
    )
    .await?;
    let campaigns = catalog::postgres::list(
        &state.store,
        &principal,
        crate::application::catalog::Resource::Campaigns,
        choices,
    )
    .await?;
    let body = views::overview(&form, &result, &stores, &ads, &campaigns)?;
    Ok(Html(views::page(
        &p.access,
        &state.identity.config.openbot_origin,
        "Reports",
        &body,
    ))
    .into_response())
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LedgerQuery {
    company_id: String,
    store_id: Option<setup::StoreId>,
    state: String,
    cursor: Option<String>,
}
async fn ledger(State(state): State<SetupHttp>, request: Request) -> Response {
    match ledger_inner(state, request).await {
        Ok(r) => r,
        Err(e) => error(e),
    }
}
async fn ledger_inner(state: SetupHttp, request: Request) -> Result<Response> {
    let token = session_token(request.headers())?;
    let query = Query::<LedgerQuery>::try_from_uri(request.uri())
        .map_err(|_| Failure::Invalid)?
        .0;
    let company = CompanyId(uuid(&query.company_id)?);
    let p = state
        .identity
        .service
        .company(&token, company, Permission::Read)
        .await?;
    let _permit = state.admission.enter(company)?;
    let events = request.uri().path() == "/reports/events";
    let parent = request
        .uri()
        .path()
        .strip_prefix("/reports/orders/")
        .map(uuid)
        .transpose()?
        .map(OrderId);
    if !events && !query.state.is_empty() {
        return Err(Failure::Invalid);
    }
    let q = reads::ListQuery {
        limit: Some(25),
        cursor: query.cursor.clone(),
        store_id: query.store_id,
        state: optional::<EventState>(&query.state, enumeration)?,
    };
    let page = reads::list(
        &state.store,
        &p,
        if events {
            reads::Resource::Events
        } else if parent.is_some() {
            reads::Resource::Refunds
        } else {
            reads::Resource::Orders
        },
        parent,
        q,
    )
    .await?;
    let mut body = if let Some(id) = parent {
        let order = reads::read_order(&state.store, &p, id).await?;
        views::order(&order)?
    } else {
        String::new()
    };
    body += &views::ledger(
        &page,
        events.then_some(query.state.as_str()),
        parent.is_some(),
        &p,
    )?;
    if let Some(cursor) = page["next_cursor"].as_str() {
        let mut next = query;
        next.cursor = Some(cursor.into());
        let url = format!(
            "{}?{}",
            request.uri().path(),
            serde_urlencoded::to_string(&next).map_err(|_| Failure::Invalid)?
        );
        body += &format!(
            "<p><a href=\"{}\">Next page</a></p>",
            crate::adapters::html::escape(&url)
        );
    }
    Ok(Html(views::page(
        &p.access,
        &state.identity.config.openbot_origin,
        if events {
            "Merchant events"
        } else {
            "Orders and refunds"
        },
        &body,
    ))
    .into_response())
}
fn error(failure: Failure) -> Response {
    let status = failure.into_response().status();
    let message = match status {
        StatusCode::UNAUTHORIZED => "Sign in again to view reports.",
        StatusCode::FORBIDDEN => "Your current permissions do not allow this operation.",
        StatusCode::NOT_FOUND => "The selected company, store or item is not available.",
        StatusCode::BAD_REQUEST => {
            "Check the filters, reporting zone and dates. Use an interval of at most 93 days. A large export or total may require a narrower interval."
        }
        _ => "The report is temporarily unavailable. Retry in a moment.",
    };
    crate::adapters::html::error(status, "Report unavailable", message)
}
