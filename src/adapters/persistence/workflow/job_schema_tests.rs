//! Isolated job fixtures use the fully migrated nullable workflow schema.
use super::*;
use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};

#[path = "job_association_tests.rs"]
mod job_association_tests;
#[path = "job_core_query_tests.rs"]
mod job_core_query_tests;

struct JobFixture {
    binding: BindingFixture,
    channel: Uuid,
    execution: Uuid,
    run: Uuid,
    thread: Uuid,
}

impl JobFixture {
    async fn new() -> Self {
        let binding = BindingFixture::new(json!([])).await;
        let channel = ChannelPersistence::create(
            &binding.fixture.persistence,
            binding.target.company.as_uuid(),
            ChannelWrite {
                name: "Jobs".into(),
                slug: "jobs".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut db = binding.fixture.persistence.pool().acquire().await.unwrap();
        let run = Uuid::new_v4();
        let execution = Uuid::new_v4();
        let thread = Uuid::new_v4();
        sqlx::query("INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'Workflow')")
            .bind(thread).bind(binding.target.company.as_uuid()).bind(channel.id).execute(&mut *db).await.unwrap();
        insert_run(
            &mut db,
            &binding,
            run,
            RunAssociation {
                channel: Some(channel.id),
                thread: Some(thread),
            },
        )
        .await
        .unwrap();
        insert_execution(&mut db, binding.target.company.as_uuid(), run, execution)
            .await
            .unwrap();
        Self {
            binding,
            channel: channel.id,
            execution,
            run,
            thread,
        }
    }

    async fn insert(
        &self,
        db: &mut PgConnection,
        payload: serde_json::Value,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type, queue_kind, workflow_execution_id, payload, thread_id) VALUES ($1, $2, $3, $4, 'workflow_execution', 'workflow', $5, $6, $7)")
            .bind(id).bind(self.binding.target.company.as_uuid()).bind(self.channel)
            .bind(Uuid::new_v4()).bind(self.execution).bind(payload).bind(self.thread).execute(db).await?;
        Ok(id)
    }

    fn payload(&self) -> serde_json::Value {
        json!({"version": 1, "execution_id": self.execution})
    }

    async fn hypothetical(&self) -> sqlx::Transaction<'_, sqlx::Postgres> {
        self.binding
            .fixture
            .persistence
            .pool()
            .begin()
            .await
            .unwrap()
    }
}

fn constraint(error: sqlx::Error, expected: &str) {
    assert_eq!(
        error.as_database_error().and_then(|e| e.constraint()),
        Some(expected),
        "{error}"
    );
}

#[tokio::test]
async fn workflow_job_schema_keeps_the_channel_nullable_for_company_associations() {
    let f = JobFixture::new().await;
    let mut db = f
        .binding
        .fixture
        .persistence
        .pool()
        .acquire()
        .await
        .unwrap();
    f.insert(&mut db, f.payload()).await.unwrap();
    let required: bool = sqlx::query_scalar("SELECT attnotnull FROM pg_attribute WHERE attrelid = 'background_tasks'::regclass AND attname = 'channel_id'")
        .fetch_one(&mut *db).await.unwrap();
    assert!(
        !required,
        "workflow company associations require nullable channels"
    );
}

#[tokio::test]
async fn workflow_job_schema_enforces_exact_ids_and_scoped_execution() {
    let mut f = JobFixture::new().await;
    for payload in [
        json!({}),
        json!({"version": null, "execution_id": f.execution}),
        json!({"version": 1, "execution_id": f.execution, "context": {}}),
        json!({"version": 1, "execution_id": Uuid::new_v4()}),
    ] {
        let mut tx = f.hypothetical().await;
        constraint(
            f.insert(&mut tx, payload).await.unwrap_err(),
            "background_tasks_workflow_shape_check",
        );
        tx.rollback().await.unwrap();
    }
    let original = f.execution;
    f.execution = Uuid::new_v4();
    let mut tx = f.hypothetical().await;
    constraint(
        f.insert(&mut tx, f.payload()).await.unwrap_err(),
        "background_tasks_workflow_execution_fk",
    );
    tx.rollback().await.unwrap();
    f.execution = original;
    let mut tx = f.hypothetical().await;
    let id = f.insert(&mut tx, f.payload()).await.unwrap();
    let saved: (String, Uuid, serde_json::Value) = sqlx::query_as(
        "SELECT queue_kind, workflow_execution_id, payload FROM background_tasks WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(saved, ("workflow".into(), f.execution, f.payload()));
    let error = sqlx::query("UPDATE background_tasks SET workflow_execution_id = $2 WHERE id = $1")
        .bind(id)
        .bind(Uuid::new_v4())
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert!(error.to_string().contains("queue identity is immutable"));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_job_schema_rejects_foreign_execution_and_a_non_workflow_job() {
    let f = JobFixture::new().await;
    let p = &f.binding.fixture.persistence;
    let company = CompanyPersistence::create(
        p,
        f.binding.target.actor.user_id(),
        CompanyWrite {
            name: "Other".into(),
            slug: "other".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        p,
        company.id,
        ChannelWrite {
            name: "Other".into(),
            slug: "other".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut tx = f.hypothetical().await;
    let error = sqlx::query("INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type, queue_kind, workflow_execution_id, payload) VALUES ($1, $2, $3, $4, 'workflow_execution', 'workflow', $5, $6)")
        .bind(Uuid::new_v4()).bind(company.id).bind(channel.id).bind(Uuid::new_v4())
        .bind(f.execution).bind(f.payload()).execute(&mut *tx).await.unwrap_err();
    constraint(error, "background_tasks_workflow_execution_fk");
    tx.rollback().await.unwrap();
    let mut tx = f.hypothetical().await;
    // Scoped exactly like the execution, so only the job's own kind is wrong.
    let error = sqlx::query("INSERT INTO background_tasks (id, company_id, channel_id, thread_id, correlation_id, task_type, workflow_execution_id, payload) VALUES ($1, $2, $3, $4, $5, 'inbound_email', $6, $7)")
        .bind(Uuid::new_v4()).bind(f.binding.target.company.as_uuid()).bind(f.channel).bind(f.thread)
        .bind(Uuid::new_v4()).bind(f.execution).bind(f.payload()).execute(&mut *tx).await.unwrap_err();
    constraint(error, "background_tasks_workflow_shape_check");
    tx.rollback().await.unwrap();
    let mut tx = f.hypothetical().await;
    let id = f.insert(&mut tx, f.payload()).await.unwrap();
    let error = sqlx::query("UPDATE background_tasks SET queue_kind = 'other', workflow_execution_id = NULL WHERE id = $1")
        .bind(id).execute(&mut *tx).await.unwrap_err();
    assert!(error.to_string().contains("queue identity is immutable"));
    tx.rollback().await.unwrap();
}
