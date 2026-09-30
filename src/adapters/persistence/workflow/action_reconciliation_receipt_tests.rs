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
            diagnostic: json!({"version":1,"ledger":"fixture"}),
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
    register_ledger(f, request).await
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
            .release_io(
                request.fence,
                policy(),
                LeaseReleaseCause::LocalInterruption
            )
            .await
            .unwrap()
    );
}
async fn new_claim(f: &AdmissionFixture, request: &mut ActionDispatchRequest) {
    let (a, b) = tokio::join!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy()),
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
    );
    let claims = [a.unwrap(), b.unwrap()];
    assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
    request.fence = claims.into_iter().flatten().next().unwrap().fence;
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
                    .complete_io(FencedWorkflowResult {
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
                    .complete_io(FencedWorkflowResult {
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
                .complete_io(FencedWorkflowResult {
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
