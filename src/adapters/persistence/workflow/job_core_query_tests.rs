//! Disposable databases expose all workflow associations to independent legacy claim connections.
use super::*;
use crate::{
    adapters::persistence::task::{BEGIN_ATTEMPT_SQL, BackgroundTaskDb, FINISH_ATTEMPT_SQL},
    entities::task::{BackgroundTask, ResumeActor, StopActor},
    task_queue::TaskPersistence,
};

async fn mixed_fixture() -> (JobFixture, Uuid, Uuid) {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let mut db = pool.acquire().await.unwrap();
    super::job_association_tests::assert_nullable_gate(&f).await;
    for association in [
        RunAssociation::default(),
        RunAssociation {
            channel: Some(f.channel),
            thread: None,
        },
    ] {
        let execution = super::job_association_tests::scoped_execution(&f, association).await;
        super::job_association_tests::scoped_job(
            &mut db,
            f.binding.target.company.as_uuid(),
            execution,
            association,
        )
        .await
        .unwrap();
    }
    let workflow = f.insert(&mut db, f.payload()).await.unwrap();
    let agent = Uuid::new_v4();
    sqlx::query("INSERT INTO agents (id, company_id, name, slug, created_by) VALUES ($1, $2, 'Worker', 'worker', $3)")
        .bind(agent).bind(f.binding.target.company.as_uuid())
        .bind(json!({"actor_type":"system", "actor_id":null, "actor_name":"Test"}))
        .execute(&mut *db).await.unwrap();
    sqlx::query("INSERT INTO channel_agents (company_id, channel_id, agent_id, position) VALUES ($1, $2, $3, 0)")
        .bind(f.binding.target.company.as_uuid()).bind(f.channel).bind(agent)
        .execute(&mut *db).await.unwrap();
    sqlx::query("INSERT INTO principals (id, company_id, kind, agent_id, display_label) VALUES (gen_random_uuid(), $1, 'agent', $2, 'Worker')")
        .bind(f.binding.target.company.as_uuid()).bind(agent).execute(&mut *db).await.unwrap();
    let legacy = Uuid::new_v4();
    sqlx::query("INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type) VALUES ($1, $2, $3, $1, 'inbound_email')")
        .bind(legacy).bind(f.binding.target.company.as_uuid()).bind(f.channel)
        .execute(&mut *db).await.unwrap();
    drop(db);
    (f, workflow, legacy)
}

#[tokio::test]
async fn workflow_job_core_competing_claimants_and_legacy_reads_are_isolated() {
    let (f, workflow, legacy) = mixed_fixture().await;
    let persistence = &f.binding.fixture.persistence;
    let deadline = chrono::Utc::now() + chrono::Duration::minutes(5);
    let barrier = tokio::sync::Barrier::new(2);
    let claim = || async {
        barrier.wait().await;
        persistence
            .claim_pending_tasks(Uuid::new_v4(), deadline, 1)
            .await
            .unwrap()
    };
    let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(claim(), claim())
    })
    .await
    .unwrap();
    let claimed: Vec<_> = first
        .into_iter()
        .chain(second)
        .map(|task| task.id)
        .collect();
    assert_eq!(claimed, vec![legacy]);
    let thread = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'Targets')",
    )
    .bind(thread)
    .bind(f.binding.target.company.as_uuid())
    .bind(f.channel)
    .execute(persistence.pool())
    .await
    .unwrap();
    for id in [workflow, legacy] {
        let inserted = sqlx::query("INSERT INTO task_channel_targets (task_id, company_id, channel_id, thread_id, recipient_role, position) VALUES ($1, $2, $3, $4, 'to', 0)")
            .bind(id).bind(f.binding.target.company.as_uuid()).bind(f.channel).bind(thread)
            .execute(persistence.pool()).await;
        if id == workflow {
            constraint(inserted.unwrap_err(), "legacy_auxiliary_task_queue_kind");
        } else {
            inserted.unwrap();
        }
    }
    assert!(
        persistence
            .list_task_channel_targets(f.binding.target.company.as_uuid(), workflow)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        persistence
            .list_task_channel_targets(f.binding.target.company.as_uuid(), legacy)
            .await
            .unwrap()
            .len(),
        1
    );

    assert!(
        !persistence
            .claim_task(workflow, Uuid::new_v4(), deadline)
            .await
            .unwrap()
    );
    assert!(
        persistence
            .get_task_by_id(workflow)
            .await
            .unwrap()
            .is_none()
    );
    let listed = persistence
        .list_company_tasks_page(f.binding.target.company.as_uuid(), None, None, false, 0, 20)
        .await
        .unwrap();
    assert_eq!(
        listed.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![legacy]
    );
    assert!(
        persistence
            .stop_task(workflow, StopActor::Operator(Uuid::new_v4()))
            .await
            .is_err()
    );
    assert!(
        persistence
            .resume_task(workflow, ResumeActor::Operator(Uuid::new_v4()))
            .await
            .is_err()
    );
    let state: (String, i32) =
        sqlx::query_as("SELECT status, retry_count FROM background_tasks WHERE id = $1")
            .bind(workflow)
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    assert_eq!(state, ("pending".into(), 0));
}

