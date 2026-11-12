use super::*;
#[cfg(unix)]
#[path = "local_agent_retention_peer_tests.rs"]
mod peer;
use crate::application::local_agents::lifecycle::LocalAgentMaintenance;
use crate::application::local_agents::protocol::{
    MappingRetirementRequest, MappingRetirementSessions,
};
use crate::application::workflow::storage::{RetentionRoot, WorkflowStorageMaintenance};
use crate::application::workflow::waits::{
    WorkflowEvent, WorkflowEventCorrelation, WorkflowEventId, WorkflowEventName,
};

struct WaitingRoot {
    scope: ActivationRequest,
    target: ActivationRequest,
    session: SessionId,
}

async fn claim_retention(s: &Setup, scope: ActivationRequest) -> ClaimedWorkflow {
    let worker = WorkflowWorkerId(Uuid::new_v4());
    // These tests exercise retention, not claim latency. Retry only a positively
    // unapplied setup claim under whole-suite contention; unknown commits fail.
    // The separate uncontended lease-budget CI gate still bounds claim work.
    for attempt in 0..3 {
        match s.fixture.persistence.claim_io(scope, worker, lease()).await {
            Ok(claim) => return claim.expect("retention fixture must be claimable"),
            Err(AppError::DatabaseTimeout(message))
                if attempt < 2 && message.contains("not applied") =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(error) => panic!("retention fixture claim failed: {error}"),
        }
    }
    unreachable!()
}

async fn complete_local(
    s: &Setup,
    scope: ActivationRequest,
) -> (SessionId, Option<ActivationRequest>) {
    let claim = claim_retention(s, scope).await;
    s.park(&claim).await;
    s.native_success().await;
    consume_local(s, scope).await
}

async fn consume_local(
    s: &Setup,
    scope: ActivationRequest,
) -> (SessionId, Option<ActivationRequest>) {
    let p = &s.fixture.persistence;
    p.resume_wait(scope).await.unwrap();
    let claim = claim_retention(s, scope).await;
    let output = s
        .handler()
        .execute(&WorkflowStepKind(KIND.into()), &claim)
        .await
        .completed_result()
        .unwrap();
    let session =
        SessionId::new(Uuid::parse_str(output["session_id"].as_str().unwrap()).unwrap()).unwrap();
    let done = p
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap()
        .unwrap();
    (session, done.successor)
}

async fn waiting_root(s: &Setup, approval: Option<Value>) -> WaitingRoot {
    let f = Fixture {
        _db: s.fixture._db.clone(),
        persistence: s.fixture.persistence.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..s.fixture.target
        },
    };
    let mut source: Value = serde_json::from_str(&registry::example(KIND).unwrap().source).unwrap();
    source["workflow_id"] = json!(f.target.workflow.to_string());
    source["steps"]["start"]["with"] = json!({"profile_id":{"literal":s.profile.id},"prompt":{"literal":"create original reciprocal session"}});
    let mut follow = source["steps"]["start"].clone();
    follow["with"] = json!({"profile_id":{"literal":s.profile.id},"prompt":{"literal":"explicit reciprocal continuation"},"session_id":{"ref":"/steps/target/output/session_id"}});
    if let Some(approval) = approval {
        follow["with"]["approval"] = json!({"literal":approval});
    }
    source["steps"]["start"]["routes"]["success"] = json!("target");
    source["steps"]["target"] = json!({"type":"wait.event","with":{
        "event":{"literal":"local.target"},"correlation":{"literal":"reciprocal"},
        "deadline":{"literal":(chrono::Utc::now()+chrono::Duration::minutes(30)).to_rfc3339()},
        "payload_schema":{"literal":{"type":"object","properties":{"session_id":{"type":"string","format":"uuid"}},"required":["session_id"],"additionalProperties":false}}},
        "routes":{"success":"follow"}});
    source["steps"]["follow"] = follow;
    let version = graph_support::publish(&f, source).await;
    let binding = bound(&f, version).await;
    let scope = admitted_as(
        &f,
        binding,
        RelatedAssociation::Company,
        &Uuid::new_v4().to_string(),
    )
    .await;
    let (session, target) = complete_local(s, scope).await;
    let target = target.unwrap();
    f.persistence.park_wait(target).await.unwrap().unwrap();
    WaitingRoot {
        scope,
        target,
        session,
    }
}

async fn target(s: &Setup, root: &WaitingRoot, session: SessionId) -> ActivationRequest {
    let p = &s.fixture.persistence;
    assert!(
        super::super::recorded(
            p,
            WorkflowEvent {
                company: root.scope.company,
                run: root.scope.run,
                id: WorkflowEventId(Uuid::new_v4()),
                name: WorkflowEventName("local.target".into()),
                correlation: WorkflowEventCorrelation("reciprocal".into()),
                payload: json!({"session_id":session})
            }
        )
        .await
        .unwrap()
    );
    match p.resume_wait(root.target).await.unwrap() {
        WaitProgress::Completed(done) => done.successor.unwrap(),
        other => panic!("event must activate the declared continuation: {other:?}"),
    }
}

