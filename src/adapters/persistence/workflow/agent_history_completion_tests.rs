//! Real native model accounting, checkpoint writes and workflow controls share one exchange.
use super::*;
use crate::services::harness::AgentExecutionOutput;
use crate::services::response_contract::StructuredResponse;

async fn settled_history() -> ModelFixture {
    let mut f = opened().await;
    let p = f.native.fixture.persistence();
    let request = reservation();
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(f.write, model_request(&request), None)
        .await
        .unwrap()
    else {
        panic!("one model request must be admitted")
    };
    f.write.expected_revision = permit.consume().revision;
    let WorkflowModelSettlement::Saved(result) = p
        .settle_workflow_model(f.write, response(&request), None)
        .await
        .unwrap()
    else {
        panic!("the model response must settle")
    };
    f.run = match *result {
        WriteOutcome::Applied(run) => run,
        other => panic!("unexpected settlement {other:?}"),
    };
    f.write.expected_revision = f.run.revision;
    f
}

fn accepted(f: &ModelFixture) -> WorkflowCheckpointUpdate {
    let structured = StructuredResponse::validate(
        f.run.identity.response_contract.as_ref().unwrap(),
        r#"{"answer":[null,3.5]}"#,
        None,
        &crate::adapters::response_schema::JsonResponseValidator,
    )
    .unwrap()
    .unwrap();
    WorkflowCheckpointUpdate::FinalOutput(Box::new(AgentExecutionOutput {
        content: structured.body().into(),
        structured: Some(structured),
        metadata: None,
        token_usage: Default::default(),
    }))
}

