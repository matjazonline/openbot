use super::*;
use binding_tests::BindingFixture;
use serde_json::json;

#[path = "job_schema_tests.rs"]
mod job_schema_tests;

#[derive(Clone, Copy, Default)]
struct RunAssociation {
    channel: Option<Uuid>,
    thread: Option<Uuid>,
}

async fn insert_run(
    db: &mut PgConnection,
    f: &BindingFixture,
    run: Uuid,
    association: RunAssociation,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO workflow_runs (company_id, id, binding_id, binding_revision, workflow_id, version_id, actor_id, trigger_id, correlation_id, bundle, input, params, resources, max_steps, max_context_bytes, deadline, channel_id, thread_id) SELECT company_id, $2, $3, 1, workflow_id, id, actor_id, $4, $5, bundle, $6, $7, '{}'::jsonb, 100, 1048576, CURRENT_TIMESTAMP + interval '1 hour', $9, $10 FROM workflow_versions WHERE company_id = $1 AND id = $8")
        .bind(f.target.company.as_uuid()).bind(run).bind(f.target.binding.as_uuid())
        .bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(json!({"input": "original"}))
        .bind(json!({"parameter": 7})).bind(f.request(None).version.as_uuid()).bind(association.channel).bind(association.thread).execute(db).await?;
    Ok(())
}

async fn insert_execution(
    db: &mut PgConnection,
    company: Uuid,
    run: Uuid,
    execution: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO workflow_executions (company_id, run_id, id, step_id, activation) VALUES ($1, $2, $3, 'entry', 1)")
        .bind(company).bind(run).bind(execution).execute(db).await?;
    Ok(())
}

fn foreign_key(error: sqlx::Error) {
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation(),
        "{error}"
    );
}

#[tokio::test]
async fn workflow_run_schema_preserves_snapshot_after_binding_removal_and_scopes_children() {
    let f = BindingFixture::new(json!([])).await;
    f.service().configure(f.request(None)).await.unwrap();
    let company = f.target.company.as_uuid();
    let pool = f.fixture.persistence.pool();
    let run = Uuid::new_v4();
    let sibling = Uuid::new_v4();
    let execution = Uuid::new_v4();
    let mut db = pool.acquire().await.unwrap();
    insert_run(&mut db, &f, run, RunAssociation::default())
        .await
        .unwrap();
    insert_run(&mut db, &f, sibling, RunAssociation::default())
        .await
        .unwrap();
    insert_execution(&mut db, company, run, execution)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workflow_bindings WHERE company_id = $1 AND id = $2")
        .bind(company)
        .bind(f.target.binding.as_uuid())
        .execute(&mut *db)
        .await
        .unwrap();
    #[derive(sqlx::FromRow)]
    struct Snapshot {
        input: serde_json::Value,
        params: serde_json::Value,
        binding_id: Uuid,
        binding_revision: i64,
        bundle: Vec<u8>,
    }
    let saved: Snapshot = sqlx::query_as("SELECT input, params, binding_id, binding_revision, bundle FROM workflow_runs WHERE company_id = $1 AND id = $2")
        .bind(company).bind(run).fetch_one(&mut *db).await.unwrap();
    assert_eq!(saved.input, json!({"input":"original"}));
    assert_eq!(saved.params, json!({"parameter":7}));
    assert_eq!(saved.binding_id, f.target.binding.as_uuid());
    assert_eq!(saved.binding_revision, 1);
    let expected: Vec<u8> = sqlx::query_scalar(
        "SELECT bundle FROM workflow_versions WHERE company_id = $1 AND id = $2",
    )
    .bind(company)
    .bind(f.request(None).version.as_uuid())
    .fetch_one(&mut *db)
    .await
    .unwrap();
    assert_eq!(saved.bundle, expected);
    foreign_key(
        insert_execution(&mut db, Uuid::new_v4(), run, Uuid::new_v4())
            .await
            .unwrap_err(),
    );
    foreign_key(sqlx::query("INSERT INTO workflow_waits (company_id, run_id, execution_id, id, reason, deadline) VALUES ($1, $2, $3, $4, 'event', CURRENT_TIMESTAMP + interval '1 hour')")
        .bind(company).bind(sibling).bind(execution).bind(Uuid::new_v4()).execute(&mut *db).await.unwrap_err());
    assert!(
        insert_execution(&mut db, company, run, Uuid::new_v4())
            .await
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .is_unique_violation()
    );
    sqlx::query("INSERT INTO workflow_waits (company_id, run_id, execution_id, id, reason, deadline) VALUES ($1, $2, $3, $4, 'event', CURRENT_TIMESTAMP + interval '1 hour')")
        .bind(company).bind(run).bind(execution).bind(Uuid::new_v4()).execute(&mut *db).await.unwrap();
}

