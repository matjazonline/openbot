//! `/ui/dashboard` — the operational overview, for one company or for all of them.
//!
//! The scope is decided once, by [`DashboardScope::resolve`], and everything downstream takes the
//! `Option<Uuid>` it produces. A company administrator is pinned to a company they manage by
//! [`super::ui::load_managed_company`]; an operator named in `OPERATOR_EMAILS` gets `None`, which
//! the queries read as "every company".
//!
//! The stream is a ticker rather than an event subscription. These are sampled gauges — a queue
//! depth has no "changed" moment to subscribe to — so re-reading on an interval is both simpler and
//! a more honest description of what the page shows.

use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{FromRequestParts, Query},
    http::{HeaderValue, header::CACHE_CONTROL, request::Parts},
    response::{
        Html, IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use serde::Deserialize;
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
    app_error::{AppError, AppResult},
    domain::monitoring::MonitoringService,
    entities::{
        company::Company,
        company_member::CompanyMembership,
        dashboard::{DashboardSnapshot, DashboardWindow, ProcessGauges},
        runtime_metrics::{MachineIdentity, RuntimeMetricSnapshot},
        value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    services::database_query_health::{DatabaseQueryHealth, DatabaseQueryHealthService},
    services::runtime_metrics::RuntimeMetricPersistence,
    use_cases::{company::CompanyUseCases, user::UserUseCases},
};

use super::ui::{load_account, load_managed_company, workspace_user};

/// How often a connected dashboard re-reads. Slow enough that a room full of open tabs is not a
/// load generator, fast enough that a queue draining is visibly a queue draining.
const TICK: Duration = Duration::from_secs(5);
const PRIVATE_NO_STORE: &str = "private, no-store";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/dashboard", get(dashboard_page))
        .route("/ui/dashboard/panels", get(dashboard_panels_fragment))
        .route("/ui/dashboard/events", get(dashboard_stream))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DashboardScopeQuery {
    System,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
struct DashboardQuery {
    company_id: Option<Uuid>,
    /// `system` keeps an operator on the deployment-wide rollup while `company_id` remembers the
    /// company workspace they can switch back to. Other values retain the existing URL behavior.
    scope: Option<DashboardScopeQuery>,
    /// Which trailing range to report over, as a [`DashboardWindow`] slug.
    window: Option<String>,
}

impl DashboardQuery {
    /// The range this request asked for, or the default.
    ///
    /// An unrecognised slug falls back rather than failing: a bookmark from before a preset was
    /// renamed should still open the dashboard, and there is nothing a 400 would tell the reader
    /// that the highlighted range in the sidebar does not already say.
    fn window(&self) -> DashboardWindow {
        self.window
            .as_deref()
            .and_then(DashboardWindow::from_slug)
            .unwrap_or_default()
    }

    fn requests_system_scope(&self) -> bool {
        self.scope == Some(DashboardScopeQuery::System)
    }
}

/// Everything a dashboard handler needs about its caller, resolved once.
struct Dashboard {
    company_use_cases: Arc<CompanyUseCases>,
    user_use_cases: Arc<UserUseCases>,
    dashboard_persistence: Arc<dyn DashboardPersistence>,
    database_query_health: Arc<DatabaseQueryHealthService>,
    dashboard_sse_connections: Arc<AtomicU64>,
    runtime_metrics: Arc<dyn RuntimeMetricPersistence>,
    runtime_identity: MachineIdentity,
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
            database_query_health: state.database_query_health.clone(),
            dashboard_sse_connections: state.dashboard_sse_connections.clone(),
            runtime_metrics: state.runtime_metrics.clone(),
            runtime_identity: state.runtime_identity.clone(),
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
    /// The surrounding company workspace, retained while `company` is `None` so the scope control
    /// and navigation rail can return to it.
    selected_company: Option<Company>,
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
        query: &DashboardQuery,
    ) -> AppResult<Self> {
        let requested = query.company_id;
        let (companies, selected) =
            load_managed_company(&dashboard.company_use_cases, dashboard.user_id, requested)
                .await?;

        let operator = dashboard.config.is_operator(email);

        // An operator asking for one company in particular gets that company unless `scope=system`
        // explicitly selects the global rollup. The lookup goes through `get_company` rather than
        // the caller's own list, because an operator is not usually a member of the companies they
        // are watching — and it discloses nothing new, since the global rollup already shows those
        // companies' tasks by name.
        //
        // Everyone else stays pinned to `load_managed_company`'s answer, which only ever returns a
        // company the caller owns or administers.
        let selected_company = match (operator, requested) {
            (true, Some(company_id)) => dashboard.company_use_cases.get_company(company_id).await?,
            _ => selected,
        };
        let company = match (operator, requested, query.requests_system_scope()) {
            (true, _, true) | (true, None, false) => None,
            (true, Some(_), false) | (false, _, _) => selected_company.clone(),
        };

        Ok(Self {
            company,
            selected_company,
            companies,
            operator,
        })
    }

    /// The filter the queries take: `None` scans every company.
    fn company_id(&self) -> Option<Uuid> {
        self.company.as_ref().map(|company| company.id)
    }

    /// The persistence scope after authorization. Only an operator may request the global rollup.
    fn authorized_company_id(&self) -> AppResult<Option<Uuid>> {
        match (self.company_id(), self.operator) {
            (company_id @ Some(_), _) | (company_id @ None, true) => Ok(company_id),
            (None, false) => Err(AppError::NotFound("Company not found".into())),
        }
    }

    /// What the caller is to the company anchoring the rail, if there is one.
    fn membership(&self, user_id: Uuid) -> CompanyMembership {
        let Some(company) = &self.selected_company else {
            return CompanyMembership::None;
        };
        if company.user_id == user_id {
            CompanyMembership::Owner
        } else if self
            .companies
            .iter()
            .any(|managed| managed.id == company.id)
        {
            CompanyMembership::Admin
        } else {
            CompanyMembership::None
        }
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
    process: Option<ProcessGauges>,
    runtime: Option<RuntimeMetricSnapshot>,
    query_health: Option<DatabaseQueryHealth>,
    machine: MachineIdentity,
}

impl Dashboard {
    async fn read(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
        operator: bool,
    ) -> AppResult<Reading> {
        Ok(Reading {
            snapshot: self
                .dashboard_persistence
                .dashboard_snapshot(company, window)
                .await?,
            process: operator
                .then(|| ProcessGauges::from_stats_json(&self.monitoring.get_stats_json())),
            runtime: load_runtime_snapshot(
                self.runtime_metrics.as_ref(),
                &self.runtime_identity,
                window,
                operator,
            )
            .await?,
            query_health: load_query_health(self.database_query_health.as_ref(), operator, company)
                .await,
            machine: self.runtime_identity.clone(),
        })
    }
}

/// Query SQL only for the one authorized view that can render it. An operator looking at a
/// company dashboard is intentionally no different from that company's own administrator here.
async fn load_query_health(
    service: &DatabaseQueryHealthService,
    operator: bool,
    company: Option<Uuid>,
) -> Option<DatabaseQueryHealth> {
    if !operator || company.is_some() {
        return None;
    }
    Some(service.snapshot().await)
}

/// Keep the authorization branch outside the persistence implementation. A company user does not
/// issue a runtime query whose result is later hidden; the deployment-wide table is never touched.
async fn load_runtime_snapshot(
    persistence: &dyn RuntimeMetricPersistence,
    identity: &MachineIdentity,
    window: DashboardWindow,
    operator: bool,
) -> AppResult<Option<RuntimeMetricSnapshot>> {
    if !operator {
        return Ok(None);
    }
    persistence.snapshot(&identity.id, window).await.map(Some)
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
        process: reading.process.as_ref(),
        query_health: reading.query_health.as_ref(),
        runtime: reading.runtime.as_ref(),
        machine: &reading.machine,
    })
}

