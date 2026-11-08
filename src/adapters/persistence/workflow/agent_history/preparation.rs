//! One bounded native owner captures original input before checkpoint/provider entry.
use super::*;
use crate::application::workflow::{
    agent::{PreparedAgent, PreparedStep},
    lease::{ClaimedWorkflow, LeasePolicy},
    publication::ResolvedAgent,
};

pub(super) struct LockedAgent {
    pub actor: WorkflowActor,
    pub facts: scope::Facts,
    pub selection: ResolvedAgent,
}
struct Candidate {
    prepared: PreparedAgent,
    context: Option<Context>,
}

pub(in crate::adapters::persistence::workflow) enum Preparation {
    Ready(Box<PreparedAgent>),
    Parked(crate::domain::workflow::WaitId),
    AtCapacity,
}

pub(in crate::adapters::persistence::workflow) async fn prepare(
    persistence: &PostgresPersistence,
    claim: &ClaimedWorkflow,
    policy: LeasePolicy,
) -> AppResult<Preparation> {
    let bound = WriteBound::start("agent_history.prepare", policy);
    let attempt = bound
        .attempt(Box::pin(async {
            let mut connection = bound.acquire(persistence.pool()).await?;
            let mut tx = bound.begin(&mut connection).await?;
            let body = Box::pin(prepare_on(&mut tx, claim, &bound)).await;
            bound.end(tx, body).await
        }))
        .await;
    let result = match attempt {
        Attempt::Done(prepared) => Ok(prepared),
        Attempt::Failed(error) => Err(error),
        Attempt::Ambiguous => {
            bound
                .resolve(Box::pin(resolve_on(persistence, claim, &bound)))
                .await
        }
    };
    match &result {
        Ok(Preparation::Ready(prepared)) => {
            if let (Some(exchange), Some(history)) =
                (prepared.history_exchange(), prepared.frozen_history())
            {
                tracing::info!(company_id=%claim.fence.scope.company.as_uuid(),run_id=%claim.fence.scope.run.as_uuid(),
                    exchange_id=%exchange.as_uuid(), selected=history.selected.len(), omitted_eligible=history.omitted_eligible,
                    selected_level=history.selected_level.as_i16(), content_level=history.content_level.as_i16(),
                    durability="verified", event="agent_history.prepared", "Agent history preparation durably verified");
            }
        }
        Ok(Preparation::AtCapacity) => {
            tracing::info!(company_id=%claim.fence.scope.company.as_uuid(),
            run_id=%claim.fence.scope.run.as_uuid(), exchange_id=%claim.fence.scope.execution.as_uuid(),
            durability="not_applied", code="history_capacity", event="agent_history.refused", "Agent history capacity refused preparation")
        }
        Ok(Preparation::Parked(wait)) => {
            tracing::info!(company_id=%claim.fence.scope.company.as_uuid(),
            run_id=%claim.fence.scope.run.as_uuid(),exchange_id=%claim.fence.scope.execution.as_uuid(),
            wait_id=%wait.as_uuid(),outcome="parked",durability="verified",event="agent_history.queued",
            "Agent exchange durably waiting for its turn")
        }
        Err(_) => {}
    }
    result
}

