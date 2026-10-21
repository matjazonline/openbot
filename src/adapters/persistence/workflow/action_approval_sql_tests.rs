use super::*;

#[tokio::test]
async fn workflow_action_approval_digest_fk_rejects_each_mismatch() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let request = p
        .http_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let frozen = FrozenAction::restore(serde_json::to_value(&request).unwrap()).unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    let scope = setup.claim.fence.scope;
    for field in ["argument", "operation", "invocation", "job", "attempt"] {
        let mut tx = p.pool().begin().await.unwrap();
        let wait = Uuid::new_v4();
        sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,notification_intent) VALUES($1,$2,$3,$4,'decision',clock_timestamp()+interval '1 minute',$5)")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait)
            .bind(json!({"version":1,"kind":"wait_registered","wait_id":wait,"run_id":scope.run.as_uuid(),"execution_id":scope.execution.as_uuid()})).execute(&mut *tx).await.unwrap();
        let argument = if field == "argument" {
            "0".repeat(64)
        } else {
            frozen.argument_digest().as_str().to_owned()
        };
        let operation = if field == "operation" {
            "0".repeat(64)
        } else {
            frozen.operation_digest().as_str().to_owned()
        };
        let invocation = if field == "invocation" {
            Uuid::new_v4()
        } else {
            intent.approval_subject().invocation.as_uuid()
        };
        let job = if field == "job" {
            Uuid::new_v4()
        } else {
            scope.job.0
        };
        let attempt = if field == "attempt" {
            999
        } else {
            setup.claim.fence.attempt.0
        };
        sqlx::query("SET CONSTRAINTS workflow_decisions_company_id_execution_id_job_id_fkey, workflow_decisions_job_id_parked_attempt_number_fkey IMMEDIATE").execute(&mut *tx).await.unwrap();
        let error=sqlx::query("INSERT INTO workflow_decisions(company_id,run_id,execution_id,id,wait_id,subject_kind,proposal_digest,action_invocation_id,argument_digest,operation_digest,job_id,parked_attempt_number) VALUES($1,$2,$3,$4,$5,'action_approval',$6,$7,$8,$9,$10,$11)")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(Uuid::new_v4()).bind(wait)
            .bind("0".repeat(64)).bind(invocation).bind(argument).bind(operation).bind(job).bind(attempt).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some(match field {
                "job" => "workflow_decisions_company_id_execution_id_job_id_fkey",
                "attempt" => "workflow_decisions_job_id_parked_attempt_number_fkey",
                _ => "workflow_decisions_action_identity",
            }),
            "{field}: {error}"
        );
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn workflow_action_approval_invalid_choice_data_and_digest_leave_open() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
    parked(&setup).await;
    let decision = view(&setup).await;
    let p = &setup.fixture.persistence;
    for field in ["choice", "data", "digest"] {
        let mut command = submission(&setup, &decision, "approve");
        match field {
            "choice" => command.choice = "accept".into(),
            "data" => command.data = json!({"approved":true}),
            _ => command.expected_artifact = "0".repeat(64),
        }
        assert!(p.submit_decision(command).await.is_err(), "{field}");
        let after = view(&setup).await;
        assert_eq!(after.state, "waiting");
        assert_eq!(after.revision, decision.revision);
    }
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}

#[tokio::test]
async fn workflow_action_approval_digest_cannot_authorize_changed_arguments() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
    parked(&setup).await;
    let p = &setup.fixture.persistence;
    p.submit_decision(submission(&setup, &view(&setup).await, "approve"))
        .await
        .unwrap();
    let next = p
        .claim_io(
            setup.claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    let mut request = p.http_step_request(&next, &setup.kind).await.unwrap();
    request.arguments["path"] = json!("/different-effect");
    let frozen = FrozenAction::restore(serde_json::to_value(&request).unwrap()).unwrap();
    assert!(
        p.http_for_action(&frozen).await.is_err(),
        "provider credential boundary checks the exact approval"
    );
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    let dispatch =
        action_dispatch::PostgresActionDispatch::new(p.clone(), http::HttpActionAuthority, NoLocal)
            .with_decision_notices(notices(p));
    let request = ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: next.fence,
        lease: lease(),
    };
    assert!(matches!(
        dispatch.reserve_remote(&request, None).await.unwrap(),
        RemoteReservationResult::ApprovalParked(_)
    ));
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}

