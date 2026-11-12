//! Shared real engine/approval proof seams. The remote server is a disposable fixture.
use super::*;
use crate::adapters::persistence::workflow::native_mcp_remote_fixture as remote;
use crate::application::workflow::{decisions::*, lease::*, polling::*, worker::WorkflowWorker};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(crate) async fn eventually<T>(mut check: impl AsyncFnMut() -> Option<T>) -> T {
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if let Some(value) = check().await {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("native broker deadline")
}

pub(crate) async fn worker(
    p: std::sync::Arc<PostgresPersistence>,
    url: String,
    run: crate::domain::workflow::RunId,
    cancel: CancellationToken,
) {
    let engine = crate::infra::workflow::WorkflowEngine::for_tests_with_mcp(
        p.as_ref().clone(),
        remote::guarded_client(&url),
    );
    let worker = WorkflowWorker {
        port: p.as_ref(),
        handler: engine.handler(),
        notices: None,
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: LeasePolicy::new(Duration::from_secs(30)).unwrap(),
        poll: PollPolicy::new(Duration::from_millis(50), 16).unwrap(),
        active_jobs: None,
        monitoring: None,
    };
    while !cancel.is_cancelled() {
        let page = p.poll_work(None, 16).await.unwrap();
        for candidate in page
            .candidates
            .into_iter()
            .filter(|candidate| candidate.scope.run == run)
        {
            worker.process(candidate, &cancel).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub(crate) async fn native_state(
    p: &PostgresPersistence,
    company: CompanyId,
    run: crate::domain::workflow::RunId,
) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('state',run.state,'waiting',run.waiting_reason,
        'live',public.native_mcp_run_live(run.company_id,run.id),'cancel',call.cancel_requested,
        'stop',public.native_mcp_stop_requested(operation.company_id,operation.id),
        'outcome',operation.outcome,'turn',operation.observed_turn_state,
        'operation_seconds',EXTRACT(EPOCH FROM operation.deadline-clock_timestamp()),
        'call_seconds',EXTRACT(EPOCH FROM call.deadline-clock_timestamp()),
        'machine_seconds',EXTRACT(EPOCH FROM machine.connection_expires_at-clock_timestamp()),
        'machine_state',machine.state,'machine_owner',machine.connection_owner IS NOT NULL,
        'credential_live',EXISTS(SELECT 1 FROM local_machine_credentials WHERE company_id=machine.company_id AND machine_id=machine.id AND generation=machine.credential_generation AND revoked_at IS NULL),
        'actor_live',EXISTS(SELECT 1 FROM company_members AS member JOIN principals AS principal ON principal.company_id=member.company_id AND principal.user_id=member.user_id JOIN users AS account ON account.id=member.user_id WHERE member.company_id=call.company_id AND member.user_id=call.actor_id AND member.role IN ('owner','admin') AND principal.kind='person'),
        'session_live',EXISTS(SELECT 1 FROM local_agent_sessions AS session JOIN local_machine_projects AS project ON project.company_id=session.company_id AND project.machine_id=session.machine_id AND project.project_id=session.selection->>'project_id' WHERE session.company_id=call.company_id AND session.id=call.session_id AND session.agent_revision_id=broker.agent_revision_id AND session.bundle_digest=call.harness_digest AND session.thread_id=call.native_thread_id),
        'dispatch_thread',operation.dispatch->>'thread_id','dispatch_turn',operation.dispatch->>'turn_id',
        'call_thread',call.native_thread_id,'call_turn',call.native_turn_id,
        'revision_live',public.native_mcp_revision_live(call.company_id,broker.agent_revision_id))
        FROM workflow_runs AS run JOIN native_mcp_calls AS call ON call.company_id=run.company_id AND call.id=run.native_mcp_call_id
        JOIN native_mcp_brokers AS broker ON broker.company_id=call.company_id AND broker.id=call.broker_id
        JOIN local_agent_operations AS operation ON operation.company_id=call.company_id AND operation.id=call.operation_id
        JOIN local_machines AS machine ON machine.company_id=call.company_id AND machine.id=call.machine_id
        WHERE run.company_id=$1 AND run.id=$2")
        .bind(company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap()
}

pub(crate) async fn approve(
    p: &PostgresPersistence,
    company: CompanyId,
    actor: WorkflowActor,
    run: crate::domain::workflow::RunId,
    remote: &remote::McpServer,
    hold: Duration,
) {
    let id=eventually(async || {
        let terminal:bool=sqlx::query_scalar("SELECT state IN ('succeeded','failed','cancelled') FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
        if terminal {
            let attempts:Option<Value>=sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_object('status',attempt.status,'code',attempt.workflow_failure_code,'class',attempt.workflow_failure_class,'stop',attempt.stop_reason)) FROM task_attempts AS attempt JOIN background_tasks AS job ON job.id=attempt.task_id JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE execution.company_id=$1 AND execution.run_id=$2")
                .bind(company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
            panic!("native MCP run ended before its protected approval: source={} attempts={attempts:?}",native_state(p,company,run).await);
        }
        sqlx::query_scalar::<_,Uuid>("SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2 AND submitted_at IS NULL")
            .bind(company.as_uuid()).bind(run.as_uuid()).fetch_optional(p.pool()).await.unwrap()
    }).await;
    tokio::time::sleep(hold).await;
    if !hold.is_zero() {
        let state = native_state(p, company, run).await;
        eprintln!("native approval source after held wait: {state}");
        assert_eq!(
            state["live"], true,
            "native source must stay eligible through held approval: {state}"
        );
    }
    let live:bool=sqlx::query_scalar("SELECT state='waiting' AND waiting_reason='decision' FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(live);
    assert_eq!(
        remote.calls(),
        0,
        "real protected call cannot open before approval"
    );
    let view = p
        .read_decision(company, actor, DecisionId(id))
        .await
        .unwrap();
    p.submit_decision(SubmitDecision {
        company,
        actor,
        decision: view.decision,
        command_key: DecisionCommandKey::parse("native-mcp-approval").unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest,
        choice: "approve".into(),
        feedback: String::new(),
        data: json!({}),
    })
    .await
    .unwrap();
}
