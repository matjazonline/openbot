use super::binding_tests::*;
use super::*;
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::channel::ChannelWrite;
use crate::application::ports::participant::PrincipalAccessPersistence;
use crate::application::workflow::{LifecycleAuthorizer, WorkflowAuthorization, WorkflowOperation};

#[tokio::test]
async fn security_email_identity_workflow_channel_and_thread_need_current_address_proof() {
    let f = BindingFixture::new(serde_json::json!([])).await;
    let p = &f.fixture.persistence;
    let company = f.target.company;
    let owner = f.target.actor;
    let admin_email = format!("admin-{}@example.test", Uuid::new_v4().simple());
    let admin = p
        .create_user(
            &format!("admin_{}", Uuid::new_v4().simple()),
            &admin_email,
            "hash",
        )
        .await
        .unwrap();
    let principal_id = crate::adapters::persistence::test_support::insert_root_member(
        p.pool(),
        crate::domain::workflow::CompanyId::new(company.as_uuid()),
        admin.id,
        crate::entities::company_member::CompanyAccessRole::Admin,
        crate::entities::access_level::AccessLevel::PUBLIC,
    )
    .await
    .as_uuid();

    let channel = crate::adapters::persistence::test_support::create_channel(
        p,
        company.as_uuid(),
        ChannelWrite {
            name: "Restricted".into(),
            slug: "restricted".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    crate::adapters::persistence::test_support::restrict_channel_to_configured_grants(
        p.pool(),
        channel.id,
    )
    .await;
    sqlx::query("INSERT INTO channel_principal_grants (company_id, channel_id, principal_id, capability, provenance) VALUES ($1, $2, $3, 'view', 'configured_allowlist')")
        .bind(company.as_uuid()).bind(channel.id).bind(principal_id)
        .execute(p.pool()).await.unwrap();
    let thread_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'Subject')",
    )
    .bind(thread_id)
    .bind(company.as_uuid())
    .bind(channel.id)
    .execute(p.pool())
    .await
    .unwrap();
    let actor = WorkflowActor::authenticated(admin.id).unwrap();
    let channel_association = RelatedAssociation::Channel(RelatedChannelId::new(channel.id));
    let thread_association = RelatedAssociation::Thread {
        target: crate::entities::address::AddressTarget::Desk(channel.id),
        thread_id: RelatedThreadId::new(thread_id),
    };
    let authorizer = LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone());

    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(admin.id)
        .execute(p.pool())
        .await
        .unwrap();
    for association in [channel_association, thread_association] {
        authorizer
            .authorize(company, actor, association, WorkflowOperation::Inspect)
            .await
            .unwrap();
    }

    // A bypassed address change clears proof on the same account. A reused session still has the
    // same user id, but both the preflight and transactional association checks must now deny it.
    sqlx::query("UPDATE users SET email_verified_at = NULL WHERE id = $1")
        .bind(admin.id)
        .execute(p.pool())
        .await
        .unwrap();
    let context = p
        .access_context_for_user(company.as_uuid(), admin.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        context.membership,
        crate::entities::company_member::CompanyMembership::Admin
    );
    assert!(context.principal_id.is_some());
    assert_eq!(
        context.grant_authority,
        crate::entities::participant::PrincipalGrantAuthority::WithheldUnverifiedEmail
    );
    for association in [channel_association, thread_association] {
        assert!(matches!(
            authorizer
                .authorize(company, actor, association, WorkflowOperation::Inspect)
                .await,
            Err(AppError::NotFound(_))
        ));
        let mut tx = p.pool().begin().await.unwrap();
        assert!(matches!(
            association::authorize_association(&mut tx, company, actor, association).await,
            Err(AppError::NotFound(_))
        ));
        tx.rollback().await.unwrap();
    }
    authorizer
        .authorize(
            company,
            actor,
            RelatedAssociation::Company,
            WorkflowOperation::Inspect,
        )
        .await
        .unwrap();
    authorizer
        .authorize(
            company,
            owner,
            channel_association,
            WorkflowOperation::Inspect,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_binding_sql_association_matches_domain_visibility_and_management() {
    let f = BindingFixture::new(serde_json::json!([])).await;
    let p = &f.fixture.persistence;
    p.create_user("admin", "admin@example.test", "hash")
        .await
        .unwrap();
    let user = p.get_by_email("admin@example.test").await.unwrap().unwrap();
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(user.id)
        .execute(p.pool())
        .await
        .unwrap();
    let principal = crate::adapters::persistence::test_support::insert_root_member(
        p.pool(),
        crate::domain::workflow::CompanyId::new(f.target.company.as_uuid()),
        user.id,
        crate::entities::company_member::CompanyAccessRole::Admin,
        crate::entities::access_level::AccessLevel::PUBLIC,
    )
    .await
    .as_uuid();

    let channel = crate::adapters::persistence::test_support::create_channel(
        p,
        f.target.company.as_uuid(),
        ChannelWrite {
            name: "Channel".into(),
            slug: "channel".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    crate::adapters::persistence::test_support::restrict_channel_to_configured_grants(
        p.pool(),
        channel.id,
    )
    .await;
    let association = RelatedAssociation::Channel(RelatedChannelId::new(channel.id));
    for role in ["admin", "member"] {
        sqlx::query("WITH cleared AS (DELETE FROM department_roles AS holder USING department_members AS member WHERE holder.company_id=member.company_id AND holder.department_id=member.department_id AND holder.principal_id=member.principal_id AND holder.role='admin' AND member.company_id = $1 AND member.root_user_id = $2 AND $3::text='member') INSERT INTO department_roles (company_id, department_id, principal_id, role) SELECT member.company_id, member.department_id, member.principal_id, 'admin' FROM department_members AS member WHERE member.company_id = $1 AND member.root_user_id = $2 AND $3::text='admin' ON CONFLICT DO NOTHING")
            .bind(f.target.company.as_uuid())
            .bind(user.id)
            .bind(role)
            .execute(p.pool())
            .await
            .unwrap();
        for mode in ["members", "public"] {
            sqlx::query("UPDATE channels SET access_mode = $2 WHERE id = $1")
                .bind(channel.id)
                .bind(mode)
                .execute(p.pool())
                .await
                .unwrap();
            for granted in [false, true] {
                sqlx::query("DELETE FROM channel_principal_grants WHERE channel_id = $1")
                    .bind(channel.id)
                    .execute(p.pool())
                    .await
                    .unwrap();
                if granted {
                    sqlx::query("INSERT INTO channel_principal_grants (company_id, channel_id, principal_id, capability, provenance) VALUES ($1, $2, $3, 'view', 'configured_allowlist')")
                        .bind(f.target.company.as_uuid()).bind(channel.id).bind(principal).execute(p.pool()).await.unwrap();
                }
                for actor in [
                    f.target.actor,
                    WorkflowActor::authenticated(user.id).unwrap(),
                ] {
                    let context = p
                        .access_context_for_user(f.target.company.as_uuid(), actor.user_id())
                        .await
                        .unwrap()
                        .unwrap();
                    let channel = ChannelPersistence::get_by_id(p, channel.id)
                        .await
                        .unwrap()
                        .unwrap();
                    let expected =
                        context.membership.administers_company() && channel.viewer_access(context);
                    let mut tx = p.pool().begin().await.unwrap();
                    let result = async {
                        authority::authorize_company(&mut tx, f.target.company, actor).await?;
                        association::authorize_association(
                            &mut tx,
                            f.target.company,
                            actor,
                            association,
                        )
                        .await
                    }
                    .await;
                    assert_eq!(
                        result.is_ok(),
                        expected,
                        "{role}/{mode}/{granted}/{actor:?}"
                    );
                    tx.rollback().await.unwrap();
                }
            }
        }
    }
}

#[tokio::test]
async fn workflow_binding_sql_grant_revocation_thread_scope_and_fixed_association() {
    let f = BindingFixture::new(serde_json::json!([])).await;
    let p = &f.fixture.persistence;
    let channel = crate::adapters::persistence::test_support::create_channel(
        p,
        f.target.company.as_uuid(),
        ChannelWrite {
            name: "Channel".into(),
            slug: "channel".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let second = crate::adapters::persistence::test_support::create_channel(
        p,
        f.target.company.as_uuid(),
        ChannelWrite {
            name: "Second".into(),
            slug: "second".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let thread = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'Subject')",
    )
    .bind(thread)
    .bind(f.target.company.as_uuid())
    .bind(channel.id)
    .execute(p.pool())
    .await
    .unwrap();
    let mut request = f.request(None);
    request.association = RelatedAssociation::Thread {
        target: crate::entities::address::AddressTarget::Desk(second.id),
        thread_id: RelatedThreadId::new(thread),
    };
    let bad = f.prepare(request).await;
    assert!(p.save_binding(bad).await.is_err());
    let mut request = f.request(None);
    request.association = RelatedAssociation::Thread {
        target: crate::entities::address::AddressTarget::Desk(channel.id),
        thread_id: RelatedThreadId::new(thread),
    };
    let configured = f.service().configure(request).await.unwrap();
    assert_eq!(f.state().await.association, configured.association);
    assert!(
        f.service()
            .configure(f.request(Some(configured.revision)))
            .await
            .is_err()
    );
    // Demote the original owner to an admin by transferring company ownership.
    p.create_user("new_owner", "new-owner@example.test", "hash")
        .await
        .unwrap();
    let new_owner = p
        .get_by_email("new-owner@example.test")
        .await
        .unwrap()
        .unwrap();
    crate::adapters::persistence::test_support::transfer_company_ownership(
        p.pool(),
        f.target.company.as_uuid(),
        new_owner.id,
        crate::entities::company_member::CompanyAccessRole::Admin,
    )
    .await;
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(f.target.actor.user_id())
        .execute(p.pool())
        .await
        .unwrap();
    crate::adapters::persistence::test_support::restrict_channel_to_configured_grants(
        p.pool(),
        channel.id,
    )
    .await;
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(f.target.company.as_uuid())
            .bind(f.target.actor.user_id())
            .fetch_one(p.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO channel_principal_grants (company_id, channel_id, principal_id, capability, provenance) VALUES ($1, $2, $3, 'view', 'configured_allowlist')")
        .bind(f.target.company.as_uuid()).bind(channel.id).bind(principal).execute(p.pool()).await.unwrap();
    let activation = f.activity(configured.revision).await;
    let mut revocation = p.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM channel_principal_grants WHERE channel_id=$1 AND principal_id=$2")
        .bind(channel.id)
        .bind(principal)
        .execute(&mut *revocation)
        .await
        .unwrap();
    // The grant writer still holds the desk row; configuration serializes with it,
    // then succeeds without borrowing the revoked human View permission.
    let (result, ()) = tokio::join!(p.save_binding(activation), async {
        crate::adapters::persistence::test_support::wait_until_backends_are_blocked(p.pool(), 1)
            .await;
        revocation.commit().await.unwrap();
    });
    result.unwrap();
    assert_eq!(
        f.state().await.revision.get(),
        configured.revision.get() + 1
    );
    assert!(
        super::super::association::authorize_association(
            &mut p.pool().acquire().await.unwrap(),
            f.target.company,
            f.target.actor,
            configured.association,
        )
        .await
        .is_err()
    );
}
