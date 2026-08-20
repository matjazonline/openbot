//! `/ui/outbox` — the Outbox workspace: every email the company has handed to the transport, and
//! what became of it.
//!
//! The shell and the company scoping are shared with the Tasks workspace next door: chrome comes
//! from [`crate::adapters::http::pages::ui_shell`] and the company from
//! [`super::ui::load_scoped_company`]. Which page of the outbox a request means is one
//! [`OutboxFilter`], carried through every fragment so a click never lands the reader on a
//! different list than the one they were looking at.
//!
//! Read-only by design: the poller owns these rows, so the workspace links to the task that wrote
//! an email rather than offering buttons that would race the transport.

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
    entities::{
        channel::Channel,
        company::Company,
        outbox::{OutboxEntry, OutboxFilter},
        task::BackgroundTask,
        value_objects::EmailAddress,
    },
    use_cases::{
        channel::ChannelUseCases, company::CompanyUseCases, thread::ThreadUseCases,
        user::UserUseCases,
    },
};

use super::{
    task::deserialize_empty_string_as_none,
    ui::{load_account, load_scoped_company, workspace_user},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/outbox", get(outbox_page))
        .route("/ui/outbox/list", get(outbox_list_fragment))
        .route("/ui/outbox/{entry_id}", get(outbox_pane))
}

/// What the workspace has selected and filtered by, all optional so `/ui/outbox` alone is a valid
/// entry point.
///
/// The selects submit an empty option for "no filter", which is why the ids are read through
/// [`deserialize_empty_string_as_none`] rather than as plain `Option`s.
#[derive(Debug, Clone, Deserialize)]
pub struct OutboxQuery {
    pub company_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub entry_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub channel_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub status: Option<String>,
    pub sort: Option<String>,
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

impl OutboxQuery {
    /// The page of the outbox this request is asking for, with the paging clamped to what the list
    /// will serve.
    fn filter(&self) -> OutboxFilter {
        OutboxFilter::new(
            self.channel_id,
            self.status
                .as_deref()
                .and_then(|status| status.parse().ok()),
            self.sort.as_deref() == Some("asc"),
            self.page,
            self.limit,
        )
    }
}

const NO_SELECTION: &str = "Select an email to see whether it ever left the building.";

/// The use cases and the caller every Outbox handler starts from.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
    thread_use_cases: Arc<ThreadUseCases>,
    user_use_cases: Arc<UserUseCases>,
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
            user_id: user.id,
        })
    }
}

impl Workspace {
    /// The company a request is scoped to, always picked from the caller's own companies so a
    /// guessed `company_id` cannot reach another user's mail.
    async fn scoped_company(&self, company_id: Option<Uuid>) -> AppResult<Company> {
        let (_, company) =
            load_scoped_company(&self.company_use_cases, self.user_id, company_id).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    fn view<'a>(&'a self, company: &'a Company) -> OutboxView<'a> {
        OutboxView {
            channel_use_cases: &self.channel_use_cases,
            thread_use_cases: &self.thread_use_cases,
            user_id: self.user_id,
            company,
        }
    }
}

/// GET /ui/outbox - The Outbox workspace for the selected company / filters / email (Protected).
#[instrument(skip(workspace))]
async fn outbox_page(
    workspace: Workspace,
    Query(query): Query<OutboxQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&workspace.user_use_cases, workspace.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let workspace_user = workspace_user(&account, &account_email);

    let (companies, company) = load_scoped_company(
        &workspace.company_use_cases,
        workspace.user_id,
        query.company_id,
    )
    .await?;
    let Some(company) = company else {
        return Ok(Html(pages::mailbox_no_company_page(&workspace_user)));
    };

    let view = workspace.view(&company);
    let filter = query.filter();
    let channels = view.channels().await?;
    let (entries, has_next) = view.page(&filter).await?;

    // An email the filters exclude is still worth showing: the URL named it, and it may well be on
    // another page of this same list.
    let selected = match query.entry_id {
        Some(entry_id) => view.entry(entry_id).await?,
        None => None,
    };
    let pane_html = match &selected {
        Some(entry) => view.pane(entry, &channels).await?,
        None => pages::outbox_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline),
    };

