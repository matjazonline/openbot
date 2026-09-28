//! Exact workflow scope and legacy isolation on the fully migrated schema.
use super::*;
use std::time::Duration;

pub(super) async fn assert_nullable_gate(f: &JobFixture) {
    let required: bool = sqlx::query_scalar("SELECT attnotnull FROM pg_attribute WHERE attrelid = 'background_tasks'::regclass AND attname = 'channel_id'")
        .fetch_one(f.binding.fixture.persistence.pool()).await.unwrap();
    assert!(!required);
}

pub(super) async fn scoped_job(
    db: &mut PgConnection,
    company: Uuid,
    execution: Uuid,
    association: RunAssociation,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO background_tasks (id, company_id, channel_id, thread_id, correlation_id, task_type, queue_kind, workflow_execution_id, payload) VALUES ($1, $2, $3, $4, $1, 'workflow_execution', 'workflow', $5, jsonb_build_object('version', 1, 'execution_id', $5::uuid::text))")
        .bind(id).bind(company).bind(association.channel).bind(association.thread).bind(execution).execute(db).await?;
    Ok(id)
}

pub(super) async fn scoped_execution(f: &JobFixture, association: RunAssociation) -> Uuid {
    let mut db = f
        .binding
        .fixture
        .persistence
        .pool()
        .acquire()
        .await
        .unwrap();
    let run = Uuid::new_v4();
    let execution = Uuid::new_v4();
    insert_run(&mut db, &f.binding, run, association)
        .await
        .unwrap();
    insert_execution(&mut db, f.binding.target.company.as_uuid(), run, execution)
        .await
        .unwrap();
    execution
}

#[tokio::test]
async fn workflow_job_association_nullable_scope_matrix_and_shared_attempts() {
    let f = JobFixture::new().await;
    assert_nullable_gate(&f).await;
    let company = f.binding.target.company.as_uuid();
    let scopes = [
        RunAssociation::default(),
        RunAssociation {
            channel: Some(f.channel),
            thread: None,
        },
        RunAssociation {
            channel: Some(f.channel),
            thread: Some(f.thread),
        },
    ];
    for (index, scope) in scopes.iter().copied().enumerate() {
        let execution = scoped_execution(&f, scope).await;
        let mut db = f
            .binding
            .fixture
            .persistence
            .pool()
            .acquire()
            .await
            .unwrap();
        let job = scoped_job(&mut db, company, execution, scope)
            .await
            .unwrap();
        let saved: (Option<Uuid>, Option<Uuid>) =
            sqlx::query_as("SELECT channel_id, thread_id FROM background_tasks WHERE id = $1")
                .bind(job)
                .fetch_one(&mut *db)
                .await
                .unwrap();
        assert_eq!(saved, (scope.channel, scope.thread));
        sqlx::query("INSERT INTO task_attempts (id, task_id, attempt_number, status, execution_generation, worker_id, machine_id) VALUES (gen_random_uuid(), $1, 1, 'processing', gen_random_uuid(), gen_random_uuid(), 'association-test')")
            .bind(job).execute(&mut *db).await.unwrap();
        for (other_index, other) in scopes.iter().copied().enumerate() {
            if index == other_index {
                continue;
            }
            constraint(
                scoped_job(&mut db, company, execution, other)
                    .await
                    .unwrap_err(),
                "background_tasks_workflow_association",
            );
            let error = sqlx::query(
                "UPDATE background_tasks SET channel_id = $2, thread_id = $3 WHERE id = $1",
            )
            .bind(job)
            .bind(other.channel)
            .bind(other.thread)
            .execute(&mut *db)
            .await
            .unwrap_err();
            constraint(error, "background_tasks_workflow_association");
        }
    }
    let mut db = f
        .binding
        .fixture
        .persistence
        .pool()
        .acquire()
        .await
        .unwrap();
    let error = sqlx::query("INSERT INTO background_tasks (id, company_id, correlation_id, task_type) VALUES (gen_random_uuid(), $1, gen_random_uuid(), 'inbound_email')")
        .bind(company).execute(&mut *db).await.unwrap_err();
    constraint(error, "background_tasks_legacy_channel_required");
}

