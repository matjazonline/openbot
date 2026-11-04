use std::{convert::Infallible, sync::Arc};

use crate::infra::events::MailboxEvent;
use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::{Html, Sse, sse::Event},
    routing::get,
};
use serde::Deserialize;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, pages},
    app_error::{AppError, AppResult},
    application::attention::{AttentionListRequest, AttentionUseCases},
    entities::{
        attention::{AttentionCursor, AttentionPage, AttentionView},
        user::Viewer,
    },
    use_cases::company::CompanyUseCases,
};

use super::ui::{load_readable_company, workspace_user};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/companies/{company_id}/attention", get(list_attention))
        .route(
            "/companies/{company_id}/attention/summary",
            get(operational_summary),
        )
        .route(
            "/companies/{company_id}/attention/events",
            get(attention_events),
        )
        .route("/ui/work", get(work_page))
        .route("/ui/work/list", get(work_list))
}

#[derive(Debug, Clone, Deserialize)]
struct ListQuery {
    view: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

impl ListQuery {
    fn view(&self) -> AppResult<AttentionView> {
        match self.view.as_deref().unwrap_or("my_work") {
            "my_work" => Ok(AttentionView::MyWork),
            "unassigned" => Ok(AttentionView::Unassigned),
            "team_work" => Ok(AttentionView::TeamWork),
            _ => Err(AppError::BadRequest(
                "Attention view must be my_work, unassigned, or team_work.".into(),
            )),
        }
    }

    fn cursor(&self) -> AppResult<Option<AttentionCursor>> {
        self.cursor
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(AppError::BadRequest)
    }
}

/// The request's view, cursor and size as the use case takes them.
fn list_request<'a>(
    query: &ListQuery,
    cursor: Option<&'a AttentionCursor>,
) -> AppResult<AttentionListRequest<'a>> {
    Ok(AttentionListRequest {
        view: query.view()?,
        cursor,
        limit: query.limit,
    })
}

