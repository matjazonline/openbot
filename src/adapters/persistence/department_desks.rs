//! Desks owned by departments, and who a members-only desk admits (01-departments p02).
//!
//! A members-only desk owned by department D admits D's members, D's role holders, the lead of
//! D's parent and the lead of each department directly under D: one step both ways
//! (01DEP-C002). Those `department_membership` grants are rewritten wholesale, in the same
//! transaction as every change to membership, roles, the tree or the desk's ownership; grants of
//! any other provenance are never touched. Only members-only desks hold them.
//!
//! Lock order, as every department and desk-ownership writer takes it (DEP-P01-D3, DEP-P02-D3):
//! the caller holds the company write lock, so the desk set and the eligible principals read
//! here cannot change under it; then the eligible principals `FOR KEY SHARE`, then the desks'
//! gates in one sorted call, then the desk rows.

use std::str::FromStr;

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::adapters::persistence::channel_gate::{
    ChannelGateAccess, ChannelGateKey, acquire_channel_gates_on,
};
use crate::app_error::{AppError, AppResult};
use crate::application::ports::channel::ChannelOwnershipWrite;
use crate::domain::workflow::CompanyId;
use crate::entities::channel::ChannelAccessMode;
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::entities::participant::{GrantProvenance, PrincipalCapability};
use crate::use_cases::department::department_not_found;

/// Who owns a desk and whether only the department may use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeskOwnership {
    pub(crate) department_id: Option<DepartmentId>,
    pub(crate) members_only: bool,
}

impl DeskOwnership {
    pub(crate) const UNOWNED: Self = Self {
        department_id: None,
        members_only: false,
    };

    /// This ownership with an edit's fields applied; absent fields keep their value.
    pub(crate) fn merged(self, edit: &ChannelOwnershipWrite) -> Self {
        Self {
            department_id: edit.department_id.unwrap_or(self.department_id),
            members_only: edit.members_only.unwrap_or(self.members_only),
        }
    }

    /// The access mode a desk stores: a members-only desk is an allowlist desk whatever its email
    /// list says, so `Channel::participant_access` and `viewer_access` need no rule of their own
    /// (02B.1). Otherwise the mode follows the email list.
    pub(crate) fn stored_mode(
        self,
        from_emails: ChannelAccessMode,
    ) -> AppResult<ChannelAccessMode> {
        if !self.members_only {
            return Ok(from_emails);
        }
        if self.department_id.is_none() {
            return Err(AppError::BadRequest(
                "A members-only desk must belong to a department.".into(),
            ));
        }
        match from_emails {
            ChannelAccessMode::Public => Err(AppError::BadRequest(
                "A members-only desk cannot be open to @public.".into(),
            )),
            ChannelAccessMode::Team | ChannelAccessMode::Allowlist => {
                Ok(ChannelAccessMode::Allowlist)
            }
        }
    }
}

/// The desk's ownership as stored, read under whatever lock the caller holds.
pub(crate) async fn desk_ownership_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desk: Uuid,
) -> AppResult<Option<DeskOwnership>> {
    let row: Option<(Option<Uuid>, bool)> = sqlx::query_as(
        "SELECT channel.department_id, channel.members_only FROM channels AS channel \
          WHERE channel.company_id = $1 AND channel.id = $2",
    )
    .bind(company.as_uuid())
    .bind(desk)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|(department_id, members_only)| DeskOwnership {
        department_id: department_id.map(DepartmentId::new),
        members_only,
    }))
}

/// Take the company write lock; desk ownership changes serialize with every department write.
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

