use super::*;

async fn observation(
    db: &mut PgConnection,
    request: &ActionDispatchRequest,
    marker: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO workflow_action_reconciliations(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,failure_code) VALUES ($1,$2,$3,$4,$5,$6,'remote','provider.rejected')")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
        .bind(request.subject.argument_digest.as_str()).bind(marker).execute(db).await?;
    Ok(())
}

fn sqlstate(error: &sqlx::Error, expected: &str) {
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some(expected),
        "{error}"
    );
}

#[tokio::test]
async fn workflow_action_uncertainty_sql_provenance_isolated_from_other_guards() {
    let (f, request) = setup().await;
    let RemoteReservationResult::Reserved(_) =
        adapter(&f, 0).reserve_remote(&request, None).await.unwrap()
    else {
        panic!("reserved")
    };
    let marker: Uuid = sqlx::query_scalar("SELECT id FROM workflow_action_dispatches")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    for mode in 0..6 {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("UPDATE workflow_runs SET state='cancelled'")
            .execute(&mut *tx)
            .await
            .unwrap();
        // Isolate the composite FK: nonexistent owners otherwise fail the BEFORE guard.
        // This DDL is rolled back with each negative and never changes production constraints.
        sqlx::query("ALTER TABLE workflow_action_reconciliations DISABLE TRIGGER workflow_action_reconciliation_guard")
            .execute(&mut *tx).await.unwrap();
        let mut wrong = request.clone();
        match mode {
            0 => wrong.fence.scope.company = CompanyId::new(Uuid::new_v4()),
            1 => wrong.fence.scope.run = RunId::new(Uuid::new_v4()),
            2 => wrong.fence.scope.execution = ExecutionId::new(Uuid::new_v4()),
            3 => wrong.subject.invocation = ActionInvocationId::new(Uuid::new_v4()),
            4 => {
                wrong.subject.argument_digest =
                    serde_json::from_value(json!("0".repeat(64))).unwrap()
            }
            _ => {}
        }
        sqlstate(
            &observation(
                &mut tx,
                &wrong,
                if mode == 5 { Uuid::new_v4() } else { marker },
            )
            .await
            .unwrap_err(),
            "23503",
        );
        tx.rollback().await.unwrap();
        assert_eq!(audit(&f).await, json!([]));
    }
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlstate(
        &observation(&mut tx, &request, marker).await.unwrap_err(),
        "23514",
    );
    tx.rollback().await.unwrap();
    assert_eq!(audit(&f).await, json!([]));
}

#[tokio::test]
async fn workflow_action_uncertainty_sql_nonremote_link_has_no_receipt_mask() {
    let (f, request) = setup().await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let marker = Uuid::new_v4();
    Dispatch::insert(&mut tx, &request, marker, "local", json!({}))
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_runs SET state='cancelled'")
        .execute(&mut *tx)
        .await
        .unwrap();
    let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_receipts")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(receipts, 0);
    sqlstate(
        &observation(&mut tx, &request, marker).await.unwrap_err(),
        "23503",
    );
    tx.rollback().await.unwrap();
    assert_eq!(facts(&f).await, (0, 0, 0));
    assert_eq!(audit(&f).await, json!([]));
}

