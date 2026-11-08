use super::*;
use crate::application::workflow::message_admission::{MessageAdmissions, MessageIntentWorkerId};
use crate::entities::access_level::AccessLevel;

async fn team_member(d: &Deployment) -> String {
    let (user, email) = verified_member(&d.fixture).await;
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(user)
        .execute(d.p().pool())
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(d.fixture.target.company.as_uuid())
        .bind(user)
        .execute(d.p().pool())
        .await
        .unwrap();
    ChannelPersistence::update(
        d.p(),
        crate::application::ports::channel::ChannelUpdate {
            company_id: d.fixture.target.company.as_uuid(),
            channel_id: d.channel,
            write: ChannelWrite {
                name: "Support".into(),
                slug: "support".into(),
                enabled: true,
                add_3rd_party: true,
                participant_emails: None,
                ..Default::default()
            },
        },
    )
    .await
    .unwrap();
    email
}

pub(super) async fn materialize(d: &Deployment, payload: RawInboundPayload) -> RunId {
    let incoming = d.email(payload).await;
    assert_eq!(incoming.intent_ids.len(), 1);
    let admissions =
        MessageAdmissions::new(d.p().clone(), d.engine.services().service(), d.p().clone());
    let intents = admissions
        .claim(MessageIntentWorkerId(Uuid::new_v4()), 4)
        .await
        .unwrap();
    assert_eq!(intents.len(), 1);
    admissions.process(intents[0].clone()).await.unwrap();
    let run: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND trigger_id=$2")
            .bind(d.fixture.target.company.as_uuid())
            .bind(incoming.intent_ids[0])
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    RunId::new(run)
}

fn from_member(d: &Deployment, member: &str, key: &str) -> RawInboundPayload {
    let mut email = customer_email(d, key, None);
    email.from = member.into();
    email.spf = crate::entities::auth::AuthVerdict::Pass;
    email.dkim = crate::entities::auth::AuthVerdict::Pass;
    email.dmarc = crate::entities::auth::AuthVerdict::Pass;
    email
}

#[tokio::test]
async fn agent_history_channel_customer_cc_after_output_refuses_release_and_retains_classified_exchange()
 {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"PRIVATE accepted agent output"}"#,
    )]))
    .await;
    let member = team_member(&d).await;
    let mut selected = example(3);
    prepend_agent(&mut selected);
    Box::pin(bind(&d, selected)).await;
    let run = materialize(&d, from_member(&d, &member, "history-member")).await;
    let mut tx = d.p().pool().begin().await.unwrap();
    // This case isolates native classified-output/release ownership. Actual knowledge
    // acquisition is covered by the genuine search/human-resume cases and final journey.
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        d.fixture.target.company,
        run,
        AccessLevel::L5,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(d.run_until(
        "classified output accepted before actual human question",
        async || waiting(&d, 1).await && delivered(&d, 1).await,
    ))
    .await;
    let before = exchanges(&d).await;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0]["state"], "completed");
    assert_eq!(before[0]["level"], 5);
    assert_eq!(
        before[0]["output"],
        json!({"body":"PRIVATE accepted agent output"})
    );
    let (_, rfc) = prompt(&d, 0).await;
    let mut response = from_member(&d, &member, "history-member-answer");
    response.cc = Some(CUSTOMER.into());
    response.headers = Some(format!(
        "Message-ID: <history-member-answer@partner.test>\nIn-Reply-To: {rfc}\nReferences: {rfc}\n"
    ));
    response.text = Some("Please proceed".into());
    d.email(response).await;
    Box::pin(d.run_until(
        "current public audience refuses final publication",
        async || {
            let state: String =
                sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
                    .bind(d.fixture.target.company.as_uuid())
                    .bind(run.as_uuid())
                    .fetch_one(d.p().pool())
                    .await
                    .unwrap();
            state == "failed"
        },
    ))
    .await;
    let provider = delivery.stop().await;
    assert_eq!(exchanges(&d).await, before);
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        1
    );
    assert!(responses_published(&d, 0).await);
    let refusals:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND event_kind='knowledge.audience_below_content'")
        .bind(d.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    assert_eq!(refusals, 1);
    let fabricated:i64=sqlx::query_scalar("SELECT count(*) FROM task_attempts AS attempt JOIN background_tasks AS job ON job.id=attempt.task_id JOIN workflow_run_events AS event ON event.company_id=job.company_id AND event.execution_id=job.workflow_execution_id WHERE event.company_id=$1 AND event.run_id=$2 AND event.event_kind='knowledge.audience_below_content'")
        .bind(d.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    assert_eq!(fabricated, 0);
    let messages = provider.messages();
    assert_eq!(messages.len(), 1);
    assert!(!messages[0].email.body_text.contains("PRIVATE accepted"));
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}
