use super::*;
#[path = "native_control.rs"]
mod control;
#[path = "native_lifecycle.rs"]
mod lifecycle;
#[path = "owner_browser.rs"]
mod owner_browser;
#[path = "native_recovery.rs"]
mod recovery;
use crate::adapters::persistence::workflow::tests::http_tests::approval_tests;
use crate::adapters::persistence::workflow::tests::http_tests::vault_tests::renewal as renewal_support;
use crate::application::workflow::{decisions::WorkflowDecisions, lease::WorkflowHandlerOutcome};

struct Journey {
    setup: Setup,
    vault: PostgresVault,
    account: ConnectedAccount,
    token: TokenServer,
    peer: BusinessPeer,
    runtime: HttpRuntime,
}
impl Journey {
    async fn new(case: Case) -> Self {
        let setup = Box::pin(native_setup(case)).await;
        Self::assemble(setup, case, case.payload()).await
    }
    async fn assemble(setup: Setup, case: Case, payload: SecretString) -> Self {
        sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
            .execute(setup.fixture.persistence.pool())
            .await
            .unwrap();
        let token = TokenServer::new().await;
        let vault =
            PostgresVault::with_registry(setup.fixture.persistence.clone(), case.registry(&token));
        let account = account_with_payload(&setup, &vault, case, payload).await;
        Box::pin(attach_source(&setup, &vault, &account, case.app())).await;
        let expected = if matches!(case, Case::ApiKey) {
            CredentialExpectation::ApiKey(SecretString::from(case.access()))
        } else {
            CredentialExpectation::Bearer(SecretString::from(case.access()))
        };
        let peer = BusinessPeer::new(
            expected,
            json!({"id":"provider-message-7","threadId":"portable-thread","accepted":true}),
            None,
        )
        .await;
        let runtime = HttpRuntime::with_integrations(
            setup.fixture.persistence.clone(),
            HttpDestinationPolicy::default(),
            lease(),
            Some(
                crate::adapters::persistence::workflow::tests::approval_test_support::notices(
                    &setup.fixture.persistence,
                ),
            ),
            Arc::new(AuthenticatedExecution {
                registry: vault.registry.clone(),
                credentials: Arc::new(vault.clone()),
                transports: peer.transports(case.destination()),
            }),
        );
        Self {
            setup,
            vault,
            account,
            token,
            peer,
            runtime,
        }
    }
}

#[tokio::test]
async fn vault_google_e2e_reviewed_supplied_mime_resumes_after_expiry_with_same_approval() {
    let sample: Value = serde_json::from_str(include_str!(
        "../../../../../samples/gmail-reviewed-send.json"
    ))
    .unwrap();
    let reviewed = json!({"raw":sample["sample"]["input"]["raw"],"threadId":"portable-thread"});
    let expires = Utc::now() + ChronoDuration::seconds(2);
    let setup = Box::pin(approval_tests::protected_customized(
        Case::Google.destination().as_str(),
        Entrance::Http,
        1,
        |source| {
            source["steps"]["start"]["with"]["method"] = json!({"literal":"POST"});
            source["steps"]["start"]["with"]["path"] =
                json!({"literal":"/gmail/v1/users/me/messages/send"});
            source["steps"]["start"]["with"]["body"] = json!({"literal":reviewed});
        },
    ))
    .await;
    let payload = renewal_support::google_payload("portable-mailbox-subject", expires);
    let f = Box::pin(Journey::assemble(setup, Case::Google, payload)).await;
    let first = Box::pin(f.runtime.run(&f.setup.kind, &f.setup.claim)).await;
    let WorkflowHandlerOutcome::Suspended(wait) = first else {
        panic!("review required");
    };
    assert_eq!(f.peer.calls(), 0);
    assert_eq!(f.token.calls(), 0);
    if let Ok(remaining) = (expires - Utc::now()).to_std() {
        tokio::time::sleep(remaining + Duration::from_millis(20)).await;
    }
    assert!(Utc::now() > expires);
    let p = &f.setup.fixture.persistence;
    let decision = approval_tests::view(&f.setup).await;
    p.submit_decision(approval_tests::submission(&f.setup, &decision, "approve"))
        .await
        .unwrap();
    assert!(
        p.confirm_suspension(f.setup.claim.fence, wait.wait)
            .await
            .unwrap()
    );
    let claim = p
        .claim_io(
            f.setup.claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    let result = Box::pin(f.runtime.run(&f.setup.kind, &claim))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(result["body"]["id"], "provider-message-7");
    assert_eq!(result["body"]["threadId"], "portable-thread");
    Box::pin(f.runtime.run(&f.setup.kind, &claim))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(f.peer.calls(), 1);
    assert_reviewed_request(&f, &reviewed);
    assert_eq!(f.token.calls(), 1);
    assert_eq!(
        f.vault
            .get(&f.account.owner, f.account.id)
            .await
            .unwrap()
            .authorization_revision,
        f.account.authorization_revision
    );
    assert_eq!(
        action_counts(&f.setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 1
        }
    );
    f.peer.close().await;
}

fn assert_reviewed_request(f: &Journey, reviewed: &Value) {
    use sha2::{Digest, Sha256};
    let expected: [u8; 32] = Sha256::digest(serde_json::to_vec(reviewed).unwrap()).into();
    assert_eq!(f.peer.body_digests(), vec![expected]);
    let requests = f.peer.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, axum::http::Method::POST);
    assert_eq!(requests[0].1.path(), "/gmail/v1/users/me/messages/send");
}