#[tokio::test]
async fn workflow_job_core_recovery_and_attempt_writes_preserve_workflow_rows() {
    let (f, workflow, legacy) = mixed_fixture().await;
    let p = &f.binding.fixture.persistence;
    let generation = Uuid::new_v4();
    let worker = Uuid::new_v4();
    sqlx::query("UPDATE background_tasks SET status = 'processing', worker_id = $2, execution_generation = $3, locked_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes', lock_expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute' WHERE id = ANY($1)")
        .bind([workflow, legacy].as_slice()).bind(worker).bind(generation).execute(p.pool()).await.unwrap();
    for task in [workflow, legacy] {
        // The shared attempt ledger continues to accept workflow records directly.
        sqlx::query("INSERT INTO task_attempts (id, task_id, attempt_number, execution_generation, status, worker_id, machine_id) VALUES (gen_random_uuid(), $1, 1, $2, 'processing', $3, 'test')")
            .bind(task).bind(generation).bind(worker).execute(p.pool()).await.unwrap();
    }
    let opened = sqlx::query(BEGIN_ATTEMPT_SQL)
        .bind(Uuid::new_v4())
        .bind(workflow)
        .bind(2_i32)
        .bind(generation)
        .bind(worker)
        .bind("test")
        .bind(None::<String>)
        .execute(p.pool())
        .await
        .unwrap();
    assert_eq!(opened.rows_affected(), 0);
    let closed = sqlx::query(FINISH_ATTEMPT_SQL)
        .bind(workflow)
        .bind(1_i32)
        .bind(generation)
        .bind("completed")
        .bind(None::<String>)
        .bind(None::<i32>)
        .bind(None::<i32>)
        .bind("completed")
        .execute(p.pool())
        .await
        .unwrap();
    assert_eq!(closed.rows_affected(), 0);
    assert!(
        p.list_task_attempts(f.binding.target.company.as_uuid(), workflow)
            .await
            .unwrap()
            .is_empty()
    );
    p.reap_expired_task_leases().await.unwrap();
    let rows: Vec<(Uuid, String, i32)> =
        sqlx::query_as("SELECT id, status, retry_count FROM background_tasks ORDER BY id")
            .fetch_all(p.pool())
            .await
            .unwrap();
    assert!(rows.contains(&(workflow, "processing".into(), 0)));
    assert!(rows.contains(&(legacy, "pending".into(), 1)));
    let status: String = sqlx::query_scalar("SELECT status FROM task_attempts WHERE task_id = $1")
        .bind(workflow)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(status, "processing");
    let legacy_attempts = p
        .list_task_attempts(f.binding.target.company.as_uuid(), legacy)
        .await
        .unwrap();
    assert_eq!(legacy_attempts.len(), 1);
}

#[tokio::test]
async fn workflow_job_core_legacy_decode_rejects_a_missing_channel() {
    let (f, _, legacy) = mixed_fixture().await;
    let row = sqlx::query_as::<_, BackgroundTaskDb>(
        "SELECT id, company_id, NULL::uuid AS channel_id, thread_id, correlation_id,
                task_type, status, payload, retry_count, max_retries, last_error,
                business_priority, business_due_at, attention_version, owner_principal_id,
                owner_principal_kind, ownership_version, worker_id, execution_generation,
                locked_at, lock_expires_at, run_at, created_at, updated_at
         FROM background_tasks WHERE id = $1",
    )
    .bind(legacy)
    .fetch_one(f.binding.fixture.persistence.pool())
    .await
    .unwrap();
    let error = BackgroundTask::try_from(row).unwrap_err().to_string();
    assert!(error.contains(&legacy.to_string()) && error.contains("channel_id"));
}

