use super::*;

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
