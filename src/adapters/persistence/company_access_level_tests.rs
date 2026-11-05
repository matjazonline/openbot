use super::*;
use crate::adapters::protocols::email::EmailIdentity;
use crate::{
    application::ports::participant::{IdentityObservation, PrincipalAccessPersistence},
    entities::{
        access_level::AccessLevel,
        company_member::CompanyMembership,
        participant::{IdentityClaimMetadata, IdentityProvenance},
        transport::QualifiedIdentity,
    },
    use_cases::{company::CompanyPersistence, participant::IdentityDirectory},
};
#[path = "company_access_level_test_support.rs"]
mod support;
use crate::adapters::persistence::test_support::test_pool;
use support::{Fixture, account};
#[path = "company_access_level_race_tests.rs"]
mod races;

#[tokio::test]
async fn access_level_sql_domain_owner_and_defaults_are_enforced() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = Fixture::new(pool).await;
    for invalid in [-1_i16, 10] {
        let error = sqlx::query("UPDATE company_members SET access_level=$3::smallint WHERE company_id=$1 AND user_id=$2")
            .bind(f.company_id).bind(f.member.id).bind(invalid).execute(&f.pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        assert!(
            sqlx::query("UPDATE company_invites SET access_level=$2::smallint WHERE company_id=$1")
                .bind(f.company_id)
                .bind(invalid)
                .execute(&f.pool)
                .await
                .is_err()
        );
    }
    assert!(
        sqlx::query(
            "UPDATE company_members SET access_level=8 WHERE company_id=$1 AND role='owner'"
        )
        .bind(f.company_id)
        .execute(&f.pool)
        .await
        .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE company_members SET access_level=NULL WHERE company_id=$1 AND role='owner'"
        )
        .bind(f.company_id)
        .execute(&f.pool)
        .await
        .is_err()
    );
    assert_eq!(f.level(f.owner.id).await, AccessLevel::MAX);
    let user = account(&f.persistence, "default").await;
    sqlx::query(
        "INSERT INTO company_members(id,company_id,user_id,role) VALUES($1,$2,$3,'member')",
    )
    .bind(Uuid::new_v4())
    .bind(f.company_id)
    .bind(user.id)
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(f.level(user.id).await, AccessLevel::PUBLIC);
    let level: i16 = sqlx::query_scalar("INSERT INTO company_invites(id,company_id,email,role) VALUES($1,$2,'default@example.test','member') RETURNING access_level::smallint")
        .bind(Uuid::new_v4()).bind(f.company_id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(level, 0);
}

#[tokio::test]
async fn access_level_invite_copies_new_grants_and_never_restores_existing_grants() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = Fixture::new(pool).await;
    let user = account(&f.persistence, "new").await;
    let first = f.invite(&user, AccessLevel::L5).await;
    f.persistence
        .accept_pending_invite(first.id, user.id, &user.email)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.level(user.id).await, AccessLevel::L5);
    for level in [AccessLevel::PUBLIC, AccessLevel::MAX] {
        let invite = f.invite(&user, level).await;
        f.persistence
            .accept_pending_invite(invite.id, user.id, &user.email)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(f.level(user.id).await, AccessLevel::L5);
    }
    f.persistence
        .set_member_access_level(f.change(f.owner.id, user.id, AccessLevel::L1))
        .await
        .unwrap()
        .unwrap();
    f.persistence
        .accept_pending_invite(first.id, user.id, &user.email)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.level(user.id).await, AccessLevel::L1);
    let owner_invite: Uuid = sqlx::query_scalar("INSERT INTO company_invites(id,company_id,email,role,access_level) VALUES($1,$2,$3,'member',0) RETURNING id")
        .bind(Uuid::new_v4()).bind(f.company_id).bind(&f.owner.email).fetch_one(&f.pool).await.unwrap();
    f.persistence
        .accept_pending_invite(owner_invite, f.owner.id, &f.owner.email)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.level(f.owner.id).await, AccessLevel::MAX);
}