#[tokio::test]
async fn workflow_job_task_projections_exclude_shared_correlations() {
    use crate::{
        application::task_counts::TaskCountsReader, entities::task::TaskBoardFilter,
        task_queue::CollaborationReadScope,
    };
    let (f, workflow, legacy) = mixed_fixture().await;
    let persistence = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let visible = [f.channel];
    let scope = CollaborationReadScope {
        company_id: company,
        visible_channel_ids: &visible,
    };
    let correlation: Uuid =
        sqlx::query_scalar("SELECT correlation_id FROM background_tasks WHERE id = $1")
            .bind(workflow)
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    assert!(
        persistence
            .get_task_chain_detail(scope, correlation.into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        persistence
            .get_collaboration_summary(scope, workflow)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        persistence
            .get_collaboration_summary(scope, legacy)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        persistence
            .task_counts(company, &visible)
            .await
            .unwrap()
            .company
            .pending,
        1
    );
    // A workflow job in the same correlation must neither add a card nor inflate its totals.
    sqlx::query("UPDATE background_tasks SET correlation_id = $2 WHERE id = $1")
        .bind(legacy)
        .bind(correlation)
        .execute(persistence.pool())
        .await
        .unwrap();
    let detail = persistence
        .get_task_chain_detail(scope, correlation.into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        detail
            .tasks
            .iter()
            .map(|entry| entry.task.id)
            .collect::<Vec<_>>(),
        vec![legacy]
    );
    let board = persistence
        .list_task_chain_board(
            company,
            TaskBoardFilter {
                channel_id: None,
                terminal_since: chrono::Utc::now() - chrono::Duration::days(1),
                per_column_limit: 20,
            },
            &visible,
        )
        .await
        .unwrap();
    let cards: Vec<_> = board.cards.values().flatten().collect();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].counts.total_tasks, 1);
}

#[tokio::test]
async fn workflow_job_task_status_history_excludes_auxiliary_events() {
    let (f, workflow, legacy) = mixed_fixture().await;
    let persistence = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    // Raw workflow status events are now rejected at the association boundary.
    let error = sqlx::query("INSERT INTO task_status_events (id, company_id, task_id, correlation_id, sequence, to_status, reason, actor_kind, retry_count, run_at) VALUES (gen_random_uuid(), $1, $2, $3, 1, 'pending', 'enqueued', 'system', 0, CURRENT_TIMESTAMP)")
        .bind(company).bind(workflow).bind(legacy).execute(persistence.pool()).await.unwrap_err();
    constraint(error, "legacy_auxiliary_task_queue_kind");
    let events = persistence
        .list_task_status_events(company, legacy.into(), None, 100)
        .await
        .unwrap();
    assert!(!events.is_empty());
    assert!(events.iter().all(|event| event.task_id == legacy));
}

#[tokio::test]
async fn workflow_job_task_instruction_replay_excludes_auxiliary_rows() {
    use crate::entities::{
        internal_note::{AskOwnerOutcome, AskOwnerToAct},
        transport::PrincipalId,
    };
    use sha2::{Digest, Sha256};
    let (f, workflow, legacy) = mixed_fixture().await;
    let persistence = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let actor: Uuid = sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 LIMIT 1")
        .bind(company)
        .fetch_one(persistence.pool())
        .await
        .unwrap();
    let thread = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'Replay')",
    )
    .bind(thread)
    .bind(company)
    .bind(f.channel)
    .execute(persistence.pool())
    .await
    .unwrap();
    for task_id in [legacy, workflow] {
        let command = AskOwnerToAct {
            company_id: company,
            channel_id: f.channel,
            thread_id: thread,
            task_id,
            expected_ownership_version: 1,
            note_ids: vec![Uuid::new_v4()],
            command_id: Uuid::new_v4(),
            handoff: None,
        };
        let fields = json!({"company_id":company,"channel_id":f.channel,"thread_id":thread,
            "task_id":task_id,"expected_ownership_version":1,"note_ids":command.note_ids,"actor":actor});
        let fingerprint = format!("{:x}", Sha256::digest(fields.to_string().as_bytes()));
        let inserted = sqlx::query("INSERT INTO task_agent_instructions (id, company_id, channel_id, thread_id, task_id, command_id, command_fingerprint, requested_by_principal_id, requested_ownership_version, wake_outcome) VALUES (gen_random_uuid(),$1,$2,$3,$4,$5,$6,$7,1,'queued')")
            .bind(company).bind(f.channel).bind(thread).bind(task_id).bind(command.command_id)
            .bind(fingerprint).bind(actor).execute(persistence.pool()).await;
        if task_id == workflow {
            constraint(inserted.unwrap_err(), "legacy_auxiliary_task_queue_kind");
        } else {
            inserted.unwrap();
        }
        let result = persistence
            .ask_owner_to_act(&command, PrincipalId::new(actor))
            .await;
        if task_id == legacy {
            assert_eq!(result.unwrap(), AskOwnerOutcome::Queued);
        } else {
            assert!(
                result.is_err(),
                "workflow replay must not return a legacy outcome"
            );
        }
    }
}

#[path = "job_external_query_tests.rs"]
mod job_external_query_tests;

#[path = "job_projection_tests.rs"]
mod job_projection_tests;

#[path = "job_projection_index_tests.rs"]
mod job_projection_index_tests;