/// The locks an ownership change of `desk` needs before its gate, in the plan-wide order: the
/// company row, the target department `FOR SHARE` (it must be this company's), and the
/// principals the desk will admit `FOR KEY SHARE`. Returns the ownership the desk will have.
/// `desk` is `None` for a desk not created yet.
pub(crate) async fn prelock_ownership_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desk: Option<Uuid>,
    edit: &ChannelOwnershipWrite,
) -> AppResult<DeskOwnership> {
    lock_company_row_on(tx, company).await?;
    let current = match desk {
        Some(desk) => desk_ownership_on(tx, company, desk)
            .await?
            .ok_or_else(crate::use_cases::channel::channel_not_found)?,
        None => DeskOwnership::UNOWNED,
    };
    let target = current.merged(edit);
    if let Some(department) = target.department_id {
        let found: Option<Uuid> = sqlx::query_scalar(
            "SELECT department.id FROM departments AS department \
              WHERE department.company_id = $1 AND department.id = $2 FOR SHARE",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .fetch_optional(&mut **tx)
        .await?;
        found.ok_or_else(department_not_found)?;
        if target.members_only {
            let eligible = eligible_on(tx, company, &[(Uuid::nil(), department.as_uuid())]).await?;
            lock_principals_on(tx, company, &eligible).await?;
        }
    }
    Ok(target)
}

/// One desk and one principal it admits.
#[derive(Debug, sqlx::FromRow)]
struct Eligible {
    channel_id: Uuid,
    principal_id: Uuid,
}

/// Who each `(desk, owning department)` pair admits: members and role holders of the department,
/// the lead of its parent, the lead of each department directly under it.
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
    desks: &[(Uuid, Uuid)],
) -> AppResult<Vec<Eligible>> {
    let (channels, departments): (Vec<Uuid>, Vec<Uuid>) = desks.iter().copied().unzip();
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

/// Re-derive the desks' `department_membership` grants. Every derived grant they hold goes,
/// whatever the desk is now; each desk that is members-only then gets `participate` and `view`
/// for each eligible principal; every desk's settings move on, so a message routed under the old
/// grants is refused at its commit. The caller holds the company write lock.
async fn rederive_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desks: &[Uuid],
) -> AppResult<()> {
    if desks.is_empty() {
        return Ok(());
    }
    let owned: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT channel.id, channel.department_id FROM channels AS channel \
          WHERE channel.company_id = $1 AND channel.id = ANY($2) \
            AND channel.members_only AND channel.department_id IS NOT NULL",
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
    // A desk deleted since it was read is skipped: deleting a desk takes no company lock.
    let locked: Vec<Uuid> = sqlx::query_scalar(
        "SELECT channel.id FROM channels AS channel \
          WHERE channel.company_id = $1 AND channel.id = ANY($2) \
          ORDER BY channel.id FOR UPDATE",
    )
    .bind(company.as_uuid())
    .bind(desks)
    .fetch_all(&mut **tx)
    .await?;
    if locked.is_empty() {
        return Ok(());
    }
    let provenance = GrantProvenance::DepartmentMembership.as_str();
    sqlx::query(
        "DELETE FROM channel_principal_grants AS derived \
          WHERE derived.company_id = $1 AND derived.channel_id = ANY($2) \
            AND derived.provenance = $3",
    )
    .bind(company.as_uuid())
    .bind(&locked)
    .bind(provenance)
    .execute(&mut **tx)
    .await?;
    let (channels, principals): (Vec<Uuid>, Vec<Uuid>) = eligible
        .iter()
        .filter(|row| {
            locked.binary_search(&row.channel_id).is_ok()
                && current.binary_search(&row.principal_id).is_ok()
        })
        .map(|row| (row.channel_id, row.principal_id))
        .unzip();
    for capability in [PrincipalCapability::Participate, PrincipalCapability::View] {
        sqlx::query(
            "INSERT INTO channel_principal_grants \
                 (company_id, channel_id, principal_id, capability, provenance) \
             SELECT $1, eligible.channel_id, eligible.principal_id, $4, $5 \
               FROM unnest($2::uuid[], $3::uuid[]) AS eligible(channel_id, principal_id) \
             ON CONFLICT (company_id, channel_id, principal_id, capability, provenance) DO NOTHING",
        )
        .bind(company.as_uuid())
        .bind(&channels)
        .bind(&principals)
        .bind(capability.as_str())
        .bind(provenance)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "UPDATE channels SET settings_revision = settings_revision + 1 \
          WHERE company_id = $1 AND id = ANY($2)",
    )
    .bind(company.as_uuid())
    .bind(&locked)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Re-derive every members-only desk any of `departments` owns. The caller holds the company
/// write lock. Desks that are not members-only hold no derived grants (an ownership change
/// re-derives its desk), so they are not visited.
pub(crate) async fn sync_department_grants_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    departments: &[DepartmentId],
) -> AppResult<()> {
    let ids: Vec<Uuid> = departments.iter().map(|id| id.as_uuid()).collect();
    let desks: Vec<Uuid> = sqlx::query_scalar(
        "SELECT channel.id FROM channels AS channel \
          WHERE channel.company_id = $1 AND channel.department_id = ANY($2) \
            AND channel.members_only \
          ORDER BY channel.id",
    )
    .bind(company.as_uuid())
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    rederive_on(tx, company, &desks).await
}

