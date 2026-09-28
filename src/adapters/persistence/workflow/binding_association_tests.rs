use super::binding_tests::*;
use super::*;
use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::application::use_cases::participant::PrincipalAccessPersistence;

#[tokio::test]
async fn workflow_binding_sql_association_matches_domain_visibility_and_management() {
    let f = BindingFixture::new(serde_json::json!([])).await;
    let p = &f.fixture.persistence;
    p.create_user("admin", "admin@example.test", "hash")
        .await
        .unwrap();
    let user = p.get_by_email("admin@example.test").await.unwrap().unwrap();
    let principal = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, 'admin')",
    )
    .bind(Uuid::new_v4())
    .bind(f.target.company.as_uuid())
    .bind(user.id)
    .execute(p.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO principals (id, company_id, kind, user_id, display_label) VALUES ($1, $2, 'person', $3, 'Admin')")
        .bind(principal).bind(f.target.company.as_uuid()).bind(user.id).execute(p.pool()).await.unwrap();
    let channel = ChannelPersistence::create(
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
    let association = RelatedAssociation::Channel(RelatedChannelId::new(channel.id));
    for role in ["admin", "member"] {
        sqlx::query("UPDATE company_members SET role = $3 WHERE company_id = $1 AND user_id = $2")
            .bind(f.target.company.as_uuid())
            .bind(user.id)
            .bind(role)
            .execute(p.pool())
            .await
            .unwrap();
        for mode in ["team", "public", "allowlist"] {
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
                    let expected = context.membership.manages_company_operations()
                        && channel.viewer_access(context);
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
    let channel = ChannelPersistence::create(
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
    let second = ChannelPersistence::create(
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
        channel_id: RelatedChannelId::new(second.id),
        thread_id: RelatedThreadId::new(thread),
    };
    let bad = f.prepare(request).await;
    assert!(p.save_binding(bad).await.is_err());
    let mut request = f.request(None);
    request.association = RelatedAssociation::Thread {
        channel_id: RelatedChannelId::new(channel.id),
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
    sqlx::query("UPDATE companies SET user_id = $2 WHERE id = $1")
        .bind(f.target.company.as_uuid())
        .bind(new_owner.id)
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET role = 'admin' WHERE company_id = $1 AND user_id = $2")
        .bind(f.target.company.as_uuid())
        .bind(f.target.actor.user_id())
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE channels SET access_mode = 'allowlist' WHERE id = $1")
        .bind(channel.id)
        .execute(p.pool())
        .await
        .unwrap();
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
    // Direct grant writers need not take the company lock; the exact grant row is locked.
    let mut revocation = p.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM channel_principal_grants WHERE channel_id = $1 AND principal_id = $2")
        .bind(channel.id)
        .bind(principal)
        .execute(&mut *revocation)
        .await
        .unwrap();
    let (result, ()) = tokio::join!(p.save_binding(activation), async {
        crate::adapters::persistence::test_support::wait_until_backends_are_blocked(p.pool(), 1)
            .await;
        revocation.commit().await.unwrap();
    });
    assert!(matches!(result, Err(AppError::NotFound(_))));
    assert_eq!(f.state().await.revision, configured.revision);
}