async fn compete_admission(
    f: &BindingFixture,
    run: Uuid,
    barrier: &tokio::sync::Barrier,
) -> Result<(), sqlx::Error> {
    let mut tx = f.fixture.persistence.pool().begin().await?;
    insert_run(&mut tx, f, run, RunAssociation::default()).await?;
    barrier.wait().await;
    sqlx::query("INSERT INTO workflow_admissions (company_id, binding_id, source_key, run_id) VALUES ($1, $2, 'v1:manual:shared', $3)")
        .bind(f.target.company.as_uuid()).bind(f.target.binding.as_uuid()).bind(run)
        .execute(&mut *tx).await?;
    tx.commit().await
}

#[tokio::test]
async fn workflow_run_schema_competing_source_claims_rollback_losing_run_and_scope_aliases() {
    let f = BindingFixture::new(json!([])).await;
    let company = f.target.company.as_uuid();
    let barrier = tokio::sync::Barrier::new(2);
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            compete_admission(&f, Uuid::new_v4(), &barrier),
            compete_admission(&f, Uuid::new_v4(), &barrier)
        )
    })
    .await
    .unwrap();
    assert_ne!(a.is_ok(), b.is_ok());
    let loser = a.err().or_else(|| b.err()).unwrap();
    assert!(loser.as_database_error().unwrap().is_unique_violation());
    let pool = f.fixture.persistence.pool();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_runs WHERE company_id = $1")
        .bind(company)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let run: Uuid =
        sqlx::query_scalar("SELECT run_id FROM workflow_admissions WHERE company_id = $1")
            .bind(company)
            .fetch_one(pool)
            .await
            .unwrap();
    for key in ["original", "alias"] {
        sqlx::query("INSERT INTO workflow_admission_commands (company_id, command_key, run_id, trigger_id) VALUES ($1, $2, $3, $4)")
            .bind(company).bind(key).bind(run).bind(Uuid::new_v4()).execute(pool).await.unwrap();
    }
    foreign_key(sqlx::query("INSERT INTO workflow_admission_commands (company_id, command_key, run_id, trigger_id) VALUES ($1, 'foreign', $2, $3)")
        .bind(Uuid::new_v4()).bind(run).bind(Uuid::new_v4()).execute(pool).await.unwrap_err());
    let mut db = pool.acquire().await.unwrap();
    let sibling = Uuid::new_v4();
    insert_run(&mut db, &f, sibling, RunAssociation::default())
        .await
        .unwrap();
    foreign_key(sqlx::query("INSERT INTO workflow_admissions (company_id, binding_id, source_key, run_id) VALUES ($1, $2, 'v1:manual:other', $3)")
        .bind(company).bind(Uuid::new_v4()).bind(sibling).execute(&mut *db).await.unwrap_err());
}

#[tokio::test]
async fn workflow_run_schema_final_audit_failure_rolls_back_all_admission_records() {
    let f = BindingFixture::new(json!([])).await;
    let company = f.target.company.as_uuid();
    let pool = f.fixture.persistence.pool();
    let run = Uuid::new_v4();
    let history = HistoryFixture::new(&f).await;
    let mut tx = pool.begin().await.unwrap();
    insert_run(
        &mut tx,
        &f,
        run,
        RunAssociation {
            channel: Some(history.channel),
            thread: Some(history.thread),
        },
    )
    .await
    .unwrap();
    insert_execution(&mut tx, company, run, Uuid::new_v4())
        .await
        .unwrap();
    history
        .insert(&mut tx, company, run, history.messages[0], 1)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workflow_admissions (company_id, binding_id, source_key, run_id) VALUES ($1, $2, 'v1:manual:rollback', $3)")
        .bind(company).bind(f.target.binding.as_uuid()).bind(run).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_admission_commands (company_id, command_key, run_id, trigger_id) VALUES ($1, 'rollback', $2, $3)")
        .bind(company).bind(run).bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
    assert!(sqlx::query("INSERT INTO workflow_run_events (company_id, run_id, sequence, event_kind, actor_id) VALUES ($1, $2, 0, 'admitted', $3)")
        .bind(company).bind(run).bind(f.target.actor.user_id()).execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    for table in [
        "workflow_runs",
        "workflow_executions",
        "workflow_admissions",
        "workflow_admission_commands",
        "workflow_run_events",
        "workflow_history_members",
    ] {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {table} WHERE company_id = $1"
        ))
        .bind(company)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(count, 0, "{table}");
    }
}

