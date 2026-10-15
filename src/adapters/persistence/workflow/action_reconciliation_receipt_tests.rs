//! Actual receipt arrival order must not hide authoritative finality breaches.
use super::action_reconciliation_snapshot_tests::{Resources, command};
use super::*;
use crate::adapters::persistence::workflow::action_reconciliation::PostgresActionReconciliation;
use crate::application::workflow::LifecycleAuthorizer;
use crate::application::workflow::binding::ResourceDirectory;
use crate::application::workflow::completion::WorkflowCompletion;
use std::sync::Arc;

struct Directory(PostgresPersistence);
#[async_trait]
impl ResourceDirectory for Directory {
    async fn inspect(
        &self,
        company: CompanyId,
        _: WorkflowActor,
        id: RuntimeResourceId,
    ) -> AppResult<Option<ResourceStatus>> {
        let authorized =
            sqlx::query_scalar("SELECT enabled FROM fixture_action_resources WHERE company_id=$1")
                .bind(company.as_uuid())
                .fetch_one(self.0.pool())
                .await?;
        Ok(Some(ResourceStatus {
            company_id: company,
            id,
            kind: TypeName::parse("fixture").unwrap(),
            authorized,
            readiness: ResourceReadiness::Ready,
            supported_contracts: [TypeName::parse("fixture.write").unwrap()].into(),
        }))
    }
}

/// Approved test-host verifier observes a durable provider ledger. Final closure
/// applies to these exact requests; future newly authorized entries remain possible.
struct LedgerVerifier {
    persistence: PostgresPersistence,
    registration: EvidenceVerifierRegistration,
}
#[async_trait]
impl ActionEvidenceVerifier for LedgerVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        &self.registration
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        if cancellation.is_cancelled() {
            return Err(AppError::Conflict("cancelled fixture verification".into()));
        }
        let mut tx = self.persistence.pool().begin().await?;
        let marker_closed: bool = sqlx::query_scalar("SELECT marker_closed FROM fixture_provider_operations WHERE invocation_id=$1 FOR UPDATE")
            .bind(snapshot.subject.invocation.as_uuid()).fetch_one(&mut *tx).await?;
        let now: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await?;
        let rows: Vec<LedgerRow> = sqlx::query_as("SELECT entry_id,final_closed,result,recover_result,observed_at FROM fixture_evidence_ledger WHERE company_id=$1 AND invocation_id=$2 ORDER BY entry_id LIMIT 129")
            .bind(snapshot.action.scope().company.as_uuid()).bind(snapshot.subject.invocation.as_uuid())
            .fetch_all(&mut *tx).await?;
        if rows.len() > 128 {
            return Err(invalid());
        }
        let disposition = if let Some(applied) = rows.iter().find(|row| row.result.is_some()) {
            VerifiedDisposition::Applied {
                recovered_result: applied
                    .recover_result
                    .then(|| applied.result.clone())
                    .flatten(),
                request: AppliedEvidenceRequest::RemoteEntry(ActionRemoteEntryId::new(
                    applied.entry_id,
                )),
            }
        } else if (marker_closed || !snapshot.entries.is_empty())
            && snapshot.entries.iter().all(|entry| {
                rows.iter()
                    .any(|row| row.entry_id == entry.id.as_uuid() && row.final_closed)
            })
        {
            VerifiedDisposition::FinalNotApplied
        } else {
            VerifiedDisposition::Unknown
        };
        tx.commit().await?;
        Ok(EvidenceAttestation {
            disposition,
            authoritative_reference: reference.clone(),
            observed_at: now,
            valid_until: now + chrono::Duration::minutes(10),
            diagnostic: AttestationDiagnostic::new(EvidenceProviderId::parse("fixture").unwrap()),
        })
    }
}
#[derive(sqlx::FromRow)]
struct LedgerRow {
    entry_id: Uuid,
    final_closed: bool,
    result: Option<Value>,
    recover_result: bool,
}

