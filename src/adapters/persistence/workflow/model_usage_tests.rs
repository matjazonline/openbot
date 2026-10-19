//! Usage provenance is enforced by SQL, independently of a future workflow model handler.
use super::*;
use crate::{
    services::model_usage::{ModelUsageStore, UsageGrouping, UsageQuery, UsageScope},
    use_cases::agent::{AgentPersistence, AgentWrite},
};

#[tokio::test]
async fn model_usage_workflow_exact_provenance_and_grouping() {
    let f = AdmissionFixture::new().await;
    let command = f.prepare(f.request("usage", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let company = command.company_id().as_uuid();
    let agent = AgentPersistence::create(
        f.persistence(),
        company,
        AgentWrite {
            name: "Usage agent".into(),
            slug: "usage-agent".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let row = usage_row(
        &f,
        command.proposed_run_id().as_uuid(),
        command.first_execution_id().as_uuid(),
        agent.id,
    )
    .await;
    let foreign = admission_source_tests::ForeignSource::new(&f).await;
    let foreign_run = admission_source_tests::foreign_parent(&f, foreign.company).await;
    let foreign_agent = AgentPersistence::create(
        f.persistence(),
        foreign.company.as_uuid(),
        AgentWrite {
            name: "Foreign usage agent".into(),
            slug: "foreign-usage".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let foreign_row = usage_row(
        &f,
        foreign_run.run_id().as_uuid(),
        foreign_run.execution_id().as_uuid(),
        foreign_agent.id,
    )
    .await;
    reject_foreign(&f, &row, &foreign_row).await;
    let mut missing = row.clone();
    for field in [
        "workflow_id",
        "version_id",
        "run_id",
        "execution_id",
        "root_run_id",
    ] {
        missing[field] = Value::Null;
    }
    assert!(sqlx::query("INSERT INTO model_usage_calls SELECT (jsonb_populate_record(NULL::model_usage_calls,$1)).*")
        .bind(missing).execute(f.persistence().pool()).await.is_err());
    sqlx::query("INSERT INTO model_usage_calls SELECT (jsonb_populate_record(NULL::model_usage_calls,$1)).*")
        .bind(row.clone()).execute(f.persistence().pool()).await.unwrap();
    let end = chrono::Utc::now();
    let query = UsageQuery {
        scope: UsageScope {
            company,
            actor: f.binding.target.actor.user_id(),
        },
        start: end - chrono::Duration::days(1),
        end,
        grouping: UsageGrouping::Workflow,
        agent: Some(agent.id),
        workflow: Some(serde_json::from_value(row["workflow_id"].clone()).unwrap()),
    };
    let report = f.persistence().usage_report(query).await.unwrap();
    assert_eq!(report.groups.len(), 1);
    assert_eq!(
        report.groups[0].workflow,
        Some(serde_json::from_value(row["workflow_id"].clone()).unwrap())
    );
    assert_eq!(report.groups[0].calls, 1);
    assert_eq!(report.groups[0].unconfirmed_calls, 1);
}

async fn usage_row(f: &AdmissionFixture, run: Uuid, execution: Uuid, agent: Uuid) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('company_id',run.company_id,'request_id',gen_random_uuid(),
        'task_id',task.id,'agent_id',$2::uuid,'workflow_id',run.workflow_id,'version_id',run.version_id,'run_id',run.id,
        'execution_id',task.workflow_execution_id,'root_run_id',budget.root_run_id,'provider','openai','model','fixture',
        'reserved_input',100,'reserved_output',50,'requested_at',statement_timestamp())
        FROM workflow_runs AS run JOIN background_tasks AS task ON task.company_id=run.company_id AND task.workflow_execution_id=$3
        JOIN workflow_run_budgets AS budget ON budget.company_id=run.company_id AND budget.run_id=run.id WHERE run.id=$1")
        .bind(run).bind(agent).bind(execution)
        .fetch_one(f.persistence().pool()).await.unwrap()
}
async fn reject_foreign(f: &AdmissionFixture, row: &Value, foreign: &Value) {
    for field in [
        "company_id",
        "task_id",
        "agent_id",
        "workflow_id",
        "version_id",
        "run_id",
        "execution_id",
        "root_run_id",
    ] {
        for value in [foreign[field].clone(), json!(Uuid::new_v4())] {
            // These fixtures deliberately reuse some UUIDs in different tenant namespaces.
            if value == row[field] {
                continue;
            }
            let mut wrong = row.clone();
            wrong[field] = value;
            let mut tx = f.persistence().pool().begin().await.unwrap();
            let insert = sqlx::query("INSERT INTO model_usage_calls SELECT (jsonb_populate_record(NULL::model_usage_calls,$1)).*")
            .bind(wrong).execute(&mut *tx).await;
            let rejected = if insert.is_err() {
                tx.rollback().await.unwrap();
                true
            } else {
                tx.commit().await.is_err()
            };
            assert!(rejected, "SQL accepted wrong {field}");
        }
    }
}
