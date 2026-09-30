//! C: committed adversarial topology, authentic owner-created scheduling histories,
//! and the real public run-update trigger. Only the final attack is rollback-only.
use super::*;

#[path = "action_reconciliation_claim_sql_candidate_support.rs"]
mod support;
use support::*;

async fn prepare_second(f: &AdmissionFixture, first: &ActionDispatchRequest) -> ActivationRequest {
    let before = all_tables(f).await;
    let scope = ActivationRequest {
        execution: ExecutionId::new(Uuid::new_v4()),
        job: WorkflowJobId(Uuid::new_v4()),
        ..first.fence.scope
    };
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation) SELECT company_id,run_id,$2,step_id,2 FROM workflow_executions WHERE id=$1")
        .bind(first.scope().execution.as_uuid()).bind(scope.execution.as_uuid())
        .execute(&mut *tx).await.expect("P1 fresh execution must be legal");
    sqlx::query("INSERT INTO background_tasks(id,company_id,channel_id,thread_id,correlation_id,task_type,payload,queue_kind,workflow_execution_id) SELECT $2,company_id,channel_id,thread_id,correlation_id,'workflow_execution',$3,'workflow',$4 FROM background_tasks WHERE id=$1")
        .bind(first.fence.scope.job.0).bind(scope.job.0)
        .bind(crate::application::workflow::activation::job_payload(scope.execution))
        .bind(scope.execution.as_uuid()).execute(&mut *tx).await.expect("P1 associated job must be legal");
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *tx).await.unwrap();
    tx.commit()
        .await
        .expect("P1 must commit with all constraints enabled");
    let after = all_tables(f).await;
    assert_preparation(&before, &after, Preparation::AddExecution(scope));
    scope
}

async fn second_action(
    f: &AdmissionFixture,
    first: &ActionDispatchRequest,
    scope: ActivationRequest,
) -> ActionDispatchRequest {
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .expect("fresh adversarial execution must be claimed by the real owner");
    let mut action = f
        .persistence()
        .action_authority(first.scope(), &first.subject)
        .await
        .unwrap()
        .action
        .request()
        .clone();
    action.scope.execution = scope.execution;
    // The fixture authority uses execution-scoped resources. A fresh action needs
    // its own target and verifier registration, not E1's frozen target identity.
    action.target = crate::application::workflow::actions::tests::request(action.scope).target;
    let intent = ActionService::new(f.persistence().clone())
        .prepare_step(action)
        .await
        .unwrap();
    let request = ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: claim.fence,
        lease: policy(),
    };
    let frozen = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    sqlx::query(
        "INSERT INTO fixture_provider_operations(invocation_id,provider_key) VALUES($1,$2)",
    )
    .bind(request.subject.invocation.as_uuid())
    .bind(frozen.idempotency_key().as_str())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    request
}

