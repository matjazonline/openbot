//! Public SQL reservation history attacks with genuine service-owned final proof.
use super::*;

struct ProofOwner {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    provider: RegisteredLedger,
    proof: Uuid,
}

enum IdentityAttack {
    HistoricalRetrofit,
    HistoricalSpoof,
}

impl IdentityAttack {
    fn statement(&self) -> &'static str {
        match self {
            Self::HistoricalRetrofit => {
                "INSERT INTO workflow_action_evidence_consumptions(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id) SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,$3,id FROM workflow_action_remote_entries WHERE company_id=$1 AND id=$2"
            }
            Self::HistoricalSpoof => {
                "INSERT INTO workflow_action_evidence_consumptions(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,reservation_xid) SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,$3,id,$4::text::xid8 FROM workflow_action_remote_entries WHERE company_id=$1 AND id=$2"
            }
        }
    }
}

#[derive(Debug, PartialEq, sqlx::Type)]
#[sqlx(transparent)]
struct ReservationIdentity(String);

struct NativeFunction {
    name: &'static str,
    migration: &'static str,
}

async fn proof_owner() -> ProofOwner {
    let (f, mut request) =
        Box::pin(setup_action(ActionRecovery::SafeRepeat, json!({"value":1}))).await;
    let verifier = ledger(&f, &request).await;
    let provider = RegisteredLedger::new(&f, &request, Delivery::Pending).await;
    assert!(
        Box::pin(registered_invoke(&f, &request, &provider))
            .await
            .is_err()
    );
    assert_eq!(provider.inner.calls.load(Ordering::SeqCst), 1);
    park(&f, &request).await;
    barrier(&f, &request).await;
    let command = proof_command(&f, &request, &verifier, "consumption-identity-final").await;
    let result = Box::pin(run_command(&f, &command, verifier)).await.unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::NotAppliedRecorded);
    assert_ordinary_source(&f, result.evidence.unwrap().as_uuid()).await;
    wait_due(&f, &request).await;
    new_claim(&f, &mut request).await;
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert_eq!(effects(&f).await, 0);
    assert_live_source(&f, &request).await;
    ProofOwner {
        fixture: f,
        request,
        provider,
        proof: result.evidence.unwrap().as_uuid(),
    }
}

