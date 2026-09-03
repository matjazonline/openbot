//! `/ui/deliveries` — the Deliveries workspace: every message the company has handed to a
//! transport, and what became of it.
//!
//! The shell and the company scoping are shared with the Tasks workspace next door: chrome comes
//! from [`crate::adapters::http::pages::ui_shell`] and the company from
//! [`super::ui::load_managed_company`]. Which page of the queue a request means is one
//! [`DeliveryFilter`], carried through every fragment so a click never lands the reader on a
//! different list than the one they were looking at.
//!
//! Read-only by design: the delivery worker owns these rows, so the workspace links to the task
//! that produced a message rather than offering buttons that would race the transport.

use std::sync::Arc;

use axum::{
    Router,
    extract::{FromRequestParts, Path, Query},
    http::request::Parts,
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{
        app_state::AppState,
        auth::{AuthError, AuthenticatedUser},
        pages,
    },
    app_error::{AppError, AppResult},
    domain::monitoring::{MonitoringService, record_pagination_observation},
    entities::{
        channel::Channel,
        company::Company,
        delivery::{DeliveryEntry, DeliveryFilter, DeliveryQuery as DeliveryPageQuery},
        task::BackgroundTask,
        transport::DeliveryId,
        value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    use_cases::{
        channel::ChannelUseCases, company::CompanyUseCases, delivery::DeliveryReader,
        thread::ThreadUseCases, user::UserUseCases,
    },
};

use super::{
    task::deserialize_empty_string_as_none,
    ui::{load_account, load_managed_company, managed_company_membership, workspace_user},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/deliveries", get(deliveries_page))
        .route("/ui/deliveries/list", get(delivery_list_fragment))
        .route("/ui/deliveries/{entry_id}", get(delivery_pane))
}

/// What the workspace has selected and filtered by, all optional so `/ui/deliveries` alone is a
/// valid entry point.
///
/// The selects submit an empty option for "no filter", which is why the ids are read through
/// [`deserialize_empty_string_as_none`] rather than as plain `Option`s.
#[derive(Debug, Clone, Deserialize)]
pub struct DeliveriesQuery {
    pub company_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub entry_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub channel_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub transport: Option<String>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub purpose: Option<String>,
    pub sort: Option<String>,
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

impl DeliveriesQuery {
    /// The page of the queue this request is asking for, with the paging clamped to what the list
    /// will serve.
    ///
    /// An unparseable vocabulary value drops that filter rather than failing the request: a stale
    /// bookmark naming a status this build no longer has should show the unfiltered list, not a
    /// 400.
    fn filter(&self) -> DeliveryFilter {
        DeliveryFilter::new(DeliveryPageQuery {
            channel_id: self.channel_id,
            status: self.status.as_deref().and_then(|value| value.parse().ok()),
            transport: self
                .transport
                .as_deref()
                .and_then(|value| value.parse().ok()),
            purpose: self.purpose.as_deref().and_then(|value| value.parse().ok()),
            sort_asc: self.sort.as_deref() == Some("asc"),
            page: self.page,
            limit: self.limit,
        })
    }
}

const NO_SELECTION: &str = "Select a delivery to see whether it ever left the building.";

/// The use cases and the caller every Deliveries handler starts from.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
    thread_use_cases: Arc<ThreadUseCases>,
    user_use_cases: Arc<UserUseCases>,
    deliveries: Arc<dyn DeliveryReader>,
    config: Arc<AppConfig>,
    monitoring: Arc<dyn MonitoringService>,
    user_id: Uuid,
}

impl FromRequestParts<AppState> for Workspace {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;

        Ok(Self {
            company_use_cases: state.company_use_cases.clone(),
            channel_use_cases: state.channel_use_cases.clone(),
            thread_use_cases: state.thread_use_cases.clone(),
            user_use_cases: state.user_use_cases.clone(),
            deliveries: state.deliveries.clone(),
            config: state.config.clone(),
            monitoring: state.monitoring.clone(),
            user_id: user.id,
        })
    }
}

impl Workspace {
    /// The company a request is scoped to, always picked from the caller's own companies so a
    /// guessed `company_id` cannot reach another user's mail.
    async fn scoped_company(&self, company_id: Option<Uuid>) -> AppResult<Company> {
        let (_, company) =
            load_managed_company(&self.company_use_cases, self.user_id, company_id).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    fn view<'a>(&'a self, company: &'a Company) -> DeliveriesView<'a> {
        DeliveriesView {
            channel_use_cases: &self.channel_use_cases,
            thread_use_cases: &self.thread_use_cases,
            deliveries: self.deliveries.as_ref(),
            monitoring: self.monitoring.as_ref(),
            user_id: self.user_id,
            company,
        }
    }
}

/// GET /ui/deliveries - The Deliveries workspace for the selected company, filters and delivery
/// (Protected).
#[instrument(skip(workspace))]
async fn deliveries_page(
    workspace: Workspace,
    Query(query): Query<DeliveriesQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&workspace.user_use_cases, workspace.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let workspace_user = workspace_user(&account, &account_email, &workspace.config);

    let (companies, company) = load_managed_company(
        &workspace.company_use_cases,
        workspace.user_id,
        query.company_id,
    )
    .await?;
    let Some(company) = company else {
        return Ok(Html(pages::mailbox_no_company_page(&workspace_user)));
    };
    let workspace_user = workspace_user
        .with_company_membership(managed_company_membership(&company, workspace.user_id));

    let view = workspace.view(&company);
    let filter = query.filter();
    let channels = view.channels().await?;
    let (entries, has_next) = view.page(&filter).await?;

    // A delivery the filters exclude is still worth showing: the URL named it, and it may well be
    // on another page of this same list.
    let selected = match query.entry_id {
        Some(entry_id) => view.entry(entry_id).await?,
        None => None,
    };
    let pane_html = match &selected {
        Some(entry) => view.pane(entry, &channels).await?,
        None => pages::delivery_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline),
    };