async fn exchange(f: &ModelFixture) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(exchange) FROM agent_history_exchanges AS exchange WHERE company_id=$1 AND execution_id=$2")
        .bind(f.write.fence.scope.company.as_uuid()).bind(f.write.fence.scope.execution.as_uuid())
        .fetch_one(f.native.fixture.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn agent_history_completion_competing_finalizers_and_durable_replay_keep_one_answer() {
    let f = Box::pin(settled_history()).await;
    let p = f.native.fixture.persistence();
    let update = accepted(&f);
    let (a, b) = tokio::join!(
        p.update_workflow_checkpoint(f.write, update.clone()),
        p.update_workflow_checkpoint(f.write, update.clone())
    );
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|v| matches!(v, WriteOutcome::Applied(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|v| matches!(v, WriteOutcome::AlreadyApplied(_)))
            .count(),
        1
    );
    let saved = exchange(&f).await;
    assert_eq!(saved["id"], json!(f.write.fence.scope.execution.as_uuid()));
    assert_eq!(saved["checkpoint_id"], json!(f.run.run_id.0));
    assert_eq!(saved["state"], "completed");
    assert_eq!(saved["output"], json!({"answer":[null,3.5]}));
    assert!(matches!(
        p.update_workflow_checkpoint(f.write, update).await.unwrap(),
        WriteOutcome::AlreadyApplied(_)
    ));
    assert_eq!(exchange(&f).await, saved);
    assert_eq!(accounting(&f).await["calls"], 1);
}

#[tokio::test]
async fn agent_history_completion_rollback_keeps_checkpoint_and_exchange_active() {
    let f = Box::pin(settled_history()).await;
    let p = f.native.fixture.persistence();
    let before = exchange(&f).await;
    let checkpoint = stored(&f.native.fixture, f.run.run_id.0).await;
    sqlx::raw_sql("CREATE FUNCTION history_commit_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected history commit failure'; END $$; CREATE CONSTRAINT TRIGGER history_commit_fault AFTER UPDATE ON task_harness_runs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION history_commit_fault();")
        .execute(p.pool()).await.unwrap();
    assert!(
        p.update_workflow_checkpoint(f.write, accepted(&f))
            .await
            .is_err()
    );
    sqlx::raw_sql("DROP TRIGGER history_commit_fault ON task_harness_runs; DROP FUNCTION history_commit_fault();").execute(p.pool()).await.unwrap();
    assert_eq!(exchange(&f).await, before);
    assert_eq!(stored(&f.native.fixture, f.run.run_id.0).await, checkpoint);
    assert!(matches!(
        p.update_workflow_checkpoint(f.write, accepted(&f))
            .await
            .unwrap(),
        WriteOutcome::Applied(_)
    ));
    assert_eq!(accounting(&f).await["calls"], 1);
}

async fn raise(f: &ModelFixture, level: i16) {
    sqlx::query("UPDATE workflow_runs SET content_level=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.write.fence.scope.company.as_uuid())
        .bind(f.write.fence.scope.run.as_uuid())
        .bind(level)
        .execute(f.native.fixture.persistence().pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn agent_history_level_enqueue_and_final_acceptance_only_raise_the_stamp() {
    let native = model_support::fixture(Fixture::new().await, lease_source()).await;
    // Preparation has already captured the original level; a later observation is joined at completion.
    let f = Box::pin(open_native(native)).await;
    raise(&f, 5).await;
    let request = reservation();
    let WorkflowModelAdmission::New(permit) = f
        .native
        .fixture
        .persistence()
        .reserve_workflow_model(f.write, model_request(&request), None)
        .await
        .unwrap()
    else {
        panic!("not admitted")
    };
    let mut write = WorkflowCheckpointWrite {
        expected_revision: permit.consume().revision,
        ..f.write
    };
    let WorkflowModelSettlement::Saved(result) = f
        .native
        .fixture
        .persistence()
        .settle_workflow_model(write, response(&request), None)
        .await
        .unwrap()
    else {
        panic!("not settled")
    };
    let WriteOutcome::Applied(run) = *result else {
        panic!("not saved")
    };
    write.expected_revision = run.revision;
    f.native
        .fixture
        .persistence()
        .update_workflow_checkpoint(write, accepted(&f))
        .await
        .unwrap();
    assert_eq!(exchange(&f).await["access_level"], 5);
    let error =
        sqlx::query("UPDATE agent_history_exchanges SET access_level=10 WHERE company_id=$1")
            .bind(write.fence.scope.company.as_uuid())
            .execute(f.native.fixture.persistence().pool())
            .await
            .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
}

#[path = "agent_history_lifecycle_tests.rs"]
mod lifecycle;

#[tokio::test]
async fn agent_history_level_competing_observation_commit_precedes_the_output_stamp() {
    let f = Box::pin(settled_history()).await;
    let p = f.native.fixture.persistence();
    let scope = f.write.fence.scope;
    let mut observation = p.pool().begin().await.unwrap();
    let observer: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *observation)
        .await
        .unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut observation,
        scope.company,
        scope.run,
        crate::entities::access_level::AccessLevel::L5,
    )
    .await
    .unwrap();
    let release = async {
        tokio::time::timeout(Duration::from_secs(3),async {
            loop {
                let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity AS waiting WHERE waiting.datname=current_database() AND waiting.wait_event_type='Lock' AND $1=ANY(pg_blocking_pids(waiting.pid)))")
                    .bind(observer).fetch_one(p.pool()).await.unwrap();
                if blocked {break;}
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("finalization must contend with the actual observation lock");
        observation.commit().await.unwrap();
    };
    let (completed, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            Box::pin(p.update_workflow_checkpoint(f.write, accepted(&f))),
            release
        )
    })
    .await
    .unwrap();
    assert!(matches!(completed.unwrap(), WriteOutcome::Applied(_)));
    assert_eq!(exchange(&f).await["access_level"], 5);
    assert_eq!(exchange(&f).await["state"], "completed");
    assert_eq!(accounting(&f).await["calls"], 1);
}

#[tokio::test]
async fn agent_history_storage_repeated_step_admission_has_new_activation_in_same_private_conversation()
 {
    use crate::application::workflow::{
        agent::AgentPreparation, agent_history::AgentHistorySessionRef,
        completion::WorkflowCompletion,
    };
    let f = Box::pin(settled_history()).await;
    let fixture = &f.native.fixture;
    let p = fixture.persistence();
    let update = accepted(&f);
    let WorkflowCheckpointUpdate::FinalOutput(ref output) = update else {
        unreachable!()
    };
    let logical: Value = serde_json::from_str(&output.content).unwrap();
    p.update_workflow_checkpoint(f.write, update).await.unwrap();
    p.complete_io(FencedWorkflowResult {
        fence: f.write.fence,
        output: logical,
        policy: f.write.lease_policy,
    })
    .await
    .unwrap()
    .unwrap();
    let mut request = fixture.request("repeated-step", fixture.manual());
    request.history_session = Some(AgentHistorySessionRef {
        from_run: f.write.fence.scope.run,
    });
    let command = fixture.prepare(request).await;
    p.admit(&command).await.unwrap();
    let job: Uuid = sqlx::query_scalar(
        "SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.first_execution_id().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let scope = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    let claim = p
        .claim_io(scope, worker(), f.write.lease_policy)
        .await
        .unwrap()
        .unwrap();
    p.prepare_agent(&claim, f.write.lease_policy)
        .await
        .unwrap()
        .expect_ready();
    p.prepare_agent(&claim, f.write.lease_policy)
        .await
        .unwrap()
        .expect_ready();
    let saved:Value=sqlx::query_scalar("SELECT jsonb_build_object('positions',jsonb_agg(position ORDER BY position),'executions',count(DISTINCT execution_id),'conversations',count(DISTINCT conversation_id)) FROM agent_history_exchanges WHERE company_id=$1")
        .bind(scope.company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        saved,
        json!({"positions":[1,2],"executions":2,"conversations":1})
    );
    let steps: Vec<String> = sqlx::query_scalar(
        "SELECT step_id FROM workflow_executions WHERE company_id=$1 ORDER BY run_id",
    )
    .bind(scope.company.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(steps, vec!["start", "start"]);
    assert_eq!(accounting(&f).await["calls"], 1);
}