async fn ledger(f: &AdmissionFixture, request: &ActionDispatchRequest) -> Arc<LedgerVerifier> {
    sqlx::raw_sql("CREATE TABLE fixture_evidence_ledger(company_id uuid NOT NULL,invocation_id uuid NOT NULL,entry_id uuid PRIMARY KEY,final_closed bool NOT NULL,result jsonb,recover_result bool NOT NULL,observed_at timestamptz NOT NULL DEFAULT clock_timestamp())")
        .execute(f.persistence().pool()).await.unwrap();
    sqlx::raw_sql("CREATE TABLE fixture_provider_operations(invocation_id uuid PRIMARY KEY,provider_key text NOT NULL,marker_closed bool NOT NULL DEFAULT false); CREATE TABLE fixture_provider_effects(entry_id uuid PRIMARY KEY,result jsonb NOT NULL)")
        .execute(f.persistence().pool()).await.unwrap();
    let verifier = register_ledger(f, request).await;
    // Every caller dispatches next on the setup claim; heartbeat it as its worker would, so
    // the dispatch window is measured from here rather than from the claim. Creating the
    // ledger tables already changes every snapshot, so no before/after pair straddles this.
    renew_lease(f, request.fence, request.lease).await;
    verifier
}
async fn register_ledger(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
) -> Arc<LedgerVerifier> {
    let action = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    sqlx::query(
        "INSERT INTO fixture_provider_operations(invocation_id,provider_key) VALUES($1,$2)",
    )
    .bind(request.subject.invocation.as_uuid())
    .bind(action.idempotency_key().as_str())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    Arc::new(LedgerVerifier {
        persistence: f.persistence().clone(),
        registration: EvidenceVerifierRegistration::approve(
            EvidenceVerifierId::parse("fixture.ledger").unwrap(),
            EvidenceVerifierVersion::parse("v1").unwrap(),
            EvidenceProviderId::parse("fixture").unwrap(),
            &action.request().contract,
            &action.request().target,
        )
        .unwrap(),
    })
}

