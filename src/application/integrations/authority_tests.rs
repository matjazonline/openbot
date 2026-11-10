use super::*;

#[test]
fn vault_authority_matrix_requires_owner_confirmation_and_current_company_authority() {
    let account = account();
    for owner_member in [false, true] {
        for admin in [false, true] {
            for confirmed in [false, true] {
                assert_eq!(
                    authorize_grant(
                        &account.owner,
                        &account,
                        &account.owner.namespace,
                        GrantAuthorityFacts {
                            owner_is_member: owner_member,
                            administrator_can_edit: admin,
                            owner_confirmed: confirmed
                        }
                    )
                    .is_ok(),
                    owner_member && admin && confirmed
                );
            }
        }
    }
    assert!(
        authorize_grant(
            &owner("other-client", "owner"),
            &account,
            &account.owner.namespace,
            GrantAuthorityFacts {
                owner_is_member: true,
                administrator_can_edit: true,
                owner_confirmed: true
            }
        )
        .is_err()
    );
    assert!(
        authorize_grant(
            &owner("openbot", "other-user"),
            &account,
            &account.owner.namespace,
            GrantAuthorityFacts {
                owner_is_member: true,
                administrator_can_edit: true,
                owner_confirmed: true
            }
        )
        .is_err()
    );
}
#[test]
fn vault_authority_deterministic_and_scheduled_steps_require_explicit_grant() {
    let account = account();
    let grant = grant(&account);
    let snapshot = snapshot(&grant);
    for kind in [
        InvocationKind::DeterministicHttp,
        InvocationKind::DeterministicMcp,
        InvocationKind::ModelTool,
        InvocationKind::DirectTool,
        InvocationKind::SealedSample,
    ] {
        let mut scoped_grant = grant.clone();
        scoped_grant.purpose = kind;
        assert_eq!(
            authorize_dispatch(
                &account,
                &scoped_grant,
                &snapshot,
                HostAuthorityFacts { kind, ..facts() }
            )
            .is_ok(),
            matches!(
                kind,
                InvocationKind::DeterministicHttp | InvocationKind::DeterministicMcp
            )
        );
    }
    assert!(
        authorize_dispatch(
            &account,
            &grant,
            &snapshot,
            HostAuthorityFacts {
                server_created_provenance: false,
                ..facts()
            }
        )
        .is_err()
    );
}
#[test]
fn vault_authority_stale_status_binding_namespace_and_versions_deny() {
    let account = account();
    let grant = grant(&account);
    let expected = snapshot(&grant);
    for status in [
        AccountStatus::Disconnected,
        AccountStatus::ReconnectRequired,
    ] {
        let mut a = account.clone();
        a.status = status;
        assert!(authorize_dispatch(&a, &grant, &expected, facts()).is_err());
    }
    let mut a = account.clone();
    a.authorization_revision = a.authorization_revision.next().unwrap();
    assert!(authorize_dispatch(&a, &grant, &expected, facts()).is_err());
    for changed in 0..7 {
        let mut s = expected.clone();
        match changed {
            0 => s.client = OwnerNamespace::parse("foreign").unwrap(),
            1 => s.account = AccountId::new(Uuid::new_v4()),
            2 => s.grant = UseGrantId::new(Uuid::new_v4()),
            3 => s.binding = HostBinding::parse("other").unwrap(),
            4 => s.app = AppId::parse("calendar").unwrap(),
            5 => s.destination = ApprovedDestination::parse("https://foreign.invalid/").unwrap(),
            _ => s.grant_revision = s.grant_revision.next().unwrap(),
        };
        assert!(authorize_dispatch(&account, &grant, &s, facts()).is_err());
    }
    for host in [
        HostAuthorityFacts {
            owner_is_member: false,
            ..facts()
        },
        HostAuthorityFacts {
            connection_enabled: false,
            ..facts()
        },
        HostAuthorityFacts {
            action_authorized: false,
            ..facts()
        },
        HostAuthorityFacts {
            connection_revision: ConnectionRevision::INITIAL.next().unwrap(),
            ..facts()
        },
        HostAuthorityFacts {
            catalog_revision: CatalogRevision::INITIAL.next().unwrap(),
            ..facts()
        },
    ] {
        assert!(authorize_dispatch(&account, &grant, &expected, host).is_err());
    }
}
#[tokio::test]
async fn vault_authority_host_port_denial_propagates() {
    struct Deny;
    struct DenyGate;
    #[async_trait::async_trait]
    impl CredentialDispatchGate for DenyGate {
        async fn check(
            &self,
            _: &AuthorizedInvocation,
            _: HostAuthorityFacts,
        ) -> IntegrationResult<()> {
            Err(IntegrationError::Denied)
        }
    }
    #[async_trait::async_trait]
    impl DispatchAuthority for Deny {
        async fn check(
            &self,
            _: &AuthorizedInvocation,
            _: &dyn CredentialDispatchGate,
        ) -> IntegrationResult<()> {
            Err(IntegrationError::Denied)
        }
    }
    let account = account();
    let grant = grant(&account);
    let invocation = AuthorizedInvocation {
        authorization: snapshot(&grant),
        operation: OperationId::parse("messages.read").unwrap(),
        target: OperationTarget::Http,
        arguments: OperationArguments::new(json!({})).unwrap(),
        request_digest: [0; 32],
        correlation: CorrelationId::parse("correlation").unwrap(),
        dispatch: DispatchId::new(Uuid::new_v4()),
        deadline: Utc::now(),
        cancellation: CancellationId::parse("cancel").unwrap(),
        host_authority: HostAuthorityReference::parse("opaque").unwrap(),
    };
    assert!(matches!(
        Deny.check(&invocation, &DenyGate).await,
        Err(IntegrationError::Denied)
    ));
    let wire = serde_json::to_string(&invocation).unwrap();
    assert!(!wire.contains("token"));
    assert!(!wire.contains("envelope"));
}
