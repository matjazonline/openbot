use std::sync::Arc;

use super::*;
use crate::{
    adapters::resend_api::{
        client::{RawReference, ReceivedEmail},
        test_support::FakeResendApi,
    },
    entities::{
        auth::AuthVerdict,
        correlation::CorrelationId,
        transport::{ExternalEventKey, InboundEventId},
        value_objects::AuthservId,
    },
    transport::{
        InboundContentType, InboundEventPayload, InboundPayloadDigest, IngressPolicyFacts,
        SafeHeaderFacts,
    },
};

const AUTHSERV: &str = "resend.com";
const EVENT_KEY: &str = "56761188-7520-42d8-8898-ff6fc54ce618";

fn config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        app_domain_name: "localhost".to_string(),
        ..AppConfig::for_test()
    })
}

/// The company account the mail is read through: a scripted API and the `authserv-id` of the
/// Resend account that received it.
fn account(api: FakeResendApi) -> CompanyResendApiAccount {
    CompanyResendApiAccount {
        api: Arc::new(api),
        authserv_id: AuthservId::new(AUTHSERV),
    }
}

/// Read one mail through the phase under test.
///
/// [`read_mail`] is a free function precisely so this needs no thread store, no channel and no
/// tenant: everything below stops before the application is asked anything.
async fn read(
    api: FakeResendApi,
    config: &AppConfig,
    event: &InboundEventRecord,
) -> Result<(InboundMessage, PendingEmailAttachments), Failure> {
    super::read_mail(&account(api), config, event, EVENT_KEY).await
}

fn event(payload: serde_json::Value) -> InboundEventRecord {
    let payload =
        InboundEventPayload::parse(payload.to_string().into_bytes()).expect("a bounded payload");
    InboundEventRecord {
        id: InboundEventId::from(uuid::Uuid::new_v4()),
        company_id: uuid::Uuid::new_v4(),
        installation_id: None,
        transport: TransportKind::Email,
        external_event_key: ExternalEventKey::parse(EVENT_KEY).expect("a valid key"),
        correlation_id: CorrelationId::new(),
        payload_digest: InboundPayloadDigest::sha256(&payload),
        payload,
        content_type: InboundContentType::parse("application/json").ok(),
        safe_header_facts: SafeHeaderFacts::default(),
        attempt_count: 0,
        max_attempts: 5,
        received_at: chrono::Utc::now(),
    }
}

fn received_event() -> InboundEventRecord {
    event(serde_json::json!({
        "type": "email.received",
        "data": { "email_id": EVENT_KEY }
    }))
}

fn received_email() -> ReceivedEmail {
    ReceivedEmail {
        id: EVENT_KEY.to_string(),
        from: "sender@example.com".to_string(),
        to: vec!["list@example.com".to_string()],
        received_for: vec!["support@acme.localhost".to_string()],
        raw: Some(RawReference {
            download_url: "https://inbound-cdn.resend.com/signed".to_string(),
            expires_at: None,
        }),
    }
}

fn raw_mime(authentication_results: &str) -> Vec<u8> {
    format!(
        "{authentication_results}\
         Message-ID: <inbound-1@example.com>\r\n\
         From: sender@example.com\r\n\
         To: support@acme.localhost\r\n\
         Subject: a question\r\n\
         \r\n\
         Please help.\r\n"
    )
    .into_bytes()
}

const RESEND_API_PASS: &str =
    "Authentication-Results: resend.com; spf=pass; dkim=pass; dmarc=pass\r\n";

/// The whole provider exchange, scripted to succeed.
fn fetching(authentication_results: &str) -> FakeResendApi {
    FakeResendApi::new()
        .retrieving(Ok(received_email()))
        .with_raw(Ok(raw_mime(authentication_results)))
}

#[tokio::test]
async fn a_received_mail_carries_its_event_key_and_correlation_id_into_the_commit() {
    let event = received_event();
    let (inbound, _) = read(fetching(RESEND_API_PASS), &config(), &event)
        .await
        .expect("the mail reads");

    // Both are what the worker checks before it will commit: the event row and the message it
    // becomes are one piece of work, and a log line has to be able to join them.
    assert_eq!(
        inbound.draft.event_key.as_ref(),
        Some(&event.external_event_key)
    );
    assert_eq!(inbound.draft.correlation_id, event.correlation_id);
    assert!(matches!(inbound.origin, IngressOrigin::ExternalTransport));
}

#[tokio::test]
async fn the_verdicts_come_from_the_configured_authserv_id_and_nowhere_else() {
    let event = received_event();
    let (inbound, _) = read(fetching(RESEND_API_PASS), &config(), &event)
        .await
        .expect("the mail reads");

    let IngressPolicyFacts::Email(facts) = inbound.draft.policy else {
        panic!("an inbound mail carries email policy facts");
    };
    assert_eq!(facts.dmarc, AuthVerdict::Pass);
}