    let list = view.list(
        &entries,
        has_next,
        &filter,
        selected.as_ref().map(|entry| entry.id.as_uuid()),
    );
    Ok(Html(pages::deliveries_page(&pages::DeliveriesPage {
        user: &workspace_user,
        companies: &companies,
        channels: &channels,
        list: &list,
        pane_html: &pane_html,
    })))
}

/// GET /ui/deliveries/list - One filtered page of the queue for the sidebar (Protected).
///
/// Answers with the address-bar URL as well as the list, so filtering and paging stay linkable
/// even though only the sidebar is swapped.
#[instrument(skip(workspace))]
async fn delivery_list_fragment(
    workspace: Workspace,
    Query(query): Query<DeliveriesQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let filter = query.filter();
    let (entries, has_next) = view.page(&filter).await?;

    let list = view.list(&entries, has_next, &filter, query.entry_id);
    Ok((
        [("HX-Push-Url", view.workspace_url(&filter, query.entry_id))],
        Html(pages::delivery_list(&list, pages::FragmentSwap::Inline)),
    )
        .into_response())
}

/// GET /ui/deliveries/{entry_id} - One delivery's detail for the pane (Protected).
#[instrument(skip(workspace))]
async fn delivery_pane(
    workspace: Workspace,
    Path(entry_id): Path<Uuid>,
    Query(query): Query<DeliveriesQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let entry = view
        .entry(entry_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Delivery not found".into()))?;
    let channels = view.channels().await?;

    Ok(Html(view.pane(&entry, &channels).await?))
}

/// Everything the workspace renders from, so each handler names its data once.
struct DeliveriesView<'a> {
    channel_use_cases: &'a ChannelUseCases,
    thread_use_cases: &'a Arc<ThreadUseCases>,
    deliveries: &'a dyn DeliveryReader,
    monitoring: &'a dyn MonitoringService,
    user_id: Uuid,
    company: &'a Company,
}

impl DeliveriesView<'_> {
    async fn channels(&self) -> AppResult<Vec<Channel>> {
        self.channel_use_cases
            .list_company_channels(self.user_id, self.company.id)
            .await
    }

    /// One filtered page of the queue, plus whether another follows it.
    async fn page(&self, filter: &DeliveryFilter) -> AppResult<(Vec<DeliveryEntry>, bool)> {
        record_pagination_observation(self.monitoring, "deliveries", filter.offset());
        let probed = self
            .deliveries
            .list_company_deliveries(self.company.id, filter)
            .await?;

        Ok(filter.split_probe(probed))
    }

    /// One delivery, but only when it really belongs to the company the request is scoped to — the
    /// id comes from the URL, so a guessed one must not reach another company's mail.
    async fn entry(&self, entry_id: Uuid) -> AppResult<Option<DeliveryEntry>> {
        let entry = self
            .deliveries
            .get_delivery(DeliveryId::new(entry_id))
            .await?;

        Ok(entry.filter(|entry| entry.company_id == self.company.id))
    }

    /// The task that produced a delivery, scoped the same way the delivery is: a `task_id` that
    /// does not belong to this company renders as "unavailable" rather than leaking another's work.
    async fn task(&self, entry: &DeliveryEntry) -> AppResult<Option<BackgroundTask>> {
        let Some(task_id) = entry.task_id else {
            return Ok(None);
        };

        let task = self
            .thread_use_cases
            .get_task_persistence()
            .await
            .get_task_by_id(task_id)
            .await?;

        Ok(task.filter(|task| task.company_id == self.company.id))
    }

    fn list<'a>(
        &'a self,
        entries: &'a [DeliveryEntry],
        has_next: bool,
        filter: &'a DeliveryFilter,
        selected_entry_id: Option<Uuid>,
    ) -> pages::DeliveryList<'a> {
        pages::DeliveryList {
            company: self.company,
            entries,
            filter,
            has_next,
            selected_entry_id,
        }
    }

    fn workspace_url(&self, filter: &DeliveryFilter, selected_entry_id: Option<Uuid>) -> String {
        format!(
            "/ui/deliveries?{}",
            pages::delivery_query(self.company.id, filter, selected_entry_id)
        )
    }

    async fn pane(&self, entry: &DeliveryEntry, channels: &[Channel]) -> AppResult<String> {
        // The task is joined in here, at render time: the transport never writes back into it, and
        // a delivery still sitting in the queue is often waiting on the task rather than a sender.
        let task = self.task(entry).await?;
        let channel = channels
            .iter()
            .find(|channel| channel.id == entry.channel_id);

        Ok(pages::delivery_detail_pane(&pages::DeliveryDetailPane {
            company_id: self.company.id,
            entry,
            task: task.as_ref(),
            channel,
        }))
    }
}
