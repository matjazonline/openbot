use super::*;
#[tokio::test]
async fn vault_binding_owner_confirmation_and_admin_cannot_use_another_owners_account() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let user = member(&setup).await;
    let (vault, account) = account(&setup, user, "personal", "sentinel-private").await;
    let p = &setup.fixture.persistence;
    let host = PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    };
    let proposal = host
        .propose(
            setup.fixture.target.actor.user_id(),
            ProposeBinding {
                company: setup.fixture.target.company.as_uuid(),
                connection: setup.connection.id,
                kind: BindingKind::Http,
                app: integration::AppId::parse("static-bearer").unwrap(),
                expected_revision: setup.connection.revision,
            },
        )
        .await
        .unwrap();
    assert!(
        host.confirm(
            owner(setup.fixture.target.actor.user_id()),
            proposal.id,
            account.id,
            account.authorization_revision
        )
        .await
        .is_err()
    );
    let grant = host
        .confirm(
            owner(user),
            proposal.id,
            account.id,
            account.authorization_revision,
        )
        .await
        .unwrap();
    assert_eq!(grant.owner, owner_for_user(user));
    assert!(
        host.confirm(
            owner(user),
            proposal.id,
            account.id,
            account.authorization_revision
        )
        .await
        .is_err()
    );
    let wire = serde_json::to_string(&grant).unwrap();
    assert!(!wire.contains("sentinel-private") && !wire.contains("enc:v2"));
    let current = p
        .list_http(setup.fixture.target.company, setup.fixture.target.actor)
        .await
        .unwrap()
        .remove(0);
    assert!(matches!(
        current.credential_source,
        CredentialSource::Integration(_)
    ));
    assert!(!current.secret_set);
    assert!(
        vault
            .grants(
                &owner_for_user(setup.fixture.target.actor.user_id()),
                account.id,
                None
            )
            .await
            .is_err()
    );
}
#[tokio::test]
async fn vault_binding_same_private_account_has_separate_http_and_mcp_grants() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "shared",
        "secret",
    )
    .await;
    let http = attach(&setup, &vault, &account).await;
    let mcp = p
        .create_mcp_connection(
            setup.fixture.target.company.as_uuid(),
            McpConnectionWrite {
                slug: "vault-mcp".into(),
                endpoint: McpEndpoint::try_from("https://mcp.example.test/rpc".to_string())
                    .unwrap(),
                enabled: true,
                auth: McpAuth::None,
                discovered_tools: vec![],
                tool_grants: vec![],
            },
        )
        .await
        .unwrap();
    let host = PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    };
    let proposal = host
        .propose(
            setup.fixture.target.actor.user_id(),
            ProposeBinding {
                company: mcp.company_id,
                connection: mcp.id,
                kind: BindingKind::Mcp,
                app: integration::AppId::parse("static-bearer").unwrap(),
                expected_revision: mcp.revision,
            },
        )
        .await
        .unwrap();
    let other = host
        .confirm(
            owner(setup.fixture.target.actor.user_id()),
            proposal.id,
            account.id,
            account.authorization_revision,
        )
        .await
        .unwrap();
    assert_ne!(http.id, other.id);
    assert_eq!(other.account, http.account);
    let current = p
        .list_mcp_connections(mcp.company_id)
        .await
        .unwrap()
        .remove(0);
    assert!(matches!(current.auth, McpAuth::Integration(_)));
    assert!(
        p.mcp_token(mcp.company_id, mcp.id, current.revision)
            .await
            .is_err()
    );
    let inspection = vault
        .grants(&account.owner, account.id, None)
        .await
        .unwrap();
    assert_eq!(inspection.grants.len(), 2);
}

