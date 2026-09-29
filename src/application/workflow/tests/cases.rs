use super::*;

#[tokio::test]
async fn admission_derives_entry_and_commits_one_job_with_snapshots() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let result = service(&store)
        .admit(request(company_id, version_id, "message:1", json!({"n":1})))
        .await
        .unwrap();
    let AdmissionResult::Created(run_id) = result else {
        panic!("expected created")
    };
    let state = store.state.lock().unwrap();
    assert_eq!(state.runs.len(), 1);
    assert_eq!(state.jobs.len(), 1);
    assert_eq!(state.jobs[0].run_id(), run_id);
    assert_eq!(state.jobs[0].step_id(), &step_id("actual_entry"));
    assert_eq!(state.runs[&(company_id, run_id)].state, RunState::Queued);
    let saved = state.admissions.values().next().unwrap();
    assert_eq!(saved.input, json!({"n":1}));
    assert_eq!(saved.params, json!(2));
    assert_eq!(saved.version_id, version_id);
}

#[tokio::test]
async fn missing_failing_or_mismatched_definition_prevents_admission() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let svc = service(&store);
    assert!(matches!(
        svc.admit(request(company_id, version(), "k1", json!(1)))
            .await,
        Err(AppError::NotFound(_))
    ));
    store.state.lock().unwrap().fail_lookup = true;
    assert!(matches!(
        svc.admit(request(company_id, version_id, "k2", json!(1)))
            .await,
        Err(AppError::Database(_))
    ));
    store.state.lock().unwrap().fail_lookup = false;
    store.state.lock().unwrap().returned_version = Some(owned(company(), version_id, 128));
    assert!(matches!(
        svc.admit(request(company_id, version_id, "k3", json!(1)))
            .await,
        Err(AppError::Internal(_))
    ));
    store.state.lock().unwrap().returned_version = Some(owned(company_id, version(), 128));
    assert!(matches!(
        svc.admit(request(company_id, version_id, "k4", json!(1)))
            .await,
        Err(AppError::Internal(_))
    ));
    assert!(store.state.lock().unwrap().admissions.is_empty());
}

#[tokio::test]
async fn aggregate_snapshot_limit_and_write_failure() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(configured(company_id, version_id, 10, json!("abc")));
    let svc = service(&store);
    assert!(matches!(
        svc.admit(request(company_id, version_id, "over", json!("abcd")))
            .await,
        Err(AppError::BadRequest(_))
    ));
    let exact = svc
        .admit(request(company_id, version_id, "exact", json!("abc")))
        .await
        .unwrap();
    assert!(matches!(exact, AdmissionResult::Created(_)));
    store.state.lock().unwrap().fail_admit = true;
    assert!(matches!(
        svc.admit(request(company_id, version_id, "failed", json!(1)))
            .await,
        Err(AppError::Database(_))
    ));
    assert_eq!(store.state.lock().unwrap().jobs.len(), 1);
}

#[tokio::test]
async fn replay_conflict_and_company_scoped_key() {
    let company_a = company();
    let company_b = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_a, version_id, 128));
    store
        .state
        .lock()
        .unwrap()
        .versions
        .insert((company_b, version_id), owned(company_b, version_id, 128));
    store.state.lock().unwrap().access.insert(
        (company_b, actor().user_id()),
        PrincipalAccessContext {
            principal_id: None,
            membership: CompanyMembership::Owner,
        },
    );
    let svc = service(&store);
    let first = svc
        .admit(request(company_a, version_id, "same", json!(1)))
        .await
        .unwrap();
    let replay = svc
        .admit(request(company_a, version_id, "same", json!(1)))
        .await
        .unwrap();
    let conflict = svc
        .admit(request(company_a, version_id, "same", json!(3)))
        .await
        .unwrap();
    let another_version = version();
    store.state.lock().unwrap().versions.insert(
        (company_a, another_version),
        owned(company_a, another_version, 128),
    );
    let changed_definition = svc
        .admit(request(company_a, another_version, "same", json!(1)))
        .await
        .unwrap();
    let other = svc
        .admit(request(company_b, version_id, "same", json!(1)))
        .await
        .unwrap();
    let AdmissionResult::Created(original) = first else {
        panic!("expected created")
    };
    assert_eq!(replay, AdmissionResult::Replayed(original));
    assert_eq!(conflict, AdmissionResult::Conflict);
    assert_eq!(changed_definition, AdmissionResult::Conflict);
    assert!(matches!(other, AdmissionResult::Created(id) if id != original));
    assert_eq!(store.state.lock().unwrap().jobs.len(), 2);
}

