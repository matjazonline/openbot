//! Untrusted stored commands and installed verifier sources at the real service boundary.
use super::*;

#[path = "action_reconciliation_native_bounds_tests.rs"]
mod native_bounds_tests;

struct SourceVerifier {
    ledger: Arc<LedgerVerifier>,
    registration: EvidenceVerifierRegistration,
    observed_at: Option<chrono::DateTime<chrono::Utc>>,
    calls: AtomicUsize,
}

impl SourceVerifier {
    fn new(ledger: Arc<LedgerVerifier>, registration: EvidenceVerifierRegistration) -> Self {
        Self {
            ledger,
            registration,
            observed_at: None,
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ActionEvidenceVerifier for SourceVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        &self.registration
    }

    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut attested = self
            .ledger
            .verify(snapshot, reference, cancellation)
            .await?;
        if let Some(observed_at) = self.observed_at {
            attested.observed_at = observed_at;
        }
        Ok(attested)
    }
}

fn invalid_evidence(result: AppResult<ReconciliationResult>) {
    let Err(AppError::BadRequest(message)) = result else {
        panic!("expected invalid-evidence BadRequest");
    };
    assert_eq!(message, "Invalid workflow reconciliation evidence");
}

fn invalid_stored_outcome(result: AppResult<ReconciliationResult>) {
    let Err(AppError::Database(message)) = result else {
        panic!("expected invalid stored-record Database error");
    };
    assert_eq!(message, "Invalid stored workflow record");
}

async fn insert_outcome_on(
    db: &mut sqlx::PgConnection,
    command: &ReconcileActionCommand,
    outcome: &Value,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO workflow_action_evidence_commands \
         (company_id,command_key,id,run_id,execution_id,invocation_id,argument_digest, \
          dispatch_id,actor_id,request_digest,expected_revision,result_revision,outcome) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$11,$12)",
    )
    .bind(command.scope.company.as_uuid())
    .bind(command.command_key.as_str())
    .bind(Uuid::new_v4())
    .bind(command.scope.run.as_uuid())
    .bind(command.scope.execution.as_uuid())
    .bind(command.subject.invocation.as_uuid())
    .bind(command.subject.argument_digest.as_str())
    .bind(command.marker.as_uuid())
    .bind(command.actor.user_id())
    .bind(command.request_digest().unwrap().as_str())
    .bind(i64::try_from(command.expected_revision.0).unwrap())
    .bind(outcome)
    .execute(db)
    .await
}

