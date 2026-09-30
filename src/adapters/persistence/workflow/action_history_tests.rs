use super::*;
use sha2::{Digest, Sha256};

#[derive(Clone)]
struct HistoricalEvidence {
    policy: Value,
    subject: Option<Vec<u8>>,
}
async fn historical(f: &AdmissionFixture, request: &ActionDispatchRequest) -> HistoricalEvidence {
    let action = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    let contract = ProviderReplayContract::approve(
        TypeName::parse("_provider._v1").unwrap(),
        &action.request().contract,
        &action.request().target,
        ProviderReplayMode::ProviderIdempotency {
            retention: Duration::from_secs(31536000),
        },
    )
    .unwrap();
    let invocation = prepare_provider(&action, Some(&contract), Duration::from_secs(1)).unwrap();
    HistoricalEvidence {
        policy: json!({"tool":action.request().contract,"approval_required":false,
            "provider_replay":invocation.proof().unwrap()}),
        subject: Some(
            replay_subject(&action.request().contract, &action.request().target).unwrap(),
        ),
    }
}
async fn insert_history(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    evidence: HistoricalEvidence,
) -> Uuid {
    let marker = Uuid::new_v4();
    Dispatch::insert_subject(
        tx,
        request,
        marker,
        "remote",
        evidence.policy,
        evidence.subject,
    )
    .await
    .unwrap();
    marker
}
async fn predicate(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
) -> Option<bool> {
    sqlx::query_scalar("SELECT workflow_action_retry_safe($1,$2)")
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .unwrap()
}

fn corrupt(evidence: &mut HistoricalEvidence, case: &str) {
    let proof = &mut evidence.policy["provider_replay"];
    match case {
        "missing" => *proof = Value::Null,
        "malformed" => *proof = json!([]),
        "version" => proof["version"] = json!(2),
        "proof_extra" => proof["extra"] = json!(true),
        "operation" => proof["operation"] = json!("0".repeat(64)),
        "registration_digit" => proof["registration"] = json!("1bad"),
        "registration_empty_segment" => proof["registration"] = json!("a..b"),
        "registration_trailing" => proof["registration"] = json!("a."),
        "registration_129" => proof["registration"] = json!("a".repeat(129)),
        "registration_128" => proof["registration"] = json!("a".repeat(128)),
        "expired" => proof["guarantee"]["retention"]["secs"] = json!(1),
        "retention_zero" => proof["guarantee"]["retention"]["secs"] = json!(0),
        "retention_fraction" => proof["guarantee"]["retention"]["secs"] = json!(1.5),
        "retention_extra" => proof["guarantee"]["retention"]["other"] = json!(1),
        "nanos" => proof["guarantee"]["retention"]["nanos"] = json!(1000000000),
        "mode" => proof["guarantee"] = json!({"mode":"safe_repeat"}),
        "null_subject" => evidence.subject = None,
        "utf8" => evidence.subject = Some(vec![255]),
        "shape" => evidence.subject = Some(b"[]".to_vec()),
        "wrong_snapshot" => {
            let mut subject: Value =
                serde_json::from_slice(evidence.subject.as_ref().unwrap()).unwrap();
            subject["tool"]["contract"]["output_schema"] = json!(false);
            let bytes = serde_json::to_vec(&subject).unwrap();
            proof["operation"] = json!(format!("{:x}", Sha256::digest(&bytes)));
            evidence.subject = Some(bytes);
        }
        "changed_policy" => evidence.policy["tool"]["policy"]["policy_revision"] = json!(2),
        "protected" => evidence.policy["approval_required"] = json!(true),
        "valid" => {}
        _ => panic!("unknown case"),
    }
}

