use std::convert::Infallible;

use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::{Html, Redirect, Sse, sse::Event},
    routing::{get, post},
};
use serde::Deserialize;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, pages},
    app_error::{AppError, AppResult},
    application::notification::NotificationPersistence,
    entities::{notification::NotificationPage, user::Viewer},
};

use super::{
    attention::{ReadContext, read_context},
    ui::{load_readable_company, workspace_user},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/companies/{company_id}/notifications",
            get(list_notifications),
        )
        .route(
            "/companies/{company_id}/notifications/{notification_id}/read",
            post(mark_read),
        )
        .route(
            "/companies/{company_id}/notifications/events",
            get(notification_events),
        )
        .route("/ui/notifications", get(notifications_page))
        .route("/ui/notifications/list", get(notifications_list))
        .route(
            "/ui/notifications/{notification_id}/open",
            get(follow_notification).post(open_notification),
        )
}

#[derive(Debug, Deserialize)]
struct NotificationQuery {
    company_id: Option<Uuid>,
}

async fn read_notifications(
    persistence: &dyn NotificationPersistence,
    viewer: &Viewer,
    context: &ReadContext,
) -> AppResult<NotificationPage> {
    persistence
        .list_notifications(
            context.access.company.id,
            viewer.user_id,
            context.principal_id,
            &context.visible_channel_ids,
        )
        .await
}

async fn list_notifications(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(company_id): Path<Uuid>,
) -> AppResult<Json<NotificationPage>> {
    let context = notification_read_context(&state, &viewer, company_id).await?;
    Ok(Json(
        read_notifications(state.notifications.as_ref(), &viewer, &context).await?,
    ))
}

async fn mark_read(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((company_id, notification_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<serde_json::Value>> {
    let context = notification_read_context(&state, &viewer, company_id).await?;
    state
        .notifications
        .open_notification(
            company_id,
            viewer.user_id,
            context.principal_id,
            &context.visible_channel_ids,
            notification_id.into(),
        )
        .await?;
    Ok(Json(serde_json::json!({ "read": true })))
}

async fn notifications_page(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<NotificationQuery>,
) -> AppResult<Html<String>> {
    let (_, selected) =
        load_readable_company(&state.company_use_cases, viewer.user_id, query.company_id).await?;
    let access = selected.ok_or_else(crate::use_cases::company::company_not_found)?;
    let context = notification_read_context(&state, &viewer, access.company.id).await?;
    let page = read_notifications(state.notifications.as_ref(), &viewer, &context).await?;
    let account = state
        .user_use_cases
        .get_user_by_id(viewer.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Account not found.".into()))?;
    let user = workspace_user(&account, &viewer.email, &state.config)
        .with_company_membership(access.membership);
    Ok(Html(pages::notifications_page(
        &pages::NotificationsPageView {
            user: &user,
            company: &access.company,
            page: &page,
        },
    )))
}

async fn notifications_list(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<NotificationQuery>,
) -> AppResult<Html<String>> {
    let (_, selected) =
        load_readable_company(&state.company_use_cases, viewer.user_id, query.company_id).await?;
    let access = selected.ok_or_else(crate::use_cases::company::company_not_found)?;
    let context = notification_read_context(&state, &viewer, access.company.id).await?;
    let page = read_notifications(state.notifications.as_ref(), &viewer, &context).await?;
    Ok(Html(pages::notification_list(&page)))
}

async fn open_notification(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(notification_id): Path<Uuid>,
) -> AppResult<Redirect> {
    let notification_id = notification_id.into();
    let company_id = state
        .notifications
        .notification_company_for_user(viewer.user_id, notification_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
    let context = notification_read_context(&state, &viewer, company_id).await?;
    let href = state
        .notifications
        .open_notification(
            company_id,
            viewer.user_id,
            context.principal_id,
            &context.visible_channel_ids,
            notification_id,
        )
        .await?;
    Ok(Redirect::to(&href))
}

async fn follow_notification(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(notification_id): Path<Uuid>,
) -> AppResult<Redirect> {
    let notification_id = notification_id.into();
    let company_id = state
        .notifications
        .notification_company_for_user(viewer.user_id, notification_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
    let context = notification_read_context(&state, &viewer, company_id).await?;
    let href = state
        .notifications
        .notification_href(
            company_id,
            viewer.user_id,
            context.principal_id,
            &context.visible_channel_ids,
            notification_id,
        )
        .await?;
    Ok(Redirect::to(&href))
}

async fn notification_events(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(company_id): Path<Uuid>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let receiver = state.events.subscribe();
    notification_read_context(&state, &viewer, company_id).await?;
    let companies = state.company_use_cases;
    let channels = state.channel_use_cases;
    let threads = state.thread_use_cases;
    let user_id = viewer.user_id;
    let stream = stream! {
        yield Ok(reconcile_event(company_id));
        let mut events = BroadcastStream::new(receiver);
        let mut periodic = tokio::time::interval(std::time::Duration::from_secs(30));
        periodic.tick().await;
        loop {
            tokio::select! {
                event = events.next() => match event {
                    Some(Ok(event)) if event.notification_scope(user_id).is_some_and(
                        |scope| scope.company_id == company_id
                    ) => yield Ok(reconcile_event(company_id)),
                    Some(Ok(_)) => {}
                    Some(Err(_)) => yield Ok(reconcile_event(company_id)),
                    None => break,
                },
                _ = periodic.tick() => match read_context(
                    &companies, &channels, &threads, &viewer, company_id
                ).await {
                    Ok(_) => yield Ok(reconcile_event(company_id)),
                    Err(_) => break,
                },
            }
        }
    };
    Ok(Sse::new(stream))
}

fn reconcile_event(company_id: Uuid) -> Event {
    Event::default()
        .event("reconcile")
        .json_data(serde_json::json!({ "company_id": company_id }))
        .expect("identifier event serializes")
}

async fn notification_read_context(
    state: &AppState,
    viewer: &Viewer,
    company_id: Uuid,
) -> AppResult<ReadContext> {
    read_context(
        &state.company_use_cases,
        &state.channel_use_cases,
        &state.thread_use_cases,
        viewer,
        company_id,
    )
    .await
}
