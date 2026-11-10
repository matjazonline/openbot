//! `memory_observation_` use-case checks (04A): every case runs `prepare` and `generate` with the
//! counted provider and inspects the requests it actually received. No network, no paid calls.
use super::*;
use crate::{
    application::{
        contract::{ActivityEvent, GenerationRequest},
        ports::{ProviderResponse, StopReason},
        test_provider::{CountedProvider, answer},
    },
    domain::{
        access_level::Consumed,
        grant::{NamespaceGrant, Operation},
    },
};
use chrono::TimeZone;
use serde_json::Value;

fn level(value: i64) -> AccessLevel {
    AccessLevel::new(value).unwrap()
}

fn client(write_max: i64) -> ClientPrincipal {
    ClientPrincipal {
        credential_id: Uuid::now_v7(),
        tenant_id: Uuid::now_v7(),
        tenant: "t".into(),
        label: "capture".into(),
        expires_at: None,
        grants: vec![NamespaceGrant {
            namespace_id: Uuid::now_v7(),
            namespace: "ns".into(),
            read_max: Some(level(9)),
            write_max: Some(level(write_max)),
            operations: [Operation::Generate, Operation::Save].into_iter().collect(),
        }],
    }
}

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 8, 10, minute, 0).unwrap()
}

fn event(id: &str, minute: u32, access_level: i64, text: &str) -> ActivityEvent {
    ActivityEvent {
        event_id: id.into(),
        source_ref: format!("chat:{id}"),
        revision: "1".into(),
        occurred_at: at(minute),
        access_level: level(access_level),
        role: EventRole::User,
        text: text.into(),
        exclude: false,
    }
}

fn request(
    ceiling: Option<i64>,
    consumed: Consumed,
    events: Vec<ActivityEvent>,
) -> GenerationRequest {
    GenerationRequest {
        idempotency_key: "gen-1".into(),
        expires_at: now() + Duration::hours(1),
        namespace: "ns".into(),
        session_id: None,
        producing_ceiling: ceiling.map(level),
        consumed,
        description: None,
        events,
        allowance_tokens: 100_000,
        max_output_tokens: 2_048,
        summary: false,
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 8, 12, 0, 0).unwrap()
}

fn exact(value: i64) -> Consumed {
    Consumed::Exact {
        level: level(value),
    }
}

fn reason(error: &ServiceError) -> Value {
    error.details.clone().unwrap_or_default()["reason"].clone()
}

const USEFUL: &str = r#"{"outcome":"save","observations":[
    {"kind":"decision","title":"PostgreSQL chosen","body":"The team chose PostgreSQL.","event_ids":["e2","e1"]},
    {"kind":"fix","title":"Login fixed","body":"The login timeout was raised.","event_ids":["e3"]}],"summary":null}"#;
const NOTHING: &str = r#"{"outcome":"nothing_to_save","observations":[],"summary":null}"#;

fn window() -> Vec<ActivityEvent> {
    vec![
        event("e1", 0, 2, "Should we use PostgreSQL?"),
        event("e2", 1, 4, "Yes, PostgreSQL. password=hunter2"),
        event(
            "e3",
            2,
            0,
            "Raised the login timeout. <private>my home address</private>",
        ),
    ]
}

/// A useful window: two typed observations at the shared floor, provenance in window order,
/// one provider call whose request holds the redacted window as data and nothing else.
#[tokio::test]
async fn memory_observation_useful_window_yields_typed_labelled_observations() {
    let prepared = prepare(&client(9), &request(Some(6), exact(4), window()), now()).unwrap();
    assert_eq!(
        prepared.label.level(),
        level(4),
        "ceiling 6, consumed exactly 4, events ≤ 4"
    );
    assert_eq!(prepared.redactions, 2);
    let provider = CountedProvider::new(vec![answer(USEFUL)]);
    let generated = generate(&provider, &prepared).await.unwrap();
    let GenerationOutcome::Saved(items) = generated.outcome else {
        panic!("{:?}", generated.outcome)
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, ObservationKind::Decision);
    assert_eq!(
        items[0].sources,
        vec![
            ("chat:e1".into(), "1".into()),
            ("chat:e2".into(), "1".into())
        ]
    );
    assert!(items.iter().all(|item| item.access_level == level(4)));
    assert_eq!(generated.usage.total(), 1_200);
    // The request actually sent.
    let sent = provider.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].schema, provider_schema());
    assert_eq!(sent[0].max_output_tokens, 2_048);
    assert!(sent[0].system.contains("untrusted data"));
    let user: Value = serde_json::from_str(&sent[0].user).unwrap();
    assert_eq!(user["summary_requested"], false);
    let events = user["events"].as_array().unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(
        events[0].as_object().unwrap().keys().collect::<Vec<_>>(),
        ["event_id", "occurred_at", "role", "text"]
    );
    assert!(!sent[0].user.contains("hunter2") && !sent[0].user.contains("home address"));
    for absent in ["access_level", "namespace", "\"ns\"", "chat:e1", "gen-1"] {
        assert!(!sent[0].user.contains(absent), "{absent} was sent");
    }
}

