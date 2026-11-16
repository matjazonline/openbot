use super::*;
use crate::entities::{address::AddressTarget, transport::PrincipalId};
use uuid::Uuid;

fn accept(headers: String) -> Result<AcceptedEmail, EmailIngressError> {
    EmailIngressAdapter::new("direct.test").accept(
        RawInboundPayload {
            headers: Some(headers),
            from: "sender@example.test".into(),
            to: "support@acme.direct.test".into(),
            text: Some("A message".into()),
            ..Default::default()
        },
        EmailIngressTrust::Application,
    )
}

#[test]
fn direct_trace_email_preserves_equal_uuid_targets_with_different_kinds() {
    let id = Uuid::new_v4();
    let targets = vec![
        AddressTarget::Desk(id),
        AddressTarget::Principal(PrincipalId::new(id)),
    ];
    let header = targets
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let accepted = accept(format!("X-MailAgents-Trace: {header}\n")).unwrap();
    assert_eq!(
        &*accepted.draft.directives.trace_targets,
        targets.as_slice()
    );
    assert_ne!(targets[0], targets[1]);
}

#[test]
fn direct_trace_email_refuses_malformed_duplicate_folded_and_over_budget_headers() {
    let id = Uuid::new_v4();
    for trace in [
        String::new(),
        id.to_string(),
        format!("other:{id}"),
        "principal:broken".into(),
        format!("desk:{id},broken"),
        vec![format!("desk:{id}"); crate::transport::MAX_TRACE_TARGETS + 1].join(","),
    ] {
        assert!(
            matches!(
                accept(format!("X-MailAgents-Trace: {trace}\n")),
                Err(EmailIngressError::RelayTrace)
            ),
            "{trace}"
        );
    }
    for headers in [
        format!("X-MailAgents-Trace: desk:{id}\nX-MailAgents-Trace: principal:{id}\n"),
        format!("X-MailAgents-Trace: desk:{id}\n principal:{id}\n"),
    ] {
        assert!(matches!(
            accept(headers),
            Err(EmailIngressError::RelayTrace)
        ));
    }
}
