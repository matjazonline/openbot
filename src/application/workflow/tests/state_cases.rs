use super::*;
use crate::domain::workflow::WaitingReason;

async fn admit_run(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
    key: &str,
) -> RunId {
    let AdmissionResult::Created(run_id) = service(store)
        .admit(request(company_id, version_id, key, json!(1)))
        .await
        .unwrap()
    else {
        panic!("expected created")
    };
    run_id
}

fn claim_request() -> ClaimRequest {
    ClaimRequest {
        worker_id: WorkerId::new(Uuid::new_v4()),
        batch: ClaimBatch::new(1).unwrap(),
        lease: LeaseDuration::new(std::time::Duration::from_secs(30)).unwrap(),
    }
}

#[tokio::test]
async fn replay_preserves_running_and_terminal_state() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let run_id = admit_run(&store, company_id, version_id, "replay").await;
    assert_eq!(
        store.head(company_id, run_id).await.unwrap().unwrap().state,
        RunState::Queued
    );
    assert_eq!(store.claim_ready(claim_request()).await.unwrap().len(), 1);
    assert_eq!(
        store.head(company_id, run_id).await.unwrap().unwrap().state,
        RunState::Running
    );
    {
        let mut state = store.state.lock().unwrap();
        state.jobs[0].claimed_until = Some(SystemTime::UNIX_EPOCH);
    }
    assert_eq!(store.claim_ready(claim_request()).await.unwrap().len(), 1);
    assert_eq!(
        store
            .head(company_id, run_id)
            .await
            .unwrap()
            .unwrap()
            .revision,
        RunRevision(2)
    );
    assert_eq!(
        service(&store)
            .admit(request(company_id, version_id, "replay", json!(1)))
            .await
            .unwrap(),
        AdmissionResult::Replayed(run_id)
    );
    assert_eq!(
        store.head(company_id, run_id).await.unwrap().unwrap().state,
        RunState::Running
    );
    {
        let mut state = store.state.lock().unwrap();
        state.runs.get_mut(&(company_id, run_id)).unwrap().state = RunState::Succeeded;
    }
    assert_eq!(
        service(&store)
            .admit(request(company_id, version_id, "replay", json!(1)))
            .await
            .unwrap(),
        AdmissionResult::Replayed(run_id)
    );
    assert_eq!(
        store.head(company_id, run_id).await.unwrap().unwrap().state,
        RunState::Succeeded
    );
}

