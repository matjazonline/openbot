//! Real ingress, compiler, worker, model HTTP endpoint, human command and delivery queue.
use super::*;
use crate::adapters::protocols::email::{OUTBOUND_EMAIL_VERSION, OutboundEmailV1};
use crate::application::services::delivery_worker::DeliveryWorker;
use crate::application::transport::{
    DeliveryRecord, ProviderSendOutcome, RenderedPart, TransportRegistry, TransportSender,
};
use crate::application::workflow::{decisions::*, metrics::WorkflowMetricPersistence};
use crate::domain::entities::transport::{DeliveryPurpose, TransportKind};
use std::sync::Mutex;

fn reviewed_source(reviewer: Uuid) -> Value {
    let mut source: Value = serde_yaml::from_str(include_str!(
        "../../../application/workflow/fixtures/reviewed-support.yaml"
    ))
    .unwrap();
    source["parameter_schema"] = json!(true);
    source["entry"] = json!("classify");
    source["steps"]["classify"] = json!({"type":"ai.classify","with":{
        "agent":{"literal":"assistant"},"context":{"object":{"untrusted_message":{"ref":"/input/message"}}},
        "output_schema":{"literal":{"type":"string","enum":["billing","sales"]}}},"routes":{"success":"history"}});
    source["steps"]["history"] = json!({"type":"context.history","with":{"max_tokens":{"literal":1000}},"routes":{"success":"draft"}});
    source["steps"]["draft"]["with"]["context"] = json!({"object":{
        "untrusted_message":{"ref":"/input/message"},"classification":{"ref":"/steps/classify/output"},
        "history":{"ref":"/steps/history/output"}}});
    source["steps"]["review"]["with"]["reviewers"] =
        json!({"literal":[{"kind":"user","id":reviewer}]});
    source["steps"]["review"]["with"]["proposal"] = json!({"object":{
        "draft":{"ref":"/steps/draft/output"},"classification":{"ref":"/steps/classify/output"},
        "history":{"ref":"/steps/history/output"}}});
    source
}

async fn decision(d: &Deployment) -> DecisionView {
    let id = sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id = $1")
        .bind(d.fixture.target.company.as_uuid())
        .fetch_one(d.p().pool())
        .await
        .unwrap();
    d.p()
        .read_decision(
            d.fixture.target.company,
            d.fixture.target.actor,
            DecisionId(id),
        )
        .await
        .unwrap()
}

async fn review(d: &Deployment, source_message: Uuid) {
    let before = decision(d).await;
    assert_eq!(before.proposal["classification"], "billing");
    assert_eq!(before.proposal["draft"]["body"], "Draft response");
    assert!(
        before.proposal["history"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.to_string().contains("Question reviewed"))
    );
    assert_eq!(before.context.trigger_message, Some(source_message));
    assert_eq!(before.context.channel, Some(d.channel));
    d.p()
        .add_decision_comment(AddDecisionComment {
            company: d.fixture.target.company,
            actor: d.fixture.target.actor,
            decision: before.decision,
            command_key: DecisionCommandKey::parse("discussion").unwrap(),
            body: "Use the approved wording".into(),
        })
        .await
        .unwrap();
    let after = decision(d).await;
    assert_eq!(after.state, "waiting");
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.comments.len(), 1);
    assert!(replies(d).await.is_empty());
    let command = SubmitDecision {
        company: d.fixture.target.company,
        actor: d.fixture.target.actor,
        decision: before.decision,
        command_key: DecisionCommandKey::parse("accept-exact").unwrap(),
        expected_revision: before.revision,
        expected_artifact: before.artifact_digest,
        choice: "approve".into(),
        feedback: "Wording checked".into(),
        data: json!({"body":"Exact accepted response"}),
    };
    let mut stale = command.clone();
    stale.expected_artifact = "0".repeat(64);
    assert!(d.p().submit_decision(stale).await.is_err());
    assert_eq!(
        d.p()
            .submit_decision(command.clone())
            .await
            .unwrap()
            .disposition,
        DecisionDisposition::Applied
    );
    assert_eq!(
        d.p().submit_decision(command).await.unwrap().disposition,
        DecisionDisposition::Replayed
    );
}

#[derive(Default)]
struct AcceptingProvider(Mutex<Vec<OutboundEmailV1>>);
#[async_trait]
impl TransportSender for AcceptingProvider {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }
    async fn send(&self, delivery: &DeliveryRecord, part: &RenderedPart) -> ProviderSendOutcome {
        if delivery.purpose == DeliveryPurpose::WorkflowReply {
            let email = part
                .payload
                .decode(TransportKind::Email, OUTBOUND_EMAIL_VERSION)
                .unwrap();
            self.0.lock().unwrap().push(email);
        }
        ProviderSendOutcome::Delivered { provider_key: None }
    }
}

