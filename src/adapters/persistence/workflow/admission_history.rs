use super::*;
use crate::application::workflow::history::{KIND, MAX_HISTORY_ITEMS};

pub(super) async fn capture(db: &mut PgConnection, command: &PreparedAdmission) -> AppResult<()> {
    let RelatedAssociation::Thread {
        channel_id,
        thread_id,
    } = command.association()
    else {
        return Ok(());
    };
    if !command
        .binding()
        .bundle()
        .compiled()
        .graph()
        .definition()
        .steps
        .values()
        .any(|step| step.step_type.as_str() == KIND)
    {
        return Ok(());
    }
    // One statement gives the count and bounded member rows the same MVCC view.
    sqlx::query(
        "WITH counts AS ( \
           SELECT count(*)::bigint AS eligible FROM thread_messages AS member \
            WHERE member.company_id=$1 AND member.channel_id=$3 AND member.thread_id=$4 \
              AND NOT EXISTS (SELECT 1 FROM internal_notes AS note \
                   WHERE note.company_id=member.company_id AND note.message_id=member.message_id) \
         ), newest AS ( \
           SELECT member.message_id, member.created_at, member.id FROM thread_messages AS member \
            WHERE member.company_id=$1 AND member.channel_id=$3 AND member.thread_id=$4 \
              AND NOT EXISTS (SELECT 1 FROM internal_notes AS note \
                   WHERE note.company_id=member.company_id AND note.message_id=member.message_id) \
            ORDER BY member.created_at DESC, member.id DESC LIMIT $5 \
         ), stored_members AS ( \
           INSERT INTO workflow_history_members (company_id,run_id,channel_id,thread_id,message_id,ordinal) \
           SELECT $1,$2,$3,$4,message_id,row_number() OVER (ORDER BY created_at,id)::integer \
             FROM newest RETURNING 1 \
         ), stored_count AS ( \
           INSERT INTO workflow_history_snapshots (company_id,run_id,eligible_count) \
           SELECT $1,$2,eligible FROM counts RETURNING 1 \
         ) SELECT (SELECT count(*) FROM stored_members)+(SELECT count(*) FROM stored_count)",
    ).bind(command.company_id().as_uuid()).bind(command.proposed_run_id().as_uuid())
        .bind(channel_id.as_uuid()).bind(thread_id.as_uuid()).bind(MAX_HISTORY_ITEMS as i64)
        .execute(db).await?;
    Ok(())
}
