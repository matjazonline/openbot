//! Every desk belongs to a department and materializes its one-step audience.
//!
//! Own members/roles, parent Lead and direct child Leads receive participate/view grants.
//! Other provenances remain independent. Writers hold the company lock, then eligible
//! principal key shares, sorted desk gates and desk rows. Grant changes and their revision
//! fence commit with the membership, role, tree or settings change that caused them.
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::adapters::persistence::channel_gate::{
    ChannelGateAccess, ChannelGateKey, acquire_channel_gates_on,
};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::CompanyId;
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::entities::participant::{GrantProvenance, PrincipalCapability};
use crate::use_cases::department::department_not_found;

pub(crate) async fn desk_ownership_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desk: Uuid,
) -> AppResult<Option<DepartmentId>> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT department_id FROM channels WHERE company_id=$1 AND id=$2",
    )
    .bind(company.as_uuid())
    .bind(desk)
    .fetch_optional(&mut **tx)
    .await?
    .map(DepartmentId::new))
}

async fn lock_company_row_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
) -> AppResult<()> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
        .bind(company.as_uuid())
        .fetch_optional(&mut **tx)
        .await?
        .map(|_| ())
        .ok_or_else(|| AppError::NotFound("Company not found.".into()))
}

/// Resolve root-on-create / preserve-on-edit and lock the owner's current audience
/// before a caller takes any existing desk gate.
pub(crate) async fn prelock_ownership_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desk: Option<Uuid>,
    requested: Option<DepartmentId>,
) -> AppResult<DepartmentId> {
    lock_company_row_on(tx, company).await?;
    let current = match desk {
        Some(desk) => desk_ownership_on(tx, company, desk)
            .await?
            .ok_or_else(crate::use_cases::channel::channel_not_found)?,
        None => super::company::root_on(tx, company).await?,
    };
    let target = requested.unwrap_or(current);
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM departments WHERE company_id=$1 AND id=$2 FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(target.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(department_not_found)?;
    let eligible = eligible_on(
        tx,
        company,
        &[DeskDepartment {
            channel_id: Uuid::nil(),
            department_id: target.as_uuid(),
        }],
    )
    .await?;
    lock_principals_on(tx, company, &eligible).await?;
    Ok(target)
}

#[derive(Debug, sqlx::FromRow)]
struct DeskDepartment {
    channel_id: Uuid,
    department_id: Uuid,
}

#[derive(Debug, sqlx::FromRow)]
struct Eligible {
    channel_id: Uuid,
    principal_id: Uuid,
}

const ELIGIBLE: &str = "
    WITH desk(channel_id, department_id) AS (
        SELECT * FROM unnest($2::uuid[], $3::uuid[])
    ), owner AS (
        SELECT desk.channel_id, department.id AS department_id, department.parent_id
          FROM desk
          JOIN departments AS department
            ON department.company_id = $1 AND department.id = desk.department_id
    )
    SELECT owner.channel_id, member.principal_id
      FROM owner JOIN department_members AS member
        ON member.company_id = $1 AND member.department_id = owner.department_id
    UNION
    SELECT owner.channel_id, holder.principal_id
      FROM owner JOIN department_roles AS holder
        ON holder.company_id = $1 AND holder.department_id = owner.department_id
    UNION
    SELECT owner.channel_id, parent_lead.principal_id
      FROM owner JOIN department_roles AS parent_lead
        ON parent_lead.company_id = $1 AND parent_lead.department_id = owner.parent_id
       AND parent_lead.role = $4
    UNION
    SELECT owner.channel_id, child_lead.principal_id
      FROM owner
      JOIN departments AS child
        ON child.company_id = $1 AND child.parent_id = owner.department_id
      JOIN department_roles AS child_lead
        ON child_lead.company_id = $1 AND child_lead.department_id = child.id
       AND child_lead.role = $4
";

async fn eligible_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desks: &[DeskDepartment],
) -> AppResult<Vec<Eligible>> {
    let channels: Vec<_> = desks.iter().map(|desk| desk.channel_id).collect();
    let departments: Vec<_> = desks.iter().map(|desk| desk.department_id).collect();
    Ok(sqlx::query_as(ELIGIBLE)
        .bind(company.as_uuid())
        .bind(&channels)
        .bind(&departments)
        .bind(DepartmentRole::Lead.as_str())
        .fetch_all(&mut **tx)
        .await?)
}

/// Lock the principals `FOR KEY SHARE`, sorted, and keep the ones still a person or agent: a
/// principal demoted meanwhile has left its departments and is skipped.
async fn lock_principals_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    eligible: &[Eligible],
) -> AppResult<Vec<Uuid>> {
    let mut principals: Vec<Uuid> = eligible.iter().map(|row| row.principal_id).collect();
    principals.sort_unstable();
    principals.dedup();
    if principals.is_empty() {
        return Ok(principals);
    }
    Ok(sqlx::query_scalar(
        "SELECT principal.id FROM principals AS principal \
          WHERE principal.company_id = $1 AND principal.id = ANY($2) \
            AND principal.kind IN ('person', 'agent') \
          ORDER BY principal.id FOR KEY SHARE",
    )
    .bind(company.as_uuid())
    .bind(&principals)
    .fetch_all(&mut **tx)
    .await?)
}