#[tokio::test]
async fn vault_binding_cap_competing_creators_reconfirmation_and_pagination() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "cap",
        "secret",
    )
    .await;
    for number in 0..127 {
        vault
            .create_confirmed(&consent(&account, number), confirmed())
            .await
            .unwrap();
    }
    let left = consent(&account, 127);
    let right = consent(&account, 128);
    let (a, b) = tokio::join!(
        vault.create_confirmed(&left, confirmed()),
        vault.create_confirmed(&right, confirmed())
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert!(matches!(
        a.as_ref().err().or(b.as_ref().err()),
        Some(integration::IntegrationError::Limit)
    ));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM integration_use_grants WHERE account_id=$1")
            .bind(account.id.as_uuid())
            .fetch_one(setup.fixture.persistence.pool())
            .await
            .unwrap();
    assert_eq!(count, 128);
    let first = vault
        .grants(&account.owner, account.id, None)
        .await
        .unwrap();
    assert_eq!(first.grants.len(), 50);
    let second = vault
        .grants(&account.owner, account.id, first.next)
        .await
        .unwrap();
    assert_eq!(second.grants.len(), 50);
    let third = vault
        .grants(&account.owner, account.id, second.next)
        .await
        .unwrap();
    assert_eq!(third.grants.len(), 28);
    assert!(third.next.is_none());
    let old = vault
        .create_confirmed(&consent(&account, 0), confirmed())
        .await
        .unwrap();
    assert_eq!(old.revision.get(), 2);
}
#[tokio::test]
async fn vault_binding_missing_consent_destination_and_shape_denials_are_non_mutating() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "shape",
        &"x".repeat(4097),
    )
    .await;
    let mut facts = confirmed();
    facts.owner_confirmed = false;
    assert!(
        vault
            .create_confirmed(&consent(&account, 0), facts)
            .await
            .is_err()
    );
    assert!(
        vault
            .create_confirmed(&consent(&account, 0), confirmed())
            .await
            .is_err()
    );
    let mut c = consent(&account, 1);
    c.purpose = InvocationKind::DeterministicMcp;
    assert!(vault.create_confirmed(&c, confirmed()).await.is_ok());
    c.purpose = InvocationKind::ModelTool;
    assert!(vault.create_confirmed(&c, confirmed()).await.is_err());
    assert!(OperationArguments::new(json!({"headers":{"X-API-Key":"override"}})).is_err());
    assert!(ApprovedDestination::parse("https://api.example.test/?token=secret").is_err());
    assert!(OperationArguments::new(json!({"headers":{"Accept":"a","accept":"b"}})).is_err());
}
#[tokio::test]
async fn vault_binding_database_rejects_partial_sources_and_cross_owner_references() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "constraints",
        "secret",
    )
    .await;
    assert!(
        sqlx::query("UPDATE workflow_http_connections SET integration_account_id=$2 WHERE id=$1")
            .bind(setup.connection.id)
            .bind(account.id.as_uuid())
            .execute(p.pool())
            .await
            .is_err()
    );
    let grant = attach(&setup, &vault, &account).await;
    assert!(
        sqlx::query("UPDATE workflow_http_connections SET bearer=true WHERE id=$1")
            .bind(setup.connection.id)
            .execute(p.pool())
            .await
            .is_err()
    );
    assert!(sqlx::query("INSERT INTO integration_use_grants(id,account_id,owner_namespace,owner_subject,host_scope,host_binding,app_id,destination,purpose,account_authorization,app_policy,revision,enabled) VALUES($1,$2,'foreign','owner','scope','binding','static-bearer','https://api.example/','http',1,repeat('0',64),1,true)")
        .bind(Uuid::new_v4()).bind(account.id.as_uuid()).execute(p.pool()).await.is_err());
    vault
        .delete(&account.owner, account.id, account.authorization_revision)
        .await
        .unwrap();
    assert!(vault.grant_metadata(grant.id).await.is_err());
}
#[tokio::test]
async fn vault_binding_member_removal_readdition_repeated_teardown_and_owner_unbind() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let user = member(&setup).await;
    let (vault, account) = account(&setup, user, "membership", "secret").await;
    let grant = attach(&setup, &vault, &account).await;
    let (invocation, context) = prepared(&setup).await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company.as_uuid();
    p.remove_member(company, user).await.unwrap();
    assert!(
        vault
            .final_dispatch_guard(&invocation, &context)
            .await
            .is_err()
    );
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',0)").bind(Uuid::new_v4()).bind(company).bind(user).execute(p.pool()).await.unwrap();
    restore_principal(p, company, user).await;
    assert!(
        vault
            .final_dispatch_guard(&invocation, &context)
            .await
            .is_err()
    );
    vault.drain(16, &CancellationToken::new()).await.unwrap();
    assert!(!vault.grant_metadata(grant.id).await.unwrap().enabled);
    let current = attach(&setup, &vault, &account).await;
    p.remove_member(company, user).await.unwrap();
    vault
        .revoke(&account.owner, current.id, current.revision)
        .await
        .unwrap();
    vault.drain(16, &CancellationToken::new()).await.unwrap();
    assert!(!vault.grant_metadata(current.id).await.unwrap().enabled);
    assert!(vault.get(&account.owner, account.id).await.is_ok());
    assert_eq!(company_binding(company), grant.host_scope);
}

