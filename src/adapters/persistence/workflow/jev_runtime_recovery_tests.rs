use super::*;

#[tokio::test]
async fn jev_runtime_lost_lease_cancels_actual_http_and_prevents_settlement() {
    use crate::application::workflow::supervise::{StopReason, SupervisedResult, supervise_io};
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let first = claim(&f, target).await;
    let (http, server) = server(
        200,
        "",
        serde_json::to_vec(&output()).unwrap(),
        Duration::from_secs(2),
    )
    .await;
    let runtime = JevRuntime::new(f.persistence.clone(), Arc::new(http), policy());
    let kind = WorkflowStepKind("jev.evaluate".into());
    let operation = runtime.execute(&kind, &first);
    let supervised = supervise_io(
        &f.persistence,
        first.clone(),
        LeasePolicy::new(Duration::from_secs(3)).unwrap(),
        first.window.run_deadline,
        operation,
        std::future::pending::<()>(),
    );
    let expire = async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '40 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(first.fence.scope.job.0).execute(f.persistence.pool()).await.unwrap();
    };
    let (result, ()) = tokio::join!(supervised, expire);
    assert!(matches!(
        result,
        SupervisedResult::Stopped(StopReason::LeaseLost)
    ));
    server.await.unwrap();
    assert_eq!(
        counts(&f.persistence, &first).await,
        json!({"calls":1,"reserved":1,"settled":0,"outputs":0})
    );
}

#[tokio::test]
async fn jev_runtime_terminal_failure_and_retry_after_survive_worker_loss() {
    for status in [401, 429] {
        let f = fixture().await;
        let c = create(&f).await;
        let target = bound(&f, &c).await;
        let first = claim(&f, target).await;
        let request = JevRequest::prepare(&first.activation.inputs).unwrap();
        let JevAdmissionResult::New(grant) = f
            .persistence
            .admit_jev(&first, &request, policy())
            .await
            .unwrap()
        else {
            panic!()
        };
        f.persistence
            .fail_jev(
                &first,
                &grant,
                JevError::Http(status, Some(Duration::from_secs(60))),
                Duration::ZERO,
                policy(),
            )
            .await
            .unwrap();
        let second = expire_reclaim(&f.persistence, &first).await;
        if status == 401 {
            let runtime = JevRuntime::new(
                f.persistence.clone(),
                Arc::new(JevHttp::loopback("http://127.0.0.1:1/v1/systemone")),
                policy(),
            );
            let WorkflowHandlerOutcome::Completed(Err(error)) = runtime
                .execute(&WorkflowStepKind("jev.evaluate".into()), &second)
                .await
            else {
                panic!()
            };
            assert_eq!(
                error.failure.class(),
                crate::domain::workflow::FailureClass::Terminal
            );
            assert_eq!(error.failure.code().as_str(), "jev.credentials");
        } else {
            assert!(matches!(
                f.persistence
                    .admit_jev(&second, &request, policy())
                    .await
                    .unwrap(),
                JevAdmissionResult::Deferred(_)
            ));
        }
        assert_eq!(
            counts(&f.persistence, &second).await,
            json!({"calls":1,"reserved":1,"settled":0,"outputs":0})
        );
    }
}

#[tokio::test]
async fn jev_runtime_corrupt_credential_is_terminal_and_optional_agent_usage_reports() {
    use crate::application::services::model_usage::{
        ModelUsageStore, UsageGrouping, UsageQuery, UsageScope,
    };
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let first = claim(&f, target).await;
    let request = JevRequest::prepare(&first.activation.inputs).unwrap();
    let JevAdmissionResult::New(_grant) = f
        .persistence
        .admit_jev(&first, &request, policy())
        .await
        .unwrap()
    else {
        panic!()
    };
    let end = chrono::Utc::now();
    let report = f
        .persistence
        .usage_report(UsageQuery {
            scope: UsageScope {
                company: c.company.as_uuid(),
                actor: target.actor.user_id(),
            },
            start: end - chrono::Duration::days(1),
            end,
            grouping: UsageGrouping::Model,
            agent: None,
            workflow: None,
        })
        .await
        .unwrap();
    assert_eq!(report.groups.len(), 1);
    assert_eq!(report.groups[0].calls, 1);
    assert_eq!(report.groups[0].unconfirmed_calls, 1);
    // Damage only this disposable fixture's encrypted envelope; current use must refuse it.
    sqlx::query("UPDATE company_typesafe_connections SET envelope='enc:v2:invalid',revision=revision+1 WHERE company_id=$1 AND id=$2").bind(c.company.as_uuid()).bind(c.id).execute(f.persistence.pool()).await.unwrap();
    let second = expire_reclaim(&f.persistence, &first).await;
    let runtime = JevRuntime::new(
        f.persistence.clone(),
        Arc::new(JevHttp::loopback("http://127.0.0.1:1/v1/systemone")),
        policy(),
    );
    let WorkflowHandlerOutcome::Completed(Err(error)) = runtime
        .execute(&WorkflowStepKind("jev.evaluate".into()), &second)
        .await
    else {
        panic!()
    };
    assert_eq!(
        error.failure.class(),
        crate::domain::workflow::FailureClass::Terminal
    );
    assert_eq!(
        counts(&f.persistence, &second).await,
        json!({"calls":1,"reserved":1,"settled":0,"outputs":0})
    );
}

