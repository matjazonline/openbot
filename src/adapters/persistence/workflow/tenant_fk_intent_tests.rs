//! fix-app 05 batches 2 and 5 against PostgreSQL: an admitted intent names its own run's binding,
//! and a settled reply intent no longer blocks the deletes that remove it.
use super::*;
use crate::application::use_cases::channel::ChannelPersistence;
use crate::application::use_cases::company::CompanyPersistence;

fn foreign_key(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|error| error.is_foreign_key_violation())
}

async fn insert_outcome(
    s: &Intents,
    intent: Uuid,
    binding: WorkflowBindingId,
    run: Uuid,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO workflow_message_intent_bindings (company_id, intent_id, binding_id, \
           binding_revision, outcome, run_id) VALUES ($1, $2, $3, 1, 'admitted', $4)",
    )
    .bind(s.fixture.target.company.as_uuid())
    .bind(intent)
    .bind(binding.as_uuid())
    .bind(run)
    .execute(s.pool())
    .await
}

/// An `admitted` outcome may name only a run of the binding it is the outcome for.
#[tokio::test]
async fn run_binding_fk_intent_outcome_names_its_own_runs_binding() {
    let s = Intents::new().await;
    let first = s.binding(MessageTrigger::Any, None).await;
    let second = s.binding(MessageTrigger::Any, None).await;
    let admitted = s.intent(TO).await;
    assert_eq!(s.drain(&s.admissions(admission(s.p()))).await, 1);
    assert_eq!(s.admitted(admitted).await, ids(&[first, second]));
    let first_run = s.runs(first).await[0];
    let second_run = s.runs(second).await[0];

    // A fresh intent, so the outcome rows below differ from the admitted ones only in the run.
    let probe = s.intent(CC).await;
    sqlx::query("UPDATE workflow_message_intents SET available_at = clock_timestamp() + interval '1 day' WHERE id = $1")
        .bind(probe)
        .execute(s.pool())
        .await
        .unwrap();
    let crossed = insert_outcome(&s, probe, first, second_run)
        .await
        .unwrap_err();
    assert!(foreign_key(&crossed), "{crossed}");
    assert_eq!(
        crossed.as_database_error().unwrap().constraint(),
        Some("workflow_message_intent_binding_run")
    );
    insert_outcome(&s, probe, first, first_run).await.unwrap();
}

/// A run names a binding revision that exists.
#[tokio::test]
async fn run_binding_fk_run_cannot_name_an_unknown_revision() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    s.intent(TO).await;
    assert_eq!(s.drain(&s.admissions(admission(s.p()))).await, 1);
    let run = s.runs(binding).await[0];
    let refused = sqlx::query(
        "UPDATE workflow_runs SET binding_revision = binding_revision + 1 WHERE company_id = $1 AND id = $2",
    )
    .bind(s.fixture.target.company.as_uuid())
    .bind(run)
    .execute(s.pool())
    .await
    .unwrap_err();
    assert!(foreign_key(&refused), "{refused}");
    assert_eq!(
        refused.as_database_error().unwrap().constraint(),
        Some("workflow_runs_binding_revision_fk")
    );
}

/// Deleting a channel with runs is still refused before SQL, exactly as before the run's binding
/// gained its foreign key; the runs and their binding revision are untouched.
#[tokio::test]
async fn run_binding_fk_channel_with_runs_is_still_refused() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    s.intent(TO).await;
    assert_eq!(s.drain(&s.admissions(admission(s.p()))).await, 1);
    let refused = ChannelPersistence::delete(s.p(), s.channel).await;
    assert!(matches!(refused, Err(AppError::Conflict(_))), "{refused:?}");
    assert_eq!(s.runs(binding).await.len(), 1);
}

/// A settled reply intent whose reply target the intent cannot outlive.
async fn settled_reply(s: &Intents) -> Uuid {
    let intent = s.intent(TO).await;
    let answered = s.message().await;
    sqlx::query(
        "UPDATE workflow_message_intents SET reply_to_message_id = $2, reply_settled = true, \
           available_at = clock_timestamp() + interval '1 day' WHERE id = $1",
    )
    .bind(intent)
    .bind(answered.as_uuid())
    .execute(s.pool())
    .await
    .unwrap();
    intent
}

async fn intents_left(s: &Intents) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_message_intents WHERE company_id = $1")
        .bind(s.fixture.target.company.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap()
}

/// `workflow_message_intent_reply_to` is a deferred NO ACTION key, not `SET NULL` (which the
/// settled-reply check refused): deleting the reply target alone is refused at commit, and deleting
/// the channel or the company succeeds. Deferred, because a thread's messages are deleted by a
/// trigger statement whose own end comes before the outer cascade reaches the intent.
#[tokio::test]
async fn tenant_fk_small_settled_reply_intent_survives_channel_and_company_deletion() {
    let s = Intents::new().await;
    let intent = settled_reply(&s).await;
    let answered: Uuid = sqlx::query_scalar(
        "SELECT reply_to_message_id FROM workflow_message_intents WHERE id = $1",
    )
    .bind(intent)
    .fetch_one(s.pool())
    .await
    .unwrap();
    let refused = sqlx::query("DELETE FROM messages WHERE company_id = $1 AND id = $2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(answered)
        .execute(s.pool())
        .await
        .unwrap_err();
    assert_eq!(
        refused.as_database_error().unwrap().constraint(),
        Some("workflow_message_intent_reply_to"),
        "{refused}"
    );

    // The application refuses a channel with conversations before SQL; the database-level
    // channel delete (its cascade through threads to the intent) is what the constraint governs.
    sqlx::query("DELETE FROM channels WHERE company_id = $1 AND id = $2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(s.channel)
        .execute(s.pool())
        .await
        .unwrap();
    assert_eq!(intents_left(&s).await, 0);

    let s = Intents::new().await;
    settled_reply(&s).await;
    CompanyPersistence::delete(s.p(), s.fixture.target.company.as_uuid())
        .await
        .unwrap();
    assert_eq!(intents_left(&s).await, 0);
}