async fn assert_ordinary_source(f: &AdmissionFixture, proof: Uuid) {
    let saved = all_tables(f).await;
    assert_eq!(saved["workflow_runs"][0]["state"], "running");
    assert_eq!(saved["workflow_runs"][0]["waiting_reason"], Value::Null);
    assert_eq!(saved["background_tasks"][0]["status"], "pending");
    for field in ["worker_id", "execution_generation", "lock_expires_at"] {
        assert_eq!(saved["background_tasks"][0][field], Value::Null);
    }
    assert_eq!(saved["task_attempts"][0]["status"], "failed");
    assert_eq!(saved["task_attempts"][0]["workflow_retry_safety"], "safe");
    assert_eq!(saved["task_attempts"][0]["workflow_retirement"], "live");
    assert!(!saved["task_attempts"][0]["finished_at"].is_null());
    assert_eq!(saved["workflow_action_claim_episodes"], json!([]));
    assert_eq!(saved["workflow_action_evidence"][0]["id"], json!(proof));
    assert_eq!(saved["workflow_action_evidence"][0]["grant_eligible"], true);
    assert_eq!(
        saved["workflow_action_evidence_coverage"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

async fn wait_due(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let due: bool = sqlx::query_scalar("SELECT run_at<=clock_timestamp() FROM background_tasks WHERE company_id=$1 AND id=$2")
                .bind(request.scope().company.as_uuid()).bind(request.fence.scope.job.0)
                .fetch_one(f.persistence().pool()).await.unwrap();
            if due { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("native two-second retry backoff becomes due");
}

async fn assert_live_source(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_runs AS owner JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id JOIN task_attempts AS attempt ON attempt.task_id=job.id WHERE owner.company_id=$1 AND owner.id=$2 AND owner.state='running' AND owner.deadline>clock_timestamp() AND execution.id=$3 AND execution.completed_at IS NULL AND job.id=$4 AND job.queue_kind='workflow' AND job.status='processing' AND job.worker_id=$5 AND job.execution_generation=$6 AND job.retry_count+1=$7 AND job.lock_expires_at>clock_timestamp() AND attempt.attempt_number=$7 AND attempt.worker_id=$5 AND attempt.execution_generation=$6 AND attempt.status='processing') AND workflow_action_replay_supported($1,$8)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid()).bind(request.fence.scope.job.0)
        .bind(request.fence.worker.0).bind(request.fence.generation.0).bind(request.fence.attempt.0)
        .bind(request.subject.invocation.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(valid, "genuine live fence and approved replay support");
}

fn native_body(definition: &NativeFunction) -> &'static str {
    let start = definition
        .migration
        .find(&format!("FUNCTION {}(", definition.name))
        .unwrap();
    definition.migration[start..]
        .split_once("$$")
        .unwrap()
        .1
        .split_once("$$")
        .unwrap()
        .0
}

async fn native_catalog(f: &AdmissionFixture) -> Value {
    let binding =
        include_str!("../../../../migrations/20260930181000_workflow_action_evidence_binding.sql");
    let bounds =
        include_str!("../../../../migrations/20261001090000_workflow_action_proof_bounds.sql");
    let evidence =
        include_str!("../../../../migrations/20260930180000_workflow_action_evidence.sql");
    for definition in [
        NativeFunction {
            name: "workflow_action_reservation_identity",
            migration: binding,
        },
        NativeFunction {
            name: "workflow_action_consumption_commit_guard",
            migration: binding,
        },
        NativeFunction {
            name: "workflow_action_not_applied_available",
            migration: bounds,
        },
        NativeFunction {
            name: "workflow_action_remote_entry_commit_guard",
            migration: evidence,
        },
    ] {
        let body: String = sqlx::query_scalar(
            "SELECT prosrc FROM pg_proc WHERE pronamespace='public'::regnamespace AND proname=$1",
        )
        .bind(definition.name)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
        assert_eq!(body, native_body(&definition), "native {}", definition.name);
    }
    let catalog: Value = sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_object('name',tgname,'relation',tgrelid::regclass::text,'enabled',tgenabled,'type',tgtype,'deferred',tgdeferrable,'initially_deferred',tginitdeferred,'function',tgfoid::regproc::text,'definition',pg_get_triggerdef(oid)) ORDER BY tgname) FROM pg_trigger WHERE NOT tgisinternal AND tgname=ANY($1)")
        .bind(vec!["workflow_action_consumption_commit_guard", "workflow_action_consumption_reservation_identity", "workflow_action_entry_reservation_identity", "workflow_action_remote_entry_commit_guard"])
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(catalog.as_array().unwrap().len(), 4);
    for trigger in catalog.as_array().unwrap() {
        assert_eq!(trigger["enabled"], "O");
        let deferred = trigger["name"].as_str().unwrap().ends_with("commit_guard");
        assert_eq!(trigger["type"], if deferred { 5 } else { 7 });
        assert_eq!(trigger["deferred"], deferred);
        assert_eq!(trigger["initially_deferred"], deferred);
        assert_eq!(
            trigger["relation"],
            if trigger["name"].as_str().unwrap().contains("consumption") {
                "workflow_action_evidence_consumptions"
            } else {
                "workflow_action_remote_entries"
            }
        );
        assert_eq!(
            trigger["function"],
            if deferred {
                trigger["name"].as_str().unwrap()
            } else {
                "workflow_action_reservation_identity"
            }
        );
    }
    catalog
}

async fn historical_entry(owner: &ProofOwner) -> Uuid {
    let f = &owner.fixture;
    let request = &owner.request;
    let before = all_tables(f).await;
    let marker = scoped_marker(f, request).await;
    let entry = Uuid::new_v4();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let xid: ReservationIdentity = sqlx::query_scalar("SELECT pg_current_xact_id()::text")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(sqlx::query("INSERT INTO workflow_action_remote_entries(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,job_id,attempt_number,execution_generation,worker_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
        .bind(request.subject.argument_digest.as_str()).bind(marker).bind(entry)
        .bind(request.fence.scope.job.0).bind(request.fence.attempt.0)
        .bind(request.fence.generation.0).bind(request.fence.worker.0).execute(&mut *tx).await.unwrap().rows_affected(), 1);
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let actual: ReservationIdentity = sqlx::query_scalar("SELECT reservation_xid::text FROM workflow_action_remote_entries WHERE company_id=$1 AND id=$2")
        .bind(request.scope().company.as_uuid()).bind(entry).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(actual, xid, "native entry identity, actually committed");
    assert_eq!(entry_consumptions(f).await, (2, 0));
    assert_live_source(f, request).await;
    let after = all_tables(f).await;
    assert_reservation_delta(&before, &after, 0);
    entry
}

fn assert_reservation_delta(before: &Value, after: &Value, consumptions: usize) {
    assert_eq!(
        before.as_object().unwrap().keys().collect::<Vec<_>>(),
        after.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    for (relation, rows) in before.as_object().unwrap() {
        let added = match relation.as_str() {
            "workflow_action_remote_entries" => 1,
            "workflow_action_evidence_consumptions" => consumptions,
            _ => 0,
        };
        if added == 0 {
            assert_eq!(rows, &after[relation], "reservation preserves {relation}");
        } else {
            let current = after[relation].as_array().unwrap();
            assert_eq!(current.len(), rows.as_array().unwrap().len() + added);
            for row in rows.as_array().unwrap() {
                assert!(current.contains(row), "retained {relation}");
            }
        }
    }
}

fn assert_native(error: &sqlx::Error, attack: &IdentityAttack) {
    let native = error
        .as_database_error()
        .expect("native PostgreSQL rejection")
        .downcast_ref::<sqlx::postgres::PgDatabaseError>();
    let message = match attack {
        IdentityAttack::HistoricalRetrofit => "workflow action final proof cannot be consumed",
        IdentityAttack::HistoricalSpoof => "invalid workflow action reservation transaction",
    };
    let function = match attack {
        IdentityAttack::HistoricalRetrofit => "workflow_action_consumption_commit_guard()",
        IdentityAttack::HistoricalSpoof => "workflow_action_reservation_identity()",
    };
    assert_eq!(native.code(), "23514");
    assert_eq!(native.message(), message);
    assert!(native.schema().is_none());
    assert!(native.constraint().is_none());
    assert!(native.table().is_none());
    assert!(native.r#where().unwrap().contains(function));
    eprintln!(
        "consumption_identity_native function={function} code={} message={message}",
        native.code()
    );
}

async fn reject_identity(attack: IdentityAttack) {
    let owner = Box::pin(proof_owner()).await;
    let f = &owner.fixture;
    let request = &owner.request;
    let catalog = native_catalog(f).await;
    let entry = historical_entry(&owner).await;
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let current: ReservationIdentity = sqlx::query_scalar("SELECT pg_current_xact_id()::text")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let prior: ReservationIdentity = sqlx::query_scalar("SELECT reservation_xid::text FROM workflow_action_remote_entries WHERE company_id=$1 AND id=$2")
        .bind(request.scope().company.as_uuid()).bind(entry).fetch_one(&mut *tx).await.unwrap();
    assert_ne!(current, prior);
    let query = sqlx::query(attack.statement())
        .bind(request.scope().company.as_uuid())
        .bind(entry)
        .bind(owner.proof);
    let query = match attack {
        IdentityAttack::HistoricalRetrofit => query,
        IdentityAttack::HistoricalSpoof => query.bind(&prior.0),
    };
    let inserted = query.execute(&mut *tx).await;
    let error = match attack {
        IdentityAttack::HistoricalRetrofit => {
            assert_eq!(inserted.unwrap().rows_affected(), 1);
            let actual: ReservationIdentity = sqlx::query_scalar("SELECT reservation_xid::text FROM workflow_action_evidence_consumptions WHERE company_id=$1 AND evidence_id=$2")
                .bind(request.scope().company.as_uuid()).bind(owner.proof).fetch_one(&mut *tx).await.unwrap();
            assert_eq!(actual, current);
            let available: Option<Uuid> =
                sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2,$3)")
                    .bind(request.scope().company.as_uuid())
                    .bind(request.subject.invocation.as_uuid())
                    .bind(entry)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
            assert_eq!(
                available,
                Some(owner.proof),
                "every nonattacked consumption predicate passes"
            );
            sqlx::query("SET CONSTRAINTS public.workflow_action_consumption_commit_guard IMMEDIATE")
                .execute(&mut *tx)
                .await
                .unwrap_err()
        }
        IdentityAttack::HistoricalSpoof => inserted.unwrap_err(),
    };
    assert_native(&error, &attack);
    tx.rollback().await.unwrap();
    assert_eq!(
        all_tables(f).await,
        before,
        "exact rollback of every public relation"
    );
    assert_eq!(native_catalog(f).await, catalog);
    assert_eq!(effects(f).await, 0);
    assert_eq!(owner.provider.inner.calls.load(Ordering::SeqCst), 1);
    f.persistence().pool().close().await;
}

async fn positive_reservation() {
    let owner = Box::pin(proof_owner()).await;
    let f = &owner.fixture;
    let catalog = native_catalog(f).await;
    let before = all_tables(f).await;
    let writer = adapter(f, 0);
    let RemoteReservationResult::Reserved(reserved) = writer
        .reserve_remote(&owner.request, Some(&owner.provider.contract))
        .await
        .unwrap()
    else {
        panic!("genuine same-transaction reserve");
    };
    assert_eq!(entry_consumptions(f).await, (2, 1));
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions AS consumption JOIN workflow_action_remote_entries AS entry ON (entry.company_id,entry.run_id,entry.execution_id,entry.invocation_id,entry.argument_digest,entry.dispatch_id,entry.id)=(consumption.company_id,consumption.run_id,consumption.execution_id,consumption.invocation_id,consumption.argument_digest,consumption.dispatch_id,consumption.remote_entry_id) WHERE consumption.company_id=$1 AND consumption.evidence_id=$2 AND consumption.remote_entry_id=$3 AND entry.reservation_xid=consumption.reservation_xid AND entry.reservation_xid IS NOT NULL)")
        .bind(owner.request.scope().company.as_uuid()).bind(owner.proof).bind(reserved.entry).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(valid, "same native transaction identity and complete scope");
    let committed = all_tables(f).await;
    assert_reservation_delta(&before, &committed, 1);
    writer.enter_remote(*reserved).await.unwrap();
    assert_eq!(
        all_tables(f).await,
        committed,
        "enter creates no second reservation or consumption"
    );
    assert_eq!(owner.provider.inner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(f).await, 0);
    assert_eq!(native_catalog(f).await, catalog);
    f.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_consumption_historical_retrofit() {
    Box::pin(reject_identity(IdentityAttack::HistoricalRetrofit)).await;
    Box::pin(positive_reservation()).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_consumption_historical_xid_spoof() {
    Box::pin(reject_identity(IdentityAttack::HistoricalSpoof)).await;
}
