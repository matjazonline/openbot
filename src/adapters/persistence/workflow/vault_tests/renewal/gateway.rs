use super::super::execution::{Delivery, ScriptedTransport};
use super::*;
use crate::adapters::persistence::workflow::tests::http_tests::approval_tests;
use crate::application::workflow::decisions::*;

#[tokio::test]
async fn vault_authoring_expired_google_publication_and_sealed_sample_never_refresh() {
    use crate::application::workflow::sample::{SampleRequest, SampleStatus};
    use crate::application::workflow::sample_capture::SampleDefinitionCapture;
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let fixture = &f.setup.fixture;
    let p = &fixture.persistence;
    let directory = http::HttpPublicationDirectory {
        persistence: p.clone(),
        selections: vec![http::HttpPublicationSelection {
            connection: f.setup.connection.id,
            slot: Some(ResourceName::parse("service").unwrap()),
            operation: TypeName::parse("http.request").unwrap(),
        }],
    };
    let draft = p
        .draft(fixture.target.company, fixture.target.workflow)
        .await
        .unwrap()
        .unwrap();
    directory
        .capture(fixture.target.actor, &draft.draft)
        .await
        .unwrap();
    let capture = crate::adapters::persistence::workflow::SampleCapture {
        persistence: p.clone(),
        http: directory.selections,
        mcp: vec![],
        refused_kinds: &[],
    };
    let request:SampleRequest = serde_json::from_value(json!({"input":{},"params":{},"agents":[],
        "scripts":[{"workflow_id":fixture.target.workflow,"step":"start","responses":[{"output":{"status":200,"headers":{},"body":{"fixture":true}}}]}]})).unwrap();
    let sealed = Box::pin(capture.capture(fixture.target, draft.draft.revision(), request))
        .await
        .unwrap();
    let report = crate::application::workflow::sample::execute(sealed).unwrap();
    assert_eq!(report.status, SampleStatus::Completed);
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("access-sentinel")
    );
    assert_eq!(f.server.calls(), 0);
    assert_eq!(f.credential().await.versions.generation.get(), 1);
    assert_eq!(
        action_counts(fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}

#[tokio::test]
async fn integration_gateway_gmail_path_policy_rejects_before_refresh_or_business_entry() {
    let setup = Box::pin(setup("https://gmail.googleapis.com", Entrance::Http)).await;
    let server = TokenServer::new().await;
    let mut registry = CodeOwnedRegistry::default();
    registry.register(server.google_entry()).unwrap();
    let vault = PostgresVault::with_registry(setup.fixture.persistence.clone(), Arc::new(registry));
    let account = managed(
        &vault,
        setup.fixture.target.actor.user_id(),
        "policy-mailbox",
        Utc::now() - ChronoDuration::seconds(1),
    )
    .await;
    attach_app(&setup, &vault, &account, AppId::parse("gmail").unwrap()).await;
    let transport = ScriptedTransport::new("never-read", "bearer", Delivery::Accepted);
    let actions = super::super::execution::runtime(&setup, &vault, transport.clone());
    assert!(
        Box::pin(actions.run(&setup.kind, &setup.claim))
            .await
            .completed_result()
            .is_err()
    );
    assert_eq!(server.calls(), 0);
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

#[tokio::test]
async fn vault_workflow_http_google_refresh_waits_for_approval_then_keeps_native_receipt() {
    let setup = Box::pin(approval_tests::protected(
        "https://api.example.test",
        Entrance::Http,
    ))
    .await;
    let server = TokenServer::new().await;
    let vault =
        PostgresVault::with_registry(setup.fixture.persistence.clone(), google_registry(&server));
    let expires = Utc::now() + ChronoDuration::seconds(5);
    let account = managed(
        &vault,
        setup.fixture.target.actor.user_id(),
        "approved-mailbox",
        expires,
    )
    .await;
    attach_app(&setup, &vault, &account, AppId::parse("gmail").unwrap()).await;
    let transport = ScriptedTransport::new("renewed-access-sentinel", "bearer", Delivery::Accepted);
    let p = &setup.fixture.persistence;
    let notices = crate::adapters::persistence::workflow::tests::approval_test_support::notices(p);
    let actions = HttpRuntime::with_integrations(
        p.clone(),
        HttpDestinationPolicy::default(),
        lease(),
        Some(notices),
        super::super::execution::gateway(&vault, transport.clone()),
    );
    let first = Box::pin(actions.run(&setup.kind, &setup.claim)).await;
    let WorkflowHandlerOutcome::Suspended(wait) = first else {
        panic!("expected approval wait");
    };
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.preparations.load(Ordering::SeqCst), 0);
    assert_eq!(server.calls(), 0);
    // Let the original cache expire during the real native approval wait.
    if let Ok(remaining) = (expires - Utc::now()).to_std() {
        tokio::time::sleep(remaining + Duration::from_millis(50)).await;
    }
    assert!(Utc::now() > expires);
    let decision = approval_tests::view(&setup).await;
    p.submit_decision(approval_tests::submission(&setup, &decision, "approve"))
        .await
        .unwrap();
    assert!(
        p.confirm_suspension(setup.claim.fence, wait.wait)
            .await
            .unwrap()
    );
    let next = p
        .claim_io(
            setup.claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    Box::pin(actions.run(&setup.kind, &next))
        .await
        .completed_result()
        .unwrap();
    Box::pin(actions.run(&setup.kind, &next))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(server.calls(), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        vault
            .get(&account.owner, account.id)
            .await
            .unwrap()
            .authorization_revision,
        account.authorization_revision
    );
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 1
        }
    );
}