const DELETE_STALE: &str = "
    WITH desired AS (
        SELECT audience.channel_id, audience.principal_id, capability.value AS capability
          FROM unnest($3::uuid[], $4::uuid[]) AS audience(channel_id, principal_id)
          CROSS JOIN unnest($5::text[]) AS capability(value)
    )
    DELETE FROM channel_principal_grants AS permission
     WHERE permission.company_id=$1 AND permission.channel_id=ANY($2)
       AND permission.provenance=$6
       AND NOT EXISTS (SELECT 1 FROM desired
            WHERE desired.channel_id=permission.channel_id
              AND desired.principal_id=permission.principal_id
              AND desired.capability=permission.capability)
";

// Repeat the bounded desk set inside the anti-join: generic plans on fresh tables
// otherwise materialize every company grant for every desired row.
const INSERT_MISSING: &str = "
    WITH desired AS (
        SELECT audience.channel_id, audience.principal_id, capability.value AS capability
          FROM unnest($3::uuid[], $4::uuid[]) AS audience(channel_id, principal_id)
          CROSS JOIN unnest($5::text[]) AS capability(value)
    )
    INSERT INTO channel_principal_grants
        (company_id, channel_id, principal_id, capability, provenance)
    SELECT $1, desired.channel_id, desired.principal_id, desired.capability, $6
      FROM desired WHERE desired.channel_id=ANY($2)
       AND NOT EXISTS (SELECT 1 FROM channel_principal_grants AS permission
            WHERE permission.company_id=$1 AND permission.channel_id=ANY($2)
              AND permission.channel_id=desired.channel_id
              AND permission.principal_id=desired.principal_id
              AND permission.capability=desired.capability AND permission.provenance=$6)
    ON CONFLICT (company_id, channel_id, principal_id, capability, provenance) DO NOTHING
";

/// Reconcile the derived set without deleting and reinserting unchanged grants. This
/// preserves grant identity/created_at and avoids a full rewrite on each root invite.
async fn reconcile_grants_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desks: &[Uuid],
    eligible: &[Eligible],
) -> AppResult<()> {
    let channels: Vec<_> = eligible.iter().map(|row| row.channel_id).collect();
    let principals: Vec<_> = eligible.iter().map(|row| row.principal_id).collect();
    let capabilities = [
        PrincipalCapability::Participate.as_str(),
        PrincipalCapability::View.as_str(),
    ];
    for statement in [DELETE_STALE, INSERT_MISSING] {
        sqlx::query(statement)
            .bind(company.as_uuid())
            .bind(desks)
            .bind(&channels)
            .bind(&principals)
            .bind(&capabilities[..])
            .bind(GrantProvenance::DepartmentMembership.as_str())
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Caller holds the company lock. A desk deleted before its gate is obtained is skipped.
async fn rederive_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desks: &[Uuid],
) -> AppResult<()> {
    if desks.is_empty() {
        return Ok(());
    }
    let owned: Vec<DeskDepartment> = sqlx::query_as(
        "SELECT id AS channel_id, department_id FROM channels \
          WHERE company_id=$1 AND id=ANY($2)",
    )
    .bind(company.as_uuid())
    .bind(desks)
    .fetch_all(&mut **tx)
    .await?;
    let eligible = eligible_on(tx, company, &owned).await?;
    let current = lock_principals_on(tx, company, &eligible).await?;
    acquire_channel_gates_on(
        tx,
        desks
            .iter()
            .map(|desk| ChannelGateKey::new(company.as_uuid(), *desk)),
        ChannelGateAccess::ChangeSettings,
    )
    .await?;
    let locked: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM channels WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR UPDATE",
    )
    .bind(company.as_uuid())
    .bind(desks)
    .fetch_all(&mut **tx)
    .await?;
    if locked.is_empty() {
        return Ok(());
    }
    let eligible: Vec<_> = eligible
        .into_iter()
        .filter(|row| {
            locked.binary_search(&row.channel_id).is_ok()
                && current.binary_search(&row.principal_id).is_ok()
        })
        .collect();
    reconcile_grants_on(tx, company, &locked, &eligible).await?;
    sqlx::query("UPDATE channels SET settings_revision=settings_revision+1 WHERE company_id=$1 AND id=ANY($2)")
        .bind(company.as_uuid()).bind(&locked).execute(&mut **tx).await?;
    Ok(())
}

/// All desks of these departments carry derived grants, including Public and root desks.
pub(crate) async fn sync_department_grants_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    departments: &[DepartmentId],
) -> AppResult<()> {
    let ids: Vec<_> = departments.iter().map(|id| id.as_uuid()).collect();
    let desks: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM channels WHERE company_id=$1 AND department_id=ANY($2) ORDER BY id",
    )
    .bind(company.as_uuid())
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    rederive_on(tx, company, &desks).await
}

pub(crate) async fn sync_desk_grants_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desk: Uuid,
) -> AppResult<()> {
    rederive_on(tx, company, &[desk]).await
}

/// Reassignment is the only ownership mutation; a stored owner can never be cleared.
#[cfg(test)]
pub(crate) async fn set_desk_ownership_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desk: Uuid,
    department: DepartmentId,
) -> AppResult<()> {
    let target = prelock_ownership_on(tx, company, Some(desk), Some(department)).await?;
    acquire_channel_gates_on(
        tx,
        [ChannelGateKey::new(company.as_uuid(), desk)],
        ChannelGateAccess::ChangeSettings,
    )
    .await?;
    sqlx::query_scalar::<_, Uuid>(
        "UPDATE channels SET department_id=$3 WHERE company_id=$1 AND id=$2 RETURNING id",
    )
    .bind(company.as_uuid())
    .bind(desk)
    .bind(target.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(crate::use_cases::channel::channel_not_found)?;
    sync_desk_grants_on(tx, company, desk).await?;
    Box::pin(super::workflow::follow_reviewers::refresh_on(tx, company)).await
}
