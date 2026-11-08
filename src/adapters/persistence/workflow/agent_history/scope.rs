use super::*;

pub(super) struct Facts {
    pub bundle: Arc<PublishedBundle>,
    pub kind: ModelStep,
    pub step: StepId,
    pub activation: i64,
    pub inputs: serde_json::Value,
    pub parent: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub thread_id: Option<Uuid>,
}
#[derive(sqlx::FromRow)]
struct FactsRow {
    actor_id: Uuid,
    bundle: Vec<u8>,
    step_id: String,
    activation: i64,
    frozen_inputs: serde_json::Value,
    parent_run_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
}

/// Unlocked immutable facts are lock-order hints until the native fence is locked.
pub(super) async fn facts_on(
    db: &mut PgConnection,
    action: ActionScope,
    actor: WorkflowActor,
) -> AppResult<Facts> {
    let row = sqlx::query_as::<_, FactsRow>(
        "SELECT run.actor_id,stored.bundle,execution.step_id,execution.activation,execution.frozen_inputs, \
         run.parent_run_id,run.channel_id,run.thread_id FROM workflow_runs AS run \
         JOIN workflow_bundles AS stored ON stored.company_id=run.company_id AND stored.digest=run.bundle_digest \
         JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id \
         WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3 \
           AND execution.activated_at IS NOT NULL AND octet_length(stored.bundle)<=$4",
    ).bind(action.company.as_uuid()).bind(action.run.as_uuid()).bind(action.execution.as_uuid())
        .bind(MAX_BUNDLE_BYTES as i32).fetch_optional(db).await?.ok_or_else(missing)?;
    if row.actor_id != actor.user_id() {
        return Err(conflict());
    }
    let bundle = restore_bundle(
        &row.bundle,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    let step = StepId::parse(row.step_id).map_err(|_| invalid())?;
    let definition = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&step)
        .ok_or_else(invalid)?;
    let kind = ModelStep::of(definition.step_type.as_str()).ok_or_else(invalid)?;
    Ok(Facts {
        bundle,
        kind,
        step,
        activation: row.activation,
        inputs: row.frozen_inputs,
        parent: row.parent_run_id,
        channel_id: row.channel_id,
        thread_id: row.thread_id,
    })
}

pub(super) async fn boundary_on(
    db: &mut PgConnection,
    action: ActionScope,
    actor: WorkflowActor,
    facts: &Facts,
) -> AppResult<AgentConversationBoundary> {
    if facts.parent.is_some() {
        return Ok(AgentConversationBoundary::ChildBranch { run: action.run });
    }
    let (session, public_input): (Uuid,bool) = sqlx::query_as(
        "SELECT session.id,mapping.thread_input_proven FROM workflow_agent_sessions AS mapping \
         JOIN agent_history_sessions AS session ON session.company_id=mapping.company_id AND session.id=mapping.session_id \
         WHERE mapping.company_id=$1 AND mapping.run_id=$2 AND session.creator_user_id=$3 \
           AND session.channel_id IS NOT DISTINCT FROM $4::uuid AND session.thread_id IS NOT DISTINCT FROM $5::uuid",
    ).bind(action.company.as_uuid()).bind(action.run.as_uuid()).bind(actor.user_id())
        .bind(facts.channel_id).bind(facts.thread_id).fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    if public_input && proven_on(db, action, facts).await? {
        let (Some(channel), Some(thread)) = (facts.channel_id, facts.thread_id) else {
            return Err(invalid());
        };
        return Ok(AgentConversationBoundary::Thread {
            channel: RelatedChannelId::new(channel),
            thread: RelatedThreadId::new(thread),
        });
    }
    Ok(AgentConversationBoundary::PrivateSession {
        session: AgentPrivateSessionId::new(session),
    })
}

async fn proven_on(db: &mut PgConnection, action: ActionScope, facts: &Facts) -> AppResult<bool> {
    let max = facts.bundle.compiled().graph().definition().steps.len();
    let earlier: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 \
         AND activation<$3 AND activated_at IS NOT NULL ORDER BY step_id LIMIT $4",
    )
    .bind(action.company.as_uuid())
    .bind(action.run.as_uuid())
    .bind(facts.activation)
    .bind((max + 1) as i64)
    .fetch_all(&mut *db)
    .await?;
    let earlier = earlier
        .into_iter()
        .map(|step| StepId::parse(step).map_err(|_| invalid()))
        .collect::<AppResult<Vec<_>>>()?;
    if !thread_history_input_proven(&facts.bundle, &facts.step, &earlier)? {
        return Ok(false);
    }
    // A message.ask may contribute only its native question's actual in-thread message.
    let unproven: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_executions AS execution \
         WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.activation<$3 \
           AND execution.activated_at IS NOT NULL AND execution.step_id=ANY($4) \
           AND NOT EXISTS(SELECT 1 FROM workflow_channel_questions AS question \
             JOIN thread_messages AS membership ON membership.company_id=question.company_id \
               AND membership.message_id=question.message_id \
             WHERE question.company_id=execution.company_id AND question.run_id=execution.run_id \
               AND question.execution_id=execution.id AND membership.channel_id=$5 AND membership.thread_id=$6))",
    ).bind(action.company.as_uuid()).bind(action.run.as_uuid()).bind(facts.activation)
        .bind(facts.bundle.compiled().graph().definition().steps.iter()
            .filter(|(_,definition)| definition.step_type.as_str()=="message.ask")
            .map(|(step,_)| step.as_str().to_owned()).collect::<Vec<_>>())
        .bind(facts.channel_id).bind(facts.thread_id).fetch_one(db).await?;
    Ok(!unproven)
}
