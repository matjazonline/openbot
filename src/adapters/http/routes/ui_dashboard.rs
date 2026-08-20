//! `/ui/dashboard` — the operational overview, for one company or for all of them.
//!
//! The scope is decided once, by [`DashboardScope::resolve`], and everything downstream takes the
//! `Option<Uuid>` it produces. A normal user is pinned to a company they belong to by
//! [`super::ui::load_scoped_company`]; an operator named in `OPERATOR_EMAILS` gets `None`, which the
//! queries read as "every company".
//!
//! The stream is a ticker rather than an event subscription. These are sampled gauges — a queue
//! depth has no "changed" moment to subscribe to — so re-reading on an interval is both simpler and
//! a more honest description of what the page shows.

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{FromRequestParts, Query},
    http::request::Parts,
    response::{
        Html, IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use serde::Deserialize;
use tokio_stream::Stream;
use tracing::{instrument, warn};
use uuid::Uuid;

use crate::{
    adapters::{
        http::{
            app_state::AppState,
            auth::{AuthError, AuthenticatedUser},
            pages,
        },
        persistence::dashboard::DashboardPersistence,
    },
    app_error::AppResult,
    domain::monitoring::MonitoringService,
    entities::{
        company::Company,
        dashboard::{DashboardSnapshot, DashboardWindow, ProcessGauges},
        value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    use_cases::{company::CompanyUseCases, user::UserUseCases},
};

use super::ui::{load_account, load_scoped_company, workspace_user};

/// How often a connected dashboard re-reads. Slow enough that a room full of open tabs is not a
/// load generator, fast enough that a queue draining is visibly a queue draining.
const TICK: Duration = Duration::from_secs(5);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/dashboard", get(dashboard_page))
        .route("/ui/dashboard/panels", get(dashboard_panels_fragment))
        .route("/ui/dashboard/events", get(dashboard_stream))
}

#[derive(Debug, Clone, Deserialize)]
pub struct DashboardQuery {
    pub company_id: Option<Uuid>,
}

/// Everything a dashboard handler needs about its caller, resolved once.
struct Dashboard {
    company_use_cases: Arc<CompanyUseCases>,
    user_use_cases: Arc<UserUseCases>,
    dashboard_persistence: Arc<dyn DashboardPersistence>,
    monitoring: Arc<dyn MonitoringService>,
    config: Arc<AppConfig>,
    user_id: Uuid,
}

impl FromRequestParts<AppState> for Dashboard {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;

        Ok(Self {
            company_use_cases: state.company_use_cases.clone(),
            user_use_cases: state.user_use_cases.clone(),
            dashboard_persistence: state.dashboard_persistence.clone(),
            monitoring: state.monitoring.clone(),
            config: state.config.clone(),
            user_id: user.id,
        })
    }
}

/// Which rollup this caller gets, and the companies their switcher can offer.
struct DashboardScope {
    /// `None` means every company — the operator rollup.
    company: Option<Company>,
    companies: Vec<Company>,
    operator: bool,
}

impl DashboardScope {
    /// Decide the scope from who is asking.
    ///
    /// This is an authorization decision, so every fallible step propagates with `?`. Reading a
    /// database error as "not an operator" would fail closed silently, and reading it as "operator"
    /// would hand out every company's traffic — neither is a default worth having.
    async fn resolve(
        dashboard: &Dashboard,
        email: &EmailAddress,
        requested: Option<Uuid>,
    ) -> AppResult<Self> {
        let (companies, selected) =
            load_scoped_company(&dashboard.company_use_cases, dashboard.user_id, requested).await?;

        let operator = dashboard.config.is_operator(email);

        // An operator asking for one company in particular gets that company: the global rollup is
        // their default, not a cage, and drilling into a row of it is the obvious next move. The
        // lookup goes through `get_company` rather than the caller's own list, because an operator
        // is not usually a member of the companies they are watching — and it discloses nothing
        // new, since the global rollup already shows those companies' tasks by name.
        //
        // Everyone else stays pinned to `load_scoped_company`'s answer, which only ever returns a
        // company the caller belongs to.
        let company = match (operator, requested) {
            (true, Some(company_id)) => dashboard.company_use_cases.get_company(company_id).await?,
            (true, None) => None,
            (false, _) => selected,
        };

        Ok(Self {
            company,
            companies,
            operator,
        })
    }

    /// The filter the queries take: `None` scans every company.
    fn company_id(&self) -> Option<Uuid> {
        self.company.as_ref().map(|company| company.id)
    }