#[tokio::test]
async fn access_level_native_writers_apply_caps_scope_and_owner_only_role_powers() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = Fixture::new(pool).await;
    let p = f.persistence.as_ref();
    let member = p
        .set_member_access_level(f.change(f.admin.id, f.member.id, AccessLevel::L4))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(member.role, CompanyAccessRole::Member);
    assert!(matches!(
        p.set_member_access_level(f.change(f.admin.id, f.member.id, AccessLevel::L5))
            .await,
        Err(AppError::Forbidden(_))
    ));
    p.set_member_access_level(f.change(f.owner.id, f.member.id, AccessLevel::L5))
        .await
        .unwrap();
    assert!(matches!(
        p.set_member_access_level(f.change(f.admin.id, f.member.id, AccessLevel::PUBLIC))
            .await,
        Err(AppError::Forbidden(_))
    ));
    assert!(matches!(
        p.set_member_access_level(f.change(f.member.id, f.admin.id, AccessLevel::PUBLIC))
            .await,
        Err(AppError::Forbidden(_))
    ));
    assert!(matches!(
        p.set_member_access_level(f.change(f.owner.id, f.owner.id, AccessLevel::L8))
            .await,
        Err(AppError::BadRequest(_))
    ));
    let stranger = account(p, "stranger").await;
    assert!(matches!(
        p.set_member_access_level(f.change(stranger.id, f.admin.id, AccessLevel::PUBLIC))
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_invite_writer_authority(&f, &stranger).await;
}

async fn assert_invite_writer_authority(f: &Fixture, stranger: &crate::entities::user::User) {
    let p = f.persistence.as_ref();
    let invite = f.invite(stranger, AccessLevel::L3).await;
    let saved = p
        .set_invite_access_level(InviteLevelChange {
            actor_id: f.admin.id,
            company_id: f.company_id,
            invite_id: invite.id,
            access_level: AccessLevel::L4,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.email, invite.email);
    assert_eq!(saved.role, invite.role);
    assert!(matches!(
        p.update_invite(UpdateCompanyInvite {
            actor_id: f.admin.id,
            company_id: f.company_id,
            invite_id: invite.id,
            email: "hijack@example.test".into(),
            role: Some(CompanyAccessRole::Admin),
            access_level: None
        })
        .await,
        Err(AppError::Forbidden(_))
    ));
    assert!(matches!(
        p.create_invite(CreateCompanyInvite {
            actor_id: f.admin.id,
            company_id: f.company_id,
            email: "new@example.test".into(),
            role: CompanyAccessRole::Admin,
            access_level: AccessLevel::PUBLIC
        })
        .await,
        Err(AppError::Forbidden(_))
    ));
    assert!(
        p.get_invite_by_id(Uuid::new_v4(), invite.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        p.set_invite_access_level(InviteLevelChange {
            actor_id: f.owner.id,
            company_id: Uuid::new_v4(),
            invite_id: invite.id,
            access_level: AccessLevel::PUBLIC
        })
        .await
        .unwrap()
        .is_none()
    );
}

fn email(address: &str) -> QualifiedIdentity {
    EmailIdentity::parse(address.into())
        .unwrap()
        .qualify_default()
}
#[tokio::test]
async fn access_level_joint_readers_external_observed_and_former_members_are_public() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = Fixture::new(pool).await;
    let p = f.persistence.as_ref();
    assert_current_readers(&f).await;
    let external = email("outside@example.test");
    p.resolve_or_create_external_identity(
        f.company_id,
        IdentityObservation {
            identity: external.clone(),
            display_label: None,
            claim_metadata: IdentityClaimMetadata::observation(),
            provenance: IdentityProvenance::TransportIngress,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        p.access_context_for_identity(f.company_id, &external)
            .await
            .unwrap()
            .access_level,
        AccessLevel::PUBLIC
    );
    sqlx::query(
        "UPDATE participant_identities SET status='observed' WHERE company_id=$1 AND subject=$2",
    )
    .bind(f.company_id)
    .bind(f.member.email.as_str())
    .execute(&f.pool)
    .await
    .unwrap();
    let observed = p
        .access_context_for_identity(f.company_id, &email(&f.member.email))
        .await
        .unwrap();
    assert_eq!(observed.membership, CompanyMembership::None);
    assert_eq!(observed.access_level, AccessLevel::PUBLIC);
    p.remove_member(f.company_id, f.admin.id).await.unwrap();
    let former = p
        .access_context_for_identity(f.company_id, &email(&f.admin.email))
        .await
        .unwrap();
    assert_eq!(former.membership, CompanyMembership::None);
    assert_eq!(former.access_level, AccessLevel::PUBLIC);
    assert!(
        p.access_context_for_user(f.company_id, f.admin.id)
            .await
            .unwrap()
            .is_none()
    );
}

async fn assert_current_readers(f: &Fixture) {
    let p = f.persistence.as_ref();
    struct Reader<'a> {
        user: &'a crate::entities::user::User,
        level: AccessLevel,
        membership: CompanyMembership,
    }
    for Reader {
        user,
        level,
        membership,
    } in [
        Reader {
            user: &f.owner,
            level: AccessLevel::MAX,
            membership: CompanyMembership::Owner,
        },
        Reader {
            user: &f.admin,
            level: AccessLevel::L4,
            membership: CompanyMembership::Admin,
        },
        Reader {
            user: &f.member,
            level: AccessLevel::L3,
            membership: CompanyMembership::Member,
        },
    ] {
        let session = p
            .access_context_for_user(f.company_id, user.id)
            .await
            .unwrap()
            .unwrap();
        let identity = p
            .access_context_for_identity(f.company_id, &email(&user.email))
            .await
            .unwrap();
        let access = CompanyPersistence::company_access(p, user.id, f.company_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.membership, membership);
        assert_eq!(identity.membership, membership);
        assert_eq!(session.access_level, level);
        assert_eq!(identity.access_level, level);
        assert_eq!(access.access_level, level);
    }
}
