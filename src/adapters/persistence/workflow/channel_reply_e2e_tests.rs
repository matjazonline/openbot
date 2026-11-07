//! Receive through the production workflow composition and deliver through the real worker.
use super::*;
#[path = "channel_reply_e2e_support.rs"]
mod support;
use support::*;

#[path = "channel_reply_e2e_hitl_tests.rs"]
mod hitl;

#[path = "channel_reply_e2e_recovery_tests.rs"]
mod recovery;

#[tokio::test]
async fn channel_reply_e2e_selected_binding_manual_and_schedule_runs_never_inherit_destination() {
    use crate::adapters::persistence::workflow::schedule_test_support::schedule_use_cases;
    use crate::application::workflow::manual::{self, StartRun};
    use crate::domain::entities::schedule::{ScheduleTimezone, ScheduleType, ScheduleWrite};
    let d = Box::pin(Deployment::new(vec![])).await;
    let mut selected = example(0);
    selected.source["input_schema"] = json!(true);
    let binding = Box::pin(bind(&d, selected)).await;
    let mut input = customer_email(&d, "non-message-source", None);
    input.text = Some("[[quiet]] Stored source without a response grant".into());
    let incoming = d.email(input).await;
    let source = incoming.inbound_message.unwrap().canonical_id;
    manual::start(
        &d.engine.services().service(),
        StartRun {
            company_id: d.fixture.target.company,
            actor: d.fixture.target.actor,
            binding_id: binding.binding,
            association: RelatedAssociation::Channel(RelatedChannelId::new(d.channel)),
            input: MessageTriggerInput::new(
                source,
                "Manual work",
                "Manual",
                "customer@partner.test",
            )
            .to_value(),
            idempotency_key: "manual-with-source-id".into(),
            rerun_of: None,
        },
    )
    .await
    .unwrap();
    let schedule = schedule_use_cases(d.p())
        .create_schedule(
            d.fixture.target.actor.user_id(),
            d.fixture.target.company.as_uuid(),
            d.channel,
            ScheduleWrite {
                name: "Scheduled work".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(3600),
                scheduled_at: None,
                timezone: ScheduleTimezone::utc(),
                binding_id: binding.binding.as_uuid(),
                enabled: true,
            },
        )
        .await
        .unwrap();
    sqlx::query("UPDATE channel_schedules SET next_run_at=clock_timestamp()-interval '1 minute' WHERE company_id=$1 AND id=$2")
        .bind(d.fixture.target.company.as_uuid()).bind(schedule.id).execute(d.p().pool()).await.unwrap();
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(d.run_until(
        "manual and materialized schedule finish without a message grant",
        async || {
            d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='succeeded'")
                .await
                == 2
                && d.count(
                    "SELECT count(*) FROM schedule_runs WHERE company_id=$1 AND run_id IS NOT NULL",
                )
                .await
                    == 1
        },
    ))
    .await;
    let provider = delivery.stop().await;
    assert!(provider.messages().is_empty());
    assert!(finals(&d).await.is_empty());
    assert_eq!(
        d.count("SELECT count(*) FROM channel_reply_runs WHERE company_id=$1")
            .await,
        0
    );
    assert_eq!(
        d.count("SELECT count(*) FROM channel_reply_contexts WHERE company_id=$1")
            .await,
        0
    );
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn channel_reply_e2e_quiet_ingress_has_no_owner_and_in_app_reply_has_no_transport() {
    use crate::adapters::protocols::email::{
        EmailIngressAdapter, EmailIngressTrust, VerifiedEmailAuth,
    };
    use crate::application::use_cases::thread::{IngressOrigin, ReplyDelivery};
    let d = Box::pin(Deployment::new(vec![])).await;
    Box::pin(bind(&d, example(0))).await;
    for (index, address) in [d.address("support.quiet"), d.address("support")]
        .into_iter()
        .enumerate()
    {
        let mut input = customer_email(&d, &format!("quiet-{index}"), None);
        input.to = address;
        input.text = Some("[[quiet]] Context only".into());
        let receipt = d.email(input).await;
        assert!(!receipt.answers());
    }
    let input = customer_email(&d, "in-app-only", None);
    let trust = EmailIngressTrust::Verified(VerifiedEmailAuth {
        spf: input.spf,
        dkim: input.dkim,
        dmarc: input.dmarc,
        spam_score: input.spam_score,
    });
    let accepted = EmailIngressAdapter::new(APP_DOMAIN)
        .accept(input, trust)
        .unwrap();
    let receipt = d
        .threads
        .ingest(accepted.into_inbound(IngressOrigin::ExternalTransport, ReplyDelivery::InAppOnly))
        .await
        .unwrap();
    assert!(receipt.accepted);
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(d.run_until("quiet intents ignored and in-app final committed",async || {
        responses_published(&d,1).await && d.count("SELECT count(*) FROM workflow_message_intents WHERE company_id=$1 AND outcome IS NOT NULL").await==3
    })).await;
    let provider = delivery.stop().await;
    let rows = finals(&d).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].message.is_some() && rows[0].delivery.is_none());
    assert!(provider.sent.lock().unwrap().is_empty());
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        1
    );
    assert_eq!(
        d.count("SELECT count(*) FROM channel_reply_contexts WHERE company_id=$1")
            .await,
        1
    );
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn channel_reply_e2e_automatic_replies_each_message_once_with_observers() {
    let d = Box::pin(Deployment::new(vec![])).await;
    let binding = Box::pin(bind(&d, example(0))).await;
    for _ in 0..2 {
        Box::pin(d.bind(example(0).source, Some(MessageTrigger::Any))).await;
    }
    let first = d.email(customer_email(&d, "auto-first", None)).await;
    let second = d
        .email(customer_email(&d, "auto-second", Some("auto-first")))
        .await;
    assert_eq!(
        first.thread.as_ref().unwrap().id,
        second.thread.as_ref().unwrap().id
    );
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(d.run_until(
        "two automatic responses and delivered receipts",
        async || {
            responses_published(&d, 2).await
                && delivered(&d, 2).await
                && d.count(
                    "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='succeeded'",
                )
                .await
                    == 6
        },
    ))
    .await;
    let sent = delivery.stop().await;
    let rows = finals(&d).await;
    for (index, ingress) in [&first, &second].iter().enumerate() {
        let source = ingress
            .inbound_message
            .as_ref()
            .unwrap()
            .canonical_id
            .as_uuid();
        let row = rows.iter().find(|row| row.source == source).unwrap();
        assert_eq!(row.thread, ingress.thread.as_ref().unwrap().id);
        assert_eq!(row.output, json!({"body":"Completed"}));
        let deliveries = sent.messages();
        let sent = deliveries
            .iter()
            .filter(|sent| Some(sent.delivery.as_uuid()) == row.delivery)
            .collect::<Vec<_>>();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].email.body_text, "Completed");
        assert_eq!(
            sent[0].email.in_reply_to.as_deref(),
            Some(if index == 0 {
                "<auto-first@partner.test>"
            } else {
                "<auto-second@partner.test>"
            })
        );
        assert_eq!(
            sent[0].email.recipients_to,
            vec![crate::entities::value_objects::EmailAddress::from(CUSTOMER)]
        );
        let owner: Uuid = sqlx::query_scalar("SELECT binding_id FROM workflow_runs WHERE id=$1")
            .bind(row.run)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
        assert_eq!(owner, binding.binding.as_uuid());
    }
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn channel_reply_e2e_silence_preserves_result_and_explicit_final_has_no_duplicate() {
    for index in [1, 2] {
        let d = Box::pin(Deployment::new(vec![])).await;
        Box::pin(bind(&d, example(index))).await;
        let mut incoming = customer_email(&d, "silent", None);
        incoming.text = Some("ignore".into());
        d.email(incoming).await;
        let delivery = Delivery::start(&d, ProviderCase::Accept);
        Box::pin(d.run_until("suppressed completion", async || {
            let rows = finals(&d).await;
            rows.len() == 1
                && rows[0].state == "suppressed"
                && delivered(&d, if index == 1 { 0 } else { 1 }).await
        }))
        .await;
        let provider = delivery.stop().await;
        let rows = finals(&d).await;
        assert!(rows[0].message.is_none() && rows[0].delivery.is_none());
        assert_eq!(
            rows[0].output,
            json!({"body":if index==1 {""} else {"Completed"}})
        );
        let sent = provider.messages();
        assert_eq!(sent.len(), if index == 1 { 0 } else { 1 });
        if index == 2 {
            assert_eq!(sent[0].email.body_text, "Sent explicitly");
        }
        assert_eq!(d.llm.finish().await.unwrap(), 0);
    }
}

