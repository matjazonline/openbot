//! Observe exact company queue ownership while retaining the original run/COMMIT witnesses.
use super::*;
use crate::app_error::AppResult;
use crate::application::workflow::lease::LeasePolicy;
use std::{future::Future, pin::Pin};

pub(super) struct Owners {
    pub first: i32,
    pub second: i32,
}

pub(super) async fn ordinary<Cancel, Completion>(
    observer: &mut sqlx::PgConnection,
    request: &ActionDispatchRequest,
    gate: i32,
    first: First,
    mut cancel: Pin<&mut Cancel>,
    mut completion: Pin<&mut Completion>,
) -> Owners
where
    Cancel: Future<Output = AppResult<CancelResult>>,
    Completion: Future<Output = AppResult<Option<CommittedWorkflowStep>>>,
{
    match first {
        First::Cancel => {
            let first = blocked(
                observer,
                gate,
                "SELECT id FROM workflow_executions",
                cancel.as_mut(),
            )
            .await;
            probe_run(observer, request, RunLock::Held).await;
            let second = tokio::select! {
                result = cancel.as_mut() => panic!("cancel escaped its execution gate: {result:?}"),
                pid = blocked(observer, first, "FROM companies", completion.as_mut()) => pid,
            };
            Owners { first, second }
        }
        First::Completion => {
            let first = blocked(
                observer,
                gate,
                "SELECT response_disposition,completed_at,committed_output",
                completion.as_mut(),
            )
            .await;
            probe_run(observer, request, RunLock::Held).await;
            let second = tokio::select! {
                result = completion.as_mut() => panic!("completion escaped its execution gate: {result:?}"),
                pid = blocked(observer, first, "FROM companies", cancel.as_mut()) => pid,
            };
            Owners { first, second }
        }
    }
}

pub(super) struct CommitObservation<'a> {
    pub observer: &'a mut sqlx::PgConnection,
    pub gate: &'a mut Gate,
    pub request: &'a ActionDispatchRequest,
    pub policy: LeasePolicy,
    pub done: &'a AtomicBool,
}

pub(super) struct ResolutionOwners {
    pub completing: i32,
    pub resolution: i32,
    pub cancel: i32,
}

pub(super) async fn resolution<Completion, Cancel>(
    context: CommitObservation<'_>,
    mut completion: Pin<&mut Completion>,
    mut cancel: Pin<&mut Cancel>,
) -> ResolutionOwners
where
    Completion: Future<Output = AppResult<Option<CommittedWorkflowStep>>>,
    Cancel: Future<Output = AppResult<CancelResult>>,
{
    let running = || !context.done.load(Ordering::SeqCst);
    let waiting: [&dyn Fn() -> bool; 1] = [&running];
    let completing = tokio::select! {
        result = completion.as_mut() => panic!("completion escaped its COMMIT gate: {result:?}"),
        pid = context.gate.stalled(context.observer, &waiting) => pid,
    };
    probe_run(context.observer, context.request, RunLock::Held).await;
    let resolution = tokio::select! {
        result = completion.as_mut() => panic!("completion escaped its COMMIT gate: {result:?}"),
        pid = resolution_behind(context.observer, completing, context.policy, &waiting) => pid,
    };
    // The resolution waits behind the completing owner, and cancel joins its company queue.
    let cancel = tokio::select! {
        result = completion.as_mut() => panic!("completion escaped its COMMIT gate: {result:?}"),
        pid = blocked(context.observer, resolution, "FROM companies", cancel.as_mut()) => pid,
    };
    ResolutionOwners {
        completing,
        resolution,
        cancel,
    }
}
