use super::*;
use crate::application::local_agents::management::{
    ControlSession, LocalAgentManagementStore, SessionCommand,
};
use crate::application::workflow::polling::{PollPolicy, WorkflowFairPolling};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

fn runner<'a, H: WorkflowHandler>(
    p: &'a PostgresPersistence,
    handler: &'a H,
) -> WorkflowWorker<'a, PostgresPersistence, H> {
    WorkflowWorker {
        port: p,
        handler,
        notices: None,
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: lease(),
        poll: PollPolicy::new(Duration::from_millis(50), 8).unwrap(),
        active_jobs: None,
        monitoring: None,
    }
}

async fn finish(s: &Setup, expected: &str, count: i64) {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE state=$1")
                .bind(expected)
                .fetch_one(s.fixture.persistence.pool())
                .await
                .unwrap();
            if rows == count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("actual production worker must discover and settle local work before its deadline");
}

#[tokio::test]
async fn cloudroom_workflow_actual_fair_poll_worker_discovers_native_result_and_completes_once() {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let claim = s.claim().await;
    s.park(&claim).await;
    assert!(
        p.poll_fair(WorkflowWorkerId(Uuid::new_v4()), 8)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    s.native_success().await;
    let page = p
        .poll_fair(WorkflowWorkerId(Uuid::new_v4()), 8)
        .await
        .unwrap();
    assert_eq!(page.candidates.len(), 1);
    assert_eq!(page.candidates[0].scope, s.scope);
    let handler = s.handler();
    let worker = runner(p, &handler);
    let wake = Notify::new();
    let stop = CancellationToken::new();
    tokio::join!(worker.run(&wake, &stop), async {
        finish(&s, "succeeded", 1).await;
        stop.cancel();
    });
    assert!(
        p.poll_fair(WorkflowWorkerId(Uuid::new_v4()), 8)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    assert_eq!(
        s.rows().await,
        json!({"calls":1,"operations":1,"intents":1,"markers":1,"receipts":1,"waits":1})
    );
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 2, "one parked attempt and one fresh result claim");
}

#[tokio::test]
async fn cloudroom_workflow_first_uncertain_disposition_survives_late_success_before_and_after_wake()
 {
    for after_wake in [false, true] {
        let s = Setup::new().await;
        let p = &s.fixture.persistence;
        let claim = s.claim().await;
        s.park(&claim).await;
        let native = s.native_started().await;
        s.native_observe(&native, native.sequence + 1, TurnState::Missing)
            .await;
        assert_eq!(
            p.machine_runtime(
                s.fixture.target.actor.user_id(),
                s.scope.company.as_uuid(),
                s.profile.selection.machine_id
            )
            .await
            .unwrap()
            .unsettled_effects,
            1
        );
        if after_wake {
            assert!(matches!(
                p.resume_wait(s.scope).await.unwrap(),
                WaitProgress::Continued
            ));
        }
        s.native_observe(&native, native.sequence + 2, TurnState::Completed)
            .await;
        assert_eq!(
            p.machine_runtime(
                s.fixture.target.actor.user_id(),
                s.scope.company.as_uuid(),
                s.profile.selection.machine_id
            )
            .await
            .unwrap()
            .unsettled_effects,
            0
        );
        let handler = s.handler();
        let worker = runner(p, &handler);
        let wake = Notify::new();
        let stop = CancellationToken::new();
        tokio::join!(worker.run(&wake, &stop), async {
            finish(&s, "failed", 1).await;
            stop.cancel();
        });
        let result:Value=sqlx::query_scalar("SELECT jsonb_build_object('workflow',call.workflow_outcome,'native',operation.outcome,'text',operation.output,'receipt',receipt.result->>'status') FROM workflow_local_agent_calls AS call JOIN local_agent_operations AS operation ON operation.company_id=call.company_id AND operation.id=call.operation_id JOIN workflow_action_receipts AS receipt ON receipt.company_id=call.company_id AND receipt.invocation_id=call.invocation_id")
            .fetch_one(p.pool()).await.unwrap();
        assert_eq!(
            result,
            json!({"workflow":"uncertain","native":"succeeded","text":"native output","receipt":"uncertain"})
        );
        let messages: i64 = sqlx::query_scalar("SELECT count(*) FROM message_deliveries")
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(messages, 0);
    }
}

async fn advance_clock(s: &Setup) {
    let p = s.fixture.persistence.pool();
    // Only this owned test database shadows SQL's clock. Frozen operations/digests
    // and all production guards remain unchanged while local deadlines become due.
    sqlx::raw_sql("CREATE SCHEMA cloudroom_test_clock; CREATE FUNCTION cloudroom_test_clock.clock_timestamp() RETURNS timestamptz LANGUAGE sql VOLATILE AS $$ SELECT pg_catalog.clock_timestamp()+interval '31 minutes' $$;")
        .execute(p).await.unwrap();
    let mut connections = Vec::new();
    for _ in 0..p.options().get_max_connections() {
        let mut connection = p.acquire().await.unwrap();
        sqlx::query("SET search_path=cloudroom_test_clock,public,pg_catalog")
            .execute(&mut *connection)
            .await
            .unwrap();
        connections.push(connection);
    }
    drop(connections);
}

struct NativeQueue {
    scopes: Vec<ActivationRequest>,
    native: Vec<NativeTurn>,
    sequence: u64,
}

async fn eight_started(s: &Setup) -> NativeQueue {
    let p = &s.fixture.persistence;
    let mut scopes = vec![s.scope];
    for _ in 1..8 {
        scopes.push(s.activate(s.fixture.target.actor,json!({"profile_id":{"literal":s.profile.id},"prompt":{"literal":"bounded native task"}}),RelatedAssociation::Company).await);
    }
    for scope in &scopes {
        let claim = p
            .claim_io(*scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .unwrap();
        s.park(&claim).await;
    }
    let (fence, mut sequence) = s.peer().await;
    assert!(p.finish_reconciliation(fence).await.unwrap());
    let deliveries = p.pending_commands(fence).await.unwrap();
    let mut native = Vec::new();
    for delivery in deliveries
        .iter()
        .filter(|delivery| delivery.operation.command().is_effect())
    {
        let turn = s
            .native_received(fence, &delivery.operation, sequence)
            .await;
        sequence = turn.sequence;
        native.push(turn);
    }
    assert_eq!(native.len(), 8);
    NativeQueue {
        scopes,
        native,
        sequence,
    }
}

async fn expire_with_worker(s: &Setup, scopes: &[ActivationRequest]) {
    let p = &s.fixture.persistence;
    assert!(matches!(
        p.resume_wait(scopes[0]).await.unwrap(),
        WaitProgress::Continued
    ));
    let first:String=sqlx::query_scalar("SELECT operation.outcome FROM local_agent_operations AS operation JOIN workflow_local_agent_calls AS call ON call.company_id=operation.company_id AND call.operation_id=operation.id WHERE call.run_id=$1")
        .bind(scopes[0].run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        first, "uncertain",
        "the workflow expiry writer must preserve possibly running native work"
    );
    let handler = s.handler();
    let worker = runner(p, &handler);
    let wake = Notify::new();
    let stop = CancellationToken::new();
    tokio::join!(worker.run(&wake, &stop), async {
        p.expire_operations(100).await.unwrap();
        finish(s, "failed", 8).await;
        stop.cancel();
    });
}

#[tokio::test]
async fn cloudroom_workflow_competing_expiry_keeps_eight_attempted_native_effects_until_positive_evidence()
 {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let NativeQueue {
        scopes,
        mut native,
        sequence,
    } = eight_started(&s).await;
    advance_clock(&s).await;
    expire_with_worker(&s, &scopes).await;
    let runtime = p
        .machine_runtime(
            s.fixture.target.actor.user_id(),
            s.scope.company.as_uuid(),
            s.profile.selection.machine_id,
        )
        .await
        .unwrap();
    assert_eq!(runtime.unsettled_effects, 8);
    let outcomes: Vec<String> = sqlx::query_scalar(
        "SELECT workflow_outcome FROM workflow_local_agent_calls ORDER BY invocation_id",
    )
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(outcomes, vec!["expired"; 8]);
    let request = ControlSession {
        actor: s.fixture.target.actor.user_id(),
        company: s.scope.company.as_uuid(),
        session_id: SessionId::new(native[0].operation.as_uuid()).unwrap(),
        request_id: OperationId::new(Uuid::now_v7()).unwrap(),
        command: SessionCommand::Send {
            prompt: LocalPrompt::try_from("explicit ninth effect".to_owned()).unwrap(),
        },
    };
    assert!(matches!(
        p.control_session(&request).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(s.rows().await["operations"], 8);
    let (replacement, _) = s.peer().await;
    let mut turn = native.remove(0);
    turn.fence = replacement;
    s.native_observe(&turn, sequence + 1, TurnState::Completed)
        .await;
    assert_eq!(
        p.machine_runtime(
            s.fixture.target.actor.user_id(),
            s.scope.company.as_uuid(),
            s.profile.selection.machine_id
        )
        .await
        .unwrap()
        .unsettled_effects,
        7
    );
    assert!(
        p.control_session(&request).await.is_ok(),
        "the exact ninth request can admit only after positive physical-terminal evidence frees a slot"
    );
    assert_eq!(s.rows().await["operations"], 9);
    assert!(
        p.claim_io(s.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none()
    );
    let receipt: String = sqlx::query_scalar(
        "SELECT result->>'status' FROM workflow_action_receipts WHERE invocation_id=$1",
    )
    .bind(turn.operation.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(receipt, "expired");
}