/// Re-derive one desk whose ownership changed. The caller holds the company write lock.
pub(crate) async fn sync_desk_grants_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desk: Uuid,
) -> AppResult<()> {
    rederive_on(tx, company, &[desk]).await
}

/// Set a desk's ownership inside the caller's transaction, which holds the company write lock:
/// the one way another writer (a department creating its desk) changes it, so it takes the same
/// locks, applies the same members-only rule and re-derives the same grants as a desk edit.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "phase 03 creates department desks through it")
)]
pub(crate) async fn set_desk_ownership_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desk: Uuid,
    edit: &ChannelOwnershipWrite,
) -> AppResult<()> {
    let target = prelock_ownership_on(tx, company, Some(desk), edit).await?;
    acquire_channel_gates_on(
        tx,
        [ChannelGateKey::new(company.as_uuid(), desk)],
        ChannelGateAccess::ChangeSettings,
    )
    .await?;
    let (stored, was_members_only, configured): (String, bool, bool) = sqlx::query_as(
        "SELECT channel.access_mode, channel.members_only, \
                EXISTS (SELECT 1 FROM channel_principal_grants AS configured \
                         WHERE configured.company_id = channel.company_id \
                           AND configured.channel_id = channel.id \
                           AND configured.provenance = $3) \
           FROM channels AS channel \
          WHERE channel.company_id = $1 AND channel.id = $2 FOR UPDATE OF channel",
    )
    .bind(company.as_uuid())
    .bind(desk)
    .bind(GrantProvenance::ConfiguredAllowlist.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(crate::use_cases::channel::channel_not_found)?;
    // The mode the email list alone gives: a members-only desk stores `allowlist` whatever its
    // list holds, so for one its configured entries decide.
    let from_emails = if was_members_only {
        if configured {
            ChannelAccessMode::Allowlist
        } else {
            ChannelAccessMode::Team
        }
    } else {
        ChannelAccessMode::from_str(&stored)
            .map_err(|error| AppError::Internal(error.to_string()))?
    };
    let mode = target.stored_mode(from_emails)?;
    sqlx::query(
        "UPDATE channels SET department_id = $3, members_only = $4, access_mode = $5, \
                settings_revision = settings_revision + 1 \
          WHERE company_id = $1 AND id = $2",
    )
    .bind(company.as_uuid())
    .bind(desk)
    .bind(target.department_id.map(DepartmentId::as_uuid))
    .bind(target.members_only)
    .bind(mode.as_str())
    .execute(&mut **tx)
    .await?;
    sync_desk_grants_on(tx, company, desk).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNED: DeskOwnership = DeskOwnership {
        department_id: Some(DepartmentId::new(Uuid::nil())),
        members_only: true,
    };

    #[test]
    fn a_members_only_desk_stores_allowlist_whatever_its_email_list() {
        for from_emails in [ChannelAccessMode::Team, ChannelAccessMode::Allowlist] {
            assert_eq!(
                OWNED.stored_mode(from_emails).unwrap(),
                ChannelAccessMode::Allowlist
            );
        }
        let public = OWNED.stored_mode(ChannelAccessMode::Public);
        assert!(matches!(public, Err(AppError::BadRequest(_))), "{public:?}");
        let unowned = DeskOwnership {
            department_id: None,
            members_only: true,
        }
        .stored_mode(ChannelAccessMode::Team);
        assert!(
            matches!(unowned, Err(AppError::BadRequest(_))),
            "{unowned:?}"
        );
    }

    #[test]
    fn a_desk_that_is_not_members_only_follows_its_email_list() {
        let owned_open = DeskOwnership {
            members_only: false,
            ..OWNED
        };
        for from_emails in [
            ChannelAccessMode::Team,
            ChannelAccessMode::Allowlist,
            ChannelAccessMode::Public,
        ] {
            assert_eq!(owned_open.stored_mode(from_emails).unwrap(), from_emails);
            assert_eq!(
                DeskOwnership::UNOWNED.stored_mode(from_emails).unwrap(),
                from_emails
            );
        }
    }

    #[test]
    fn an_ownership_edit_keeps_what_it_does_not_name() {
        let detach = ChannelOwnershipWrite {
            department_id: Some(None),
            members_only: None,
        };
        assert_eq!(
            OWNED.merged(&detach),
            DeskOwnership {
                department_id: None,
                members_only: true
            }
        );
        assert_eq!(OWNED.merged(&ChannelOwnershipWrite::default()), OWNED);
    }
}
