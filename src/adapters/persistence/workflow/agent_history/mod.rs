//! Native identity, original input capture and checkpoint-owned logical exchanges.
use super::*;
use crate::application::workflow::{
    actions::ActionScope, agent_history::*, lease::WorkflowFence, registry::ModelStep,
};
use crate::entities::agent_capability::AgentId;
use persistence_bound::{Attempt, End, Evidence, WriteBound};

mod capture;
mod checkpoint;
mod frozen;
mod selection;
pub(super) use frozen::audience_on;
#[cfg(test)]
pub(super) use selection::{
    CANDIDATES_SQL as SELECTION_CANDIDATES_SQL, COUNT_SQL as SELECTION_COUNT_SQL,
};
mod preparation;
mod recall;
pub(super) mod turn_waits;
mod turns;
pub(super) use checkpoint::{
    LinkChange, accepted_on, attach_on, check_prepared_on, linked_on, require_active_on,
};
mod scope;
mod storage;
pub(super) use preparation::{Preparation, prepare};

#[async_trait]
impl AgentHistoryIdentity for PostgresPersistence {
    async fn resolve_agent_history_identity(
        &self,
        request: ResolveAgentHistoryIdentity,
    ) -> AppResult<Option<AgentConversationIdentity>> {
        let bound = WriteBound::start("agent_history.identity", request.lease_policy);
        // Keep the native persistence boundary boxed rather than growing the runtime chain.
        let attempt = bound
            .attempt(Box::pin(async {
                let mut connection = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut connection).await?;
                let body = resolve_on(&mut tx, request.fence, &bound).await;
                bound.end(tx, body.map(End::Commit)).await
            }))
            .await;
        match attempt {
            Attempt::Done(identity) => Ok(identity),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(async {
                        let mut connection = bound.acquire(&self.pool).await?;
                        let mut tx = bound.begin(&mut connection).await?;
                        let body = async {
                            let Some(context) =
                                authorized_on(&mut tx, request.fence, &bound).await?
                            else {
                                return Ok(Evidence::Applied(None));
                            };
                            Ok(match find_on(&mut tx, &context).await? {
                                Some(id) => Evidence::Applied(Some(context.identity(id))),
                                None => Evidence::NotApplied,
                            })
                        }
                        .await;
                        bound.roll_back(tx, body).await
                    }))
                    .await
            }
        }
    }
}

struct Context {
    company: CompanyId,
    actor: WorkflowActor,
    agent: AgentId,
    boundary: AgentConversationBoundary,
}
impl Context {
    fn identity(&self, conversation: Uuid) -> AgentConversationIdentity {
        AgentConversationIdentity {
            company: self.company,
            actor: self.actor,
            agent: self.agent,
            boundary: self.boundary,
            conversation: AgentConversationId::new(conversation),
        }
    }
    fn columns(&self) -> ScopeColumns {
        match self.boundary {
            AgentConversationBoundary::Thread { channel, thread } => ScopeColumns {
                kind: "thread",
                channel: Some(channel.as_uuid()),
                thread: Some(thread.as_uuid()),
                session: None,
                child: None,
            },
            AgentConversationBoundary::PrivateSession { session } => ScopeColumns {
                kind: "private_session",
                session: Some(session.as_uuid()),
                channel: None,
                thread: None,
                child: None,
            },
            AgentConversationBoundary::ChildBranch { run } => ScopeColumns {
                kind: "child_branch",
                child: Some(run.as_uuid()),
                channel: None,
                thread: None,
                session: None,
            },
        }
    }
}
struct ScopeColumns {
    kind: &'static str,
    channel: Option<Uuid>,
    thread: Option<Uuid>,
    session: Option<Uuid>,
    child: Option<Uuid>,
}

async fn resolve_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    bound: &WriteBound,
) -> AppResult<Option<AgentConversationIdentity>> {
    let Some(context) = authorized_on(tx, fence, bound).await? else {
        return Ok(None);
    };
    let id = conversation_on(tx, &context)
        .await?
        .ok_or_else(|| AppError::Conflict("Agent conversation storage is full".into()))?;
    if lease::live_window(tx, fence, bound.started())
        .await?
        .is_none()
    {
        return Err(ownership_lost());
    }
    Ok(Some(context.identity(id)))
}

async fn conversation_on(tx: &mut PgConnection, context: &Context) -> AppResult<Option<Uuid>> {
    if let Some(id) = find_on(tx, context).await? {
        return Ok(Some(id));
    }
    let proposed = Uuid::new_v4();
    let available: bool = sqlx::query_scalar("SELECT agent_history_conversation_available($1) AND agent_history_storage_available($1,$2)")
        .bind(context.company.as_uuid()).bind(proposed)
        .fetch_one(&mut *tx)
        .await?;
    if !available {
        return Ok(None);
    }
    let columns = context.columns();
    sqlx::query(
        "INSERT INTO agent_history_conversations \
         (company_id,id,agent_id,scope_kind,channel_id,thread_id,session_id,child_run_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING",
    )
    .bind(context.company.as_uuid())
    .bind(proposed)
    .bind(context.agent.as_uuid())
    .bind(columns.kind)
    .bind(columns.channel)
    .bind(columns.thread)
    .bind(columns.session)
    .bind(columns.child)
    .execute(&mut *tx)
    .await?;
    let id = find_on(tx, context).await?.ok_or_else(invalid)?;
    Ok(Some(id))
}

async fn find_on(db: &mut PgConnection, context: &Context) -> AppResult<Option<Uuid>> {
    let columns = context.columns();
    Ok(sqlx::query_scalar(
        "SELECT id FROM agent_history_conversations WHERE company_id=$1 AND agent_id=$2 \
         AND scope_kind=$3 AND channel_id IS NOT DISTINCT FROM $4::uuid \
         AND thread_id IS NOT DISTINCT FROM $5::uuid AND session_id IS NOT DISTINCT FROM $6::uuid \
         AND child_run_id IS NOT DISTINCT FROM $7::uuid",
    )
    .bind(context.company.as_uuid())
    .bind(context.agent.as_uuid())
    .bind(columns.kind)
    .bind(columns.channel)
    .bind(columns.thread)
    .bind(columns.session)
    .bind(columns.child)
    .fetch_optional(db)
    .await?)
}

async fn authorized_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    bound: &WriteBound,
) -> AppResult<Option<Context>> {
    let preparation::LockedAgent {
        actor,
        facts,
        selection,
    } = preparation::locked_on(tx, fence, bound).await?;
    let action = ActionScope {
        company: fence.scope.company,
        run: fence.scope.run,
        execution: fence.scope.execution,
    };
    if !agent_history_eligible(facts.kind) {
        return Ok(None);
    }
    let boundary = scope::boundary_on(tx, action, actor, &facts).await?;
    Ok(Some(Context {
        company: action.company,
        actor,
        agent: selection.agent().agent_id,
        boundary,
    }))
}

fn ownership_lost() -> AppError {
    AppError::Execution(crate::app_error::ExecutionFailure::OwnershipLost)
}
