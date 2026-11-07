use super::*;

struct Workers {
    shutdown: CancellationToken,
    handles: Vec<(&'static str, tokio::task::JoinHandle<()>)>,
}

impl Workers {
    async fn without_publisher(d: &Deployment) -> Self {
        let shutdown = CancellationToken::new();
        let mut handles = d.engine.spawn(&shutdown);
        let index = handles
            .iter()
            .position(|(name, _)| *name == "workflow final reply publisher")
            .unwrap();
        let (_, publisher) = handles.remove(index);
        publisher.abort();
        assert!(publisher.await.unwrap_err().is_cancelled());
        Self { shutdown, handles }
    }

    async fn stop(mut self) {
        self.shutdown.cancel();
        for (name, handle) in self.handles.drain(..) {
            tokio::time::timeout(Duration::from_secs(10), handle)
                .await
                .unwrap_or_else(|_| panic!("{name} did not stop"))
                .unwrap();
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.shutdown.cancel();
        for (_, handle) in &self.handles {
            handle.abort();
        }
    }
}

pub(super) fn reconstruct(d: &mut Deployment) {
    let config = AppConfig {
        app_domain_name: APP_DOMAIN.into(),
        ..AppConfig::for_test()
    };
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(Arc::new(EmailRenderer::new(APP_DOMAIN)))
            .unwrap(),
    );
    let composition = composition::engine(d.p(), &config, renderers);
    d.engine = composition.engine;
    d.knowledge = composition.knowledge;
}

#[tokio::test]
async fn channel_reply_e2e_committed_intent_survives_reconstruction_without_its_wakeup() {
    let mut d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"Durable business result"}"#,
    )]))
    .await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    let workers = Workers::without_publisher(&d).await;
    let incoming = d.email(customer_email(&d, "reconstruct", None)).await;
    let settled = tokio::time::timeout(Duration::from_secs(30), async {
        while finals(&d).await.is_empty() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    workers.stop().await;
    settled.expect("real workflow workers commit a final decision without a publisher");
    let saved = finals(&d).await;
    assert_eq!(saved[0].state, "pending");
    assert!(saved[0].message.is_none() && saved[0].delivery.is_none());
    assert_eq!(
        saved[0].source,
        incoming.inbound_message.unwrap().canonical_id.as_uuid()
    );
    // A new engine owns new wakeups. Only the durable row can lead it to this work.
    reconstruct(&mut d);
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(d.run_until(
        "reconstructed publisher polls the committed final intent",
        async || responses_published(&d, 1).await && delivered(&d, 1).await,
    ))
    .await;
    let provider = delivery.stop().await;
    let published = finals(&d).await;
    assert_eq!(published[0].run, saved[0].run);
    assert_eq!(published[0].output, saved[0].output);
    assert_eq!(provider.messages().len(), 1);
    assert_eq!(
        provider.messages()[0].email.body_text,
        "Durable business result"
    );
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn channel_reply_e2e_remote_acceptance_before_receipt_keeps_unknown_after_shutdown() {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"Accepted remotely once"}"#,
    )]))
    .await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    d.email(customer_email(&d, "lost-provider-receipt", None))
        .await;
    let delivery = Delivery::start(&d, ProviderCase::LostReceipt);
    Box::pin(d.run_until(
        "remote side counted acceptance, before its receipt",
        async || responses_published(&d, 1).await && delivery.provider.messages().len() == 1,
    ))
    .await;
    let provider = delivery.stop().await;
    assert_eq!(d.count("SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND status='outcome_unknown'").await,1);
    let saved = finals(&d).await;
    let reconstructed = Delivery::start(&d, ProviderCase::Accept);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let next = reconstructed.stop().await;
    assert!(next.messages().is_empty());
    assert_eq!(provider.messages().len(), 1);
    let after = finals(&d).await;
    assert_eq!(after[0].message, saved[0].message);
    assert_eq!(after[0].delivery, saved[0].delivery);
    assert_eq!(after[0].output, saved[0].output);
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}
