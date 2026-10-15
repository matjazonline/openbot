//! 04.8 group B3 (R-006 Q3): the history B1 leaves when an actual CancelCommand interrupts a
//! provider that applied its effect and then hung, recovered through the real reconciliation
//! verifier. Unlike the parked 04.7 histories here, this run was never released for
//! reconciliation: its uncertainty row comes from the cancellation itself.
use super::*;

struct Dropped<'a>(&'a AtomicUsize);
impl Drop for Dropped<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// The ledger provider applies the effect durably, and then the response never arrives.
struct Hung {
    inner: LedgerProvider,
    started: Notify,
    dropped: AtomicUsize,
}
#[async_trait]
impl RemoteAction for Hung {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(
        &self,
        action: &FrozenAction,
        invocation: &ProviderInvocation,
    ) -> AppResult<Value> {
        let _guard = Dropped(&self.dropped);
        let withheld = self.inner.invoke(action, invocation).await;
        assert!(withheld.is_err(), "applied, with the response withheld");
        self.started.notify_one();
        std::future::pending().await
    }
}

/// B1's durable shape after cancel-while-hung: run cancelled, attempt retired by the cancel,
/// marker and entry kept, no receipt, one cancellation uncertainty row, the effect applied.
async fn cancelled_hung_history(f: &AdmissionFixture) -> Value {
    let state = all_tables(f).await;
    assert_eq!(state["workflow_runs"][0]["state"], "cancelled");
    assert_eq!(
        state["task_attempts"][0]["workflow_retirement"], "cancel",
        "retired by the cancellation, never parked"
    );
    for (table, count) in [
        ("workflow_action_dispatches", 1),
        ("workflow_action_remote_entries", 1),
        ("workflow_action_receipts", 0),
        ("workflow_action_reconciliations", 1),
        ("workflow_action_evidence", 0),
    ] {
        assert_eq!(state[table].as_array().unwrap().len(), count, "{table}");
    }
    assert_eq!(
        state["workflow_action_reconciliations"][0]["failure_code"],
        "workflow.cancelled"
    );
    assert_eq!(effects(f).await, 1, "the hung provider applied its effect");
    state
}

#[tokio::test]
async fn workflow_action_cancellation_applied_then_hung_history_reconciles_after_cancel() {
    let (f, request) = Box::pin(setup()).await;
    resources(&f).await;
    let ledger = ledger(&f, &request).await;
    let provider = Hung {
        inner: LedgerProvider::new(
            &f,
            Delivery::Apply {
                result: good(),
                recover: true,
                lose: true,
            },
        ),
        started: Notify::new(),
        dropped: AtomicUsize::new(0),
    };
    let token = CancellationToken::new();
    let service = ActionService::new(adapter(&f, 0));
    let dispatch = async {
        let result = service.dispatch_remote(&request, &provider, &token).await;
        (
            result.map(|observation| matches!(observation, RemoteDispatchObservation::Interrupted)),
            provider.dropped.load(Ordering::SeqCst),
        )
    };
    let durable = async {
        provider.started.notified().await;
        assert_eq!(provider.dropped.load(Ordering::SeqCst), 0, "still pending");
        let head = f
            .persistence()
            .head(request.scope().company, request.scope().run)
            .await
            .unwrap()
            .unwrap();
        let cancelled = f
            .persistence()
            .cancel(CancelCommand {
                company_id: request.scope().company,
                run_id: request.scope().run,
                actor: f.binding.target.actor,
                command_key: IdempotencyKey::parse("b3-hung-cancel").unwrap(),
                expected_revision: head.revision,
            })
            .await
            .unwrap();
        assert!(
            matches!(cancelled, CancelResult::Applied { .. }),
            "{cancelled:?}"
        );
    };
    let ((interrupted, dropped), ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(dispatch, durable)
    })
    .await
    .expect("bounded hung invocation");
    assert!(interrupted.unwrap(), "the hung invocation ends Interrupted");
    assert_eq!(
        dropped, 1,
        "the actual future is dropped before dispatch returns"
    );
    assert!(!token.is_cancelled(), "durable, not token, interruption");
    assert_eq!(provider.inner.calls.load(Ordering::SeqCst), 1);
    let before = cancelled_hung_history(&f).await;

    let verifier = ObservedVerifier::new(ledger);
    let command = verified_command(&f, &request, &verifier, "b3-hung-applied-truth").await;
    let result = Box::pin(execute(&f, &command, Arc::new(verifier)))
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: true }
    );
    let after = all_tables(&f).await;
    linked_result(&after, &command, &result);
    for table in [
        "background_tasks",
        "workflow_executions",
        "task_attempts",
        "workflow_budget_receipts",
        "workflow_root_budget_usage",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
        "fixture_provider_effects",
    ] {
        assert_eq!(
            before[table], after[table],
            "recovery is audit-only: {table}"
        );
    }
    let run = &after["workflow_runs"][0];
    assert_eq!(
        run["state"], "cancelled",
        "recovered truth does not reopen the run"
    );
    assert_eq!(run["revision"], result.revision.0);
    let evidence = &after["workflow_action_evidence"][0];
    assert_eq!(evidence["disposition"], "applied");
    let receipts = after["workflow_action_receipts"].as_array().unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["result"], good());
    assert_eq!(receipts[0]["reconciliation_evidence_id"], evidence["id"]);
    assert!(receipts[0]["remote_entry_id"].is_null());
    assert_eq!(effects(&f).await, 1);
    assert!(
        f.persistence()
            .settled()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(all_tables(&f).await, after, "terminal claim is inert");
    Box::pin(terminal_dispatch_inert(&f, &request, &after)).await;
}
