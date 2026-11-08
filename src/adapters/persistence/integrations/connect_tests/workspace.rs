use super::*;
use crate::application::integrations::private::CredentialAdapter;
struct WorkspaceAuthorization {
    adapter: Arc<dyn CredentialAdapter>,
}
#[async_trait::async_trait]
impl AuthorizationProvider for WorkspaceAuthorization {
    fn callback_error(&self, _: &str) -> ConnectStatus {
        ConnectStatus::Denied
    }
    fn authorization_url(
        &self,
        r: AuthorizationRequest<'_>,
    ) -> IntegrationResult<SanitizedConnectUrl> {
        let mut url = url::Url::parse("https://workspace.example/authorize").unwrap();
        url.query_pairs_mut()
            .append_pair("state", r.state)
            .append_pair("code_challenge", r.challenge);
        SanitizedConnectUrl::from_registered_endpoint(
            url.into(),
            "https://workspace.example/authorize",
        )
    }
    async fn exchange(&self, _: ProviderExchange<'_>) -> Result<SecretString, ConnectStatus> {
        Ok(SecretString::from(json!({"workspace":"installation-without-email","client":"same-client","token":"workspace-token-sentinel","generation":1,"permissions":["resource.read"]}).to_string()))
    }
    fn finalize(&self, r: FinalizeAuthorization<'_>) -> Result<AuthorizedAccount, ConnectStatus> {
        let mut value: Value = serde_json::from_str(r.candidate.expose_secret()).unwrap();
        value["generation"] = json!(r.generation.get());
        let payload = SecretString::from(value.to_string());
        self.adapter
            .validate_use(&payload, r.app)
            .map_err(|_| ConnectStatus::MissingPermissions)?;
        let identity = self
            .adapter
            .validate(&payload)
            .map_err(|_| ConnectStatus::IdentityMismatch)?
            .ok_or(ConnectStatus::IdentityMismatch)?;
        Ok(AuthorizedAccount {
            identity,
            payload,
            metadata: AccountMetadata::new(
                "Workspace installation".into(),
                Some("Granted resource.read".into()),
            )
            .unwrap(),
        })
    }
}
#[tokio::test]
async fn integration_connect_workspace_identity_needs_neither_email_nor_refresh_token() {
    let f = Box::pin(Fixture::new()).await;
    let mut registry = static_registry();
    let mut entry = super::super::tests::workspace_connect_entry();
    entry.authorization = Some(Arc::new(WorkspaceAuthorization {
        adapter: entry.adapter.clone(),
    }));
    registry.register(entry).unwrap();
    let vault = PostgresVault::with_registry(f.vault.persistence.clone(), Arc::new(registry));
    let service = f.service_on(vault.clone());
    let start = service
        .begin_connect(
            &f.owner,
            BeginConnect {
                app: AppId::parse("workspace-a").unwrap(),
                config: AuthConfigId::parse("workspace-a").unwrap(),
                requested_capabilities: RequestedCapabilities::try_from(vec![
                    Capability::parse("resource.read").unwrap(),
                ])
                .unwrap(),
                return_target: SafeReturnTarget::parse("/ui/profile/vault".into()).unwrap(),
                reconnect: None,
            },
        )
        .await
        .unwrap();
    let url = url::Url::parse(start.authorization_url.as_str()).unwrap();
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let done = service
        .finish_connect(
            &f.owner,
            ConnectCallback {
                provider: ProviderId::parse("workspace-fixture").unwrap(),
                state,
                code: Some(SecretString::from("workspace-code")),
                error: None,
            },
            work(CancellationToken::new()),
        )
        .await
        .unwrap();
    assert_eq!(done.status, ConnectStatus::Connected);
    let account = vault
        .get(f.owner.owner(), done.account.unwrap())
        .await
        .unwrap();
    assert_eq!(
        account.external_identity.as_ref().unwrap().subject.as_str(),
        "installation-without-email"
    );
    let raw = vault
        .read_usable(f.owner.owner(), account.id)
        .await
        .unwrap();
    let payload: Value = serde_json::from_str(raw.payload.expose_secret()).unwrap();
    assert!(payload.get("email").is_none());
    assert!(payload.get("refresh_token").is_none());
}
