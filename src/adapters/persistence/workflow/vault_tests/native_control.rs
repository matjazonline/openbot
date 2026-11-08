use super::*;
use crate::adapters::persistence::integration_bindings::binding;
use crate::application::integrations::wire::{self, Command, Reply};

fn confirmation(f: &Journey, p: &BindingProposal) -> OwnerConfirmation {
    OwnerConfirmation {
        owner: VerifiedOwner::from_verified_client(f.account.owner.clone()),
        account: f.account.id,
        binding: binding(p.company, p.connection, p.kind),
        host_scope: company_binding(p.company),
        purpose: InvocationKind::DeterministicHttp,
        expected_authorization: f.account.authorization_revision,
        app: p.app.clone(),
        destination: p.destination.clone(),
        host_authority: HostAuthorityReference::parse(p.id.to_string()).unwrap(),
    }
}
fn grant_command(c: &OwnerConfirmation) -> Command {
    Command::CreateGrant {
        account: c.account,
        expected: c.expected_authorization,
        scope: c.host_scope.clone(),
        binding: c.binding.clone(),
        app: c.app.clone(),
        destination: c.destination.clone(),
        purpose: c.purpose,
        host_authority: c.host_authority.clone(),
    }
}
async fn round_trip(
    gateway: &dyn IntegrationGateway,
    owner: &VerifiedOwner,
    command: Command,
    c: Option<OwnerConfirmation>,
) -> Reply {
    let deadline = Utc::now() + ChronoDuration::seconds(20);
    let request = wire::Request {
        version: wire::PROTOCOL_VERSION,
        identity: DispatchId::new(Uuid::new_v4()),
        owner: owner.owner().clone(),
        deadline,
        cancellation: CancellationId::parse(Uuid::new_v4().to_string()).unwrap(),
        command,
    };
    let local = wire::control_transport::LocalContext::new(
        &request,
        owner,
        ConnectWork {
            deadline,
            cancellation: CancellationToken::new(),
        },
        c,
    )
    .unwrap();
    let bytes = wire::control_transport::exchange(gateway, local, &request.encode().unwrap())
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(Case::Bearer.access()));
    wire::Response::decode(&bytes, request.identity)
        .unwrap()
        .reply
}
#[tokio::test]
async fn integration_service_contract_wire_grant_requires_original_host_consent_and_revoke_denies_native_dispatch()
 {
    let f = Box::pin(Journey::new(Case::Bearer)).await;
    let (host, proposal) = new_proposal(&f).await;
    let unavailable =
        Arc::new(crate::application::integrations::unavailable::UnavailableIntegrations);
    let gateway = f
        .vault
        .gateway(Arc::new(host), unavailable.clone(), unavailable);
    let owner = VerifiedOwner::from_verified_client(f.account.owner.clone());
    assert_grant_identities_bound(&f, gateway.as_ref(), &owner, &proposal).await;
    let denied = round_trip(
        gateway.as_ref(),
        &owner,
        grant_command(&confirmation(&f, &proposal)),
        None,
    )
    .await;
    assert!(matches!(
        denied,
        Reply::Failure {
            failure: wire::Failure::Denied
        }
    ));
    let c = confirmation(&f, &proposal);
    let grant = round_trip(gateway.as_ref(), &owner, grant_command(&c), Some(c)).await;
    let Reply::Grant { grant } = grant else {
        panic!("genuine consent must create grant");
    };
    let c = confirmation(&f, &proposal);
    assert!(
        matches!(
            round_trip(gateway.as_ref(), &owner, grant_command(&c), Some(c)).await,
            Reply::Failure { .. }
        ),
        "host proposal is consumed only once"
    );
    assert!(matches!(
        round_trip(
            gateway.as_ref(),
            &owner,
            Command::RevokeGrant {
                grant: grant.id,
                expected: grant.revision
            },
            None
        )
        .await,
        Reply::Done {}
    ));
    assert!(
        Box::pin(f.runtime.run(&f.setup.kind, &f.setup.claim))
            .await
            .completed_result()
            .is_err()
    );
    assert_eq!(f.peer.calls(), 0);
    assert_eq!(f.token.calls(), 0);
    let inspection = f
        .vault
        .grants(&f.account.owner, f.account.id, None)
        .await
        .unwrap();
    assert!(inspection.grants.iter().all(|g| !g.enabled));
    f.peer.close().await;
}

async fn assert_grant_identities_bound(
    f: &Journey,
    gateway: &dyn IntegrationGateway,
    owner: &VerifiedOwner,
    proposal: &BindingProposal,
) {
    let before = gateway
        .inspect_grants(owner, f.account.id, None)
        .await
        .unwrap();
    for identity in [true, false] {
        let c = confirmation(f, proposal);
        let deadline = Utc::now() + ChronoDuration::seconds(20);
        let mut request = wire::Request {
            version: wire::PROTOCOL_VERSION,
            identity: DispatchId::new(Uuid::new_v4()),
            owner: owner.owner().clone(),
            deadline,
            cancellation: CancellationId::parse(Uuid::new_v4().to_string()).unwrap(),
            command: grant_command(&c),
        };
        let local = wire::control_transport::LocalContext::new(
            &request,
            owner,
            ConnectWork {
                deadline,
                cancellation: CancellationToken::new(),
            },
            Some(c),
        )
        .unwrap();
        if identity {
            request.identity = DispatchId::new(Uuid::new_v4());
        } else {
            request.cancellation = CancellationId::parse(Uuid::new_v4().to_string()).unwrap();
        }
        assert!(matches!(
            wire::control_transport::exchange(gateway, local, &request.encode().unwrap()).await,
            Err(IntegrationError::Denied)
        ));
    }
    let after = gateway
        .inspect_grants(owner, f.account.id, None)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    // The positive exchange immediately following this helper still consumes the
    // original proposal, proving the rejected envelopes did not consume consent.
}

async fn new_proposal(f: &Journey) -> (PostgresIntegrationBindings, BindingProposal) {
    let host = PostgresIntegrationBindings {
        persistence: f.vault.persistence.clone(),
        vault: f.vault.clone(),
    };
    let current = f
        .vault
        .persistence
        .list_http(f.setup.fixture.target.company, f.setup.fixture.target.actor)
        .await
        .unwrap()
        .remove(0);
    let proposal = host
        .propose(
            f.setup.fixture.target.actor.user_id(),
            ProposeBinding {
                company: f.setup.fixture.target.company.as_uuid(),
                connection: current.id,
                kind: BindingKind::Http,
                app: Case::Bearer.app(),
                expected_revision: current.revision,
            },
        )
        .await
        .unwrap();
    (host, proposal)
}
