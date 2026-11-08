use super::*;
#[path = "native_journeys.rs"]
mod native_journeys;
#[path = "native_mcp_portability.rs"]
mod native_mcp_portability;
#[path = "shared_runtime.rs"]
mod shared_runtime;
use crate::adapters::persistence::integrations::renewal_tests::{
    business_peer::{BusinessPeer, CredentialExpectation},
    server::{Reply, TokenServer},
    workspace,
};
use crate::application::integrations::private::CredentialStore;
use crate::application::integrations::registry::{
    CodeOwnedRegistry, Registration, static_registry,
};
use crate::domain::integrations::*;

#[derive(Clone, Copy)]
enum Case {
    Bearer,
    ApiKey,
    Google,
    Workspace,
}
impl Case {
    fn app(self) -> AppId {
        AppId::parse(match self {
            Self::Bearer => "static-bearer",
            Self::ApiKey => "portable-key-fixture",
            Self::Google => "gmail",
            Self::Workspace => "workspace-strict",
        })
        .unwrap()
    }
    fn destination(self) -> ApprovedDestination {
        ApprovedDestination::parse(if matches!(self, Self::Google) {
            "https://gmail.googleapis.com"
        } else {
            "https://api.example.test"
        })
        .unwrap()
    }
    fn path(self) -> &'static str {
        if matches!(self, Self::Google) {
            "/gmail/v1/users/me/threads/portable-thread"
        } else {
            "/portable/resource"
        }
    }
    fn access(self) -> &'static str {
        match self {
            Self::Google => "renewed-access-sentinel",
            Self::Workspace => "workspace-renewed-token",
            _ => "native-static-private-sentinel",
        }
    }
    fn registry(self, server: &TokenServer) -> Arc<CodeOwnedRegistry> {
        let mut registry = static_registry();
        let entry = match self {
            Self::Bearer => None,
            Self::ApiKey => {
                let base = registry
                    .get(&AppId::parse("static-api-key").unwrap())
                    .unwrap();
                let mut app = base.app.clone();
                app.id = self.app();
                Some(Registration {
                    provider: base.provider.clone(),
                    config: base.config.clone(),
                    app,
                    adapter: base.adapter.clone(),
                    authorization: None,
                    renewal: crate::application::integrations::renewal::RenewalStrategy::NoRenewal,
                })
            }
            Self::Google => Some(server.google_entry()),
            Self::Workspace => {
                *server.state.reply.lock().unwrap() = Reply::Json(workspace::response());
                Some(workspace::registration(server))
            }
        };
        if let Some(entry) = entry {
            registry.register(entry).unwrap();
        }
        Arc::new(registry)
    }
    fn payload(self) -> SecretString {
        match self {
            Self::Google => super::super::renewal::google_payload(
                "portable-mailbox-subject",
                Utc::now() - ChronoDuration::seconds(1),
            ),
            Self::Workspace => workspace::payload(),
            _ => SecretString::from(self.access()),
        }
    }
}
async fn native_setup(case: Case) -> Setup {
    // Keep the publication/admission future behind a pointer in outer fixture frames.
    let (setup, original) = Box::pin(native_setup_pair(case)).await;
    // A single-run fixture must not retain the extra workflow's capacity slot.
    // The pair variant retains it only for journeys that execute both workflows.
    assert!(
        setup
            .fixture
            .persistence
            .release_io(
                original.fence,
                lease(),
                crate::application::workflow::lease::LeaseReleaseCause::LocalInterruption
            )
            .await
            .unwrap()
    );
    setup
}
async fn native_setup_pair(case: Case) -> (Setup, ClaimedWorkflow) {
    let mut setup = Box::pin(setup(case.destination().as_str(), Entrance::Http)).await;
    let original = setup.claim.clone();
    setup.fixture.target.workflow = WorkflowId::new(Uuid::new_v4());
    let mut source: Value = serde_json::from_str(
        &crate::application::workflow::registry::example("http.request")
            .unwrap()
            .source,
    )
    .unwrap();
    source["workflow_id"] = json!(setup.fixture.target.workflow);
    source["steps"]["start"]["with"]["method"] = json!({"literal":"GET"});
    source["steps"]["start"]["with"]["path"] = json!({"literal":case.path()});
    source["steps"]["start"]["with"]
        .as_object_mut()
        .unwrap()
        .remove("body");
    let directory = crate::adapters::persistence::workflow::http::HttpPublicationDirectory {
        persistence: setup.fixture.persistence.clone(),
        selections: vec![
            crate::adapters::persistence::workflow::http::HttpPublicationSelection {
                connection: setup.connection.id,
                slot: Some(ResourceName::parse("service").unwrap()),
                operation: TypeName::parse("http.request").unwrap(),
            },
        ],
    };
    let definitions = DefinitionService::new(
        Preflight,
        setup.fixture.persistence.clone(),
        WorkflowSourceDecoder,
        directory,
    );
    setup.version = VersionId::new(Uuid::new_v4());
    Box::pin(setup.fixture.publish_source(
        &definitions,
        setup.fixture.target.workflow,
        source.to_string(),
        setup.version,
    ))
    .await
    .unwrap();
    let target = Box::pin(bound(
        &setup.fixture,
        &setup.connection,
        setup.version,
        Entrance::Http,
        RelatedAssociation::Company,
    ))
    .await;
    setup.claim = Box::pin(
        crate::adapters::persistence::workflow::tests::http_tests::support::claimed_with_key(
            &setup.fixture,
            target,
            RelatedAssociation::Company,
            "portable-native",
        ),
    )
    .await;
    (setup, original)
}
async fn account_for(setup: &Setup, vault: &PostgresVault, case: Case) -> ConnectedAccount {
    account_with_payload(setup, vault, case, case.payload()).await
}
async fn account_with_payload(
    setup: &Setup,
    vault: &PostgresVault,
    case: Case,
    payload: SecretString,
) -> ConnectedAccount {
    let entry: &Registration = vault.registry.get(&case.app()).unwrap();
    vault
        .create(
            &owner_for_user(setup.fixture.target.actor.user_id()),
            AccountWrite {
                key: AccountKey::parse("native-portable").unwrap(),
                provider: entry.provider.id.clone(),
                config: entry.config.id.clone(),
                method: entry.provider.method,
                schema: entry.provider.schema.clone(),
                identity: entry.adapter.validate(&payload).unwrap(),
                metadata: AccountMetadata::new(
                    "Portable account without login identity".into(),
                    None,
                )
                .unwrap(),
                payload,
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn integration_portability_all_auth_methods_use_native_http_headers_and_unchanged_workflow_contract()
 {
    for case in [Case::Bearer, Case::ApiKey, Case::Google, Case::Workspace] {
        Box::pin(native_http(case)).await;
    }
}
async fn native_http(case: Case) {
    let setup = Box::pin(native_setup(case)).await;
    let server = TokenServer::new().await;
    let vault =
        PostgresVault::with_registry(setup.fixture.persistence.clone(), case.registry(&server));
    let account = Box::pin(account_for(&setup, &vault, case)).await;
    Box::pin(attach_source(&setup, &vault, &account, case.app())).await;
    let expected = if matches!(case, Case::ApiKey) {
        CredentialExpectation::ApiKey(SecretString::from(case.access()))
    } else {
        CredentialExpectation::Bearer(SecretString::from(case.access()))
    };
    let peer = BusinessPeer::new(
        expected,
        json!({"accepted":true,"echo":case.access()}),
        None,
    )
    .await;
    let gateway = Arc::new(AuthenticatedExecution {
        registry: vault.registry.clone(),
        credentials: Arc::new(vault.clone()),
        transports: peer.transports(case.destination()),
    });
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
    assert_eq!(result["body"]["accepted"], true);
    assert!(!result.to_string().contains(case.access()));
    assert_eq!(peer.calls(), 1);
    assert_eq!(peer.body_digests().len(), 1);
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 1
        }
    );
    let renewals = usize::from(matches!(case, Case::Google | Case::Workspace));
    assert_eq!(server.calls(), renewals);
    Box::pin(runtime.run(&setup.kind, &setup.claim))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(peer.calls(), 1);
    assert_eq!(server.calls(), renewals);
    assert!(
        static_registry()
            .get(&AppId::parse("portable-key-fixture").unwrap())
            .is_err()
    );
    peer.close().await;
}

#[tokio::test]
async fn vault_google_e2e_owner_consent_reuses_one_private_secret_in_two_native_workflows() {
    let case = Case::Bearer;
    let (setup, original) = Box::pin(native_setup_pair(case)).await;
    // These journeys do not inject frozen-data faults; exercise the real snapshot guard.
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(setup.fixture.persistence.pool())
        .await
        .unwrap();
    let registry = crate::application::integrations::registry::static_registry();
    let vault = PostgresVault::with_registry(setup.fixture.persistence.clone(), Arc::new(registry));
    let account = Box::pin(account_for(&setup, &vault, case)).await;
    Box::pin(attach_source(&setup, &vault, &account, case.app())).await;
    let peer = BusinessPeer::new(
        CredentialExpectation::Bearer(SecretString::from(case.access())),
        json!({"accepted":true,"echo":case.access()}),
        None,
    )
    .await;
    let gateway = Arc::new(AuthenticatedExecution {
        registry: vault.registry.clone(),
        credentials: Arc::new(vault.clone()),
        transports: peer.transports(case.destination()),
    });
    let runtime = HttpRuntime::with_integrations(
        setup.fixture.persistence.clone(),
        HttpDestinationPolicy::default(),
        lease(),
        None,
        gateway,
    );
    assert_ne!(original.fence.scope.run, setup.claim.fence.scope.run);
    for claim in [&original, &setup.claim] {
        let result = Box::pin(runtime.run(&setup.kind, claim))
            .await
            .completed_result()
            .unwrap();
        assert_eq!(result["body"]["accepted"], true);
        assert!(!result.to_string().contains(case.access()));
    }
    assert_eq!(peer.calls(), 2);
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 2,
            entries: 2,
            receipts: 2
        }
    );
    let foreign = AccountOwner {
        namespace: account.owner.namespace.clone(),
        subject: OwnerSubject::parse(Uuid::new_v4().to_string()).unwrap(),
    };
    assert!(vault.get(&foreign, account.id).await.is_err());
    assert!(vault.read_usable(&foreign, account.id).await.is_err());
    let owner_metadata =
        serde_json::to_string(&vault.get(&account.owner, account.id).await.unwrap()).unwrap();
    assert!(!owner_metadata.contains(case.access()) && !owner_metadata.contains("envelope"));
    peer.close().await;
}
