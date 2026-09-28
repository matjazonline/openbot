use super::*;
#[path = "batch_recovery_tests.rs"]
mod recovery_tests;
#[path = "wakeup_tests.rs"]
mod wakeup_tests;
use crate::application::workflow::{activation::*, batch::*};
use std::time::Duration;
use tokio::sync::Barrier;

fn budget(steps: u8) -> BatchBudget {
    BatchBudget::new(steps, Duration::from_secs(5)).unwrap()
}
fn source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["output_schema"] = json!(true);
    source["input_schema"] =
        json!({"type":"object","properties":{"value":true},"required":["value"]});
    source["steps"]["start"]["with"] =
        json!({"value":{"ref":"/input/value"},"output_schema":{"literal":true}});
    source["steps"]["start"]["routes"] = json!({"success":"rule"});
    source["steps"]["rule"] = json!({"type":"decision.rule", "with":{"data":{"literal":null},"data_schema":{"literal":true}},
        "rule":{"cases":[{"when":{"exists":"/steps/start/output"},"choice":"yes"},{"when":{"literal":true},"choice":"no"}],"default":"no"},
        "routes":{"choices":{"yes":"finish","no":"$end"}}});
    source["steps"]["finish"] = json!({"type":"data.map","with":{"value":{"ref":"/steps/rule/output/data"},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    source
}
async fn fixture(source: Value) -> (AdmissionFixture, ActivationRequest) {
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(source).await).await;
    let command = f.prepare(f.request("batch", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let job =
        sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id = $1")
            .bind(command.first_execution_id().as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    let request = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    (f, request)
}
async fn snapshot(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('executions',(SELECT jsonb_agg(to_jsonb(execution) ORDER BY activation) FROM workflow_executions AS execution),'jobs',(SELECT jsonb_agg(to_jsonb(job) ORDER BY id) FROM background_tasks AS job),'events',(SELECT jsonb_agg(to_jsonb(event) ORDER BY sequence) FROM workflow_run_events AS event),'runs',(SELECT jsonb_agg(to_jsonb(run)) FROM workflow_runs AS run))")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_batch_records_map_rule_end_and_null_output() {
    let (f, request) = fixture(source()).await;
    let result = f
        .persistence()
        .advance_pure(request, budget(64))
        .await
        .unwrap();
    assert_eq!(result.disposition, BatchDisposition::Completed);
    assert_eq!(result.completed, 3);
    let state = snapshot(&f).await;
    assert_eq!(state["runs"][0]["state"], "succeeded");
    assert_eq!(state["executions"][0]["frozen_inputs"]["value"], 1);
    assert_eq!(state["executions"][0]["committed_output"], 1);
    assert_eq!(state["executions"][1]["frozen_choice"], "yes");
    assert_eq!(
        state["executions"][1]["committed_output"],
        json!({"choice":"yes","data":null})
    );
    assert_eq!(state["executions"][1]["committed_route"], "choice:yes");
    assert_eq!(state["executions"][2]["committed_output"], Value::Null);
    assert_eq!(state["events"].as_array().unwrap().len(), 4);
    for job in state["jobs"].as_array().unwrap() {
        assert_eq!(job["status"], "completed");
        assert_eq!(job["payload"].as_object().unwrap().len(), 2);
    }
    let replay = f
        .persistence()
        .advance_pure(request, budget(64))
        .await
        .unwrap();
    assert_eq!(replay.disposition, BatchDisposition::Replay);
    assert_eq!(snapshot(&f).await, state);
}

#[tokio::test]
async fn workflow_batch_yield_replay_and_resume_once() {
    let (f, request) = fixture(source()).await;
    let first = f
        .persistence()
        .advance_pure(request, budget(1))
        .await
        .unwrap();
    assert_eq!(first.disposition, BatchDisposition::Yielded);
    let next = first.continuation.unwrap();
    let before = snapshot(&f).await;
    assert_eq!(
        before["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|j| j["status"] == "pending")
            .count(),
        1
    );
    let replay = f
        .persistence()
        .advance_pure(request, budget(64))
        .await
        .unwrap();
    assert_eq!(replay.continuation, Some(next));
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(
        f.persistence()
            .advance_pure(next, budget(64))
            .await
            .unwrap()
            .completed,
        2
    );
}

#[tokio::test]
async fn workflow_batch_real_competitors_make_one_progression() {
    let (f, request) = fixture(source()).await;
    let barrier = Barrier::new(2);
    let advance = || async {
        barrier.wait().await;
        f.persistence()
            .advance_pure(request, budget(1))
            .await
            .unwrap()
    };
    let (a, b) = tokio::join!(advance(), advance());
    assert_eq!(a.completed + b.completed, 1);
    assert!(matches!(
        (&a.disposition, &b.disposition),
        (BatchDisposition::Yielded, BatchDisposition::Replay)
            | (BatchDisposition::Replay, BatchDisposition::Yielded)
    ));
    assert_eq!(a.continuation, b.continuation);
    let state = snapshot(&f).await;
    assert_eq!(state["executions"].as_array().unwrap().len(), 2);
    assert_eq!(state["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(state["events"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn workflow_batch_nonpure_boundary_is_unactivated_pending() {
    let mut source = source();
    let example: Value =
        serde_json::from_str(&registry::example("wait.timer").unwrap().source).unwrap();
    source["steps"]["rule"] = example["steps"]["start"].clone();
    source["steps"].as_object_mut().unwrap().remove("finish");
    let (f, request) = fixture(source).await;
    let result = f
        .persistence()
        .advance_pure(request, budget(64))
        .await
        .unwrap();
    assert_eq!(result.disposition, BatchDisposition::Boundary);
    assert_eq!(result.completed, 1);
    let state = snapshot(&f).await;
    assert_eq!(state["executions"][1]["activated_at"], Value::Null);
    let before = state;
    assert_eq!(
        f.persistence()
            .advance_pure(result.continuation.unwrap(), budget(64))
            .await
            .unwrap()
            .completed,
        0
    );
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
async fn workflow_batch_failures_rollback_at_each_write_boundary() {
    for (table, condition) in [
        ("workflow_executions", "NEW.activated_at IS NOT NULL"),
        ("workflow_executions", "NEW.completed_at IS NOT NULL"),
        ("workflow_run_events", "NEW.sequence > 1"),
        ("workflow_executions", "NEW.activation > 1"),
    ] {
        let (f, request) = fixture(source()).await;
        let before = snapshot(&f).await;
        sqlx::raw_sql(&format!("CREATE FUNCTION batch_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {condition} THEN RAISE EXCEPTION 'injected batch failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER batch_fault AFTER INSERT OR UPDATE ON {table} FOR EACH ROW EXECUTE FUNCTION batch_fault();"))
            .execute(f.persistence().pool()).await.unwrap();
        assert!(
            f.persistence()
                .advance_pure(request, budget(64))
                .await
                .is_err()
        );
        assert_eq!(snapshot(&f).await, before);
        sqlx::raw_sql(&format!(
            "DROP TRIGGER batch_fault ON {table}; DROP FUNCTION batch_fault();"
        ))
        .execute(f.persistence().pool())
        .await
        .unwrap();
        assert_eq!(
            f.persistence()
                .advance_pure(request, budget(64))
                .await
                .unwrap()
                .completed,
            3
        );
    }
}

#[tokio::test]
async fn workflow_batch_invalid_scope_lease_due_state_and_bounds_fail_closed() {
    let (f, request) = fixture(source()).await;
    let before = snapshot(&f).await;
    for bad in [
        ActivationRequest {
            company: CompanyId::new(Uuid::new_v4()),
            ..request
        },
        ActivationRequest {
            run: RunId::new(Uuid::new_v4()),
            ..request
        },
        ActivationRequest {
            execution: ExecutionId::new(Uuid::new_v4()),
            ..request
        },
        ActivationRequest {
            job: WorkflowJobId(Uuid::new_v4()),
            ..request
        },
    ] {
        assert!(f.persistence().advance_pure(bad, budget(64)).await.is_err());
        assert_eq!(snapshot(&f).await, before);
    }
    for statement in [
        "UPDATE background_tasks SET run_at = clock_timestamp() + interval '1 hour'",
        "UPDATE background_tasks SET status = 'processing', worker_id = gen_random_uuid(), execution_generation = gen_random_uuid(), locked_at = clock_timestamp(), lock_expires_at = clock_timestamp() + interval '1 hour'",
        "UPDATE workflow_runs SET state = 'cancelled'",
        "UPDATE workflow_runs SET state = 'waiting',waiting_reason = 'timer'",
        "UPDATE workflow_runs SET max_steps = 1",
    ] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::raw_sql(statement).execute(&mut *tx).await.unwrap();
        assert!(
            crate::adapters::persistence::workflow::batch::advance_on(&mut tx, request, budget(64))
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        assert_eq!(snapshot(&f).await, before);
    }
    assert!(BatchBudget::new(0, Duration::from_secs(1)).is_err());
    assert!(BatchBudget::new(65, Duration::from_secs(1)).is_err());
    assert!(BatchBudget::new(1, Duration::ZERO).is_err());
    assert!(BatchBudget::new(1, Duration::from_secs(6)).is_err());
}

#[tokio::test]
async fn workflow_batch_frozen_rule_choice_and_default_survive_replay() {
    for predicate in [true, false] {
        let mut source = source();
        source["steps"]["rule"]["rule"]["cases"] = json!([{"when":{"eq":[{"ref":"/steps/start/output"},{"literal":predicate}]},"choice":"yes"}]);
        // Output is 1, so neither boolean equals it: the declared default wins.
        let (f, request) = fixture(source).await;
        let next = f
            .persistence()
            .advance_pure(request, budget(1))
            .await
            .unwrap()
            .continuation
            .unwrap();
        let frozen = f.persistence().activate(next).await.unwrap();
        assert_eq!(frozen.choice.as_ref().unwrap().as_str(), "no");
        // A later available historical execution cannot change the frozen rule.
        let before = snapshot(&f).await;
        sqlx::query("UPDATE workflow_runs SET input = '{\"value\":true}'::jsonb WHERE id = $1")
            .bind(request.run.as_uuid())
            .execute(f.persistence().pool())
            .await
            .unwrap();
        assert_eq!(f.persistence().activate(next).await.unwrap(), frozen);
        let result = f
            .persistence()
            .advance_pure(next, budget(64))
            .await
            .unwrap();
        assert_eq!(result.completed, 1);
        let state = snapshot(&f).await;
        assert_eq!(state["executions"][1]["committed_route"], "choice:no");
        assert_eq!(
            state["executions"][1]["frozen_inputs"],
            before["executions"][1]["frozen_inputs"]
        );
        assert_eq!(
            f.persistence()
                .advance_pure(next, budget(64))
                .await
                .unwrap()
                .disposition,
            BatchDisposition::Replay
        );
    }
}

#[tokio::test]
async fn workflow_batch_elapsed_budget_yields_and_deadline_rolls_back() {
    let (f, request) = fixture(source()).await;
    sqlx::raw_sql("CREATE FUNCTION batch_delay() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(0.03); RETURN NEW; END $$; CREATE TRIGGER batch_delay AFTER UPDATE OF completed_at ON workflow_executions FOR EACH ROW EXECUTE FUNCTION batch_delay();")
        .execute(f.persistence().pool()).await.unwrap();
    let tiny = BatchBudget::new(64, Duration::from_millis(1)).unwrap();
    let result = f.persistence().advance_pure(request, tiny).await.unwrap();
    assert_eq!(result.disposition, BatchDisposition::Yielded);
    assert_eq!(result.completed, 1);
    let next = result.continuation.unwrap();
    sqlx::query("UPDATE workflow_runs SET deadline = clock_timestamp() + interval '10 milliseconds' WHERE id = $1").bind(request.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let before = snapshot(&f).await;
    assert!(
        f.persistence()
            .advance_pure(next, budget(64))
            .await
            .is_err()
    );
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
async fn workflow_batch_run_lock_contention_cannot_cross_deadline_or_timeout() {
    let (f, request) = fixture(source()).await;
    sqlx::query("UPDATE workflow_runs SET deadline = clock_timestamp() + interval '200 milliseconds' WHERE id = $1").bind(request.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let before = snapshot(&f).await;
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id = $1 FOR UPDATE")
        .bind(request.run.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let advance = f.persistence().advance_pure(request, budget(64));
    tokio::pin!(advance);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut advance)
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(210)).await;
    blocker.commit().await.unwrap();
    assert!(advance.await.is_err());
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
async fn workflow_batch_output_schema_invalidates_entire_transaction() {
    let mut source = source();
    source["output_schema"] = json!({"type":"string"});
    let (f, request) = fixture(source).await;
    let before = snapshot(&f).await;
    assert!(
        f.persistence()
            .advance_pure(request, budget(64))
            .await
            .is_err()
    );
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
async fn workflow_batch_sql_rejects_mutated_choice_route_successor_and_duplicate_job() {
    let (f, request) = fixture(source()).await;
    f.persistence()
        .advance_pure(request, budget(64))
        .await
        .unwrap();
    let before = snapshot(&f).await;
    for statement in [
        "UPDATE workflow_executions SET frozen_choice = 'no' WHERE step_id = 'rule'",
        "UPDATE workflow_executions SET committed_route = 'choice:no' WHERE step_id = 'rule'",
        "UPDATE workflow_executions SET route_target = '$end',successor_execution_id = NULL WHERE step_id = 'start'",
        "UPDATE workflow_executions SET successor_execution_id = gen_random_uuid() WHERE step_id = 'start'",
        "INSERT INTO background_tasks (company_id,channel_id,thread_id,correlation_id,task_type,payload,queue_kind,workflow_execution_id) SELECT company_id,channel_id,thread_id,correlation_id,task_type,payload,queue_kind,workflow_execution_id FROM background_tasks LIMIT 1",
        "INSERT INTO workflow_executions (company_id,run_id,id,step_id,activation) SELECT company_id,run_id,gen_random_uuid(),step_id,activation FROM workflow_executions LIMIT 1",
    ] {
        assert!(
            sqlx::raw_sql(statement)
                .execute(f.persistence().pool())
                .await
                .is_err(),
            "{statement}"
        );
        assert_eq!(snapshot(&f).await, before);
    }
}
