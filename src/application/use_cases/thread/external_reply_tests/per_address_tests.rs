//! Address boundaries survive durable reload, task execution, and the email sent to the customer.
use super::*;
use crate::adapters::persistence::test_support::own_database;
use crate::entities::message::ThreadEntryKind;
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};

#[path = "test_support.rs"]
mod support;
use support::*;

const SUPPORT: &str = "Your Team upgrade is ready.";
const SALES: &str = "Your Team price is confirmed.";
const BILLING: &str = "March invoice has been resent.";
const BODY: &str = "Please upgrade to Team. @billing, resend March's invoice.";

#[tokio::test]
async fn separately_addressed_teams_run_in_the_worker_and_send_their_own_replies() {
    let Some(database) = own_database().await else {
        return;
    };
    let mut billing_llm = scripted_llm(vec![LlmTurn::text(BILLING)]).await;
    let mut support_llm = scripted_llm(vec![LlmTurn::text(SUPPORT)]).await;
    let mut sales_llm = scripted_llm(vec![LlmTurn::text(SALES)]).await;
    let fx = fixture(database.pool.clone(), &billing_llm.base_url).await;
    let support = extra_channel(&fx, "Support", &support_llm.base_url).await;
    let sales = extra_channel(&fx, "Sales", &sales_llm.base_url).await;
    let raw = addressed(
        &fx,
        &address(&fx, "support+sales"),
        Some(&format!("{}, bob@client.com", fx.channel_address())),
        BODY,
    );
    let ingest = fx.threads.ingest_test_email(raw.clone()).await.unwrap();
    assert!(ingest.accepted);
    assert_eq!(ingest.task_ids.len(), 2);
    let support_task = ingest.task_ids[0];
    let billing_task = ingest.task_ids[1];
    assert_board_and_activity(
        &fx,
        &ingest.task_ids,
        &[support.id, sales.id, fx.channel.id],
    )
    .await;

    // Guarantee Support publishes first, then exercise Billing's actual already-replied guard.
    make_due_at(&fx, billing_task, Utc::now() + chrono::Duration::hours(1)).await;
    work_until(&fx, &[support_task], |tasks| {
        tasks[0].status == TaskStatus::Completed
    })
    .await;
    let billing_thread = task(&fx, billing_task).await.thread_id.unwrap();
    let context = fx.threads.get_agent_history(billing_thread).await.unwrap();
    assert!(context.iter().any(
        |entry| entry.entry_kind == ThreadEntryKind::Delegation && entry.body.contains(SUPPORT)
    ));
    deliver_on(&fx, &support).await;
    make_due_at(&fx, billing_task, Utc::now()).await;
    work_until(&fx, &[billing_task], |tasks| {
        tasks[0].status == TaskStatus::Completed
    })
    .await;
    deliver_on(&fx, &fx.channel).await;

    assert_worked_example_mail(&fx);
    let billing_prompt = prompt(&mut billing_llm);
    assert!(
        !billing_prompt.contains("--- Step"),
        "a separate address has no upstream pipeline output"
    );
    assert!(billing_prompt.contains("Delegation") && billing_prompt.contains(SUPPORT));
    assert!(!prompt(&mut support_llm).contains("--- Step"));
    let sales_prompt = prompt(&mut sales_llm);
    assert!(sales_prompt.contains("--- Step 1") && sales_prompt.contains(SUPPORT));
    assert_reply_kinds(&fx, support.id, SUPPORT, BILLING).await;
    assert_reply_kinds(&fx, sales.id, SUPPORT, BILLING).await;
    assert_reply_kinds(&fx, fx.channel.id, BILLING, SUPPORT).await;
    assert_redelivery(&fx, raw, &ingest.task_ids).await;
    assert_eq!(billing_llm.finish().await, Ok(1));
    assert_eq!(support_llm.finish().await, Ok(1));
    assert_eq!(sales_llm.finish().await, Ok(1));
}

fn assert_worked_example_mail(fx: &Fixture) {
    let mails = fx.transport.sent();
    assert_eq!(mails.len(), 2);
    let support_address = address(fx, "support");
    let support = mails
        .iter()
        .find(|mail| mail.from.as_str() == support_address)
        .unwrap();
    let billing = mails
        .iter()
        .find(|mail| mail.from.as_str() == fx.channel_address())
        .unwrap();
    for mail in &mails {
        assert_eq!(
            mail.recipients_to,
            vec![EmailAddress::from(fx.customer_email.clone())]
        );
        assert_eq!(
            mail.in_reply_to.as_ref().map(MessageId::as_str),
            Some(CUSTOMER_FIRST_MESSAGE_ID)
        );
        assert_eq!(mail.subject, "Re: Upgrade and invoice");
    }
    assert_ne!(support.message_id, billing.message_id);
    assert_eq!(
        support.recipients_cc,
        vec![EmailAddress::from("bob@client.com")]
    );
    assert!(billing.recipients_cc.is_empty());
    assert!(
        support
            .body_text
            .contains(&format!("[Support ({}", address(fx, "support")))
    );
    assert!(
        support
            .body_text
            .contains(&format!("[Sales ({}", address(fx, "sales")))
    );
    assert!(support.body_text.contains(SUPPORT) && support.body_text.contains(SALES));
    assert!(!support.body_text.contains(BILLING));
    assert_eq!(billing.body_text, agent_response_body(BILLING));
}

