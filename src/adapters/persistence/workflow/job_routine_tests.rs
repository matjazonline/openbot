//! Exercise indirect legacy triggers against hypothetical workflow jobs in an isolated DB.
use super::*;

use super::notification_fixtures;

#[path = "job_source_association_tests.rs"]
mod source_association_tests;

#[path = "job_auxiliary_association_tests.rs"]
mod auxiliary_association_tests;
#[path = "job_auxiliary_fixtures.rs"]
mod auxiliary_fixtures;

#[path = "job_handoff_history_tests.rs"]
mod handoff_history_tests;

#[path = "job_tenant_association_tests.rs"]
mod tenant_association_tests;

async fn legacy(f: &JobFixture, db: &mut PgConnection) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type) VALUES ($1, $2, $3, $1, 'inbound_email')")
        .bind(id).bind(f.binding.target.company.as_uuid()).bind(f.channel)
        .execute(db).await.unwrap();
    id
}

async fn agent(f: &JobFixture, db: &mut PgConnection) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents (id, company_id, name, slug, created_by) VALUES ($1, $2, 'Worker', 'worker', $3)")
        .bind(id).bind(f.binding.target.company.as_uuid())
        .bind(json!({"actor_type":"system", "actor_id":null, "actor_name":"Test"}))
        .execute(&mut *db).await.unwrap();
    sqlx::query("INSERT INTO channel_agents (company_id, channel_id, agent_id, position) VALUES ($1, $2, $3, 0)")
        .bind(f.binding.target.company.as_uuid()).bind(f.channel).bind(id)
        .execute(&mut *db).await.unwrap();
    sqlx::query("INSERT INTO principals (id, company_id, kind, agent_id, display_label) VALUES (gen_random_uuid(), $1, 'agent', $2, 'Worker')")
        .bind(f.binding.target.company.as_uuid()).bind(id).execute(db).await.unwrap();
    id
}

