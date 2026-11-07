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
pub(super) fn gateway(
    p: &PostgresPersistence,
    c: &AppConfig,
) -> anyhow::Result<(Arc<dyn IntegrationGateway>, Arc<dyn IntegrationBindings>)> {
    let registry = Arc::new(configured_registry(c, GoogleOAuthConfig::from_env())?);
    let vault = PostgresVault::with_registry(p.clone(), registry.clone());
    let bindings = Arc::new(PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    });
    let connect = Arc::new(ConnectService {
        registry,
        store: Arc::new(vault.clone()),
        codec: Arc::new(SignedConnectState::new(&c.jwt_secret)),
        authority: Arc::new(PostgresConnectAuthority {
            persistence: p.clone(),
        }),
    });
    Ok((vault.gateway(bindings.clone(), connect), bindings))
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
