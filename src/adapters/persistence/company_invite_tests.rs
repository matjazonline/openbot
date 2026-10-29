use super::*;
use crate::adapters::persistence::test_support::test_pool;
use crate::entities::user::User;
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::company_invite::CompanyInviteUseCases;
use crate::use_cases::user::UserPersistence;
use std::sync::Arc;

struct InviteSecurityFixture {
    persistence: Arc<PostgresPersistence>,
    claimant: User,
    invite: CompanyInvite,
}

impl InviteSecurityFixture {
    async fn new(pool: sqlx::PgPool) -> Self {
        let persistence = Arc::new(PostgresPersistence::new(pool));
        let suffix = Uuid::new_v4().simple().to_string();
        let owner_email = format!("invite-owner-{suffix}@example.test");
        let claimant_email = format!("invite-claimant-{suffix}@example.test");
        let owner = persistence
            .create_user(&format!("invite_owner_{suffix}"), &owner_email, "hash")
            .await
            .unwrap();
        let claimant = persistence
            .create_user(
                &format!("invite_claimant_{suffix}"),
                &claimant_email,
                "hash",
            )
            .await
            .unwrap();
        let company = CompanyPersistence::create(
            persistence.as_ref(),
            owner.id,
            CompanyWrite {
                name: "Invite security".into(),
                slug: format!("invite-security-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let invite = persistence
            .create_invite(company.id, &claimant_email, CompanyAccessRole::Member)
            .await
            .unwrap();
        Self {
            persistence,
            claimant,
            invite,
        }
    }

    async fn set_proof(&self, verified: bool) {
        sqlx::query("UPDATE users SET email_verified_at = CASE WHEN $2 THEN CURRENT_TIMESTAMP ELSE NULL END WHERE id = $1")
                .bind(self.claimant.id)
                .bind(verified)
                .execute(self.persistence.pool())
                .await
                .unwrap();
    }

    fn use_cases(&self) -> CompanyInviteUseCases {
        CompanyInviteUseCases::new(self.persistence.clone(), self.persistence.clone())
    }

    async fn assert_forged_account_cannot_accept(&self) {
        let p = self.persistence.as_ref();
        let forged = p
            .create_user(
                &format!("forged_{}", Uuid::new_v4().simple()),
                &format!("forged-{}@example.test", Uuid::new_v4().simple()),
                "hash",
            )
            .await
            .unwrap();
        sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(forged.id)
            .execute(p.pool())
            .await
            .unwrap();
        assert!(
            p.accept_pending_invite(self.invite.id, forged.id, &self.claimant.email)
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn security_email_identity_invite_accept_requires_current_account_even_for_terminal_result() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = InviteSecurityFixture::new(pool).await;
    let p = fixture.persistence.as_ref();
    let user = &fixture.claimant;
    let invite = &fixture.invite;
    assert!(
        p.list_invites_for_user(user.id, &user.email)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        p.accept_pending_invite(invite.id, user.id, &user.email)
            .await
            .unwrap()
            .is_none()
    );

    fixture.assert_forged_account_cannot_accept().await;

    fixture.set_proof(true).await;
    let fresh = UserPersistence::get_by_id(p, user.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        p.list_invites_for_user(user.id, &user.email)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fixture
            .use_cases()
            .accept_invite(&fresh, invite.id)
            .await
            .unwrap()
            .status,
        "accepted"
    );
    assert_eq!(
        fixture
            .use_cases()
            .accept_invite(&fresh, invite.id)
            .await
            .unwrap()
            .status,
        "accepted"
    );

    fixture.set_proof(false).await;
    assert!(
        p.list_invites_for_user(user.id, &user.email)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        fixture.use_cases().accept_invite(&fresh, invite.id).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        p.get_invite_by_id(invite.id).await.unwrap().unwrap().status,
        "accepted"
    );
}

#[tokio::test]
async fn security_email_identity_invite_decline_requires_current_account_even_for_terminal_result()
{
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = InviteSecurityFixture::new(pool).await;
    let p = fixture.persistence.as_ref();
    let user = &fixture.claimant;
    let invite = &fixture.invite;
    assert!(
        p.decline_pending_invite(invite.id, user.id, &user.email)
            .await
            .unwrap()
            .is_none()
    );
    fixture.set_proof(true).await;
    let fresh = UserPersistence::get_by_id(p, user.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .use_cases()
            .decline_invite(&fresh, invite.id)
            .await
            .unwrap()
            .status,
        "declined"
    );
    assert_eq!(
        fixture
            .use_cases()
            .decline_invite(&fresh, invite.id)
            .await
            .unwrap()
            .status,
        "declined"
    );
    fixture.set_proof(false).await;
    assert!(matches!(
        fixture.use_cases().decline_invite(&fresh, invite.id).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        p.get_invite_by_id(invite.id).await.unwrap().unwrap().status,
        "declined"
    );
}

#[tokio::test]
async fn security_email_identity_concurrent_address_change_defeats_stale_invite_acceptance() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = InviteSecurityFixture::new(pool).await;
    fixture.set_proof(true).await;
    let stale = UserPersistence::get_by_id(fixture.persistence.as_ref(), fixture.claimant.id)
        .await
        .unwrap()
        .unwrap();
    let mut address_change = fixture.persistence.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
        .bind(stale.id)
        .fetch_one(&mut *address_change)
        .await
        .unwrap();
    let invite_id = fixture.invite.id;
    let invitations = fixture.use_cases();
    let acceptance =
        tokio::spawn(async move { invitations.accept_invite(&stale, invite_id).await });
    let next_address = format!("after-{}@example.test", Uuid::new_v4().simple());
    sqlx::query("UPDATE users SET email = $2, email_verified_at = NULL WHERE id = $1")
        .bind(fixture.claimant.id)
        .bind(next_address)
        .execute(&mut *address_change)
        .await
        .unwrap();
    address_change.commit().await.unwrap();
    assert!(matches!(
        acceptance.await.unwrap(),
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        fixture
            .persistence
            .get_invite_by_id(invite_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "pending"
    );
    let membership: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM company_members WHERE company_id = $1 AND user_id = $2",
    )
    .bind(fixture.invite.company_id)
    .bind(fixture.claimant.id)
    .fetch_one(fixture.persistence.pool())
    .await
    .unwrap();
    assert_eq!(membership, 0);
}

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
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(member.id)
        .execute(&persistence.pool)
        .await
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
    assert_eq!(accepted_again.unwrap().status, "accepted");

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

/// Removing a member demotes their principal rather than deleting it, so what they wrote keeps
/// an author; the owner's row is never removed this way.
#[tokio::test]
async fn removing_a_member_demotes_their_principal_and_spares_the_owner() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let owner_email = format!("removal_owner_{suffix}@example.com");
    persistence
        .create_user(&format!("removal_owner_{suffix}"), &owner_email, "hash")
        .await
        .unwrap();
    let owner = persistence
        .get_by_email(&owner_email)
        .await
        .unwrap()
        .unwrap();
    let member_email = format!("removal_member_{suffix}@example.com");
    persistence
        .create_user(&format!("removal_member_{suffix}"), &member_email, "hash")
        .await
        .unwrap();
    let member = persistence
        .get_by_email(&member_email)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(member.id)
        .execute(&pool)
        .await
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Removal".to_string(),
            slug: format!("removal-{suffix}"),
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
    let member_principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company.id)
            .bind(member.id)
            .fetch_one(&pool)
            .await
            .unwrap();

    persistence
        .remove_member(company.id, member.id)
        .await
        .unwrap();
    persistence
        .remove_member(company.id, owner.id)
        .await
        .unwrap();

    let (kind, user_id): (String, Option<Uuid>) =
        sqlx::query_as("SELECT kind, user_id FROM principals WHERE id = $1")
            .bind(member_principal)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kind, "external", "the principal outlives the membership");
    assert_eq!(user_id, None);
    assert!(
        persistence
            .list_members_by_company(company.id)
            .await
            .unwrap()
            .iter()
            .all(|row| row.user_id != member.id),
        "the membership is gone"
    );
    let owner_kind: String =
        sqlx::query_scalar("SELECT kind FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company.id)
            .bind(owner.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(owner_kind, "person", "the owner is never demoted this way");

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}
