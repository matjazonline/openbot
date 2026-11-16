use super::*;
use crate::entities::{address::AddressTarget, transport::PrincipalId};

#[test]
fn direct_trace_render_preserves_same_uuid_principal_and_desk_hops() {
    let mut ctx = context("customer@example.test");
    let relay = ctx.relay.as_mut().unwrap();
    let id = relay.source_target.desk_id().unwrap();
    relay.trace_targets = vec![AddressTarget::Principal(PrincipalId::new(id))];
    let email = frozen(&renderer().render(&envelope(ctx)).unwrap());
    let (_, trace) = email.wire_relay().unwrap();
    assert_eq!(
        trace,
        vec![
            AddressTarget::Principal(PrincipalId::new(id)),
            AddressTarget::Desk(id)
        ]
    );
}

#[test]
fn direct_trace_renderer_bounds_frozen_and_wire_traces_without_raising_the_hop_limit() {
    let mut ctx = context("customer@example.test");
    let relay = ctx.relay.as_mut().unwrap();
    relay.source_target = AddressTarget::Desk(Uuid::from_u128(100));
    relay.hop_count = crate::transport::MAX_INGRESS_HOPS;
    relay.trace_targets = (0..crate::transport::MAX_TRACE_TARGETS)
        .map(|i| AddressTarget::Desk(Uuid::from_u128(i as u128)))
        .collect();
    let email = frozen(&renderer().render(&envelope(ctx.clone())).unwrap());
    let (hops, trace) = email.wire_relay().unwrap();
    assert_eq!(hops, crate::transport::MAX_INGRESS_HOPS + 1);
    assert_eq!(trace.len(), crate::transport::MAX_TRACE_TARGETS + 1);
    // The spent next hop is still an outbound reply; any receiving platform refuses it.
    ctx.relay
        .as_mut()
        .unwrap()
        .trace_targets
        .push(AddressTarget::Desk(Uuid::nil()));
    assert!(renderer().render(&envelope(ctx.clone())).is_err());
    ctx.relay.as_mut().unwrap().trace_targets.clear();
    ctx.relay.as_mut().unwrap().hop_count = u32::MAX;
    assert!(renderer().render(&envelope(ctx)).is_err());
}

#[tokio::test]
async fn direct_trace_sender_and_correlation_refuse_an_oversized_frozen_payload_before_delivery() {
    let transport = Arc::new(RecordingTransport::default());
    let sender = EmailSender::new(
        DeploymentMailTransports::only(transport.clone()),
        Arc::new(NeverInternal::default()),
    );
    let mut parts = renderer()
        .render(&envelope(context("customer@example.test")))
        .unwrap();
    let mut email = frozen(&parts);
    email.relay.as_mut().unwrap().trace_targets =
        vec![AddressTarget::Desk(Uuid::nil()); crate::transport::MAX_TRACE_TARGETS + 1];
    parts[0].payload =
        TransportPayload::encode(TransportKind::Email, OUTBOUND_EMAIL_VERSION, &email).unwrap();
    assert!(renderer().rendered_correlation(&parts[0]).is_err());
    assert!(matches!(
        sender.send(&record(), &parts[0]).await,
        ProviderSendOutcome::Terminal {
            class: FailureClass::InvalidPayload,
            ..
        }
    ));
    assert!(transport.sent().is_empty());
}
