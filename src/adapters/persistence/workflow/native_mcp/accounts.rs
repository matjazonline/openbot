//! Account hints for live effects only. Historical readers never enter this module.
use super::super::action_authority::ActorAccountHints;
use super::*;
use crate::application::workflow::actions::ActionScope;

#[derive(PartialEq, Eq)]
pub(in crate::adapters::persistence::workflow) struct NativeAccounts {
    call: Uuid,
    operation: Uuid,
    actor: Uuid,
    source: Option<(ActionScope, ActorAccountHints)>,
}

impl NativeAccounts {
    pub(in crate::adapters::persistence::workflow) fn users(
        &self,
    ) -> impl Iterator<Item = Uuid> + '_ {
        std::iter::once(self.actor).chain(self.source.iter().flat_map(|(_, hints)| hints.users()))
    }
}

#[derive(sqlx::FromRow)]
struct Run {
    actor_id: Uuid,
    call_id: Option<Uuid>,
    private: bool,
}

#[derive(sqlx::FromRow)]
struct Source {
    operation_id: Uuid,
    actor_id: Uuid,
    operation_actor: Option<Uuid>,
    origin: String,
}

pub(in crate::adapters::persistence::workflow) async fn current_on(
    db: &mut PgConnection,
    scope: ActionScope,
) -> AppResult<Option<NativeAccounts>> {
    let run = sqlx::query_as::<_, Run>(
        "SELECT run.actor_id,run.native_mcp_call_id AS call_id,
         EXISTS(SELECT 1 FROM native_mcp_brokers AS broker WHERE broker.company_id=run.company_id
           AND broker.version_id=run.version_id) AS private
         FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    let Some(run) = run else { return Ok(None) };
    let Some(call) = run.call_id else {
        return if run.private {
            Err(missing())
        } else {
            Ok(None)
        };
    };
    if !run.private {
        return Err(missing());
    }
    let source = sqlx::query_as::<_, Source>(
        "SELECT call.operation_id,call.actor_id,operation.actor_id AS operation_actor,operation.origin
         FROM native_mcp_calls AS call JOIN local_agent_operations AS operation
           ON operation.company_id=call.company_id AND operation.id=call.operation_id
         WHERE call.company_id=$1 AND call.id=$2 AND call.run_id=$3 AND call.retired_at IS NULL",
    )
    .bind(scope.company.as_uuid())
    .bind(call)
    .bind(scope.run.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    if source.actor_id != run.actor_id || source.operation_actor != Some(run.actor_id) {
        return Err(missing());
    }
    let origin = crate::domain::local_agents::OperationOrigin::try_from(source.origin.as_str())
        .map_err(|_| invalid())?;
    let original = match origin {
        crate::domain::local_agents::OperationOrigin::Direct => None,
        crate::domain::local_agents::OperationOrigin::Workflow => {
            let (original, _) = super::super::local_agents::parked_subject_on(
                db,
                scope.company,
                OperationId::new(source.operation_id).map_err(|_| invalid())?,
            )
            .await?
            .ok_or_else(missing)?;
            let hints = super::super::action_authority::actor_hints_on(db, original, None)
                .await?
                .ok_or_else(missing)?;
            if hints.actor()?.user_id() != run.actor_id {
                return Err(missing());
            }
            Some((original, hints))
        }
    };
    Ok(Some(NativeAccounts {
        call,
        operation: source.operation_id,
        actor: run.actor_id,
        source: original,
    }))
}
