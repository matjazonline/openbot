//! Definite admission outcomes survive retries without inventing a run or a dependency manifest.
use super::*;
use crate::entities::department::RoleAdmissionRefusal;
use crate::entities::workflow_escalation::AdmissionRefusalId;

#[path = "admission_refusal_claims.rs"]
mod claims;
#[path = "admission_refusal_replay.rs"]
mod replay;
pub(super) use claims::settle_on as settle_replay_on;
pub(super) use replay::{RefusalKey, saved_binding_on};
pub(super) use replay::{RefusalReplay, resolve_on};

pub(super) fn fingerprint(command: &PreparedAdmission) -> AppResult<String> {
    let association = binding_rows::association_columns(command.association());
    crate::application::workflow::decisions::hex_digest(&serde_json::json!({
        "version": 1,
        "input": command.input(),
        "channel": association.0,
        "thread": association.1,
        "rerun_of": command.rerun_of(),
        "history_session": command.history_session(),
        "delivery_policy": command.delivery_policy().as_str(),
    }))
}

pub(super) async fn record_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    source: &admission_binding::SourceKey,
    refusal: &RoleAdmissionRefusal,
) -> AppResult<bool> {
    if !admission::claim_holds(tx, command).await? {
        return Ok(false);
    }
    let id = AdmissionRefusalId::new(Uuid::new_v4());
    let association = binding_rows::association_columns(command.association());
    sqlx::query(
        "INSERT INTO workflow_admission_refusals \
         (company_id,id,binding_id,binding_revision,source_key,request_fingerprint,channel_id,thread_id,role_refusal) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(command.company_id().as_uuid())
    .bind(id.as_uuid())
    .bind(command.binding().id().as_uuid())
    .bind(revision(command.binding().revision().get())?)
    .bind(source.as_str())
    .bind(fingerprint(command)?)
    .bind(association.0)
    .bind(association.1)
    .bind(serde_json::to_value(refusal).map_err(|_| invalid())?)
    .execute(&mut **tx)
    .await?;
    replay::insert_command_on(tx, command, id).await?;
    if !claims::settle_on(tx, command, refusal, command.binding().revision()).await? {
        return Ok(false);
    }
    super::escalations::admission_refused_on(tx, command.company_id(), id).await?;
    Ok(true)
}
