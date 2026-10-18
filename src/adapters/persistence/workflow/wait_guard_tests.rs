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
    assert!(p.record_event(signal(scope)).await.is_err());
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
        assert!(matches!(
            p.record_event(event).await,
            Err(AppError::BadRequest(_))
        ));
        assert_eq!(snapshot_all(&f).await, before);
    }
    let event = signal(scope);
    recorded(p, event.clone()).await.unwrap();
    let error = sqlx::query("UPDATE workflow_wait_events SET payload='null'::jsonb WHERE id=$1")
        .bind(event.id.0)
        .execute(p.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    sqlx::query("INSERT INTO workflow_wait_events(company_id,run_id,id,event_name,correlation,payload) SELECT $1,$2,gen_random_uuid(),'review.completed','request-1',$3 FROM generate_series(1,127)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(event.payload).execute(p.pool()).await.unwrap();
    assert!(matches!(
        p.record_event(signal(scope)).await,
        Err(AppError::Conflict(_))
    ));
    p.park_wait(scope).await.unwrap().unwrap();
    completed(p.resume_wait(scope).await.unwrap());
    let before = snapshot_all(&f).await;
    completed(p.resume_wait(scope).await.unwrap());
    assert_eq!(snapshot_all(&f).await, before);
}
#[tokio::test]
async fn workflow_wait_consumed_event_does_not_resume_new_wait() {
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
    recorded(p, event.clone()).await.unwrap();
    let next = completed(p.resume_wait(first).await.unwrap())
        .successor
        .unwrap();
    p.park_wait(next).await.unwrap().unwrap();
    assert!(recorded(p, event).await.unwrap());
    assert_eq!(p.resume_wait(next).await.unwrap(), WaitProgress::Pending);
    recorded(p, signal(next)).await.unwrap();
    completed(p.resume_wait(next).await.unwrap());
}
#[tokio::test]
async fn workflow_wait_event_deadline_guard_is_independent_of_run_deadline() {
    let (f, scope) = fixture_source(source(false)).await;
    let p = f.persistence();
    p.park_wait(scope).await.unwrap().unwrap();
    recorded(p, signal(scope)).await.unwrap();
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
        recorded(p, signal(scope)).await.unwrap();
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
    recorded(p, event.clone()).await.unwrap();
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
        let error=sqlx::query("INSERT INTO workflow_wait_events(company_id,run_id,consumed_execution_id,id,event_name,correlation,payload) VALUES($1,$2,$3,$4,'e','c','null')").bind(company).bind(run).bind(a.execution.as_uuid()).bind(Uuid::new_v4()).execute(p.pool()).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23503")
        );
    }
}

#[tokio::test]
async fn workflow_wait_event_bound_competing_producers() {
    let (f, scope) = fixture_source(source(false)).await;
    let p = f.persistence();
    let event = signal(scope);
    sqlx::query("INSERT INTO workflow_wait_events(company_id,run_id,id,event_name,correlation,payload) SELECT $1,$2,gen_random_uuid(),'review.completed','request-1',$3 FROM generate_series(1,127)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(event.payload).execute(p.pool()).await.unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let (p, barrier) = (p.clone(), barrier.clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            p.record_event(signal(scope)).await
        }));
    }
    let mut recorded = 0;
    let mut rejected = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(EventIngress::Recorded) => recorded += 1,
            Err(AppError::Conflict(_)) => rejected += 1,
            result => panic!("unexpected producer result: {result:?}"),
        }
    }
    assert_eq!((recorded, rejected), (1, 7));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_wait_events WHERE consumed_execution_id IS NULL",
    )
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(count, 128);
}

#[tokio::test]
async fn workflow_wait_event_stale_event_matches_only_its_correlation() {
    let mut chain = source(false);
    chain["steps"]["second"] = chain["steps"]["start"].clone();
    chain["steps"]["third"] = chain["steps"]["start"].clone();
    chain["steps"]["start"]["routes"]["success"] = json!("second");
    chain["steps"]["second"]["routes"]["success"] = json!("third");
    chain["steps"]["second"]["with"]["correlation"] = json!({"literal":"request-2"});
    let (f, first) = fixture_source(chain).await;
    let p = f.persistence();
    p.park_wait(first).await.unwrap().unwrap();
    recorded(p, signal(first)).await.unwrap();
    let surplus = signal(first);
    recorded(p, surplus.clone()).await.unwrap();
    let second = completed(p.resume_wait(first).await.unwrap())
        .successor
        .unwrap();
    p.park_wait(second).await.unwrap().unwrap();
    assert_eq!(p.resume_wait(second).await.unwrap(), WaitProgress::Pending);
    let mut matching = signal(second);
    matching.correlation.0 = "request-2".into();
    recorded(p, matching).await.unwrap();
    let third = completed(p.resume_wait(second).await.unwrap())
        .successor
        .unwrap();
    p.park_wait(third).await.unwrap().unwrap();
    completed(p.resume_wait(third).await.unwrap());
    let consumer: Uuid =
        sqlx::query_scalar("SELECT consumed_execution_id FROM workflow_wait_events WHERE id=$1")
            .bind(surplus.id.0)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(consumer, third.execution.as_uuid());
}

#[tokio::test]
async fn workflow_wait_event_id_collision_across_competing_runs_is_conflict() {
    let (f, a) = fixture_source(source(false)).await;
    let p = f.persistence();
    let command = f.prepare(f.request("second", f.manual())).await;
    p.admit(&command).await.unwrap();
    let b = scope_for(p, &command).await;
    let first = signal(a);
    let mut second = signal(b);
    second.id = first.id;
    let mut blocker = p.pool().begin().await.unwrap();
    sqlx::query("LOCK TABLE workflow_wait_events IN SHARE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (left, right) = (p.clone(), p.clone());
    let left = tokio::spawn(async move { left.record_event(first).await });
    let right = tokio::spawn(async move { right.record_event(second).await });
    // Both producers have passed the empty identity read under distinct run locks.
    tokio::time::timeout(std::time::Duration::from_millis(800), async {
        loop {
            let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_locks WHERE relation='workflow_wait_events'::regclass AND mode='RowExclusiveLock' AND NOT granted")
                .fetch_one(p.pool()).await.unwrap();
            if waiting == 2 { break; }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    blocker.commit().await.unwrap();
    let outcomes = [left.await.unwrap(), right.await.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, Ok(EventIngress::Recorded)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, Err(AppError::Conflict(_))))
            .count(),
        1
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_wait_events")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}
