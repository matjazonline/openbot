use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{collections::HashSet, str::FromStr};
use uuid::Uuid;

use crate::{
    adapters::persistence::{PostgresPersistence, participant::create_person_principal_on},
    app_error::{AppError, AppResult},
    entities::{
        company_invite::CompanyInvite,
        company_member::{CompanyAccessRole, CompanyMember},
        member_removal::{DelegatedAskAtStake, MemberWorkAtStake, OwnedTaskAtStake, takers_for},
        task::{TaskOwnerCandidate, TaskStatus},
        transport::PrincipalId,
        value_objects::AvatarUrl,
    },
    task_queue::TaskPersistence,
    use_cases::company_invite::CompanyInvitePersistence,
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyInviteDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub company_name: Option<String>,
    pub email: String,
    pub role: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<CompanyInviteDb> for CompanyInvite {
    type Error = AppError;

    fn try_from(db: CompanyInviteDb) -> Result<Self, Self::Error> {
        Ok(CompanyInvite {
            id: db.id,
            company_id: db.company_id,
            company_name: db.company_name,
            email: db.email,
            role: CompanyAccessRole::from_str(&db.role).map_err(AppError::Internal)?,
            status: db.status,
            created_at: db.created_at,
        })
    }
}

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyMemberDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub user_id: Uuid,
    pub username: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<CompanyMemberDb> for CompanyMember {
    type Error = AppError;

    fn try_from(db: CompanyMemberDb) -> Result<Self, Self::Error> {
        Ok(CompanyMember {
            id: db.id,
            company_id: db.company_id,
            user_id: db.user_id,
            username: db.username,
            email: db.email,
            avatar_url: db.avatar_url.map(AvatarUrl::from),
            role: CompanyAccessRole::from_str(&db.role).map_err(AppError::Internal)?,
            created_at: db.created_at,
        })
    }
}

#[async_trait]
impl CompanyInvitePersistence for PostgresPersistence {
    async fn create_invite(
        &self,
        company_id: Uuid,
        email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH invite AS (
                INSERT INTO company_invites (id, company_id, email, role, status)
                VALUES ($1, $2, $3, $4, 'pending')
                ON CONFLICT (company_id, email)
                DO UPDATE SET role = EXCLUDED.role, status = 'pending', created_at = CURRENT_TIMESTAMP
                RETURNING id, company_id, email, role, status, created_at
            )
            SELECT invite.id, invite.company_id, company.name AS company_name, invite.email,
                   invite.role, invite.status, invite.created_at
            FROM invite
            JOIN companies AS company ON company.id = invite.company_id
            "#,
        )
        .bind(uuid)
        .bind(company_id)
        .bind(email)
        .bind(role.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.try_into()
    }

    async fn get_invite_by_id(&self, id: Uuid) -> AppResult<Option<CompanyInvite>> {
        let db = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT invite.id, invite.company_id, company.name AS "company_name?", invite.email,
                      invite.role, invite.status, invite.created_at AS "created_at!"
               FROM company_invites AS invite
               JOIN companies AS company ON company.id = invite.company_id
               WHERE invite.id = $1"#,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn list_invites_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyInvite>> {
        let db_list = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT invite.id, invite.company_id, company.name AS "company_name?", invite.email,
                      invite.role, invite.status, invite.created_at AS "created_at!"
               FROM company_invites AS invite
               JOIN companies AS company ON company.id = invite.company_id
               WHERE invite.company_id = $1
               ORDER BY invite.created_at DESC, invite.id DESC LIMIT 200"#,
            company_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn update_invite(
        &self,
        id: Uuid,
        new_email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite> {
        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH invite AS (
                UPDATE company_invites
                SET email = $1, role = $2
                WHERE id = $3
                RETURNING id, company_id, email, role, status, created_at
            )
            SELECT invite.id, invite.company_id, company.name AS company_name, invite.email,
                   invite.role, invite.status, invite.created_at
            FROM invite
            JOIN companies AS company ON company.id = invite.company_id
            "#,
        )
        .bind(new_email)
        .bind(role.as_str())
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into)
            .transpose()?
            .ok_or_else(|| AppError::Internal("Invite not found.".into()))
    }

    async fn delete_invite(&self, id: Uuid) -> AppResult<()> {
        sqlx::query!("DELETE FROM company_invites WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(())
    }

    async fn list_invites_by_email(&self, email: &str) -> AppResult<Vec<CompanyInvite>> {
        let db_list = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT invite.id, invite.company_id, company.name AS "company_name?", invite.email,
                      invite.role, invite.status, invite.created_at AS "created_at!"
               FROM company_invites AS invite
               JOIN companies AS company ON company.id = invite.company_id
               WHERE invite.email = $1
               ORDER BY invite.created_at DESC, invite.id DESC LIMIT 200"#,
            email
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn accept_pending_invite(
        &self,
        invite_id: Uuid,
        user_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>> {
        let member_id = Uuid::new_v4();
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH accepted AS (
                UPDATE company_invites
                SET status = 'accepted'
                WHERE id = $1 AND status = 'pending' AND email = $2
                RETURNING id, company_id, email, role, status, created_at
            ), membership AS (
                INSERT INTO company_members (id, company_id, user_id, role)
                SELECT $3, company_id, $4, role
                FROM accepted
                ON CONFLICT (company_id, user_id)
                DO UPDATE SET role = EXCLUDED.role
                RETURNING company_id
            )
            SELECT accepted.id, accepted.company_id, company.name AS company_name, accepted.email,
                   accepted.role, accepted.status, accepted.created_at
            FROM accepted
            JOIN membership ON membership.company_id = accepted.company_id
            JOIN companies AS company ON company.id = accepted.company_id
            "#,
        )
        .bind(invite_id)
        .bind(user_email)
        .bind(member_id)
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        if let Some(invite) = &db {
            let username: String =
                sqlx::query_scalar("SELECT username::text FROM users WHERE id = $1")
                    .bind(user_id)
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(AppError::from)?;
            create_person_principal_on(
                &mut transaction,
                invite.company_id,
                user_id,
                &username,
                user_email,
            )
            .await?;
        }
        transaction.commit().await.map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn decline_pending_invite(
        &self,
        invite_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>> {
        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH declined AS (
                UPDATE company_invites
                SET status = 'declined'
                WHERE id = $1 AND status = 'pending' AND email = $2
                RETURNING id, company_id, email, role, status, created_at
            )
            SELECT declined.id, declined.company_id, company.name AS company_name, declined.email,
                   declined.role, declined.status, declined.created_at
            FROM declined
            JOIN companies AS company ON company.id = declined.company_id
            "#,
        )
        .bind(invite_id)
        .bind(user_email)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    /// The people invited into this company.
    ///
    /// The owner also holds a `company_members` row -- it is the relational fact a person
    /// principal's foreign key needs -- but `owner` is not an invitation role, so this surface
    /// and the two writes below leave that row alone.
    async fn list_members_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyMember>> {
        let db_list = sqlx::query_as!(
            CompanyMemberDb,
            r#"SELECT member.id, member.company_id, member.user_id,
                      account.username AS "username?", account.email AS "email?", account.avatar_url,
                      member.role, member.created_at AS "created_at!"
               FROM company_members AS member
               JOIN users AS account ON account.id = member.user_id
               WHERE member.company_id = $1 AND member.role <> 'owner'
               ORDER BY member.created_at ASC, member.id ASC LIMIT 200"#,
            company_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn update_member_role(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        role: CompanyAccessRole,
    ) -> AppResult<Option<CompanyMember>> {
        let db = sqlx::query_as!(
            CompanyMemberDb,
            r#"WITH updated AS (
                   UPDATE company_members
                   SET role = $3
                   WHERE company_id = $1 AND user_id = $2 AND role <> 'owner'
                   RETURNING id, company_id, user_id, role, created_at
               )
               SELECT updated.id, updated.company_id, updated.user_id,
                      account.username AS "username?", account.email AS "email?", account.avatar_url,
                      updated.role, updated.created_at AS "created_at!"
               FROM updated
               JOIN users AS account ON account.id = updated.user_id"#,
            company_id,
            user_id,
            role.as_str(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    /// Take somebody off the team without erasing what they were party to.
    ///
    /// Their principal is demoted to an external actor first. The member row is what a person
    /// principal's foreign key hangs on, so deleting it alone would cascade the principal away and
    /// take its identities, grants and thread participation with it -- a former colleague would
    /// vanish from every thread they ever wrote in.
    async fn remove_member(&self, company_id: Uuid, user_id: Uuid) -> AppResult<()> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        sqlx::query!(
            r#"UPDATE principals
               SET kind = 'external', user_id = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE company_id = $1 AND user_id = $2
                 AND EXISTS (
                     SELECT 1 FROM company_members AS member
                     WHERE member.company_id = $1 AND member.user_id = $2
                       AND member.role <> 'owner'
                 )"#,
            company_id,
            user_id
        )
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        sqlx::query!(
            "DELETE FROM company_members
             WHERE company_id = $1 AND user_id = $2 AND role <> 'owner'",
            company_id,
            user_id
        )
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        transaction.commit().await.map_err(AppError::from)?;

        Ok(())
    }

    /// What removing this person would move, before anything is moved.
    ///
    /// Two reads and, when there is anything to hand over, one candidate read per affected
    /// channel. The candidate list comes from [`TaskPersistence::list_task_owner_candidates`]
    /// rather than a second copy of its eligibility rules, so what this pane offers and what an
    /// ownership command accepts cannot drift.
    async fn member_work_at_stake(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<MemberWorkAtStake> {
        let Some(principal_id) = sqlx::query_scalar!(
            "SELECT id FROM principals
             WHERE company_id = $1 AND user_id = $2 AND kind = 'person'",
            company_id,
            user_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?
        else {
            // Nobody to hold work: an invite that was never accepted, or a principal already
            // demoted by an earlier removal.
            return Ok(MemberWorkAtStake::default());
        };

        let owned = self.owned_tasks_at_stake(company_id, principal_id).await?;
        let asks = self
            .delegated_asks_at_stake(company_id, principal_id)
            .await?;
        let truncated = owned.len() > MemberWorkAtStake::MAX_PER_KIND
            || asks.len() > MemberWorkAtStake::MAX_PER_KIND;
        let mut owned_tasks = owned;
        owned_tasks.truncate(MemberWorkAtStake::MAX_PER_KIND);
        let mut delegated_asks = asks;
        delegated_asks.truncate(MemberWorkAtStake::MAX_PER_KIND);

        let owner_candidates = self
            .shared_owner_candidates(company_id, principal_id, &owned_tasks, &delegated_asks)
            .await?;

        Ok(MemberWorkAtStake {
            owned_tasks,
            delegated_asks,
            owner_candidates,
            truncated,
        })
    }
}

/// The task statuses that still need an owner.
///
/// `completed` and `stopped` are finished (`task_is_terminal` in the delegation controls), and a
/// `failed` task has already given up its run. `dead_letter` is here because it is a row a person
/// still has to act on — the attention feed lists it — so handing it over is a real decision.
const LIVE_TASK_STATUSES: [&str; 5] = [
    "pending",
    "processing",
    "pending_approval",
    "waiting_for_third_party_reply",
    "dead_letter",
];

impl PostgresPersistence {
    async fn owned_tasks_at_stake(
        &self,
        company_id: Uuid,
        principal_id: Uuid,
    ) -> AppResult<Vec<OwnedTaskAtStake>> {
        let probe = i64::try_from(MemberWorkAtStake::MAX_PER_KIND + 1)
            .map_err(|_| AppError::Internal("Work-at-stake bound does not fit a limit".into()))?;
        let rows = sqlx::query!(
            r#"SELECT id, channel_id, correlation_id, task_type, status, ownership_version
               FROM background_tasks
               WHERE company_id = $1 AND owner_principal_id = $2
                 AND owner_principal_kind = 'person'
                 AND status = ANY($3)
               ORDER BY created_at, id
               LIMIT $4"#,
            company_id,
            principal_id,
            &LIVE_TASK_STATUSES.map(String::from)[..],
            probe
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        rows.into_iter()
            .map(|row| {
                Ok(OwnedTaskAtStake {
                    task_id: row.id,
                    channel_id: row.channel_id,
                    correlation_id: row.correlation_id,
                    task_type: row.task_type,
                    status: TaskStatus::from_str(&row.status).map_err(AppError::Internal)?,
                    ownership_version: u64::try_from(row.ownership_version).map_err(|_| {
                        AppError::Internal(format!("Invalid ownership version on task {}", row.id))
                    })?,
                })
            })
            .collect()
    }

    /// Asks that name this person as the one expected to answer.
    ///
    /// An outreach target names an address or an internal channel, never a principal, so the person
    /// is found through `participant_identities` — the same identity rows a person principal is
    /// created with. `DISTINCT` because one address shape can be claimed by more than one identity
    /// row (an observation and an account claim for the same mailbox).
    async fn delegated_asks_at_stake(
        &self,
        company_id: Uuid,
        principal_id: Uuid,
    ) -> AppResult<Vec<DelegatedAskAtStake>> {
        let probe = i64::try_from(MemberWorkAtStake::MAX_PER_KIND + 1)
            .map_err(|_| AppError::Internal("Work-at-stake bound does not fit a limit".into()))?;
        let rows = sqlx::query!(
            r#"SELECT DISTINCT outreach.task_id, task.channel_id, outreach.id AS outreach_id,
                      target.id AS target_id, outreach.version, outreach.subject,
                      target.email::text AS "email!", outreach.expires_at
               FROM task_outreach_targets AS target
               JOIN task_outreaches AS outreach
                 ON outreach.company_id = target.company_id AND outreach.id = target.outreach_id
               JOIN background_tasks AS task
                 ON task.company_id = outreach.company_id AND task.id = outreach.task_id
               JOIN participant_identities AS identity
                 ON identity.company_id = target.company_id
                AND identity.principal_id = $2
                AND identity.transport = target.external_transport
                AND identity.namespace = target.external_namespace
                AND identity.subject = target.external_subject
               WHERE target.company_id = $1
                 AND target.status = 'active'
                 AND outreach.status IN ('waiting', 'timeout_pending_approval')
                 AND task.status NOT IN ('completed', 'stopped')
               ORDER BY outreach.expires_at, target.id
               LIMIT $3"#,
            company_id,
            principal_id,
            probe
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        rows.into_iter()
            .map(|row| {
                Ok(DelegatedAskAtStake {
                    task_id: row.task_id,
                    channel_id: row.channel_id,
                    outreach_id: row.outreach_id,
                    target_id: row.target_id,
                    outreach_version: u64::try_from(row.version).map_err(|_| {
                        AppError::Internal(format!(
                            "Invalid outreach version on {}",
                            row.outreach_id
                        ))
                    })?,
                    subject: row.subject,
                    asked_address: row.email,
                    expires_at: row.expires_at,
                })
            })
            .collect()
    }

    /// Who *all* of this work could go to instead, one read per distinct affected channel.
    ///
    /// The admin picks one taker for the whole batch, so the offer is the **intersection** of what
    /// each affected channel allows: a candidate eligible on one item's channel and not another's
    /// would be a choice half the commands then refuse. Asks count as affected channels too — the
    /// redirected question goes out from the asking task's channel, so whoever answers it has to
    /// be eligible there.
    ///
    /// `takers_for` then narrows the intersection to people when any ask is at stake, because an
    /// agent has no identity a person-addressed ask can be re-asked at. An empty result means
    /// nobody can take this work over and the removal is refused — there is no unassign fallback.
    async fn shared_owner_candidates(
        &self,
        company_id: Uuid,
        departing: Uuid,
        tasks: &[OwnedTaskAtStake],
        asks: &[DelegatedAskAtStake],
    ) -> AppResult<Vec<TaskOwnerCandidate>> {
        let affected = tasks
            .iter()
            .map(|task| task.channel_id)
            .chain(asks.iter().map(|ask| ask.channel_id));
        let mut shared: Option<Vec<TaskOwnerCandidate>> = None;
        let mut read_channels: HashSet<Uuid> = HashSet::new();
        for channel_id in affected {
            if !read_channels.insert(channel_id) {
                continue;
            }
            let candidates: Vec<TaskOwnerCandidate> =
                TaskPersistence::list_task_owner_candidates(self, company_id, channel_id)
                    .await?
                    .into_iter()
                    .filter(|candidate| {
                        candidate.owner.principal_id().map(PrincipalId::as_uuid) != Some(departing)
                    })
                    .collect();
            match shared.as_mut() {
                None => shared = Some(candidates),
                Some(shared) => shared.retain(|kept| {
                    candidates
                        .iter()
                        .any(|candidate| candidate.owner == kept.owner)
                }),
            }
        }
        Ok(takers_for(shared.unwrap_or_default(), asks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};
    use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
    use crate::use_cases::user::UserPersistence;

    #[tokio::test]
    async fn postgres_company_invite_and_member_persistence_works() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let persistence = PostgresPersistence::new(pool);

        // Create test owner and user
        let owner_username = format!("owner_{}", Uuid::new_v4().simple());
        let owner_email = format!("{}@example.com", owner_username);
        let _ = persistence
            .create_user(&owner_username, &owner_email, "hash")
            .await;
        let owner = persistence
            .get_by_email(&owner_email)
            .await
            .unwrap()
            .unwrap();

        let member_username = format!("member_{}", Uuid::new_v4().simple());
        let member_email = format!("{}@example.com", member_username);
        let _ = persistence
            .create_user(&member_username, &member_email, "hash")
            .await;
        let member = persistence
            .get_by_email(&member_email)
            .await
            .unwrap()
            .unwrap();

        // Create company
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Test Corp".to_string(),
                slug: "test-corp".to_string(),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        // 1. Create Invite
        let invite = persistence
            .create_invite(company.id, &member_email, CompanyAccessRole::Admin)
            .await
            .unwrap();
        assert_eq!(invite.email, member_email);
        assert_eq!(invite.role, CompanyAccessRole::Admin);
        assert_eq!(invite.status, "pending");

        // 2. List Invites by Company
        let invites = persistence
            .list_invites_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(invites.len(), 1);

        // 3. Update Invite Email
        let updated_email = format!("new_{}", member_email);
        let updated = persistence
            .update_invite(invite.id, &updated_email, CompanyAccessRole::Member)
            .await
            .unwrap();
        assert_eq!(updated.email, updated_email);
        assert_eq!(updated.role, CompanyAccessRole::Member);

        // Update back
        let _ = persistence
            .update_invite(invite.id, &member_email, CompanyAccessRole::Admin)
            .await
            .unwrap();

        // 4. Accept invite and add the member atomically
        let accepted = persistence
            .accept_pending_invite(invite.id, member.id, &member_email)
            .await
            .unwrap();
        assert_eq!(accepted.unwrap().status, "accepted");
        let accepted_again = persistence
            .accept_pending_invite(invite.id, member.id, &member_email)
            .await
            .unwrap();
        assert!(accepted_again.is_none());

        // 5. List team members
        let members = persistence
            .list_members_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].username, Some(member_username));
        assert_eq!(members[0].role, CompanyAccessRole::Admin);

        let changed = persistence
            .update_member_role(company.id, member.id, CompanyAccessRole::Member)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(changed.role, CompanyAccessRole::Member);
        let members = persistence
            .list_members_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(members[0].role, CompanyAccessRole::Member);

        // 6. Remove team member. Their principal survives as an outsider, so the threads they
        // were party to keep their author instead of losing them to a cascade.
        let principal_before: Uuid = sqlx::query_scalar!(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
            company.id,
            member.id
        )
        .fetch_one(&persistence.pool)
        .await
        .unwrap();
        persistence
            .remove_member(company.id, member.id)
            .await
            .unwrap();
        let members_after = persistence
            .list_members_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(members_after.len(), 0);
        let (kind, still_attached): (String, Option<Uuid>) =
            sqlx::query_as("SELECT kind, user_id FROM principals WHERE id = $1")
                .bind(principal_before)
                .fetch_one(&persistence.pool)
                .await
                .unwrap();
        assert_eq!(kind, "external");
        assert_eq!(still_attached, None);

        // The owner's own member row is the anchor of their person principal, so it is not the
        // team-management surface's to remove.
        persistence
            .remove_member(company.id, owner.id)
            .await
            .unwrap();
        let owner_kind: String = sqlx::query_scalar!(
            "SELECT kind FROM principals WHERE company_id = $1 AND user_id = $2",
            company.id,
            owner.id
        )
        .fetch_one(&persistence.pool)
        .await
        .unwrap();
        assert_eq!(owner_kind, "person");

        // 7. Delete invite
        persistence.delete_invite(invite.id).await.unwrap();
        let invites_after = persistence
            .list_invites_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(invites_after.len(), 0);

        // Cleanup company
        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }

    /// Everything a removal test needs: a company, its owner, a joined member and a channel.
    struct RemovalFixture {
        pool: sqlx::PgPool,
        company_id: Uuid,
        channel_id: Uuid,
        owner_principal: Uuid,
        member_user_id: Uuid,
        member_principal: Uuid,
    }

    /// A company with one joined member, built the way the product builds one: an invite the member
    /// accepts, so their principal and its email identity are the real ones.
    async fn removal_fixture(persistence: &PostgresPersistence, label: &str) -> RemovalFixture {
        let pool = persistence.pool.clone();
        let suffix = Uuid::new_v4().simple().to_string();

        let owner_email = format!("{label}_owner_{suffix}@example.com");
        persistence
            .create_user(&format!("{label}_owner_{suffix}"), &owner_email, "hash")
            .await
            .unwrap();
        let owner = persistence
            .get_by_email(&owner_email)
            .await
            .unwrap()
            .unwrap();
        let member_email = format!("{label}_member_{suffix}@example.com");
        persistence
            .create_user(&format!("{label}_member_{suffix}"), &member_email, "hash")
            .await
            .unwrap();
        let member = persistence
            .get_by_email(&member_email)
            .await
            .unwrap()
            .unwrap();

        let company = CompanyPersistence::create(
            persistence,
            owner.id,
            CompanyWrite {
                name: "Owned Work".to_string(),
                slug: format!("{label}-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let invite = persistence
            .create_invite(company.id, &member_email, CompanyAccessRole::Member)
            .await
            .unwrap();
        persistence
            .accept_pending_invite(invite.id, member.id, &member_email)
            .await
            .unwrap()
            .unwrap();
        let member_principal: Uuid = sqlx::query_scalar!(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
            company.id,
            member.id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let owner_principal: Uuid = sqlx::query_scalar!(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
            company.id,
            owner.id
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let channel = ChannelPersistence::create(
            persistence,
            company.id,
            ChannelWrite {
                name: "Release Desk".to_string(),
                slug: format!("{label}-desk-{suffix}"),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();

        RemovalFixture {
            pool,
            company_id: company.id,
            channel_id: channel.id,
            owner_principal,
            member_user_id: member.id,
            member_principal,
        }
    }

    impl RemovalFixture {
        /// A task owned by the member, in whichever status the test needs.
        async fn owned_task(&self, status: &str) -> Uuid {
            self.owned_task_on(self.channel_id, status).await
        }

        /// The same, on a channel of the test's choosing — what makes the candidate list of one
        /// task differ from another's.
        async fn owned_task_on(&self, channel_id: Uuid, status: &str) -> Uuid {
            let task_id = Uuid::new_v4();
            sqlx::query!(
                r#"INSERT INTO background_tasks (
                       id, company_id, channel_id, correlation_id, task_type, status,
                       owner_principal_id, owner_principal_kind
                   ) VALUES ($1, $2, $3, gen_random_uuid(), 'agent_run', $4, $5, 'person')"#,
                task_id,
                self.company_id,
                channel_id,
                status,
                self.member_principal
            )
            .execute(&self.pool)
            .await
            .unwrap();
            task_id
        }

        /// One more person on the team, so a candidate list has somebody in it besides the owner
        /// and the member who is leaving.
        async fn another_member(&self, persistence: &PostgresPersistence, label: &str) -> Uuid {
            let suffix = Uuid::new_v4().simple().to_string();
            let email = format!("{label}_{suffix}@example.com");
            persistence
                .create_user(&format!("{label}_{suffix}"), &email, "hash")
                .await
                .unwrap();
            let user = persistence.get_by_email(&email).await.unwrap().unwrap();
            let invite = persistence
                .create_invite(self.company_id, &email, CompanyAccessRole::Member)
                .await
                .unwrap();
            persistence
                .accept_pending_invite(invite.id, user.id, &email)
                .await
                .unwrap()
                .unwrap();
            sqlx::query_scalar!(
                "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
                self.company_id,
                user.id
            )
            .fetch_one(&self.pool)
            .await
            .unwrap()
        }

        /// An agent assigned to the member's channel, and the principal it would own work as.
        ///
        /// An agent is a perfectly good *task* owner, which is exactly why it has to drop out of
        /// the offer once an ask is at stake: there is no `participant_identities` row for one, so
        /// no address a person's question could be re-asked at.
        async fn channel_agent(&self, persistence: &PostgresPersistence, label: &str) -> Uuid {
            let suffix = Uuid::new_v4().simple().to_string();
            let agent = crate::use_cases::agent::AgentPersistence::create(
                persistence,
                self.company_id,
                crate::use_cases::agent::AgentWrite {
                    name: format!("{label} agent"),
                    slug: format!("{label}-agent-{suffix}"),
                    created_by: Some(crate::entities::creation::CreationProvenance::system()),
                    harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            sqlx::query!(
                r#"INSERT INTO channel_agents (company_id, channel_id, agent_id, "position")
                   VALUES ($1, $2, $3, 0)"#,
                self.company_id,
                self.channel_id,
                agent.id
            )
            .execute(&self.pool)
            .await
            .unwrap();
            sqlx::query_scalar!(
                "SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2",
                self.company_id,
                agent.id
            )
            .fetch_one(&self.pool)
            .await
            .unwrap()
        }

        /// A channel only the company owner can be given work on: an allowlist channel with no
        /// principal grants, which is what `list_task_owner_candidates` narrows on.
        async fn allowlist_channel(&self, persistence: &PostgresPersistence, label: &str) -> Uuid {
            let suffix = Uuid::new_v4().simple().to_string();
            ChannelPersistence::create(
                persistence,
                self.company_id,
                ChannelWrite {
                    name: "Restricted Desk".to_string(),
                    slug: format!("{label}-locked-{suffix}"),
                    enabled: false,
                    participant_emails: Some(vec![format!("outsider-{suffix}@example.com")]),
                    ..ChannelWrite::default()
                },
            )
            .await
            .unwrap()
            .id
        }

        /// An unowned task carrying an outreach that asks somebody by address.
        ///
        /// The address triple comes from a `participant_identities` row so the test cannot drift
        /// from however a person principal's email identity is qualified.
        async fn ask(&self, principal: Uuid) -> (Uuid, Uuid, Uuid) {
            let task_id = Uuid::new_v4();
            sqlx::query!(
                r#"INSERT INTO background_tasks (
                       id, company_id, channel_id, correlation_id, task_type, status
                   ) VALUES ($1, $2, $3, gen_random_uuid(), 'agent_run',
                             'waiting_for_third_party_reply')"#,
                task_id,
                self.company_id,
                self.channel_id
            )
            .execute(&self.pool)
            .await
            .unwrap();

            let identity = sqlx::query!(
                r#"SELECT transport, namespace, subject FROM participant_identities
                   WHERE company_id = $1 AND principal_id = $2 AND transport = 'email'
                   LIMIT 1"#,
                self.company_id,
                principal
            )
            .fetch_one(&self.pool)
            .await
            .unwrap();

            let outreach_id = Uuid::new_v4();
            sqlx::query!(
                r#"INSERT INTO task_outreaches (
                       id, task_id, company_id, status, required_threshold_percent, expires_at,
                       outreach_key, subject, body
                   ) VALUES ($1, $2, $3, 'waiting', 100, CURRENT_TIMESTAMP + INTERVAL '2 days',
                             $4, 'Can you confirm the invoice?', 'Please answer')"#,
                outreach_id,
                task_id,
                self.company_id,
                format!("ask-{}", Uuid::new_v4().simple())
            )
            .execute(&self.pool)
            .await
            .unwrap();

            let target_id = Uuid::new_v4();
            sqlx::query!(
                r#"INSERT INTO task_outreach_targets (
                       id, outreach_id, company_id, email, target_kind, external_transport,
                       external_namespace, external_subject, status
                   ) VALUES ($1, $2, $3, $4, 'external', $5, $6, $7, 'active')"#,
                target_id,
                outreach_id,
                self.company_id,
                format!("{}@{}", identity.subject, identity.namespace),
                identity.transport,
                identity.namespace,
                identity.subject
            )
            .execute(&self.pool)
            .await
            .unwrap();

            (task_id, outreach_id, target_id)
        }

        async fn work_at_stake(&self, persistence: &PostgresPersistence) -> MemberWorkAtStake {
            persistence
                .member_work_at_stake(self.company_id, self.member_user_id)
                .await
                .unwrap()
        }
    }

    /// A member who still owns live work can be removed, and the work survives them.
    ///
    /// This is the trigger acting as the **safety net** it now is: the guided flow
    /// (`CompanyInviteUseCases::remove_company_team_member`) refuses a removal while anything is at
    /// stake, and this test calls the persistence layer directly — the one path that still reaches
    /// a demotion with an owned task on it. `background_tasks` names its owner by
    /// `(company_id, principal_id, kind)`, so without the release trigger the whole removal
    /// transaction would fail on that foreign key instead.
    #[tokio::test]
    async fn removing_a_member_releases_the_live_work_they_still_own() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let fixture = removal_fixture(&persistence, "release").await;
        let company_id = fixture.company_id;
        let member_principal = fixture.member_principal;

        // A task the member owns, mid-run, with a worker's lease and an open attempt on it.
        let task_id = Uuid::new_v4();
        let worker_id = Uuid::new_v4();
        let generation = Uuid::new_v4();
        sqlx::query!(
            r#"INSERT INTO background_tasks (
                   id, company_id, channel_id, correlation_id, task_type, status,
                   owner_principal_id, owner_principal_kind, worker_id, execution_generation,
                   locked_at, lock_expires_at, transition_reason, transition_actor_kind,
                   transition_actor_id
               ) VALUES (
                   $1, $2, $3, gen_random_uuid(), 'agent_run', 'processing', $4, 'person', $5, $6,
                   CURRENT_TIMESTAMP, CURRENT_TIMESTAMP + INTERVAL '5 minutes',
                   'claimed', 'worker', $5
               )"#,
            task_id,
            company_id,
            fixture.channel_id,
            member_principal,
            worker_id,
            generation
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query!(
            r#"INSERT INTO task_attempts (
                   id, task_id, attempt_number, status, execution_generation, worker_id, machine_id
               ) VALUES ($1, $2, 1, 'processing', $3, $4, 'test-machine')"#,
            Uuid::new_v4(),
            task_id,
            generation,
            worker_id
        )
        .execute(&pool)
        .await
        .unwrap();

        persistence
            .remove_member(company_id, fixture.member_user_id)
            .await
            .unwrap();

        let demoted = sqlx::query!(
            "SELECT kind, user_id FROM principals WHERE id = $1",
            member_principal
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(demoted.kind, "external");
        assert_eq!(demoted.user_id, None);

        let released = sqlx::query!(
            r#"SELECT owner_principal_id, owner_principal_kind, status, worker_id,
                      execution_generation, locked_at, lock_expires_at, ownership_version
                 FROM background_tasks
                WHERE id = $1"#,
            task_id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(released.owner_principal_id, None);
        assert_eq!(released.owner_principal_kind, None);
        assert_eq!(released.status, "pending", "the run is requeued, not lost");
        assert_eq!(released.worker_id, None, "the lease is revoked");
        assert_eq!(released.execution_generation, None);
        assert_eq!(released.locked_at, None);
        assert_eq!(released.lock_expires_at, None);
        assert_eq!(released.ownership_version, 2);

        let fenced = sqlx::query!(
            "SELECT status, stop_reason FROM task_attempts WHERE task_id = $1",
            task_id
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(fenced.len(), 1);
        assert_eq!(fenced[0].status, "failed");
        assert_eq!(
            fenced[0].stop_reason.as_deref(),
            Some("ownership_transferred")
        );

        let removals = sqlx::query!(
            r#"SELECT operation, reason, actor_kind, previous_owner_principal_id,
                      previous_owner_kind, new_owner_principal_id, new_owner_kind,
                      sequence, from_version, to_version
                 FROM task_ownership_events
                WHERE task_id = $1 AND operation = 'owner_removed'"#,
            task_id
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(removals.len(), 1, "one removal event per released task");
        let removal = &removals[0];
        assert_eq!(removal.reason, "owner_removed");
        assert_eq!(removal.actor_kind, "system");
        assert_eq!(removal.previous_owner_principal_id, Some(member_principal));
        assert_eq!(removal.previous_owner_kind.as_deref(), Some("human"));
        assert_eq!(removal.new_owner_principal_id, None);
        assert_eq!(removal.new_owner_kind.as_deref(), Some("unassigned"));
        assert_eq!((removal.from_version, removal.to_version), (1, 2));
        assert_eq!(removal.sequence, 2);

        CompanyPersistence::delete(&persistence, company_id)
            .await
            .unwrap();
    }

    /// The delegation audit trail outlives the person it names.
    ///
    /// `delegation_control_commands` is append-only and its actor foreign key carries the
    /// principal's `kind`, so a demotion used to be refused outright. `ON UPDATE CASCADE` lets the
    /// denormalised `actor_kind` follow the principal while every field the trail is actually read
    /// by — the actor's id, the authority, the operation, the versions, the stored result and when
    /// it happened — stays exactly as written.
    #[tokio::test]
    async fn the_delegation_audit_row_survives_a_removal_unchanged() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let fixture = removal_fixture(&persistence, "audit").await;
        let (task_id, outreach_id, target_id) = fixture.ask(fixture.owner_principal).await;

        let command_row = Uuid::new_v4();
        let command_id = Uuid::new_v4();
        sqlx::query!(
            r#"INSERT INTO delegation_control_commands (
                   id, company_id, task_id, outreach_id, target_id, command_id,
                   command_fingerprint, operation, actor_principal_id, actor_kind, authority,
                   reason, from_version, to_version, result
               ) VALUES ($1, $2, $3, $4, $5, $6, 'fingerprint', 'cancel_target', $7, 'person',
                         'company_manager', 'target_unavailable', 1, 2,
                         '{"version": "1"}'::jsonb)"#,
            command_row,
            fixture.company_id,
            task_id,
            outreach_id,
            target_id,
            command_id,
            fixture.member_principal
        )
        .execute(&pool)
        .await
        .unwrap();
        let before = sqlx::query!(
            r#"SELECT actor_principal_id, actor_kind, authority, operation, reason, from_version,
                      to_version, result, occurred_at
                 FROM delegation_control_commands WHERE id = $1"#,
            command_row
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        persistence
            .remove_member(fixture.company_id, fixture.member_user_id)
            .await
            .unwrap();

        let after = sqlx::query!(
            r#"SELECT actor_principal_id, actor_kind, authority, operation, reason, from_version,
                      to_version, result, occurred_at
                 FROM delegation_control_commands WHERE id = $1"#,
            command_row
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            after.actor_principal_id, before.actor_principal_id,
            "who issued the command never moves"
        );
        assert_eq!(after.actor_principal_id, fixture.member_principal);
        assert_eq!(after.authority, before.authority);
        assert_eq!(after.operation, before.operation);
        assert_eq!(after.reason, before.reason);
        assert_eq!(after.from_version, before.from_version);
        assert_eq!(after.to_version, before.to_version);
        assert_eq!(after.result, before.result);
        assert_eq!(after.occurred_at, before.occurred_at);
        assert_eq!(
            (before.actor_kind.as_str(), after.actor_kind.as_str()),
            ("person", "external"),
            "the one denormalised field follows the principal, by ON UPDATE CASCADE"
        );

        CompanyPersistence::delete(&persistence, fixture.company_id)
            .await
            .unwrap();
    }

    /// The pre-check reports the tasks that still need an owner, and only those.
    #[tokio::test]
    async fn work_at_stake_names_the_live_tasks_a_member_owns() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let fixture = removal_fixture(&persistence, "stake").await;
        let live = fixture.owned_task("pending").await;
        let finished = fixture.owned_task("completed").await;

        let at_stake = fixture.work_at_stake(&persistence).await;
        let task_ids: Vec<Uuid> = at_stake
            .owned_tasks
            .iter()
            .map(|task| task.task_id)
            .collect();
        assert_eq!(task_ids, vec![live], "only work that still needs an owner");
        assert!(!task_ids.contains(&finished));
        assert!(at_stake.delegated_asks.is_empty());
        assert!(!at_stake.truncated);

        let blocked = &at_stake.owned_tasks[0];
        assert_eq!(blocked.channel_id, fixture.channel_id);
        assert_eq!(blocked.status, TaskStatus::Pending);
        assert_eq!(blocked.ownership_version, 1, "what a command must fence on");
        let candidate_ids: Vec<Uuid> = at_stake
            .owner_candidates
            .iter()
            .filter_map(|candidate| candidate.owner.principal_id())
            .map(PrincipalId::as_uuid)
            .collect();
        assert!(
            candidate_ids.contains(&fixture.owner_principal),
            "the company owner can take it: {candidate_ids:?}"
        );
        assert!(
            !candidate_ids.contains(&fixture.member_principal),
            "the person being removed is never offered as the new owner"
        );

        CompanyPersistence::delete(&persistence, fixture.company_id)
            .await
            .unwrap();
    }

    /// One picker for the whole batch means one candidate list: the intersection of what every
    /// affected channel allows.
    ///
    /// A teammate who could take the task on the open channel but not on the restricted one is not
    /// offered at all — picking them would be a choice half the ownership commands then refuse.
    #[tokio::test]
    async fn shared_owner_candidates_are_the_ones_every_affected_channel_allows() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let fixture = removal_fixture(&persistence, "shared").await;
        let teammate = fixture.another_member(&persistence, "helper").await;
        let restricted = fixture.allowlist_channel(&persistence, "shared").await;

        // One task on the team channel, where the teammate is eligible.
        fixture.owned_task("pending").await;
        let open_only: Vec<Uuid> = fixture
            .work_at_stake(&persistence)
            .await
            .owner_candidates
            .iter()
            .filter_map(|candidate| candidate.owner.principal_id())
            .map(PrincipalId::as_uuid)
            .collect();
        assert!(
            open_only.contains(&teammate) && open_only.contains(&fixture.owner_principal),
            "both can take work on the team channel: {open_only:?}"
        );

        // A second task on the restricted channel, where only the company owner is eligible.
        fixture.owned_task_on(restricted, "pending").await;
        let shared: Vec<Uuid> = fixture
            .work_at_stake(&persistence)
            .await
            .owner_candidates
            .iter()
            .filter_map(|candidate| candidate.owner.principal_id())
            .map(PrincipalId::as_uuid)
            .collect();
        assert_eq!(
            shared,
            vec![fixture.owner_principal],
            "only an owner every task accepts is offered for all of them"
        );
        assert!(!shared.contains(&fixture.member_principal));

        CompanyPersistence::delete(&persistence, fixture.company_id)
            .await
            .unwrap();
    }

    /// An ask that names the member is at stake; the same ask pointed at somebody else is not.
    #[tokio::test]
    async fn work_at_stake_names_an_open_ask_waiting_on_the_member() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let fixture = removal_fixture(&persistence, "asked").await;
        let (theirs_task, theirs_outreach, theirs_target) =
            fixture.ask(fixture.member_principal).await;
        let (_, _, someone_elses) = fixture.ask(fixture.owner_principal).await;

        let at_stake = fixture.work_at_stake(&persistence).await;
        assert_eq!(at_stake.owned_tasks, Vec::new());
        assert_eq!(at_stake.delegated_asks.len(), 1, "{at_stake:#?}");
        let ask = &at_stake.delegated_asks[0];
        assert_eq!(ask.task_id, theirs_task);
        assert_eq!(
            ask.channel_id, fixture.channel_id,
            "whose channel decides who can answer"
        );
        assert_eq!(ask.outreach_id, theirs_outreach);
        assert_eq!(ask.target_id, theirs_target);
        assert_ne!(ask.target_id, someone_elses);
        assert_eq!(ask.outreach_version, 1, "what a command must fence on");
        assert_eq!(ask.subject, "Can you confirm the invoice?");
        // An ask needs a taker of its own now, so the asking task's channel is an affected one.
        let candidate_ids: Vec<Uuid> = at_stake
            .owner_candidates
            .iter()
            .filter_map(|candidate| candidate.owner.principal_id())
            .map(PrincipalId::as_uuid)
            .collect();
        assert!(
            candidate_ids.contains(&fixture.owner_principal),
            "somebody can be re-asked the question: {candidate_ids:?}"
        );
        assert!(!candidate_ids.contains(&fixture.member_principal));

        // A cancelled target is nobody's decision any more.
        sqlx::query!(
            "UPDATE task_outreach_targets SET status = 'cancelled' WHERE id = $1",
            theirs_target
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            fixture
                .work_at_stake(&persistence)
                .await
                .delegated_asks
                .is_empty(),
            "resolving the ask clears it from the pre-check"
        );

        CompanyPersistence::delete(&persistence, fixture.company_id)
            .await
            .unwrap();
    }

    /// An agent may inherit a task but is never offered a person's question.
    ///
    /// The one selection has to be valid for everything it is applied to, so the same channel
    /// offers the agent while only tasks are at stake and stops offering it the moment an ask is.
    /// An agent has no `participant_identities` row — `create_agent_principal_on` writes none —
    /// so there is no address `ReassignPersonTarget` could re-ask the question at.
    #[tokio::test]
    async fn an_agent_is_offered_for_tasks_and_dropped_once_an_ask_is_at_stake() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let fixture = removal_fixture(&persistence, "narrow").await;
        let agent_principal = fixture.channel_agent(&persistence, "narrow").await;

        // Proof the concept is person-only rather than merely unpopulated in this fixture.
        let agent_identities: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM participant_identities WHERE company_id = $1 AND principal_id = $2",
            fixture.company_id,
            agent_principal
        )
        .fetch_one(&pool)
        .await
        .unwrap()
        .unwrap_or_default();
        assert_eq!(
            agent_identities, 0,
            "an agent principal carries no contact identity at all"
        );

        fixture.owned_task("pending").await;
        let tasks_only: Vec<Uuid> = fixture
            .work_at_stake(&persistence)
            .await
            .owner_candidates
            .iter()
            .filter_map(|candidate| candidate.owner.principal_id())
            .map(PrincipalId::as_uuid)
            .collect();
        assert!(
            tasks_only.contains(&agent_principal),
            "an agent can take a task over: {tasks_only:?}"
        );

        fixture.ask(fixture.member_principal).await;
        let with_an_ask = fixture.work_at_stake(&persistence).await;
        let narrowed: Vec<Uuid> = with_an_ask
            .owner_candidates
            .iter()
            .filter_map(|candidate| candidate.owner.principal_id())
            .map(PrincipalId::as_uuid)
            .collect();
        assert!(
            !narrowed.contains(&agent_principal),
            "an agent cannot answer a person's question: {narrowed:?}"
        );
        assert!(
            narrowed.contains(&fixture.owner_principal),
            "the people on the channel still can: {narrowed:?}"
        );
        assert!(
            with_an_ask
                .owner_candidates
                .iter()
                .all(|candidate| candidate.owner.principal_id().is_some()),
            "and every option still names somebody"
        );

        CompanyPersistence::delete(&persistence, fixture.company_id)
            .await
            .unwrap();
    }

    /// The common case: nothing live, so the pre-check is empty and the removal is unremarkable.
    #[tokio::test]
    async fn a_member_with_nothing_live_has_nothing_at_stake() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let fixture = removal_fixture(&persistence, "quiet").await;
        fixture.owned_task("stopped").await;

        assert!(fixture.work_at_stake(&persistence).await.is_empty());
        persistence
            .remove_member(fixture.company_id, fixture.member_user_id)
            .await
            .unwrap();

        // Once demoted there is no person principal left, and the pre-check says so rather than
        // failing on the missing row.
        assert!(
            persistence
                .member_work_at_stake(fixture.company_id, fixture.member_user_id)
                .await
                .unwrap()
                .is_empty()
        );

        CompanyPersistence::delete(&persistence, fixture.company_id)
            .await
            .unwrap();
    }
}
