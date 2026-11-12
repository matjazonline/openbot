use super::*;

pub(in crate::adapters::persistence::workflow) async fn settle_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    refusal: &RoleAdmissionRefusal,
    binding_revision: BindingRevision,
) -> AppResult<bool> {
    if !admission::claim_holds(tx, command).await? {
        return Ok(false);
    }
    let detail = serde_json::to_value(refusal).map_err(|_| invalid())?;
    match command.admission_claim() {
        None => {}
        Some(AdmissionClaim::Message(claim)) => {
            sqlx::query(
                "INSERT INTO workflow_message_intent_bindings \
                 (company_id,intent_id,binding_id,binding_revision,outcome,reason,role_refusal) \
                 VALUES ($1,$2,$3,$4,'refused',$5,$6) \
                 ON CONFLICT (company_id,intent_id,binding_id) DO NOTHING",
            )
            .bind(claim.company.as_uuid())
            .bind(claim.intent)
            .bind(command.binding().id().as_uuid())
            .bind(revision(binding_revision.get())?)
            .bind(refusal.kind.code())
            .bind(detail)
            .execute(&mut **tx)
            .await?;
        }
        Some(AdmissionClaim::Schedule(claim)) => {
            let schedule: Uuid = sqlx::query_scalar(
                "UPDATE schedule_runs SET run_id=NULL,refusal=$3,role_refusal=$4,last_error=$5, \
                 materialization_status='failed',materialization_worker_id=NULL, \
                 materialization_generation=NULL,materialization_locked_at=NULL, \
                 materialization_lock_expires_at=NULL,updated_at=clock_timestamp() \
                 WHERE company_id=$1 AND id=$2 RETURNING schedule_id",
            )
            .bind(claim.company.as_uuid())
            .bind(claim.occurrence)
            .bind(refusal.kind.code())
            .bind(detail)
            .bind(refusal.to_string())
            .fetch_one(&mut **tx)
            .await?;
            sqlx::query("UPDATE channel_schedules SET last_error=$3 WHERE company_id=$1 AND id=$2")
                .bind(claim.company.as_uuid())
                .bind(schedule)
                .bind(refusal.to_string())
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(true)
}
