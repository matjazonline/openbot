use super::*;
use crate::application::services::model_usage::{
    ModelUsageStore, UsageGrouping, UsageQuery, UsageScope,
};
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::storage::*;
use crate::use_cases::agent::{AgentPersistence, AgentWrite};
use chrono::{Duration, Utc};
use run_lineage_tests::admitted;

#[path = "autonomous_agent_storage_tests.rs"]
mod autonomous_agent_storage_tests;

#[path = "storage_boundary_tests.rs"]
mod boundary_tests;
#[path = "storage_bundle_tests.rs"]
mod bundle_tests;
#[path = "storage_protection_tests.rs"]
mod protection_tests;
#[path = "storage_report_tests.rs"]
mod report_tests;
#[path = "storage_retry_tests.rs"]
mod retry_tests;

async fn old(f: &AdmissionFixture, scope: ActivationRequest) -> RetentionRoot {
    let command = CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: f.binding.target.actor,
        command_key: IdempotencyKey::parse(format!("retire-{}", scope.run)).unwrap(),
        expected_revision: f
            .persistence()
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    assert!(matches!(
        f.persistence().cancel(command).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    age(f, scope).await
}
async fn age(f: &AdmissionFixture, scope: ActivationRequest) -> RetentionRoot {
    let finished_at = sqlx::query_scalar(
        "UPDATE workflow_runs SET created_at=clock_timestamp()-interval '100 days',
        finished_at=clock_timestamp()-interval '91 days' WHERE id=$1 RETURNING finished_at",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    RetentionRoot {
        finished_at,
        company: scope.company,
        run: scope.run,
    }
}
async fn count(f: &AdmissionFixture, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}
async fn usage(f: &AdmissionFixture, scope: ActivationRequest, category: Option<&str>) {
    let agent = AgentPersistence::create(
        f.persistence(),
        scope.company.as_uuid(),
        AgentWrite {
            name: "Storage usage".into(),
            slug: format!("storage-{}", Uuid::new_v4()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO model_usage_calls(company_id,request_id,task_id,agent_id,workflow_id,version_id,run_id,execution_id,root_run_id,
        provider,model,requested_at,reserved_input,reserved_output,input_tokens,output_tokens,usage_source,settled_at)
        SELECT run.company_id,gen_random_uuid(),job.id,$4,run.workflow_id,run.version_id,run.id,$2,link.root_run_id,
        'openai','fixture',statement_timestamp()-interval '1 minute',100,50,
        CASE WHEN $3::text IS NOT NULL THEN 80 END,CASE WHEN $3::text IS NOT NULL THEN 20 END,$3,
        CASE WHEN $3::text IS NOT NULL THEN statement_timestamp() END
        FROM workflow_runs AS run JOIN workflow_run_budgets AS link ON link.company_id=run.company_id AND link.run_id=run.id
        JOIN background_tasks AS job ON job.company_id=run.company_id AND job.workflow_execution_id=$2 WHERE run.id=$1")
        .bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(category).bind(agent.id)
        .execute(f.persistence().pool()).await.unwrap();
}
fn query(f: &AdmissionFixture) -> UsageQuery {
    let end = Utc::now();
    UsageQuery {
        scope: UsageScope {
            company: f.binding.target.company.as_uuid(),
            actor: f.binding.target.actor.user_id(),
        },
        start: end - Duration::days(1),
        end,
        grouping: UsageGrouping::Model,
        agent: None,
        workflow: None,
    }
}
async fn report(f: &AdmissionFixture, query: UsageQuery) -> Value {
    serde_json::to_value(f.persistence().usage_report(query).await.unwrap()).unwrap()
}

#[tokio::test]
async fn run_retention_complete_tree_jobs_usage_and_idempotent_retry() {
    let source = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let (f, scopes) = AdmissionFixture::chain(source, 2).await;
    for category in [Some("reported"), Some("estimated"), Some("mixed"), None] {
        usage(&f, *scopes.last().unwrap(), category).await;
    }
    let root = old(&f, scopes[0]).await;
    let activations: i64 = sqlx::query_scalar(
        "SELECT activations FROM workflow_root_budget_usage WHERE root_run_id=$1",
    )
    .bind(scopes[0].run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert!(
        activations < scopes.len() as i64,
        "admitted runs are not activation consumption"
    );
    let unactivated_terminal:bool=sqlx::query_scalar("SELECT run.state IN ('failed','cancelled','succeeded') AND step.activated_at IS NULL
        FROM workflow_runs AS run JOIN workflow_executions AS step ON step.company_id=run.company_id AND step.run_id=run.id WHERE step.id=$1")
        .bind(scopes.last().unwrap().execution.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(unactivated_terminal);

    let q = query(&f);
    let before = report(&f, q.clone()).await;
    assert_eq!(before["scanned_calls"], 4);
    let deleted = f.persistence().retire_tree(root).await.unwrap().unwrap();
    assert_eq!(deleted.runs, 3);
    assert!(deleted.jobs >= 3);
    assert_eq!(deleted.calls, 4);
    for table in [
        "workflow_runs",
        "workflow_executions",
        "background_tasks",
        "task_attempts",
        "workflow_waits",
        "workflow_run_events",
        "model_usage_calls",
        "workflow_root_budgets",
        "workflow_run_budgets",
    ] {
        assert_eq!(count(&f, table).await, 0, "{table}");
    }
    assert_eq!(count(&f, "model_usage_history").await, 4);
    assert_eq!(report(&f, q).await, before);
    assert_eq!(f.persistence().retire_tree(root).await.unwrap(), None);
    assert!(f.persistence().sweep_unused_bundles(256).await.unwrap() > 0);
    assert_eq!(count(&f, "workflow_bundles").await, 0);
}

#[tokio::test]
async fn run_retention_rollback_after_runs_before_jobs_preserves_every_owner() {
    let f = AdmissionFixture::new().await;
    let scope = admitted(&f, "rollback").await;
    usage(&f, scope, None).await;
    let root = old(&f, scope).await;
    let before = f.counts().await;
    sqlx::raw_sql("CREATE FUNCTION fail_retained_job() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'retention fault'; END $$;
        CREATE TRIGGER fail_retained_job BEFORE DELETE ON background_tasks FOR EACH ROW EXECUTE FUNCTION fail_retained_job()")
        .execute(f.persistence().pool()).await.unwrap();
    assert!(f.persistence().retire_tree(root).await.is_err());
    assert_eq!(f.counts().await, before);
    assert_eq!(count(&f, "model_usage_calls").await, 1);
    assert_eq!(count(&f, "model_usage_history").await, 0);
    sqlx::query("DROP TRIGGER fail_retained_job ON background_tasks")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        f.persistence()
            .retire_tree(root)
            .await
            .unwrap()
            .unwrap()
            .calls,
        1
    );
}

#[tokio::test]
async fn run_retention_global_cursor_skips_protected_prefix_and_keeps_young_live() {
    let f = AdmissionFixture::new().await;
    let mut protected = Vec::new();
    for i in 0..5 {
        let scope = admitted(&f, &format!("protected-{i}")).await;
        let child = f.lineage_child(scope).await;
        // A terminal ancestor beside a live child is a constructed SQL safety case.
        run_finished_at_tests::drive(
            &f,
            scope.run,
            crate::application::workflow::reads::RunStateName::Cancelled,
        )
        .await;
        protected.push((age(&f, scope).await, child));
    }
    let young = admitted(&f, "young").await;
    old(&f, young).await;
    sqlx::query("UPDATE workflow_runs SET finished_at=clock_timestamp() WHERE id=$1")
        .bind(young.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let live = admitted(&f, "live").await;
    let foreign = f.foreign().await;
    let eligible = admitted(&foreign, "foreign-old").await;
    let eligible = old(&foreign, eligible).await;
    let mut after = None;
    let mut seen = Vec::new();
    loop {
        let page = f.persistence().retention_page(after).await.unwrap();
        for root in page.roots {
            let result = f.persistence().retire_tree(root).await.unwrap();
            seen.push(root.run);
            assert_eq!(result.is_some(), root.run == eligible.run);
        }
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    assert_eq!(seen.len(), 6);
    assert!(seen.contains(&eligible.run));
    assert!(
        f.persistence()
            .head(young.company, young.run)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        f.persistence()
            .head(live.company, live.run)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(count(&f, "workflow_runs").await, 12);
}

#[tokio::test]
async fn run_retention_competing_collectors_delete_exactly_once() {
    let f = AdmissionFixture::new().await;
    let scope = admitted(&f, "compete").await;
    let root = old(&f, scope).await;
    let (left, right) = tokio::join!(
        f.persistence().retire_tree(root),
        f.persistence().retire_tree(root)
    );
    assert_eq!(
        u8::from(left.unwrap().is_some()) + u8::from(right.unwrap().is_some()),
        1
    );
    assert_eq!(count(&f, "workflow_runs").await, 0);
}

#[tokio::test]
async fn run_retention_archive_guards_refuse_forgery_duplicates_and_mutation() {
    let f = AdmissionFixture::new().await;
    let scope = admitted(&f, "history-guards").await;
    usage(&f, scope, Some("reported")).await;
    let columns = "company_id,request_id,agent_id,workflow_id,version_id,provider,model,requested_at,reserved_input,reserved_output,input_tokens,output_tokens,usage_source,settled_at";
    let copy = format!(
        "INSERT INTO model_usage_history({columns}) SELECT {columns} FROM model_usage_calls"
    );
    assert!(
        sqlx::query(&copy.replace(
            "SELECT company_id,request_id",
            "SELECT company_id,gen_random_uuid()"
        ))
        .execute(f.persistence().pool())
        .await
        .is_err()
    );
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query(&copy).execute(&mut *tx).await.unwrap();
    assert!(tx.commit().await.is_err());
    let root = old(&f, scope).await;
    f.persistence().retire_tree(root).await.unwrap().unwrap();
    for sql in [
        "UPDATE model_usage_history SET input_tokens=input_tokens+1",
        "DELETE FROM model_usage_history",
    ] {
        assert!(
            sqlx::query(sql)
                .execute(f.persistence().pool())
                .await
                .is_err()
        );
    }
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(scope.company.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(count(&f, "model_usage_history").await, 0);
}

#[tokio::test]
async fn witness_sweep_committed_state_proofs_bounded_and_current_transaction_protected() {
    let f = AdmissionFixture::new().await;
    let first = admitted(&f, "state-1").await;
    let second = admitted(&f, "state-2").await;
    for scope in [first, second] {
        run_finished_at_tests::drive(
            &f,
            scope.run,
            crate::application::workflow::reads::RunStateName::Running,
        )
        .await;
    }
    assert_eq!(count(&f, "workflow_action_state_witnesses").await, 2);
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='timer' WHERE id=$1")
        .bind(first.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    // A separate sweeper cannot see this transaction's witness or delete its current proof.
    assert_eq!(
        f.persistence().sweep_obsolete_witnesses(1).await.unwrap(),
        1
    );
    assert!(
        sqlx::query(
            "DELETE FROM workflow_action_state_witnesses WHERE transaction_id=pg_current_xact_id()"
        )
        .execute(&mut *tx)
        .await
        .is_err()
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        f.persistence().sweep_obsolete_witnesses(1).await.unwrap(),
        1
    );
    assert_eq!(count(&f, "workflow_action_state_witnesses").await, 0);
    assert_eq!(
        f.persistence().sweep_obsolete_witnesses(1).await.unwrap(),
        0
    );
}