#[path = "action_reconciliation_audit_tests.rs"]
mod audit_tests;
#[path = "action_reconciliation_provider_tests.rs"]
mod provider_tests;
async fn ledger_row(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    entry: Uuid,
    result: Option<Value>,
) {
    sqlx::query("INSERT INTO fixture_evidence_ledger(company_id,invocation_id,entry_id,final_closed,result,recover_result) VALUES($1,$2,$3,true,$4,true)")
        .bind(request.scope().company.as_uuid())
        .bind(request.subject.invocation.as_uuid())
        .bind(entry)
        .bind(result)
        .execute(f.persistence().pool())
        .await
        .unwrap();
}
async fn reconcile(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    marker: Uuid,
    verifier: Arc<LedgerVerifier>,
    key: &str,
) -> ReconciliationResult {
    let mut command = command(f, request, marker).await;
    command.command_key = IdempotencyKey::parse(key).unwrap();
    command.input = EvidenceInput::VerifiedReference {
        registration: verifier.registration.id().clone(),
        reference: EvidenceRecordReference::parse(key).unwrap(),
    };
    let persistence = f.persistence().clone();
    let service = ActionReconciliationService::new(
        PostgresActionReconciliation::new(persistence.clone(), Resources),
        LifecycleAuthorizer::new(
            persistence.clone(),
            persistence.clone(),
            persistence.clone(),
        ),
        Directory(persistence),
        Some(verifier),
    );
    service
        .reconcile(&command, &CancellationToken::new(), Duration::from_secs(5))
        .await
        .unwrap()
}
async fn park(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    assert!(
        f.persistence()
            .settled()
            .release_io(
                request.fence,
                policy(),
                LeaseReleaseCause::LocalInterruption
            )
            .await
            .unwrap()
    );
}
/// The exact owner's DB-clock windows at one labelled phase boundary of a lease-sensitive
/// scenario. Read-only diagnostics: logged past libtest capture, never a result by itself.
#[derive(Debug)]
struct LeasePhase {
    owner: bool,
    lease_ms: f64,
    run_ms: f64,
}
async fn lease_phase(
    f: &AdmissionFixture,
    fence: WorkflowFence,
    scenario: &str,
    phase: &str,
    started: std::time::Instant,
) -> LeasePhase {
    let elapsed = started.elapsed();
    let (owner, lease_ms, run_ms, now): (bool, f64, f64, String) = sqlx::query_as("SELECT job.status='processing' AND job.worker_id=$2 AND job.execution_generation=$3 AND job.retry_count+1=$4, COALESCE(extract(epoch FROM job.lock_expires_at-clock_timestamp())*1000,0)::float8, (extract(epoch FROM run.deadline-clock_timestamp())*1000)::float8, clock_timestamp()::text FROM background_tasks AS job JOIN workflow_runs AS run ON run.company_id=job.company_id AND run.id=$5 WHERE job.id=$1")
        .bind(fence.scope.job.0).bind(fence.worker.0).bind(fence.generation.0)
        .bind(fence.attempt.0).bind(fence.scope.run.as_uuid())
        .fetch_one(f.persistence().pool()).await.unwrap();
    uncaptured(&format!(
        "PHASE scenario={scenario} phase={phase} elapsed_ms={} owner={owner} attempt={} lease_remaining_ms={lease_ms:.0} run_remaining_ms={run_ms:.0} db_now={now}",
        elapsed.as_millis(),
        fence.attempt.0
    ));
    LeasePhase {
        owner,
        lease_ms,
        run_ms,
    }
}
/// A worker's genuine heartbeat at a quiescent phase boundary of a lease-sensitive scenario
/// (DEC-010): the production `renew_io`, with the common three-second policy, before the old
/// window ends. A lost lease fails here as a `LEASE LAPSE` naming its phase; a refusal while
/// the exact owner is still live is a different, hard failure. Snapshots taken under the old
/// window are rebased by exactly the one field a renewal writes, this job's
/// `lock_expires_at`, read back as committed; every other row and column must still match
/// wherever they are compared.
async fn renew_phase(
    f: &AdmissionFixture,
    fence: WorkflowFence,
    scenario: &str,
    phase: &str,
    started: std::time::Instant,
    snapshots: &mut [&mut Value],
) {
    let lapse = |state: &LeasePhase| !state.owner || state.lease_ms <= 0.0 || state.run_ms <= 0.0;
    let label = format!("{phase}:renew");
    let current = lease_phase(f, fence, scenario, &label, started).await;
    assert!(
        !lapse(&current),
        "LEASE LAPSE scenario={scenario} phase={phase} before renewal: {current:?}"
    );
    let revision = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT revision FROM workflow_runs WHERE company_id=$1 AND id=$2",
        )
        .bind(fence.scope.company.as_uuid())
        .bind(fence.scope.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
    };
    let before = revision().await;
    let renewed = f.persistence().renew_io(fence, policy()).await;
    let after = lease_phase(f, fence, scenario, &format!("{phase}:renewed"), started).await;
    match renewed {
        Ok(Some(_)) => {}
        // A definite NotApplied keeps the old window, which must still be live.
        Err(AppError::DatabaseTimeout(_)) if !lapse(&after) => {}
        Ok(None) | Err(AppError::DatabaseTimeout(_)) if lapse(&after) => {
            panic!("LEASE LAPSE scenario={scenario} phase={phase} at renewal: {after:?}")
        }
        other => panic!(
            "renewal refused while the exact owner is live, scenario={scenario} phase={phase}: {other:?} {after:?}"
        ),
    }
    assert_eq!(
        revision().await,
        before,
        "a heartbeat moved the run revision"
    );
    let expiry: Value = sqlx::query_scalar(
        "SELECT to_jsonb(job)->'lock_expires_at' FROM background_tasks AS job WHERE id=$1",
    )
    .bind(fence.scope.job.0)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let job = json!(fence.scope.job.0);
    for snapshot in snapshots.iter_mut() {
        let mut rows = snapshot["background_tasks"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|row| row["id"] == job);
        let row = rows.next().expect("the renewed job is in the snapshot");
        assert!(rows.next().is_none());
        row["lock_expires_at"] = expiry.clone();
    }
}
/// What a real worker does while it owns the step: renew the fixture lease through the
/// production renew path, at points that no before/after snapshot pair straddles (the
/// pattern approved for the retained-truth scenarios). A renewal moves only
/// lock_expires_at, which the run-revision trigger ignores, so no saved command's expected
/// revision changes; that is asserted here. A definite NotApplied keeps the old window,
/// which must then still be live by the DB clock.
async fn renew_fixture_lease(f: &AdmissionFixture, fence: WorkflowFence) {
    renew_lease(f, fence, policy()).await;
}
async fn renew_lease(f: &AdmissionFixture, fence: WorkflowFence, lease: LeasePolicy) {
    let revision = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT revision FROM workflow_runs WHERE company_id=$1 AND id=$2",
        )
        .bind(fence.scope.company.as_uuid())
        .bind(fence.scope.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
    };
    let before = revision().await;
    match f.persistence().renew_io(fence, lease).await {
        Ok(window) => assert!(
            window.is_some(),
            "the fixture lease lapsed between heartbeats"
        ),
        Err(AppError::DatabaseTimeout(_)) => {
            let live: bool = sqlx::query_scalar(
                "SELECT lock_expires_at > clock_timestamp() FROM background_tasks WHERE id=$1",
            )
            .bind(fence.scope.job.0)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
            assert!(live, "the fixture lease lapsed between heartbeats");
        }
        Err(error) => panic!("heartbeat: {error:?}"),
    }
    assert_eq!(
        revision().await,
        before,
        "a heartbeat moved the run revision"
    );
}
async fn new_claim(f: &AdmissionFixture, request: &mut ActionDispatchRequest) {
    let workers = [worker(), worker()];
    let scope = request.fence.scope;
    // Under load a claimant can report a definite NotApplied (it wrote nothing): beside the
    // winner it lost, and with no claim at all the pair re-polls as workers would.
    let claims = f
        .persistence()
        .settled()
        .competing(
            |p, claimant| p.claim_io(scope, workers[claimant], policy()),
            Option::is_some,
        )
        .await;
    let winner = claims.iter().any(|claim| matches!(claim, Ok(Some(_))));
    let claims = claims.map(|claim| match claim {
        // A fail-closed unknown COMMIT (Q2(b)) is accepted only beside a winner: the claim is
        // exclusive, so the unknown claimant cannot hold the lease. That is asserted below
        // from the job and its attempts, not assumed.
        Err(AppError::Conflict(message))
            if message.contains("commit outcome unknown") && winner =>
        {
            uncaptured(
                "BACKSTOP unknown=lease.claim scenario=new_claim (winner and loser asserted below)",
            );
            None
        }
        claim => claim.unwrap(),
    });
    // Exactly one claims.
    assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
    request.fence = claims.into_iter().flatten().next().unwrap().fence;
    let (owner, generation): (Uuid, Uuid) =
        sqlx::query_as("SELECT worker_id,execution_generation FROM background_tasks WHERE id=$1")
            .bind(request.fence.scope.job.0)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(
        (owner, generation),
        (request.fence.worker.0, request.fence.generation.0),
        "the winner holds the lease"
    );
    let loser = workers
        .into_iter()
        .find(|claimant| *claimant != request.fence.worker)
        .unwrap();
    let loser_attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1 AND worker_id=$2")
            .bind(request.fence.scope.job.0)
            .bind(loser.0)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(loser_attempts, 0, "the losing claimant wrote no attempt");
}
async fn projection(f: &AdmissionFixture) -> (String, bool) {
    sqlx::query_as("SELECT effect_state,evidence_conflict FROM workflow_action_effect_states")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}
