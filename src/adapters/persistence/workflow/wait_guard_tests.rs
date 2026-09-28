use super::deadline_tests::scope_for;
use super::*;
#[tokio::test]
async fn workflow_wait_poison_timer_does_not_block_valid_due_work() {
    let mut due = source(true);
    due["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()-chrono::Duration::seconds(1)).to_rfc3339()});
    let (f, a) = fixture_source(due).await;
    let p = f.persistence();
    let command = f.prepare(f.request("valid-due", f.manual())).await;
    p.admit(&command).await.unwrap();
    let b = scope_for(p, &command).await;
    p.park_wait(a).await.unwrap().unwrap();
    p.park_wait(b).await.unwrap().unwrap();
    sqlx::query("UPDATE workflow_runs SET max_steps=1 WHERE id=$1")
        .bind(a.run.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    assert_eq!(p.sweep_waits(8).await.unwrap(), 2);
    assert_eq!(p.sweep_waits(8).await.unwrap(), 0);
    assert_eq!(
        p.resume_wait(a).await.unwrap(),
        WaitProgress::Failed {
            disposition: CommitDisposition::Replayed,
            reason: WaitFailureReason::ActivationLimit
        }
    );
    let result = completed(p.resume_wait(b).await.unwrap());
    assert!(
        result
            .output
            .get("deadline")
            .and_then(Value::as_str)
            .is_some()
    );
    assert!(result.successor.is_some());
}
#[tokio::test]
async fn workflow_wait_sweep_selection_has_bounded_lock_wait() {
    let (f, _) = fixture_source(source(true)).await;
    let p = f.persistence();
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("LOCK TABLE workflow_waits IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(4), p.sweep_waits(8))
            .await
            .unwrap()
            .is_err()
    );
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn workflow_wait_event_facts_faults_and_bounds_are_enforced() {
    let (f, scope) = fixture_source(source(false)).await;
    let p = f.persistence();
    let before = snapshot_all(&f).await;
    sqlx::query("ALTER TABLE workflow_wait_events ADD CONSTRAINT injected CHECK(false)")
        .execute(p.pool())
        .await
        .unwrap();
    assert!(p.record_signal(signal(scope)).await.is_err());
    assert_eq!(snapshot_all(&f).await, before);
    sqlx::query("ALTER TABLE workflow_wait_events DROP CONSTRAINT injected")
        .execute(p.pool())
        .await
        .unwrap();
    for (name, correlation) in [
        ("".into(), "ok".into()),
        ("x".repeat(129), "ok".into()),
        ("ok".into(), "".into()),
        ("ok".into(), "x".repeat(257)),
    ] {
        let mut event = signal(scope);
        event.name.0 = name;
        event.correlation.0 = correlation;
        assert!(!p.record_signal(event).await.unwrap());
        assert_eq!(snapshot_all(&f).await, before);
    }
    let event = signal(scope);
    p.record_signal(event.clone()).await.unwrap();
    let error = sqlx::query("UPDATE workflow_wait_events SET payload='null'::jsonb WHERE id=$1")
        .bind(event.id.0)
        .execute(p.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    sqlx::query("INSERT INTO workflow_wait_events(company_id,run_id,execution_id,id,event_name,correlation,payload) SELECT $1,$2,$3,gen_random_uuid(),'review.completed','request-1',$4 FROM generate_series(1,127)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(event.payload).execute(p.pool()).await.unwrap();
    assert!(p.record_signal(signal(scope)).await.is_err());
    p.park_wait(scope).await.unwrap().unwrap();
    completed(p.resume_wait(scope).await.unwrap());
    let before = snapshot_all(&f).await;
    completed(p.resume_wait(scope).await.unwrap());
    assert_eq!(snapshot_all(&f).await, before);
}
#[tokio::test]
async fn workflow_wait_old_execution_events_do_not_resume_new_wait() {
    let mut chain = source(false);
    chain["steps"]["next"] = chain["steps"]["start"].clone();
    chain["steps"]["start"]["routes"]["success"] = json!("next");
    // Begin with a real pure step so activation/progression enters the first wait.
    chain["entry"] = json!("map");
    chain["steps"]["map"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"start"}});
    let (f, entry) = fixture_source(chain).await;
    let p = f.persistence();
    let advanced = p
        .advance_pure(
            entry,
            BatchBudget::new(4, Duration::from_millis(1000)).unwrap(),
        )
        .await
        .unwrap();
    let first = advanced.continuation.unwrap();
    p.park_wait(first).await.unwrap().unwrap();
    let event = signal(first);
    p.record_signal(event.clone()).await.unwrap();
    let next = completed(p.resume_wait(first).await.unwrap())
        .successor
        .unwrap();
    p.park_wait(next).await.unwrap().unwrap();
    assert!(p.record_signal(event).await.unwrap());
    assert!(!p.record_signal(signal(first)).await.unwrap());
    assert_eq!(p.resume_wait(next).await.unwrap(), WaitProgress::Pending);
    p.record_signal(signal(next)).await.unwrap();
    completed(p.resume_wait(next).await.unwrap());
}
#[tokio::test]
async fn workflow_wait_event_deadline_guard_is_independent_of_run_deadline() {
    let (f, scope) = fixture_source(source(false)).await;
    let p = f.persistence();
    p.park_wait(scope).await.unwrap().unwrap();
    p.record_signal(signal(scope)).await.unwrap();
    sqlx::query("UPDATE workflow_waits SET deadline=clock_timestamp()+interval '600 milliseconds' WHERE run_id=$1").bind(scope.run.as_uuid()).execute(p.pool()).await.unwrap();
    let before = snapshot_all(&f).await;
    sqlx::raw_sql("CREATE FUNCTION wait_event_slow() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(0.7); RETURN NEW; END $$; CREATE TRIGGER wait_event_slow AFTER INSERT ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION wait_event_slow();").execute(p.pool()).await.unwrap();
    assert!(p.resume_wait(scope).await.is_err());
    assert_eq!(snapshot_all(&f).await, before);
}
#[tokio::test]
async fn workflow_wait_expiry_competes_with_resume() {
    for _ in 0..3 {
        let (f, scope) = fixture_source(source(false)).await;
        let p = f.persistence();
        p.park_wait(scope).await.unwrap().unwrap();
        p.record_signal(signal(scope)).await.unwrap();
        let barrier = Barrier::new(2);
        let expire = async {
            barrier.wait().await;
            let mut tx = p.pool().begin().await.unwrap();
            sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
                .bind(scope.run.as_uuid())
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1 AND state='waiting'").bind(scope.run.as_uuid()).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            p.sweep_waits(8).await.unwrap()
        };
        let resume = async {
            barrier.wait().await;
            p.resume_wait(scope).await.unwrap()
        };
        let (_, result) = tokio::join!(expire, resume);
        let expected = if matches!(result, WaitProgress::Completed(_)) {
            2
        } else {
            1
        };
        assert_eq!(
            snapshot_all(&f).await["base"]["executions"]
                .as_array()
                .unwrap()
                .len(),
            expected
        );
        assert_eq!(p.sweep_waits(8).await.unwrap(), 0);
    }
}
#[tokio::test]
async fn workflow_wait_event_scope_and_consumption_foreign_keys() {
    let (f, a) = fixture_source(source(false)).await;
    let p = f.persistence();
    let command = f.prepare(f.request("second", f.manual())).await;
    p.admit(&command).await.unwrap();
    let b = scope_for(p, &command).await;
    let event = signal(a);
    p.record_signal(event.clone()).await.unwrap();
    p.park_wait(b).await.unwrap().unwrap();
    let error = sqlx::query("UPDATE workflow_waits SET consumed_event_id=$2 WHERE execution_id=$1")
        .bind(b.execution.as_uuid())
        .bind(event.id.0)
        .execute(p.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO companies(id,name,slug,user_id) SELECT $1,'Foreign',$2,user_id FROM companies WHERE id=$3")
        .bind(foreign)
        .bind(format!("foreign-{foreign}"))
        .bind(a.company.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    for (company, run) in [
        (foreign, a.run.as_uuid()),
        (a.company.as_uuid(), b.run.as_uuid()),
    ] {
        let error=sqlx::query("INSERT INTO workflow_wait_events(company_id,run_id,execution_id,id,event_name,correlation,payload) VALUES($1,$2,$3,$4,'e','c','null')").bind(company).bind(run).bind(a.execution.as_uuid()).bind(Uuid::new_v4()).execute(p.pool()).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23503")
        );
    }
}