    let list = view.list(
        &entries,
        has_next,
        &filter,
        selected.as_ref().map(|entry| entry.id),
    );
    Ok(Html(pages::outbox_page(&pages::OutboxPage {
        user: &workspace_user,
        companies: &companies,
        channels: &channels,
        list: &list,
        pane_html: &pane_html,
    })))
}

/// GET /ui/outbox/list - One filtered page of the outbox for the sidebar (Protected).
///
/// Answers with the address-bar URL as well as the list, so filtering and paging stay linkable
/// even though only the sidebar is swapped.
#[instrument(skip(workspace))]
async fn outbox_list_fragment(
    workspace: Workspace,
    Query(query): Query<OutboxQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let filter = query.filter();
    let (entries, has_next) = view.page(&filter).await?;

    let list = view.list(&entries, has_next, &filter, query.entry_id);
    Ok((
        [("HX-Push-Url", view.workspace_url(&filter, query.entry_id))],
        Html(pages::outbox_list(&list, pages::FragmentSwap::Inline)),
    )
        .into_response())
}

/// GET /ui/outbox/{entry_id} - One queued email's detail for the pane (Protected).
#[instrument(skip(workspace))]
async fn outbox_pane(
    workspace: Workspace,
    Path(entry_id): Path<Uuid>,
    Query(query): Query<OutboxQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let entry = view
        .entry(entry_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Outbox email not found".into()))?;
    let channels = view.channels().await?;

    Ok(Html(view.pane(&entry, &channels).await?))
}

/// Everything the workspace renders from, so each handler names its data once.
struct OutboxView<'a> {
    channel_use_cases: &'a ChannelUseCases,
    thread_use_cases: &'a Arc<ThreadUseCases>,
    user_id: Uuid,
    company: &'a Company,
}

impl OutboxView<'_> {
    async fn channels(&self) -> AppResult<Vec<Channel>> {
        self.channel_use_cases
            .list_company_channels(self.user_id, self.company.id)
            .await
    }

    /// One filtered page of the outbox, plus whether another follows it.
    async fn page(&self, filter: &OutboxFilter) -> AppResult<(Vec<OutboxEntry>, bool)> {
        let probed = self
            .thread_use_cases
            .get_task_persistence()
            .await
            .list_company_outbox_page(
                self.company.id,
                filter.channel_id,
                filter.status,
                filter.sort_asc,
                filter.offset(),
                filter.probe_limit(),
            )
            .await?;

        Ok(filter.split_probe(probed))
    }

    /// One email, but only when it really belongs to the company the request is scoped to — the id
    /// comes from the URL, so a guessed one must not reach another company's mail.
    async fn entry(&self, entry_id: Uuid) -> AppResult<Option<OutboxEntry>> {
        let entry = self
            .thread_use_cases
            .get_task_persistence()
            .await
            .get_outbox_entry(entry_id)
            .await?;

        Ok(entry.filter(|entry| entry.company_id == self.company.id))
    }

    /// The task that produced an email, scoped the same way the email is: a `task_id` that does
    /// not belong to this company renders as "unavailable" rather than leaking another's work.
    async fn task(&self, entry: &OutboxEntry) -> AppResult<Option<BackgroundTask>> {
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
        entries: &'a [OutboxEntry],
        has_next: bool,
        filter: &'a OutboxFilter,
        selected_entry_id: Option<Uuid>,
    ) -> pages::OutboxList<'a> {
        pages::OutboxList {
            company: self.company,
            entries,
            filter,
            has_next,
            selected_entry_id,
        }
    }

    fn workspace_url(&self, filter: &OutboxFilter, selected_entry_id: Option<Uuid>) -> String {
        format!(
            "/ui/outbox?{}",
            pages::outbox_query(self.company.id, filter, selected_entry_id)
        )
    }

    async fn pane(&self, entry: &OutboxEntry, channels: &[Channel]) -> AppResult<String> {
        // The task is joined in here, at render time: the transport never writes back into it, and
        // an email still sitting in the queue is often waiting on the task rather than the sender.
        let task = self.task(entry).await?;
        // An email outlives the channel it was queued for, so a missing one is normal and the pane
        // falls back to the name its payload recorded.
        let channel = entry
            .channel_id
            .and_then(|channel_id| channels.iter().find(|channel| channel.id == channel_id));

        Ok(pages::outbox_detail_pane(&pages::OutboxDetailPane {
            company_id: self.company.id,
            entry,
            task: task.as_ref(),
            channel,
        }))
    }
}