struct HistoryFixture {
    channel: Uuid,
    thread: Uuid,
    messages: [Uuid; 2],
}
impl HistoryFixture {
    async fn new(f: &BindingFixture) -> Self {
        use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
        let company = f.target.company.as_uuid();
        let channel = ChannelPersistence::create(
            &f.fixture.persistence,
            company,
            ChannelWrite {
                name: "History".into(),
                slug: "history".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let history = Self {
            channel: channel.id,
            thread: Uuid::new_v4(),
            messages: [Uuid::new_v4(), Uuid::new_v4()],
        };
        let pool = f.fixture.persistence.pool();
        sqlx::query("INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'History')")
            .bind(history.thread).bind(company).bind(history.channel).execute(pool).await.unwrap();
        for message in history.messages {
            sqlx::query("INSERT INTO messages (id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash) SELECT $1, $2, id, 'History', 'body', 'inbound', 'human', $3, $4 FROM principals WHERE company_id = $2 AND user_id = $5")
                .bind(message).bind(company).bind(Uuid::new_v4()).bind(vec![0_u8; 32]).bind(f.target.actor.user_id()).execute(pool).await.unwrap();
            sqlx::query("INSERT INTO thread_messages (id, company_id, channel_id, thread_id, message_id, entry_kind) VALUES ($1, $2, $3, $4, $5, 'conversation')")
                .bind(Uuid::new_v4()).bind(company).bind(history.channel).bind(history.thread).bind(message).execute(pool).await.unwrap();
        }
        history
    }
    async fn insert(
        &self,
        db: &mut PgConnection,
        company: Uuid,
        run: Uuid,
        message: Uuid,
        ordinal: i32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO workflow_history_members (company_id, run_id, channel_id, thread_id, message_id, ordinal) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(company).bind(run).bind(self.channel).bind(self.thread).bind(message).bind(ordinal).execute(db).await?;
        Ok(())
    }
}

#[tokio::test]
async fn workflow_run_schema_history_membership_roundtrip_scope_and_bounds() {
    let f = BindingFixture::new(json!([])).await;
    let history = HistoryFixture::new(&f).await;
    let company = f.target.company.as_uuid();
    let run = Uuid::new_v4();
    let other_run = Uuid::new_v4();
    let mut db = f.fixture.persistence.pool().acquire().await.unwrap();
    insert_run(
        &mut db,
        &f,
        run,
        RunAssociation {
            channel: Some(history.channel),
            thread: Some(history.thread),
        },
    )
    .await
    .unwrap();
    insert_run(&mut db, &f, other_run, RunAssociation::default())
        .await
        .unwrap();

    history
        .insert(&mut db, company, run, history.messages[0], 1)
        .await
        .unwrap();
    let saved: Vec<Uuid> = sqlx::query_scalar("SELECT message_id FROM workflow_history_members WHERE company_id = $1 AND run_id = $2 ORDER BY ordinal")
        .bind(company).bind(run).fetch_all(&mut *db).await.unwrap();
    assert_eq!(saved, vec![history.messages[0]]);
    foreign_key(
        history
            .insert(&mut db, Uuid::new_v4(), run, history.messages[1], 2)
            .await
            .unwrap_err(),
    );
    foreign_key(
        history
            .insert(&mut db, company, other_run, history.messages[1], 2)
            .await
            .unwrap_err(),
    );
    foreign_key(
        history
            .insert(&mut db, company, run, Uuid::new_v4(), 2)
            .await
            .unwrap_err(),
    );
    let wrong_thread = HistoryFixture {
        thread: Uuid::new_v4(),
        ..history
    };
    foreign_key(
        wrong_thread
            .insert(&mut db, company, run, history.messages[1], 2)
            .await
            .unwrap_err(),
    );
    assert!(
        history
            .insert(&mut db, company, run, history.messages[1], 1)
            .await
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .is_unique_violation()
    );
    for ordinal in [0, 10001] {
        assert_eq!(
            history
                .insert(&mut db, company, run, history.messages[1], ordinal)
                .await
                .unwrap_err()
                .as_database_error()
                .unwrap()
                .code()
                .as_deref(),
            Some("23514")
        );
    }
    history
        .insert(&mut db, company, run, history.messages[1], 10000)
        .await
        .unwrap();
}