#[tokio::test]
async fn workflow_job_association_parents_and_scope_reject_mutation() {
    let f = JobFixture::new().await;
    assert_nullable_gate(&f).await;
    let pool = f.binding.fixture.persistence.pool();
    let mut db = pool.acquire().await.unwrap();
    f.insert(&mut db, f.payload()).await.unwrap();
    for field in ["company_id", "id", "channel_id", "thread_id"] {
        let error = sqlx::query(&format!(
            "UPDATE workflow_runs SET {field} = $2 WHERE id = $1"
        ))
        .bind(f.run)
        .bind(Uuid::new_v4())
        .execute(&mut *db)
        .await
        .unwrap_err();
        constraint(error, "workflow_run_association_immutable");
    }
    for field in ["company_id", "id", "run_id"] {
        let error = sqlx::query(&format!(
            "UPDATE workflow_executions SET {field} = $2 WHERE id = $1"
        ))
        .bind(f.execution)
        .bind(Uuid::new_v4())
        .execute(&mut *db)
        .await
        .unwrap_err();
        constraint(error, "workflow_execution_identity_immutable");
    }
    for assignment in ["step_id = 'other'", "activation = 2"] {
        let error = sqlx::query(&format!(
            "UPDATE workflow_executions SET {assignment} WHERE id = $1"
        ))
        .bind(f.execution)
        .execute(&mut *db)
        .await
        .unwrap_err();
        constraint(error, "workflow_execution_identity_immutable");
    }
    for scope in [
        RunAssociation {
            channel: Some(Uuid::new_v4()),
            thread: Some(f.thread),
        },
        RunAssociation {
            channel: Some(f.channel),
            thread: Some(Uuid::new_v4()),
        },
    ] {
        constraint(
            scoped_job(
                &mut db,
                f.binding.target.company.as_uuid(),
                f.execution,
                scope,
            )
            .await
            .unwrap_err(),
            "background_tasks_workflow_association",
        );
    }
}

