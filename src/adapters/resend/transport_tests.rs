use std::time::Duration;

use super::*;
use crate::{
    adapters::{
        protocols::email::{MailHeader, MailMessage},
        resend::{client::ResendSendResponse, test_support::FakeResendApi},
    },
    entities::value_objects::EmailAddress,
};

fn mail() -> MailMessage {
    MailMessage {
        from: EmailAddress::from("support@acme.localhost".to_string()),
        from_name: Some("Acme Support".to_string()),
        recipients_to: vec![EmailAddress::from("someone@example.com".to_string())],
        recipients_cc: vec![EmailAddress::from("watcher@example.com".to_string())],
        subject: "Re: an order".to_string(),
        body_text: "here you go".to_string(),
        message_id: Some(MessageId::from("<delivery-abc@localhost>".to_string())),
        in_reply_to: Some(MessageId::from("<theirs-1@example.com>".to_string())),
        references: vec![
            MessageId::from("<root@example.com>".to_string()),
            MessageId::from("<theirs-1@example.com>".to_string()),
        ],
        headers: vec![
            MailHeader {
                name: "Auto-Submitted".to_string(),
                value: "auto-replied".to_string(),
            },
            MailHeader {
                name: "X-MailAgents-Hop-Count".to_string(),
                value: "1".to_string(),
            },
        ],
    }
}

async fn send_with(api: FakeResendApi) -> (MailSendOutcome, Arc<FakeResendApi>) {
    let api = Arc::new(api);
    let outcome = ResendMailTransport::new(api.clone()).send(mail()).await;
    (outcome, api)
}

#[tokio::test]
async fn a_send_carries_the_frozen_threading_and_loop_headers() {
    let (outcome, api) = send_with(FakeResendApi::new().accepting("resend-id-1")).await;

    assert_eq!(outcome, MailSendOutcome::Accepted { provider_key: None });
    let request = api.only_send();
    assert_eq!(request.from, "Acme Support <support@acme.localhost>");
    assert_eq!(request.to, vec!["someone@example.com".to_string()]);
    assert_eq!(request.cc, vec!["watcher@example.com".to_string()]);
    assert_eq!(request.subject, "Re: an order");
    assert_eq!(request.text, "here you go");
    assert_eq!(
        request.headers.get("Message-ID").map(String::as_str),
        Some("<delivery-abc@localhost>")
    );
    assert_eq!(
        request.headers.get("In-Reply-To").map(String::as_str),
        Some("<theirs-1@example.com>")
    );
    assert_eq!(
        request.headers.get("References").map(String::as_str),
        Some("<root@example.com> <theirs-1@example.com>")
    );
    // Loop control has to survive: ingress refuses mail carrying `Auto-Submitted`, and the hop
    // count is what bounds channel-to-channel delegation.
    assert_eq!(
        request.headers.get("Auto-Submitted").map(String::as_str),
        Some("auto-replied")
    );
    assert_eq!(
        request
            .headers
            .get("X-MailAgents-Hop-Count")
            .map(String::as_str),
        Some("1")
    );
}

#[tokio::test]
async fn the_idempotency_key_is_the_rendered_message_id_and_so_is_stable_across_attempts() {
    let (_, first) = send_with(FakeResendApi::new().accepting("resend-id-1")).await;
    let (_, second) = send_with(FakeResendApi::new().accepting("resend-id-2")).await;

    assert_eq!(
        first.only_send().idempotency_key.as_deref(),
        Some("<delivery-abc@localhost>")
    );
    assert_eq!(
        first.only_send().idempotency_key,
        second.only_send().idempotency_key,
        "two attempts at one delivery must replay one key"
    );
}

