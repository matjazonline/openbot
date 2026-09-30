//! Append effect uncertainty inside the existing run-first retirement transaction.
use super::*;
use crate::application::workflow::actions::ActionScope;
use crate::domain::workflow::FailureCode;

pub(super) async fn record(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    execution: Option<ExecutionId>,
    code: &FailureCode,
) -> AppResult<()> {
    // The run's frozen activation and per-execution action bounds limit this set.
    // Receipts retain precedence; no observation here grants another provider call.
    sqlx::query("INSERT INTO workflow_action_reconciliations (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,failure_code) SELECT marker.company_id,marker.run_id,marker.execution_id,marker.invocation_id,marker.argument_digest,marker.id,marker.effect_kind,$4 FROM workflow_action_dispatches AS marker WHERE marker.company_id=$1 AND marker.run_id=$2 AND ($3::uuid IS NULL OR marker.execution_id=$3) AND marker.effect_kind='remote' AND NOT EXISTS (SELECT 1 FROM workflow_action_receipts AS receipt WHERE receipt.company_id=marker.company_id AND receipt.invocation_id=marker.invocation_id) AND NOT EXISTS (SELECT 1 FROM workflow_action_reconciliations AS observation WHERE observation.company_id=marker.company_id AND observation.invocation_id=marker.invocation_id) ON CONFLICT (company_id,invocation_id) DO NOTHING")
        .bind(company.as_uuid()).bind(run.as_uuid()).bind(execution.map(ExecutionId::as_uuid)).bind(code.as_str()).execute(db).await?;
    Ok(())
}

/// Owning run must already be locked; one shared execution conflict decision.
pub(super) async fn conflicted_on(db: &mut PgConnection, scope: ActionScope) -> AppResult<bool> {
    sqlx::query_scalar("SELECT workflow_action_has_evidence_conflict($1,$2)")
        .bind(scope.company.as_uuid())
        .bind(scope.execution.as_uuid())
        .fetch_one(db)
        .await
        .map_err(Into::into)
}
