use super::*;
use crate::application::workflow::lease::{LeasePolicy, WorkflowLeaseRecovery};

#[tokio::test]
async fn channel_reply_terminal_exhausted_pending_is_effect_free_failure() {
    let (f, scope) = fixture(terminal("channel.no_reply", json!("done"))).await;
    sqlx::query("UPDATE background_tasks SET retry_count=max_retries WHERE id=$1")
        .bind(scope.job.0)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let barrier = Barrier::new(2);
    let retire = || async {
        barrier.wait().await;
        f.persistence()
            .retire_exhausted_work(scope, LeasePolicy::new(Duration::from_secs(3)).unwrap())
            .await
            .unwrap()
    };
    let (a, b) = tokio::join!(retire(), retire());
    assert_ne!(a, b);
    let saved = snapshot(&f).await;
    assert_eq!(saved["runs"][0]["state"], "failed");
    assert_eq!(saved["jobs"][0]["status"], "failed");
    assert_eq!(saved["executions"][0]["completed_at"], Value::Null);
    assert_eq!(saved["executions"][0]["response_disposition"], "automatic");
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct Effects {
    messages: i64,
    deliveries: i64,
    contexts: i64,
    runs: i64,
}

#[derive(sqlx::FromRow)]
struct Pending {
    job: Uuid,
    execution: Uuid,
}

fn terminal(kind: &str, value: Value) -> Value {
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    source["steps"]["start"]["with"] =
        json!({"value":{"literal":value},"output_schema":{"literal":true}});
    source
}

#[tokio::test]
async fn channel_reply_terminal_final_error_cannot_become_suppressed_success() {
    let mut source = terminal("channel.no_reply", json!(42));
    source["output_schema"] = json!({"type":"string"});
    source["steps"]["start"]["routes"]["error"] = json!("$end");
    let (f, scope) = fixture(source).await;
    let done = f
        .persistence()
        .advance_pure(scope, budget(4))
        .await
        .unwrap();
    assert_eq!(done.disposition, BatchDisposition::Failed);
    assert_eq!(done.response_disposition, None);
    let state = snapshot(&f).await;
    assert_eq!(state["runs"][0]["state"], "failed");
    assert_eq!(state["executions"][0]["committed_route"], "final_error");
    assert_eq!(state["executions"][0]["response_disposition"], "automatic");
    let replay = f
        .persistence()
        .advance_pure(scope, budget(4))
        .await
        .unwrap();
    assert_eq!(replay.response_disposition, None);
    assert_eq!(snapshot(&f).await, state);
}

#[tokio::test]
async fn channel_reply_terminal_preserves_business_value_and_replays_after_restart() {
    for value in [
        json!(null),
        json!(""),
        json!({}),
        json!([]),
        json!({"answer":"done","no_reply":false}),
    ] {
        let (f, scope) = fixture(terminal("channel.no_reply", value.clone())).await;
        let done = f
            .persistence()
            .advance_pure(scope, budget(4))
            .await
            .unwrap();
        assert_eq!(
            done.response_disposition,
            Some(ResponseDisposition::Suppressed)
        );
        let before = snapshot(&f).await;
        assert_eq!(before["runs"][0]["state"], "succeeded");
        assert_eq!(before["executions"][0]["committed_output"], value);
        assert_eq!(
            before["executions"][0]["response_disposition"],
            "suppressed"
        );
        let restarted = PostgresPersistence::new(f.persistence().pool().clone());
        let replay = restarted.advance_pure(scope, budget(4)).await.unwrap();
        assert_eq!(replay.disposition, BatchDisposition::Replay);
        assert_eq!(replay.response_disposition, done.response_disposition);
        assert_eq!(snapshot(&f).await, before);
        let authority: Effects = sqlx::query_as("SELECT (SELECT count(*) FROM messages) AS messages,(SELECT count(*) FROM message_deliveries) AS deliveries,(SELECT count(*) FROM channel_reply_contexts) AS contexts,(SELECT count(*) FROM channel_reply_runs) AS runs")
            .fetch_one(f.persistence().pool()).await.unwrap();
        assert_eq!(
            authority,
            Effects {
                messages: 0,
                deliveries: 0,
                contexts: 0,
                runs: 0
            }
        );
    }
}

#[tokio::test]
async fn channel_reply_terminal_ordinary_output_flags_cannot_suppress() {
    let value = json!({"no_reply":true,"response_disposition":"suppressed","answer":"done"});
    let (f, scope) = fixture(terminal("data.map", value.clone())).await;
    let done = f
        .persistence()
        .advance_pure(scope, budget(4))
        .await
        .unwrap();
    assert_eq!(
        done.response_disposition,
        Some(ResponseDisposition::Automatic)
    );
    let before = snapshot(&f).await;
    assert_eq!(before["executions"][0]["committed_output"], value);
    assert_eq!(before["executions"][0]["response_disposition"], "automatic");
    assert_eq!(
        f.persistence()
            .advance_pure(scope, budget(4))
            .await
            .unwrap()
            .response_disposition,
        done.response_disposition
    );
}

#[tokio::test]
async fn channel_reply_terminal_invalid_output_fails_or_takes_ordinary_error_route() {
    for recover in [false, true] {
        let mut source = terminal("channel.no_reply", json!(42));
        source["output_schema"] = json!({"type":"string"});
        if recover {
            source["steps"]["start"]["routes"]["error"] = json!("recover");
            source["steps"]["recover"] =
                terminal("data.map", json!("recovered"))["steps"]["start"].clone();
        }
        let (f, scope) = fixture(source).await;
        let failed = f
            .persistence()
            .advance_pure(scope, budget(4))
            .await
            .unwrap();
        assert_eq!(failed.disposition, BatchDisposition::Failed);
        assert_eq!(failed.response_disposition, None);
        let mut state = snapshot(&f).await;
        assert_eq!(state["executions"][0]["response_disposition"], "automatic");
        if recover {
            let next = sqlx::query_as::<_, Pending>(
                "SELECT id AS job,workflow_execution_id AS execution FROM background_tasks WHERE status='pending'",
            )
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
            let done = f
                .persistence()
                .advance_pure(
                    ActivationRequest {
                        job: WorkflowJobId(next.job),
                        execution: ExecutionId::new(next.execution),
                        ..scope
                    },
                    budget(4),
                )
                .await
                .unwrap();
            assert_eq!(
                done.response_disposition,
                Some(ResponseDisposition::Automatic)
            );
            state = snapshot(&f).await;
        }
        assert_eq!(
            state["runs"][0]["state"],
            if recover { "succeeded" } else { "failed" }
        );
    }
}

#[tokio::test]
async fn channel_reply_terminal_competing_completions_commit_one_disposition() {
    let (f, scope) = fixture(terminal("channel.no_reply", json!("done"))).await;
    let barrier = Barrier::new(2);
    let complete = || async {
        barrier.wait().await;
        f.persistence()
            .advance_pure(scope, budget(4))
            .await
            .unwrap()
    };
    let (a, b) = tokio::join!(complete(), complete());
    assert_ne!(a.disposition, b.disposition);
    assert_eq!(
        a.response_disposition,
        Some(ResponseDisposition::Suppressed)
    );
    assert_eq!(a.response_disposition, b.response_disposition);
    let state = snapshot(&f).await;
    assert_eq!(state["executions"].as_array().unwrap().len(), 1);
    assert_eq!(state["events"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn channel_reply_terminal_metadata_is_atomic_and_immutable() {
    let (f, scope) = fixture(terminal("channel.no_reply", json!("done"))).await;
    assert!(
        sqlx::query("UPDATE workflow_executions SET response_disposition='suppressed' WHERE id=$1")
            .bind(scope.execution.as_uuid())
            .execute(f.persistence().pool())
            .await
            .is_err()
    );
    let before = snapshot(&f).await;
    sqlx::raw_sql("CREATE FUNCTION terminal_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected terminal failure'; END $$; CREATE TRIGGER terminal_fault AFTER UPDATE ON workflow_runs FOR EACH ROW EXECUTE FUNCTION terminal_fault();")
        .execute(f.persistence().pool()).await.unwrap();
    assert!(
        f.persistence()
            .advance_pure(scope, budget(4))
            .await
            .is_err()
    );
    assert_eq!(snapshot(&f).await, before);
    sqlx::raw_sql("DROP TRIGGER terminal_fault ON workflow_runs; DROP FUNCTION terminal_fault();")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    f.persistence()
        .advance_pure(scope, budget(4))
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE workflow_executions SET response_disposition='automatic' WHERE id=$1")
            .bind(scope.execution.as_uuid())
            .execute(f.persistence().pool())
            .await
            .is_err()
    );
    assert_eq!(
        snapshot(&f).await["executions"][0]["response_disposition"],
        "suppressed"
    );
}
