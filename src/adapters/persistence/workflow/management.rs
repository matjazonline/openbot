//! Configuration authority preserves person/company locks without granting history access.
use super::*;
use crate::application::ports::administration::ManagementTarget;
use crate::entities::{
    agent_capability::AgentId, department::DepartmentId, transport::PrincipalId,
};
use sqlx::PgConnection;

/// Public management transactions acquire the actor account before company authority.
/// Nested `_on` callers retain their already-established account/transaction prefix.
pub(in crate::adapters::persistence) fn begin(
    persistence: &PostgresPersistence,
    company: CompanyId,
    actor: WorkflowActor,
) -> impl std::future::Future<Output = AppResult<Transaction<'_, Postgres>>> + Send {
    super::super::company_invite::authority::start(
        persistence.pool(),
        super::super::company_invite::authority::TeamScope {
            company_id: company.as_uuid(),
            actor_id: actor.user_id(),
        },
        None,
    )
}

pub(in crate::adapters::persistence) async fn authorize_target_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    target: ManagementTarget,
) -> AppResult<crate::entities::access_level::LevelActor> {
    let level =
        authority::authorize_person_with_locks(db, company, actor, row_locks::RowLocks::Wait)
            .await?;
    super::super::administration::management_role_on(db, actor.user_id(), company, target)
        .await?
        .ok_or_else(missing)?;
    Ok(level)
}

/// Captures can prepare a new draft; existing definitions always use their stored/bound owner.
pub(in crate::adapters::persistence) fn authorize_publication_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    workflow: WorkflowId,
) -> impl std::future::Future<Output = AppResult<crate::entities::access_level::LevelActor>> + Send + '_
{
    authorize_target_on(
        db,
        company,
        actor,
        ManagementTarget::DefinitionCreation(workflow),
    )
}

pub(in crate::adapters::persistence) fn authorize_association_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    association: RelatedAssociation,
) -> impl std::future::Future<Output = AppResult<crate::entities::access_level::LevelActor>> + Send + '_
{
    authorize_association_with_locks(db, company, actor, association, row_locks::RowLocks::Wait)
}

async fn target_on(
    db: &mut PgConnection,
    company: CompanyId,
    association: RelatedAssociation,
    locks: row_locks::RowLocks,
) -> AppResult<ManagementTarget> {
    match association.target() {
        None => Ok(ManagementTarget::Root),
        Some(AddressTarget::Desk(desk)) => {
            let unit: Uuid =
                sqlx::query_scalar(&locks.sql(
                    "SELECT department_id FROM channels WHERE company_id=$1 AND id=$2 FOR SHARE",
                ))
                .bind(company.as_uuid())
                .bind(desk)
                .fetch_optional(db)
                .await?
                .ok_or_else(missing)?;
            Ok(ManagementTarget::Unit(DepartmentId::new(unit)))
        }
        Some(AddressTarget::Principal(principal)) => {
            let agent: Option<Uuid> = sqlx::query_scalar(
                &locks
                    .sql("SELECT agent_id FROM principals WHERE company_id=$1 AND id=$2 FOR SHARE"),
            )
            .bind(company.as_uuid())
            .bind(principal.as_uuid())
            .fetch_optional(db)
            .await?
            .ok_or_else(missing)?;
            Ok(agent.map_or(ManagementTarget::Root, |id| {
                ManagementTarget::Agent(AgentId::new(id))
            }))
        }
    }
}

/// The actor's management role over `association`'s owner, read under the caller's locks, for a
/// management event's history (Plan06 02B); `None` when the actor manages none of it.
pub(in crate::adapters::persistence) async fn association_role_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    association: RelatedAssociation,
) -> AppResult<Option<crate::application::ports::administration::ManagementRole>> {
    let target = target_on(db, company, association, row_locks::RowLocks::Wait).await?;
    super::super::administration::management_role_on(db, actor.user_id(), company, target).await
}

pub(in crate::adapters::persistence) async fn authorize_association_with_locks(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    association: RelatedAssociation,
    locks: row_locks::RowLocks,
) -> AppResult<crate::entities::access_level::LevelActor> {
    let level = authority::authorize_person_with_locks(db, company, actor, locks).await?;
    let target = target_on(db, company, association, locks).await?;
    super::super::administration::management_role_on(db, actor.user_id(), company, target)
        .await?
        .ok_or_else(missing)?;
    if let RelatedAssociation::Thread { target, thread_id } = association {
        sqlx::query_scalar::<_, Uuid>(&locks.sql(
            "SELECT id FROM threads WHERE company_id=$1 AND id=$2 \
             AND channel_id IS NOT DISTINCT FROM $3::uuid \
             AND principal_id IS NOT DISTINCT FROM $4::uuid FOR SHARE",
        ))
        .bind(company.as_uuid())
        .bind(thread_id.as_uuid())
        .bind(target.desk_id())
        .bind(target.principal_id().map(PrincipalId::as_uuid))
        .fetch_optional(db)
        .await?
        .ok_or_else(missing)?;
    }
    Ok(level)
}

pub(in crate::adapters::persistence) async fn authorize_run_with_locks(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    actor: WorkflowActor,
    locks: row_locks::RowLocks,
) -> AppResult<crate::entities::access_level::LevelActor> {
    // The existing caller has taken its sorted account prefix. Company ownership must
    // precede reading the immutable run association, including native NOWAIT paths.
    authority::authorize_person_with_locks(db, company, actor, locks).await?;
    #[derive(sqlx::FromRow)]
    struct Target {
        channel_id: Option<Uuid>,
        principal_id: Option<Uuid>,
        thread_id: Option<Uuid>,
    }
    let target: Target = sqlx::query_as(
        "SELECT channel_id,principal_id,thread_id FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    let association = association::association_from_columns(association::AssociationColumns {
        channel_id: target.channel_id,
        principal_id: target.principal_id,
        thread_id: target.thread_id,
    })?;
    authorize_association_with_locks(db, company, actor, association, locks).await
}
