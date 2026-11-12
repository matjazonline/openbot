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
        let authority = load_on(&mut tx, scope, subject, actor, RunPhase::Claimed).await?;
        tx.commit().await?;
        Ok(authority)
    }
}

/// The unlocked actor is only a lock-order hint. `load_on` verifies it again under
/// the run lock. Keep a denied authority distinct from operational errors so receipt
/// return can still report known lost ownership before reporting access denial.
pub(super) fn prelock_on(
    db: &mut PgConnection,
    scope: ActionScope,
) -> impl std::future::Future<Output = AppResult<Option<WorkflowActor>>> + '_ {
    prelock_accounts_on(db, scope, None)
}

pub(super) async fn prelock_accounts_on(
    tx: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
) -> AppResult<Option<WorkflowActor>> {
    prelock_accounts_with_locks_on(tx, scope, reviewer, super::row_locks::RowLocks::Wait).await
}

pub(super) async fn prelock_accounts_with_locks_on(
    tx: &mut PgConnection,
    scope: ActionScope,
    reviewer: Option<WorkflowActor>,
    locks: super::row_locks::RowLocks,
) -> AppResult<Option<WorkflowActor>> {
    let actor: Option<Uuid> =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_optional(&mut *tx)
            .await?;
    let Some(actor) = actor else { return Ok(None) };
    // Provisioning may act for a signed-in message author instead of the run owner.
    // Both accounts precede the company lock; this hint never authorizes the effect.
    let requester = super::provisioning::requester_on(tx, scope).await?;
    let mut accounts = vec![actor];
    if let Some(requester) = requester {
        accounts.push(requester.user_id());
    }
    if let Some(reviewer) = reviewer {
        accounts.push(reviewer.user_id());
    }
    let policy: Option<Value> = sqlx::query_scalar("SELECT context.approval_policy FROM channel_reply_runs AS owned \
        JOIN channel_reply_contexts AS context USING(company_id,intent_id) WHERE owned.company_id=$1 AND owned.run_id=$2 AND context.approval_policy IS NOT NULL")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_optional(&mut *tx).await?;
    if let Some(policy) = policy {
        let policy: crate::application::workflow::channel_reply::ReplyApprovalPolicy =
            serde_json::from_value(policy).map_err(|_| invalid())?;
        accounts.extend_from_slice(policy.reviewers());
    }
    accounts.sort_unstable();
    accounts.dedup();
    let locked: Vec<Uuid> = sqlx::query_scalar(
        &locks.sql("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR SHARE"),
    )
    .bind(&accounts)
    .fetch_all(&mut *tx)
    .await?;
    if !locked.contains(&actor) {
        return Err(missing());
    }
    let actor = WorkflowActor::authenticated(actor)?;
    match authority::authorize_company_with_locks(tx, scope.company, actor, locks).await {
        Ok(_) => {
            if super::provisioning::requester_on(tx, scope).await? != requester {
                return Err(conflict());
            }
            Ok(Some(actor))
        }
        Err(AppError::NotFound(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) enum RunPhase {
    Claimed,
    LocalAgentWait,
}

pub(super) async fn load_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActionScope,
    subject: &ApprovalSubject,
    actor: Option<WorkflowActor>,
    phase: RunPhase,
) -> AppResult<ActionRunAuthority> {
    load_with_locks_on(
        tx,
        scope,
        subject,
        actor,
        phase,
        super::row_locks::RowLocks::Wait,
    )
    .await
}

pub(super) async fn load_with_locks_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActionScope,
    subject: &ApprovalSubject,
    actor: Option<WorkflowActor>,
    phase: RunPhase,
    locks: super::row_locks::RowLocks,
) -> AppResult<ActionRunAuthority> {
    let actor = actor.ok_or_else(missing)?;
    let states: &[&str] = match phase {
        RunPhase::Claimed => &["queued", "running"],
        RunPhase::LocalAgentWait => &["waiting"],
    };
    let run = sqlx::query_as::<_, RunAuthorityRow>(
        &locks.sql("SELECT actor_id,version_id,workflow_id,stored.bundle AS bundle,resources FROM workflow_runs AS run LEFT JOIN workflow_bundles AS stored ON stored.company_id=run.company_id AND stored.digest=run.bundle_digest \
             WHERE run.company_id=$1 AND id=$2 AND state=ANY($3) \
             AND deadline > clock_timestamp() FOR UPDATE OF run"),
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(states)
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
        "SELECT state=ANY($3) AND deadline > clock_timestamp() \
             FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(states)
    .fetch_one(&mut **tx)
    .await?;
    if !active {
        return Err(conflict());
    }
    let origin =
        super::agent_origin::authority_with_locks_on(tx, action.request(), actor, &bundle, locks)
            .await?;
    if let ActionOriginAuthority::Agent(selection) = &origin {
        super::agent_authority::check_execution_with_locks_on(
            tx,
            scope,
            selection.agent(),
            AgentUseMode::Tools,
            locks,
        )
        .await?;
    }
    Ok(ActionRunAuthority {
        origin,
        action,
        actor,
        bundle,
        resources,
    })
}