#[tokio::test]
async fn a_second_to_pipeline_replies_from_its_own_first_channel() {
    let Some(database) = own_database().await else {
        return;
    };
    let mut first_llm = scripted_llm(vec![LlmTurn::text("First address answer.")]).await;
    let mut third_llm = scripted_llm(vec![LlmTurn::text("Third channel answer.")]).await;
    let mut fourth_llm = scripted_llm(vec![LlmTurn::text("Fourth channel answer.")]).await;
    let fx = fixture(database.pool.clone(), &first_llm.base_url).await;
    let first = extra_channel(&fx, "Target1", &first_llm.base_url).await;
    let third = extra_channel(&fx, "Target3", &third_llm.base_url).await;
    let fourth = extra_channel(&fx, "Target4", &fourth_llm.base_url).await;
    let to = format!(
        "{}, {}",
        address(&fx, "target1"),
        address(&fx, "target3+target4")
    );
    let ingest = fx
        .threads
        .ingest_test_email(addressed(&fx, &to, None, "Each team, please answer."))
        .await
        .unwrap();
    assert_eq!(ingest.task_ids.len(), 2);
    work_until(&fx, &ingest.task_ids, |tasks| {
        tasks
            .iter()
            .all(|task| task.status == TaskStatus::Completed)
    })
    .await;
    deliver_on(&fx, &first).await;
    deliver_on(&fx, &third).await;
    let mails = fx.transport.sent();
    let first_mail = mails
        .iter()
        .find(|mail| mail.from.as_str() == address(&fx, "target1"))
        .unwrap();
    let pipeline_mail = mails
        .iter()
        .find(|mail| mail.from.as_str() == address(&fx, "target3"))
        .unwrap();
    assert_eq!(
        first_mail.body_text,
        agent_response_body("First address answer.")
    );
    assert!(
        pipeline_mail.body_text.contains("[Target3 (")
            && pipeline_mail.body_text.contains("[Target4 (")
    );
    assert!(
        pipeline_mail.body_text.contains("Third channel answer.")
            && pipeline_mail.body_text.contains("Fourth channel answer.")
    );
    assert!(!pipeline_mail.body_text.contains("First address answer."));
    assert!(!prompt(&mut first_llm).contains("--- Step"));
    assert!(!prompt(&mut third_llm).contains("--- Step"));
    let last_prompt = prompt(&mut fourth_llm);
    assert!(last_prompt.contains("--- Step 1") && last_prompt.contains("Third channel answer."));
    assert_reply_kinds(
        &fx,
        fourth.id,
        "Third channel answer.",
        "First address answer.",
    )
    .await;
    assert_eq!(first_llm.finish().await, Ok(1));
    assert_eq!(third_llm.finish().await, Ok(1));
    assert_eq!(fourth_llm.finish().await, Ok(1));
}

#[tokio::test]
async fn a_failing_sibling_retries_without_repeating_the_successful_reply() {
    let Some(database) = own_database().await else {
        return;
    };
    let mut failure = ScriptedResponse::json(
        serde_json::json!({"error": {"message": "fixture provider refused"}}),
    );
    failure.status = 400;
    let billing_llm = scripted_scenario(vec![ScriptedExchange::new(|_| Ok(()), failure)]).await;
    let support_llm = scripted_llm(vec![LlmTurn::text(SUPPORT)]).await;
    let fx = fixture(database.pool.clone(), &billing_llm.base_url).await;
    let support = extra_channel(&fx, "Support", &support_llm.base_url).await;
    let raw = addressed(
        &fx,
        &address(&fx, "support"),
        Some(&fx.channel_address()),
        BODY,
    );
    let ingest = fx.threads.ingest_test_email(raw).await.unwrap();
    assert_eq!(ingest.task_ids.len(), 2);
    let support_task = ingest.task_ids[0];
    let billing_task = ingest.task_ids[1];
    work_until(&fx, &ingest.task_ids, |tasks| {
        tasks[0].status == TaskStatus::Completed && tasks[1].retry_count == 1
    })
    .await;
    deliver_on(&fx, &support).await;
    let failed = task(&fx, billing_task).await;
    assert_eq!(failed.status, TaskStatus::Pending);
    assert!(failed.run_at > Utc::now());
    assert_eq!(task(&fx, support_task).await.retry_count, 0);
    assert_eq!(
        fx.transport.only_mail().body_text,
        agent_response_body(SUPPORT)
    );
    assert_eq!(billing_llm.finish().await, Ok(1));

    // A repaired provider only reruns Billing. The completed sibling has no second model turn.
    let repaired = scripted_llm(vec![LlmTurn::text(BILLING)]).await;
    let agent_id = fx.channel.agent_ids.as_ref().unwrap()[0];
    register_scripted_agent_base_url(agent_id, &repaired.base_url);
    make_due_at(&fx, billing_task, Utc::now()).await;
    work_until(&fx, &[billing_task], |tasks| {
        tasks[0].status == TaskStatus::Completed
    })
    .await;
    deliver_on(&fx, &fx.channel).await;
    assert_eq!(fx.transport.sent().len(), 2);
    assert_eq!(task(&fx, support_task).await.retry_count, 0);
    let attempts: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT task_id, count(*) FROM task_attempts WHERE task_id = ANY($1) GROUP BY task_id",
    )
    .bind(&ingest.task_ids)
    .fetch_all(&fx.pool)
    .await
    .unwrap();
    assert!(attempts.contains(&(support_task, 1)));
    assert!(attempts.contains(&(billing_task, 2)));
    assert_eq!(support_llm.finish().await, Ok(1));
    assert_eq!(repaired.finish().await, Ok(1));
}