/// A trivial window is a valid skip; empty, malformed, truncated and refused output are
/// distinct failures; provider errors keep their retry semantics.
#[tokio::test]
async fn memory_observation_outcomes_distinguish_skip_empty_malformed_and_provider_errors() {
    let prepared = prepare(
        &client(9),
        &request(Some(6), exact(0), vec![event("e1", 0, 0, "ok thanks")]),
        now(),
    )
    .unwrap();
    let truncated = Ok(ProviderResponse {
        stop: StopReason::MaxTokens,
        text: NOTHING.into(),
        usage: Default::default(),
    });
    let refusal = Ok(ProviderResponse {
        stop: StopReason::Refusal,
        text: String::new(),
        usage: Default::default(),
    });
    let lower_label = r#"{"outcome":"save","observations":[{"kind":"fact","title":"t","body":"b","event_ids":["e1"],"access_level":0}],"summary":null}"#;
    let cases = [
        (
            answer(NOTHING),
            GenerationOutcome::Skipped(SkipReason::NothingToSave),
        ),
        (answer(""), GenerationOutcome::Failed("empty_output".into())),
        (
            answer("{\"outcome\":"),
            GenerationOutcome::Failed("malformed_output".into()),
        ),
        (
            answer(lower_label),
            GenerationOutcome::Failed("malformed_output".into()),
        ),
        (
            truncated,
            GenerationOutcome::Failed("output_truncated".into()),
        ),
        (
            refusal,
            GenerationOutcome::Failed("provider_refused".into()),
        ),
        (
            Err(ProviderError::NotSent),
            GenerationOutcome::Retryable("not_sent".into()),
        ),
        (
            Err(ProviderError::Rejected("overloaded_error".into())),
            GenerationOutcome::Retryable("overloaded_error".into()),
        ),
        (
            Err(ProviderError::Terminal("authentication_error".into())),
            GenerationOutcome::Failed("authentication_error".into()),
        ),
        (Err(ProviderError::Ambiguous), GenerationOutcome::Unknown),
    ];
    for (response, expected) in cases {
        let provider = CountedProvider::new(vec![response]);
        let generated = generate(&provider, &prepared).await.unwrap();
        assert_eq!(generated.outcome, expected);
        assert_eq!(provider.calls(), 1, "exactly one call, never a retry here");
    }
}

/// Privacy-only input reaches neither storage nor the provider; privacy runs again at the
/// generation boundary on whatever is about to be sent.
#[tokio::test]
async fn memory_observation_privacy_only_input_never_reaches_the_provider() {
    let mut excluded = event("e1", 0, 3, "internal");
    excluded.exclude = true;
    let private = event("e2", 1, 3, "<private>everything here</private>   ");
    let prepared = prepare(
        &client(9),
        &request(Some(6), exact(0), vec![excluded, private]),
        now(),
    )
    .unwrap();
    assert_eq!((prepared.events.len(), prepared.dropped), (0, 2));
    let provider = CountedProvider::new(vec![]);
    let generated = generate(&provider, &prepared).await.unwrap();
    assert_eq!(
        generated.outcome,
        GenerationOutcome::Skipped(SkipReason::NothingToProcess)
    );
    assert_eq!(provider.calls(), 0);
    // Text that reaches the boundary unfiltered (as if loaded from storage) is redacted again.
    let mut tampered = prepare(
        &client(9),
        &request(Some(6), exact(0), vec![event("e1", 0, 0, "plain")]),
        now(),
    )
    .unwrap();
    tampered.events[0].text = "token=abc123secret and sk-ant-api03-ZZZZZZZZZZZZZZZZZZZZ".into();
    tampered.description = Some("<private>client note</private> describe".into());
    let provider = CountedProvider::new(vec![answer(NOTHING)]);
    generate(&provider, &tampered).await.unwrap();
    let sent = &provider.requests()[0].user;
    assert!(
        !sent.contains("abc123secret") && !sent.contains("sk-ant") && !sent.contains("client note"),
        "{sent}"
    );
}

