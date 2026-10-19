use super::continuation_tests::{call, call_park, counters, resume};
use super::*;

async fn history(setup: &Setup) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('run',(SELECT to_jsonb(run) FROM workflow_runs AS run WHERE id=$1),'executions',(SELECT jsonb_agg(to_jsonb(execution) ORDER BY id) FROM workflow_executions AS execution WHERE run_id=$1),'jobs',(SELECT jsonb_agg(to_jsonb(job) ORDER BY job.id) FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.id=job.workflow_execution_id WHERE execution.run_id=$1),'waits',(SELECT jsonb_agg(to_jsonb(wait) ORDER BY id) FROM workflow_waits AS wait WHERE run_id=$1),'decisions',(SELECT jsonb_agg(to_jsonb(decision) ORDER BY id) FROM workflow_decisions AS decision WHERE run_id=$1),'events',(SELECT jsonb_agg(to_jsonb(event) ORDER BY sequence) FROM workflow_run_events AS event WHERE run_id=$1))")
        .bind(setup.claim.fence.scope.run.as_uuid()).fetch_one(setup.fixture.persistence.pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_action_approval_replay_preserves_result_after_later_successor() {
    let setup = Box::pin(protected_customized("http://127.0.0.1:1", Entrance::Tool, 1, |source| {
        source["steps"]["start"]["routes"]["error"] = json!("finish");
        source["steps"]["finish"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"$end"}});
    })).await;
    let first = call_park(&setup, &setup.claim, 0).await;
    let p = &setup.fixture.persistence;
    let command = submission(&setup, &first, "approve");
    let applied = p.submit_decision(command.clone()).await.unwrap();
    assert!(applied.successor.is_none());
    let claim = p
        .claim_io(
            setup.claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    let second = call_park(&setup, &claim, 1).await;
    let rejected = p
        .submit_decision(submission(&setup, &second, "reject"))
        .await
        .unwrap();
    assert!(rejected.successor.is_some());
    let before = history(&setup).await;
    let replayed = p.submit_decision(command).await.unwrap();
    assert_eq!(replayed.disposition, DecisionDisposition::Replayed);
    assert_eq!(replayed.revision, applied.revision);
    assert_eq!(replayed.successor, applied.successor);
    assert_eq!(history(&setup).await, before);
}

#[tokio::test]
async fn workflow_action_approval_audit_kind_binding_preserves_ordinary_uniqueness() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
    let first = call_park(&setup, &setup.claim, 0).await;
    let claim = resume(&setup, &first).await;
    let second = call_park(&setup, &claim, 1).await;
    resume(&setup, &second).await;
    let p = &setup.fixture.persistence;
    let query = "INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id,decision_id) SELECT company_id,run_id,sequence+100,$2,actor_id,execution_id,$3 FROM workflow_run_events WHERE decision_id=$1 AND event_kind='action_approval_opened'";
    for (kind, decision) in [
        ("ordinary_fixture", Some(first.decision.0)),
        ("action_approval_opened", None),
        ("action_approved", None),
        ("http.approval_rejected", None),
        ("http.approval_expired", None),
        ("mcp.approval_rejected", None),
        ("mcp.approval_expired", None),
        ("message.approval_rejected", None),
        ("message.approval_expired", None),
    ] {
        let error = sqlx::query(query)
            .bind(first.decision.0)
            .bind(kind)
            .bind(decision)
            .execute(p.pool())
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("workflow_approval_event_kind"),
            "{kind}: {error}"
        );
    }
    sqlx::query(query)
        .bind(first.decision.0)
        .bind("ordinary_fixture")
        .bind(None::<Uuid>)
        .execute(p.pool())
        .await
        .unwrap();
    let error = sqlx::query(query)
        .bind(second.decision.0)
        .bind("ordinary_fixture")
        .bind(None::<Uuid>)
        .execute(p.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_run_event_execution_kind")
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_events WHERE run_id=$1 AND event_kind='action_approved' AND decision_id IN ($2,$3)")
        .bind(claim.fence.scope.run.as_uuid()).bind(first.decision.0).bind(second.decision.0).fetch_one(p.pool()).await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn workflow_action_approval_operator_retry_preserves_nonzero_continuation_witness() {
    use crate::application::workflow::lease::WorkflowFailure;
    use crate::application::workflow::{IdempotencyKey, RetryCommand, RetryResult};
    use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
    let first = call_park(&setup, &setup.claim, 0).await;
    let claim = resume(&setup, &first).await;
    let p = &setup.fixture.persistence;
    let scope = claim.fence.scope;
    assert!(
        p.release_io(
            claim.fence,
            lease(),
            LeaseReleaseCause::Classified(WorkflowFailure {
                failure: StepFailure::new(
                    FailureClass::Terminal,
                    FailureCode::parse("provider.rejected").unwrap(),
                    None
                )
                .unwrap(),
                safety: RetrySafety::SafeToRetry,
            })
        )
        .await
        .unwrap()
    );
    assert_eq!(
        counters(&setup).await,
        json!({"failures":1,"continuations":1,"ordinal":3,"status":"failed"})
    );
    let result = p
        .retry(RetryCommand {
            company_id: scope.company,
            run_id: scope.run,
            actor: setup.fixture.target.actor,
            command_key: IdempotencyKey::parse("after-approval").unwrap(),
            expected_revision: p
                .head(scope.company, scope.run)
                .await
                .unwrap()
                .unwrap()
                .revision,
        })
        .await
        .unwrap();
    assert!(matches!(result, RetryResult::Applied { .. }), "{result:?}");
    assert_eq!(
        counters(&setup).await,
        json!({"failures":1,"continuations":1,"ordinal":3,"status":"pending"})
    );
    let witness: Value = sqlx::query_scalar("SELECT jsonb_build_object('company',witness.company_id,'run',witness.run_id,'execution',witness.execution_id,'job',witness.job_id,'retired',witness.retired_attempt,'attempt',attempt.attempt_number,'status',attempt.status,'initial',state.initial_state) FROM workflow_action_schedule_witnesses AS witness JOIN task_attempts AS attempt ON attempt.task_id=witness.job_id AND attempt.attempt_number=witness.retired_attempt JOIN workflow_action_state_witnesses AS state ON state.company_id=witness.company_id AND state.run_id=witness.run_id AND state.transaction_id=witness.transaction_id WHERE witness.job_id=$1")
        .bind(scope.job.0).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        witness,
        json!({"company":scope.company.as_uuid(),"run":scope.run.as_uuid(),"execution":scope.execution.as_uuid(),"job":scope.job.0,"retired":2,"attempt":2,"status":"failed","initial":"failed"})
    );
    let next = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.fence.attempt.0, 3);
    assert!(!p.validate_io(claim.fence, lease()).await.unwrap());
    assert!(!p.validate_io(setup.claim.fence, lease()).await.unwrap());
    assert_eq!(
        counters(&setup).await,
        json!({"failures":1,"continuations":1,"ordinal":3,"status":"processing"})
    );
}