#[tokio::test]
async fn channel_reply_e2e_provider_retry_keeps_execution_and_wire_identity() {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"One model effect"}"#,
    )]))
    .await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    d.email(customer_email(&d, "retry", None)).await;
    let delivery = Delivery::start(&d, ProviderCase::Retry);
    Box::pin(
        d.run_until("retry settled without rerunning business", async || {
            responses_published(&d, 1).await && delivered(&d, 1).await
        }),
    )
    .await;
    let provider = delivery.stop().await;
    let sent = provider.messages();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].delivery, sent[1].delivery);
    assert_eq!(sent[0].email.message_id, sent[1].email.message_id);
    assert_eq!(sent[0].email.body_text, sent[1].email.body_text);
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_executions WHERE company_id=$1 AND step_id='start'")
            .await,
        1
    );
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn channel_reply_e2e_provider_unknown_survives_worker_reconstruction_without_resend() {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"Saved before transport"}"#,
    )]))
    .await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    d.email(customer_email(&d, "unknown", None)).await;
    let delivery = Delivery::start(&d, ProviderCase::Unknown);
    Box::pin(d.run_until("provider uncertainty persisted",async || {
        responses_published(&d,1).await && d.count("SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND status='outcome_unknown'").await==1
    })).await;
    let first = delivery.stop().await;
    let identities = finals(&d).await;
    let reconstructed = Delivery::start(&d, ProviderCase::Accept);
    // This is a new live worker, with a real queue poll rather than a sequential callback replay.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let next = reconstructed.stop().await;
    assert_eq!(first.sent.lock().unwrap().len(), 1);
    assert!(next.sent.lock().unwrap().is_empty());
    let after = finals(&d).await;
    assert_eq!(after[0].message, identities[0].message);
    assert_eq!(after[0].delivery, identities[0].delivery);
    assert_eq!(after[0].output, json!({"body":"Saved before transport"}));
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}
