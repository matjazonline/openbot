use super::*;
use sqlx::PgPool;
use tokio::sync::Notify;

// Intercept only AFTER the real adapter has committed or performed final entry.
// This exercises service orchestration with genuine SQL markers and authority checks.
struct Intercept<'a> {
    inner: Dispatch,
    pool: &'a PgPool,
    after_marker: Option<&'a str>,
    ambiguous_ack: bool,
    entry_delay: Duration,
}
#[async_trait]
impl ActionDispatch for Intercept<'_> {
    async fn local(&self, request: &ActionDispatchRequest) -> AppResult<LocalDispatchResult> {
        self.inner.local(request).await
    }
    async fn remote(
        &self,
        request: &ActionDispatchRequest,
        provider: &dyn RemoteAction,
        cancel: &CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        supervise_remote(self, request, provider, cancel).await
    }
}
#[async_trait]
impl RemoteDispatch for Intercept<'_> {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        let reserved = self.inner.reserve_remote(request, contract).await?;
        if matches!(reserved, RemoteReservationResult::Reserved(_)) {
            if let Some(sql) = self.after_marker {
                sqlx::query(sql)
                    .bind(request.scope().company.as_uuid())
                    .execute(self.pool)
                    .await?;
            }
            if self.ambiguous_ack {
                return Err(AppError::Timeout(
                    "Injected lost commit acknowledgement".into(),
                ));
            }
        }
        Ok(reserved)
    }
    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry> {
        let entered = self.inner.enter_remote(reservation).await?;
        tokio::time::sleep(self.entry_delay).await;
        Ok(entered)
    }
    async fn finish_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation> {
        self.inner.finish_remote(entry, result).await
    }
    async fn owns_remote(&self, request: &ActionDispatchRequest) -> AppResult<bool> {
        self.inner.owns_remote(request).await
    }
}

#[tokio::test]
async fn workflow_action_dispatch_service_post_marker_revoke_cancel_zero_provider_polls() {
    for sql in [
        "UPDATE fixture_action_resources SET enabled=false WHERE company_id=$1",
        "UPDATE fixture_action_policies SET approval=true WHERE company_id=$1",
        "DELETE FROM company_members WHERE company_id=$1",
        "UPDATE workflow_runs SET state='cancelled' WHERE company_id=$1",
    ] {
        let (f, request) = setup().await;
        let provider = Provider {
            f: &f,
            calls: AtomicUsize::new(0),
            lost: false,
        };
        let writer = Intercept {
            inner: adapter(&f, 0),
            pool: f.persistence().pool(),
            after_marker: Some(sql),
            ambiguous_ack: false,
            entry_delay: Duration::ZERO,
        };
        assert!(
            ActionService::new(writer)
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(facts(&f).await, (1, 0, 0));
    }
}

#[tokio::test]
async fn workflow_action_dispatch_delayed_first_poll_checks_monotonic_window() {
    let (f, request) = setup().await;
    sqlx::query(
        "UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '0.3 seconds'",
    )
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let provider = Provider {
        f: &f,
        calls: AtomicUsize::new(0),
        lost: false,
    };
    let writer = Intercept {
        inner: adapter(&f, 0),
        pool: f.persistence().pool(),
        after_marker: None,
        ambiguous_ack: false,
        entry_delay: Duration::from_millis(400),
    };
    assert!(matches!(
        ActionService::new(writer)
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(facts(&f).await, (1, 0, 0));
}

#[tokio::test]
async fn workflow_action_dispatch_committed_marker_ambiguous_ack_retry_restart_reclaim_never_send()
{
    let (f, request) = setup().await;
    let provider = Provider {
        f: &f,
        calls: AtomicUsize::new(0),
        lost: false,
    };
    let writer = Intercept {
        inner: adapter(&f, 0),
        pool: f.persistence().pool(),
        after_marker: None,
        ambiguous_ack: true,
        entry_delay: Duration::ZERO,
    };
    let service = ActionService::new(writer);
    assert!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(facts(&f).await, (1, 0, 0));
    assert!(matches!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    let fresh = ActionService::new(adapter(&f, 0));
    assert!(matches!(
        fresh
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    expire(&f, request.fence.scope).await;
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(request.scope().run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        state, "waiting",
        "Reconcile overrides the read-handler's historical safe default"
    );
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(facts(&f).await, (1, 0, 0));
}

struct PendingProvider {
    started: Notify,
    polls: AtomicUsize,
    dropped: AtomicUsize,
}
struct PendingGuard<'a>(&'a AtomicUsize);
impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl RemoteAction for PendingProvider {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, _: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        let _guard = PendingGuard(&self.dropped);
        self.started.notify_one();
        std::future::pending().await
    }
}
#[tokio::test]
async fn workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline() {
    for mode in 0..4 {
        let (f, request) = setup().await;
        let provider = PendingProvider {
            started: Notify::new(),
            polls: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
        };
        let cancel = CancellationToken::new();
        if mode == 2 {
            sqlx::query(
                "UPDATE workflow_runs SET deadline=clock_timestamp()+interval '0.4 seconds'",
            )
            .execute(f.persistence().pool())
            .await
            .unwrap();
        }
        let service = ActionService::new(adapter(&f, 0));
        let interrupt = async {
            provider.started.notified().await;
            match mode {
                0 => {
                    sqlx::query("UPDATE background_tasks SET worker_id=$1")
                        .bind(worker().0)
                        .execute(f.persistence().pool())
                        .await
                        .unwrap();
                }
                1 => {
                    sqlx::query("UPDATE workflow_runs SET state='cancelled'")
                        .execute(f.persistence().pool())
                        .await
                        .unwrap();
                }
                2 => (),
                _ => cancel.cancel(),
            }
        };
        let result = tokio::time::timeout(Duration::from_secs(4), async {
            tokio::join!(
                service.dispatch_remote(&request, &provider, &cancel),
                interrupt
            )
            .0
        })
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(result, RemoteDispatchObservation::Interrupted));
        assert_eq!(provider.polls.load(Ordering::SeqCst), 1);
        assert_eq!(
            provider.dropped.load(Ordering::SeqCst),
            1,
            "the actual future must be dropped before dispatch returns"
        );
        assert_eq!(facts(&f).await, (1, 0, 0));
    }
}
