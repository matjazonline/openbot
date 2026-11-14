//! Private canonical ingress owns gates before accounts/company and never waits
//! for those rows while holding a gate (D3). Queue backoff owns a separate transaction.
use super::*;

#[path = "agent_notice_message.rs"]
mod message;
#[path = "agent_notice_route.rs"]
mod route;

type CommitResult = (
    NotificationProjectionCommitOutcome,
    NotificationProjectionResult,
);

enum Attempt {
    Complete,
    LeaseLost,
    Deferred(Deferral),
}

#[derive(Clone, Copy)]
enum Deferral {
    Busy,
    Stale,
}

impl Deferral {
    fn class(self) -> &'static str {
        match self {
            Self::Busy => "route_busy",
            Self::Stale => "route_stale",
        }
    }
}

fn outcome(kind: NotificationProjectionCommitOutcome) -> CommitResult {
    (kind, NotificationProjectionResult::default())
}

pub(in crate::adapters::persistence::notification) async fn commit_on(
    p: &PostgresPersistence,
    commit: NotificationProjectionCommit<'_>,
) -> AppResult<CommitResult> {
    let mut tx = p.pool.begin().await?;
    // Canonical ingress is a substantive nested writer; keep its future off
    // the notification worker's stock-stack frame.
    let attempted = Box::pin(attempt_on(&mut tx, &commit)).await;
    match attempted {
        Ok(Attempt::Complete) => {
            tx.commit().await?;
            Ok(outcome(NotificationProjectionCommitOutcome::Applied))
        }
        Ok(Attempt::LeaseLost) => {
            tx.rollback().await?;
            Ok(outcome(NotificationProjectionCommitOutcome::LeaseLost))
        }
        Ok(Attempt::Deferred(reason)) => {
            // Await the WHOLE rollback, including gates, before queue-row ownership.
            tx.rollback().await?;
            let changed = p
                .fail_notification_projection(NotificationProjectionFailure {
                    fence: commit.fence,
                    class: reason.class(),
                    detail: "Agent notice route is busy or changed before commit",
                })
                .await?;
            tracing::info!(event_id=%commit.fence.row,reason=reason.class(),owned=changed,"Agent notice projection deferred");
            Ok(outcome(if changed {
                NotificationProjectionCommitOutcome::Deferred
            } else {
                NotificationProjectionCommitOutcome::LeaseLost
            }))
        }
        Err(error) => {
            tx.rollback().await?;
            if matches!(error, AppError::DatabaseTimeout(_)) {
                let mut db = p.pool.acquire().await?;
                if super::super::read_claimed_event(&mut db, commit.fence)
                    .await?
                    .is_none()
                {
                    return Ok(outcome(NotificationProjectionCommitOutcome::LeaseLost));
                }
            }
            Err(error)
        }
    }
}

async fn attempt_on(
    tx: &mut Transaction<'_, Postgres>,
    commit: &NotificationProjectionCommit<'_>,
) -> AppResult<Attempt> {
    let Some(event) = super::super::read_claimed_event(tx, commit.fence).await? else {
        return Ok(Attempt::LeaseLost);
    };
    // Gate waits are bounded by both the queue's actual lease and the ordinary
    // persistence statement ceiling. No event row lock is held during the wait.
    sqlx::query(
        "SELECT set_config('statement_timeout',COALESCE((SELECT LEAST(20000,GREATEST(1, \
        ceil(extract(epoch FROM(lock_expires_at-clock_timestamp()))*1000)::bigint)) \
        FROM notification_events WHERE id=$1 AND execution_id=$2 AND owner_worker_id=$3 AND status='processing'),1)::text,true)",
    )
    .bind(commit.fence.row.as_uuid())
    .bind(commit.fence.execution.as_uuid())
    .bind(commit.fence.owner.as_uuid())
    .execute(&mut **tx)
    .await?;
    let NotificationDisposition::Agent { principal_id, .. } = commit.expected.disposition else {
        return Ok(Attempt::Deferred(Deferral::Stale));
    };
    let key = commit
        .agent_message_key
        .ok_or_else(|| AppError::Internal("An agent notice needs its minted message key".into()))?;
    let hint = route::read_on(tx, &event, principal_id).await?;
    match route::lock_on(tx, event.company_id, hint.as_ref()).await? {
        route::Ownership::Busy => return Ok(Attempt::Deferred(Deferral::Busy)),
        route::Ownership::Stale => return Ok(Attempt::Deferred(Deferral::Stale)),
        route::Ownership::Owned => {}
    }
    if route::read_on(tx, &event, principal_id).await? != hint {
        return Ok(Attempt::Deferred(Deferral::Stale));
    }
    lock_source_on(tx, &event).await?;
    if super::super::locked_event(tx, commit.fence)
        .await?
        .is_none()
    {
        return Ok(Attempt::LeaseLost);
    }
    if super::super::resolve_projection_on(tx, &event).await? != *commit.expected {
        return Ok(Attempt::Deferred(Deferral::Stale));
    }
    let source = notice_on(tx, &event)
        .await?
        .ok_or_else(|| AppError::Internal("Missing owned agent notice".into()))?;
    match hint {
        Some(route) if route::eligible_on(tx, &event, &source, &route).await? => {
            Box::pin(message::commit_on(tx, &event, &source, &route, key)).await?;
        }
        _ => fail_route_on(tx, &event, &source).await?,
    }
    if !super::super::control::finish_event_on(tx, commit.fence).await? {
        return Ok(Attempt::LeaseLost);
    }
    Ok(Attempt::Complete)
}

async fn fail_route_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
    source: &NoticeSource,
) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_escalation_recipient_failures(company_id,notice_id,failure_kind,reason_code) \
        VALUES($1,$2,'route','escalation.notification_route_unavailable') ON CONFLICT(company_id,notice_id,failure_kind) DO NOTHING")
        .bind(event.company_id).bind(event.source_id).execute(&mut *db).await?;
    super::super::escalation_failures::wake_failed_assignments_on(
        db,
        CompanyId::new(event.company_id),
        Some(EscalationId::new(source.case_id)),
    )
    .await
}