#[tokio::test]
async fn workflow_action_uncertainty_sql_remote_receipt_guard_and_append_only() {
    let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let provider = dedup(&f, contract, false);
    assert!(matches!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    cancel(&f, &request).await;
    let marker: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_action_dispatches WHERE effect_kind='remote'")
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlstate(
        &observation(&mut tx, &request, marker).await.unwrap_err(),
        "23514",
    );
    tx.rollback().await.unwrap();
    assert_eq!(audit(&f).await, json!([]));
    assert_eq!(facts(&f).await, (1, 1, 0));

    let (f, request) = setup().await;
    assert!(matches!(
        adapter(&f, 0).reserve_remote(&request, None).await.unwrap(),
        RemoteReservationResult::Reserved(_)
    ));
    assert!(
        f.persistence()
            .release_io(request.fence, policy(), classified())
            .await
            .unwrap()
    );
    let first = audit(&f).await;
    for sql in [
        "UPDATE workflow_action_reconciliations SET observed_at=observed_at",
        "DELETE FROM workflow_action_reconciliations",
    ] {
        let error = sqlx::query(sql)
            .execute(f.persistence().pool())
            .await
            .unwrap_err();
        sqlstate(&error, "P0001");
        assert_eq!(audit(&f).await, first);
    }
    let mut tx = f.persistence().pool().begin().await.unwrap();
    action_uncertainty::record(
        &mut tx,
        request.scope().company,
        request.scope().run,
        Some(request.scope().execution),
        &FailureCode::parse("later.observation").unwrap(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(audit(&f).await, first);
    assert_eq!(first[0]["failure_code"], "provider.rejected");
}

async fn sibling(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    value: u64,
) -> ActionDispatchRequest {
    let mut action = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action
        .request()
        .clone();
    action.arguments = json!({"value":value});
    let intent = ActionService::new(f.persistence().clone())
        .prepare_step(action)
        .await
        .unwrap();
    ActionDispatchRequest {
        subject: intent.approval_subject(),
        ..request.clone()
    }
}

async fn terminal(f: &AdmissionFixture, request: &ActionDispatchRequest, code: &str) {
    if code == "workflow.cancelled" {
        cancel(f, request).await;
    } else {
        sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute'")
            .execute(f.persistence().pool()).await.unwrap();
        assert!(
            f.persistence()
                .expire_run(request.fence.scope)
                .await
                .unwrap()
        );
        assert!(
            !f.persistence()
                .expire_run(request.fence.scope)
                .await
                .unwrap()
        );
    }
}

fn assert_terminal_retirement(saved: &Value, code: &str) {
    assert_eq!(
        saved["runs"][0]["state"],
        if code == "workflow.cancelled" {
            "cancelled"
        } else {
            "failed"
        }
    );
    assert_eq!(saved["jobs"][0]["status"], "failed");
    assert_eq!(saved["jobs"][0]["retry_count"], 1);
    assert_eq!(saved["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(saved["attempts"][0]["workflow_failure_code"], code);
    assert_eq!(saved["attempts"][0]["workflow_failure_class"], "terminal");
    assert_eq!(saved["attempts"][0]["workflow_retry_safety"], "unknown");
    assert_eq!(
        saved["attempts"][0]["workflow_retirement"],
        if code == "workflow.cancelled" {
            "cancel"
        } else {
            "deadline"
        }
    );
    for field in ["completed_at", "committed_output", "successor_execution_id"] {
        assert_eq!(saved["executions"][0][field], Value::Null);
    }
}

async fn terminal_audit(code: &str) {
    let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let requests = [
        request.clone(),
        sibling(&f, &request, 2).await,
        sibling(&f, &request, 3).await,
    ];
    for (index, current) in requests.iter().enumerate() {
        let provider = dedup(&f, contract.clone(), index != 2);
        let result = ActionService::new(adapter(&f, 0))
            .dispatch_remote(current, &provider, &CancellationToken::new())
            .await;
        if index == 2 {
            assert!(matches!(
                result.unwrap(),
                RemoteDispatchObservation::Committed(_)
            ));
        } else {
            assert!(matches!(result, Err(AppError::Timeout(_))));
        }
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
    assert_eq!(audit(&f).await, json!([]));
    assert_eq!(facts(&f).await, (3, 1, 0));
    assert_eq!(effect_count(&f).await, 3);
    terminal(&f, &request, code).await;
    let observations = audit(&f).await;
    assert_eq!(observations.as_array().unwrap().len(), 2);
    let linked: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_reconciliations AS observation JOIN workflow_action_dispatches AS marker ON marker.company_id=observation.company_id AND marker.run_id=observation.run_id AND marker.execution_id=observation.execution_id AND marker.invocation_id=observation.invocation_id AND marker.argument_digest=observation.argument_digest AND marker.id=observation.dispatch_id AND marker.effect_kind=observation.effect_kind WHERE NOT EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt WHERE receipt.company_id=observation.company_id AND receipt.invocation_id=observation.invocation_id)")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(linked, 2);
    for current in &requests[..2] {
        let observation = observations
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["invocation_id"] == current.subject.invocation.as_uuid().to_string())
            .unwrap();
        assert_eq!(observation["failure_code"], code);
        assert_eq!(
            observation["argument_digest"],
            current.subject.argument_digest.as_str()
        );
        assert!(!observation["observed_at"].is_null());
    }
    let saved = snapshot(&f).await;
    assert_terminal_retirement(&saved, code);
    assert_eq!(facts(&f).await, (3, 1, 0));
    assert_eq!(effect_count(&f).await, 3);
    assert!(
        !f.persistence()
            .release_io(request.fence, policy(), classified())
            .await
            .unwrap()
    );
    assert_eq!(audit(&f).await, observations);
    assert_eq!(snapshot(&f).await, saved);
}

#[tokio::test]
async fn workflow_action_uncertainty_cancel_records_all_unresolved_before_any_audit() {
    terminal_audit("workflow.cancelled").await;
}

#[tokio::test]
async fn workflow_action_uncertainty_deadline_records_all_unresolved_before_any_audit() {
    terminal_audit("workflow.run_deadline").await;
}
