use super::*;

#[derive(Clone, Copy)]
enum TemporalChange {
    Lease,
    Run,
    Invocation,
    Cancel,
    Unexpired,
}

#[tokio::test]
async fn vault_dispatch_authority_lease_expiring_during_module_wait_denies() {
    Box::pin(contested_gate(TemporalChange::Lease)).await;
}
#[tokio::test]
async fn vault_dispatch_authority_run_expiring_during_module_wait_denies() {
    Box::pin(contested_gate(TemporalChange::Run)).await;
}
#[tokio::test]
async fn vault_dispatch_authority_invocation_expiring_during_module_wait_denies() {
    Box::pin(contested_gate(TemporalChange::Invocation)).await;
}
#[tokio::test]
async fn vault_dispatch_authority_cancellation_during_module_wait_denies() {
    Box::pin(contested_gate(TemporalChange::Cancel)).await;
}
#[tokio::test]
async fn vault_dispatch_authority_unexpired_contested_gate_still_succeeds() {
    Box::pin(contested_gate(TemporalChange::Unexpired)).await;
}

async fn contested_gate(change: TemporalChange) {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "temporal",
        "secret",
    )
    .await;
    attach(&setup, &vault, &account).await;
    let pool = setup.fixture.persistence.pool();
    let expiry = Utc::now() + ChronoDuration::seconds(2);
    if matches!(change, TemporalChange::Run) {
        sqlx::query("UPDATE workflow_runs SET deadline=$2 WHERE id=$1")
            .bind(setup.claim.fence.scope.run.as_uuid())
            .bind(expiry)
            .execute(pool)
            .await
            .unwrap();
    }
    let cancelled = CancellationToken::new();
    let (invocation, context) = prepare_bounded(&setup, change, expiry, cancelled.clone()).await;
    let mut held = pool.begin().await.unwrap();
    sqlx::query("SELECT owner_subject FROM integration_owner_states WHERE owner_namespace=$1 AND owner_subject=$2 FOR UPDATE")
        .bind(account.owner.namespace.as_str()).bind(account.owner.subject.as_str())
        .fetch_one(&mut *held).await.unwrap();
    if matches!(change, TemporalChange::Lease) {
        sqlx::query("UPDATE background_tasks SET lock_expires_at=$2 WHERE id=$1")
            .bind(setup.claim.fence.scope.job.0)
            .bind(expiry)
            .execute(pool)
            .await
            .unwrap();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let remote = calls.clone();
    let mut worker = tokio::spawn(async move {
        let result = vault.final_dispatch_guard(&invocation, &context).await;
        if result.is_ok() {
            remote.fetch_add(1, Ordering::SeqCst);
        }
        result
    });
    crate::adapters::persistence::test_support::wait_until_backends_are_blocked(pool, 1).await;
    match change {
        TemporalChange::Cancel => cancelled.cancel(),
        TemporalChange::Lease | TemporalChange::Run | TemporalChange::Invocation => {
            await_expiry(pool, expiry).await;
        }
        TemporalChange::Unexpired => {}
    }
    let cancelled_result = if matches!(change, TemporalChange::Cancel) {
        Some(
            tokio::time::timeout(Duration::from_secs(1), &mut worker)
                .await
                .unwrap()
                .unwrap(),
        )
    } else {
        None
    };
    held.rollback().await.unwrap();
    let result = match cancelled_result {
        Some(result) => result,
        None => worker.await.unwrap(),
    };
    let expected = usize::from(matches!(change, TemporalChange::Unexpired));
    assert_eq!(result.is_ok(), expected == 1, "{result:?}");
    assert_eq!(calls.load(Ordering::SeqCst), expected);
}

async fn prepare_bounded(
    setup: &Setup,
    change: TemporalChange,
    expiry: chrono::DateTime<Utc>,
    cancelled: CancellationToken,
) -> (AuthorizedInvocation, ExecutionContext) {
    let p = &setup.fixture.persistence;
    let request = p
        .http_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    p.prepare_integration_invocation(
        ActionDispatchRequest {
            subject: intent.approval_subject(),
            fence: setup.claim.fence,
            lease: lease(),
        },
        integration::DispatchId::new(Uuid::new_v4()),
        if matches!(change, TemporalChange::Invocation) {
            expiry
        } else {
            Utc::now() + ChronoDuration::seconds(20)
        },
        cancelled,
    )
    .await
    .unwrap()
}
async fn await_expiry(pool: &sqlx::PgPool, expiry: chrono::DateTime<Utc>) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1")
                .bind(expiry)
                .fetch_one(pool)
                .await
                .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