#[tokio::test]
async fn workflow_job_routines_harness_and_principal_removal_preserve_isolation() {
    let f = JobFixture::new().await;
    let mut tx = f.hypothetical().await;
    let worker = agent(&f, &mut tx).await;
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    sqlx::query("UPDATE agents SET harness_kind = 'ai_agents' WHERE id = $1")
        .bind(worker)
        .execute(&mut *tx)
        .await
        .unwrap();
    let task = legacy(&f, &mut tx).await;
    sqlx::query("SAVEPOINT harness")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("UPDATE agents SET harness_kind = 'rig' WHERE id = $1")
        .bind(worker)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    constraint(error, "agent_harness_has_unsettled_tasks");
    sqlx::query("ROLLBACK TO SAVEPOINT harness")
        .execute(&mut *tx)
        .await
        .unwrap();
    // Agent deletion cascades to the principal, whose trigger releases legacy ownership.
    sqlx::query("DELETE FROM agents WHERE id = $1")
        .bind(worker)
        .execute(&mut *tx)
        .await
        .unwrap();
    let released: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_ownership_events WHERE task_id = $1 AND operation = 'owner_removed'")
        .bind(task).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(released, 1);
    let untouched: bool = sqlx::query_scalar("SELECT status = 'pending' AND ownership_version = 1 AND owner_principal_id IS NULL FROM background_tasks WHERE id = $1")
        .bind(workflow).fetch_one(&mut *tx).await.unwrap();
    assert!(untouched);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_job_routines_channel_target_cleanup_only_deletes_legacy() {
    let f = JobFixture::new().await;
    let target = ChannelPersistence::create(
        &f.binding.fixture.persistence,
        f.binding.target.company.as_uuid(),
        ChannelWrite {
            name: "Target".into(),
            slug: "target".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut tx = f.hypothetical().await;
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    let task = legacy(&f, &mut tx).await;
    let thread = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'Target')",
    )
    .bind(thread)
    .bind(f.binding.target.company.as_uuid())
    .bind(target.id)
    .execute(&mut *tx)
    .await
    .unwrap();
    for id in [task] {
        sqlx::query("INSERT INTO task_channel_targets (task_id, company_id, channel_id, thread_id, recipient_role, position) VALUES ($1, $2, $3, $4, 'to', 0)")
            .bind(id).bind(f.binding.target.company.as_uuid()).bind(target.id).bind(thread).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(target.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let remaining: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM background_tasks")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(remaining, vec![workflow]);
    tx.rollback().await.unwrap();
}

async fn outreach(f: &JobFixture, db: &mut PgConnection, task: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO task_outreaches (id, task_id, company_id, status, required_threshold_percent, expires_at, outreach_key, subject, body) VALUES ($1, $2, $3, 'waiting', 100, CURRENT_TIMESTAMP + interval '1 hour', $1::text, 'Subject', 'Body')")
        .bind(id).bind(task).bind(f.binding.target.company.as_uuid()).execute(&mut *db).await.unwrap();
    sqlx::query("UPDATE task_outreaches SET status = 'timeout_pending_approval' WHERE id = $1")
        .bind(id)
        .execute(db)
        .await
        .unwrap();
    id
}

async fn ownership(f: &JobFixture, db: &mut PgConnection, task: Uuid) {
    sqlx::query("INSERT INTO task_ownership_events (task_id, company_id, sequence, from_version, to_version, command_id, command_fingerprint, operation, actor_kind, previous_owner_kind, new_owner_kind, reason) VALUES ($1, $2, 2, 1, 2, gen_random_uuid(), 'test', 'release', 'system', 'unassigned', 'unassigned', 'released')")
        .bind(task).bind(f.binding.target.company.as_uuid()).execute(db).await.unwrap();
}

#[tokio::test]
async fn workflow_job_routines_notifications_only_project_legacy() {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let mut listener = sqlx::postgres::PgListener::connect_with(pool)
        .await
        .unwrap();
    listener
        .listen_all([
            "task_chain_changed",
            "task_ownership_changed",
            "attention_changed",
            "workflow_test_done",
        ])
        .await
        .unwrap();
    let mut tx = f.hypothetical().await;
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    let correlation: Uuid =
        sqlx::query_scalar("SELECT correlation_id FROM background_tasks WHERE id = $1")
            .bind(workflow)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let task = legacy(&f, &mut tx).await;
    let legacy_outreach = outreach(&f, &mut tx, task).await;
    ownership(&f, &mut tx, task).await;
    for id in [workflow, task] {
        sqlx::query(
            "SELECT enqueue_actionable_notification_event($1, 'task', $2, 'task_failure', NULL)",
        )
        .bind(f.binding.target.company.as_uuid())
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    // The source-association constraint now makes workflow draft/delivery/approval
    // records impossible; source_association_tests covers raw INSERT and UPDATE rejection.
    let forbidden = vec![workflow, correlation];
    let mut allowed = notification_fixtures::auxiliary_sources(&f, &mut tx, Some(task)).await;
    allowed.extend(notification_fixtures::auxiliary_sources(&f, &mut tx, None).await);
    let sources: Vec<Uuid> = sqlx::query_scalar("SELECT source_id FROM notification_events")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert!(sources.contains(&task) && sources.contains(&legacy_outreach));
    assert!(sources.iter().all(|id| !forbidden.contains(id)));
    // Draft and delivery sources enqueue events; approvals emit attention wakeups only.
    for id in [allowed[1], allowed[2], allowed[4], allowed[5]] {
        assert!(sources.contains(&id));
    }
    finish_notifications(tx).await;
    assert_notifications(&mut listener, &allowed, &forbidden).await;
}

async fn assert_notifications(
    listener: &mut sqlx::postgres::PgListener,
    allowed: &[Uuid],
    forbidden_ids: &[Uuid],
) {
    let mut seen = std::collections::HashSet::new();
    loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), listener.recv())
            .await
            .unwrap()
            .unwrap();
        if event.channel() == "workflow_test_done" {
            break;
        }
        for forbidden in forbidden_ids {
            assert!(
                !event.payload().contains(&forbidden.to_string()),
                "{}: {}",
                event.channel(),
                event.payload()
            );
        }
        for id in allowed {
            if event.payload().contains(&id.to_string()) {
                seen.insert(*id);
            }
        }
    }
    assert!(
        allowed.iter().all(|id| seen.contains(id)),
        "every legacy/taskless source emitted a wakeup"
    );
}

async fn finish_notifications(mut tx: sqlx::Transaction<'_, sqlx::Postgres>) {
    // Delete tasks FIRST: legacy sources retain their normal SET NULL/cascade behavior,
    // and workflow tasks cannot leave provenance-losing draft/delivery/approval sources.
    sqlx::query("DELETE FROM background_tasks")
        .execute(&mut *tx)
        .await
        .unwrap();
    let attached: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM response_drafts WHERE task_id IS NOT NULL) + \
                (SELECT COUNT(*) FROM message_deliveries WHERE task_id IS NOT NULL) + \
                (SELECT COUNT(*) FROM human_approvals WHERE task_id IS NOT NULL)",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(attached, 0);
    sqlx::query("DELETE FROM response_drafts")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM human_approvals")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM message_deliveries")
        .execute(&mut *tx)
        .await
        .unwrap();
    // Commit notification delivery only after removing fixture rows.
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT pg_notify('workflow_test_done', 'done')")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
