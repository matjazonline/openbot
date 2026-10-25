//! Every addressed channel gets its own admission intent carrying its address facts (p08 D1.3);
//! routing, passive copies and deduplication decide associations, never whether anything runs.
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

/// `(slug, role, mentioned, admissible)` per committed intent, in association order.
fn intents(
    fixture: &ChannelFixture,
    result: &InboundIngestResult,
) -> Vec<(String, &'static str, bool, bool)> {
    let request = fixture.threads.committed_requests().pop().unwrap();
    assert_eq!(result.intent_ids.len(), request.intents.len());
    request
        .intents
        .iter()
        .map(|intent| {
            let slug = result
                .channel_matches
                .iter()
                .find(|matched| matched.channel.id == intent.channel_id)
                .unwrap()
                .channel
                .slug
                .to_string();
            (
                slug,
                intent.role.as_str(),
                intent.mentioned,
                request.admissible,
            )
        })
        .collect()
}

#[tokio::test]
async fn a_plus_address_gives_each_channel_its_own_intent() {
    let fixture = fixture(&["support", "sales"]).await;
    let result = ingest(
        &fixture,
        "support+sales@acme.mailagents.com",
        None,
        "Please help.",
    )
    .await;
    assert_eq!(
        intents(&fixture, &result),
        vec![
            ("support".into(), "to", false, true),
            ("sales".into(), "to", false, true)
        ]
    );
}

#[tokio::test]
async fn a_cc_address_gets_an_intent_with_its_role_and_mention() {
    let fixture = fixture(&["support", "sales", "billing"]).await;
    let result = ingest(
        &fixture,
        "support+sales@acme.mailagents.com",
        Some("billing@acme.mailagents.com"),
        "@billing, please resend the invoice.",
    )
    .await;
    assert_eq!(
        intents(&fixture, &result),
        vec![
            ("support".into(), "to", false, true),
            ("sales".into(), "to", false, true),
            ("billing".into(), "cc", true, true)
        ]
    );
}

/// A Cc'd channel that is not mentioned is still a stored copy with an intent: whether it is
/// answered is its bindings' message trigger, not a channel setting (p08 F001).
#[tokio::test]
async fn an_unmentioned_cc_address_still_gets_an_intent() {
    let fixture = fixture(&["support", "sales", "billing"]).await;
    let result = ingest(
        &fixture,
        "support+sales@acme.mailagents.com",
        Some("billing@acme.mailagents.com"),
        "Please help with an upgrade.",
    )
    .await;
    assert_eq!(
        intents(&fixture, &result)[2],
        ("billing".into(), "cc", false, true)
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
async fn separate_to_addresses_each_get_an_intent() {
    let fixture = fixture(&["target1", "target3", "target4"]).await;
    let result = ingest(
        &fixture,
        "target1@acme.mailagents.com, target3+target4@acme.mailagents.com",
        None,
        "Please help.",
    )
    .await;
    let slugs: Vec<_> = intents(&fixture, &result)
        .into_iter()
        .map(|(slug, ..)| slug)
        .collect();
    assert_eq!(slugs, vec!["target1", "target3", "target4"]);
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
        intents(&fixture, &result),
        vec![
            ("support".into(), "to", false, true),
            ("sales".into(), "to", true, true)
        ]
    );
    assert_eq!(result.channel_matches.len(), 2);
}

/// A quiet address makes the whole message quiet: every intent is kept for reply correlation,
/// and none may admit a binding.
#[tokio::test]
async fn a_quiet_cc_address_makes_every_intent_inadmissible() {
    let fixture = fixture(&["support", "sales", "billing"]).await;
    let result = ingest(
        &fixture,
        "support+sales@acme.mailagents.com",
        Some("billing.quiet@acme.mailagents.com"),
        "@billing, please resend the invoice.",
    )
    .await;
    let committed = intents(&fixture, &result);
    assert_eq!(committed.len(), 3);
    assert!(committed.iter().all(|(.., admissible)| !admissible));
    assert_eq!(result.channel_matches.len(), 3);
}
