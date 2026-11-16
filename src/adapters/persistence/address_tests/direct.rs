use super::*;
#[path = "bindings.rs"]
mod bindings;
#[path = "ingress.rs"]
mod ingress;
#[path = "rejoin.rs"]
mod rejoin;
use crate::{
    application::ports::participant::PrincipalAccessPersistence,
    entities::{
        company_member::CompanyMembership,
        participant::{Principal, PrincipalAccessContext, PrincipalGrantAuthority, PrincipalKind},
        transport::PrincipalId,
    },
    use_cases::agent::{AgentPersistence, AgentWrite},
};

async fn principal(f: &Fixture, id: Uuid) -> Principal {
    f.persistence
        .principal_by_id(f.company.id, PrincipalId::new(id))
        .await
        .unwrap()
        .unwrap()
}

async fn agent_principal(f: &Fixture) -> Uuid {
    let agent = AgentPersistence::create(
        &f.persistence,
        f.company.id,
        AgentWrite {
            name: "Direct agent".into(),
            slug: "direct-agent".into(),
            ..AgentWrite::default()
        },
    )
    .await
    .unwrap();
    sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2")
        .bind(f.company.id)
        .bind(agent.id)
        .fetch_one(f.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn direct_thread_policy_sql_matches_domain_for_live_and_archived_targets() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (member, name) = account(&f.persistence, "direct_member").await;
    let person = f.join(member, &name).await;
    let (teammate, name) = account(&f.persistence, "direct_teammate").await;
    f.join(teammate, &name).await;
    let (admin, name) = account(&f.persistence, "direct_admin").await;
    f.join(admin, &name).await;
    sqlx::query("UPDATE company_members SET role = 'admin' WHERE company_id = $1 AND user_id = $2")
        .bind(f.company.id)
        .bind(admin)
        .execute(f.pool())
        .await
        .unwrap();
    let agent = agent_principal(&f).await;
    let mut viewers = Vec::new();
    for user in [f.owner, member, teammate, admin] {
        viewers.push(
            f.persistence
                .access_context_for_user(f.company.id, user)
                .await
                .unwrap()
                .unwrap(),
        );
    }
    viewers.push(PrincipalAccessContext {
        principal_id: None,
        membership: CompanyMembership::None,
        access_level: crate::entities::access_level::AccessLevel::PUBLIC,
        grant_authority: PrincipalGrantAuthority::Available,
    });
    for (target, public, retired) in [
        (person, false, false),
        (agent, false, false),
        (agent, true, false),
        (person, false, true),
    ] {
        if retired {
            f.persistence
                .remove_member(f.company.id, member)
                .await
                .unwrap();
        } else if target == agent {
            sqlx::query(
                "UPDATE principals SET open_to_public = $3 WHERE company_id = $1 AND id = $2",
            )
            .bind(f.company.id)
            .bind(agent)
            .bind(public)
            .execute(f.pool())
            .await
            .unwrap();
        }
        let target = principal(&f, target).await;
        for viewer in &viewers {
            // Retirement also removes the source's live membership. Rehydrate that context rather
            // than accidentally asserting authority from the saved pre-removal actor.
            let current = if retired && viewer.principal_id == Some(target.id) {
                PrincipalAccessContext::external(target.id)
            } else {
                *viewer
            };
            let (can_view, can_write): (bool, bool) = sqlx::query_as(
                "SELECT principal_direct_can_view($1,$2,$3), principal_direct_can_participate($1,$2,$3)",
            ).bind(f.company.id).bind(target.id.as_uuid())
                .bind(current.principal_id.map(PrincipalId::as_uuid))
                .fetch_one(f.pool()).await.unwrap();
            assert_eq!(
                can_view,
                target.direct_viewer_access(current),
                "target {target:?}, actor {current:?}"
            );
            assert_eq!(
                can_write,
                target.direct_access(current).authorized,
                "target {target:?}, actor {current:?}"
            );
        }
    }
}

#[tokio::test]
async fn direct_thread_personal_ownership_survives_removal_rejoin_and_email_reuse() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "direct_owner").await;
    let email = email_of(f.pool(), user).await;
    let original = f.join(user, &name).await;
    let stored = principal(&f, original).await;
    assert_eq!(stored.historical_person_user_id, Some(user));
    f.persistence
        .remove_member(f.company.id, user)
        .await
        .unwrap();
    let archived = principal(&f, original).await;
    assert_eq!(archived.kind, PrincipalKind::External);
    assert_eq!(archived.user_id, None);
    assert_eq!(archived.historical_person_user_id, Some(user));
    for forged in [None, Some(Uuid::new_v4())] {
        let mut transaction = f.pool().begin().await.unwrap();
        let error = sqlx::query(
            "UPDATE principals SET historical_person_user_id=$3 WHERE company_id=$1 AND id=$2",
        )
        .bind(f.company.id)
        .bind(original)
        .bind(forged)
        .execute(&mut *transaction)
        .await
        .unwrap_err();
        assert_eq!(
            refused_by(error),
            ("23514".into(), "principals_historical_person_owner".into())
        );
        transaction.rollback().await.unwrap();
    }
    assert_eq!(
        f.join(user, &name).await,
        original,
        "same account restores its own principal"
    );

    HostAccountDeletion::delete_user_account(&f.persistence, user)
        .await
        .unwrap();
    let replacement_name = format!("replacement_{}", Uuid::new_v4().simple());
    f.persistence
        .create_user(&replacement_name, &email, "hash")
        .await
        .unwrap();
    let replacement = f.persistence.get_by_email(&email).await.unwrap().unwrap();
    let new_principal = f.join(replacement.id, &replacement_name).await;
    assert_ne!(
        new_principal, original,
        "reused email does not transfer personal targets"
    );
    assert_eq!(
        principal(&f, original).await.historical_person_user_id,
        Some(user)
    );
    assert_eq!(
        principal(&f, new_principal).await.historical_person_user_id,
        Some(replacement.id)
    );
    let mut transaction = f.pool().begin().await.unwrap();
    let error = sqlx::query(
        "UPDATE principals SET kind='person', user_id=$3 WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company.id)
    .bind(original)
    .bind(replacement.id)
    .execute(&mut *transaction)
    .await
    .unwrap_err();
    assert_eq!(
        refused_by(error),
        ("23514".into(), "principals_historical_person_owner".into())
    );
    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn direct_public_settings_principal_round_trip_revision_and_kind_constraint() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let id = agent_principal(&f).await;
    let initial = principal(&f, id).await;
    assert!(!initial.open_to_public);
    sqlx::query("UPDATE principals SET open_to_public = true WHERE company_id=$1 AND id=$2")
        .bind(f.company.id)
        .bind(id)
        .execute(f.pool())
        .await
        .unwrap();
    let opened = principal(&f, id).await;
    assert!(opened.open_to_public);
    assert_eq!(opened.settings_revision, initial.settings_revision + 1);
    sqlx::query("UPDATE principals SET open_to_public = true WHERE company_id=$1 AND id=$2")
        .bind(f.company.id)
        .bind(id)
        .execute(f.pool())
        .await
        .unwrap();
    assert_eq!(
        principal(&f, id).await.settings_revision,
        opened.settings_revision
    );
    sqlx::query("UPDATE principals SET open_to_public = false WHERE company_id=$1 AND id=$2")
        .bind(f.company.id)
        .bind(id)
        .execute(f.pool())
        .await
        .unwrap();
    assert!(!principal(&f, id).await.open_to_public);
    let owner = f.principal_of(f.owner).await;
    let error =
        sqlx::query("UPDATE principals SET open_to_public=true WHERE company_id=$1 AND id=$2")
            .bind(f.company.id)
            .bind(owner)
            .execute(f.pool())
            .await
            .unwrap_err();
    assert_eq!(
        refused_by(error),
        ("23514".into(), "principals_public_agent_check".into())
    );
    assert!(
        f.persistence
            .principal_by_id(Uuid::new_v4(), PrincipalId::new(id))
            .await
            .unwrap()
            .is_none()
    );
    let before = principal(&f, owner).await.settings_revision;
    f.persistence
        .rename_person_address(f.owner, f.company.id, &ChannelSlug::from("new-owner"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(principal(&f, owner).await.settings_revision, before + 1);
}

async fn lock_rejoin_prefix(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company: Uuid,
    user: Uuid,
) {
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut **transaction)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(user)
        .execute(&mut **transaction)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(company)
        .execute(&mut **transaction)
        .await
        .unwrap();
}

#[tokio::test]
async fn direct_competing_personal_rejoin_claimants_restore_one_lifetime_target() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "rejoin").await;
    let old_email = email_of(f.pool(), user).await;
    let original = f.join(user, &name).await;
    f.persistence
        .remove_member(f.company.id, user)
        .await
        .unwrap();
    let email = EmailAddress::from(format!(
        "rejoin-new-{}@example.test",
        Uuid::new_v4().simple()
    ));
    rejoin::change_email(&f, user, &email).await;
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',1)")
        .bind(Uuid::new_v4()).bind(f.company.id).bind(user).execute(f.pool()).await.unwrap();
    let mut first = f.pool().begin().await.unwrap();
    lock_rejoin_prefix(&mut first, f.company.id, user).await;
    let one = create_person_principal_on(&mut first, f.company.id, user, &name, &email)
        .await
        .unwrap();
    let pool = f.pool().clone();
    let company = f.company.id;
    let second = tokio::spawn(async move {
        let mut transaction = pool.begin().await.unwrap();
        lock_rejoin_prefix(&mut transaction, company, user).await;
        let id = create_person_principal_on(&mut transaction, company, user, &name, &email)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        id
    });
    wait_until_a_backend_is_blocked(f.pool()).await;
    assert!(
        !second.is_finished(),
        "the competing production resolver must wait for ownership"
    );
    first.commit().await.unwrap();
    let two = second.await.unwrap();
    assert_eq!(one.as_uuid(), original);
    assert_eq!(two, one);
    assert_eq!(f.addresses_of(original).await.len(), 1);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM principals WHERE company_id=$1 AND historical_person_user_id=$2",
    )
    .bind(f.company.id)
    .bind(user)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
    let old_identity = EmailIdentity::parse(EmailAddress::from(old_email))
        .unwrap()
        .qualify_default();
    assert_eq!(
        f.persistence
            .access_context_for_identity(f.company.id, &old_identity)
            .await
            .unwrap()
            .membership,
        CompanyMembership::None,
    );
}

#[tokio::test]
async fn direct_thread_native_owner_shape_rejects_null_person_and_historical_system() {
    // Trigger changes affect the entire table, so this test owns its migrated database.
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (user, _) = account(&f.persistence, "native_owner").await;
    for (kind, live, historical) in [("person", Some(user), None), ("system", None, Some(user))] {
        let mut transaction = f.pool().begin().await.unwrap();
        sqlx::query("ALTER TABLE principals DISABLE TRIGGER principals_direct_settings")
            .execute(&mut *transaction)
            .await
            .unwrap();
        let error = sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,historical_person_user_id,display_label) VALUES($1,$2,$3,$4,$5,'invalid')")
            .bind(Uuid::new_v4()).bind(f.company.id).bind(kind).bind(live).bind(historical)
            .execute(&mut *transaction).await.unwrap_err();
        assert_eq!(
            refused_by(error),
            ("23514".into(), "principals_historical_person_check".into())
        );
        transaction.rollback().await.unwrap();
    }
}
