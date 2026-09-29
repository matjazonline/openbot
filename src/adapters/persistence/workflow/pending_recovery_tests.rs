use super::*;
use crate::application::workflow::{lease::*, polling::*};

#[tokio::test]
async fn workflow_poison_competing_batches_settle_once_and_stop_polling() {
    let mut source = source();
    source["output_schema"] = json!({"type":"string"});
    let (f, scope) = fixture(source).await;
    let barrier = Barrier::new(2);
    let run = || async {
        barrier.wait().await;
        f.persistence().advance_pure(scope, budget(64)).await
    };
    let (a, b) = tokio::join!(run(), run());
    assert!(a.is_ok() || b.is_ok());
    let saved = snapshot(&f).await;
    assert_eq!(
        saved["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["event_kind"] == "workflow.invalid_output")
            .count(),
        1
    );
    for _ in 0..2 {
        assert!(
            f.persistence()
                .poll_work(None, 128)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
    }
    assert_eq!(snapshot(&f).await, saved);
}

#[tokio::test]
async fn workflow_poison_failure_commit_abort_and_lost_ack_preserve_results() {
    let mut source = source();
    source["output_schema"] = json!({"type":"string"});
    let (f, scope) = fixture(source).await;
    let before = snapshot(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let result =
        crate::adapters::persistence::workflow::batch::advance_on(&mut tx, scope, budget(64))
            .await
            .unwrap();
    assert_eq!(result.disposition, BatchDisposition::Failed);
    tx.rollback().await.unwrap();
    assert_eq!(snapshot(&f).await, before);
    sqlx::raw_sql("CREATE FUNCTION poison_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_kind='workflow.invalid_output' THEN RAISE EXCEPTION 'injected'; END IF; RETURN NEW; END $$; CREATE TRIGGER poison_fault AFTER INSERT ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION poison_fault();").execute(f.persistence().pool()).await.unwrap();
    assert!(
        f.persistence()
            .advance_pure(scope, budget(64))
            .await
            .is_err()
    );
    assert_eq!(snapshot(&f).await, before);
    sqlx::raw_sql(
        "DROP TRIGGER poison_fault ON workflow_run_events; DROP FUNCTION poison_fault();",
    )
    .execute(f.persistence().pool())
    .await
    .unwrap();
    f.persistence()
        .advance_pure(scope, budget(64))
        .await
        .unwrap();
    let saved = snapshot(&f).await;
    let replay = PostgresPersistence::new(f.persistence().pool().clone());
    replay.advance_pure(scope, budget(64)).await.unwrap();
    assert_eq!(snapshot(&f).await, saved);
}

#[tokio::test]
async fn workflow_poison_activation_budget_is_durable() {
    let (f, scope) = fixture(source()).await;
    sqlx::query("UPDATE workflow_runs SET max_steps=1 WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        f.persistence()
            .advance_pure(scope, budget(64))
            .await
            .unwrap()
            .disposition,
        BatchDisposition::Failed
    );
    assert_eq!(snapshot(&f).await["runs"][0]["state"], "failed");
    assert!(
        f.persistence()
            .poll_work(None, 128)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
}

#[tokio::test]
async fn workflow_poison_exhausted_pending_competitors_keep_allowance() {
    let (f, scope) = fixture(source()).await;
    sqlx::query("UPDATE background_tasks SET retry_count=max_retries WHERE id=$1")
        .bind(scope.job.0)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let before = snapshot(&f).await;
    let page = f.persistence().poll_work(None, 128).await.unwrap();
    assert_eq!(page.candidates[0].work, PollWork::ExhaustedPending);
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
    assert_eq!(
        saved["jobs"][0]["retry_count"],
        before["jobs"][0]["retry_count"]
    );
    assert_eq!(saved["jobs"][0]["status"], "failed");
    for _ in 0..2 {
        assert!(
            f.persistence()
                .poll_work(None, 128)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
    }
}

#[tokio::test]
async fn workflow_poison_invalid_activation_input_is_not_reclaimed() {
    let (f, scope) = fixture(source()).await;
    sqlx::query("UPDATE workflow_runs SET input='{}'::jsonb WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        f.persistence()
            .advance_pure(scope, budget(64))
            .await
            .unwrap()
            .disposition,
        BatchDisposition::Failed
    );
    let saved = snapshot(&f).await;
    assert_eq!(saved["executions"][0]["frozen_inputs"], Value::Null);
    assert_eq!(saved["jobs"][0]["status"], "failed");
    assert_eq!(saved["runs"][0]["state"], "failed");
    assert!(
        f.persistence()
            .poll_work(None, 128)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
}

#[tokio::test]
async fn workflow_poison_final_error_end_commits_once() {
    let mut source = source();
    source["output_schema"] = json!({"type":"string"});
    source["steps"]["finish"]["routes"]["error"] = json!("$end");
    let (f, scope) = fixture(source).await;
    let done = f
        .persistence()
        .advance_pure(scope, budget(64))
        .await
        .unwrap();
    assert_eq!(done.disposition, BatchDisposition::Failed);
    let saved = snapshot(&f).await;
    assert_eq!(saved["executions"][2]["committed_route"], "final_error");
    f.persistence()
        .advance_pure(done.last, budget(64))
        .await
        .unwrap();
    assert_eq!(snapshot(&f).await, saved);
}

#[tokio::test]
async fn workflow_poison_full_page_and_final_error_branch() {
    let mut source = source();
    source["output_schema"] = json!({"type":"string"});
    source["steps"]["finish"]["routes"]["error"] = json!("recover");
    source["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"literal":"recovered"},"output_schema":{"literal":{"type":"string"}}},"routes":{"success":"$end"}});
    let (f, scope) = fixture(source).await;
    let command = f.prepare(f.request("second-poison", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let page = f.persistence().poll_work(None, 2).await.unwrap();
    assert_eq!(page.candidates.len(), 2);
    for candidate in page.candidates {
        f.persistence()
            .advance_pure(candidate.scope, budget(64))
            .await
            .unwrap();
    }
    let recovery = f.persistence().poll_work(None, 2).await.unwrap();
    assert_eq!(recovery.candidates.len(), 2);
    for candidate in recovery.candidates {
        assert_ne!(candidate.scope.execution, scope.execution);
        assert_eq!(
            f.persistence().step_kind(candidate.scope).await.unwrap().0,
            "data.map"
        );
        assert_eq!(
            f.persistence()
                .advance_pure(candidate.scope, budget(64))
                .await
                .unwrap()
                .completed,
            1
        );
    }
    for _ in 0..2 {
        assert!(
            f.persistence()
                .poll_work(None, 2)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
    }
    let saved = snapshot(&f).await;
    assert!(
        saved["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|run| run["state"] == "succeeded")
    );
}
