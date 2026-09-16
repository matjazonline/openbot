//! A single throttled open-work snapshot per workspace tab.
use super::{
    live_updates::{Wake, task_count_wake_ups},
    ui::load_managed_company,
};
use crate::{
    adapters::http::{app_state::AppState, auth::AuthError, pages},
    app_error::{AppError, AppResult},
    application::task_counts::TaskCountsReader,
    domain::monitoring::MonitoringService,
    entities::user::Viewer,
    infra::events::MailboxEvents,
    use_cases::{channel::ChannelUseCases, company::CompanyUseCases},
};
use axum::{
    Router,
    extract::{FromRequestParts, Query},
    http::request::Parts,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
    routing::get,
};
use serde::Deserialize;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::time::{Instant, Interval, MissedTickBehavior};
use tokio_stream::{Stream, StreamExt};
use uuid::Uuid;

const TASK_COUNTS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const TASK_COUNTS_RECHECK_INTERVAL: Duration = Duration::from_secs(60);

pub fn router() -> Router<AppState> {
    Router::new().route("/ui/task-counts/events", get(task_counts_stream))
}

struct Workspace {
    companies: Arc<CompanyUseCases>,
    channels: Arc<ChannelUseCases>,
    reader: Arc<dyn TaskCountsReader>,
    monitoring: Arc<dyn MonitoringService>,
    events: MailboxEvents,
    viewer: Viewer,
}

impl FromRequestParts<AppState> for Workspace {
    type Rejection = AuthError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self {
            companies: state.company_use_cases.clone(),
            channels: state.channel_use_cases.clone(),
            reader: state.task_counts.clone(),
            monitoring: state.monitoring.clone(),
            events: state.events.clone(),
            viewer: Viewer::from_request_parts(parts, state).await?,
        })
    }
}

#[derive(Deserialize)]
struct CountsQuery {
    company_id: Uuid,
}

impl Workspace {
    async fn fragment(&self, company_id: Uuid) -> AppResult<String> {
        let channels = self
            .channels
            .list_managed_readable_channels(&self.viewer, company_id)
            .await?;
        let ids = channels
            .iter()
            .map(|channel| channel.id)
            .collect::<Vec<_>>();
        let snapshot = self.reader.task_counts(company_id, &ids).await?;
        Ok(pages::task_counts_fragment(&snapshot))
    }
}

async fn task_counts_stream(
    workspace: Workspace,
    Query(query): Query<CountsQuery>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let company_id = query.company_id;
    let (_, company) = load_managed_company(
        &workspace.companies,
        workspace.viewer.user_id,
        Some(company_id),
    )
    .await?;
    company.ok_or_else(|| AppError::NotFound("Company not found".into()))?;
    // Subscribe before the first read. Queued wakes survive reads, including the initial one.
    let mut changes = Box::pin(task_count_wake_ups(
        &workspace.events,
        "task-counts",
        company_id,
    ));
    let stream = async_stream::stream! {
        let mut scheduler = RefreshScheduler::new();
        let mut previous = None;
        while scheduler.next(&mut changes).await {
            let started = Instant::now();
            let result = workspace.fragment(company_id).await;
            scheduler.finished();
            let outcome = match &result {
                Ok(fragment) if previous.as_ref() == Some(fragment) => "unchanged",
                Ok(_) => "changed",
                Err(_) => "error",
            };
            workspace.monitoring.record_histogram("task_counts_refresh_duration_seconds",
                started.elapsed().as_secs_f64(), &[("outcome", outcome)]);
            match result {
                Ok(fragment) => {
                    if previous.as_ref() != Some(&fragment) {
                        let event = Event::default().event(pages::TASK_COUNTS_EVENT).data(&fragment);
                        previous = Some(fragment);
                        yield Ok(event);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, %company_id, "Task counts stream query failed");
                    break;
                }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Completion-based throttle, not a debounce. Only this stream owns this cadence.
struct RefreshScheduler {
    pending: bool,
    eligible: Instant,
    recheck: Interval,
}

impl RefreshScheduler {
    fn new() -> Self {
        let now = Instant::now();
        let mut recheck = tokio::time::interval_at(
            now + TASK_COUNTS_RECHECK_INTERVAL,
            TASK_COUNTS_RECHECK_INTERVAL,
        );
        recheck.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Self {
            pending: true,
            eligible: now,
            recheck,
        }
    }

    fn finished(&mut self) {
        self.eligible = Instant::now() + TASK_COUNTS_REFRESH_INTERVAL;
    }

    async fn next<S: Stream<Item = Wake> + Unpin>(&mut self, changes: &mut S) -> bool {
        loop {
            tokio::select! {
                // An endless stream of wakes must not starve an eligible pass or the recheck.
                biased;
                _ = tokio::time::sleep_until(self.eligible), if self.pending => {
                    self.pending = false;
                    return true;
                }
                _ = self.recheck.tick() => self.pending = true,
                wake = changes.next() => match wake {
                    Some(_) => self.pending = true,
                    None => return false,
                },
            }
        }
    }
}

#[cfg(test)]
#[path = "ui_task_counts_tests.rs"]
mod tests;
