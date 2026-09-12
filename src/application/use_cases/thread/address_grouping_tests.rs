//! Address boundaries survive routing, passive copies, and deduplication.
use super::*;

async fn fixture(slugs: &[&str]) -> ChannelFixture {
    let mut fixture = channel_fixture(TestChannel::default());
    let template = fixture
        .use_cases
        .channel_persistence
        .get_by_id(fixture.channel_id)
        .await
        .unwrap()
        .unwrap();
    let channels = slugs
        .iter()
        .map(|slug| Channel {
            id: Uuid::new_v4(),
            slug: (*slug).into(),
            name: (*slug).into(),
            ..template.clone()
        })
        .collect();
    fixture.use_cases = ThreadUseCases::for_test(
        fixture.threads.clone(),
        Arc::new(MockChannelPersistence {
            channels: Mutex::new(channels),
        }),
        fixture.use_cases.company_persistence.clone(),
        fixture.participants.clone(),
        fixture.tasks.clone(),
        internal_test_config(),
    );
    fixture
}

async fn ingest(
    fixture: &ChannelFixture,
    to: &str,
    cc: Option<&str>,
    body: &str,
) -> InboundIngestResult {
    let result = fixture
        .use_cases
        .ingest_test_email(RawInboundPayload {
            to: to.into(),
            cc: cc.map(str::to_owned),
            from: "team@acme.com".into(),
            subject: Some("Upgrade and invoice".into()),
            text: Some(body.into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(result.accepted, "{:?}", result.reason());
    result
}

fn task_targets(fixture: &ChannelFixture, result: &InboundIngestResult) -> Vec<Vec<String>> {
    let targets = fixture.tasks.task_targets.lock().unwrap();
    result
        .task_ids
        .iter()
        .map(|id| {
            targets[id]
                .iter()
                .map(|target| {
                    result
                        .channel_matches
                        .iter()
                        .find(|matched| matched.channel.id == target.channel_id)
                        .unwrap()
                        .channel
                        .slug
                        .to_string()
                })
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn one_plus_address_remains_one_ordered_task() {
    let fixture = fixture(&["support", "sales"]).await;
    let result = ingest(
        &fixture,
        "support+sales@acme.mailagents.com",
        None,
        "Please help.",
    )
    .await;
    assert_eq!(
        task_targets(&fixture, &result),
        vec![vec!["support", "sales"]]
    );
}

#[tokio::test]
async fn an_activated_cc_address_gets_its_own_task_after_the_to_pipeline() {
    let fixture = fixture(&["support", "sales", "billing"]).await;
    let result = ingest(
        &fixture,
        "support+sales@acme.mailagents.com",
        Some("billing@acme.mailagents.com"),
        "@billing, please resend the invoice.",
    )
    .await;
    assert_eq!(
        task_targets(&fixture, &result),
        vec![vec!["support", "sales"], vec!["billing"]]
    );
    assert_eq!(fixture.tasks.tasks.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_passive_cc_address_is_filed_without_a_task() {
    let fixture = fixture(&["support", "sales", "billing"]).await;
    let result = ingest(
        &fixture,
        "support+sales@acme.mailagents.com",
        Some("billing@acme.mailagents.com"),
        "Please help with an upgrade.",
    )
    .await;
    assert_eq!(
        task_targets(&fixture, &result),
        vec![vec!["support", "sales"]]
    );
    let billing = result
        .channel_matches
        .iter()
        .find(|matched| matched.channel.slug == "billing")
        .unwrap();
    assert_eq!(
        billing.inbound_message.canonical_id,
        result.inbound_message.unwrap().canonical_id
    );
    assert_eq!(fixture.threads.threads().len(), 3);
}

#[tokio::test]
async fn separate_to_addresses_have_independent_ordered_tasks() {
    let fixture = fixture(&["target1", "target3", "target4"]).await;
    let result = ingest(
        &fixture,
        "target1@acme.mailagents.com, target3+target4@acme.mailagents.com",
        None,
        "Please help.",
    )
    .await;
    assert_eq!(
        task_targets(&fixture, &result),
        vec![vec!["target1"], vec!["target3", "target4"]]
    );
}

#[tokio::test]
async fn a_repeated_cc_channel_stays_in_its_first_address() {
    let fixture = fixture(&["support", "sales"]).await;
    let result = ingest(
        &fixture,
        "support+sales@acme.mailagents.com",
        Some("sales@acme.mailagents.com"),
        "@sales, please help.",
    )
    .await;
    assert_eq!(
        task_targets(&fixture, &result),
        vec![vec!["support", "sales"]]
    );
    assert_eq!(result.channel_matches.len(), 2);
}

#[tokio::test]
async fn a_quiet_cc_address_suppresses_every_address_task() {
    let fixture = fixture(&["support", "sales", "billing"]).await;
    let result = ingest(
        &fixture,
        "support+sales@acme.mailagents.com",
        Some("billing.quiet@acme.mailagents.com"),
        "@billing, please resend the invoice.",
    )
    .await;
    assert!(result.task_ids.is_empty());
    assert!(fixture.tasks.tasks.lock().unwrap().is_empty());
    assert_eq!(result.channel_matches.len(), 3);
}
