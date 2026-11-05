//! Isolated job fixtures use the fully migrated nullable workflow schema.
use super::*;
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::channel::ChannelWrite;

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
        let binding = configured_binding().await;
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

    async fn insert(&self, db: &mut PgConnection) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, workflow_execution_id, thread_id) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(id).bind(self.binding.target.company.as_uuid()).bind(self.channel)
            .bind(Uuid::new_v4()).bind(self.execution).bind(self.thread).execute(db).await?;
        Ok(id)
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
    f.insert(&mut db).await.unwrap();
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
    let original = f.execution;
    f.execution = Uuid::new_v4();
    let mut tx = f.hypothetical().await;
    constraint(
        f.insert(&mut tx).await.unwrap_err(),
        "background_tasks_workflow_execution_fk",
    );
    tx.rollback().await.unwrap();
    f.execution = original;
    let mut tx = f.hypothetical().await;
    let id = f.insert(&mut tx).await.unwrap();
    let saved: Uuid =
        sqlx::query_scalar("SELECT workflow_execution_id FROM background_tasks WHERE id=$1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(saved, f.execution);
    let error = sqlx::query("UPDATE background_tasks SET workflow_execution_id=$2 WHERE id=$1")
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
async fn workflow_job_schema_rejects_foreign_execution_and_missing_owner() {
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
    let error = sqlx::query("INSERT INTO background_tasks (id,company_id,channel_id,correlation_id,workflow_execution_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(company.id).bind(channel.id).bind(Uuid::new_v4())
        .bind(f.execution).execute(&mut *tx).await.unwrap_err();
    constraint(error, "background_tasks_workflow_execution_fk");
    tx.rollback().await.unwrap();
    let required: bool = sqlx::query_scalar("SELECT attnotnull FROM pg_attribute WHERE attrelid='background_tasks'::regclass AND attname='workflow_execution_id'")
        .fetch_one(p.pool()).await.unwrap();
    assert!(required);
    let retired: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_attribute WHERE NOT attisdropped AND ((attrelid='background_tasks'::regclass AND attname IN ('task_type','payload','queue_kind')) OR (attrelid='task_harness_runs'::regclass AND attname='owner_kind'))")
        .fetch_one(p.pool()).await.unwrap();
    assert_eq!(retired, 0);
    let mut tx = f.hypothetical().await;
    let id = f.insert(&mut tx).await.unwrap();
    let error = sqlx::query("UPDATE background_tasks SET workflow_execution_id=NULL WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("queue identity is immutable"));
    tx.rollback().await.unwrap();
}