#[tokio::test]
async fn jev_runtime_queued_capacity_rechecks_revocation_model_and_current_key() {
    for change in ["rotate", "disable", "model"] {
        let f = fixture().await;
        let c = create(&f).await;
        let target = bound(&f, &c).await;
        let claim = claim(&f, target).await;
        let (http, server) = server(
            200,
            "",
            serde_json::to_vec(&output()).unwrap(),
            Duration::ZERO,
        )
        .await;
        let http = Arc::new(http);
        let capacity = http.occupy_capacity().await;
        let runtime = JevRuntime::new(f.persistence.clone(), http.clone(), policy());
        let kind = WorkflowStepKind("jev.evaluate".into());
        let operation = runtime.execute(&kind, &claim);
        tokio::pin!(operation);
        // This notification is emitted at actual transport capacity acquisition, not on a timer.
        tokio::select! { _=http.wait_for_capacity_request()=>{}, _=&mut operation=>panic!("capacity must block") }
        assert_eq!(
            counts(&f.persistence, &claim).await,
            json!({"calls":0,"reserved":0,"settled":0,"outputs":0})
        );
        if change == "rotate" {
            f.persistence
                .rotate_jev(
                    c.company,
                    target.actor,
                    c.id,
                    c.revision,
                    secrecy::SecretString::from("queued-current-key"),
                )
                .await
                .unwrap();
        } else {
            f.persistence
                .update_jev(
                    c.company,
                    target.actor,
                    c.id,
                    JevConnectionUpdate {
                        expected_revision: c.revision,
                        enabled: change != "disable",
                        models: [JevModel::parse(if change == "model" {
                            "jev-1.13.0"
                        } else {
                            "jev-1.14.0"
                        })
                        .unwrap()]
                        .into(),
                    },
                )
                .await
                .unwrap();
        }
        drop(capacity);
        match operation.await {
            WorkflowHandlerOutcome::Completed(Ok(value)) if change == "rotate" => {
                assert_eq!(value, output());
                assert!(server.await.unwrap().contains("Bearer queued-current-key"));
            }
            WorkflowHandlerOutcome::Completed(Err(error)) if change != "rotate" => {
                assert_eq!(
                    error.failure.class(),
                    crate::domain::workflow::FailureClass::Terminal
                );
                assert_eq!(
                    counts(&f.persistence, &claim).await,
                    json!({"calls":0,"reserved":0,"settled":0,"outputs":0})
                );
                server.abort();
                assert!(server.await.unwrap_err().is_cancelled());
            }
            _ => panic!("current authority must determine dispatch"),
        }
    }
}

#[tokio::test]
async fn jev_runtime_ledger_rejects_cross_tenant_and_mismatched_provenance() {
    use crate::adapters::persistence::credentials::CredentialCipher;
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let first = claim(&f, target).await;
    let request = JevRequest::prepare(&first.activation.inputs).unwrap();
    let JevAdmissionResult::New(grant) = f
        .persistence
        .admit_jev(&first, &request, policy())
        .await
        .unwrap()
    else {
        panic!()
    };
    let mut foreign = Fixture::in_database(f._db.clone(), "jev-foreign-ledger").await;
    foreign.persistence = PostgresPersistence::with_credential_cipher(
        foreign.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let foreign_connection = create(&foreign).await;
    let foreign_target = bound(&foreign, &foreign_connection).await;
    let other = claim(&foreign, foreign_target).await;
    let JevAdmissionResult::New(foreign_grant) = f
        .persistence
        .admit_jev(
            &other,
            &JevRequest::prepare(&other.activation.inputs).unwrap(),
            policy(),
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    let same = Fixture {
        _db: f._db.clone(),
        persistence: f.persistence.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..f.target
        },
    };
    let same_target = bound(&same, &c).await;
    let sibling = claim(&same, same_target).await;
    let JevAdmissionResult::New(sibling_grant) = f
        .persistence
        .admit_jev(
            &sibling,
            &JevRequest::prepare(&sibling.activation.inputs).unwrap(),
            policy(),
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    let original:Value=sqlx::query_scalar("SELECT to_jsonb(invocation) FROM workflow_jev_invocations AS invocation WHERE company_id=$1 AND request_id=$2").bind(c.company.as_uuid()).bind(grant.request_id()).fetch_one(f.persistence.pool()).await.unwrap();
    for case in [
        "foreign_execution",
        "foreign_usage",
        "same_tenant_usage",
        "same_tenant_run",
    ] {
        let mut row = original.clone();
        row["generation"] = json!(Uuid::new_v4());
        match case {
            "foreign_execution" => {
                row["request_id"] = json!(Uuid::new_v4());
                row["run_id"] = json!(other.fence.scope.run.as_uuid());
                row["execution_id"] = json!(other.fence.scope.execution.as_uuid());
            }
            "foreign_usage" => row["request_id"] = json!(foreign_grant.request_id()),
            "same_tenant_usage" => row["request_id"] = json!(sibling_grant.request_id()),
            _ => {
                row["request_id"] = json!(Uuid::new_v4());
                row["run_id"] = json!(sibling.fence.scope.run.as_uuid());
            }
        }
        let mut tx = f.persistence.pool().begin().await.unwrap();
        // Remove the matching ledger row only in this rollback-only transaction so the
        // composite FK, rather than the primary key, is what rejects reused usage IDs.
        sqlx::query("DELETE FROM workflow_jev_invocations WHERE company_id=$1 AND request_id=$2")
            .bind(c.company.as_uuid())
            .bind(sibling_grant.request_id())
            .execute(&mut *tx)
            .await
            .unwrap();
        let error=sqlx::query("INSERT INTO workflow_jev_invocations SELECT (jsonb_populate_record(NULL::workflow_jev_invocations,$1)).*").bind(row).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("23503"),
            "{case}: {error}"
        );
        tx.rollback().await.unwrap();
    }
    assert_eq!(
        counts(&f.persistence, &first).await,
        json!({"calls":1,"reserved":1,"settled":0,"outputs":0})
    );
}
