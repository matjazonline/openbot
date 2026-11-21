//! Plan06 01B.7: inviting into the company is root management; an admin invitation stays the owner's.
use super::*;
use crate::adapters::persistence::test_support::{
    WriteBarrier, competing_writes, insert_root_member, invite_command, invite_update_command,
    own_database,
};
use crate::entities::access_level::AccessLevel;
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::company_invite::{
    CompanyInviteUseCases, CreateCompanyInvite, DeleteCompanyInvite, InviteLevelChange,
};
use crate::use_cases::user::UserPersistence;
use std::sync::Arc;

struct Team {
    p: Arc<PostgresPersistence>,
    company: Uuid,
    owner: Uuid,
    admin: Uuid,
    admin_email: String,
    member: Uuid,
}

impl Team {
    async fn new(pool: sqlx::PgPool) -> Self {
        let p = Arc::new(PostgresPersistence::new(pool));
        let suffix = Uuid::new_v4().simple().to_string();
        let mut users = Vec::new();
        for name in ["owner", "admin", "member"] {
            users.push(
                p.create_user(
                    &format!("invite_{name}_{suffix}"),
                    &format!("invite-{name}-{suffix}@example.test"),
                    "hash",
                )
                .await
                .unwrap()
                .id,
            );
        }
        let company = CompanyPersistence::create(
            p.as_ref(),
            users[0],
            CompanyWrite {
                name: "Delegated invites".into(),
                slug: format!("delegated-invites-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap()
        .id;
        for (user, role) in [
            (users[1], CompanyAccessRole::Admin),
            (users[2], CompanyAccessRole::Member),
        ] {
            insert_root_member(
                p.pool(),
                crate::domain::workflow::CompanyId::new(company),
                user,
                role,
                AccessLevel::try_from(5_i16).unwrap(),
            )
            .await;
        }
        Self {
            p: p.clone(),
            company,
            owner: users[0],
            admin: users[1],
            admin_email: format!("invite-admin-{suffix}@example.test"),
            member: users[2],
        }
    }

    fn cases(&self) -> CompanyInviteUseCases {
        CompanyInviteUseCases::new(self.p.clone(), self.p.clone())
    }

    async fn create(
        &self,
        actor: Uuid,
        email: &str,
        role: CompanyAccessRole,
    ) -> CreateCompanyInvite {
        CreateCompanyInvite {
            actor_id: actor,
            ..invite_command(&self.p, self.company, email, role).await
        }
    }

    fn delete(&self, actor: Uuid, invite: Uuid) -> DeleteCompanyInvite {
        DeleteCompanyInvite {
            actor_id: actor,
            company_id: self.company,
            invite_id: invite,
        }
    }

    fn level(&self, actor: Uuid, invite: Uuid) -> InviteLevelChange {
        InviteLevelChange {
            actor_id: actor,
            company_id: self.company,
            invite_id: invite,
            access_level: AccessLevel::try_from(1_i16).unwrap(),
        }
    }

    async fn role_of(&self, invite: Uuid) -> CompanyAccessRole {
        self.p
            .get_invite_by_id(self.company, invite)
            .await
            .unwrap()
            .expect("the invitation remains")
            .role
    }
}

fn forbidden<T: std::fmt::Debug>(result: AppResult<T>) {
    assert!(matches!(result, Err(AppError::Forbidden(_))), "{result:?}");
}

#[tokio::test]
async fn delegated_admin_invites_root_admin_writes_member_invitations_only() {
    let Some(db) = own_database().await else {
        return;
    };
    let t = Team::new(db.pool.clone()).await;
    let cases = t.cases();

    let invited = cases
        .create_company_invite(
            t.create(t.admin, "new@example.test", CompanyAccessRole::Member)
                .await,
        )
        .await
        .unwrap();
    assert_eq!(invited.role, CompanyAccessRole::Member);
    let renamed = cases
        .update_company_invite(UpdateCompanyInvite {
            actor_id: t.admin,
            ..invite_update_command(
                &t.p,
                t.company,
                invited.id,
                "renamed@example.test",
                CompanyAccessRole::Member,
            )
            .await
        })
        .await
        .unwrap();
    assert_eq!(renamed.email.as_str(), "renamed@example.test");

    // An admin invitation is a root role assignment, refused by the use case and its recheck.
    let admin_invite = t
        .create(
            t.admin,
            "second-admin@example.test",
            CompanyAccessRole::Admin,
        )
        .await;
    forbidden(cases.create_company_invite(admin_invite.clone()).await);
    forbidden(t.p.create_invite(admin_invite).await);
    let promote = UpdateCompanyInvite {
        actor_id: t.admin,
        ..invite_update_command(
            &t.p,
            t.company,
            invited.id,
            "renamed@example.test",
            CompanyAccessRole::Admin,
        )
        .await
    };
    forbidden(cases.update_company_invite(promote.clone()).await);
    forbidden(t.p.update_invite(promote).await);
    assert_eq!(t.role_of(invited.id).await, CompanyAccessRole::Member);

    // The owner's admin invitation cannot be rewritten, demoted or withdrawn by an admin,
    // including through creation's upsert on the same address.
    let owners = cases
        .create_company_invite(
            t.create(
                t.owner,
                "owners-admin@example.test",
                CompanyAccessRole::Admin,
            )
            .await,
        )
        .await
        .unwrap();
    let overwrite = t
        .create(
            t.admin,
            "owners-admin@example.test",
            CompanyAccessRole::Member,
        )
        .await;
    forbidden(cases.create_company_invite(overwrite.clone()).await);
    forbidden(t.p.create_invite(overwrite).await);
    let demote = UpdateCompanyInvite {
        actor_id: t.admin,
        ..invite_update_command(
            &t.p,
            t.company,
            owners.id,
            "owners-admin@example.test",
            CompanyAccessRole::Member,
        )
        .await
    };
    forbidden(cases.update_company_invite(demote.clone()).await);
    forbidden(t.p.update_invite(demote).await);
    forbidden(
        cases
            .delete_company_invite(t.admin, t.company, owners.id)
            .await,
    );
    assert_eq!(t.role_of(owners.id).await, CompanyAccessRole::Admin);

    // A plain member invites no one; the admin withdraws its own member invitation.
    let by_member = t
        .create(
            t.member,
            "by-member@example.test",
            CompanyAccessRole::Member,
        )
        .await;
    assert!(
        cases
            .create_company_invite(by_member.clone())
            .await
            .is_err()
    );
    forbidden(t.p.create_invite(by_member).await);
    assert!(
        cases
            .delete_company_invite(t.member, t.company, invited.id)
            .await
            .is_err()
    );
    cases
        .delete_company_invite(t.admin, t.company, invited.id)
        .await
        .unwrap();
    assert!(
        t.p.get_invite_by_id(t.company, invited.id)
            .await
            .unwrap()
            .is_none()
    );
}

/// Every invitation write rechecks under the company lock, including a delete that skipped the
/// use case, a level-only edit, and a member invitation that would demote a current admin.
#[tokio::test]
async fn delegated_admin_invites_persistence_rechecks_delete_level_and_admin_address() {
    let Some(db) = own_database().await else {
        return;
    };
    let t = Team::new(db.pool.clone()).await;
    let other = Team::new(db.pool.clone()).await;
    let cases = t.cases();
    let owners = cases
        .create_company_invite(
            t.create(t.owner, "owners@example.test", CompanyAccessRole::Admin)
                .await,
        )
        .await
        .unwrap();
    let members = cases
        .create_company_invite(
            t.create(t.admin, "members@example.test", CompanyAccessRole::Member)
                .await,
        )
        .await
        .unwrap();

    forbidden(t.p.delete_invite(t.delete(t.admin, owners.id)).await);
    forbidden(t.p.delete_invite(t.delete(t.member, members.id)).await);
    let foreign = DeleteCompanyInvite {
        company_id: other.company,
        ..t.delete(other.owner, members.id)
    };
    assert!(matches!(
        t.p.delete_invite(foreign).await,
        Err(AppError::NotFound(_))
    ));
    let stranger = t.delete(other.admin, members.id);
    assert!(matches!(
        t.p.delete_invite(stranger).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(t.role_of(owners.id).await, CompanyAccessRole::Admin);
    assert_eq!(t.role_of(members.id).await, CompanyAccessRole::Member);

    forbidden(
        t.p.set_invite_access_level(t.level(t.admin, owners.id))
            .await,
    );
    assert_eq!(
        t.p.set_invite_access_level(t.level(t.admin, members.id))
            .await
            .unwrap()
            .unwrap()
            .access_level,
        AccessLevel::try_from(1_i16).unwrap()
    );

    let demotion = t
        .create(t.admin, &t.admin_email, CompanyAccessRole::Member)
        .await;
    forbidden(cases.create_company_invite(demotion.clone()).await);
    let redirect = UpdateCompanyInvite {
        actor_id: t.admin,
        ..invite_update_command(
            &t.p,
            t.company,
            members.id,
            &t.admin_email,
            CompanyAccessRole::Member,
        )
        .await
    };
    forbidden(t.p.update_invite(redirect).await);
    t.p.create_invite(
        t.create(t.owner, &t.admin_email, CompanyAccessRole::Member)
            .await,
    )
    .await
    .unwrap();

    t.p.delete_invite(t.delete(t.admin, members.id))
        .await
        .unwrap();
    assert!(
        t.p.get_invite_by_id(t.company, members.id)
            .await
            .unwrap()
            .is_none()
    );
}

/// The owner's admin invitation commits while the admin's member upsert for the same address
/// waits on the company lock; the admin then sees it and is refused instead of demoting it.
#[tokio::test]
async fn delegated_admin_competing_owner_admin_invite_precedes_admin_member_upsert() {
    let Some(db) = own_database().await else {
        return;
    };
    let t = Team::new(db.pool.clone()).await;
    let address = "contested@example.test";
    let owner = t.create(t.owner, address, CompanyAccessRole::Admin).await;
    let admin = t.create(t.admin, address, CompanyAccessRole::Member).await;
    let result = competing_writes(
        t.p.pool(),
        WriteBarrier::CompanyInvites,
        t.p.create_invite(owner),
        t.p.create_invite(admin),
    )
    .await;
    let invite = result.first.unwrap();
    forbidden(result.second);
    assert_eq!(t.role_of(invite.id).await, CompanyAccessRole::Admin);
}

#[tokio::test]
async fn delegated_admin_competing_admin_member_invite_precedes_owner_admin_upsert() {
    let Some(db) = own_database().await else {
        return;
    };
    let t = Team::new(db.pool.clone()).await;
    let address = "contested@example.test";
    let admin = t.create(t.admin, address, CompanyAccessRole::Member).await;
    let owner = t.create(t.owner, address, CompanyAccessRole::Admin).await;
    let result = competing_writes(
        t.p.pool(),
        WriteBarrier::CompanyInvites,
        t.p.create_invite(admin),
        t.p.create_invite(owner),
    )
    .await;
    let first = result.first.unwrap();
    let second = result.second.unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(t.role_of(first.id).await, CompanyAccessRole::Admin);
}

/// The owner promotes a member invitation to admin while an admin's delete waits on that
/// invitation; the delete then sees the promoted role and is refused instead of removing it.
#[tokio::test]
async fn delegated_admin_competing_owner_promotion_precedes_admin_invite_delete() {
    let Some(db) = own_database().await else {
        return;
    };
    let t = Team::new(db.pool.clone()).await;
    let invite =
        t.p.create_invite(
            t.create(t.admin, "promoted@example.test", CompanyAccessRole::Member)
                .await,
        )
        .await
        .unwrap();
    let promote = invite_update_command(
        &t.p,
        t.company,
        invite.id,
        "promoted@example.test",
        CompanyAccessRole::Admin,
    )
    .await;
    let result = competing_writes(
        t.p.pool(),
        WriteBarrier::CompanyInvites,
        t.p.update_invite(promote),
        t.p.delete_invite(t.delete(t.admin, invite.id)),
    )
    .await;
    assert_eq!(
        result.first.unwrap().unwrap().role,
        CompanyAccessRole::Admin
    );
    forbidden(result.second);
    assert_eq!(t.role_of(invite.id).await, CompanyAccessRole::Admin);
}

/// Plan06 02B: the owner's team-page root writes land in the root department's history as the
/// owner's, and the removal's row is written before the person's principal is retired.
#[tokio::test]
async fn administration_audit_team_page_root_role_and_removal_are_the_owners() {
    use crate::use_cases::company_invite::{MemberRoleChange, RemoveCompanyMember};
    let Some(db) = own_database().await else {
        return;
    };
    let t = Team::new(db.pool.clone()).await;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(t.company)
            .fetch_one(t.p.pool())
            .await
            .unwrap();
    let member_principal: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(t.company)
    .bind(t.member)
    .fetch_one(t.p.pool())
    .await
    .unwrap();
    let rows = || async {
        sqlx::query_as::<_, (String, Option<String>, Option<Uuid>, Option<String>)>(
            "SELECT transition_kind, acting_role, principal_id, role FROM department_administration_events \
             WHERE company_id=$1 AND department_id=$2 AND actor_kind='person' ORDER BY created_at, id",
        )
        .bind(t.company)
        .bind(root)
        .fetch_all(t.p.pool())
        .await
        .unwrap()
    };
    t.p.update_member_role(MemberRoleChange {
        actor_id: t.owner,
        company_id: t.company,
        member_user_id: t.member,
        role: CompanyAccessRole::Admin,
    })
    .await
    .unwrap();
    // Setting the role it already has changes nothing and records nothing.
    t.p.update_member_role(MemberRoleChange {
        actor_id: t.owner,
        company_id: t.company,
        member_user_id: t.member,
        role: CompanyAccessRole::Admin,
    })
    .await
    .unwrap();
    t.p.remove_member(RemoveCompanyMember {
        actor_id: t.owner,
        company_id: t.company,
        member_user_id: t.member,
    })
    .await
    .unwrap();
    assert_eq!(
        rows().await,
        vec![
            (
                "role_assigned".to_owned(),
                Some("owner".to_owned()),
                Some(member_principal),
                Some("admin".to_owned())
            ),
            (
                "member_removed".to_owned(),
                Some("owner".to_owned()),
                Some(member_principal),
                None
            ),
        ]
    );
}
