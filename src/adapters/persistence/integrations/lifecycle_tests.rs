use super::super::lifecycle;
use super::*;
use crate::adapters::persistence::test_support::{
    insert_company_with_owner, wait_until_a_backend_is_blocked,
};
async fn retire(f: &Fixture, owner: &AccountOwner) {
    let mut tx = f.pool().begin().await.unwrap();
    lifecycle::retire_owner_on(&mut tx, owner).await.unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn vault_storage_durable_owner_teardown_restart_competitors_and_no_resurrection() {
    let f = Fixture::isolated().await;
    let account = f.create("retire").await;
    let other = AccountOwner {
        namespace: OwnerNamespace::parse("other-client").unwrap(),
        subject: f.owner.subject.clone(),
    };
    let survivor = f
        .vault
        .create(&other, static_write("survives", "other-secret"))
        .await
        .unwrap();
    retire(&f, &f.owner).await;
    retire(&f, &f.owner).await;
    assert!(f.vault.read_usable(&f.owner, account.id).await.is_err());
    assert_eq!(
        f.vault.get(&f.owner, account.id).await.unwrap().status,
        AccountStatus::Disconnected
    );
    assert!(matches!(
        f.vault
            .create(&f.owner, static_write("resurrection", "secret"))
            .await,
        Err(IntegrationError::Denied)
    ));
    let restarted = PostgresVault::new(f.vault.persistence.clone());
    let barrier = tokio::sync::Barrier::new(2);
    let stop = CancellationToken::new();
    let left = async {
        barrier.wait().await;
        f.vault.drain(1, &stop).await.unwrap()
    };
    let right = async {
        barrier.wait().await;
        restarted.drain(1, &stop).await.unwrap()
    };
    let (left, right) = tokio::join!(left, right);
    assert_eq!(left + right, 1);
    assert!(matches!(
        f.vault.get(&f.owner, account.id).await,
        Err(IntegrationError::NotFound)
    ));
    assert!(f.vault.read_usable(&other, survivor.id).await.is_ok());
    assert_eq!(restarted.drain(16, &stop).await.unwrap(), 0);
    assert!(matches!(
        restarted
            .create(&f.owner, static_write("late", "secret"))
            .await,
        Err(IntegrationError::Denied)
    ));
}
#[tokio::test]
async fn vault_storage_teardown_poison_full_batch_backoff_restart_and_terminal_failure() {
    let f = Fixture::isolated().await;
    let mut ids = Vec::new();
    for index in 0..16 {
        let owner = AccountOwner {
            namespace: f.owner.namespace.clone(),
            subject: OwnerSubject::parse(format!("{}-{index}", f.owner.subject.as_str())).unwrap(),
        };
        let account = f
            .vault
            .create(&owner, static_write("poison", "secret"))
            .await
            .unwrap();
        ids.push(account.id.as_uuid());
        retire(&f, &owner).await;
    }
    let mut held = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM integration_accounts WHERE id=ANY($1) FOR UPDATE")
        .bind(&ids)
        .fetch_all(&mut *held)
        .await
        .unwrap();
    let stop = CancellationToken::new();
    assert_eq!(f.vault.drain(16, &stop).await.unwrap(), 16);
    let attempted:i64=sqlx::query_scalar("SELECT count(*) FROM integration_teardown_intents WHERE attempts=1 AND state='pending' AND available_at>clock_timestamp()").fetch_one(f.pool()).await.unwrap();
    assert_eq!(attempted, 16);
    assert_eq!(f.vault.drain(16, &stop).await.unwrap(), 0);
    // Drive one real blocked retry to the terminal limit; the rest remain deferred.
    sqlx::query("UPDATE integration_teardown_intents SET attempts=7,available_at=clock_timestamp()-interval '1 second' WHERE id=(SELECT id FROM integration_teardown_intents ORDER BY id LIMIT 1)").execute(f.pool()).await.unwrap();
    assert_eq!(f.vault.drain(16, &stop).await.unwrap(), 1);
    let terminal: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM integration_teardown_intents WHERE attempts=8 AND state='failed'",
    )
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(terminal, 1);
    assert_eq!(f.vault.drain(16, &stop).await.unwrap(), 0);
    held.rollback().await.unwrap();
    sqlx::query("UPDATE integration_teardown_intents SET available_at=clock_timestamp()-interval '1 second' WHERE state='pending'").execute(f.pool()).await.unwrap();
    let restarted = PostgresVault::new(f.vault.persistence.clone());
    assert_eq!(restarted.drain(16, &stop).await.unwrap(), 15);
    let retained:i64=sqlx::query_scalar("SELECT count(*) FROM integration_accounts WHERE id=ANY($1) AND envelope IS NULL AND status='disconnected'").bind(&ids).fetch_one(f.pool()).await.unwrap();
    assert_eq!(retained, 1);
}
#[tokio::test]
async fn vault_storage_supervised_teardown_shutdown_cancels_actual_database_work() {
    let f = Fixture::isolated().await;
    let account = f.create("cancel").await;
    retire(&f, &f.owner).await;
    let mut held = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM integration_accounts WHERE id=$1 FOR UPDATE")
        .bind(account.id.as_uuid())
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let stop = CancellationToken::new();
    let worker_stop = stop.clone();
    let vault = f.vault.clone();
    let worker = tokio::spawn(async move {
        crate::application::integrations::lifecycle::run(&vault, &worker_stop).await
    });
    wait_until_a_backend_is_blocked(f.pool()).await;
    stop.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(3), worker)
        .await
        .unwrap()
        .unwrap();
    let state: (String, i32) =
        sqlx::query_as("SELECT state,attempts FROM integration_teardown_intents")
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(state, ("pending".into(), 0));
    held.rollback().await.unwrap();
    assert_eq!(
        f.vault.drain(16, &CancellationToken::new()).await.unwrap(),
        1
    );
}
async fn host_user(f: &Fixture) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users(id,username,email,password_hash) VALUES($1,$2,$3,'fixture-hash')",
    )
    .bind(id)
    .bind(format!("vault-{}", id.simple()))
    .bind(format!("vault-{}@example.test", id.simple()))
    .execute(f.pool())
    .await
    .unwrap();
    id
}
#[tokio::test]
async fn vault_storage_host_company_membership_and_user_deletion_preserve_other_accounts() {
    use crate::{
        application::ports::account_deletion::HostAccountDeletion,
        use_cases::{company::CompanyPersistence, company_invite::CompanyInvitePersistence},
    };
    let mut f = Fixture::isolated().await;
    let user = host_user(&f).await;
    let other = host_user(&f).await;
    f.owner = crate::adapters::integrations::owner::owner_for_user(user);
    let other_owner = crate::adapters::integrations::owner::owner_for_user(other);
    let account = f.create("host-owner").await;
    let survivor = f
        .vault
        .create(&other_owner, static_write("survivor", "secret"))
        .await
        .unwrap();
    let company = Uuid::new_v4();
    insert_company_with_owner(
        f.pool(),
        company,
        "Company",
        &format!("vault-{}", company.simple()),
        other,
    )
    .await;
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',0)").bind(Uuid::new_v4()).bind(company).bind(user).execute(f.pool()).await.unwrap();
    f.vault
        .persistence
        .remove_member(company, user)
        .await
        .unwrap();
    assert!(f.vault.read_usable(&f.owner, account.id).await.is_ok());
    f.vault.persistence.delete(company).await.unwrap();
    assert!(f.vault.read_usable(&other_owner, survivor.id).await.is_ok());
    f.vault.drain(16, &CancellationToken::new()).await.unwrap();
    assert!(f.vault.read_usable(&f.owner, account.id).await.is_ok());
    let owned = Uuid::new_v4();
    insert_company_with_owner(
        f.pool(),
        owned,
        "Owned",
        &format!("vault-{}", owned.simple()),
        user,
    )
    .await;
    f.vault.persistence.delete_user_account(user).await.unwrap();
    f.vault.persistence.delete_user_account(user).await.unwrap();
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
        .bind(user)
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert!(!exists);
    assert!(f.vault.read_usable(&f.owner, account.id).await.is_err());
    let restarted = PostgresVault::new(f.vault.persistence.clone());
    restarted
        .drain(16, &CancellationToken::new())
        .await
        .unwrap();
    assert!(matches!(
        restarted.get(&f.owner, account.id).await,
        Err(IntegrationError::NotFound)
    ));
    assert!(
        restarted
            .read_usable(&other_owner, survivor.id)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn vault_storage_supervised_teardown_shutdown_cancels_pool_acquisition() {
    struct Observed {
        vault: PostgresVault,
        entered: Arc<tokio::sync::Notify>,
    }
    #[async_trait::async_trait]
    impl TeardownDrain for Observed {
        async fn drain(&self, limit: u16, stop: &CancellationToken) -> IntegrationResult<usize> {
            self.entered.notify_one();
            self.vault.drain(limit, stop).await
        }
    }
    let f = Fixture::isolated().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(f.pool().connect_options().as_ref().clone())
        .await
        .unwrap();
    let held = pool.acquire().await.unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let observed = Observed {
        vault: PostgresVault::new(PostgresPersistence::new(pool.clone())),
        entered: entered.clone(),
    };
    let stop = CancellationToken::new();
    let worker_stop = stop.clone();
    let worker = tokio::spawn(async move {
        crate::application::integrations::lifecycle::run(&observed, &worker_stop).await
    });
    // The current-thread runtime runs the observed drain through the pending acquire
    // before delivering this notification, while the only connection stays held.
    entered.notified().await;
    stop.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(3), worker)
        .await
        .unwrap()
        .unwrap();
    drop(held);
    pool.close().await;
}