async fn resolve_on(
    persistence: &PostgresPersistence,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<Evidence<Preparation>> {
    let mut connection = bound.acquire(persistence.pool()).await?;
    // A successful park intentionally retired the fence. Serialize its receipt behind
    // the original COMMIT, then release this source lock before current global authority.
    let mut tx = bound.begin(&mut connection).await?;
    let body = turn_waits::receipt_on(&mut tx, claim.fence).await;
    if let Some(wait) = bound.roll_back(tx, body).await? {
        return Ok(Evidence::Applied(Preparation::Parked(wait)));
    }
    // This separate transaction may authorize Ready work; keep global-before-local order.
    let mut tx = bound.begin(&mut connection).await?;
    let body = Box::pin(proof_on(&mut tx, claim, bound)).await;
    bound.roll_back(tx, body).await
}

async fn prepare_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<End<Preparation>> {
    let candidate = Box::pin(candidate_on(tx, claim, bound)).await?;
    let prepared = match candidate.context {
        None => candidate.prepared,
        Some(context) => {
            let Some(conversation) = conversation_on(tx, &context).await? else {
                return Ok(End::Rollback(Preparation::AtCapacity));
            };
            let Some(exchange) =
                storage::enqueue_on(tx, claim.fence, conversation, &candidate.prepared).await?
            else {
                return Ok(End::Rollback(Preparation::AtCapacity));
            };
            if !turns::acquire_on(tx, context.company, exchange).await? {
                if lease::live_window(tx, claim.fence, bound.started())
                    .await?
                    .is_none()
                {
                    return Err(ownership_lost());
                }
                return Ok(End::Commit(Preparation::Parked(
                    turn_waits::park_on(tx, claim.fence).await?,
                )));
            }
            Box::pin(frozen::freeze_on(
                tx,
                claim.fence,
                candidate.prepared.with_history(exchange)?,
            ))
            .await?
        }
    };
    if lease::live_window(tx, claim.fence, bound.started())
        .await?
        .is_none()
    {
        return Err(ownership_lost());
    }
    Ok(End::Commit(Preparation::Ready(Box::new(prepared))))
}

async fn proof_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<Evidence<Preparation>> {
    if let Some(wait) = turn_waits::parked_on(tx, claim.fence).await? {
        return Ok(Evidence::Applied(Preparation::Parked(wait)));
    }
    let candidate = Box::pin(candidate_on(tx, claim, bound)).await?;
    let Some(context) = candidate.context else {
        return Ok(Evidence::Applied(Preparation::Ready(Box::new(
            candidate.prepared,
        ))));
    };
    let Some(conversation) = find_on(tx, &context).await? else {
        return Ok(Evidence::NotApplied);
    };
    let Some(exchange) =
        storage::matching_on(tx, claim.fence, conversation, &candidate.prepared).await?
    else {
        return Ok(Evidence::NotApplied);
    };
    if !turns::owned_on(tx, context.company, exchange).await? {
        return Ok(Evidence::NotApplied);
    }
    let Some(prepared) =
        frozen::restore_on(tx, claim.fence, candidate.prepared.with_history(exchange)?).await?
    else {
        return Ok(Evidence::NotApplied);
    };
    Ok(Evidence::Applied(Preparation::Ready(Box::new(prepared))))
}

pub(super) async fn locked_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    bound: &WriteBound,
) -> AppResult<LockedAgent> {
    let action = ActionScope {
        company: fence.scope.company,
        run: fence.scope.run,
        execution: fence.scope.execution,
    };
    let actor = action_authority::prelock_on(tx, action)
        .await?
        .ok_or_else(missing)?;
    let facts = scope::facts_on(tx, action, actor).await?;
    let association = binding_rows::association_from_columns(facts.channel_id, facts.thread_id)?;
    association::authorize_association(tx, action.company, actor, association).await?;
    let selection =
        agent_manifest::execution_selection_on(tx, action, actor, &facts.bundle).await?;
    agent_authority::check_execution_on(tx, action, selection.agent(), selection.mode()).await?;
    // Global account/company/association/agent authority precedes the native local fence.
    if !checkpoints::lock_owner(tx, fence, bound).await? {
        return Err(ownership_lost());
    }
    Ok(LockedAgent {
        actor,
        facts,
        selection,
    })
}

async fn candidate_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<Candidate> {
    let LockedAgent {
        actor,
        facts,
        selection,
    } = locked_on(tx, claim.fence, bound).await?;
    if facts.step != claim.activation.step || facts.inputs != claim.activation.inputs {
        return Err(missing());
    }
    let prepared = build(selection, &facts)?;
    let context = if agent_history_eligible(facts.kind) {
        let action = ActionScope {
            company: claim.fence.scope.company,
            run: claim.fence.scope.run,
            execution: claim.fence.scope.execution,
        };
        Some(Context {
            company: action.company,
            actor,
            agent: prepared.selection().agent().agent_id,
            boundary: scope::boundary_on(tx, action, actor, &facts).await?,
        })
    } else {
        None
    };
    Ok(Candidate { prepared, context })
}

fn build(selection: ResolvedAgent, facts: &scope::Facts) -> AppResult<PreparedAgent> {
    let step = match facts.kind {
        ModelStep::AgentRun => PreparedStep::AgentRun {
            targets: agent_preparation::targets(selection.agent().company_id, selection.tools())?,
        },
        ModelStep::Classify => PreparedStep::Classify,
        ModelStep::Decide => PreparedStep::Decide {
            contract: facts
                .bundle
                .compiled()
                .step_output_schema(&facts.step)
                .cloned()
                .ok_or_else(invalid)?,
        },
    };
    PreparedAgent::new(selection, &facts.inputs, step)
}
