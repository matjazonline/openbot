use super::*;
#[tokio::test]
async fn workflow_wait_due_timer_sweeps_once_and_future_timer_waits() {
    let mut due = source(true);
    due["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()-chrono::Duration::seconds(1)).to_rfc3339()});
    let (f, scope) = fixture_source(due).await;
    let p = f.persistence();
    p.park_wait(scope).await.unwrap().unwrap();
    let barrier = Barrier::new(2);
    let sweep = || async {
        barrier.wait().await;
        p.sweep_waits(16).await.unwrap()
    };
    let (a, b) = tokio::join!(sweep(), sweep());
    assert_eq!(a + b, 1);
    assert_eq!(p.sweep_waits(16).await.unwrap(), 0);
    assert_eq!(
        snapshot_all(&f).await["base"]["executions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let (f, scope) = fixture_source(source(true)).await;
    let p = f.persistence();
    let parked = p.park_wait(scope).await.unwrap().unwrap();
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Pending);
    let deadline: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT deadline FROM workflow_runs WHERE id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert!(parked.deadline <= deadline);
    assert!(p.sweep_waits(0).await.is_err());
    assert!(p.sweep_waits(129).await.is_err());
}
#[tokio::test]
async fn workflow_wait_expiry_and_cancellation_do_not_schedule() {
    for cancel in [true, false] {
        let (f, scope) = fixture_source(source(false)).await;
        let p = f.persistence();
        p.park_wait(scope).await.unwrap().unwrap();
        p.record_signal(signal(scope)).await.unwrap();
        if cancel {
            sqlx::query(
                "UPDATE workflow_runs SET state='cancelled',waiting_reason=NULL WHERE id=$1",
            )
            .bind(scope.run.as_uuid())
            .execute(p.pool())
            .await
            .unwrap();
        } else {
            sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1").bind(scope.run.as_uuid()).execute(p.pool()).await.unwrap();
        }
        assert_eq!(p.sweep_waits(8).await.unwrap(), 1);
        assert_eq!(p.sweep_waits(8).await.unwrap(), 0);
        assert_eq!(
            p.resume_wait(scope).await.unwrap(),
            WaitProgress::Expired(CommitDisposition::Replayed)
        );
        let state = snapshot_all(&f).await;
        assert_eq!(state["base"]["executions"].as_array().unwrap().len(), 1);
        assert_eq!(state["waits"][0]["consumed_event_id"], Value::Null);
        assert_eq!(
            state["base"]["runs"][0]["state"],
            if cancel { "cancelled" } else { "failed" }
        );
    }
}
#[tokio::test]
async fn workflow_wait_cancel_races_resume_under_run_lock() {
    for _ in 0..3 {
        let (f, scope) = fixture_source(source(false)).await;
        let p = f.persistence();
        p.park_wait(scope).await.unwrap().unwrap();
        p.record_signal(signal(scope)).await.unwrap();
        let barrier = Barrier::new(2);
        let cancel = async {
            barrier.wait().await;
            let mut tx = p.pool().begin().await.unwrap();
            sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
                .bind(scope.run.as_uuid())
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query(
                "UPDATE workflow_runs SET state='cancelled',waiting_reason=NULL WHERE id=$1",
            )
            .bind(scope.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
            tx.commit().await.unwrap();
        };
        let resume = async {
            barrier.wait().await;
            p.resume_wait(scope).await.unwrap()
        };
        let (_, result) = tokio::join!(cancel, resume);
        let count = if matches!(result, WaitProgress::Completed(_)) {
            2
        } else {
            assert_eq!(result, WaitProgress::Expired(CommitDisposition::Committed));
            1
        };
        let state = snapshot_all(&f).await;
        assert_eq!(state["base"]["executions"].as_array().unwrap().len(), count);
        assert_eq!(state["base"]["runs"][0]["state"], "cancelled");
    }
}
#[tokio::test]
async fn workflow_wait_commit_crossing_deadline_rolls_back() {
    for park in [true, false] {
        let (f, scope) = fixture_source(source(false)).await;
        let p = f.persistence();
        if !park {
            p.park_wait(scope).await.unwrap().unwrap();
            p.record_signal(signal(scope)).await.unwrap();
        }
        sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+interval '600 milliseconds' WHERE id=$1").bind(scope.run.as_uuid()).execute(p.pool()).await.unwrap();
        let before = snapshot_all(&f).await;
        sqlx::raw_sql("CREATE FUNCTION wait_slow() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(0.7); RETURN NULL; END $$; CREATE TRIGGER wait_slow AFTER INSERT OR UPDATE ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION wait_slow();").execute(p.pool()).await.unwrap();
        if park {
            assert!(p.park_wait(scope).await.is_err());
        } else {
            assert!(p.resume_wait(scope).await.is_err());
        }
        assert_eq!(snapshot_all(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_wait_same_thread_messages_remain_independent() {
    use super::super::super::admission_history_tests::Conversation;
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(source(false)).await).await;
    let conversation = Conversation::new(&f).await;
    let second = Uuid::new_v4();
    conversation
        .insert(
            &mut f.persistence().pool().acquire().await.unwrap(),
            &f,
            second,
        )
        .await;
    let a = f
        .prepare(conversation.request(&f, "wait-a", conversation.message))
        .await;
    let b = f.prepare(conversation.request(&f, "wait-b", second)).await;
    let p = f.persistence();
    p.admit(&a).await.unwrap();
    p.admit(&b).await.unwrap();
    let scope_a = scope_for(p, &a).await;
    let scope_b = scope_for(p, &b).await;
    p.park_wait(scope_a).await.unwrap().unwrap();
    p.park_wait(scope_b).await.unwrap().unwrap();
    p.record_signal(signal(scope_b)).await.unwrap();
    let next = completed(p.resume_wait(scope_b).await.unwrap())
        .successor
        .unwrap();
    p.advance_pure(
        next,
        BatchBudget::new(4, Duration::from_millis(1000)).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(p.resume_wait(scope_a).await.unwrap(), WaitProgress::Pending);
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(scope_b.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(state, "succeeded");
}
pub(super) async fn scope_for(
    p: &PostgresPersistence,
    command: &PreparedAdmission,
) -> ActivationRequest {
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    }
}