#[tokio::test]
async fn workflow_action_history_matrix_shared_predicate_and_actual_rust_retirement() {
    let (f, request) = setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
    let cases = [
        "valid",
        "registration_128",
        "missing",
        "malformed",
        "version",
        "proof_extra",
        "operation",
        "registration_digit",
        "registration_empty_segment",
        "registration_trailing",
        "registration_129",
        "expired",
        "retention_zero",
        "retention_fraction",
        "retention_extra",
        "nanos",
        "mode",
        "null_subject",
        "utf8",
        "shape",
        "wrong_snapshot",
        "changed_policy",
        "protected",
    ];
    for case in cases {
        let expected = matches!(case, "valid" | "registration_128");
        let mut evidence = historical(&f, &request).await;
        corrupt(&mut evidence, case);
        let mut tx = f.persistence().pool().begin().await.unwrap();
        insert_history(&mut tx, &request, evidence).await;
        assert_eq!(predicate(&mut tx, &request).await, Some(expected), "{case}");
        sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
            .bind(request.fence.scope.job.0).execute(&mut *tx).await.unwrap();
        recovery::retire_on(&mut tx, request.fence, lease::Retirement::Expired)
            .await
            .unwrap();
        let label: String = sqlx::query_scalar("SELECT workflow_retry_safety FROM task_attempts WHERE task_id=$1 AND attempt_number=$2")
            .bind(request.fence.scope.job.0).bind(request.fence.attempt.0).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(label, if expected { "safe" } else { "unknown" }, "{case}");
        tx.rollback().await.unwrap();
    }
}

async fn cloned_intent(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    operation: Value,
) -> ActionDispatchRequest {
    let mut cloned = request.clone();
    cloned.subject.invocation = ActionInvocationId::new(Uuid::new_v4());
    let digest = format!(
        "{:x}",
        Sha256::digest(cloned.subject.invocation.as_uuid().as_bytes())
    );
    sqlx::query("INSERT INTO workflow_action_intents (company_id,run_id,execution_id,id,operation_key,operation_digest,argument_digest,idempotency_key,operation,policy_decision) SELECT company_id,run_id,execution_id,$2,operation_key,$3,argument_digest,'workflow-action:v1:' || $3,$4,policy_decision FROM workflow_action_intents WHERE id=$1")
        .bind(request.subject.invocation.as_uuid()).bind(cloned.subject.invocation.as_uuid())
        .bind(digest).bind(operation).execute(&mut **tx).await.unwrap();
    cloned
}

