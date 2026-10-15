use super::invalid;
use super::*;
use crate::application::workflow::actions::*;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(sqlx::FromRow)]
struct RunAuthorityRow {
    actor_id: Uuid,
    version_id: Uuid,
    workflow_id: Uuid,
    bundle: Vec<u8>,
    resources: Value,
}

#[async_trait]
impl ActionAuthorities for PostgresPersistence {
    async fn action_authority(
        &self,
        scope: ActionScope,
        subject: &ApprovalSubject,
    ) -> AppResult<ActionRunAuthority> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','5s',true), set_config('statement_timeout','5s',true)").execute(&mut *tx).await?;
        let actor = prelock_on(&mut tx, scope).await?;
        let authority = load_on(&mut tx, scope, subject, actor).await?;
        tx.commit().await?;
        Ok(authority)
    }
}

/// The unlocked actor is only a lock-order hint. `load_on` verifies it again under
/// the run lock. Keep a denied authority distinct from operational errors so receipt
/// return can still report known lost ownership before reporting access denial.
pub(super) async fn prelock_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActionScope,
) -> AppResult<Option<WorkflowActor>> {
    let actor: Option<Uuid> =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_optional(&mut **tx)
            .await?;
    let Some(actor) = actor else { return Ok(None) };
    let actor = WorkflowActor::authenticated(actor)?;
    match authority::authorize_company(tx, scope.company, actor).await {
        Ok(()) => Ok(Some(actor)),
        Err(AppError::NotFound(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) async fn load_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActionScope,
    subject: &ApprovalSubject,
    actor: Option<WorkflowActor>,
) -> AppResult<ActionRunAuthority> {
    let actor = actor.ok_or_else(missing)?;
    let run = sqlx::query_as::<_, RunAuthorityRow>(
        "SELECT actor_id,version_id,workflow_id,bundle,resources FROM workflow_runs \
             WHERE company_id=$1 AND id=$2 AND state IN ('queued','running') \
             AND deadline > clock_timestamp() FOR UPDATE",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    if run.actor_id != actor.user_id() {
        return Err(conflict());
    }
    let operation: Value = sqlx::query_scalar(
        "SELECT operation FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2 \
             AND execution_id=$3 AND id=$4 AND argument_digest=$5",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(subject.invocation.as_uuid())
    .bind(subject.argument_digest.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    let action = FrozenAction::restore(operation)?;
    let bundle = restore_bundle(
        &run.bundle,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    if action.scope() != scope
        || action.argument_digest() != &subject.argument_digest
        || bundle.company_id() != scope.company
        || bundle.compiled().graph().definition().version_id.as_uuid() != run.version_id
        || bundle.compiled().graph().definition().workflow_id.as_uuid() != run.workflow_id
    {
        return Err(invalid());
    }
    let resources: BTreeMap<_, _> = serde_json::from_value(run.resources).map_err(|_| invalid())?;
    if resources.len() > 256 {
        return Err(invalid());
    }
    let active: bool = sqlx::query_scalar(
        "SELECT state IN ('queued','running') AND deadline > clock_timestamp() \
             FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    if !active {
        return Err(conflict());
    }
    Ok(ActionRunAuthority {
        action,
        actor,
        bundle,
        resources,
    })
}