#[tokio::test]
async fn workflow_action_approval_identical_operation_new_call_id_reuses_one_receipt() {
    use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
    use tokio_util::sync::CancellationToken;
    let server = scripted_scenario(vec![ScriptedExchange::new(
        |_| Ok(()),
        ScriptedResponse::json(json!({"ok":true})),
    )])
    .await;
    let setup = Box::pin(protected(&server.base_url, Entrance::Tool)).await;
    let decision = call_park(&setup, &setup.claim, 0).await;
    let claim = resume(&setup, &decision).await;
    let dispatch = actions(&setup);
    let first = dispatch
        .call_tool(claim.fence, call(&setup, 0), &CancellationToken::new())
        .await
        .unwrap();
    let mut duplicate = call(&setup, 0);
    duplicate.call = ModelToolCallId::parse("another-model-call").unwrap();
    let second = dispatch
        .call_tool(claim.fence, duplicate, &CancellationToken::new())
        .await
        .unwrap();
    let (RemoteDispatchObservation::Committed(first), RemoteDispatchObservation::Committed(second)) =
        (first, second)
    else {
        panic!("both calls must return the existing receipt")
    };
    assert_eq!(first.result, second.result);
    assert_eq!(server.finish().await, Ok(1));
    assert_eq!(action_counts(&setup.fixture).await.dispatches, 1);
    assert_eq!(counters(&setup).await["continuations"], 1);
}

#[tokio::test]
async fn workflow_action_approval_same_arguments_different_target_or_operation_need_new_approval() {
    for variant in ["target", "operation"] {
        let extra = (variant == "operation").then(|| TypeName::parse("registered.other").unwrap());
        let setup = Box::pin(protected_with_operation(
            "http://127.0.0.1:1",
            Entrance::Tool,
            1,
            |_| {},
            extra,
        ))
        .await;
        let p = &setup.fixture.persistence;
        let original = p
            .http_step_request(&setup.claim, &setup.kind)
            .await
            .unwrap();
        let original_frozen =
            FrozenAction::restore(serde_json::to_value(&original).unwrap()).unwrap();
        parked(&setup).await;
        let first = view(&setup).await;
        let claim = resume(&setup, &first).await;
        let mut changed = original.clone();
        let mut connection = setup.connection.clone();
        let name = if variant == "target" {
            connection = Box::pin(configured(
                &setup.fixture,
                "http://127.0.0.1:1",
                Entrance::Tool,
            ))
            .await;
            "registered.request"
        } else {
            "registered.other"
        };
        let mut operation = operation(setup.fixture.target.company, name);
        operation.approval_required = true;
        if variant == "target" {
            p.register_http_operation(
                setup.fixture.target.company,
                setup.fixture.target.actor,
                connection.id,
                connection.revision,
                operation.clone(),
            )
            .await
            .unwrap();
        }
        changed.target = ActionTarget::Local {
            resource: connection.id,
            resource_kind: TypeName::parse("http").unwrap(),
        };
        changed.operation_key = TypeName::parse(name).unwrap();
        changed.contract = operation.tool;
        let frozen = FrozenAction::restore(serde_json::to_value(&changed).unwrap()).unwrap();
        assert_eq!(original_frozen.argument_digest(), frozen.argument_digest());
        assert_ne!(
            original_frozen.operation_digest(),
            frozen.operation_digest()
        );
        assert!(p.http_for_action(&frozen).await.is_err());
        let intent = ActionService::new(p.clone())
            .prepare_step(changed)
            .await
            .unwrap();
        let dispatch = action_dispatch::PostgresActionDispatch::new(
            p.clone(),
            http::HttpActionAuthority,
            NoLocal,
        )
        .with_decision_notices(notices(p));
        let result = dispatch
            .reserve_remote(
                &ActionDispatchRequest {
                    subject: intent.approval_subject(),
                    fence: claim.fence,
                    lease: lease(),
                },
                None,
            )
            .await
            .unwrap();
        assert!(
            matches!(result, RemoteReservationResult::ApprovalParked(_)),
            "{variant}"
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM workflow_decisions WHERE execution_id=$1")
                .bind(claim.fence.scope.execution.as_uuid())
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(count, 2);
        assert_eq!(action_counts(&setup.fixture).await.dispatches, 0);
    }
}
