//! The run event stream the runs workspace reloads on (P08-D4 §D4.1 `workflow_run_changed`).
//! Wake-ups only, never state:
//! - `resync` first and on lag: every view reads again in full.
//! - `runs` and `run-<id>` for a run of this company on a channel the caller can see.
//! - `tick` every 30 s, once the caller's access has been checked again (the stream ends when it
//!   no longer holds). Step progress inside a run does not notify, so the pane of an unfinished
//!   run reloads on it; the list does not.
//!
//! The stream subscribes before it authorizes or reads anything, so a change between the page's
//! render and this subscription is covered by the first `resync`.
use super::*;
use crate::application::workflow::RelatedChannelId;
use crate::infra::events::{MailboxEvent, RunScope};
use async_stream::stream;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::convert::Infallible;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};

/// How often an open stream re-checks access and wakes the pane of an unfinished run.
const RECHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

pub(super) fn resync_event() -> Event {
    Event::default().event("resync").data("{}")
}

fn tick_event() -> Event {
    Event::default().event("tick").data("{}")
}

/// The events one run change wakes: the list, and the pane showing that run.
pub(super) fn run_events(run: Uuid) -> [Event; 2] {
    [
        Event::default().event("runs").data(run.to_string()),
        Event::default()
            .event(format!("run-{run}"))
            .data(run.to_string()),
    ]
}

/// Whether the caller may see a run of this scope: its channel, or the company for a run
/// without one.
async fn sees(ui: &Ui, company: Uuid, scope: &RunScope) -> AppResult<bool> {
    let association = match scope.channel_id {
        Some(channel) => RelatedAssociation::Channel(RelatedChannelId::new(channel)),
        None => RelatedAssociation::Company,
    };
    Visibility::new(&ui.engine, super::super::company(company), ui.actor()?)
        .allows(association)
        .await
}

/// GET /ui/workflow-runs/events — the stream described above.
pub(super) async fn stream(
    State(state): State<AppState>,
    ui: Ui,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let receiver = state.events.subscribe();
    let company = ui.company(query.company_id).await?.id;
    Ok(
        Sse::new(run_event_stream(receiver, ui, company, RECHECK_INTERVAL))
            .keep_alive(KeepAlive::default()),
    )
}

/// The stream's events for a receiver subscribed before `ui` was authorized for `company`,
/// re-checking that access every `recheck`.
pub(super) fn run_event_stream(
    receiver: tokio::sync::broadcast::Receiver<MailboxEvent>,
    ui: Ui,
    company: Uuid,
    recheck: std::time::Duration,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        yield Ok(resync_event());
        let mut events = BroadcastStream::new(receiver);
        let mut periodic = tokio::time::interval(recheck);
        periodic.tick().await;
        loop {
            tokio::select! {
                event = events.next() => match event {
                    Some(Ok(MailboxEvent::Resync)) | Some(Err(_)) => yield Ok(resync_event()),
                    Some(Ok(event)) => {
                        let Some(scope) = event.run_scope(company) else { continue };
                        match sees(&ui, company, &scope).await {
                            Ok(true) => {
                                for event in run_events(scope.run_id) {
                                    yield Ok(event);
                                }
                            }
                            Ok(false) => {}
                            Err(_) => break,
                        }
                    }
                    None => break,
                },
                _ = periodic.tick() => match ui.company(company).await {
                    Ok(_) => yield Ok(tick_event()),
                    Err(_) => break,
                },
            }
        }
    }
}
