//! One frozen action per local-agent step; native execution is owned by the connection.
use super::*;
use crate::adapters::persistence::local_agents::shared;
use crate::application::local_agents::workflow::{
    self as contract, FrozenLocalRun, LocalRunInputs,
};
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::{actions::*, lease::*, polling::WorkflowStepKind};
use crate::domain::local_agents::*;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::BTreeSet;

mod admission;
mod continuation;
mod delivery;
mod preparation;
mod resolution;

pub(super) use continuation::prelock_on as prelock_wait_on;
pub(super) use continuation::resume_on;
pub(in crate::adapters::persistence) use delivery::authorize_operation_on;

pub(super) async fn confirm_suspension(
    persistence: &PostgresPersistence,
    fence: WorkflowFence,
    wait: Uuid,
) -> AppResult<bool> {
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        let mut connection = persistence.pool().acquire().await?;
        continuation::confirm_on(&mut connection, fence, wait).await
    })
    .await
    .map_err(|_| lease::timed_out())?
}

type Dispatch = action_dispatch::PostgresActionDispatch<LocalActionAuthority, ()>;

#[derive(Clone)]
pub struct PostgresLocalAgentWorkflow {
    persistence: PostgresPersistence,
    notices: Option<crate::application::workflow::decisions::DecisionNotices>,
}

impl PostgresLocalAgentWorkflow {
    pub fn new(
        persistence: PostgresPersistence,
        notices: Option<crate::application::workflow::decisions::DecisionNotices>,
    ) -> Self {
        Self {
            persistence,
            notices,
        }
    }
}

#[async_trait]
impl contract::LocalAgentWorkflow for PostgresLocalAgentWorkflow {
    async fn run_local_agent(
        &self,
        claim: &ClaimedWorkflow,
        _inputs: &LocalRunInputs,
        policy: LeasePolicy,
    ) -> AppResult<contract::LocalRunProgress> {
        use super::persistence_bound::{Attempt, End, WriteBound};
        let bound = WriteBound::start("local_agent.run", policy);
        let dispatch = Dispatch::new(self.persistence.clone(), LocalActionAuthority, ());
        let attempted = bound
            .attempt(Box::pin(async {
                let mut connection = bound.acquire(self.persistence.pool()).await?;
                let mut tx = bound.begin(&mut connection).await?;
                let result =
                    admission::run_on(&mut tx, claim, policy, &dispatch, self.notices.as_ref())
                        .await;
                bound.end(tx, result.map(End::Commit)).await
            }))
            .await;
        match attempted {
            Attempt::Done(result) => Ok(result),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(resolution::resolve_on(
                        self, &dispatch, claim, policy, &bound,
                    )))
                    .await
            }
        }
    }
}

#[derive(sqlx::FromRow)]
struct SavedCall {
    invocation_id: Uuid,
    operation: Value,
    wait_id: Option<Uuid>,
    dispatch_id: Option<Uuid>,
}

impl SavedCall {
    fn action(&self) -> AppResult<FrozenAction> {
        let action = FrozenAction::restore(self.operation.clone())?;
        // A stored matching digest alone is insufficient for this privileged builtin.
        FrozenAction::local_agent(action.request().clone())
    }

    fn subject(&self) -> AppResult<ApprovalSubject> {
        Ok(ApprovalSubject {
            invocation: ActionInvocationId::new(self.invocation_id),
            argument_digest: self.action()?.argument_digest().clone(),
        })
    }
}

async fn saved_on(db: &mut PgConnection, scope: ActionScope) -> AppResult<Option<SavedCall>> {
    Ok(sqlx::query_as::<_, SavedCall>(
        "SELECT call.invocation_id,intent.operation,call.wait_id,call.dispatch_id \
         FROM workflow_local_agent_calls AS call JOIN workflow_action_intents AS intent \
           ON intent.company_id=call.company_id AND intent.id=call.invocation_id \
         WHERE call.company_id=$1 AND call.run_id=$2 AND call.execution_id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_optional(db)
    .await?)
}

fn action_scope(scope: ActivationRequest) -> ActionScope {
    ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    }
}

/// Both current and original workflow associations must authorize session continuation.
/// Run rows precede machine/profile locks in every caller.
#[derive(sqlx::FromRow, PartialEq, Eq)]
pub(in crate::adapters::persistence) struct RunAssociation {
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
}

pub(in crate::adapters::persistence) async fn authorize_run_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: Uuid,
    actor: WorkflowActor,
) -> AppResult<RunAssociation> {
    let stored = sqlx::query_as::<_, RunAssociation>(
        "SELECT channel_id,thread_id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(run)
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    let association = match (stored.channel_id, stored.thread_id) {
        (None, None) => RelatedAssociation::Company,
        (Some(channel), None) => RelatedAssociation::Channel(RelatedChannelId::new(channel)),
        (Some(channel), Some(thread)) => RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(channel),
            thread_id: RelatedThreadId::new(thread),
        },
        _ => return Err(invalid()),
    };
    association::authorize_association(db, company, actor, association).await?;
    Ok(stored)
}

struct LocalActionAuthority;

#[async_trait]
impl action_dispatch::SqlActionAuthority for LocalActionAuthority {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        authority: &ActionRunAuthority,
    ) -> AppResult<action_dispatch::LockedActionAuthority> {
        let action = FrozenAction::local_agent(authority.action.request().clone())?;
        authority::authorize_company(tx, action.scope().company, authority.actor).await?;
        let args: FrozenLocalRun =
            serde_json::from_value(action.request().arguments.clone()).map_err(|_| invalid())?;
        authorize_run_on(
            tx,
            action.scope().company,
            action.scope().run.as_uuid(),
            authority.actor,
        )
        .await?;
        if let Some(id) = args.session_id {
            let session = shared::session_on(tx, action.scope().company, id)
                .await?
                .ok_or_else(missing)?;
            preparation::validate_continuation_on(
                tx,
                action.scope(),
                authority.actor,
                args.profile_id,
                &session,
            )
            .await?;
        }
        let locked = shared::lock_profile_on(tx, action.scope().company, args.profile_id).await?;
        if locked.machine.state != MachineState::Enabled {
            return Err(conflict());
        }
        shared::validate_selection_on(
            tx,
            action.scope().company,
            locked.machine.id,
            &args.selection,
        )
        .await?;
        Ok(action_dispatch::LockedActionAuthority {
            resource: crate::application::workflow::binding::ResourceStatus {
                id: RuntimeResourceId::new(args.profile_id.as_uuid()),
                company_id: action.scope().company,
                kind: TypeName::parse(contract::RESOURCE_KIND).map_err(|_| invalid())?,
                supported_contracts: BTreeSet::from([
                    TypeName::parse(contract::KIND).map_err(|_| invalid())?
                ]),
                authorized: true,
                readiness: crate::application::workflow::binding::ResourceReadiness::Ready,
            },
            policy: CurrentActionPolicy {
                tool: contract::builtin_snapshot(action.scope().company)?,
                approval_required: args.approval.is_some(),
            },
        })
    }
}
