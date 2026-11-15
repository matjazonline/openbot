use super::invalid;
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
        if matches!(
            action.request().context.origin,
            crate::application::agent::ToolCallOrigin::ChannelFinal(_)
        ) {
            return Err(AppError::BadRequest(
                "Final responses require their terminal owner".into(),
            ));
        }
        let mut tx = self.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','5s',true), set_config('statement_timeout','5s',true)")
            .execute(&mut *tx).await?;
        let scope = action.scope();
        // Authority before run, matching dispatch and company/member writers. Existing immutable
        // intents can replay even after closure; only a new freeze needs current authority.
        if read(&mut tx, action).await?.is_none() {
            super::action_authority::prelock_on(&mut tx, scope)
                .await?
                .ok_or_else(missing)?;
        }
        let active: bool = sqlx::query_scalar(
            "SELECT state IN ('queued','running') AND deadline > clock_timestamp() AND NOT EXISTS(SELECT 1 FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2 AND execution_id=$3) FROM workflow_runs \
             WHERE company_id=$1 AND id=$2 FOR UPDATE",
        ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
            .fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
        let existing = read(&mut tx, action).await?;
        let replayed = existing.is_some();
        if existing.is_none() && !active {
            return Err(conflict());
        }
        if existing.is_none() {
            super::native_mcp::live_on(&mut tx, action.request()).await?;
        }
        let intent = match existing {
            Some(row) => restore(action, row, replayed)?,
            None => insert_on(&mut tx, action, model_call).await?,
        };
        if let Some(call) = model_call {
            map_call(&mut tx, action, call, &intent).await?;
        }
        tx.commit().await?;
        Ok(intent)
    }
}

/// Insert a new frozen intent inside its owner's transaction. The caller holds current
/// authority, run and execution locks and has established that this logical call is new.
pub(super) async fn insert_on(
    db: &mut PgConnection,
    action: &FrozenAction,
    model_call: Option<&ModelToolCallId>,
) -> AppResult<ActionIntent> {
    super::agent_origin::freeze_on(db, action.request(), model_call).await?;
    let row = insert(db, action).await?;
    restore(action, row, false)
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
    let levels = super::knowledge_release::evaluate_on(
        db,
        action,
        super::knowledge_release::MessageRecipients::Current,
    )
    .await?;
    let origin = OriginColumns::try_from(action.request().context.origin)?;
    let decision = match action.decision() {
        ActionPolicyDecision::Unevaluated => "unevaluated",
        ActionPolicyDecision::ApprovalRequired => "approval_required",
    };
    Ok(sqlx::query_as::<_, IntentRow>(
        "INSERT INTO workflow_action_intents \
         (company_id,run_id,execution_id,id,operation_key,operation_digest,argument_digest,idempotency_key,operation,policy_decision,content_level_at_freeze,destination_level_at_freeze,origin_kind,agent_manifest_id,agent_id,agent_revision_id,agent_binding_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11::smallint,$12::smallint,$13,$14,$15,$16,$17) \
         RETURNING id,operation,argument_digest,idempotency_key,policy_decision",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(Uuid::new_v4()).bind(action.operation_key().as_str()).bind(action.operation_digest().as_str())
        .bind(action.argument_digest().as_str()).bind(action.idempotency_key().as_str())
        .bind(action.saved_operation()).bind(decision).bind(levels.content.as_i16()).bind(levels.destination.as_i16())
        .bind(origin.kind).bind(origin.manifest).bind(origin.agent).bind(origin.revision).bind(origin.binding).fetch_one(db).await?)
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

struct OriginColumns {
    kind: &'static str,
    manifest: Option<Uuid>,
    agent: Option<Uuid>,
    revision: Option<Uuid>,
    binding: Option<Uuid>,
}
impl TryFrom<crate::application::agent::ToolCallOrigin> for OriginColumns {
    type Error = AppError;
    fn try_from(origin: crate::application::agent::ToolCallOrigin) -> AppResult<Self> {
        use crate::application::agent::ToolCallOrigin;
        Ok(match origin {
            ToolCallOrigin::Workflow => Self {
                kind: "workflow",
                manifest: None,
                agent: None,
                revision: None,
                binding: None,
            },
            ToolCallOrigin::Agent(origin) => Self {
                kind: "agent",
                manifest: Some(origin.manifest.as_uuid()),
                agent: Some(origin.agent.as_uuid()),
                revision: Some(origin.revision.as_uuid()),
                binding: Some(origin.binding.as_uuid()),
            },
            ToolCallOrigin::ChannelFinal(_) => return Err(missing()),
        })
    }
}