/// Labels come from trusted input: the ceiling without precise evidence, an incoming level-6
/// answer summarized from public text, and no lower label from the model.
#[tokio::test]
async fn memory_observation_labels_inherit_trusted_classification_only() {
    let label = |ceiling, consumed, events| {
        prepare(&client(9), &request(ceiling, consumed, events), now()).map(|p| p.label.level())
    };
    // Ceiling 6, consumed exactly 4 → 4.
    assert_eq!(label(Some(6), exact(4), window()).unwrap(), level(4));
    // Missing precise evidence → the producing ceiling, 6, even for public-looking text.
    assert_eq!(
        label(
            Some(6),
            Consumed::Unknown,
            vec![event("e1", 0, 0, "public")]
        )
        .unwrap(),
        level(6)
    );
    // An incoming level-6 answer among public text: the whole generation context is 6.
    let answer6 = vec![
        event("e1", 0, 0, "What is the plan?"),
        event("e2", 1, 6, "The plan is public now."),
        event("e3", 2, 0, "Thanks"),
    ];
    let prepared = prepare(&client(9), &request(Some(6), exact(0), answer6), now()).unwrap();
    assert_eq!(prepared.label.level(), level(6));
    let short = r#"{"outcome":"save","observations":[{"kind":"outcome","title":"Plan","body":"Public plan.","event_ids":["e1"]}],"summary":null}"#;
    let generated = generate(&CountedProvider::new(vec![answer(short)]), &prepared)
        .await
        .unwrap();
    let GenerationOutcome::Saved(items) = generated.outcome else {
        panic!()
    };
    assert_eq!(
        items[0].access_level,
        level(6),
        "a short public-looking answer is not declassified"
    );
    // An excluded event still counts as known context for the refusal, but not for the label.
    let mut hidden = event("e2", 1, 5, "never sent");
    hidden.exclude = true;
    assert_eq!(
        label(Some(6), exact(1), vec![event("e1", 0, 1, "x"), hidden]).unwrap(),
        level(1)
    );
    // Refusals: missing ceiling, a known source above it (excluded or not), consumed above it,
    // and a label above the write grant.
    let missing = label(None, exact(0), window()).unwrap_err();
    assert_eq!(reason(&missing), "missing_trusted_ceiling");
    let mut above = event("e9", 9, 7, "secret");
    let higher = label(Some(6), exact(0), vec![above.clone()]).unwrap_err();
    assert_eq!(
        (higher.code, reason(&higher)),
        (
            ErrorCode::ClassificationRefused,
            json!("source_above_ceiling")
        )
    );
    above.exclude = true;
    assert_eq!(
        reason(&label(Some(6), exact(0), vec![event("e1", 0, 0, "x"), above]).unwrap_err()),
        "source_above_ceiling"
    );
    assert_eq!(
        reason(&label(Some(6), exact(7), window()).unwrap_err()),
        "consumed_exceeds_ceiling"
    );
    let narrow = prepare(&client(3), &request(Some(6), exact(4), window()), now()).unwrap_err();
    assert_eq!(narrow.code, ErrorCode::Forbidden);
    // A destination not authorized for the level is refused before any call.
    let prepared = prepare(&client(9), &request(Some(6), exact(4), window()), now()).unwrap();
    let provider = CountedProvider::new(vec![]).with_max_level(level(3));
    let refused = generate(&provider, &prepared).await.unwrap_err();
    assert_eq!((refused.code, provider.calls()), (ErrorCode::Forbidden, 0));
}