async fn counts(f: &AdmissionFixture) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM workflow_action_receipts),(SELECT count(*) FROM workflow_action_actual_receipt_observations),(SELECT count(*) FROM workflow_action_evidence_conflicts)")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_action_reconciliation_old_actual_after_new_receipt_keeps_same_and_different_truth()
 {
    for (result, completed) in [
        (json!({"items":[],"token_count":0}), false),
        (json!({"items":[],"token_count":1}), false),
        (json!({"items":[],"token_count":0}), true),
        (json!({"items":[],"token_count":1}), true),
    ] {
        let (f, mut request) = setup().await;
        let verifier = ledger(&f, &request).await;
        let writer = adapter(&f, 0);
        let RemoteReservationResult::Reserved(reserved) =
            writer.reserve_remote(&request, None).await.unwrap()
        else {
            panic!("marker")
        };
        let marker = reserved.marker;
        let old = writer.enter_remote(*reserved).await.unwrap();
        ledger_row(&f, &request, old.entry, None).await;
        park(&f, &request).await;
        assert!(matches!(
            reconcile(&f, &request, marker, verifier, "final-a")
                .await
                .outcome,
            ReconciliationOutcome::Scheduled {
                receipt_only: false
            }
        ));
        assert_eq!(projection(&f).await, ("not_applied".into(), false));
        new_claim(&f, &mut request).await;
        let RemoteReservationResult::Reserved(reserved) =
            writer.reserve_remote(&request, None).await.unwrap()
        else {
            panic!("proof marker")
        };
        let new = writer.enter_remote(*reserved).await.unwrap();
        assert_eq!(projection(&f).await, ("needs_reconciliation".into(), false));
        assert!(matches!(
            writer
                .finish_remote(new, json!({"items":[],"token_count":0}))
                .await
                .unwrap(),
            RemoteDispatchObservation::Committed(_)
        ));
        assert_eq!(counts(&f).await, (1, 1, 0));
        if completed {
            assert!(
                f.persistence()
                    .settled()
                    .complete_io(FencedWorkflowResult {
                        policy: crate::application::workflow::lease::LeasePolicy::new(
                            std::time::Duration::from_secs(3)
                        )
                        .unwrap(),
                        fence: request.fence,
                        output: json!({"items":[],"token_count":0})
                    })
                    .await
                    .unwrap()
                    .is_some()
            );
        }
        let history: Value = sqlx::query_scalar(
            "SELECT to_jsonb(execution) FROM workflow_executions AS execution WHERE id=$1",
        )
        .bind(request.scope().execution.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
        let old_id = old.entry;
        assert!(matches!(
            writer.finish_remote(old, result.clone()).await.unwrap(),
            RemoteDispatchObservation::Interrupted
        ));
        let facts = counts(&f).await;
        assert_eq!(facts.0, 1);
        assert_eq!(facts.1, 2);
        assert!(facts.2 >= 1);
        assert_eq!(projection(&f).await, ("committed".into(), true));
        sqlx::query("INSERT INTO workflow_action_actual_receipt_observations(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,result) SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,$1,$2 FROM workflow_action_receipts ON CONFLICT(company_id,remote_entry_id,result_digest) DO NOTHING")
            .bind(old_id).bind(result).execute(f.persistence().pool()).await.unwrap();
        assert_eq!(
            counts(&f).await,
            facts,
            "repeat actual delivery does not add observations/conflicts"
        );
        if completed {
            let after: Value = sqlx::query_scalar(
                "SELECT to_jsonb(execution) FROM workflow_executions AS execution WHERE id=$1",
            )
            .bind(request.scope().execution.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
            assert_eq!(
                after, history,
                "late conflict preserves completed output/route/successor"
            );
            assert!(
                f.persistence()
                    .settled()
                    .complete_io(FencedWorkflowResult {
                        policy: crate::application::workflow::lease::LeasePolicy::new(
                            std::time::Duration::from_secs(3)
                        )
                        .unwrap(),
                        fence: request.fence,
                        output: json!({"items":[],"token_count":0})
                    })
                    .await
                    .unwrap()
                    .is_some()
            );
            continue;
        }
        assert!(
            f.persistence()
                .settled()
                .complete_io(FencedWorkflowResult {
                    policy: crate::application::workflow::lease::LeasePolicy::new(
                        std::time::Duration::from_secs(3)
                    )
                    .unwrap(),
                    fence: request.fence,
                    output: json!({"written":true})
                })
                .await
                .unwrap()
                .is_none()
        );
        let code: String = sqlx::query_scalar(
            "SELECT workflow_failure_code FROM task_attempts ORDER BY attempt_number DESC LIMIT 1",
        )
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
        assert_eq!(code, "action.evidence_conflict");
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_old_actual_after_recovered_receipt_retains_first_result() {
    let (f, request) = setup().await;
    let verifier = ledger(&f, &request).await;
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reserved) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("marker")
    };
    let marker = reserved.marker;
    let old = writer.enter_remote(*reserved).await.unwrap();
    ledger_row(&f, &request, old.entry, Some(json!({"written":true}))).await;
    park(&f, &request).await;
    assert!(matches!(
        reconcile(&f, &request, marker, verifier, "applied-a")
            .await
            .outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    ));
    writer
        .finish_remote(old, json!({"written":false}))
        .await
        .unwrap();
    assert_eq!(counts(&f).await, (1, 1, 1));
    let receipt: Value = sqlx::query_scalar("SELECT result FROM workflow_action_receipts")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(receipt, json!({"written":true}));
    assert_eq!(projection(&f).await, ("committed".into(), true));
}

async fn sql_observation(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    entry: &RemoteEntry,
    result: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO workflow_action_actual_receipt_observations(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,result) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid()).bind(request.scope().execution.as_uuid())
        .bind(request.subject.invocation.as_uuid()).bind(request.subject.argument_digest.as_str()).bind(entry.marker).bind(entry.entry).bind(result)
        .execute(&mut **tx).await.map(|_| ())
}
#[tokio::test]
async fn workflow_action_reconciliation_sql_actual_receipt_conflict_and_audit_guards() {
    let (f, request) = setup().await;
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reserved) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("marker")
    };
    let entry = writer.enter_remote(*reserved).await.unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sql_observation(&mut tx, &request, &entry, json!({"written":false}))
        .await
        .unwrap();
    let error = tx.commit().await.unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .message()
            .contains("requires canonical receipt")
    );
    assert_eq!(counts(&f).await, (0, 0, 0));
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sql_observation(&mut tx, &request, &entry, json!({"written":false}))
        .await
        .unwrap();
    sqlx::query("INSERT INTO workflow_action_receipts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,remote_entry_id,result) VALUES($1,$2,$3,$4,$5,$6,'remote',$7,$8)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid()).bind(request.scope().execution.as_uuid())
        .bind(request.subject.invocation.as_uuid()).bind(request.subject.argument_digest.as_str()).bind(entry.marker).bind(entry.entry)
        .bind(json!({"written":true})).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(counts(&f).await, (1, 2, 1));
    let error=sqlx::query("INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,actual_observation_id,reason) SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,NULL,remote_entry_id,id,'finality_breach' FROM workflow_action_actual_receipt_observations LIMIT 1")
        .execute(f.persistence().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_action_conflict_nullable_evidence_shape")
    );
    for sql in [
        "UPDATE workflow_action_actual_receipt_observations SET result=result",
        "DELETE FROM workflow_action_actual_receipt_observations",
    ] {
        assert!(
            sqlx::query(sql)
                .execute(f.persistence().pool())
                .await
                .unwrap_err()
                .as_database_error()
                .unwrap()
                .message()
                .contains("append-only")
        );
    }
}