#[tokio::test]
async fn vault_binding_proposal_cap_competing_creators_and_expiry_pruning() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let host = PostgresIntegrationBindings::new(p.clone());
    let command = || ProposeBinding {
        company: setup.fixture.target.company.as_uuid(),
        connection: setup.connection.id,
        kind: BindingKind::Http,
        app: integration::AppId::parse("static-bearer").unwrap(),
        expected_revision: setup.connection.revision,
    };
    let actor = setup.fixture.target.actor.user_id();
    let mut first = None;
    for _ in 0..7 {
        let proposal = host.propose(actor, command()).await.unwrap();
        first.get_or_insert(proposal.id);
    }
    let (left, right) = tokio::join!(
        host.propose(actor, command()),
        host.propose(actor, command())
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM integration_binding_proposals WHERE company_id=$1",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(count, 8);
    let expired = first.unwrap();
    sqlx::query("UPDATE integration_binding_proposals SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(expired).execute(p.pool()).await.unwrap();
    host.propose(actor, command()).await.unwrap();
    assert!(host.proposal(&owner(actor), expired).await.is_err());
}

#[tokio::test]
async fn vault_binding_company_teardown_preserves_other_real_company_grants_and_private_account() {
    use crate::{
        adapters::persistence::test_support::insert_company_with_owner,
        use_cases::company::CompanyPersistence,
    };
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let user = setup.fixture.target.actor.user_id();
    let (vault, account) = account(&setup, user, "companies", "secret").await;
    let original = attach(&setup, &vault, &account).await;
    let company = Uuid::new_v4();
    insert_company_with_owner(
        p.pool(),
        company,
        "Other company",
        &format!("vault-other-{}", company.simple()),
        user,
    )
    .await;
    restore_principal(p, company, user).await;
    let connection = p
        .create_http(
            CompanyId::new(company),
            setup.fixture.target.actor,
            HttpConnectionWrite {
                origin: HttpOrigin::parse("https://other.example.test").unwrap(),
                bearer: false,
            },
        )
        .await
        .unwrap();
    let host = PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    };
    let proposal = host
        .propose(
            user,
            ProposeBinding {
                company,
                connection: connection.id,
                kind: BindingKind::Http,
                app: integration::AppId::parse("static-bearer").unwrap(),
                expected_revision: connection.revision,
            },
        )
        .await
        .unwrap();
    let other = host
        .confirm(
            owner(user),
            proposal.id,
            account.id,
            account.authorization_revision,
        )
        .await
        .unwrap();
    p.delete(setup.fixture.target.company.as_uuid())
        .await
        .unwrap();
    vault.drain(16, &CancellationToken::new()).await.unwrap();
    assert!(!vault.grant_metadata(original.id).await.unwrap().enabled);
    assert!(vault.grant_metadata(other.id).await.unwrap().enabled);
    assert_eq!(
        vault.get(&account.owner, account.id).await.unwrap().status,
        integration::AccountStatus::Ready
    );
    assert!(matches!(
        p.list_http(CompanyId::new(company), setup.fixture.target.actor)
            .await
            .unwrap()
            .remove(0)
            .credential_source,
        CredentialSource::Integration(_)
    ));
}
