use super::*;
use crate::application::workflow::actions::*;
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct IntentRow {
    id: Uuid,
    operation: Value,
    argument_digest: String,
    idempotency_key: String,
    policy_decision: String,
}

#[async_trait]
impl ActionIntents for PostgresPersistence {
    async fn prepare(
        &self,
        action: &FrozenAction,
        model_call: Option<&ModelToolCallId>,
    ) -> AppResult<ActionIntent> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','5s',true), set_config('statement_timeout','5s',true)")
            .execute(&mut *tx).await?;
        // The run is always locked first, matching cancellation/progression and future dispatch.
        let scope = action.scope();
        let active: bool = sqlx::query_scalar(
            "SELECT state IN ('queued','running') AND deadline > clock_timestamp() FROM workflow_runs \
             WHERE company_id=$1 AND id=$2 FOR UPDATE",
        ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid())
            .fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
        let existing = read(&mut tx, action).await?;
        let replayed = existing.is_some();
        if existing.is_none() && !active {
            return Err(conflict());
        }
        let row = match existing {
            Some(row) => row,
            None => insert(&mut tx, action).await?,
        };
        let intent = restore(action, row, replayed)?;
        if let Some(call) = model_call {
            map_call(&mut tx, action, call, &intent).await?;
        }
        tx.commit().await?;
        Ok(intent)
    }
}

async fn read(db: &mut PgConnection, action: &FrozenAction) -> AppResult<Option<IntentRow>> {
    let scope = action.scope();
    Ok(sqlx::query_as::<_, IntentRow>(
        "SELECT id,operation,argument_digest,idempotency_key,policy_decision FROM workflow_action_intents \
         WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND operation_digest=$4",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(action.operation_digest().as_str()).fetch_optional(db).await?)
}

async fn insert(db: &mut PgConnection, action: &FrozenAction) -> AppResult<IntentRow> {
    let scope = action.scope();
    let decision = match action.decision() {
        ActionPolicyDecision::Unevaluated => "unevaluated",
        ActionPolicyDecision::ApprovalRequired => "approval_required",
    };
    Ok(sqlx::query_as::<_, IntentRow>(
        "INSERT INTO workflow_action_intents \
         (company_id,run_id,execution_id,id,operation_key,operation_digest,argument_digest,idempotency_key,operation,policy_decision) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
         RETURNING id,operation,argument_digest,idempotency_key,policy_decision",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(Uuid::new_v4()).bind(action.operation_key().as_str()).bind(action.operation_digest().as_str())
        .bind(action.argument_digest().as_str()).bind(action.idempotency_key().as_str())
        .bind(action.saved_operation()).bind(decision).fetch_one(db).await?)
}

fn restore(action: &FrozenAction, row: IntentRow, replayed: bool) -> AppResult<ActionIntent> {
    let saved = FrozenAction::restore(row.operation.clone())?;
    if row.operation != *action.saved_operation()
        || saved.operation_digest() != action.operation_digest()
        || row.argument_digest != action.argument_digest().as_str()
        || row.idempotency_key != action.idempotency_key().as_str()
    {
        return Err(conflict());
    }
    let decision = match row.policy_decision.as_str() {
        "unevaluated" => ActionPolicyDecision::Unevaluated,
        "approval_required" => ActionPolicyDecision::ApprovalRequired,
        _ => return Err(invalid()),
    };
    if decision != saved.decision() {
        return Err(invalid());
    }
    Ok(ActionIntent {
        invocation: ActionInvocationId::new(row.id),
        argument_digest: action.argument_digest().clone(),
        idempotency_key: action.idempotency_key(),
        decision,
        replayed,
    })
}

async fn map_call(
    db: &mut PgConnection,
    action: &FrozenAction,
    call: &ModelToolCallId,
    intent: &ActionIntent,
) -> AppResult<()> {
    let scope = action.scope();
    let mapped: Option<Uuid> = sqlx::query_scalar(
        "SELECT invocation_id FROM workflow_action_model_calls WHERE company_id=$1 AND run_id=$2 \
         AND execution_id=$3 AND model_call_id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(call.as_str())
    .fetch_optional(&mut *db)
    .await?;
    if let Some(mapped) = mapped {
        return if mapped == intent.invocation.as_uuid() {
            Ok(())
        } else {
            Err(conflict())
        };
    }
    sqlx::query("INSERT INTO workflow_action_model_calls \
        (company_id,run_id,execution_id,model_call_id,invocation_id,argument_digest) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(call.as_str()).bind(intent.invocation.as_uuid()).bind(intent.argument_digest.as_str())
        .execute(db).await?;
    Ok(())
}