    fn view(&self) -> pages::DashboardScopeView<'_> {
        match &self.company {
            Some(company) => pages::DashboardScopeView::Company(company),
            None => pages::DashboardScopeView::Global,
        }
    }
}

/// One reading plus the process gauges that go beside it.
struct Reading {
    snapshot: DashboardSnapshot,
    process: ProcessGauges,
}

impl Dashboard {
    async fn read(&self, company: Option<Uuid>, window: DashboardWindow) -> AppResult<Reading> {
        Ok(Reading {
            snapshot: self
                .dashboard_persistence
                .dashboard_snapshot(company, window)
                .await?,
            process: ProcessGauges::from_stats_json(&self.monitoring.get_stats_json()),
        })
    }
}

/// Render the panels for a scope that has already been authorized.
///
/// Takes the pieces rather than a `Dashboard` so the SSE loop, which authorized before the stream
/// began, can call it on every tick without re-resolving anything.
fn render(
    scope: &DashboardScope,
    reading: &Reading,
    user: &pages::MailboxUser<'_>,
    window: DashboardWindow,
) -> String {
    pages::dashboard_panels(&pages::DashboardPage {
        user,
        scope: scope.view(),
        companies: &scope.companies,
        snapshot: &reading.snapshot,
        window,
        process: &reading.process,
    })
}

/// GET /ui/dashboard - The overview for the caller's scope (Protected).
#[instrument(skip(dashboard))]
async fn dashboard_page(
    dashboard: Dashboard,
    Query(query): Query<DashboardQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&dashboard.user_use_cases, dashboard.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let user = workspace_user(&account, &account_email);

    let scope = DashboardScope::resolve(&dashboard, &account_email, query.company_id).await?;

    // A user with no company and no operator grant has nothing to show; the shell's own empty
    // state says so rather than rendering a page of zeroes.
    if scope.company.is_none() && !scope.operator {
        return Ok(Html(pages::mailbox_no_company_page(&user)));
    }

    let window = DashboardWindow::last_hour();
    let reading = dashboard.read(scope.company_id(), window).await?;

    Ok(Html(pages::dashboard_page(&pages::DashboardPage {
        user: &user,
        scope: scope.view(),
        companies: &scope.companies,
        snapshot: &reading.snapshot,
        window,
        process: &reading.process,
    })))
}

/// GET /ui/dashboard/panels - The panels alone, for a client that is not streaming (Protected).
#[instrument(skip(dashboard))]
async fn dashboard_panels_fragment(
    dashboard: Dashboard,
    Query(query): Query<DashboardQuery>,
) -> AppResult<Response> {
    let account = load_account(&dashboard.user_use_cases, dashboard.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let user = workspace_user(&account, &account_email);

    let scope = DashboardScope::resolve(&dashboard, &account_email, query.company_id).await?;
    let window = DashboardWindow::last_hour();
    let reading = dashboard.read(scope.company_id(), window).await?;

    Ok(Html(render(&scope, &reading, &user, window)).into_response())
}

/// GET /ui/dashboard/events - The panels, re-rendered on a tick (Protected).
#[instrument(skip(dashboard))]
async fn dashboard_stream(
    dashboard: Dashboard,
    Query(query): Query<DashboardQuery>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Authorize once, here: the loop that follows re-reads under the scope decided now and never
    // re-checks it, exactly as the thread streams do.
    let account = load_account(&dashboard.user_use_cases, dashboard.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let scope = DashboardScope::resolve(&dashboard, &account_email, query.company_id).await?;
    let company = scope.company_id();
    let window = DashboardWindow::last_hour();

    let stream = async_stream::stream! {
        let user = workspace_user(&account, &account_email);
        let mut ticker = tokio::time::interval(TICK);
        // The first tick is immediate. Spend it: a reader that just connected wants the current
        // numbers, not the ones from five seconds hence.
        loop {
            ticker.tick().await;

            match dashboard.read(company, window).await {
                Ok(reading) => {
                    yield Ok(Event::default()
                        .event(pages::DASHBOARD_EVENT)
                        .data(render(&scope, &reading, &user, window)));
                }
                // One failed read is not a reason to drop a connection the reader would only
                // re-establish; skip the tick and try again on the next one.
                Err(error) => warn!(%error, "Dashboard read failed, skipping a tick"),
            }
        }
    };

    // Without the heartbeat an idle stream looks dead to the proxy in front of the app. The ticker
    // alone would carry it, but only while reads keep succeeding.
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
