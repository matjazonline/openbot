use super::*;

#[tokio::test]
async fn channel_reply_hitl_prompt_ownership_cannot_be_edited_or_removed() {
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    for query in [
        "UPDATE workflow_channel_questions SET respondent_id=$2 WHERE run_id=$1",
        "UPDATE workflow_channel_questions SET source_message_id=$2 WHERE run_id=$1",
    ] {
        let error = sqlx::query(query)
            .bind(asked.run.as_uuid())
            .bind(Uuid::new_v4())
            .execute(s.pool())
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("workflow_guard:workflow_question_guard")
        );
    }
    let error = sqlx::query("DELETE FROM workflow_channel_questions WHERE run_id=$1")
        .bind(asked.run.as_uuid())
        .execute(s.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:workflow_question_guard")
    );
}

#[tokio::test]
async fn channel_reply_hitl_prompt_failed_atomic_write_leaves_no_prompt_or_wait() {
    let s = Box::pin(Intents::new()).await;
    let pending = Box::pin(pending(&s, 1)).await;
    sqlx::raw_sql("CREATE FUNCTION reject_test_question() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test question failure' USING ERRCODE='23514'; END $$; CREATE TRIGGER reject_test_question BEFORE INSERT ON workflow_channel_questions FOR EACH ROW EXECUTE FUNCTION reject_test_question();")
        .execute(s.pool()).await.unwrap();
    let outcome = runtime(s.p())
        .execute(&WorkflowStepKind("message.ask".into()), &pending.claim)
        .await;
    assert!(
        matches!(outcome, WorkflowHandlerOutcome::Completed(Err(_))),
        "{outcome:?}"
    );
    let c = counts(&s, pending.scope.run).await;
    assert_eq!(c.questions, 0);
    assert_eq!(c.waits, 0);
    assert_eq!(c.destinations, 0);
    assert_eq!(c.dispatches, 0);
    let canonical: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messages WHERE company_id=$1 AND direction='outbound'",
    )
    .bind(pending.scope.company.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(canonical, 0);
}

#[tokio::test]
async fn channel_reply_hitl_prompt_canonical_content_and_delivery_membership_are_frozen() {
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let delivery = delivery(s.pool(), asked.run, 0).await;
    let error = sqlx::query(
        "UPDATE messages SET clean_text_body='edited question' WHERE company_id=$1 AND id=$2",
    )
    .bind(asked.answer.company.as_uuid())
    .bind(asked.question)
    .execute(s.pool())
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    let extra = Uuid::new_v4();
    let error=sqlx::query("INSERT INTO message_deliveries SELECT (jsonb_populate_record(NULL::message_deliveries,to_jsonb(original)||jsonb_build_object('id',$2::uuid,'idempotency_key',$3::text))).* FROM message_deliveries AS original WHERE original.id=$1")
        .bind(delivery).bind(extra).bind(format!("question-extra-{extra}"))
        .execute(s.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    let error = sqlx::query(
        "UPDATE thread_messages SET thread_id=$3 WHERE company_id=$1 AND message_id=$2",
    )
    .bind(asked.answer.company.as_uuid())
    .bind(asked.question)
    .bind(Uuid::new_v4())
    .execute(s.pool())
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    let size: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND message_id=$2",
    )
    .bind(asked.answer.company.as_uuid())
    .bind(asked.question)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(size, 1);
}

#[tokio::test]
async fn channel_reply_hitl_prompt_true_company_teardown_removes_its_owned_question_and_unknown_delivery()
 {
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let delivery = delivery(s.pool(), asked.run, 0).await;
    deliver(s.pool(), delivery, "unknown").await;
    s.p()
        .consume_message_receipts(asked.answer, lease())
        .await
        .unwrap();
    let company = asked.answer.company.as_uuid();
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(company)
        .execute(s.pool())
        .await
        .unwrap();
    let questions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_channel_questions WHERE company_id=$1")
            .bind(company)
            .fetch_one(s.pool())
            .await
            .unwrap();
    let deliveries: i64 =
        sqlx::query_scalar("SELECT count(*) FROM message_deliveries WHERE company_id=$1")
            .bind(company)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(questions, 0);
    assert_eq!(deliveries, 0);
}