#[derive(Clone, Copy, Debug)]
enum Mutation {
    Disconnect,
    Membership,
    Rebind,
    Operation,
}
#[tokio::test]
async fn vault_google_e2e_stalled_refresh_rechecks_disconnect_membership_rebind_and_operation() {
    for mutation in [
        Mutation::Disconnect,
        Mutation::Membership,
        Mutation::Rebind,
        Mutation::Operation,
    ] {
        Box::pin(stalled(mutation)).await;
    }
}
async fn stalled(mutation: Mutation) {
    let mut f = Box::pin(Journey::new(Case::Google)).await;
    if matches!(mutation, Mutation::Membership) {
        Box::pin(as_nonowner_admin(&mut f)).await;
    }
    let service = Box::pin(renewal_support::independent_service(&f.vault)).await;
    let rebind = if matches!(mutation, Mutation::Rebind) {
        Some(rebind_account(&f, &service.vault).await)
    } else {
        None
    };
    let held = f.token.state.gate.clone().acquire_owned().await.unwrap();
    let (outcome, ()) = tokio::join!(
        Box::pin(f.runtime.run(&f.setup.kind, &f.setup.claim)),
        async {
            f.token.wait_calls(1).await;
            let mutation = Box::pin(mutate(&f, &service.vault, mutation, rebind.as_ref()));
            tokio::pin!(mutation);
            tokio::select! {
                () = &mut mutation => {
                    // Changes to other rows can commit while refresh holds the credential lock.
                    assert_eq!(f.peer.calls(), 0);
                    drop(held);
                }
                () = renewal_support::wait_blocked(&f.vault, &service.application) => {
                    assert_eq!(f.peer.calls(), 0);
                    drop(held);
                    mutation.await;
                }
            }
        }
    );
    assert!(outcome.completed_result().is_err(), "{mutation:?}");
    assert_eq!(f.peer.calls(), 0, "{mutation:?}");
    assert!(
        f.peer.requests().is_empty(),
        "{mutation:?} must not send even an unauthenticated request"
    );
    assert_eq!(f.token.calls(), 1);
    assert_eq!(
        action_counts(&f.setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
    service.vault.persistence.pool().close().await;
    f.peer.close().await;
}
async fn as_nonowner_admin(f: &mut Journey) {
    let user = member(&f.setup).await;
    // Manual admission requires an owner/admin. This actor's authority comes
    // from a removable membership, while the private account owner stays separate.
    sqlx::query(
        "UPDATE company_members SET role='admin',access_level=9 WHERE company_id=$1 AND user_id=$2",
    )
    .bind(f.setup.fixture.target.company.as_uuid())
    .bind(user)
    .execute(f.setup.fixture.persistence.pool())
    .await
    .unwrap();
    let target = Box::pin(bound(
        &f.setup.fixture,
        &f.setup.connection,
        f.setup.version,
        Entrance::Http,
        RelatedAssociation::Company,
    ))
    .await;
    assert!(
        f.setup
            .fixture
            .persistence
            .release_io(
                f.setup.claim.fence,
                lease(),
                crate::application::workflow::lease::LeaseReleaseCause::LocalInterruption
            )
            .await
            .unwrap()
    );
    f.setup.fixture.target.actor = WorkflowActor::authenticated(user).unwrap();
    f.setup.claim = Box::pin(
        crate::adapters::persistence::workflow::tests::http_tests::support::claimed_with_key(
            &f.setup.fixture,
            target,
            RelatedAssociation::Company,
            "member-native-run",
        ),
    )
    .await;
}
async fn mutate(
    f: &Journey,
    vault: &PostgresVault,
    mutation: Mutation,
    rebind: Option<&ConnectedAccount>,
) {
    let p = &vault.persistence;
    let company = f.setup.fixture.target.company;
    let actor = f.setup.fixture.target.actor;
    match mutation {
        Mutation::Disconnect => {
            vault
                .disconnect(
                    &f.account.owner,
                    f.account.id,
                    f.account.authorization_revision,
                )
                .await
                .unwrap();
        }
        Mutation::Membership => {
            p.remove_member(company.as_uuid(), actor.user_id())
                .await
                .unwrap();
        }
        Mutation::Operation => {
            let current = p
                .list_http(company, actor)
                .await
                .unwrap()
                .into_iter()
                .find(|c| c.id == f.setup.connection.id)
                .unwrap();
            p.revoke_http_operation(
                company,
                actor,
                current.id,
                current.revision,
                TypeName::parse("http.request").unwrap(),
            )
            .await
            .unwrap();
        }
        Mutation::Rebind => {
            let account = rebind.expect("rebind account must exist before the refresh barrier");
            let setup = Setup {
                fixture: Fixture {
                    _db: f.setup.fixture._db.clone(),
                    persistence: p.clone(),
                    target: f.setup.fixture.target,
                },
                connection: f.setup.connection.clone(),
                version: f.setup.version,
                claim: f.setup.claim.clone(),
                kind: f.setup.kind.clone(),
            };
            Box::pin(attach_source(&setup, vault, account, Case::Google.app())).await;
        }
    }
}

async fn rebind_account(f: &Journey, vault: &PostgresVault) -> ConnectedAccount {
    let entry = vault.registry.get(&Case::Google.app()).unwrap();
    let payload =
        renewal_support::google_payload("different-mailbox", Utc::now() + ChronoDuration::hours(1));
    vault
        .create(
            &f.account.owner,
            AccountWrite {
                key: AccountKey::parse("different-mailbox").unwrap(),
                provider: entry.provider.id.clone(),
                config: entry.config.id.clone(),
                method: entry.provider.method,
                schema: entry.provider.schema.clone(),
                identity: entry.adapter.validate(&payload).unwrap(),
                metadata: AccountMetadata::new("Other mailbox".into(), None).unwrap(),
                payload,
            },
        )
        .await
        .unwrap()
}