async fn wait_for_lock(pool: &sqlx::PgPool, pid: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT COALESCE(wait_event_type = 'Lock', false) FROM pg_stat_activity WHERE pid = $1")
                .bind(pid).fetch_one(pool).await.unwrap();
            if waiting { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("actual competing parent update/job insert must wait");
}

struct ParentMutation {
    table: &'static str,
    id: Uuid,
    assignment: &'static str,
    expected: &'static str,
}

fn parent_mutations(f: &JobFixture) -> [ParentMutation; 2] {
    [
        ParentMutation {
            table: "workflow_runs",
            id: f.run,
            assignment: "channel_id = NULL, thread_id = NULL",
            expected: "workflow_run_association_immutable",
        },
        ParentMutation {
            table: "workflow_executions",
            id: f.execution,
            assignment: "run_id = gen_random_uuid()",
            expected: "workflow_execution_identity_immutable",
        },
    ]
}

#[tokio::test]
async fn workflow_job_association_insert_serializes_parent_update() {
    let f = JobFixture::new().await;
    assert_nullable_gate(&f).await;
    let pool = f.binding.fixture.persistence.pool();
    for ParentMutation {
        table,
        id,
        assignment,
        expected,
    } in parent_mutations(&f)
    {
        let mut writer = pool.begin().await.unwrap();
        let job = f.insert(&mut writer, f.payload()).await.unwrap();
        let mut editor = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *editor)
            .await
            .unwrap();
        let update = tokio::spawn(async move {
            let error = sqlx::query(&format!("UPDATE {table} SET {assignment} WHERE id = $1"))
                .bind(id)
                .execute(&mut *editor)
                .await
                .unwrap_err();
            constraint(error, expected);
            editor.rollback().await.unwrap();
        });
        wait_for_lock(pool, pid).await;
        writer.commit().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), update)
            .await
            .unwrap()
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM background_tasks WHERE id = $1 AND channel_id = $2 AND thread_id = $3")
            .bind(job).bind(f.channel).bind(f.thread).fetch_one(pool).await.unwrap();
        assert_eq!(count, 1);
        sqlx::query("DELETE FROM background_tasks WHERE id = $1")
            .bind(job)
            .execute(pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn workflow_job_association_prior_parent_update_serializes_insert() {
    let f = JobFixture::new().await;
    assert_nullable_gate(&f).await;
    let pool = f.binding.fixture.persistence.pool();
    for ParentMutation {
        table,
        id,
        assignment,
        expected,
    } in parent_mutations(&f)
    {
        let mut editor = pool.begin().await.unwrap();
        sqlx::query(&format!("UPDATE {table} SET id = id WHERE id = $1"))
            .bind(id)
            .execute(&mut *editor)
            .await
            .unwrap();
        let mut writer = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *writer)
            .await
            .unwrap();
        let company = f.binding.target.company.as_uuid();
        let execution = f.execution;
        let association = RunAssociation {
            channel: Some(f.channel),
            thread: Some(f.thread),
        };
        let insert = tokio::spawn(async move {
            let job = scoped_job(&mut writer, company, execution, association)
                .await
                .unwrap();
            writer.commit().await.unwrap();
            job
        });
        wait_for_lock(pool, pid).await;
        let error = sqlx::query(&format!("UPDATE {table} SET {assignment} WHERE id = $1"))
            .bind(id)
            .execute(&mut *editor)
            .await
            .unwrap_err();
        constraint(error, expected);
        editor.rollback().await.unwrap();
        let job = tokio::time::timeout(Duration::from_secs(5), insert)
            .await
            .unwrap()
            .unwrap();
        sqlx::query("DELETE FROM background_tasks WHERE id = $1")
            .bind(job)
            .execute(pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn workflow_job_association_nullable_jobs_survive_legacy_control_and_recovery() {
    use crate::{
        entities::task::{ResumeActor, StopActor},
        task_queue::TaskPersistence,
    };
    let f = JobFixture::new().await;
    assert_nullable_gate(&f).await;
    let p = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let mut jobs = Vec::new();
    for scope in [
        RunAssociation::default(),
        RunAssociation {
            channel: Some(f.channel),
            thread: None,
        },
        RunAssociation {
            channel: Some(f.channel),
            thread: Some(f.thread),
        },
    ] {
        let execution = scoped_execution(&f, scope).await;
        let job = scoped_job(
            &mut p.pool().acquire().await.unwrap(),
            company,
            execution,
            scope,
        )
        .await
        .unwrap();
        jobs.push(job);
    }
    let deadline = chrono::Utc::now() + chrono::Duration::minutes(5);
    assert!(
        p.claim_pending_tasks(Uuid::new_v4(), deadline, 10)
            .await
            .unwrap()
            .is_empty()
    );
    for job in &jobs {
        assert!(!p.claim_task(*job, Uuid::new_v4(), deadline).await.unwrap());
        assert!(p.get_task_by_id(*job).await.unwrap().is_none());
        assert!(
            p.stop_task(*job, StopActor::Operator(Uuid::new_v4()))
                .await
                .is_err()
        );
        assert!(
            p.resume_task(*job, ResumeActor::Operator(Uuid::new_v4()))
                .await
                .is_err()
        );
        assert!(
            p.list_task_attempts(company, *job)
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        p.list_company_tasks_page(company, None, None, false, 0, 20)
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("UPDATE background_tasks SET status = 'processing', worker_id = gen_random_uuid(), execution_generation = gen_random_uuid(), locked_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes', lock_expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute' WHERE id = ANY($1)")
        .bind(&jobs).execute(p.pool()).await.unwrap();
    let before: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT to_jsonb(task) FROM background_tasks AS task WHERE id = ANY($1) ORDER BY id",
    )
    .bind(&jobs)
    .fetch_all(p.pool())
    .await
    .unwrap();
    p.reap_expired_task_leases().await.unwrap();
    let after: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT to_jsonb(task) FROM background_tasks AS task WHERE id = ANY($1) ORDER BY id",
    )
    .bind(&jobs)
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(before, after);
}
