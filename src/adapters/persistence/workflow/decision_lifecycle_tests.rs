use super::*;

#[tokio::test]
async fn workflow_decision_past_deadline_never_parks_and_unavailable_never_routes() {
    let past = Box::pin(DecisionFixture::configured(1, |source| {
        source["steps"]["start"]["with"]["deadline"] = json!({"literal":"2000-01-01T00:00:00Z"});
    }))
    .await;
    assert!(
        past.persistence()
            .park_decision(past.scope, &past.notices)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(past.counts().await["run"], "failed");
    assert_eq!(past.counts().await["waits"], 0);
    let unavailable = Box::pin(DecisionFixture::configured(1, |source| {
        source["steps"]["start"]["with"]["reviewers"] =
            json!({"literal":[{"kind":"user","id":Uuid::new_v4()}]});
        source["steps"]["start"]["routes"]["error"] = json!("finish");
    }))
    .await;
    assert!(
        unavailable
            .persistence()
            .park_decision(unavailable.scope, &unavailable.notices)
            .await
            .unwrap()
            .is_none()
    );
    assert_failed_without_park(&unavailable).await;
    assert_eq!(unavailable.counts().await["executions"], 1);
}

#[tokio::test]
async fn workflow_decision_submission_crossing_deadline_rolls_back() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    sqlx::raw_sql("CREATE FUNCTION slow_decision_submission() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_kind='decision_submitted' THEN PERFORM pg_sleep(0.3); END IF; RETURN NEW; END $$; CREATE TRIGGER slow_decision_submission BEFORE INSERT ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION slow_decision_submission();").execute(f.persistence().pool()).await.unwrap();
    sqlx::query("UPDATE workflow_waits SET deadline=clock_timestamp()+interval '0.15 seconds' WHERE run_id=$1").bind(f.scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    assert!(matches!(
        f.persistence()
            .submit_decision(f.submit(&view, 0, "crossing"))
            .await,
        Err(AppError::Conflict(_))
    ));
    let read = f
        .persistence()
        .read_decision(f.scope.company, f.actor(0), view.decision)
        .await
        .unwrap();
    assert_eq!(read.state, "waiting");
    assert_eq!(read.revision, 1);
    assert!(read.submission.is_none());
    assert_eq!(f.counts().await["executions"], 1);
}

#[tokio::test]
async fn workflow_decision_two_runs_one_company_do_not_serialize() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let first = f.park().await;
    let admission = f
        .admission
        .prepare(f.admission.request("second", f.admission.manual()))
        .await;
    f.persistence().admit(&admission).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(admission.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let second = ActivationRequest {
        company: admission.company_id(),
        run: admission.proposed_run_id(),
        execution: admission.first_execution_id(),
        job: WorkflowJobId(job),
    };
    f.persistence()
        .park_decision(second, &f.notices)
        .await
        .unwrap()
        .unwrap();
    let decision = sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE run_id=$1")
        .bind(second.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let second_view = f
        .persistence()
        .read_decision(f.scope.company, f.actor(0), DecisionId(decision))
        .await
        .unwrap();
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(f.scope.run.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let command = f.submit(&first, 0, "first");
    let p = f.persistence().clone();
    let first_submit = tokio::spawn(async move { p.submit_decision(command).await });
    let observed = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    let second_result = f
        .persistence()
        .submit_decision(f.submit(&second_view, 0, "second"))
        .await;
    blocker.rollback().await.unwrap();
    let first_result = first_submit.await.unwrap();
    observed.unwrap();
    assert_eq!(
        second_result.unwrap().disposition,
        DecisionDisposition::Applied
    );
    assert_eq!(
        first_result.unwrap().disposition,
        DecisionDisposition::Applied
    );
}

#[tokio::test]
async fn workflow_decision_notices_are_atomic_private_and_never_settle() {
    let f = Box::pin(DecisionFixture::configured(2, |source| {
        source["steps"]["start"]["with"]["proposal"] =
            json!({"literal":{"private":"secret-proposal-05"}});
    }))
    .await;
    sqlx::query("ALTER TABLE workflow_decision_reviewers ADD CONSTRAINT injected_notice_failure CHECK(ordinal=0)").execute(f.persistence().pool()).await.unwrap();
    assert!(
        f.persistence()
            .park_decision(f.scope, &f.notices)
            .await
            .is_err()
    );
    let counts = f.counts().await;
    for key in ["waits", "decisions", "reviewers", "deliveries"] {
        assert_eq!(counts[key], 0);
    }
    sqlx::query("ALTER TABLE workflow_decision_reviewers DROP CONSTRAINT injected_notice_failure")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let view = f.park().await;
    f.persistence()
        .add_decision_comment(AddDecisionComment {
            company: f.scope.company,
            actor: f.actor(0),
            decision: view.decision,
            command_key: DecisionCommandKey::parse("private").unwrap(),
            body: "secret-comment-05".into(),
        })
        .await
        .unwrap();
    let parts: String = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(part))::text FROM message_delivery_parts AS part",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert!(!parts.contains("secret-proposal-05"));
    assert!(!parts.contains("secret-comment-05"));
    assert!(parts.contains(&view.decision.0.to_string()));
    let deliveries: Vec<Uuid> = sqlx::query_scalar("SELECT notice_delivery_id FROM workflow_decision_reviewers WHERE decision_id=$1 ORDER BY ordinal").bind(view.decision.0).fetch_all(f.persistence().pool()).await.unwrap();
    sqlx::query("UPDATE message_deliveries SET status='dead_letter' WHERE id=$1")
        .bind(deliveries[0])
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let still_open = f
        .persistence()
        .read_decision(f.scope.company, f.actor(0), view.decision)
        .await
        .unwrap();
    assert_eq!(still_open.state, "waiting");
    assert!(still_open.submission.is_none());
    f.persistence()
        .submit_decision(f.submit(&view, 0, "accept"))
        .await
        .unwrap();
    let before = f
        .persistence()
        .read_decision(f.scope.company, f.actor(0), view.decision)
        .await
        .unwrap();
    sqlx::query("UPDATE message_deliveries SET status='delivered',delivered_at=clock_timestamp() WHERE id=$1").bind(deliveries[1]).execute(f.persistence().pool()).await.unwrap();
    assert_eq!(
        f.persistence()
            .read_decision(f.scope.company, f.actor(0), view.decision)
            .await
            .unwrap(),
        before
    );
}
