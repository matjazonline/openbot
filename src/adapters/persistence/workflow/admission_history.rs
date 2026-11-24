use super::*;
use crate::application::workflow::history::{KIND, MAX_HISTORY_ITEMS};

pub(super) async fn capture(db: &mut PgConnection, command: &PreparedAdmission) -> AppResult<()> {
    capture_for(
        db,
        command.company_id(),
        command.proposed_run_id(),
        command.association(),
        command.binding(),
    )
    .await
}

/// The history a run of `binding` on `association` sees: its thread's messages at the run's
/// ceiling, frozen at admission. A channel call's child captures its fresh thread the same way
/// (gate AWC-P03-D1 r1 D2).
pub(super) async fn capture_for(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    association: RelatedAssociation,
    binding: &crate::application::workflow::binding::ConfiguredBinding,
) -> AppResult<()> {
    let RelatedAssociation::Thread { target, thread_id } = association else {
        return Ok(());
    };
    if !binding
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
        "WITH eligible AS ( \
           SELECT member.message_id,member.created_at,member.id FROM thread_messages AS member \
           JOIN messages AS message ON (message.company_id,message.id)=(member.company_id,member.message_id) \
           JOIN workflow_runs AS run ON run.company_id=message.company_id AND run.id=$2 \
           WHERE member.company_id=$1 AND member.channel_id IS NOT DISTINCT FROM $3::uuid AND member.thread_id=$4 \
             AND member.principal_id IS NOT DISTINCT FROM $6::uuid \
             AND message.access_level<=run.knowledge_ceiling \
             AND NOT EXISTS (SELECT 1 FROM internal_notes AS note \
                 WHERE note.company_id=member.company_id AND note.message_id=member.message_id) \
         ), counts AS ( SELECT count(*)::bigint AS eligible FROM eligible \
         ), newest AS ( \
           SELECT message_id,created_at,id FROM eligible \
           ORDER BY created_at DESC,id DESC LIMIT $5 \
         ), stored_members AS ( \
           INSERT INTO workflow_history_members (company_id,run_id,channel_id,thread_id,message_id,ordinal,principal_id) \
           SELECT $1,$2,$3,$4,message_id,row_number() OVER (ORDER BY created_at,id)::integer,$6 \
             FROM newest RETURNING 1 \
         ), stored_count AS ( \
           INSERT INTO workflow_history_snapshots (company_id,run_id,eligible_count) \
           SELECT $1,$2,eligible FROM counts RETURNING 1 \
         ) SELECT (SELECT count(*) FROM stored_members)+(SELECT count(*) FROM stored_count)",
    ).bind(company.as_uuid()).bind(run.as_uuid())
        .bind(target.desk_id()).bind(thread_id.as_uuid()).bind(MAX_HISTORY_ITEMS as i64).bind(target.principal_id().map(|id| id.as_uuid()))
        .execute(db).await?;
    Ok(())
}
