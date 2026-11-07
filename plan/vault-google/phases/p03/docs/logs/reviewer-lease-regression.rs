
#[tokio::test]
async fn reviewer_final_gate_denies_lease_expired_during_module_lock_wait() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(&setup, setup.fixture.target.actor.user_id(), "lease-repro", "secret").await;
    attach(&setup, &vault, &account).await;
    let (invocation, context) = prepared(&setup).await;
    let pool = setup.fixture.persistence.pool();
    let mut held = pool.begin().await.unwrap();
    sqlx::query("SELECT owner_subject FROM integration_owner_states WHERE owner_namespace=$1 AND owner_subject=$2 FOR UPDATE")
        .bind(account.owner.namespace.as_str()).bind(account.owner.subject.as_str()).fetch_one(&mut *held).await.unwrap();
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '1 second' WHERE id=$1")
        .bind(setup.claim.fence.scope.job.0).execute(pool).await.unwrap();
    let worker=tokio::spawn(async move { vault.final_dispatch_guard(&invocation,&context).await });
    crate::adapters::persistence::test_support::wait_until_backends_are_blocked(pool,1).await;
    tokio::time::sleep(Duration::from_millis(1300)).await;
    held.rollback().await.unwrap();
    let result=worker.await.unwrap();
    let expired:bool=sqlx::query_scalar("SELECT lock_expires_at<=clock_timestamp() FROM background_tasks WHERE id=$1")
        .bind(setup.claim.fence.scope.job.0).fetch_one(pool).await.unwrap();
    assert!(expired);
    assert!(result.is_err(), "final_dispatch_guard accepted an expired lease after waiting on module owner lock: {result:?}");
}