/// The case that decides whether this integration is safe. A sender composes a message already
/// carrying a header that claims everything passed; only the receiving MTA's own header, at the
/// top, may be believed -- and a message with no such header must reach `guard_ingress` with
/// nothing to authenticate it.
#[tokio::test]
async fn a_forged_or_absent_authentication_result_leaves_nothing_to_authenticate_the_mail() {
    for headers in [
        "",
        "Authentication-Results: mx.attacker.example; spf=pass; dkim=pass; dmarc=pass\r\n",
        "Authentication-Results: resend.com; dmarc=fail\r\n\
         Authentication-Results: resend.com; dmarc=pass\r\n",
    ] {
        let event = received_event();
        let (inbound, _) = read(fetching(headers), &config(), &event)
            .await
            .expect("the mail reads");
        let IngressPolicyFacts::Email(facts) = inbound.draft.policy else {
            panic!("an inbound mail carries email policy facts");
        };
        // `guard_ingress` refuses every external message whose DMARC verdict is not a pass, so
        // anything but `Pass` here is the message being turned away.
        assert_ne!(facts.dmarc, AuthVerdict::Pass, "for headers: {headers:?}");
    }
}

#[tokio::test]
async fn the_recipient_is_the_address_the_mail_was_received_for() {
    let mut email = received_email();
    // A mailing list rewrote `To:`; the routing fact is what Resend accepted the mail for.
    email.to = vec!["list@example.com".to_string()];
    email.received_for = vec!["support@acme.localhost".to_string()];
    let api = FakeResendApi::new()
        .retrieving(Ok(email))
        .with_raw(Ok(raw_mime(RESEND_API_PASS)));
    let event = received_event();

    let (inbound, _) = read(api, &config(), &event).await.expect("the mail reads");

    assert!(
        format!("{:?}", inbound.routing).contains("support"),
        "routing should name the received-for channel: {:?}",
        inbound.routing
    );
}

#[tokio::test]
async fn a_transient_provider_failure_asks_again_and_a_definite_one_does_not() {
    for (error, expect_retry) in [
        (
            ResendApiError::RateLimited {
                retry_after: None,
                detail: "slow down".to_string(),
            },
            true,
        ),
        (
            ResendApiError::Unavailable {
                detail: "connection reset".to_string(),
            },
            true,
        ),
        (
            ResendApiError::Refused {
                status: 404,
                detail: "no such email".to_string(),
            },
            false,
        ),
        (ResendApiError::TooLarge { limit: 16 }, false),
        (
            ResendApiError::Malformed {
                detail: "not json".to_string(),
            },
            false,
        ),
    ] {
        let event = received_event();
        let Err(failure) = read(
            FakeResendApi::new().retrieving(Err(error.clone())),
            &config(),
            &event,
        )
        .await
        else {
            panic!("a failed retrieve must not produce a mail");
        };
        assert_eq!(failure.retry, expect_retry, "for {error:?}");
    }
}

#[tokio::test]
async fn a_download_that_exceeds_the_message_bound_is_terminal_rather_than_retried() {
    let api = FakeResendApi::new()
        .retrieving(Ok(received_email()))
        .with_raw(Err(ResendApiError::TooLarge {
            limit: crate::adapters::protocols::email::parser::MAX_INBOUND_MESSAGE_BYTES,
        }));
    let event = received_event();

    let Err(failure) = read(api, &config(), &event).await else {
        panic!("an oversized mail must not read");
    };

    assert!(!failure.retry, "the same mail is the same size next time");
}

#[tokio::test]
async fn a_mail_resend_api_will_not_hand_over_the_bytes_of_is_terminal() {
    let mut email = received_email();
    email.raw = None;
    let event = received_event();

    let Err(failure) = read(
        FakeResendApi::new().retrieving(Ok(email)),
        &config(),
        &event,
    )
    .await
    else {
        panic!("a mail with no raw representation must not read");
    };

    assert!(!failure.retry);
}

/// The verdicts follow the *company's* account, not the deployment's. A mail carrying the header
/// of a different Resend account than the one that received it authenticates nothing.
#[tokio::test]
async fn only_the_receiving_companys_authserv_id_is_believed() {
    let elsewhere = CompanyResendApiAccount {
        api: Arc::new(fetching(RESEND_API_PASS)),
        authserv_id: AuthservId::new("mx.another-tenant.example"),
    };
    let event = received_event();

    let (inbound, _) = super::read_mail(&elsewhere, &config(), &event, EVENT_KEY)
        .await
        .expect("the mail reads");

    let IngressPolicyFacts::Email(facts) = inbound.draft.policy else {
        panic!("an inbound mail carries email policy facts");
    };
    assert_ne!(facts.dmarc, AuthVerdict::Pass);
}

#[test]
fn only_a_received_event_names_a_mail_to_fetch() {
    assert_eq!(
        event_email_id(&received_event()).expect("a readable envelope"),
        Some(EVENT_KEY.to_string())
    );
    for event_type in ["email.delivered", "email.bounced", "domain.created"] {
        let record = event(serde_json::json!({
            "type": event_type,
            "data": { "email_id": EVENT_KEY }
        }));
        assert_eq!(event_email_id(&record).expect("a readable envelope"), None);
    }
}

#[test]
fn a_payload_this_build_cannot_read_is_terminal_rather_than_retried_five_times() {
    let record = event(serde_json::json!({ "type": "email.received" }));
    let failure = event_email_id(&record).expect_err("an envelope with no data object");

    assert!(!failure.retry);
    assert_eq!(failure.class, InboundEventErrorClass::Decode);
}