#[tokio::test]
async fn workflow_action_history_numeric_subject_and_u64_revision_domain() {
    let (f, request) = setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
    let operation: Value =
        sqlx::query_scalar("SELECT operation FROM workflow_action_intents WHERE id=$1")
            .bind(request.subject.invocation.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    let revisions = [
        "1",
        "9223372036854775808",
        "18446744073709551615",
        "0",
        "-1",
        "1.5",
        "18446744073709551616",
        "\"1\"",
        "null",
    ];
    for revision in revisions {
        let mut operation = operation.clone();
        operation["contract"]["policy"]["policy_revision"] =
            serde_json::from_str(revision).unwrap();
        operation["contract"]["contract"]["output_schema"]["examples"] =
            serde_json::from_str("[-0.0,1e2,1.0,1e-10]").unwrap();
        let mut evidence = historical(&f, &request).await;
        let subject = json!({"tool":operation["contract"],"target":operation["target"]});
        // Use the same production canonical encoding, not JSONB text serialization.
        let bytes = if matches!(
            revision,
            "1" | "9223372036854775808" | "18446744073709551615"
        ) {
            replay_subject(
                &serde_json::from_value(operation["contract"].clone()).unwrap(),
                &serde_json::from_value(operation["target"].clone()).unwrap(),
            )
            .unwrap()
        } else {
            serde_json::to_vec(&subject).unwrap()
        };
        evidence.policy["tool"] = operation["contract"].clone();
        evidence.policy["provider_replay"]["operation"] =
            json!(format!("{:x}", Sha256::digest(&bytes)));
        evidence.subject = Some(bytes);
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let cloned = cloned_intent(&mut tx, &request, operation).await;
        insert_history(&mut tx, &cloned, evidence).await;
        assert_eq!(
            predicate(&mut tx, &request).await,
            Some(matches!(
                revision,
                "1" | "9223372036854775808" | "18446744073709551615"
            )),
            "{revision}"
        );
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn workflow_action_history_unsafe_sibling_and_129_fact_overflow() {
    let (f, request) = setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
    let operation: Value =
        sqlx::query_scalar("SELECT operation FROM workflow_action_intents WHERE id=$1")
            .bind(request.subject.invocation.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    // Owner FKs key-share-lock the run during insertion. Prepare authority
    // before the write transaction rather than asking another connection for its run lock.
    let evidence = historical(&f, &request).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(predicate(&mut tx, &request).await, None);
    insert_history(&mut tx, &request, evidence.clone()).await;
    let sibling = cloned_intent(&mut tx, &request, operation.clone()).await;
    let mut unsafe_evidence = evidence.clone();
    corrupt(&mut unsafe_evidence, "missing");
    insert_history(&mut tx, &sibling, unsafe_evidence).await;
    assert_eq!(predicate(&mut tx, &request).await, Some(false));
    tx.rollback().await.unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    for index in 0..129 {
        let cloned = cloned_intent(&mut tx, &request, operation.clone()).await;
        insert_history(&mut tx, &cloned, evidence.clone()).await;
        if index == 127 {
            assert_eq!(predicate(&mut tx, &request).await, Some(true));
        }
    }
    assert_eq!(predicate(&mut tx, &request).await, Some(false));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_action_history_direct_sql_forged_safe_retirement_rejected() {
    let (f, request) = setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
    let before = snapshot(&f).await;
    let mut evidence = historical(&f, &request).await;
    corrupt(&mut evidence, "missing");
    let mut tx = f.persistence().pool().begin().await.unwrap();
    insert_history(&mut tx, &request, evidence).await;
    sqlx::query("UPDATE task_attempts SET status='failed',finished_at=clock_timestamp(),workflow_failure_class='terminal',workflow_failure_code='provider.rejected',workflow_retry_safety='safe',workflow_retirement='live' WHERE task_id=$1")
        .bind(request.fence.scope.job.0).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE background_tasks SET status='failed',retry_count=retry_count+1,worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE id=$1")
        .bind(request.fence.scope.job.0).execute(&mut *tx).await.unwrap();
    let error = sqlx::query("SET CONSTRAINTS workflow_fenced_retirement_guard IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "workflow action outcome unknown at retirement"
    );
    tx.rollback().await.unwrap();
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(facts(&f).await, (0, 0, 0));
}

async fn reopen_sql_on(
    tx: &mut Transaction<'_, Postgres>,
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
) {
    let company = request.scope().company.as_uuid();
    let run = request.scope().run.as_uuid();
    let expected: i64 = sqlx::query_scalar("SELECT revision FROM workflow_runs WHERE id=$1")
        .bind(run)
        .fetch_one(&mut **tx)
        .await
        .unwrap();
    sqlx::query("UPDATE background_tasks SET status='pending',run_at=clock_timestamp()+interval '1 second' WHERE id=$1")
        .bind(request.fence.scope.job.0).execute(&mut **tx).await.unwrap();
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL,terminal_execution_id=NULL WHERE id=$1")
        .bind(run).execute(&mut **tx).await.unwrap();
    let sequence: i64 = sqlx::query_scalar("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id) SELECT $1,$2,COALESCE(MAX(sequence),0)+1,'control_retry',$3 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 RETURNING sequence")
        .bind(company).bind(run).bind(f.binding.target.actor.user_id()).fetch_one(&mut **tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_control_commands(company_id,command_key,run_id,actor_id,operation,expected_revision,result,result_revision,audit_sequence) SELECT $1,'retention-reopen',$2,$3,'retry',$4,'applied',revision,$5 FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(company).bind(run).bind(f.binding.target.actor.user_id()).bind(expected).bind(sequence).execute(&mut **tx).await.unwrap();
}

#[tokio::test]
async fn workflow_action_history_direct_sql_reopen_rechecks_current_retention() {
    use crate::application::workflow::lease::WorkflowFailure;
    use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
    let (f, request) = setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
    let mut evidence = historical(&f, &request).await;
    evidence.policy["provider_replay"]["guarantee"]["retention"]["secs"] = json!(60);
    sqlx::query(
        "UPDATE workflow_runs SET deadline=clock_timestamp()+interval '30 seconds' WHERE id=$1",
    )
    .bind(request.scope().run.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    insert_history(&mut tx, &request, evidence).await;
    tx.commit().await.unwrap();
    f.persistence()
        .release_io(
            request.fence,
            policy(),
            LeaseReleaseCause::Classified(WorkflowFailure {
                failure: StepFailure::new(
                    FailureClass::Terminal,
                    FailureCode::parse("provider.rejected").unwrap(),
                    None,
                )
                .unwrap(),
                safety: RetrySafety::SafeToRetry,
            }),
        )
        .await
        .unwrap();
    let initially_safe: bool = sqlx::query_scalar("SELECT workflow_control_retry_safe($1,$2,$3)")
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().run.as_uuid())
        .bind(request.fence.scope.job.0)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert!(initially_safe);
    let before = snapshot(&f).await;
    for expired in [false, true] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        if expired {
            // Only the horizon changes; immutable evidence and the historical safe label remain.
            sqlx::query(
                "UPDATE workflow_runs SET deadline=clock_timestamp()+interval '1 hour' WHERE id=$1",
            )
            .bind(request.scope().run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        assert_eq!(predicate(&mut tx, &request).await, Some(!expired));
        reopen_sql_on(&mut tx, &f, &request).await;
        let result = sqlx::query("SET CONSTRAINTS workflow_control_retry_guard IMMEDIATE")
            .execute(&mut *tx)
            .await;
        if expired {
            assert_eq!(
                result.unwrap_err().as_database_error().unwrap().message(),
                "invalid explicit workflow retry"
            );
        } else {
            result.unwrap();
        }
        tx.rollback().await.unwrap();
        assert_eq!(snapshot(&f).await, before);
    }
}

fn wrong_entry_request(request: &ActionDispatchRequest, case: usize) -> ActionDispatchRequest {
    let mut wrong = request.clone();
    match case {
        0 => wrong.fence.scope.company = CompanyId::new(Uuid::new_v4()),
        1 => wrong.fence.scope.run = RunId::new(Uuid::new_v4()),
        2 => wrong.fence.scope.execution = ExecutionId::new(Uuid::new_v4()),
        3 => wrong.subject.invocation = ActionInvocationId::new(Uuid::new_v4()),
        4 => wrong.subject.argument_digest = serde_json::from_value(json!("0".repeat(64))).unwrap(),
        5 => wrong.fence.scope.job = WorkflowJobId(Uuid::new_v4()),
        6 => wrong.fence.attempt = WorkflowAttempt(99),
        7 => wrong.fence.generation = WorkflowGeneration(Uuid::new_v4()),
        _ => wrong.fence.worker = worker(),
    }
    wrong
}

#[tokio::test]
async fn workflow_action_history_remote_entry_exact_provenance_and_deferred_fence() {
    let (f, request) = setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
    for case in 0..10 {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let marker = insert_history(&mut tx, &request, historical(&f, &request).await).await;
        let error = Dispatch::reserve_entry_on(
            &mut tx,
            &wrong_entry_request(&request, case),
            if case == 9 { Uuid::new_v4() } else { marker },
            Uuid::new_v4(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error,AppError::Database(message) if message.contains("foreign key")),
            "{case}"
        );
        tx.rollback().await.unwrap();
    }
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let marker = insert_history(&mut tx, &request, historical(&f, &request).await).await;
    assert!(
        Dispatch::reserve_entry_on(&mut tx, &request, marker, Uuid::new_v4())
            .await
            .unwrap()
    );
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(request.fence.scope.job.0).execute(&mut *tx).await.unwrap();
    let error = sqlx::query("SET CONSTRAINTS workflow_action_remote_entry_commit_guard IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "workflow action remote entry lost live fence"
    );
    tx.rollback().await.unwrap();
    assert_eq!(facts(&f).await, (0, 0, 0));
    let entries: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_remote_entries")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(entries, 0);
}

#[tokio::test]
async fn workflow_action_history_remote_entry_append_only_and_receipt_requires_entry() {
    let (f, request) = setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let marker = insert_history(&mut tx, &request, historical(&f, &request).await).await;
    let entry = Uuid::new_v4();
    assert!(
        Dispatch::reserve_entry_on(&mut tx, &request, marker, entry)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    for sql in [
        "UPDATE workflow_action_remote_entries SET worker_id=gen_random_uuid() WHERE id=$1",
        "DELETE FROM workflow_action_remote_entries WHERE id=$1",
    ] {
        let error = sqlx::query(sql)
            .bind(entry)
            .execute(f.persistence().pool())
            .await
            .unwrap_err();
        assert!(
            error
                .as_database_error()
                .unwrap()
                .message()
                .contains("append-only")
        );
    }
    for remote_entry in [None, Some(Uuid::new_v4())] {
        let error=sqlx::query("INSERT INTO workflow_action_receipts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,remote_entry_id,result) VALUES ($1,$2,$3,$4,$5,$6,'remote',$7,$8)")
            .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
            .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
            .bind(request.subject.argument_digest.as_str()).bind(marker).bind(remote_entry).bind(json!({"written":true}))
            .execute(f.persistence().pool()).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some(if remote_entry.is_none() {
                "23514"
            } else {
                "23503"
            })
        );
    }
    assert_eq!(facts(&f).await, (1, 0, 0));
}
