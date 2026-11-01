//! Ownership is one fact held in two columns, and invites cannot rewrite either.
//!
//! `companies.user_id` and the owner's `company_members` row are checked against each other at
//! commit, and the invite writes are conditional in SQL, so all of it is asserted on a real
//! database.

use super::*;
use crate::adapters::persistence::test_support::{
    test_pool, transfer_company_ownership, wait_until_a_backend_is_blocked,
};
use crate::entities::user::User;
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::company_invite::CompanyInviteUseCases;
use crate::use_cases::user::UserPersistence;
use std::sync::Arc;

struct OwnedCompany {
    persistence: Arc<PostgresPersistence>,
    owner: User,
    company_id: Uuid,
}

impl OwnedCompany {
    async fn new(pool: sqlx::PgPool) -> Self {
        let persistence = Arc::new(PostgresPersistence::new(pool));
        let owner = Self::account(&persistence, "owner").await;
        let company_id = CompanyPersistence::create(
            persistence.as_ref(),
            owner.id,
            CompanyWrite {
                name: "Owned".into(),
                slug: format!("owned-{}", Uuid::new_v4().simple()),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap()
        .id;
        Self {
            persistence,
            owner,
            company_id,
        }
    }

    async fn account(persistence: &PostgresPersistence, label: &str) -> User {
        let username = format!("{label}_{}", Uuid::new_v4().simple());
        let user = persistence
            .create_user(&username, &format!("{username}@example.test"), "hash")
            .await
            .unwrap();
        sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(user.id)
            .execute(persistence.pool())
            .await
            .unwrap();
        UserPersistence::get_by_id(persistence, user.id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn member(&self, label: &str, role: CompanyAccessRole) -> User {
        let user = Self::account(&self.persistence, label).await;
        let invite = self
            .persistence
            .create_invite(self.company_id, &user.email, role)
            .await
            .unwrap();
        self.persistence
            .accept_pending_invite(invite.id, user.id, &user.email)
            .await
            .unwrap()
            .unwrap();
        user
    }

    fn use_cases(&self) -> CompanyInviteUseCases {
        CompanyInviteUseCases::new(self.persistence.clone(), self.persistence.clone())
    }

    async fn role_of(&self, user_id: Uuid) -> Option<String> {
        sqlx::query_scalar(
            "SELECT role FROM company_members WHERE company_id = $1 AND user_id = $2",
        )
        .bind(self.company_id)
        .bind(user_id)
        .fetch_optional(self.persistence.pool())
        .await
        .unwrap()
    }

    async fn owner_column(&self) -> Uuid {
        sqlx::query_scalar("SELECT user_id FROM companies WHERE id = $1")
            .bind(self.company_id)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }

    /// Run `statement` in its own transaction and return the constraint the commit names.
    async fn commit_refusal(&self, statement: &str, user_id: Uuid) -> Option<String> {
        let mut transaction = self.persistence.pool().begin().await.unwrap();
        sqlx::query(statement)
            .bind(self.company_id)
            .bind(user_id)
            .execute(&mut *transaction)
            .await
            .expect("deferred: the statement itself succeeds");
        let refused = transaction
            .commit()
            .await
            .expect_err("the commit is refused");
        refused
            .as_database_error()
            .and_then(|error| error.constraint())
            .map(str::to_owned)
    }

    async fn cleanup(&self) {
        let _ = CompanyPersistence::delete(self.persistence.as_ref(), self.company_id).await;
    }
}

/// An invite addressed to the owner -- written before the guard, or raced against an owner's
/// address change -- is accepted without touching the owner's row.
#[tokio::test]
async fn company_owner_accepting_an_invite_to_own_company_stays_owner() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = OwnedCompany::new(pool).await;
    let invite = fixture
        .persistence
        .create_invite(
            fixture.company_id,
            &fixture.owner.email,
            CompanyAccessRole::Member,
        )
        .await
        .unwrap();

    let accepted = fixture
        .persistence
        .accept_pending_invite(invite.id, fixture.owner.id, &fixture.owner.email)
        .await
        .unwrap()
        .expect("the invite reads back accepted");
    assert_eq!(accepted.status, "accepted");
    assert_eq!(
        fixture.role_of(fixture.owner.id).await.as_deref(),
        Some("owner")
    );
    assert!(
        fixture
            .persistence
            .list_members_by_company(fixture.company_id)
            .await
            .unwrap()
            .is_empty(),
        "the owner did not become a removable member"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn company_owner_row_cannot_be_demoted_by_hand() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = OwnedCompany::new(pool).await;
    let constraint = fixture
        .commit_refusal(
            "UPDATE company_members SET role = 'member' WHERE company_id = $1 AND user_id = $2",
            fixture.owner.id,
        )
        .await;
    assert_eq!(
        constraint.as_deref(),
        Some("companies_owner_member_agreement")
    );
    assert_eq!(
        fixture.role_of(fixture.owner.id).await.as_deref(),
        Some("owner")
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn company_owner_row_cannot_be_deleted_by_hand() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = OwnedCompany::new(pool).await;
    let constraint = fixture
        .commit_refusal(
            "DELETE FROM company_members WHERE company_id = $1 AND user_id = $2",
            fixture.owner.id,
        )
        .await;
    assert!(
        matches!(
            constraint.as_deref(),
            Some("companies_owner_member_fk" | "companies_owner_member_agreement")
        ),
        "refused by {constraint:?}"
    );
    assert_eq!(
        fixture.role_of(fixture.owner.id).await.as_deref(),
        Some("owner")
    );
    fixture.cleanup().await;
}

/// Pointing `companies.user_id` at someone else without moving the owner row fails, whether the
/// new name is a member or nobody in the company at all.
#[tokio::test]
async fn company_owner_column_cannot_change_alone() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = OwnedCompany::new(pool).await;
    let member = fixture.member("admin", CompanyAccessRole::Admin).await;
    let stranger = OwnedCompany::account(&fixture.persistence, "stranger").await;

    let to_member = fixture
        .commit_refusal("UPDATE companies SET user_id = $2 WHERE id = $1", member.id)
        .await;
    assert_eq!(
        to_member.as_deref(),
        Some("companies_owner_member_agreement")
    );
    let to_stranger = fixture
        .commit_refusal(
            "UPDATE companies SET user_id = $2 WHERE id = $1",
            stranger.id,
        )
        .await;
    assert!(
        matches!(
            to_stranger.as_deref(),
            Some("companies_owner_member_fk" | "companies_owner_member_agreement")
        ),
        "refused by {to_stranger:?}"
    );
    assert_eq!(fixture.owner_column().await, fixture.owner.id);

    // The complete transfer, in one transaction, is the shape that commits.
    transfer_company_ownership(
        fixture.persistence.pool(),
        fixture.company_id,
        member.id,
        CompanyAccessRole::Admin,
    )
    .await;
    assert_eq!(fixture.owner_column().await, member.id);
    assert_eq!(fixture.role_of(member.id).await.as_deref(), Some("owner"));
    assert_eq!(
        fixture.role_of(fixture.owner.id).await.as_deref(),
        Some("admin")
    );
    fixture.cleanup().await;
}

/// The use case refuses an invite to the owner's own address on create and on edit, whatever its
/// case, and a malformed address is a form error rather than an internal one.
#[tokio::test]
async fn company_invite_refuses_the_owner_address_and_malformed_ones() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = OwnedCompany::new(pool).await;
    let invitations = fixture.use_cases();
    let shouted = fixture.owner.email.to_uppercase();

    let created = invitations
        .create_company_invite(
            fixture.owner.id,
            fixture.company_id,
            &shouted,
            CompanyAccessRole::Member,
        )
        .await;
    assert!(
        matches!(created, Err(AppError::BadRequest(_))),
        "{created:?}"
    );
    let malformed = invitations
        .create_company_invite(
            fixture.owner.id,
            fixture.company_id,
            "not-an-address",
            CompanyAccessRole::Member,
        )
        .await;
    assert!(
        matches!(malformed, Err(AppError::BadRequest(_))),
        "{malformed:?}"
    );

    let invite = invitations
        .create_company_invite(
            fixture.owner.id,
            fixture.company_id,
            &format!("guest-{}@example.test", Uuid::new_v4().simple()),
            CompanyAccessRole::Member,
        )
        .await
        .unwrap();
    let edited = invitations
        .update_company_invite(
            fixture.owner.id,
            fixture.company_id,
            invite.id,
            &shouted,
            None,
        )
        .await;
    assert!(matches!(edited, Err(AppError::BadRequest(_))), "{edited:?}");
    assert!(
        fixture
            .persistence
            .list_invites_by_company(fixture.company_id)
            .await
            .unwrap()
            .iter()
            .all(|invite| !invite.email.eq_ignore_ascii_case(&fixture.owner.email)),
        "no invite names the owner"
    );
    fixture.cleanup().await;
}

/// The edit is conditional in its own `UPDATE`: another company's id or an accepted invite matches
/// nothing.
#[tokio::test]
async fn company_invite_update_matches_only_a_pending_invite_of_its_company() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = OwnedCompany::new(pool.clone()).await;
    let other = OwnedCompany::new(pool).await;
    let guest = OwnedCompany::account(&fixture.persistence, "guest").await;
    let invite = fixture
        .persistence
        .create_invite(fixture.company_id, &guest.email, CompanyAccessRole::Member)
        .await
        .unwrap();

    let foreign = fixture
        .persistence
        .update_invite(
            other.company_id,
            invite.id,
            "x@example.test",
            CompanyAccessRole::Admin,
        )
        .await
        .unwrap();
    assert!(foreign.is_none(), "another company's id matches nothing");

    fixture
        .persistence
        .accept_pending_invite(invite.id, guest.id, &guest.email)
        .await
        .unwrap()
        .unwrap();
    let after_acceptance = fixture
        .persistence
        .update_invite(
            fixture.company_id,
            invite.id,
            "x@example.test",
            CompanyAccessRole::Admin,
        )
        .await
        .unwrap();
    assert!(
        after_acceptance.is_none(),
        "an accepted invite matches nothing"
    );
    let stored = fixture
        .persistence
        .get_invite_by_id(invite.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.email, guest.email);
    assert_eq!(stored.role, CompanyAccessRole::Member);
    fixture.cleanup().await;
    other.cleanup().await;
}

/// Competing claimants: the edit reads the invite while it is pending, and the acceptance commits
/// before the edit's write runs. The write matches nothing, the owner hears the existing answer,
/// and the accepted invite is unchanged.
#[tokio::test]
async fn company_invite_edit_of_an_invite_accepted_after_the_read_is_refused() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = OwnedCompany::new(pool).await;
    let guest = OwnedCompany::account(&fixture.persistence, "racer").await;
    let invite = fixture
        .persistence
        .create_invite(fixture.company_id, &guest.email, CompanyAccessRole::Member)
        .await
        .unwrap();

    // Hold the invite row so the edit's UPDATE has to wait behind the acceptance.
    let mut acceptance = fixture.persistence.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM company_invites WHERE id = $1 FOR UPDATE")
        .bind(invite.id)
        .fetch_one(&mut *acceptance)
        .await
        .unwrap();
    let edit = {
        let invitations = fixture.use_cases();
        let owner = fixture.owner.id;
        let company_id = fixture.company_id;
        tokio::spawn(async move {
            invitations
                .update_company_invite(
                    owner,
                    company_id,
                    invite.id,
                    "rewritten@example.test",
                    Some(CompanyAccessRole::Admin),
                )
                .await
        })
    };
    wait_until_a_backend_is_blocked(fixture.persistence.pool()).await;
    sqlx::query("UPDATE company_invites SET status = 'accepted' WHERE id = $1")
        .bind(invite.id)
        .execute(&mut *acceptance)
        .await
        .unwrap();
    acceptance.commit().await.unwrap();

    match edit.await.unwrap() {
        Err(AppError::BadRequest(message)) => {
            assert_eq!(message, "Only a pending invitation can be changed.")
        }
        other => panic!("expected the pending-only refusal, got {other:?}"),
    }
    let stored = fixture
        .persistence
        .get_invite_by_id(invite.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "accepted");
    assert_eq!(stored.email, guest.email);
    assert_eq!(stored.role, CompanyAccessRole::Member);
    fixture.cleanup().await;
}
