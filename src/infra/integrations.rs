use crate::{
    adapters::{
        integrations::{google_http::GoogleHttp, oauth_state::SignedConnectState},
        persistence::{
            PostgresPersistence, integration_bindings::PostgresIntegrationBindings,
            integration_connect_authority::PostgresConnectAuthority, integrations::PostgresVault,
        },
    },
    application::{
        integrations::{
            connect::ConnectService,
            registry::{CodeOwnedRegistry, static_registry},
            *,
        },
        use_cases::integration_binding::IntegrationBindings,
    },
    domain::integrations::*,
    infra::config::{AppConfig, GoogleOAuthConfig},
};
use secrecy::SecretString;
use std::sync::Arc;
pub(super) struct IntegrationServices {
    pub gateway: Arc<dyn IntegrationGateway>,
    pub bindings: Arc<dyn IntegrationBindings>,
    pub discovery: Arc<dyn crate::application::use_cases::mcp::McpIntegrationDiscovery>,
}
pub(super) fn gateway(
    p: &PostgresPersistence,
    c: &AppConfig,
    mcp: Arc<dyn crate::application::services::mcp_client::McpClient>,
    memory_url: Option<&str>,
) -> anyhow::Result<IntegrationServices> {
    // No integration call may reach the memory service either (phase 05 E5).
    let http = match memory_url {
        Some(url) => {
            crate::adapters::guarded_http::HttpDestinationPolicy::default().reserving(url)?
        }
        None => crate::adapters::guarded_http::HttpDestinationPolicy::default(),
    };
    let registry = Arc::new(configured_registry(c, GoogleOAuthConfig::from_env())?);
    let vault = PostgresVault::with_registry(p.clone(), registry.clone());
    let bindings = Arc::new(PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    });
    let connect = Arc::new(ConnectService {
        registry: registry.clone(),
        store: Arc::new(vault.clone()),
        codec: Arc::new(SignedConnectState::new(&c.jwt_secret)),
        authority: Arc::new(PostgresConnectAuthority {
            persistence: p.clone(),
        }),
    });
    let execution = Arc::new(
        crate::application::integrations::execute::AuthenticatedExecution {
            registry,
            credentials: Arc::new(vault.clone()),
            transports: Arc::new(
                crate::adapters::integrations::execution::IntegrationTransports { http, mcp },
            ),
        },
    );
    let gateway = vault.gateway(bindings.clone(), connect, execution);
    let discovery = Arc::new(crate::adapters::persistence::PostgresMcpDiscovery {
        vault,
        gateway: gateway.clone(),
    });
    Ok(IntegrationServices {
        gateway,
        bindings,
        discovery,
    })
}
pub(crate) fn configured_registry(
    c: &AppConfig,
    google: Option<GoogleOAuthConfig>,
) -> anyhow::Result<CodeOwnedRegistry> {
    let mut registry = static_registry();
    if let Some(google) = google {
        let domain = c.app_domain_name.trim_end_matches('/');
        let base = if domain.contains("://") {
            domain.to_owned()
        } else {
            format!(
                "{}://{domain}",
                if domain.starts_with("localhost") || domain.starts_with("127.0.0.1") {
                    "http"
                } else {
                    "https"
                }
            )
        };
        let redirect = url::Url::parse(&format!("{base}/auth/integrations/google/callback"))?;
        anyhow::ensure!(
            (redirect.scheme() == "https"
                || (redirect.scheme() == "http"
                    && matches!(
                        redirect.host_str(),
                        Some("localhost" | "127.0.0.1" | "[::1]")
                    )))
                && redirect.username().is_empty()
                && redirect.password().is_none()
                && redirect.query().is_none()
                && redirect.fragment().is_none(),
            "Invalid integration callback base URL"
        );
        registry.register(
            crate::adapters::integrations::google_authorization::registration(
                ExternalSubject::parse(google.client_id)?,
                SecretString::from(google.client_secret),
                redirect,
                Arc::new(
                    GoogleHttp::new()
                        .map_err(|_| anyhow::anyhow!("Google integration client unavailable"))?,
                ),
            )?,
        )?;
    }
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integration_portability_production_composition_registers_no_test_provider_or_app() {
        let config = AppConfig::for_test();
        for google in [
            None,
            Some(GoogleOAuthConfig {
                client_id: "production-composition-fixture-client".into(),
                client_secret: "not-a-live-secret".into(),
            }),
        ] {
            let enabled = google.is_some();
            let registry = configured_registry(&config, google).unwrap();
            assert!(
                registry
                    .get(&AppId::parse("static-bearer").unwrap())
                    .is_ok()
            );
            assert!(
                registry
                    .get(&AppId::parse("static-api-key").unwrap())
                    .is_ok()
            );
            assert_eq!(
                registry.get(&AppId::parse("gmail").unwrap()).is_ok(),
                enabled
            );
            for fixture in [
                "portable-key-fixture",
                "workspace-reader",
                "workspace-writer",
                "workspace-other-client",
                "workspace-strict",
                "gmail-mcp-fixture",
            ] {
                assert!(
                    registry.get(&AppId::parse(fixture).unwrap()).is_err(),
                    "{fixture}"
                );
            }
            for app in registry.binding_apps() {
                assert!(matches!(
                    app.provider.as_str(),
                    "static-bearer" | "static-api-key" | "google"
                ));
            }
        }
    }
}