#[tokio::test]
async fn a_rendered_header_never_overwrites_one_built_from_a_typed_field() {
    let mut forged = mail();
    forged.headers.push(MailHeader {
        name: "in-reply-to".to_string(),
        value: "<somewhere-else@example.com>".to_string(),
    });
    let api = Arc::new(FakeResendApi::new().accepting("resend-id-1"));
    ResendMailTransport::new(api.clone()).send(forged).await;

    assert_eq!(
        api.only_send()
            .headers
            .get("In-Reply-To")
            .map(String::as_str),
        Some("<theirs-1@example.com>"),
        "the threading header is the renderer's decision, not a free-form entry"
    );
}

#[tokio::test]
async fn a_display_name_that_could_change_the_envelope_is_dropped_rather_than_sent() {
    let mut spoofed = mail();
    spoofed.from_name = Some("Acme <evil@elsewhere.example>".to_string());
    let api = Arc::new(FakeResendApi::new().accepting("resend-id-1"));
    ResendMailTransport::new(api.clone()).send(spoofed).await;

    assert_eq!(api.only_send().from, "support@acme.localhost");
}

#[tokio::test]
async fn a_definite_refusal_is_terminal_and_names_which_kind_it_was() {
    for (status, expected) in [
        (422_u16, FailureClass::InvalidPayload),
        (401, FailureClass::Authentication),
        (403, FailureClass::Authentication),
        (404, FailureClass::DestinationUnavailable),
    ] {
        let (outcome, _) = send_with(FakeResendApi::new().sending(Err(ResendError::Refused {
            status,
            detail: "no".to_string(),
        })))
        .await;
        assert!(
            matches!(outcome, MailSendOutcome::Rejected { class, .. } if class == expected),
            "{status} should be {expected:?}, was {outcome:?}"
        );
    }
}

#[tokio::test]
async fn a_rate_limit_carries_the_wait_the_provider_asked_for() {
    let (outcome, _) = send_with(FakeResendApi::new().sending(Err(ResendError::RateLimited {
        retry_after: Some(Duration::from_secs(30)),
        detail: "slow down".to_string(),
    })))
    .await;

    assert!(matches!(
        outcome,
        MailSendOutcome::RateLimited {
            retry_after: Some(wait),
            ..
        } if wait == Duration::from_secs(30)
    ));
}

#[tokio::test]
async fn an_ambiguous_request_is_retryable_because_the_idempotency_key_is_replayed() {
    let (outcome, _) = send_with(FakeResendApi::new().sending(Err(ResendError::Unavailable {
        detail: "operation timed out".to_string(),
    })))
    .await;

    assert!(
        matches!(outcome, MailSendOutcome::Retryable { .. }),
        "was {outcome:?}"
    );
}

#[tokio::test]
async fn an_unreadable_answer_stays_ambiguous_despite_the_idempotency_key() {
    // The key makes a *repeat* safe. It says nothing about whether an answer this build could not
    // read was an acceptance, so this arm must not become a blind retry.
    for error in [
        ResendError::Malformed {
            detail: "not json".to_string(),
        },
        ResendError::TooLarge { limit: 16 },
    ] {
        let (outcome, _) = send_with(FakeResendApi::new().sending(Err(error))).await;
        assert!(
            matches!(outcome, MailSendOutcome::Unknown { .. }),
            "was {outcome:?}"
        );
    }
}

#[tokio::test]
async fn a_platform_notice_with_no_message_id_records_the_providers_own_id() {
    let mut notice = mail();
    notice.message_id = None;
    notice.in_reply_to = None;
    notice.references = Vec::new();
    let api = Arc::new(FakeResendApi::new().sending(Ok(ResendSendResponse {
        id: "56761188-7520-42d8-8898-ff6fc54ce618".to_string(),
    })));

    let outcome = ResendMailTransport::new(api.clone()).send(notice).await;

    assert_eq!(
        outcome,
        MailSendOutcome::Accepted {
            provider_key: Some(
                ExternalMessageKey::parse("56761188-7520-42d8-8898-ff6fc54ce618").unwrap()
            ),
        }
    );
    // Nothing to be idempotent about: there is no rendered id to replay.
    assert_eq!(api.only_send().idempotency_key, None);
}