async fn deliver_reviewed(d: &Deployment) {
    let provider = Arc::new(AcceptingProvider::default());
    let transports = TransportRegistry::new()
        .register(Arc::new(EmailRenderer::new(APP_DOMAIN)), provider.clone())
        .unwrap();
    let worker = Arc::new(DeliveryWorker::new(
        Arc::new(d.p().clone()),
        Arc::new(transports),
    ));
    let (shutdown, receiver) = tokio::sync::broadcast::channel(1);
    let handle = tokio::spawn(worker.run(receiver));
    // Abort on unwind as well; a failed assertion must not leave a queue claimant behind.
    let guard = handle.abort_handle();
    struct Stop(tokio::task::AbortHandle);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _guard = Stop(guard);
    d.run_until("reviewed reply committed", async || {
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id = $1 AND state = 'succeeded'")
            .await
            == 1
    })
    .await;
    shutdown.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .unwrap()
        .unwrap();
    let sent = provider.0.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].body_text, "Exact accepted response");
    assert_eq!(
        sent[0].in_reply_to.as_deref(),
        Some("<reviewed@partner.test>")
    );
    assert_eq!(
        sent[0].recipients_to,
        vec![crate::entities::value_objects::EmailAddress::from(CUSTOMER)]
    );
}

#[tokio::test]
async fn workflow_cutover_reviewed_email_delivers_exact_accepted_artifact() {
    let d = Box::pin(Deployment::new(vec![
        LlmTurn::text("\"billing\""),
        LlmTurn::text(r#"{"body":"Draft response"}"#),
    ]))
    .await;
    let empty = d.p().workflow_metrics().await.unwrap();
    assert_eq!(empty.active_runs, 0.0);
    assert_eq!(empty.open_decisions, 0.0);
    Box::pin(d.bind(
        reviewed_source(d.fixture.target.actor.user_id()),
        Some(MessageTrigger::Direct),
    ))
    .await;
    let inbound = Box::pin(d.email(customer_email(&d, "reviewed", None))).await;
    Box::pin(d.run_until("human review", async || {
        d.count("SELECT count(*) FROM workflow_decisions WHERE company_id = $1")
            .await
            == 1
    }))
    .await;
    let metrics = d.p().workflow_metrics().await.unwrap();
    assert_eq!(metrics.open_decisions, 1.0);
    assert_eq!(metrics.waiting_runs, 1.0);
    assert!(metrics.oldest_decision_seconds >= 0.0);
    assert!(metrics.charged_tokens_last24h > 0.0);
    assert_eq!(metrics.reserved_tokens, 0.0);
    assert_eq!(metrics.unpriced_model_calls, 2.0);
    Box::pin(review(
        &d,
        inbound
            .inbound_message
            .as_ref()
            .unwrap()
            .canonical_id
            .as_uuid(),
    ))
    .await;
    Box::pin(deliver_reviewed(&d)).await;
    let closed = decision(&d).await;
    assert_eq!(closed.comments.len(), 1);
    assert_eq!(closed.proposal["draft"]["body"], "Draft response");
    assert_eq!(
        closed.submission.unwrap().data,
        json!({"body":"Exact accepted response"})
    );
    let metrics = d.p().workflow_metrics().await.unwrap();
    assert_eq!(metrics.open_decisions, 0.0);
    assert_eq!(metrics.waiting_runs, 0.0);
    assert!(metrics.delivered_last24h >= 1.0);
    assert_eq!(metrics.completed_steps_last24h, 5.0);
    Box::pin(check_current_price_estimate(&d)).await;
    assert_eq!(d.llm.finish().await.unwrap(), 2);
}

async fn check_current_price_estimate(d: &Deployment) {
    use crate::application::services::model_usage::{
        ModelUsageStore, PriceRates, PriceWrite, TokenPrice, UsageScope,
    };
    use crate::entities::value_objects::{ModelName, ModelProvider};
    d.p()
        .save_model_price(PriceWrite {
            scope: UsageScope {
                company: d.fixture.target.company.as_uuid(),
                actor: d.fixture.target.actor.user_id(),
            },
            provider: ModelProvider::canonical(SCRIPTED_PROVIDER),
            model: ModelName::canonical(SCRIPTED_MODEL),
            expected_revision: None,
            rates: Some(PriceRates {
                input: TokenPrice::parse("1").unwrap(),
                output: TokenPrice::parse("1").unwrap(),
            }),
        })
        .await
        .unwrap();
    let metrics = d.p().workflow_metrics().await.unwrap();
    assert_eq!(metrics.unpriced_model_calls, 0.0);
    assert!(
        (metrics.charged_estimated_usd_last24h - metrics.charged_tokens_last24h / 1_000_000.0)
            .abs()
            < 1e-12
    );
}
