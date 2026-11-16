//! The queue's own live fence settles notice handling. A notice's intended
//! admission has one immutable owner; descendants cannot claim that ownership.
use super::admission_binding::SourceKey;
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::message_admission::{FailedIntent, IntentClaim};
use crate::entities::workflow_escalation::EscalationId;

#[derive(sqlx::FromRow)]
struct Subject {
    notice_id: Uuid,
    case_id: Uuid,
    decision_id: Uuid,
}

pub(super) async fn has_intent_on(db: &mut PgConnection, claim: IntentClaim) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_message_intents AS intent \
        JOIN workflow_escalation_notice_targets AS target ON target.company_id=intent.company_id AND target.message_id=intent.message_id \
        WHERE intent.company_id=$1 AND intent.id=$2)")
        .bind(claim.company.as_uuid()).bind(claim.intent).fetch_one(db).await?)
}

pub(super) async fn has_message_on(
    db: &mut PgConnection,
    company: CompanyId,
    message: Uuid,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_message_notification_origins WHERE company_id=$1 AND message_id=$2)")
        .bind(company.as_uuid()).bind(message).fetch_one(db).await?)
}

pub(super) async fn binding_matches_on(
    db: &mut PgConnection,
    command: &PreparedAdmission,
) -> AppResult<bool> {
    let source = SourceKey::from_trigger(command.trigger());
    let Some(message) = SourceKey::message(source.as_str()) else {
        return Ok(true);
    };
    Ok(sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM workflow_escalation_notice_targets AS target \
        WHERE target.company_id=$1 AND target.message_id=$2 AND \
          ((target.binding_id,target.binding_revision) IS DISTINCT FROM ($3::uuid,$4::bigint) \
            OR workflow_escalation_target_admissible(target) IS NOT TRUE))",
    )
    .bind(command.company_id().as_uuid())
    .bind(message)
    .bind(command.binding().id().as_uuid())
    .bind(revision(command.binding().revision().get())?)
    .fetch_one(db)
    .await?)
}

pub(super) async fn capture_root_on(
    db: &mut PgConnection,
    company: CompanyId,
    source: &SourceKey,
    run: RunId,
) -> AppResult<()> {
    let Some(message) = SourceKey::message(source.as_str()) else {
        return Ok(());
    };
    sqlx::query("INSERT INTO workflow_escalation_handling_roots(company_id,notice_id,run_id,live_run_id,message_id,intent_id,binding_id,binding_revision) \
        SELECT target.company_id,target.notice_id,run.id,run.id,target.message_id,target.intent_id,target.binding_id,target.binding_revision \
        FROM workflow_escalation_notice_targets AS target JOIN workflow_runs AS run ON run.company_id=target.company_id \
          AND run.id=$3 AND run.binding_id=target.binding_id AND run.binding_revision=target.binding_revision \
        WHERE target.company_id=$1 AND target.message_id=$2 AND run.parent_run_id IS NULL AND run.rerun_of_run_id IS NULL \
        ON CONFLICT(company_id,notice_id) DO NOTHING")
        .bind(company.as_uuid()).bind(message).bind(run.as_uuid()).execute(db).await?;
    Ok(())
}

pub(super) async fn finish_intent(
    p: &PostgresPersistence,
    claim: IntentClaim,
    matched: Option<RunId>,
) -> AppResult<bool> {
    let mut tx = p.pool().begin().await?;
    let subject = lock_intent_on(&mut tx, claim.company, claim.intent).await?;
    if !message_intents::finish_on(&mut tx, claim, matched).await? {
        tx.rollback().await?;
        return Ok(false);
    }
    if let Some(subject) = subject {
        record_failure_on(&mut tx, claim.company, &subject).await?;
    }
    tx.commit().await?;
    Ok(true)
}

pub(super) async fn expire_intent(
    p: &PostgresPersistence,
    company: CompanyId,
    intent: Uuid,
) -> AppResult<Option<FailedIntent>> {
    let mut tx = p.pool().begin().await?;
    let subject = lock_intent_on(&mut tx, company, intent).await?;
    let expired = message_intents::expire_on(&mut tx, company, intent).await?;
    if expired.is_some()
        && let Some(subject) = subject
    {
        record_failure_on(&mut tx, company, &subject).await?;
    }
    tx.commit().await?;
    Ok(expired.and_then(message_intents::ExpiredRow::failure))
}

async fn lock_intent_on(
    db: &mut PgConnection,
    company: CompanyId,
    intent: Uuid,
) -> AppResult<Option<Subject>> {
    sqlx::query(
        "SELECT set_config('lock_timeout','5s',true),set_config('statement_timeout','20s',true)",
    )
    .execute(&mut *db)
    .await?;
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(company.as_uuid())
        .execute(&mut *db)
        .await?;
    let subject:Option<Subject>=sqlx::query_as("SELECT target.notice_id,notice.case_id,notice.decision_id \
        FROM workflow_message_intents AS intent JOIN workflow_escalation_notice_targets AS target \
          ON target.company_id=intent.company_id AND target.message_id=intent.message_id \
        JOIN workflow_escalation_notices AS notice ON notice.company_id=target.company_id AND notice.id=target.notice_id \
        WHERE intent.company_id=$1 AND intent.id=$2")
        .bind(company.as_uuid()).bind(intent).fetch_optional(&mut *db).await?;
    if let Some(subject) = &subject {
        lock_subject_on(db, company, subject).await?;
    }
    // Recheck the actual clock fence after every wait, including an intent row
    // held without an UPDATE. The shared writer then owns this row already.
    sqlx::query("SELECT id FROM workflow_message_intents WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(company.as_uuid())
        .bind(intent)
        .execute(db)
        .await?;
    Ok(subject)
}

pub(super) async fn run_failed_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<bool> {
    let subject:Option<Subject>=sqlx::query_as("SELECT ownership.notice_id,notice.case_id,notice.decision_id \
        FROM workflow_escalation_handling_roots AS ownership JOIN workflow_escalation_notices AS notice \
          ON notice.company_id=ownership.company_id AND notice.id=ownership.notice_id \
        WHERE ownership.company_id=$1 AND ownership.run_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(subject) = subject else {
        return Ok(false);
    };
    lock_subject_on(db, scope.company, &subject).await?;
    record_failure_on(db, scope.company, &subject).await?;
    Ok(true)
}

async fn lock_subject_on(
    db: &mut PgConnection,
    company: CompanyId,
    subject: &Subject,
) -> AppResult<()> {
    sqlx::query(
        "SELECT id FROM workflow_escalation_cases WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(company.as_uuid())
    .bind(subject.case_id)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "SELECT id FROM workflow_escalation_decisions WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(company.as_uuid())
    .bind(subject.decision_id)
    .execute(db)
    .await?;
    Ok(())
}

async fn record_failure_on(
    db: &mut PgConnection,
    company: CompanyId,
    subject: &Subject,
) -> AppResult<()> {
    sqlx::query("WITH reason AS MATERIALIZED(SELECT workflow_escalation_agent_handling_failure($1,$2) AS code) \
        INSERT INTO workflow_escalation_recipient_failures(company_id,notice_id,failure_kind,reason_code) \
        SELECT $1,$2,'handling',reason.code FROM reason WHERE reason.code IS NOT NULL \
        ON CONFLICT(company_id,notice_id,failure_kind) DO NOTHING")
        .bind(company.as_uuid()).bind(subject.notice_id).execute(&mut *db).await?;
    crate::adapters::persistence::notification::wake_failed_assignments_on(
        db,
        company,
        Some(EscalationId::new(subject.case_id)),
    )
    .await
}
