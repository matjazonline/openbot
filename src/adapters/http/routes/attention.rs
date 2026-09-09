use std::{convert::Infallible, sync::Arc, time::Instant};

use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::{Html, Sse, sse::Event},
    routing::{get, post},
};
use serde::Deserialize;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, pages},
    app_error::{AppError, AppResult},
    application::attention::AttentionPersistence,
    domain::monitoring::MonitoringService,
    entities::{
        attention::{
            AttentionCursor, AttentionPage, AttentionQuery, AttentionSourceCommand,
            AttentionSourceKind, AttentionView, BusinessPriority, HandoffResolution,
            NewManualHandoff, ResolveHandoffCommand,
        },
        company::CompanyAccess,
        transport::PrincipalId,
        user::Viewer,
    },
    use_cases::{channel::ChannelUseCases, company::CompanyUseCases, thread::ThreadUseCases},
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
            "/companies/{company_id}/attention/handoffs",
            post(create_handoff),
        )
        .route(
            "/companies/{company_id}/attention/{source_kind}/{source_id}",
            post(change_source),
        )
        .route(
            "/companies/{company_id}/attention/handoffs/{handoff_id}/resolve",
            post(resolve_handoff),
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
    #[serde(default)]
    all_owned: bool,
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

pub(super) struct ReadContext {
    pub(super) access: CompanyAccess,
    pub(super) principal_id: PrincipalId,
    pub(super) visible_channel_ids: Vec<Uuid>,
}

pub(super) async fn read_context(
    companies: &CompanyUseCases,
    channels: &ChannelUseCases,
    threads: &ThreadUseCases,
    viewer: &Viewer,
    company_id: Uuid,
) -> AppResult<ReadContext> {
    let access = companies
        .company_access(viewer.user_id, company_id)
        .await?
        .filter(|access| access.membership.is_team())
        .ok_or_else(crate::use_cases::company::company_not_found)?;
    let principal_id = threads
        .principal_access_for_user(company_id, viewer.user_id)
        .await?
        .and_then(|context| context.principal_id)
        .ok_or_else(crate::use_cases::company::company_not_found)?;
    let visible_channel_ids = channels
        .list_readable_channels(viewer, company_id)
        .await?
        .into_iter()
        .map(|channel| channel.id)
        .collect();
    Ok(ReadContext {
        access,
        principal_id,
        visible_channel_ids,
    })
}

async fn read_page(
    attention: &dyn AttentionPersistence,
    monitoring: &dyn MonitoringService,
    context: &ReadContext,
    query: &ListQuery,
) -> AppResult<AttentionPage> {
    let view = query.view()?;
    if view == AttentionView::TeamWork && !context.access.membership.manages_company_operations() {
        return Err(crate::use_cases::company::company_not_found());
    }
    let cursor = query.cursor()?;
    let started = Instant::now();
    let page = attention
        .list_attention(AttentionQuery {
            company_id: context.access.company.id,
            principal_id: context.principal_id,
            visible_channel_ids: &context.visible_channel_ids,
            view,
            all_owned: query.all_owned && view == AttentionView::MyWork,
            cursor: cursor.as_ref(),
            limit: query.limit.unwrap_or(AttentionQuery::DEFAULT_LIMIT),
        })
        .await?;
    monitoring.record_histogram(
        "attention_query_duration",
        started.elapsed().as_secs_f64() * 1_000.0,
        &[("view", view_label(view))],
    );
    monitoring.record_gauge(
        "attention_working_set_size",
        page.working_set_size as f64,
        &[("view", view_label(view))],
    );
    if page.truncated {
        monitoring.increment_counter(
            "attention_working_set_truncated",
            1,
            &[("view", view_label(view))],
        );
    }
    Ok(page)
}

const fn view_label(view: AttentionView) -> &'static str {
    match view {
        AttentionView::MyWork => "my_work",
        AttentionView::Unassigned => "unassigned",
        AttentionView::TeamWork => "team_work",
    }
}

async fn list_attention(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<AttentionPage>> {
    let context = read_context(
        &state.company_use_cases,
        &state.channel_use_cases,
        &state.thread_use_cases,
        &viewer,
        company_id,
    )
    .await?;
    Ok(Json(
        read_page(
            state.attention.as_ref(),
            state.monitoring.as_ref(),
            &context,
            &query,
        )
        .await?,
    ))
}

async fn operational_summary(
    State(attention): State<Arc<dyn AttentionPersistence>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    State(monitoring): State<Arc<dyn MonitoringService>>,
    viewer: Viewer,
    Path(company_id): Path<Uuid>,
) -> AppResult<Json<crate::entities::attention::OperationalSummary>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    if !context.access.membership.manages_company_operations() {
        return Err(crate::use_cases::company::company_not_found());
    }
    let started = Instant::now();
    let summary = attention
        .operational_summary(company_id, &context.visible_channel_ids)
        .await?;
    monitoring.record_histogram(
        "operational_summary_query_duration",
        started.elapsed().as_secs_f64() * 1_000.0,
        &[],
    );
    Ok(Json(summary))
}

