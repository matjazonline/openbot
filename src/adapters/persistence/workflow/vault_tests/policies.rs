use super::*;
#[tokio::test]
async fn vault_dispatch_authority_policy_changes_require_fresh_owner_consent() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "policy",
        "secret",
    )
    .await;
    attach(&setup, &vault, &account).await;
    let (invocation, context) = prepared(&setup).await;
    vault
        .final_dispatch_guard(&invocation, &context)
        .await
        .unwrap();
    for change in 0..3 {
        assert!(
            changed_policy(&vault, change)
                .final_dispatch_guard(&invocation, &context)
                .await
                .is_err()
        );
    }
}
#[tokio::test]
async fn vault_binding_old_proposal_cannot_confirm_changed_app_policy_or_destination() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "proposal-policy",
        "secret",
    )
    .await;
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
    let changed = PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: changed_policy(&vault, 2),
    };
    assert!(
        changed
            .confirm(
                owner(setup.fixture.target.actor.user_id()),
                proposal.id,
                account.id,
                account.authorization_revision
            )
            .await
            .is_err()
    );
    assert!(
        vault
            .grants(&account.owner, account.id, None)
            .await
            .unwrap()
            .grants
            .is_empty()
    );
    assert!(sqlx::query("UPDATE workflow_http_connections SET origin='https://other.example.test',revision=revision+1 WHERE id=$1").bind(setup.connection.id).execute(p.pool()).await.is_err());
}
#[tokio::test]
async fn vault_dispatch_authority_direct_http_tools_and_foreign_binding_cannot_mint_context() {
    use crate::application::agent::ToolCallOrigin;
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "direct",
        "secret",
    )
    .await;
    attach(&setup, &vault, &account).await;
    let step = p
        .http_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let call = HttpToolCall {
        origin: ToolCallOrigin::Workflow,
        connection: setup.connection.id,
        operation: step.operation_key.clone(),
        arguments: step.arguments.clone(),
        call: ModelToolCallId::parse("direct-call").unwrap(),
    };
    assert!(p.http_tool_request(step.scope, &call).await.is_err());
    let foreign = vault
        .create_confirmed(&consent(&account, 33), confirmed())
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_http_connections SET integration_grant_id=$2,integration_grant_revision=$3,revision=revision+1 WHERE id=$1")
        .bind(setup.connection.id).bind(foreign.id.as_uuid()).bind(foreign.revision.get()).execute(p.pool()).await.unwrap();
    assert!(
        p.http_step_request(&setup.claim, &setup.kind)
            .await
            .is_err()
    );
}
