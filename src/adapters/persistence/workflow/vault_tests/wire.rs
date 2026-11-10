use super::*;
use crate::application::integrations::wire::test_transport::{Fault, SerializationGateway};

#[tokio::test]
async fn integration_service_contract_http_static_methods_keep_native_effect_and_redaction() {
    for (app, method) in [("static-bearer", "bearer"), ("static-api-key", "X-API-Key")] {
        let (setup, vault, account) = Box::pin(fixture(app)).await;
        let transport =
            ScriptedTransport::new("execution-private-sentinel", method, Delivery::Accepted);
        let gateway = Arc::new(SerializationGateway::new(
            super::gateway(&vault, transport.clone()),
            account.owner,
            Fault::None,
        ));
        let runtime = HttpRuntime::with_integrations(
            setup.fixture.persistence.clone(),
            HttpDestinationPolicy::default(),
            lease(),
            None,
            gateway,
        );
        let result = Box::pin(runtime.run(&setup.kind, &setup.claim))
            .await
            .completed_result()
            .unwrap();
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("execution-private-sentinel")
        );
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            action_counts(&setup.fixture).await,
            ActionCounts {
                dispatches: 1,
                entries: 1,
                receipts: 1
            }
        );
    }
}

#[tokio::test]
async fn integration_service_contract_http_local_cancel_is_not_replaced_by_decoded_context() {
    let (setup, vault, account) = Box::pin(fixture("static-bearer")).await;
    let transport =
        ScriptedTransport::new("execution-private-sentinel", "bearer", Delivery::Accepted);
    let gateway = Arc::new(SerializationGateway::new(
        super::gateway(&vault, transport.clone()),
        account.owner,
        Fault::CancelBeforeDispatch,
    ));
    let runtime = HttpRuntime::with_integrations(
        setup.fixture.persistence.clone(),
        HttpDestinationPolicy::default(),
        lease(),
        None,
        gateway,
    );
    assert!(
        Box::pin(runtime.run(&setup.kind, &setup.claim))
            .await
            .completed_result()
            .is_err()
    );
    assert_eq!(transport.preparations.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}
