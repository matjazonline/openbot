use super::{private::*, registry::*, *};
use crate::domain::integrations::{authority::*, *};
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

fn owner(namespace: &str, subject: &str) -> AccountOwner {
    AccountOwner {
        namespace: OwnerNamespace::parse(namespace).unwrap(),
        subject: OwnerSubject::parse(subject).unwrap(),
    }
}
fn account() -> ConnectedAccount {
    ConnectedAccount {
        id: AccountId::new(Uuid::new_v4()),
        owner: owner("openbot", "owner"),
        key: AccountKey::parse("google/personal").unwrap(),
        provider_id: ProviderId::parse("google").unwrap(),
        auth_config_id: AuthConfigId::parse("google-production").unwrap(),
        auth_method: AuthMethod::OAuthAuthorizationCode,
        credential_schema: CredentialSchema::parse("google-v1").unwrap(),
        external_identity: None,
        revision: AccountRevision::INITIAL,
        authorization_revision: AuthorizationRevision::INITIAL,
        status: AccountStatus::Ready,
        metadata: AccountMetadata::new("Personal".into(), None).unwrap(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}
fn grant(account: &ConnectedAccount) -> UseGrant {
    UseGrant {
        id: UseGrantId::new(Uuid::new_v4()),
        account: account.id,
        owner: account.owner.clone(),
        client: account.owner.namespace.clone(),
        binding: HostBinding::parse("company/connection").unwrap(),
        host_scope: HostBinding::parse("company").unwrap(),
        purpose: InvocationKind::DeterministicHttp,
        app: AppId::parse("gmail").unwrap(),
        app_policy: AppPolicyDigest::parse("0".repeat(64)).unwrap(),
        destination: ApprovedDestination::parse("https://gmail.googleapis.com/").unwrap(),
        account_authorization: account.authorization_revision,
        revision: GrantRevision::INITIAL,
        enabled: true,
    }
}
fn snapshot(grant: &UseGrant) -> AuthorizationSnapshot {
    AuthorizationSnapshot {
        account: grant.account,
        grant: grant.id,
        client: grant.client.clone(),
        binding: grant.binding.clone(),
        app: grant.app.clone(),
        app_policy: grant.app_policy.clone(),
        destination: grant.destination.clone(),
        account_authorization: grant.account_authorization,
        grant_revision: grant.revision,
        connection_revision: ConnectionRevision::INITIAL,
        catalog_revision: CatalogRevision::INITIAL,
    }
}
fn facts() -> HostAuthorityFacts {
    HostAuthorityFacts {
        owner_is_member: true,
        connection_enabled: true,
        action_authorized: true,
        server_created_provenance: true,
        kind: InvocationKind::DeterministicHttp,
        connection_revision: ConnectionRevision::INITIAL,
        catalog_revision: CatalogRevision::INITIAL,
    }
}
#[test]
fn vault_contract_identifier_and_page_bounds_survive_deserialization() {
    for size in [0, 129] {
        assert!(AccountKey::parse("k".repeat(size)).is_err());
    }
    assert!(AccountKey::parse("k".repeat(128)).is_ok());
    assert!(OwnerSubject::parse("é".repeat(129)).is_err());
    assert!(OwnerSubject::parse("é".repeat(128)).is_ok());
    assert!(serde_json::from_value::<ProviderId>(json!("x".repeat(129))).is_err());
    assert!(serde_json::from_value::<AccountRevision>(json!(0)).is_err());
    assert!(PageRequest::new(None, 0).is_err());
    assert!(PageRequest::new(None, 51).is_err());
    assert!(PageRequest::new(None, 50).is_ok());
    assert_eq!((MAX_ACCOUNTS, MAX_GRANTS), (128, 128));
}
#[test]
fn vault_contract_metadata_is_bounded_and_has_no_credential_fields() {
    let metadata =
        AccountMetadata::new("Workspace".into(), Some("no email required".into())).unwrap();
    let serialized = serde_json::to_string(&metadata).unwrap();
    assert!(!serialized.contains("token"));
    assert!(
        serde_json::from_value::<AccountMetadata>(
            json!({"label":"ok", "description":null,"token":"sentinel"})
        )
        .is_err()
    );
    assert!(AccountMetadata::new("a".into(), Some("x".repeat(4096))).is_err());
    assert!(AccountMetadata::new("x".repeat(257), None).is_err());
    let output = serde_json::to_string(&account()).unwrap();
    assert!(!output.contains("envelope"));
    assert!(!output.contains("refresh_token"));
}
#[test]
fn vault_contract_revision_table_separates_refresh_rewrap_and_authorization() {
    let initial = CredentialVersions::INITIAL;
    assert_eq!(initial.changed(CredentialChange::Rewrap).unwrap(), initial);
    let refresh = initial.changed(CredentialChange::Refresh).unwrap();
    assert_eq!(refresh.authorization, initial.authorization);
    assert_eq!(refresh.revision.get(), 2);
    assert_eq!(refresh.generation.get(), 2);
    for change in [
        CredentialChange::Replace,
        CredentialChange::Reconnect,
        CredentialChange::Disconnect,
    ] {
        let next = initial.changed(change).unwrap();
        assert_eq!(next.authorization.get(), 2);
        assert_eq!(next.revision.get(), 2);
        assert_eq!(next.generation.get(), 2);
    }
    assert!(AccountRevision::new(i64::MAX).unwrap().next().is_err());
    assert_eq!(GrantRevision::INITIAL.next().unwrap().get(), 2);
}
#[test]
fn integration_contract_static_payloads_require_registered_shape_and_injection() {
    let bearer = StaticCredentialAdapter::bearer();
    let header = StaticCredentialAdapter::api_key_header("X-API-Key").unwrap();
    for bad in [
        "{}",
        "[]",
        r#"{"token":42}"#,
        r#"{"token":"ok","extra":true}"#,
        "line\nbreak",
    ] {
        assert!(bearer.validate(&SecretString::from(bad)).is_err());
    }
    let secret = SecretString::from(r#"{"token":"sentinel"}"#);
    let injection = bearer.injection(&secret).unwrap();
    assert!(matches!(injection.method, InjectionMethod::Bearer));
    assert_eq!(injection.value.expose_secret(), "sentinel");
    assert!(header.validate(&secret).is_err());
    assert!(
        matches!(header.injection(&SecretString::from(r#"{"api_key":"sentinel"}"#)).unwrap().method, InjectionMethod::ApiKeyHeader(name) if name == "X-API-Key")
    );
    for name in ["Authorization", "Host", "Cookie", "X-Injected\r\n"] {
        assert!(StaticCredentialAdapter::api_key_header(name).is_err());
    }
    assert!(!format!("{secret:?}").contains("sentinel"));
}
#[test]
fn vault_contract_static_credential_bound_is_aggregate_and_inclusive() {
    let registry = static_registry();
    let app = AppId::parse("static-bearer").unwrap();
    let schema = CredentialSchema::parse("static-v1").unwrap();
    assert!(
        registry
            .validate(
                &app,
                AuthMethod::StaticSecret,
                &schema,
                &SecretString::from("x".repeat(MAX_CREDENTIAL_BYTES))
            )
            .is_ok()
    );
    assert!(
        registry
            .validate(
                &app,
                AuthMethod::StaticSecret,
                &schema,
                &SecretString::from("x".repeat(MAX_CREDENTIAL_BYTES + 1))
            )
            .is_err()
    );
}
fn google_payload() -> serde_json::Value {
    json!({"subject":"google-subject", "mailbox":"mailbox@gmail.com", "client":"client-identity", "scopes":["mail.read"], "refresh_token":"sentinel-refresh", "access_token":"sentinel-access",
        "expires_at":"2030-01-01T00:00:00Z", "generation":1, "refresh_retry_at":null})
}
#[test]
fn integration_contract_managed_google_is_typed_and_shared_between_apps() {
    let adapter = crate::adapters::integrations::google_credentials::GoogleCredentialAdapter {
        client: ExternalSubject::parse("client-identity").unwrap(),
    };
    let raw = SecretString::from(google_payload().to_string());
    let identity = adapter.validate(&raw).unwrap().unwrap();
    assert_eq!(identity.subject.as_str(), "google-subject");
    let mut registry = CodeOwnedRegistry::default();
    for app in ["gmail", "calendar-fixture"] {
        registry
            .register(Registration {
                authorization: None,
                renewal: crate::application::integrations::renewal::RenewalStrategy::NoRenewal,
                provider: AuthProvider {
                    id: ProviderId::parse("google").unwrap(),
                    method: AuthMethod::OAuthAuthorizationCode,
                    schema: CredentialSchema::parse("google-v1").unwrap(),
                },
                config: AuthConfig {
                    id: AuthConfigId::parse("google-client").unwrap(),
                    provider: ProviderId::parse("google").unwrap(),
                    client_identity: ExternalSubject::parse("client-identity").unwrap(),
                    client_secret_reference: Some(
                        ClientSecretReference::parse("google-client-secret").unwrap(),
                    ),
                },
                app: AppDefinition {
                    id: AppId::parse(app).unwrap(),
                    provider: ProviderId::parse("google").unwrap(),
                    config: AuthConfigId::parse("google-client").unwrap(),
                    destinations: vec![],
                    capabilities: vec![],
                },
                adapter: Arc::new(
                    crate::adapters::integrations::google_credentials::GoogleCredentialAdapter {
                        client: ExternalSubject::parse("client-identity").unwrap(),
                    },
                ),
            })
            .unwrap();
        assert_eq!(
            registry
                .validate(
                    &AppId::parse(app).unwrap(),
                    AuthMethod::OAuthAuthorizationCode,
                    &CredentialSchema::parse("google-v1").unwrap(),
                    &raw
                )
                .unwrap()
                .unwrap(),
            identity
        );
    }
    for mutation in ["client", "generation", "refresh_token", "unknown"] {
        let mut value = google_payload();
        value[mutation] = json!("");
        assert!(
            adapter
                .validate(&SecretString::from(value.to_string()))
                .is_err()
        );
    }
}
#[test]
fn vault_contract_managed_scope_and_aggregate_bounds() {
    let adapter = crate::adapters::integrations::google_credentials::GoogleCredentialAdapter {
        client: ExternalSubject::parse("client-identity").unwrap(),
    };
    for scopes in [
        vec!["s".to_string(); 65],
        vec!["x".repeat(257)],
        vec!["x".repeat(256); 64],
    ] {
        let mut payload = google_payload();
        payload["scopes"] = json!(scopes);
        assert!(
            adapter
                .validate(&SecretString::from(payload.to_string()))
                .is_err()
        );
    }
    let mut payload = google_payload();
    payload["scopes"] = json!(vec!["s"; 64]);
    assert!(
        adapter
            .validate(&SecretString::from(payload.to_string()))
            .is_ok()
    );
}
#[test]
fn integration_contract_unknown_registry_and_method_schema_mismatch_fail_closed() {
    let registry = static_registry();
    let schema = CredentialSchema::parse("static-v1").unwrap();
    let secret = SecretString::from("sentinel");
    assert!(registry.get(&AppId::parse("unknown").unwrap()).is_err());
    assert!(
        registry
            .validate(
                &AppId::parse("static-bearer").unwrap(),
                AuthMethod::OAuthAuthorizationCode,
                &schema,
                &secret
            )
            .is_err()
    );
    assert!(
        registry
            .validate(
                &AppId::parse("static-bearer").unwrap(),
                AuthMethod::StaticSecret,
                &CredentialSchema::parse("managed-v1").unwrap(),
                &secret
            )
            .is_err()
    );
    let identity = ExternalIdentity {
        kind: ExternalIdentityKind::parse("workspace").unwrap(),
        subject: ExternalSubject::parse("workspace-123").unwrap(),
        authorization_key: AuthorizationKey::parse("workspace-123/client").unwrap(),
    };
    assert!(!serde_json::to_string(&identity).unwrap().contains("email"));
}
#[test]
fn integration_contract_gateway_arguments_and_safe_return_are_bounded() {
    for value in [
        json!({"headers":{"Authorization":"sentinel"}}),
        json!({"nested":[{"injection":"bearer"}]}),
        json!({"body":"x".repeat(16384)}),
    ] {
        assert!(OperationArguments::new(value).is_err());
    }
    assert!(
        OperationArguments::new(json!({"headers":{"Accept":"application/json"},"body":"hello"}))
            .is_ok()
    );
    for target in [
        "https://foreign.invalid/",
        "//foreign.invalid",
        "/ui/%2f%2fforeign",
        "/ui/\\foreign",
        "/ui/profile?secret=sentinel",
    ] {
        assert!(SafeReturnTarget::parse(target.into()).is_err());
    }
    assert!(SafeReturnTarget::parse("/ui/profile/vault".into()).is_ok());
    assert!(RequestedCapabilities::try_from(vec![Capability::parse("read").unwrap(); 65]).is_err());
    assert!(
        SanitizedConnectUrl::from_registered_endpoint(
            "https://auth.example/authorize?client_secret=sentinel".into(),
            "https://auth.example/authorize"
        )
        .is_err()
    );
    assert!(
        SanitizedConnectUrl::from_registered_endpoint(
            "https://foreign.example/authorize".into(),
            "https://auth.example/authorize"
        )
        .is_err()
    );
    assert!(
        SanitizedConnectUrl::from_registered_endpoint(
            "https://auth.example/authorize?state=opaque".into(),
            "https://auth.example/authorize"
        )
        .is_ok()
    );
}
#[path = "authority_tests.rs"]
mod authority_tests;

#[test]
fn integration_contract_static_json_whitespace_cannot_bypass_shape_validation() {
    let adapter = StaticCredentialAdapter::bearer();
    for prefix in [" ", "\t", "\n", " \t\r\n"] {
        for invalid in [
            r#"{"token":42}"#,
            r#"{"token":"value","extra":true}"#,
            "[]",
            "{}",
        ] {
            assert!(
                adapter
                    .validate(&SecretString::from(format!("{prefix}{invalid}")))
                    .is_err()
            );
        }
        let good = SecretString::from(format!("{prefix}{}", r#"{"token":"value"}"#));
        assert!(adapter.injection(&good).unwrap().value.expose_secret() == "value");
    }
}
#[test]
fn integration_contract_operation_headers_cannot_override_adapter_credentials() {
    for name in [
        "X-API-Key",
        "x-api-key",
        "X-Custom-Auth",
        "Authorization",
        "Host",
        "Cookie",
    ] {
        assert!(OperationArguments::new(json!({"headers":{name:"sentinel"}})).is_err());
    }
    assert!(
        OperationArguments::new(
            json!({"headers":{"Accept":"application/json","If-Match":"version"}})
        )
        .is_ok()
    );
    assert!(OperationArguments::new(json!({"headers":{"Accept":"safe\r\ninjected"}})).is_err());
    assert!(StaticCredentialAdapter::api_key_header("Accept").is_err());
}