/// Source ordering and revision identity, context exhaustion and request expiry are refused
/// before any provider call; a session summary stays inside its session's window.
#[tokio::test]
async fn memory_observation_windows_are_ordered_bounded_and_summaries_scoped() {
    let refuse = |request: GenerationRequest| prepare(&client(9), &request, now()).unwrap_err();
    let out_of_order = vec![event("e1", 5, 0, "later"), event("e2", 1, 0, "earlier")];
    assert_eq!(
        reason(&refuse(request(Some(6), exact(0), out_of_order))),
        "events_out_of_order"
    );
    // Identifiers are sent or stored as given: a secret-shaped one is refused.
    let mut keyed = event("e1", 0, 0, "a");
    keyed.event_id = "sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUV".into();
    assert_eq!(
        reason(&refuse(request(Some(6), exact(0), vec![keyed]))),
        "secret_in_identifier"
    );
    let mut url = event("e1", 0, 0, "a");
    url.source_ref = "https://docs.test/x?token=abc123".into();
    assert_eq!(
        reason(&refuse(request(Some(6), exact(0), vec![url]))),
        "secret_in_identifier"
    );
    let mut revision = event("e1", 0, 0, "a");
    revision.revision = "token=abc123".into();
    assert_eq!(
        reason(&refuse(request(Some(6), exact(0), vec![revision]))),
        "secret_in_identifier"
    );
    let duplicate = vec![event("e1", 0, 0, "a"), event("e1", 1, 0, "b")];
    assert_eq!(
        reason(&refuse(request(Some(6), exact(0), duplicate))),
        "duplicate_event"
    );
    // One source revision with two different texts or levels is a conflict.
    let mut first = event("e1", 0, 0, "v1 text");
    let mut second = event("e2", 1, 0, "other text");
    first.source_ref = "doc:1".into();
    second.source_ref = "doc:1".into();
    assert_eq!(
        refuse(request(
            Some(6),
            exact(0),
            vec![first.clone(), second.clone()]
        ))
        .code,
        ErrorCode::Conflict
    );
    second.revision = "2".into();
    assert!(
        prepare(
            &client(9),
            &request(Some(6), exact(0), vec![first, second]),
            now()
        )
        .is_ok(),
        "revisions of one source"
    );
    // Context exhaustion.
    let many: Vec<_> = (0..513)
        .map(|n| event(&format!("e{n}"), 0, 0, "x"))
        .collect();
    assert_eq!(
        reason(&refuse(request(Some(6), exact(0), many))),
        "window_too_large"
    );
    let big: Vec<_> = (0..3)
        .map(|n| event(&format!("e{n}"), 0, 0, &"y".repeat(200 * 1024)))
        .collect();
    assert_eq!(
        reason(&refuse(request(Some(6), exact(0), big))),
        "window_too_large"
    );
    let mut poor = request(Some(6), exact(0), window());
    poor.allowance_tokens = 5_000;
    let poor = refuse(poor);
    assert_eq!(reason(&poor), "allowance_below_reservation");
    assert!(poor.details.unwrap()["reserve_tokens"].as_u64().unwrap() > 5_000);
    let mut expired = request(Some(6), exact(0), window());
    expired.expires_at = now();
    assert_eq!(refuse(expired).code, ErrorCode::RequestExpired);
    // A summary needs its session; it carries the window's floor and cites the whole window.
    let mut summary = request(Some(6), exact(4), window());
    summary.summary = true;
    assert_eq!(reason(&refuse(summary.clone())), "summary_without_session");
    summary.session_id = Some(Uuid::now_v7());
    let prepared = prepare(&client(9), &summary, now()).unwrap();
    let text = r#"{"outcome":"save","observations":[],"summary":"The team chose PostgreSQL and fixed login."}"#;
    let provider = CountedProvider::new(vec![answer(text), answer(USEFUL)]);
    let GenerationOutcome::Saved(items) = generate(&provider, &prepared).await.unwrap().outcome
    else {
        panic!()
    };
    assert_eq!(
        (items.len(), items[0].kind, items[0].access_level),
        (1, ObservationKind::Summary, level(4))
    );
    assert_eq!(items[0].sources.len(), 3);
    let sent: Value = serde_json::from_str(&provider.requests()[0].user).unwrap();
    assert_eq!(sent["summary_requested"], true);
    // A summary request answered without a summary is malformed, not a partial save.
    assert_eq!(
        generate(&provider, &prepared).await.unwrap().outcome,
        GenerationOutcome::Failed("malformed_output".into())
    );
}