#[tokio::test]
async fn competing_admissions_create_one_run_and_job() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128)).with_admission_barrier();
    let svc = service(&store);
    let (left, right) = tokio::join!(
        svc.admit(request(company_id, version_id, "race", json!(1))),
        svc.admit(request(company_id, version_id, "race", json!(1))),
    );
    let results = [left.unwrap(), right.unwrap()];
    assert!(
        matches!(results, [AdmissionResult::Created(id), AdmissionResult::Replayed(other)] | [AdmissionResult::Replayed(id), AdmissionResult::Created(other)] if id == other)
    );
    let state = store.state.lock().unwrap();
    assert_eq!(state.runs.len(), 1);
    assert_eq!(state.jobs.len(), 1);
}

#[tokio::test]
async fn competing_claimants_claim_once_and_bounds_reject() {
    assert!(IdempotencyKey::parse("").is_err());
    assert!(IdempotencyKey::parse("x".repeat(MAX_IDEMPOTENCY_KEY_BYTES + 1)).is_err());
    assert!(ClaimBatch::new(0).is_err());
    assert!(ClaimBatch::new(MAX_CLAIM_BATCH + 1).is_err());
    assert!(LeaseDuration::new(std::time::Duration::ZERO).is_err());
    assert!(LeaseDuration::new(std::time::Duration::from_secs(MAX_LEASE_SECONDS + 1)).is_err());
    assert!(ClaimBatch::new(MAX_CLAIM_BATCH).is_ok());
    assert!(LeaseDuration::new(std::time::Duration::from_secs(MAX_LEASE_SECONDS)).is_ok());
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    service(&store)
        .admit(request(company_id, version_id, "claim", json!(1)))
        .await
        .unwrap();
    let store = store.with_claim_barrier();
    let claim = |worker_id| ClaimRequest {
        worker_id,
        batch: ClaimBatch::new(1).unwrap(),
        lease: LeaseDuration::new(std::time::Duration::from_secs(30)).unwrap(),
    };
    let (left, right) = tokio::join!(
        store.claim_ready(claim(WorkerId::new(Uuid::new_v4()))),
        store.claim_ready(claim(WorkerId::new(Uuid::new_v4())))
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_eq!(left.len() + right.len(), 1);
    let owned = left.first().or_else(|| right.first()).unwrap();
    assert_eq!(owned.company_id(), company_id);
    assert_eq!(owned.step_id(), &step_id("actual_entry"));
    assert!(owned.lease_expires_at > SystemTime::now());
    let head = store
        .head(company_id, owned.run_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(head.state, RunState::Running);
    assert_eq!(head.revision, RunRevision(2));
}

#[tokio::test]
async fn cancellation_checks_scope_and_expected_revision() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let svc = service(&store);
    assert_eq!(
        svc.cancel(CancelWorkflowRequest {
            expected_revision: RunRevision(1),
            command_key: IdempotencyKey::parse("cancel").unwrap(),
            company_id,
            actor: actor(),
            run_id: RunId::new(Uuid::new_v4())
        })
        .await
        .unwrap(),
        CancelResult::NotFound
    );
    let AdmissionResult::Created(run_id) = svc
        .admit(request(company_id, version_id, "cancel", json!(1)))
        .await
        .unwrap()
    else {
        panic!("expected created")
    };
    store.state.lock().unwrap().returned_head = Some(RunHead {
        causality: test_causality(company(), run_id),
        workflow_id: WorkflowId::new(Uuid::nil()),
        version_id,
        association: RelatedAssociation::Company,
        state: RunState::Queued,
        revision: RunRevision(1),
    });
    assert!(matches!(
        svc.cancel(CancelWorkflowRequest {
            expected_revision: RunRevision(1),
            command_key: IdempotencyKey::parse("cancel").unwrap(),
            company_id,
            actor: actor(),
            run_id
        })
        .await,
        Err(AppError::Internal(_))
    ));
    store.state.lock().unwrap().returned_head = Some(RunHead {
        causality: test_causality(company_id, RunId::new(Uuid::new_v4())),
        workflow_id: WorkflowId::new(Uuid::nil()),
        version_id,
        association: RelatedAssociation::Company,
        state: RunState::Queued,
        revision: RunRevision(1),
    });
    assert!(matches!(
        svc.cancel(CancelWorkflowRequest {
            expected_revision: RunRevision(1),
            command_key: IdempotencyKey::parse("cancel").unwrap(),
            company_id,
            actor: actor(),
            run_id
        })
        .await,
        Err(AppError::Internal(_))
    ));
    store.state.lock().unwrap().returned_head = None;
    store
        .state
        .lock()
        .unwrap()
        .runs
        .get_mut(&(company_id, run_id))
        .unwrap()
        .revision = RunRevision(2);
    store.state.lock().unwrap().returned_head = Some(RunHead {
        causality: test_causality(company_id, run_id),
        workflow_id: WorkflowId::new(Uuid::nil()),
        version_id,
        association: RelatedAssociation::Company,
        state: RunState::Queued,
        revision: RunRevision(1),
    });
    assert_eq!(
        svc.cancel(CancelWorkflowRequest {
            expected_revision: RunRevision(1),
            command_key: IdempotencyKey::parse("cancel").unwrap(),
            company_id,
            actor: actor(),
            run_id
        })
        .await
        .unwrap(),
        CancelResult::RevisionConflict {
            current_revision: RunRevision(2)
        }
    );
}

#[tokio::test]
async fn cancellation_preserves_errors_and_invalidates_pending_job() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let svc = service(&store);
    let AdmissionResult::Created(run_id) = svc
        .admit(request(company_id, version_id, "cancel", json!(1)))
        .await
        .unwrap()
    else {
        panic!("expected created")
    };
    store.state.lock().unwrap().fail_head = true;
    assert!(matches!(
        svc.cancel(CancelWorkflowRequest {
            expected_revision: RunRevision(1),
            command_key: IdempotencyKey::parse("cancel").unwrap(),
            company_id,
            actor: actor(),
            run_id
        })
        .await,
        Err(AppError::Database(_))
    ));
    store.state.lock().unwrap().fail_head = false;
    store.state.lock().unwrap().fail_cancel = true;
    assert!(matches!(
        svc.cancel(CancelWorkflowRequest {
            expected_revision: RunRevision(1),
            command_key: IdempotencyKey::parse("cancel").unwrap(),
            company_id,
            actor: actor(),
            run_id
        })
        .await,
        Err(AppError::Database(_))
    ));
    store.state.lock().unwrap().fail_cancel = false;
    assert_eq!(
        svc.cancel(CancelWorkflowRequest {
            expected_revision: RunRevision(1),
            command_key: IdempotencyKey::parse("cancel").unwrap(),
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
    assert!(store.state.lock().unwrap().jobs[0].invalidated);
    assert_eq!(
        store.head(company_id, run_id).await.unwrap().unwrap().state,
        RunState::Cancelled
    );
    assert_eq!(
        svc.cancel(CancelWorkflowRequest {
            expected_revision: RunRevision(1),
            command_key: IdempotencyKey::parse("cancel").unwrap(),
            company_id,
            actor: actor(),
            run_id
        })
        .await
        .unwrap(),
        CancelResult::AlreadyTerminalOrApplied {
            revision: RunRevision(2)
        }
    );
}
