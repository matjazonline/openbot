//! Actual legacy source records exercise their installed notification triggers.
use super::*;

pub(super) async fn auxiliary_sources(
    f: &JobFixture,
    db: &mut PgConnection,
    task: Option<Uuid>,
) -> Vec<Uuid> {
    let company = f.binding.target.company.as_uuid();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company)
            .bind(f.binding.target.actor.user_id())
            .fetch_one(&mut *db)
            .await
            .unwrap();
    let thread = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1, $2, $3, 'Sources')",
    )
    .bind(thread)
    .bind(company)
    .bind(f.channel)
    .execute(&mut *db)
    .await
    .unwrap();
    let approval = Uuid::new_v4();
    sqlx::query("INSERT INTO human_approvals (id, company_id, channel_id, thread_id, task_id, step_key, approver_email, action_type, action_title, action_summary, token, expires_at) VALUES ($1, $2, $3, $4, $5, 'step', 'review@example.test', 'test', 'Review', 'Summary', gen_random_uuid(), CURRENT_TIMESTAMP + interval '1 hour')")
        .bind(approval).bind(company).bind(f.channel).bind(thread).bind(task).execute(&mut *db).await.unwrap();
    let draft = Uuid::new_v4();
    sqlx::query("INSERT INTO response_drafts (id, version, company_id, channel_id, thread_id, task_id, author_principal_id, reviewer_principal_id, proposed_message_id, subject, body, attachment_snapshot, recipient_snapshot, transport_snapshot, publication_snapshot, created_by_principal_id, updated_by_principal_id) VALUES ($1, 1, $2, $3, $4, $5, $6, $6, gen_random_uuid(), 'Review', 'Body', '{\"version\":\"1\",\"items\":[]}', '{\"version\":\"1\",\"to\":[],\"cc\":[]}', '{\"version\":\"1\"}', '{\"version\":\"1\"}', $6, $6)")
        .bind(draft).bind(company).bind(f.channel).bind(thread).bind(task).bind(principal).execute(&mut *db).await.unwrap();
    sqlx::query("INSERT INTO response_reviews (company_id, draft_id, draft_version, reviewer_principal_id, expires_at) VALUES ($1, $2, 1, $3, CURRENT_TIMESTAMP + interval '1 hour')")
        .bind(company).bind(draft).bind(principal).execute(&mut *db).await.unwrap();
    let delivery = delivery(f, db, task, principal).await;
    vec![approval, draft, delivery]
}

async fn delivery(
    f: &JobFixture,
    db: &mut PgConnection,
    task: Option<Uuid>,
    principal: Uuid,
) -> Uuid {
    let company = f.binding.target.company.as_uuid();
    let binding: Uuid = sqlx::query_scalar("SELECT id FROM channel_bindings WHERE company_id = $1 AND channel_id = $2 AND transport = 'email'")
        .bind(company).bind(f.channel).fetch_one(&mut *db).await.unwrap();
    let message = Uuid::new_v4();
    sqlx::query("INSERT INTO messages (id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash, audience) VALUES ($1, $2, $3, 'Test', 'Body', 'outbound', 'system', $1, decode(repeat('00',32),'hex'), 'external_conversation')")
        .bind(message).bind(company).bind(principal).execute(&mut *db).await.unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO message_deliveries (id, company_id, channel_id, message_id, source_binding_id, destination_binding_id, task_id, correlation_id, transport, purpose, idempotency_key, max_attempts, message_audience) VALUES ($1, $2, $3, $4, $5, $5, $6, $1, 'email', 'reply', $1::text, 3, 'external_conversation')")
        .bind(id).bind(company).bind(f.channel).bind(message).bind(binding).bind(task).execute(&mut *db).await.unwrap();
    sqlx::query("UPDATE message_deliveries SET status = 'dead_letter' WHERE id = $1")
        .bind(id)
        .execute(db)
        .await
        .unwrap();
    id
}