async fn list_attention(
    State(attention): State<Arc<AttentionUseCases>>,
    viewer: Viewer,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<AttentionPage>> {
    let cursor = query.cursor()?;
    let listing = attention
        .list(&viewer, company_id, list_request(&query, cursor.as_ref())?)
        .await?;
    Ok(Json(listing.page))
}

async fn operational_summary(
    State(attention): State<Arc<AttentionUseCases>>,
    viewer: Viewer,
    Path(company_id): Path<Uuid>,
) -> AppResult<Json<crate::entities::attention::OperationalSummary>> {
    Ok(Json(
        attention.operational_summary(&viewer, company_id).await?,
    ))
}

/// Whether an event changes what this viewer's work list may show: a resync, a delivery or other
/// attention source in a visible channel, or a workflow run in a visible channel or company-wide
/// (P08-D4 §D4.2).
fn wakes_attention(event: &MailboxEvent, company_id: Uuid, visible_channel_ids: &[Uuid]) -> bool {
    event.is_resync()
        || event
            .attention_scope(company_id)
            .is_some_and(|scope| visible_channel_ids.contains(&scope.channel_id))
        || event.run_scope(company_id).is_some_and(|scope| {
            scope
                .channel_id
                .is_none_or(|channel| visible_channel_ids.contains(&channel))
        })
}

async fn attention_events(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(company_id): Path<Uuid>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Subscribe before authorization. The first reconcile event makes any earlier gap harmless,
    // while this order also retains a source change racing with the access query.
    let receiver = state.events.subscribe();
    let attention = state.attention_use_cases.clone();
    let context = attention.read_context(&viewer, company_id).await?;
    let stream = stream! {
        yield Ok(Event::default().event("reconcile").json_data(
            serde_json::json!({ "company_id": company_id })
        ).expect("identifier event serializes"));
        let mut events = BroadcastStream::new(receiver);
        let mut periodic = tokio::time::interval(std::time::Duration::from_secs(30));
        let mut visible_channel_ids = context.visible_channel_ids;
        periodic.tick().await;
        loop {
            tokio::select! {
                event = events.next() => match event {
                    Some(Ok(event)) => if wakes_attention(&event, company_id, &visible_channel_ids) {
                        yield Ok(Event::default().event("reconcile").json_data(
                            serde_json::json!({ "company_id": company_id })
                        )
                            .expect("identifier event serializes"));
                    },
                    Some(Err(_)) => yield Ok(Event::default().event("reconcile").json_data(
                        serde_json::json!({ "company_id": company_id })
                    ).expect("identifier event serializes")),
                    None => break,
                },
                _ = periodic.tick() => match attention.read_context(&viewer, company_id).await {
                    Ok(context) => {
                        visible_channel_ids = context.visible_channel_ids;
                        yield Ok(Event::default().event("reconcile").json_data(
                            serde_json::json!({ "company_id": company_id })
                        ).expect("identifier event serializes"));
                    }
                    Err(_) => break,
                },
            }
        }
    };
    Ok(Sse::new(stream))
}

#[derive(Debug, Deserialize)]
struct WorkQuery {
    company_id: Option<Uuid>,
    view: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

async fn work_page(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<WorkQuery>,
) -> AppResult<Html<String>> {
    let (company_list, selected) =
        load_readable_company(&state.company_use_cases, viewer.user_id, query.company_id).await?;
    let access = selected.ok_or_else(crate::use_cases::company::company_not_found)?;
    let list_query = ListQuery {
        view: query.view,
        cursor: query.cursor,
        limit: query.limit,
    };
    let cursor = list_query.cursor()?;
    let listing = state
        .attention_use_cases
        .list(
            &viewer,
            access.company.id,
            list_request(&list_query, cursor.as_ref())?,
        )
        .await?;
    let page = listing.page;
    let account = state
        .user_use_cases
        .get_user_by_id(viewer.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Account not found.".into()))?;
    let user = workspace_user(&account, &viewer.email, &state.config)
        .with_company_membership(access.membership);
    Ok(Html(pages::attention_page(&pages::AttentionPageView {
        user: &user,
        companies: &company_list,
        company: &access.company,
        view: list_query.view()?,
        page: &page,
        manager: access.membership.manages_company_operations(),
    })))
}

async fn work_list(
    State(attention): State<Arc<AttentionUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    viewer: Viewer,
    Query(query): Query<WorkQuery>,
) -> AppResult<Html<String>> {
    let (_, selected) = load_readable_company(&companies, viewer.user_id, query.company_id).await?;
    let access = selected.ok_or_else(crate::use_cases::company::company_not_found)?;
    let list_query = ListQuery {
        view: query.view,
        cursor: query.cursor,
        limit: query.limit,
    };
    let view = list_query.view()?;
    let cursor = list_query.cursor()?;
    let listing = attention
        .list(
            &viewer,
            access.company.id,
            list_request(&list_query, cursor.as_ref())?,
        )
        .await?;
    Ok(Html(pages::attention_list(
        access.company.id,
        view,
        &listing.page,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::events::RunScope;

    fn run(company_id: Uuid, channel_id: Option<Uuid>) -> MailboxEvent {
        MailboxEvent::WorkflowRunChanged(RunScope {
            company_id,
            channel_id,
            thread_id: None,
            run_id: Uuid::new_v4(),
        })
    }

    /// p08 D4.2 freshness: a run change wakes the list when its channel is readable or it has
    /// none; another channel's or company's run does not; a resync always does.
    #[test]
    fn a_run_change_wakes_the_work_list_it_could_change() {
        let company = Uuid::new_v4();
        let readable = Uuid::new_v4();
        let visible = [readable];
        assert!(wakes_attention(
            &run(company, Some(readable)),
            company,
            &visible
        ));
        assert!(wakes_attention(&run(company, None), company, &visible));
        assert!(!wakes_attention(
            &run(company, Some(Uuid::new_v4())),
            company,
            &visible
        ));
        assert!(!wakes_attention(
            &run(Uuid::new_v4(), Some(readable)),
            company,
            &visible
        ));
        assert!(wakes_attention(&MailboxEvent::Resync, company, &visible));
    }
}

#[cfg(test)]
#[path = "attention_authorization_tests.rs"]
mod authorization_tests;