#[derive(Deserialize)]
struct HandoffBody {
    id: Uuid,
    command_id: Uuid,
    channel_id: Uuid,
    thread_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
    title: String,
    next_action: String,
    responsible_principal_id: Option<Uuid>,
    priority: Option<BusinessPriority>,
    due_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn create_handoff(
    State(attention): State<Arc<dyn AttentionPersistence>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path(company_id): Path<Uuid>,
    Json(body): Json<HandoffBody>,
) -> AppResult<Json<serde_json::Value>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    if !context.visible_channel_ids.contains(&body.channel_id) {
        return Err(AppError::NotFound("Channel not found.".into()));
    }
    let version = attention
        .create_handoff(NewManualHandoff {
            id: body.id,
            company_id,
            channel_id: body.channel_id,
            thread_id: body.thread_id,
            correlation_id: body.correlation_id.map(Into::into),
            title: body.title,
            next_action: body.next_action,
            responsible_principal_id: body.responsible_principal_id.map(PrincipalId::new),
            priority: body.priority.unwrap_or_default(),
            due_at: body.due_at,
            command_id: body.command_id,
            actor_principal_id: context.principal_id,
        })
        .await?;
    Ok(Json(
        serde_json::json!({ "id": body.id, "version": version }),
    ))
}

#[derive(Deserialize)]
struct ChangeBody {
    command_id: Uuid,
    expected_version: u64,
    priority: BusinessPriority,
    due_at: Option<chrono::DateTime<chrono::Utc>>,
    responsible_principal_id: Option<Uuid>,
}

async fn change_source(
    State(attention): State<Arc<dyn AttentionPersistence>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, source_kind, source_id)): Path<(Uuid, String, Uuid)>,
    Json(body): Json<ChangeBody>,
) -> AppResult<Json<serde_json::Value>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    let source_kind = source_kind
        .parse::<AttentionSourceKind>()
        .map_err(AppError::BadRequest)?;
    let version = attention
        .change_source_attributes(AttentionSourceCommand {
            company_id,
            source_kind,
            source_id,
            command_id: body.command_id,
            expected_version: body.expected_version,
            actor_principal_id: context.principal_id,
            visible_channel_ids: context.visible_channel_ids,
            priority: body.priority,
            due_at: body.due_at,
            responsible_principal_id: body.responsible_principal_id.map(PrincipalId::new),
        })
        .await?;
    Ok(Json(serde_json::json!({ "version": version })))
}

#[derive(Deserialize)]
struct ResolveBody {
    command_id: Uuid,
    expected_version: u64,
    resolution: HandoffResolution,
}

async fn resolve_handoff(
    State(attention): State<Arc<dyn AttentionPersistence>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path((company_id, handoff_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ResolveBody>,
) -> AppResult<Json<serde_json::Value>> {
    let context = read_context(&companies, &channels, &threads, &viewer, company_id).await?;
    let version = attention
        .resolve_handoff(ResolveHandoffCommand {
            company_id,
            handoff_id,
            command_id: body.command_id,
            expected_version: body.expected_version,
            actor_principal_id: context.principal_id,
            visible_channel_ids: context.visible_channel_ids,
            resolution: body.resolution,
        })
        .await?;
    Ok(Json(serde_json::json!({ "version": version })))
}

async fn attention_events(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(company_id): Path<Uuid>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Subscribe before authorization. The first reconcile event makes any earlier gap harmless,
    // while this order also retains a source change racing with the access query.
    let receiver = state.events.subscribe();
    let context = read_context(
        &state.company_use_cases,
        &state.channel_use_cases,
        &state.thread_use_cases,
        &viewer,
        company_id,
    )
    .await?;
    let companies = state.company_use_cases;
    let channels = state.channel_use_cases;
    let threads = state.thread_use_cases;
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
                    Some(Ok(event)) => if event.attention_scope(company_id).is_some_and(
                        |scope| visible_channel_ids.contains(&scope.channel_id)
                    ) {
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
                _ = periodic.tick() => match read_context(
                    &companies, &channels, &threads, &viewer, company_id
                ).await {
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
    #[serde(default)]
    all_owned: bool,
}

async fn work_page(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<WorkQuery>,
) -> AppResult<Html<String>> {
    let (company_list, selected) =
        load_readable_company(&state.company_use_cases, viewer.user_id, query.company_id).await?;
    let access = selected.ok_or_else(crate::use_cases::company::company_not_found)?;
    let context = read_context(
        &state.company_use_cases,
        &state.channel_use_cases,
        &state.thread_use_cases,
        &viewer,
        access.company.id,
    )
    .await?;
    let list_query = ListQuery {
        view: query.view,
        cursor: query.cursor,
        limit: query.limit,
        all_owned: query.all_owned,
    };
    let page = read_page(
        state.attention.as_ref(),
        state.monitoring.as_ref(),
        &context,
        &list_query,
    )
    .await?;
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
        all_owned: list_query.all_owned,
        page: &page,
        manager: access.membership.manages_company_operations(),
    })))
}

async fn work_list(
    State(attention): State<Arc<dyn AttentionPersistence>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(channels): State<Arc<ChannelUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    State(monitoring): State<Arc<dyn MonitoringService>>,
    viewer: Viewer,
    Query(query): Query<WorkQuery>,
) -> AppResult<Html<String>> {
    let (_, selected) = load_readable_company(&companies, viewer.user_id, query.company_id).await?;
    let access = selected.ok_or_else(crate::use_cases::company::company_not_found)?;
    let context = read_context(&companies, &channels, &threads, &viewer, access.company.id).await?;
    let list_query = ListQuery {
        view: query.view,
        cursor: query.cursor,
        limit: query.limit,
        all_owned: query.all_owned,
    };
    let view = list_query.view()?;
    let page = read_page(
        attention.as_ref(),
        monitoring.as_ref(),
        &context,
        &list_query,
    )
    .await?;
    Ok(Html(pages::attention_list(
        access.company.id,
        view,
        &page,
        list_query.all_owned,
    )))
}