#[tokio::test]
async fn cancellation_covers_nonterminal_and_preserves_all_terminal_states() {
    for (index, origin) in [
        RunState::Queued,
        RunState::Running,
        RunState::Waiting(WaitingReason::Decision),
    ]
    .into_iter()
    .enumerate()
    {
        let company_id = company();
        let version_id = version();
        let store = MemoryStore::new(owned(company_id, version_id, 128));
        let run_id = admit_run(
            &store,
            company_id,
            version_id,
            &format!("nonterminal-{index}"),
        )
        .await;
        {
            let mut state = store.state.lock().unwrap();
            state.runs.get_mut(&(company_id, run_id)).unwrap().state = origin;
        }
        if matches!(origin, RunState::Waiting(_)) {
            assert!(store.claim_ready(claim_request()).await.unwrap().is_empty());
        }
        assert!(matches!(
            service(&store)
                .cancel(CancelWorkflowRequest {
                    company_id,
                    actor: actor(),
                    run_id
                })
                .await
                .unwrap(),
            CancelResult::Applied { .. }
        ));
        assert_eq!(
            store.head(company_id, run_id).await.unwrap().unwrap().state,
            RunState::Cancelled
        );
        assert!(store.state.lock().unwrap().jobs[0].invalidated);
        assert!(store.claim_ready(claim_request()).await.unwrap().is_empty());
    }
    for (index, origin) in [RunState::Succeeded, RunState::Failed, RunState::Cancelled]
        .into_iter()
        .enumerate()
    {
        let company_id = company();
        let version_id = version();
        let store = MemoryStore::new(owned(company_id, version_id, 128));
        let run_id = admit_run(&store, company_id, version_id, &format!("terminal-{index}")).await;
        let before = {
            let mut state = store.state.lock().unwrap();
            let run = state.runs.get_mut(&(company_id, run_id)).unwrap();
            run.state = origin;
            run.revision
        };
        assert_eq!(
            service(&store)
                .cancel(CancelWorkflowRequest {
                    company_id,
                    actor: actor(),
                    run_id
                })
                .await
                .unwrap(),
            CancelResult::AlreadyTerminalOrApplied { revision: before }
        );
        let head = store.head(company_id, run_id).await.unwrap().unwrap();
        assert_eq!((head.state, head.revision), (origin, before));
        assert!(store.claim_ready(claim_request()).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn claim_and_cancel_serialize_without_reviving_cancelled_work() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let run_id = admit_run(&store, company_id, version_id, "claim-cancel").await;
    let store = store.with_claim_barrier();
    let barrier = store.claim_barrier.as_ref().unwrap().clone();
    let svc = service(&store);
    let (claim, cancel) = tokio::join!(store.claim_ready(claim_request()), async {
        barrier.wait().await;
        svc.cancel(CancelWorkflowRequest {
            company_id,
            actor: actor(),
            run_id,
        })
        .await
    });
    assert!(claim.unwrap().len() <= 1);
    match cancel.unwrap() {
        CancelResult::Applied { .. } => {}
        CancelResult::RevisionConflict { .. } => {
            assert!(matches!(
                svc.cancel(CancelWorkflowRequest {
                    company_id,
                    actor: actor(),
                    run_id
                })
                .await
                .unwrap(),
                CancelResult::Applied { .. }
            ));
        }
        other => panic!("unexpected race result: {other:?}"),
    }
    {
        let state = store.state.lock().unwrap();
        assert_eq!(state.runs[&(company_id, run_id)].state, RunState::Cancelled);
        assert!(state.jobs[0].invalidated);
        assert!(state.jobs[0].claimed_until.is_none());
    }
    let store = MemoryStore {
        claim_barrier: None,
        ..store
    };
    assert!(store.claim_ready(claim_request()).await.unwrap().is_empty());
}

#[tokio::test]
async fn claim_first_then_cancel_revokes_established_ownership() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let run_id = admit_run(&store, company_id, version_id, "claim-first").await;
    let initial = store.head(company_id, run_id).await.unwrap().unwrap();
    assert_eq!(
        (initial.state, initial.revision),
        (RunState::Queued, RunRevision(1))
    );

    let claims = store.claim_ready(claim_request()).await.unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].run_id(), run_id);
    let claimed = store.head(company_id, run_id).await.unwrap().unwrap();
    assert_eq!(
        (claimed.state, claimed.revision),
        (RunState::Running, RunRevision(2))
    );
    assert!(store.state.lock().unwrap().jobs[0].claimed_until.is_some());

    assert_eq!(
        store
            .cancel(CancelCommand {
                company_id,
                run_id,
                expected_revision: initial.revision
            })
            .await
            .unwrap(),
        CancelResult::RevisionConflict {
            current_revision: claimed.revision
        }
    );
    assert!(store.state.lock().unwrap().jobs[0].claimed_until.is_some());
    assert_eq!(
        service(&store)
            .cancel(CancelWorkflowRequest {
                company_id,
                actor: actor(),
                run_id
            })
            .await
            .unwrap(),
        CancelResult::Applied {
            revision: RunRevision(3)
        }
    );
    {
        let state = store.state.lock().unwrap();
        assert_eq!(
            (
                state.runs[&(company_id, run_id)].state,
                state.runs[&(company_id, run_id)].revision
            ),
            (RunState::Cancelled, RunRevision(3))
        );
        assert!(state.jobs[0].invalidated);
        assert!(state.jobs[0].claimed_until.is_none());
    }
    assert!(store.claim_ready(claim_request()).await.unwrap().is_empty());
}

#[tokio::test]
async fn cancel_first_prevents_any_claim() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let run_id = admit_run(&store, company_id, version_id, "cancel-first").await;
    assert_eq!(
        service(&store)
            .cancel(CancelWorkflowRequest {
                company_id,
                actor: actor(),
                run_id
            })
            .await
            .unwrap(),
        CancelResult::Applied {
            revision: RunRevision(2)
        }
    );
    assert!(store.claim_ready(claim_request()).await.unwrap().is_empty());
    let head = store.head(company_id, run_id).await.unwrap().unwrap();
    assert_eq!(
        (head.state, head.revision),
        (RunState::Cancelled, RunRevision(2))
    );
    let state = store.state.lock().unwrap();
    assert!(state.jobs[0].invalidated);
    assert!(state.jobs[0].claimed_until.is_none());
}
