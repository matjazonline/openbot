use super::*;

/// Matches the schema ordinal ceiling. Fetch one extra membership so approaching
/// the bound rejects atomically rather than silently truncating a run's context.
pub(super) const MAX_HISTORY_MEMBERS: i64 = 10_000;

pub(super) async fn capture(db: &mut PgConnection, command: &PreparedAdmission) -> AppResult<()> {
    let RelatedAssociation::Thread {
        channel_id,
        thread_id,
    } = command.association()
    else {
        return Ok(());
    };
    // One statement = one committed MVCC membership snapshot. Timestamps only
    // order this finite set; they are never a visibility cutoff. The extra row
    // detects overflow before any membership insert, and the caller rolls back
    // the entire admission. FKs pin the exact committed memberships thereafter.
    let count: i64 = sqlx::query_scalar(
        "WITH captured AS MATERIALIZED ( \
           SELECT message_id, created_at, id FROM thread_messages \
           WHERE company_id = $1 AND channel_id = $3 AND thread_id = $4 \
           ORDER BY created_at, id LIMIT $5 \
         ), inserted AS ( \
           INSERT INTO workflow_history_members (company_id, run_id, channel_id, thread_id, message_id, ordinal) \
           SELECT $1, $2, $3, $4, message_id, (row_number() OVER (ORDER BY created_at, id))::integer \
           FROM captured WHERE (SELECT count(*) FROM captured) < $5 RETURNING ordinal \
         ) SELECT count(*)::bigint FROM captured",
    ).bind(command.company_id().as_uuid()).bind(command.proposed_run_id().as_uuid())
        .bind(channel_id.as_uuid()).bind(thread_id.as_uuid()).bind(MAX_HISTORY_MEMBERS + 1)
        .fetch_one(db).await?;
    if count > MAX_HISTORY_MEMBERS {
        return Err(AppError::BadRequest(
            "Workflow history membership limit exceeded".into(),
        ));
    }
    Ok(())
}
