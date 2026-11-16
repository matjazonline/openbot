use super::*;

pub(super) async fn defer_control_on(
    db: &mut PgConnection,
    fence: &ExecutionLease<NotificationEventId>,
    deadline: DateTime<Utc>,
) -> AppResult<bool> {
    let changed=sqlx::query("UPDATE notification_events SET \
        status=CASE WHEN attempt_count>=max_attempts THEN 'dead_letter' ELSE 'pending' END, \
        available_at=$4,last_error_class='stale_projection',last_error_detail='Escalation control is not due', \
        execution_id=NULL,owner_worker_id=NULL,locked_at=NULL,lock_expires_at=NULL \
        WHERE id=$1 AND status='processing' AND execution_id=$2 AND owner_worker_id=$3 AND lock_expires_at>clock_timestamp()")
        .bind(fence.row.as_uuid()).bind(fence.execution.as_uuid()).bind(fence.owner.as_uuid()).bind(deadline).execute(db).await?;
    Ok(changed.rows_affected() == 1)
}

pub(super) enum OwnedProjection {
    Current(Box<NotificationProjection>),
    Stale,
    LeaseLost,
}

pub(super) async fn owned_projection_on(
    db: &mut Transaction<'_, Postgres>,
    commit: &NotificationProjectionCommit<'_>,
) -> AppResult<OwnedProjection> {
    let Some(hint) = read_claimed_event(db, commit.fence).await? else {
        return Ok(OwnedProjection::LeaseLost);
    };
    let prepared =
        projection_lock::lock_on(db, &hint, projection_lock::ProjectionPurpose::Project).await?;
    let Some(event) = locked_event(db, commit.fence).await? else {
        return Ok(OwnedProjection::LeaseLost);
    };
    match prepared {
        projection_lock::LockedProjection::Current(current)
            if current.event == event && *current == *commit.expected =>
        {
            Ok(OwnedProjection::Current(current))
        }
        _ => Ok(OwnedProjection::Stale),
    }
}

pub(super) async fn finish_event_on(
    db: &mut PgConnection,
    fence: &ExecutionLease<NotificationEventId>,
) -> AppResult<bool> {
    let completed=sqlx::query("UPDATE notification_events SET status='projected',projected_at=CURRENT_TIMESTAMP, \
        execution_id=NULL,owner_worker_id=NULL,locked_at=NULL,lock_expires_at=NULL \
        WHERE id=$1 AND status='processing' AND execution_id=$2 AND owner_worker_id=$3 AND lock_expires_at>clock_timestamp()")
        .bind(fence.row.as_uuid()).bind(fence.execution.as_uuid()).bind(fence.owner.as_uuid()).execute(db).await?;
    Ok(completed.rows_affected() == 1)
}
