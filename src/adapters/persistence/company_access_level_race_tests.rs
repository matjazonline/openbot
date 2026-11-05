use super::*;
use crate::adapters::persistence::test_support::{
    own_database, wait_until_a_backend_is_blocked, wait_until_backends_are_blocked,
};

#[tokio::test]
async fn access_level_demotion_committed_first_refuses_a_waiting_assignment() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let mut demotion = f.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.company_id)
        .execute(&mut *demotion)
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(f.company_id)
        .bind(f.admin.id)
        .execute(&mut *demotion)
        .await
        .unwrap();
    let p = f.persistence.clone();
    let change = f.change(f.admin.id, f.member.id, AccessLevel::L4);
    let assignment = tokio::spawn(async move { p.set_member_access_level(change).await });
    wait_until_a_backend_is_blocked(&f.pool).await;
    demotion.commit().await.unwrap();
    assert!(matches!(
        assignment.await.unwrap(),
        Err(AppError::Forbidden(_))
    ));
    assert_eq!(f.level(f.member.id).await, AccessLevel::L3);
}

async fn pause_member_write(f: &Fixture, user: Uuid) -> sqlx::Transaction<'_, sqlx::Postgres> {
    // A real SQL trigger pauses the actual writer after it acquired its authority locks.
    sqlx::query(&format!(r#"CREATE FUNCTION pause_level_write() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN IF NEW.user_id='{user}'::uuid THEN PERFORM pg_advisory_xact_lock(91827); END IF; RETURN NEW; END $$"#))
        .execute(&f.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER pause_level_write BEFORE INSERT OR UPDATE ON company_members FOR EACH ROW EXECUTE FUNCTION pause_level_write()")
        .execute(&f.pool).await.unwrap();
    let mut gate = f.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(91827)")
        .execute(&mut *gate)
        .await
        .unwrap();
    gate
}

#[tokio::test]
async fn access_level_assignment_committed_first_finishes_before_later_demotion() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let gate = pause_member_write(&f, f.member.id).await;
    let p = f.persistence.clone();
    let change = f.change(f.admin.id, f.member.id, AccessLevel::L4);
    let assignment = tokio::spawn(async move { p.set_member_access_level(change).await });
    wait_until_a_backend_is_blocked(&f.pool).await;
    let p = f.persistence.clone();
    let change = f.change(f.owner.id, f.admin.id, AccessLevel::PUBLIC);
    let demotion = tokio::spawn(async move { p.set_member_access_level(change).await });
    wait_until_backends_are_blocked(&f.pool, 2).await;
    gate.commit().await.unwrap();
    assert_eq!(
        assignment.await.unwrap().unwrap().unwrap().access_level,
        AccessLevel::L4
    );
    assert_eq!(
        demotion.await.unwrap().unwrap().unwrap().access_level,
        AccessLevel::PUBLIC
    );
    assert_eq!(f.level(f.member.id).await, AccessLevel::L4);
}

#[tokio::test]
async fn access_level_owner_loss_is_rechecked_after_a_waiting_invite_write() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let mut transfer = f.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.company_id)
        .execute(&mut *transfer)
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(f.company_id)
        .bind(f.owner.id)
        .execute(&mut *transfer)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE company_members SET role='owner',access_level=9 WHERE company_id=$1 AND user_id=$2",
    )
    .bind(f.company_id)
    .bind(f.admin.id)
    .execute(&mut *transfer)
    .await
    .unwrap();
    sqlx::query("UPDATE companies SET user_id=$2 WHERE id=$1")
        .bind(f.company_id)
        .bind(f.admin.id)
        .execute(&mut *transfer)
        .await
        .unwrap();
    let p = f.persistence.clone();
    let command = CreateCompanyInvite {
        actor_id: f.owner.id,
        company_id: f.company_id,
        email: "waiting@example.test".into(),
        role: CompanyAccessRole::Member,
        access_level: AccessLevel::L8,
    };
    let write = tokio::spawn(async move { p.create_invite(command).await });
    wait_until_a_backend_is_blocked(&f.pool).await;
    transfer.commit().await.unwrap();
    assert!(matches!(write.await.unwrap(), Err(AppError::Forbidden(_))));
    assert_eq!(
        f.level(f.owner.id).await,
        AccessLevel::MAX,
        "former owner retains their previous level"
    );
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM company_invites WHERE email='waiting@example.test')",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert!(!exists);
}

#[tokio::test]
async fn access_level_acceptance_and_own_invite_assignment_do_not_deadlock_or_restore_levels() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let invite = f.invite(&f.admin, AccessLevel::L2).await;
    let gate = pause_member_write(&f, f.admin.id).await;
    let p = f.persistence.clone();
    let id = invite.id;
    let user = f.admin.clone();
    let acceptance =
        tokio::spawn(async move { p.accept_pending_invite(id, user.id, &user.email).await });
    wait_until_a_backend_is_blocked(&f.pool).await;
    let p = f.persistence.clone();
    let change = InviteLevelChange {
        actor_id: f.admin.id,
        company_id: f.company_id,
        invite_id: invite.id,
        access_level: AccessLevel::L3,
    };
    let assignment = tokio::spawn(async move { p.set_invite_access_level(change).await });
    wait_until_backends_are_blocked(&f.pool, 2).await;
    gate.commit().await.unwrap();
    assert_eq!(
        acceptance.await.unwrap().unwrap().unwrap().status,
        "accepted"
    );
    assert!(
        assignment.await.unwrap().unwrap().is_none(),
        "closed invitation cannot be rewritten"
    );
    assert_eq!(f.level(f.admin.id).await, AccessLevel::L4);
    let stored = f
        .persistence
        .get_invite_by_id(f.company_id, invite.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.access_level, AccessLevel::L2);
}

#[tokio::test]
async fn access_level_busy_authority_returns_a_bounded_conflict_without_a_write() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let mut blocker = f.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.company_id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        f.persistence
            .set_member_access_level(f.change(f.admin.id, f.member.id, AccessLevel::L4)),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(AppError::Conflict(_))));
    blocker.rollback().await.unwrap();
    assert_eq!(f.level(f.member.id).await, AccessLevel::L3);
}
