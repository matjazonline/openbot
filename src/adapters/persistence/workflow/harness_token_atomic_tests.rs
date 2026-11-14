use super::*;

#[tokio::test]
async fn workflow_harness_token_competing_reservation_replans_before_new_reviewer_lock() {
    use super::account_orders::blocked_by;
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let child = refusal::child_checkpoint(&f).await;
    let notices = notices(p);
    let mut checkpoint_gate = p.pool().begin().await.unwrap();
    let checkpoint_blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *checkpoint_gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM task_harness_runs WHERE id=$1 FOR UPDATE")
        .bind(child.checkpoint.0)
        .execute(&mut *checkpoint_gate)
        .await
        .unwrap();
    let mut reviewer_gate = p.pool().begin().await.unwrap();
    let reviewer_blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *reviewer_gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.reviewer.user_id())
        .execute(&mut *reviewer_gate)
        .await
        .unwrap();
    let mut child_request = reservation();
    child_request.input_tokens = 50;
    child_request.output_tokens = 50;
    let sibling = p.reserve_workflow_model(child, model_request(&child_request), Some(&notices));
    tokio::pin!(sibling);
    let sibling_pid = tokio::select! {
        pid=blocked_by(p.pool(),checkpoint_blocker,"%task_harness_runs%FOR UPDATE%")=>pid,
        result=sibling.as_mut()=>panic!("sibling must reach the checkpoint gate: {result:?}"),
    };
    let mut root_request = reservation();
    root_request.input_tokens = 50;
    root_request.output_tokens = 50;
    let reserve =
        p.reserve_workflow_model(model.write, model_request(&root_request), Some(&notices));
    tokio::pin!(reserve);
    tokio::select! {
        _=blocked_by(p.pool(),sibling_pid,"%companies%FOR NO KEY UPDATE%")=>{},
        result=reserve.as_mut()=>panic!("root must wait behind the sibling's company ownership: {result:?}"),
    }
    checkpoint_gate.rollback().await.unwrap();
    let finish = async {
        blocked_by(
            p.pool(),
            reviewer_blocker,
            "%FROM users WHERE id=ANY%FOR SHARE%",
        )
        .await;
        sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
            .bind(model.write.fence.scope.company.as_uuid())
            .execute(&mut *reviewer_gate)
            .await
            .expect(
                "stale no-review preparation must release company before locking its new reviewer",
            );
        reviewer_gate.rollback().await.unwrap();
    };
    let (sibling, reserve, ()) = tokio::join!(sibling, reserve, finish);
    assert!(matches!(sibling.unwrap(), WorkflowModelAdmission::New(_)));
    assert!(matches!(
        reserve.unwrap(),
        WorkflowModelAdmission::Parked(_)
    ));
    assert_single_reservation(&accounting(model).await, child_request.request_id.0);
    let decisions: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_decisions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(
        decisions, 1,
        "one replan and one park, with no stale-attempt effects"
    );
}

fn assert_single_reservation(usage: &Value, request: Uuid) {
    assert_eq!(usage["calls"], 1);
    assert_eq!(usage["rows"].as_array().unwrap().len(), 1);
    assert_eq!(usage["rows"][0]["request_id"], request.to_string());
}

async fn snapshot(f: &TokenFixture) -> Value {
    let p = f.model.native.fixture.persistence();
    let rows: Value = sqlx::query_scalar("SELECT jsonb_build_object(
        'jobs',(SELECT jsonb_agg(to_jsonb(job) ORDER BY id) FROM background_tasks AS job),
        'attempts',(SELECT jsonb_agg(to_jsonb(attempt) ORDER BY id) FROM task_attempts AS attempt),
        'waits',(SELECT jsonb_agg(to_jsonb(wait) ORDER BY id) FROM workflow_waits AS wait),
        'decisions',(SELECT jsonb_agg(to_jsonb(decision) ORDER BY id) FROM workflow_decisions AS decision),
        'deliveries',(SELECT count(*) FROM message_deliveries),
        'usage',(SELECT jsonb_agg(to_jsonb(usage)) FROM workflow_root_budget_usage AS usage))")
        .fetch_one(p.pool()).await.unwrap();
    json!({"rows":rows,"checkpoint":stored(&f.model.native.fixture,f.model.run.run_id.0).await,
        "accounting":accounting(&f.model).await})
}

#[tokio::test]
async fn workflow_harness_token_notification_commit_failure_rolls_back_pause() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let before = snapshot(&f).await;
    sqlx::raw_sql("CREATE FUNCTION fail_token_notice() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture token notice'; END; $$;
        CREATE CONSTRAINT TRIGGER fail_token_notice AFTER INSERT ON message_deliveries DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_token_notice();")
        .execute(p.pool()).await.unwrap();
    assert!(
        p.reserve_workflow_model(
            f.model.write,
            model_request(&reservation()),
            Some(&notices(p))
        )
        .await
        .is_err()
    );
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
async fn workflow_harness_token_approval_commit_failure_rolls_back_allowance_and_reopen() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    refusal::park(&f).await;
    let approve = command(p, f.model.write.fence.scope, f.reviewer, "approve").await;
    let before = snapshot(&f).await;
    sqlx::raw_sql("CREATE FUNCTION fail_token_choice() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture token choice'; END; $$;
        CREATE CONSTRAINT TRIGGER fail_token_choice AFTER UPDATE ON workflow_decisions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_token_choice();")
        .execute(p.pool()).await.unwrap();
    assert!(p.submit_decision(approve).await.is_err());
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
async fn workflow_harness_token_sql_cannot_change_proposal_or_grant_allowance() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    refusal::park(&f).await;
    let before = snapshot(&f).await;
    for statement in [
        "UPDATE workflow_decisions SET token_proposal=jsonb_set(token_proposal,'{proposed_allowance}','999999'::jsonb)",
        "UPDATE workflow_root_budget_usage SET authorized_tokens=999999",
        "UPDATE task_harness_runs SET model_blocked_decision=NULL,revision=revision+1,checkpoint=jsonb_set(checkpoint,'{revision}',to_jsonb(revision+1))",
    ] {
        let mut tx = p.pool().begin().await.unwrap();
        match sqlx::query(statement).execute(&mut *tx).await {
            Ok(_) => assert!(tx.commit().await.is_err(), "{statement}"),
            Err(_) => tx.rollback().await.unwrap(),
        }
        assert_eq!(snapshot(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_harness_token_reviewer_revocation_wins_before_approval_commit() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    refusal::park(&f).await;
    let approve = command(p, f.model.write.fence.scope, f.reviewer, "approve").await;
    let before = snapshot(&f).await;
    let mut revoke = p.pool().begin().await.unwrap();
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(f.model.write.fence.scope.company.as_uuid())
        .bind(f.reviewer.user_id())
        .execute(&mut *revoke)
        .await
        .unwrap();
    let (result, ()) = tokio::join!(p.submit_decision(approve), async {
        crate::adapters::persistence::test_support::wait_until_backends_are_blocked(p.pool(), 1)
            .await;
        revoke.commit().await.unwrap();
    });
    assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
    assert_eq!(snapshot(&f).await, before);
}
