//! Independent handoff consumers rely on the database's live legacy run invariant.
use super::super::auxiliary_fixtures;
use super::*;
use crate::adapters::persistence::thread_handoff::{
    DraftedHandoffRun, HandoffRunEnd, complete_handoff_run_on, fail_handoff_run_on,
    handoff_run_for_task_on, resolve_handoff_for_draft_on,
};
use crate::application::thread_handoff::ThreadHandoffPolicyPersistence;
use crate::entities::transport::PrincipalId;

struct Handoff {
    id: Uuid,
    generation: Uuid,
    draft: Uuid,
    principal: PrincipalId,
}

async fn handoff(f: &JobFixture, db: &mut PgConnection, task: Uuid) -> Handoff {
    auxiliary_fixtures::seed(f, db, task).await;
    let (id, generation, principal): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT handoff_id, generation, requested_by_principal_id
         FROM thread_handoff_runs WHERE task_id = $1",
    )
    .bind(task)
    .fetch_one(&mut *db)
    .await
    .unwrap();
    sqlx::query("UPDATE thread_handoffs SET state = 'drafting', responsible_principal_id = $2 WHERE id = $1")
        .bind(id).bind(principal).execute(&mut *db).await.unwrap();
    let draft: Uuid = sqlx::query_scalar("SELECT id FROM response_drafts WHERE task_id = $1")
        .bind(task)
        .fetch_one(db)
        .await
        .unwrap();
    Handoff {
        id,
        generation,
        draft,
        principal: PrincipalId::new(principal),
    }
}

async fn finish(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company: Uuid,
    task: Uuid,
    h: &Handoff,
) -> AppResult<()> {
    let run = handoff_run_for_task_on(tx, company, task).await?.unwrap();
    complete_handoff_run_on(
        tx,
        DraftedHandoffRun {
            company_id: company,
            task_id: task,
            run,
            draft_id: h.draft,
            draft_version: 1,
            agent: h.principal,
        },
    )
    .await
}

async fn refuse_workflow_run(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company: Uuid,
    workflow: Uuid,
    task: Uuid,
    h: &Handoff,
) {
    sqlx::query("SAVEPOINT workflow_run")
        .execute(&mut **tx)
        .await
        .unwrap();
    let error = sqlx::query(
        "INSERT INTO thread_handoff_runs (company_id, task_id, handoff_id, generation,
         requested_by_principal_id, command_id) VALUES ($1, $2, $3, $4, $5, gen_random_uuid())",
    )
    .bind(company)
    .bind(workflow)
    .bind(h.id)
    .bind(h.generation)
    .bind(h.principal.as_uuid())
    .execute(&mut **tx)
    .await
    .unwrap_err();
    constraint(error, "legacy_auxiliary_task_queue_kind");
    sqlx::query("ROLLBACK TO SAVEPOINT workflow_run")
        .execute(&mut **tx)
        .await
        .unwrap();
    assert!(
        handoff_run_for_task_on(tx, company, workflow)
            .await
            .unwrap()
            .is_none()
    );
    fail_handoff_run_on(tx, company, workflow, HandoffRunEnd::TaskFailed, "workflow")
        .await
        .unwrap();
    sqlx::query("SAVEPOINT workflow_complete")
        .execute(&mut **tx)
        .await
        .unwrap();
    let run = handoff_run_for_task_on(tx, company, task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        complete_handoff_run_on(
            tx,
            DraftedHandoffRun {
                company_id: company,
                task_id: workflow,
                run,
                draft_id: h.draft,
                draft_version: 1,
                agent: h.principal,
            }
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    sqlx::query("ROLLBACK TO SAVEPOINT workflow_complete")
        .execute(&mut **tx)
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_job_handoff_consumers_refuse_workflow_run_and_preserve_legacy_completion() {
    let f = JobFixture::new().await;
    let p = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let mut tx = f.hypothetical().await;
    let workflow = f.insert(&mut tx, f.payload()).await.unwrap();
    let task = legacy(&f, &mut tx).await;
    let h = handoff(&f, &mut tx, task).await;
    refuse_workflow_run(&mut tx, company, workflow, task, &h).await;
    finish(&mut tx, company, task, &h).await.unwrap();
    tx.commit().await.unwrap(); // Independent connections observe these isolated fixture rows.
    let before: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(handoff) FROM thread_handoffs AS handoff WHERE id = $1",
    )
    .bind(h.id)
    .fetch_one(p.pool())
    .await
    .unwrap();
    p.expire_thread_handoff_draft(company, workflow, "workflow")
        .await
        .unwrap();
    let after: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(handoff) FROM thread_handoffs AS handoff WHERE id = $1",
    )
    .bind(h.id)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(before, after);
    let draft = p
        .thread_handoff_draft(company, h.id, h.generation, &[f.channel])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.task_id, task);
    assert_eq!(draft.draft_id, h.draft);
    assert!(
        p.thread_handoff_draft(company, h.id, h.generation, &[])
            .await
            .unwrap()
            .is_none()
    );
    let mut tx = p.pool().begin().await.unwrap();
    resolve_handoff_for_draft_on(&mut tx, company, h.draft, h.principal, Uuid::new_v4())
        .await
        .unwrap();
    let events: Vec<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT operation, task_id FROM thread_handoff_events WHERE handoff_id = $1 ORDER BY to_version",
    ).bind(h.id).fetch_all(&mut *tx).await.unwrap();
    assert_eq!(
        events,
        vec![
            ("draft_ready".into(), Some(task)),
            ("resolved".into(), None)
        ]
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn workflow_job_handoff_independent_expiry_keeps_legacy_event_and_is_idempotent() {
    let f = JobFixture::new().await;
    let p = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let mut tx = p.pool().begin().await.unwrap();
    let task = legacy(&f, &mut tx).await;
    let h = handoff(&f, &mut tx, task).await;
    finish(&mut tx, company, task, &h).await.unwrap();
    tx.commit().await.unwrap();
    p.expire_thread_handoff_draft(Uuid::new_v4(), task, "foreign")
        .await
        .unwrap();
    for _ in 0..2 {
        p.expire_thread_handoff_draft(company, task, "expired")
            .await
            .unwrap();
    }
    let states: (String, String) = sqlx::query_as(
        "SELECT run.state, handoff.state FROM thread_handoff_runs AS run
         JOIN thread_handoffs AS handoff ON handoff.id = run.handoff_id WHERE run.task_id = $1",
    )
    .bind(task)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(states, ("failed".into(), "needs_instruction".into()));
    let events: Vec<(String, Option<Uuid>, Option<String>)> = sqlx::query_as(
        "SELECT operation, task_id, failure_reason FROM thread_handoff_events
         WHERE handoff_id = $1 ORDER BY to_version",
    )
    .bind(h.id)
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(
        events,
        vec![
            ("draft_ready".into(), Some(task), None),
            ("draft_failed".into(), Some(task), Some("expired".into())),
        ]
    );
}