/// GET /ui/dashboard - The overview for the caller's scope (Protected).
#[instrument(skip(dashboard))]
async fn dashboard_page(
    dashboard: Dashboard,
    Query(query): Query<DashboardQuery>,
) -> AppResult<Response> {
    let account = load_account(&dashboard.user_use_cases, dashboard.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let user = workspace_user(&account, &account_email, &dashboard.config);

    let scope = DashboardScope::resolve(&dashboard, &account_email, &query).await?;
    let user = user.with_company_membership(scope.membership(dashboard.user_id));

    // A user with no company and no operator grant has nothing to show; the shell's own empty
    // state says so rather than rendering a page of zeroes.
    if scope.company.is_none() && !scope.operator {
        return Ok(no_store(
            Html(pages::mailbox_no_company_page(&user)).into_response(),
        ));
    }

    // No reading here: the shell carries a placeholder and asks for the panels itself, so the page
    // is not held open for the rollup's aggregates.
    Ok(no_store(
        Html(pages::dashboard_page(&pages::DashboardShell {
            user: &user,
            scope: scope.view(),
            selected_company: scope.selected_company.as_ref(),
            companies: &scope.companies,
            window: query.window(),
        }))
        .into_response(),
    ))
}

/// GET /ui/dashboard/panels - The panels alone, for a client that is not streaming (Protected).
#[instrument(skip(dashboard))]
async fn dashboard_panels_fragment(
    dashboard: Dashboard,
    Query(query): Query<DashboardQuery>,
) -> AppResult<Response> {
    let account = load_account(&dashboard.user_use_cases, dashboard.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let user = workspace_user(&account, &account_email, &dashboard.config);

    let scope = DashboardScope::resolve(&dashboard, &account_email, &query).await?;
    let company = scope.authorized_company_id()?;
    let window = query.window();
    let reading = dashboard.read(company, window, scope.operator).await?;

    Ok(no_store(
        Html(render(&scope, &reading, &user, window)).into_response(),
    ))
}

/// GET /ui/dashboard/events - The panels, re-rendered on a tick (Protected).
#[instrument(skip(dashboard))]
async fn dashboard_stream(
    dashboard: Dashboard,
    Query(query): Query<DashboardQuery>,
) -> AppResult<Response> {
    // Authorize once, here: the loop that follows re-reads under the scope decided now and never
    // re-checks it, exactly as the thread streams do.
    let account = load_account(&dashboard.user_use_cases, dashboard.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let scope = DashboardScope::resolve(&dashboard, &account_email, &query).await?;
    let company = scope.authorized_company_id()?;
    let operator = scope.operator;
    let window = query.window();

    let stream = async_stream::stream! {
        let _connection = DashboardSseConnection::new(
            dashboard.dashboard_sse_connections.clone(),
            dashboard.monitoring.clone(),
        );
        let user = workspace_user(&account, &account_email, &dashboard.config);
        let mut ticker = tokio::time::interval(TICK);
        // The first tick is immediate. Spend it: a reader that just connected wants the current
        // numbers, not the ones from five seconds hence.
        loop {
            ticker.tick().await;

            match dashboard.read(company, window, operator).await {
                Ok(reading) => {
                    yield Ok::<Event, Infallible>(Event::default()
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
    Ok(no_store(
        Sse::new(stream)
            .keep_alive(KeepAlive::default())
            .into_response(),
    ))
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(PRIVATE_NO_STORE));
    response
}

struct DashboardSseConnection {
    active: Arc<AtomicU64>,
    monitoring: Arc<dyn MonitoringService>,
}

impl DashboardSseConnection {
    fn new(active: Arc<AtomicU64>, monitoring: Arc<dyn MonitoringService>) -> Self {
        let current = active.fetch_add(1, Ordering::AcqRel) + 1;
        monitoring.record_gauge("active_dashboard_sse_connections", current as f64, &[]);
        Self { active, monitoring }
    }
}

impl Drop for DashboardSseConnection {
    fn drop(&mut self) {
        let previous = self
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                Some(value.saturating_sub(1))
            })
            .unwrap_or(0);
        self.monitoring.record_gauge(
            "active_dashboard_sse_connections",
            previous.saturating_sub(1) as f64,
            &[],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::{
        adapters::monitoring::InMemoryMonitor,
        entities::runtime_metrics::{
            MachineId, RuntimeMetricObservation, RuntimeMetricSample, RuntimeMetricSnapshot,
        },
        services::{
            database_query_health::{
                DatabaseQueryHealthError, DatabaseQueryHealthPersistence,
                DatabaseQueryHealthSnapshot, QueryHealthFailureCategory,
            },
            runtime_metrics::RuntimeMetricProbeError,
        },
    };

    #[derive(Default)]
    struct RuntimeReads(AtomicUsize);

    #[async_trait]
    impl RuntimeMetricPersistence for RuntimeReads {
        async fn probe_and_record(
            &self,
            _observation: RuntimeMetricObservation,
            _failed: &[RuntimeMetricSample],
        ) -> Result<RuntimeMetricSample, RuntimeMetricProbeError> {
            unreachable!("the route read test never samples")
        }

        async fn snapshot(
            &self,
            _machine_id: &MachineId,
            _window: DashboardWindow,
        ) -> AppResult<RuntimeMetricSnapshot> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(RuntimeMetricSnapshot::default())
        }

        async fn prune_before(&self, _cutoff: DateTime<Utc>) -> AppResult<u64> {
            unreachable!("the route read test never prunes")
        }
    }

    #[derive(Default)]
    struct QueryReads(AtomicUsize);

    #[async_trait]
    impl DatabaseQueryHealthPersistence for QueryReads {
        async fn database_query_health(
            &self,
        ) -> Result<DatabaseQueryHealthSnapshot, DatabaseQueryHealthError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(DatabaseQueryHealthError {
                category: QueryHealthFailureCategory::ExtensionUnavailable,
            })
        }
    }

    #[tokio::test]
    async fn infrastructure_is_not_queried_until_operator_authorization_succeeds() {
        let persistence = RuntimeReads::default();
        let identity = MachineIdentity {
            id: MachineId::new("serving-machine"),
            region: None,
        };

        let hidden =
            load_runtime_snapshot(&persistence, &identity, DashboardWindow::last_hour(), false)
                .await
                .unwrap();
        assert!(hidden.is_none());
        assert_eq!(persistence.0.load(Ordering::SeqCst), 0);

        let visible =
            load_runtime_snapshot(&persistence, &identity, DashboardWindow::last_hour(), true)
                .await
                .unwrap();
        assert!(visible.is_some());
        assert_eq!(persistence.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn query_health_is_read_only_for_an_operator_in_system_scope() {
        let persistence = Arc::new(QueryReads::default());
        let service = DatabaseQueryHealthService::new(persistence.clone());

        assert!(load_query_health(&service, false, None).await.is_none());
        assert!(
            load_query_health(&service, true, Some(Uuid::new_v4()))
                .await
                .is_none()
        );
        assert_eq!(persistence.0.load(Ordering::SeqCst), 0);

        assert!(matches!(
            load_query_health(&service, true, None).await,
            Some(DatabaseQueryHealth::Unavailable(
                QueryHealthFailureCategory::ExtensionUnavailable
            ))
        ));
        assert_eq!(persistence.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dashboard_responses_are_private_and_not_stored() {
        let response = no_store(Html("dashboard".to_string()).into_response());
        assert_eq!(
            response.headers().get(CACHE_CONTROL).unwrap(),
            PRIVATE_NO_STORE
        );
    }

    #[test]
    fn dashboard_connection_gauge_increments_and_cleans_up_on_drop() {
        let active = Arc::new(AtomicU64::new(0));
        let monitor = Arc::new(InMemoryMonitor::new());
        {
            let _first = DashboardSseConnection::new(active.clone(), monitor.clone());
            let _second = DashboardSseConnection::new(active.clone(), monitor.clone());
            assert_eq!(active.load(Ordering::SeqCst), 2);
            assert_eq!(
                monitor.get_stats_json()["gauges"]["active_dashboard_sse_connections"],
                2.0
            );
        }
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(
            monitor.get_stats_json()["gauges"]["active_dashboard_sse_connections"],
            0.0
        );
    }

    #[test]
    fn only_an_operator_may_resolve_the_global_dashboard_scope() {
        let company_user_scope = DashboardScope {
            company: None,
            selected_company: None,
            companies: Vec::new(),
            operator: false,
        };
        assert!(matches!(
            company_user_scope.authorized_company_id(),
            Err(AppError::NotFound(_))
        ));

        let operator_scope = DashboardScope {
            operator: true,
            ..company_user_scope
        };
        assert_eq!(operator_scope.authorized_company_id().unwrap(), None);
    }
}