async fn retire(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let provider = LedgerProvider::new(f, Delivery::Pending);
    let observed = invoke(f, request, &provider).await;
    assert!(
        matches!(&observed, Err(AppError::Timeout(_))),
        "fixture provider must report uncertainty for {:?}; error={:?}, calls={}",
        request.fence.scope,
        observed.err(),
        provider.calls.load(Ordering::SeqCst)
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    park(f, request).await;
    barrier(f, request).await;
}

async fn schedule_candidate(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    verifier: &Arc<LedgerVerifier>,
    key: &str,
) -> ReconcileActionCommand {
    let mut c = proof_command(f, request, verifier, key).await;
    c.marker = ActionRemoteMarkerId::new(scoped_marker(f, request).await);
    let result = run_command(f, &c, verifier.clone())
        .await
        .expect("complete scheduling owner commits");
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    assert_eq!(
        result.revision,
        f.persistence()
            .head(c.scope.company, c.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision
    );
    let valid: bool = sqlx::query_scalar("SELECT count(*)=1 AND bool_and(command.result_revision=owner.revision AND witness.transaction_id=state.transaction_id AND state.initial_state='waiting' AND state.initial_waiting_reason='reconciliation') FROM workflow_action_claim_episodes AS episode JOIN workflow_action_evidence_commands AS command ON command.company_id=episode.company_id AND command.command_key=episode.command_key JOIN workflow_runs AS owner ON owner.company_id=episode.company_id AND owner.id=episode.run_id JOIN workflow_action_schedule_witnesses AS witness ON witness.company_id=episode.company_id AND witness.run_id=episode.run_id AND witness.execution_id=episode.execution_id AND witness.job_id=episode.job_id AND witness.retired_attempt=episode.retired_attempt JOIN workflow_action_state_witnesses AS state ON state.company_id=witness.company_id AND state.run_id=witness.run_id AND state.transaction_id=witness.transaction_id WHERE episode.company_id=$1 AND episode.command_key=$2")
        .bind(c.scope.company.as_uuid()).bind(c.command_key.as_str()).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(
        valid,
        "real command, final revision, episode and paired witnesses committed"
    );
    c
}

async fn hide_first(f: &AdmissionFixture, first: &ActionDispatchRequest) {
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(
        sqlx::query(
            "UPDATE background_tasks SET status='stopped' WHERE id=$1 AND status='pending'"
        )
        .bind(first.fence.scope.job.0)
        .execute(&mut *tx)
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    waiting_transition(&mut tx, first.fence.scope)
        .await
        .unwrap();
    tx.commit()
        .await
        .expect("P2 must commit separately from both schedules");
    assert_preparation(
        &before,
        &all_tables(f).await,
        Preparation::HideJob(first.fence.scope),
    );
}

async fn attack(
    f: &AdmissionFixture,
    first: &ActionDispatchRequest,
    second: &ActionDispatchRequest,
    commands: &[ReconcileActionCommand; 2],
) {
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(
        candidate_keys(&mut tx, first.fence.scope).await,
        vec![commands[1].command_key.as_str().to_owned()]
    );
    assert_eq!(
        sqlx::query(
            "UPDATE background_tasks SET status='pending' WHERE id=$1 AND status='stopped'"
        )
        .bind(first.fence.scope.job.0)
        .execute(&mut *tx)
        .await
        .expect("attack restore preparation")
        .rows_affected(),
        1
    );
    assert_eq!(
        candidate_keys(&mut tx, first.fence.scope).await,
        commands
            .iter()
            .map(|c| c.command_key.as_str().to_owned())
            .collect::<Vec<_>>()
    );
    for (request, command) in [first, second].into_iter().zip(commands) {
        let scope = request.fence.scope;
        let key: Option<String> =
            sqlx::query_scalar("SELECT public.workflow_action_pending_claim_episode($1,$2,$3,$4)")
                .bind(scope.company.as_uuid())
                .bind(scope.run.as_uuid())
                .bind(scope.execution.as_uuid())
                .bind(scope.job.0)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(
            key.as_deref(),
            Some(command.command_key.as_str()),
            "each exact execution/job is independently applicable"
        );
        let mut job: Value =
            sqlx::query_scalar("SELECT to_jsonb(job) FROM background_tasks AS job WHERE id=$1")
                .bind(scope.job.0)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        let saved = before["background_tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == json!(scope.job.0))
            .unwrap();
        job["status"] = saved["status"].clone();
        assert_eq!(
            &job, saved,
            "restore preserves exact counters, lease fields and job history"
        );
    }
    assert_run_prerequisites(&mut tx, first.fence.scope).await;
    let error = waiting_transition(&mut tx, first.fence.scope)
        .await
        .expect_err("two independently applicable episodes must fail actual public capture");
    guard(
        &error,
        "multiple current workflow reconciliation claim episodes",
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(f).await,
        "complete target attack rollback including revisions and all protected history"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_multiple_applicable_candidates() {
    // Box extended real owner/provider seams to retain stock 2 MiB test stacks.
    let (f, first) = Box::pin(limited()).await;
    let verifier = ledger(&f, &first).await;
    let catalog = capture_catalog(&f).await;
    Box::pin(retire(&f, &first)).await;
    let second_scope = prepare_second(&f, &first).await;
    let second = Box::pin(second_action(&f, &first, second_scope)).await;
    let second_verifier = verifier_for(&f, &second).await;
    Box::pin(retire(&f, &second)).await;
    assert_distinct_histories(&f, &first, &second).await;
    let first_command = Box::pin(schedule_candidate(&f, &first, &verifier, "candidate-1")).await;
    assert_eq!(job_status(&f, second_scope).await, "failed");
    hide_first(&f, &first).await;
    let second_command = Box::pin(schedule_candidate(
        &f,
        &second,
        &second_verifier,
        "candidate-2",
    ))
    .await;
    assert_eq!(job_status(&f, first.fence.scope).await, "stopped");
    single_transition(&f, second_scope, &second_command).await;
    attack(&f, &first, &second, &[first_command, second_command]).await;
    assert_eq!(catalog, capture_catalog(&f).await);
    f.persistence().pool().close().await;
    drop(verifier);
    drop(second_verifier);
    drop(f);
    Box::pin(normal_control()).await;
}