async fn cancel(s: &Setup, scope: ActivationRequest, key: &str) {
    let p = &s.fixture.persistence;
    let revision = p
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    p.cancel(CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: s.fixture.target.actor,
        command_key: IdempotencyKey::parse(key).unwrap(),
        expected_revision: revision,
    })
    .await
    .unwrap();
}

async fn prepare_racing_collection(s: &Setup, claim: &ClaimedWorkflow) -> WorkflowHandlerOutcome {
    let p = &s.fixture.persistence;
    let mut blocker = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM local_machines WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(s.scope.company.as_uuid())
        .bind(s.profile.selection.machine_id.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let handler = s.handler();
    let kind = WorkflowStepKind(KIND.into());
    let collect = async {
        super::continuation::wait_for_lock(s).await;
        let page = p.retain_local_agents(None).await.unwrap();
        assert_eq!(
            (page.archived_calls, page.removed_sessions),
            (0, 0),
            "in-flight authorized preparation owns the company before its durable call exists"
        );
        blocker.rollback().await.unwrap();
    };
    let (outcome, ()) = tokio::join!(handler.execute(&kind, claim), collect);
    outcome
}

async fn age(s: &Setup, roots: &[ActivationRequest]) {
    let p = &s.fixture.persistence;
    let ids: Vec<Uuid> = roots.iter().map(|scope| scope.run.as_uuid()).collect();
    let mut tx = p.pool().begin().await.unwrap();
    for statement in [
        "ALTER TABLE workflow_runs DISABLE TRIGGER workflow_run_snapshot_immutable",
        "ALTER TABLE workflow_local_agent_calls DISABLE TRIGGER workflow_local_agent_call_guard",
        "ALTER TABLE local_agent_operations DISABLE TRIGGER local_operation_inputs_immutable",
        "ALTER TABLE local_agent_operations DISABLE TRIGGER local_operation_settlement",
        "ALTER TABLE local_agent_sessions DISABLE TRIGGER local_session_inputs_immutable",
    ] {
        sqlx::query(statement).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '100 days',finished_at=clock_timestamp()-interval '91 days' WHERE company_id=$1 AND id=ANY($2) AND state IN ('succeeded','failed','cancelled')")
        .bind(s.scope.company.as_uuid()).bind(&ids).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE local_agent_operations AS operation SET created_at=clock_timestamp()-interval '100 days',settled_at=clock_timestamp()-interval '91 days' WHERE company_id=$1 AND settled_at IS NOT NULL AND EXISTS(SELECT 1 FROM workflow_local_agent_calls AS call WHERE call.company_id=operation.company_id AND call.operation_id=operation.id AND call.run_id=ANY($2))")
        .bind(s.scope.company.as_uuid()).bind(&ids).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE workflow_local_agent_calls SET created_at=clock_timestamp()-interval '99 days' WHERE company_id=$1 AND run_id=ANY($2)")
        .bind(s.scope.company.as_uuid()).bind(&ids).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE local_agent_sessions SET created_at=clock_timestamp()-interval '100 days' WHERE company_id=$1 AND workflow_run_id=ANY($2)")
        .bind(s.scope.company.as_uuid()).bind(&ids).execute(&mut *tx).await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    for statement in [
        "ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable",
        "ALTER TABLE workflow_local_agent_calls ENABLE TRIGGER workflow_local_agent_call_guard",
        "ALTER TABLE local_agent_operations ENABLE TRIGGER local_operation_inputs_immutable",
        "ALTER TABLE local_agent_operations ENABLE TRIGGER local_operation_settlement",
        "ALTER TABLE local_agent_sessions ENABLE TRIGGER local_session_inputs_immutable",
    ] {
        sqlx::query(statement).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
}

async fn retention_root(s: &Setup, scope: ActivationRequest) -> RetentionRoot {
    let finished_at =
        sqlx::query_scalar("SELECT finished_at FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(s.fixture.persistence.pool())
            .await
            .unwrap();
    RetentionRoot {
        finished_at,
        company: scope.company,
        run: scope.run,
    }
}

#[tokio::test]
async fn cloudroom_retention_two_terminal_roots_with_reciprocal_native_sessions_eventually_retire()
{
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let a = waiting_root(&s, None).await;
    let b = waiting_root(&s, None).await;
    let follow_a = target(&s, &a, b.session).await;
    let follow_b = target(&s, &b, a.session).await;
    assert_eq!(complete_local(&s, follow_a).await.0, b.session);
    assert_eq!(complete_local(&s, follow_b).await.0, a.session);
    let cross:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_local_agent_calls AS call JOIN local_agent_sessions AS session ON session.company_id=call.company_id AND session.id=call.session_id WHERE call.company_id=$1 AND ((call.run_id=$2 AND session.workflow_run_id=$3) OR (call.run_id=$3 AND session.workflow_run_id=$2))")
        .bind(s.scope.company.as_uuid()).bind(a.scope.run.as_uuid()).bind(b.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        cross, 2,
        "both real roots must own a continuation of the other's session"
    );
    age(&s, &[a.scope, b.scope]).await;
    let roots = [
        retention_root(&s, a.scope).await,
        retention_root(&s, b.scope).await,
    ];
    assert!(p.retire_tree(roots[0]).await.unwrap().is_none());
    assert!(p.retire_tree(roots[1]).await.unwrap().is_none());
    let (one, two) = tokio::join!(p.retain_local_agents(None), p.retain_local_agents(None));
    assert_eq!(one.unwrap().archived_calls + two.unwrap().archived_calls, 4);
    for root in roots {
        assert!(p.retire_tree(root).await.unwrap().is_some());
    }
    let left: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_local_agent_retention_proofs WHERE company_id=$1",
    )
    .bind(s.scope.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        left, 0,
        "only the run owner finally removes its closure proofs"
    );
}

#[tokio::test]
async fn cloudroom_retention_prepared_and_queued_continuation_preserves_old_original_session() {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let approval = json!({"reviewers":[{"kind":"principal","id":s.fixture.principal(s.fixture.target.actor.user_id()).await}],"deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()});
    let a = waiting_root(&s, Some(approval)).await;
    let b = waiting_root(&s, None).await;
    let follow = target(&s, &a, b.session).await;
    let claim = p
        .claim_io(follow, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    cancel(&s, b.scope, "retention-original-terminal").await;
    age(&s, &[b.scope]).await;
    assert!(matches!(
        prepare_racing_collection(&s, &claim).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    p.retain_local_agents(None).await.unwrap();
    assert!(
        p.retire_tree(retention_root(&s, b.scope).await)
            .await
            .unwrap()
            .is_none()
    );
    let (fence, _) = s.peer().await;
    assert!(p.finish_reconciliation(fence).await.unwrap());
    let request = MappingRetirementRequest {
        nonce: Uuid::new_v4(),
        sessions: MappingRetirementSessions::parse("old mapping", vec![b.session]).unwrap(),
    };
    assert!(
        p.retired_mappings(fence, &request)
            .await
            .unwrap()
            .is_empty(),
        "a pending server approval is not local mapping retirement authority"
    );
    let command =
        super::super::approval_test_support::command(p, follow, s.fixture.target.actor, "approve")
            .await;
    p.submit_decision(command).await.unwrap();
    let claim = p
        .claim_io(follow, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    s.park(&claim).await;
    p.retain_local_agents(None).await.unwrap();
    assert!(
        p.retired_mappings(fence, &request)
            .await
            .unwrap()
            .is_empty(),
        "a queued Send unseen by the peer still pins its original mapping"
    );
    s.native_success().await;
    assert_eq!(consume_local(&s, follow).await.0, b.session);
    age(&s, &[a.scope]).await;
    p.retain_local_agents(None).await.unwrap();
    let (current, _) = s.peer().await;
    assert!(p.finish_reconciliation(current).await.unwrap());
    assert!(matches!(
        p.retired_mappings(fence, &request).await,
        Err(AppError::InvalidCredentials)
    ));
    assert_eq!(
        p.retired_mappings(current, &request)
            .await
            .unwrap()
            .as_ref(),
        &[b.session]
    );
}

#[tokio::test]
async fn cloudroom_retention_cancelled_workflow_needs_guarded_old_physical_proof_without_a_success_receipt()
 {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let claim = s.claim().await;
    s.park(&claim).await;
    let native = s.native_started().await;
    cancel(&s, s.scope, "native-retention-cancel").await;
    s.native_complete(&native).await;
    let mut tx = p.pool().begin().await.unwrap();
    let error=sqlx::query("INSERT INTO workflow_local_agent_retention_proofs(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,settled_at)
        SELECT call.company_id,call.run_id,call.execution_id,call.invocation_id,call.argument_digest,call.dispatch_id,operation.settled_at
        FROM workflow_local_agent_calls AS call JOIN local_agent_operations AS operation ON operation.company_id=call.company_id AND operation.id=call.operation_id
        WHERE call.company_id=$1 AND call.run_id=$2")
        .bind(s.scope.company.as_uuid()).bind(s.scope.run.as_uuid()).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
    age(&s, &[s.scope]).await;
    let page = p.retain_local_agents(None).await.unwrap();
    assert_eq!((page.archived_calls, page.removed_sessions), (1, 1));
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_receipts WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.scope.company.as_uuid())
    .bind(s.scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        receipts, 0,
        "physical history closure must not manufacture a successful workflow receipt"
    );
    assert!(
        p.retire_tree(retention_root(&s, s.scope).await)
            .await
            .unwrap()
            .is_some()
    );
}