async fn store_outcome(f: &AdmissionFixture, command: &ReconcileActionCommand, outcome: Value) {
    // A new audit-less row models untrusted persisted input. No accepted receipt
    // is updated, and the real scope/digest/positive revision pass native guards.
    let mut db = f.persistence().pool().acquire().await.unwrap();
    insert_outcome_on(&mut db, command, &outcome).await.unwrap();
    drop(db);
    let stored: Value = sqlx::query_scalar(
        "SELECT outcome FROM workflow_action_evidence_commands \
         WHERE company_id=$1 AND command_key=$2",
    )
    .bind(command.scope.company.as_uuid())
    .bind(command.command_key.as_str())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(
        stored, outcome,
        "candidate actually committed through native guards"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_validation_malformed_stored_outcomes() {
    let (f, request, observed) = Box::pin(prepared()).await;
    let malformed = [
        json!({}),
        json!({"kind":"unknown-variant"}),
        json!({"kind":[]}),
        json!({"kind":"blocked"}),
        json!({"kind":"blocked","reason":{}}),
        json!({"kind":"applied_recorded","receipt":"yes"}),
        json!({"kind":"applied_recorded","receipt":null}),
    ];
    for (number, outcome) in malformed.into_iter().enumerate() {
        let command = verified_command(&f, &request, &observed, &format!("decode-{number}")).await;
        store_outcome(&f, &command, outcome).await;
        let before = all_tables(&f).await;
        invalid_stored_outcome(run_command(&f, &command, observed.clone()).await);
        assert_eq!(observed.calls.load(Ordering::SeqCst), 0);
        assert_eq!(all_tables(&f).await, before, "malformed replay {number}");
    }

    let command = verified_command(&f, &request, &observed, "decode-valid-refusal").await;
    store_outcome(&f, &command, json!({"kind":"revision_conflict"})).await;
    let before = all_tables(&f).await;
    let replay = run_command(&f, &command, observed.clone()).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.outcome, ReconciliationOutcome::RevisionConflict);
    assert_eq!(replay.revision, command.expected_revision);
    assert_eq!(replay.evidence, None);
    assert_eq!(observed.calls.load(Ordering::SeqCst), 0);
    assert_eq!(all_tables(&f).await, before);
}

async fn different_target_registration(
    f: &AdmissionFixture,
    original: &FrozenAction,
) -> EvidenceVerifierRegistration {
    let mut action = original.request().clone();
    action.target = ActionTarget::Local {
        resource: Uuid::new_v4(),
        resource_kind: TypeName::parse("fixture").unwrap(),
    };
    let intent = ActionService::new(f.persistence().clone())
        .prepare_step(action)
        .await
        .unwrap();
    let frozen = f
        .persistence()
        .action_authority(original.scope(), &intent.approval_subject())
        .await
        .unwrap()
        .action;
    EvidenceVerifierRegistration::approve(
        EvidenceVerifierId::parse("fixture.ledger").unwrap(),
        EvidenceVerifierVersion::parse("v1").unwrap(),
        EvidenceProviderId::parse("fixture").unwrap(),
        &frozen.request().contract,
        &frozen.request().target,
    )
    .unwrap()
}

fn pending_source_result(
    result: AppResult<RemoteDispatchObservation>,
    started: std::time::Instant,
) {
    match result {
        Err(AppError::Timeout(message)) => {
            assert_eq!(message, "provider request remains active");
        }
        Err(error) => panic!("Pending source error at {:?}: {error:?}", started.elapsed()),
        Ok(result) => {
            let variant = match result {
                RemoteDispatchObservation::Committed(_) => "Committed",
                RemoteDispatchObservation::ApprovalRequired => "ApprovalRequired",
                RemoteDispatchObservation::PossibleDispatchExists => "PossibleDispatchExists",
                RemoteDispatchObservation::Interrupted => "Interrupted",
            };
            panic!(
                "Pending source returned {variant} at {:?}",
                started.elapsed()
            );
        }
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_validation_installed_source_mismatches() {
    let started = std::time::Instant::now();
    let (f, request) = Box::pin(setup()).await;
    let claimed_at = started.elapsed();
    resources(&f).await;
    let inner_ledger = ledger(&f, &request).await;
    let observed = Arc::new(ObservedVerifier::new(inner_ledger.clone()));
    let original = inner_ledger.registration();
    // Dispatch authority is intentionally live-only. Prepare both genuine
    // frozen owner actions before parking, then retain their immutable values.
    let action = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    let lease_remaining: f64 = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (lock_expires_at-clock_timestamp()))::float8 FROM background_tasks WHERE id=$1",
    )
    .bind(request.fence.scope.job.0)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    eprintln!(
        "source fixture claimed_at={claimed_at:?} before_invoke={:?} lease_remaining={lease_remaining}",
        started.elapsed()
    );
    let pending = LedgerProvider::new(&f, Delivery::Pending);
    pending_source_result(invoke(&f, &request, &pending).await, started);
    assert_eq!(pending.calls.load(Ordering::SeqCst), 1);
    // The provider fixture identifies the original intent by scope and argument digest.
    // Add the equal-digest target sibling only after its genuine Pending call.
    let other_target = different_target_registration(&f, &action).await;
    park(&f, &request).await;
    let wrong_id = EvidenceVerifierRegistration::approve(
        EvidenceVerifierId::parse("fixture.other-id").unwrap(),
        original.version().clone(),
        original.provider().clone(),
        &action.request().contract,
        &action.request().target,
    )
    .unwrap();
    assert_eq!(wrong_id.company(), original.company());
    assert_eq!(wrong_id.operation(), original.operation());
    assert_ne!(wrong_id.id(), original.id());

    // Registrations come from actual host-approved frozen owner actions.
    let (foreign, foreign_request) = Box::pin(setup()).await;
    let foreign_ledger = ledger(&foreign, &foreign_request).await;
    assert_ne!(foreign_ledger.registration().company(), original.company());
    assert_eq!(other_target.company(), original.company());
    assert_eq!(other_target.id(), original.id());
    assert_ne!(other_target.operation(), original.operation());
    for (number, registration) in [wrong_id, foreign_ledger.registration.clone(), other_target]
        .into_iter()
        .enumerate()
    {
        let command = verified_command(&f, &request, &observed, &format!("source-{number}")).await;
        let source = Arc::new(SourceVerifier::new(inner_ledger.clone(), registration));
        let before = all_tables(&f).await;
        invalid_evidence(run_command(&f, &command, source.clone()).await);
        assert_eq!(source.calls.load(Ordering::SeqCst), 0);
        assert_eq!(all_tables(&f).await, before, "source mismatch {number}");
    }
    let command = verified_command(&f, &request, &observed, "source-matching").await;
    let matching = Arc::new(SourceVerifier::new(inner_ledger.clone(), original.clone()));
    let result = run_command(&f, &command, matching.clone()).await.unwrap();
    assert_eq!(matching.calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.outcome, ReconciliationOutcome::UnknownRecorded);
    assert!(result.evidence.is_some());
}

#[derive(sqlx::FromRow)]
struct EntryTimes {
    marker_time: chrono::DateTime<chrono::Utc>,
    entry_time: chrono::DateTime<chrono::Utc>,
}

async fn entry_times(f: &AdmissionFixture, request: &ActionDispatchRequest) -> EntryTimes {
    sqlx::query_as(
        "SELECT marker.created_at AS marker_time,entry.created_at AS entry_time \
         FROM workflow_action_dispatches AS marker \
         JOIN workflow_action_remote_entries AS entry \
           ON entry.company_id=marker.company_id AND entry.dispatch_id=marker.id \
         WHERE marker.company_id=$1 AND marker.invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn workflow_action_reconciliation_validation_observation_before_real_entry() {
    let (f, request, observed) = Box::pin(prepared()).await;
    barrier(&f, &request).await;
    let EntryTimes {
        marker_time,
        entry_time,
    } = entry_times(&f, &request).await;
    assert!(marker_time < entry_time, "real reserve precedes real entry");
    let command = verified_command(&f, &request, &observed, "observed-before-entry").await;
    let mut source = SourceVerifier::new(observed.ledger.clone(), observed.registration().clone());
    source.observed_at = Some(marker_time);
    let source = Arc::new(source);
    let before = all_tables(&f).await;
    invalid_evidence(run_command(&f, &command, source.clone()).await);
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    assert_eq!(all_tables(&f).await, before);

    let command = verified_command(&f, &request, &observed, "observed-at-entry").await;
    let mut source = SourceVerifier::new(observed.ledger.clone(), observed.registration().clone());
    source.observed_at = Some(entry_time);
    let source = Arc::new(source);
    let result = run_command(&f, &command, source.clone()).await.unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let stored: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT observed_at FROM workflow_action_evidence WHERE company_id=$1 AND id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(result.evidence.unwrap().as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(stored, entry_time);
}