#[derive(Clone, Copy, Debug)]
enum ReopenCase {
    PriorApproval,
    Rejected,
    WrongContinuationCount,
    Valid,
}

pub(super) async fn submitted_on(db: &mut sqlx::PgConnection, setup: &Setup, choice: &str) {
    let scope = setup.claim.fence.scope;
    sqlx::query("UPDATE workflow_decisions AS decision SET submitted_at=clock_timestamp(),submitted_by=reviewer.principal_id,command_key='guard-test',command_fingerprint=$2,choice=$3,feedback='',data='{}',revision=2 FROM workflow_decision_reviewers AS reviewer WHERE reviewer.decision_id=decision.id AND decision.execution_id=$1")
        .bind(scope.execution.as_uuid()).bind("0".repeat(64)).bind(choice).execute(&mut *db).await.unwrap();
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE execution_id=$1")
        .bind(scope.execution.as_uuid()).execute(&mut *db).await.unwrap();
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(db)
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_action_approval_reopen_checks_transaction_choice_and_accounting() {
    for case in [
        ReopenCase::PriorApproval,
        ReopenCase::Rejected,
        ReopenCase::WrongContinuationCount,
        ReopenCase::Valid,
    ] {
        let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
        parked(&setup).await;
        let p = &setup.fixture.persistence;
        let mut tx = p.pool().begin().await.unwrap();
        let choice = match case {
            ReopenCase::Rejected => "reject",
            _ => "approve",
        };
        submitted_on(&mut tx, &setup, choice).await;
        if matches!(case, ReopenCase::PriorApproval) {
            tx.commit().await.unwrap();
            tx = p.pool().begin().await.unwrap();
        }
        let same_transaction: bool = sqlx::query_scalar("SELECT submitted_xact=pg_current_xact_id() FROM workflow_decisions WHERE execution_id=$1")
            .bind(setup.claim.fence.scope.execution.as_uuid()).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(same_transaction, !matches!(case, ReopenCase::PriorApproval));
        let increment = match case {
            ReopenCase::WrongContinuationCount => 0,
            _ => 1,
        };
        sqlx::query(
            "UPDATE background_tasks SET status='pending',continuation_count=continuation_count+$2 WHERE id=$1",
        )
        .bind(setup.claim.fence.scope.job.0)
        .bind(increment)
        .execute(&mut *tx)
        .await
        .unwrap();
        let result = tx.commit().await;
        if matches!(case, ReopenCase::Valid) {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            let database = error.as_database_error().unwrap();
            assert_eq!(database.code().as_deref(), Some("23514"), "{case:?}");
            assert_eq!(
                database.message(),
                "workflow job reopen requires an approval or delivery continuation",
                "{case:?}"
            );
            let job: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('status',status,'retry_count',retry_count) FROM background_tasks WHERE id=$1")
                .bind(setup.claim.fence.scope.job.0).fetch_one(p.pool()).await.unwrap();
            assert_eq!(job, json!({"status":"completed","retry_count":0}));
        }
    }
}

#[tokio::test]
async fn workflow_action_approval_unprovable_notice_key_fails_without_park() {
    let (origin, server) = server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").await;
    let setup = Box::pin(protected(&origin, Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    sqlx::query("UPDATE users SET email=$2 WHERE id=$1")
        .bind(setup.fixture.target.actor.user_id())
        .bind(format!("{}@example.test", "a".repeat(400)))
        .execute(p.pool())
        .await
        .unwrap();
    let failure = Box::pin(actions(&setup).run(&setup.kind, &setup.claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "http.approval_required");
    assert_eq!(
        failure.failure.class(),
        crate::domain::workflow::FailureClass::Terminal
    );
    let counts: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('waits',(SELECT count(*) FROM workflow_waits),'decisions',(SELECT count(*) FROM workflow_decisions),'reviewers',(SELECT count(*) FROM workflow_decision_reviewers),'deliveries',(SELECT count(*) FROM message_deliveries))")
        .fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        counts,
        json!({"waits":0,"decisions":0,"reviewers":0,"deliveries":0})
    );
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
    assert!(!server.is_finished(), "provider was not contacted");
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
