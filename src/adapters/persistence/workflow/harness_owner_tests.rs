//! SQL owner boundaries remain independent of native driver correctness.
use super::*;
use crate::{
    entities::harness::HarnessKind,
    services::harness::runs::{RigExecutionPolicy, RunCheckpoint, RunIdentity},
    use_cases::agent::{AgentPersistence, AgentWrite},
};

struct NativeFixture {
    fixture: AdmissionFixture,
    fence: WorkflowFence,
    row: Value,
}

async fn native_row() -> NativeFixture {
    let (f, scope) = fixture().await;
    native_for(f, scope).await
}

async fn native_for(f: AdmissionFixture, scope: ActivationRequest) -> NativeFixture {
    let agent = AgentPersistence::create(
        f.persistence(),
        scope.company.as_uuid(),
        AgentWrite {
            name: "Checkpoint".into(),
            slug: "checkpoint".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let claim = f
        .persistence()
        .settled()
        .claim_io(
            scope,
            worker(),
            LeasePolicy::new(Duration::from_secs(60)).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    let run = RunCheckpoint::new(
        RunIdentity {
            company_id: scope.company.as_uuid(),
            task_id: scope.job.0,
            agent_id: agent.id,
            harness: HarnessKind::Rig,
            provider: "openai".into(),
            model: "fixture".into(),
            response_contract: None,
            capability_fingerprint: "b".repeat(64),
        },
        "untrusted context".into(),
        RigExecutionPolicy::default(),
    )
    .unwrap();
    let fence = claim.fence;
    sqlx::query("INSERT INTO company_model_connections(company_id,provider,api_key,models) VALUES($1,'openai','fixture-key','{fixture}')")
        .bind(scope.company.as_uuid())
        .execute(&f.persistence().pool)
        .await
        .unwrap();
    let row = json!({"id":run.run_id.0,"company_id":scope.company.as_uuid(),
        "task_id":scope.job.0,"agent_id":agent.id,"owner_kind":"workflow",
        "workflow_run_id":scope.run.as_uuid(),"workflow_execution_id":scope.execution.as_uuid(),
        "workflow_generation":fence.generation.0,"workflow_worker_id":fence.worker.0,
        "workflow_attempt":fence.attempt.0,"workflow_prepared":prepared(agent.id),
        "schema_version":1,"revision":0,"state":"active","checkpoint":run,
        "created_at":chrono::Utc::now(),"updated_at":chrono::Utc::now()});
    NativeFixture {
        fixture: f,
        fence,
        row,
    }
}

/// The selection fields the store's current-authority check reads.
fn prepared(agent: uuid::Uuid) -> Value {
    json!({"selection":{"agent":{"agent_id":agent,"tools":[],"skills":[],
        "model":{"provider":"openai","model":"fixture","max_output_tokens":512}},"skills":[]}})
}

async fn insert(db: &mut sqlx::PgConnection, row: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO task_harness_runs SELECT (jsonb_populate_record(NULL::task_harness_runs,$1)).*")
        .bind(row).execute(db).await.map(|_| ())
}

#[tokio::test]
async fn workflow_harness_owner_exact_scope_and_workflow_owner_only() {
    let NativeFixture {
        fixture: f,
        fence: _,
        row,
    } = native_row().await;
    for field in [
        "company_id",
        "task_id",
        "agent_id",
        "workflow_run_id",
        "workflow_execution_id",
        "workflow_generation",
        "workflow_worker_id",
    ] {
        let mut wrong = row.clone();
        wrong[field] = json!(Uuid::new_v4());
        if ["company_id", "task_id", "agent_id"].contains(&field) {
            wrong["checkpoint"]["identity"][field] = wrong[field].clone();
        }
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let rejected = match insert(&mut tx, &wrong).await {
            Err(_) => {
                tx.rollback().await.unwrap();
                true
            }
            Ok(()) => tx.commit().await.is_err(),
        };
        assert!(rejected, "accepted wrong {field}");
    }
    // A workflow is the only owner a checkpoint can have (p08 D4.8).
    for (field, value) in [
        ("owner_kind", json!("legacy")),
        ("workflow_attempt", json!(2)),
        ("workflow_prepared", Value::Null),
        ("final_output_consumed_at", json!(chrono::Utc::now())),
    ] {
        let mut wrong = row.clone();
        wrong[field] = value;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        assert!(
            insert(&mut tx, &wrong).await.is_err(),
            "accepted wrong {field}"
        );
        tx.rollback().await.unwrap();
    }
    let mut tx = f.persistence().pool().begin().await.unwrap();
    insert(&mut tx, &row).await.unwrap();
    tx.commit().await.unwrap();
    let id: Uuid = serde_json::from_value(row["id"].clone()).unwrap();
    let saved: Value =
        sqlx::query_scalar("SELECT workflow_prepared FROM task_harness_runs WHERE id=$1")
            .bind(id)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(saved, row["workflow_prepared"]);
}

#[tokio::test]
async fn workflow_harness_owner_mutation_and_expired_fence_refuse() {
    let NativeFixture {
        fixture: f,
        fence,
        row,
    } = native_row().await;
    let id: Uuid = serde_json::from_value(row["id"].clone()).unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    insert(&mut tx, &row).await.unwrap();
    tx.commit().await.unwrap();
    for assignment in [
        "workflow_prepared='{}'",
        "workflow_worker_id=gen_random_uuid()",
        "workflow_generation=gen_random_uuid()",
        "workflow_attempt=workflow_attempt+1",
        "owner_kind='legacy'",
    ] {
        let sql = format!(
            "UPDATE task_harness_runs SET revision=revision+1, checkpoint=jsonb_set(checkpoint,'{revision}',to_jsonb(revision+1)), {assignment} WHERE id=$1",
            revision = "{revision}"
        );
        assert!(
            sqlx::query(&sql)
                .bind(id)
                .execute(f.persistence().pool())
                .await
                .is_err(),
            "accepted {assignment}"
        );
    }
    for path in ["{identity,model}", "{policy,model_calls}"] {
        let sql = "UPDATE task_harness_runs SET revision=revision+1,
            checkpoint=jsonb_set(jsonb_set(checkpoint,'{revision}',to_jsonb(revision+1)),$2::text[],'1') WHERE id=$1";
        let error = sqlx::query(sql)
            .bind(id)
            .bind(path)
            .execute(f.persistence().pool())
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    let update = "UPDATE task_harness_runs SET revision=revision+1,checkpoint=jsonb_set(checkpoint,'{revision}',to_jsonb(revision+1)) WHERE id=$1";
    sqlx::query(update)
        .bind(id)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    expire(&f, fence.scope).await;
    assert!(
        sqlx::query(update)
            .bind(id)
            .execute(f.persistence().pool())
            .await
            .is_err()
    );
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM task_harness_runs WHERE id=$1")
        .bind(id)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(revision, 1);
}

#[tokio::test]
async fn workflow_harness_owner_competing_open_has_one_execution_identity() {
    let NativeFixture {
        fixture: f,
        fence: _,
        row,
    } = native_row().await;
    let mut other = row.clone();
    let id = Uuid::new_v4();
    other["id"] = json!(id);
    other["checkpoint"]["run_id"] = json!(id);
    let barrier = Barrier::new(2);
    let f = &f;
    let barrier = &barrier;
    let compete = |row: Value| async move {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        barrier.wait().await;
        match insert(&mut tx, &row).await {
            Ok(()) => tx.commit().await.is_ok(),
            Err(_) => {
                tx.rollback().await.unwrap();
                false
            }
        }
    };
    let (left, right) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(compete(row), compete(other))
    })
    .await
    .unwrap();
    assert_ne!(left, right);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM task_harness_runs")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[path = "harness_store_tests.rs"]
mod store_tests;

#[tokio::test]
async fn workflow_harness_owner_existing_foreign_agent_is_not_a_valid_owner() {
    let NativeFixture {
        fixture: f, row, ..
    } = native_row().await;
    let foreign = admission_source_tests::ForeignSource::new(&f).await;
    let agent = AgentPersistence::create(
        f.persistence(),
        foreign.company.as_uuid(),
        AgentWrite {
            name: "Foreign checkpoint".into(),
            slug: "foreign-checkpoint".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut wrong = row;
    wrong["agent_id"] = json!(agent.id);
    wrong["checkpoint"]["identity"]["agent_id"] = json!(agent.id);
    let mut tx = f.persistence().pool().begin().await.unwrap();
    insert(&mut tx, &wrong).await.unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("task_harness_runs_company_id_agent_id_fkey")
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM task_harness_runs")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[cfg(test)]
#[path = "harness_model_owner_tests.rs"]
mod model_owner_tests;
