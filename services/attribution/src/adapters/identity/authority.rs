use crate::{application::identity::*, infra::identity::IdentityConfig};
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use std::{sync::Arc, time::Duration};
// 100 legal company names can exceed 64 KiB once encoded as JSON. The contract-derived
// maximum-page test fails at 80% of this hard cap, before growth silently consumes it.
pub(super) const MAX_RESPONSE: usize = 128 * 1024;
pub struct HttpAuthority {
    config: Arc<IdentityConfig>,
    client: Client,
}
impl HttpAuthority {
    pub fn new(config: Arc<IdentityConfig>) -> anyhow::Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(2))
            .pool_max_idle_per_host(4)
            .build()?;
        Ok(Self { config, client })
    }
    fn url(&self, path: &str) -> String {
        format!("{}api/attribution/v1/{path}", self.config.openbot_origin)
    }
    fn bearer(&self, path: &str, grant: &IdentityGrant) -> RequestBuilder {
        self.client.get(self.url(path)).bearer_auth(&grant.0)
    }
    async fn json<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        let mut response = request.send().await.map_err(|_| Failure::Unavailable)?;
        match response.status() {
            StatusCode::OK => (),
            StatusCode::UNAUTHORIZED => return Err(Failure::Unauthorized),
            StatusCode::FORBIDDEN => return Err(Failure::Forbidden),
            StatusCode::NOT_FOUND => return Err(Failure::NotFound),
            StatusCode::CONFLICT => return Err(Failure::Conflict),
            _ => return Err(Failure::Unavailable),
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE as u64)
        {
            return Err(Failure::Unavailable);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Failure::Unavailable)? {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err(Failure::Unavailable);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Failure::Unavailable)
    }
}
#[async_trait]
impl Authority for HttpAuthority {
    async fn exchange(&self, code: &Code, verifier: &PkceVerifier) -> Result<IdentityGrant> {
        let response: GrantResponse = self
            .json(
                self.client
                    .post(self.url("token"))
                    .basic_auth("attribution", Some(&self.config.client_secret))
                    .form(&[
                        ("grant_type", "authorization_code"),
                        ("code", &code.0),
                        (
                            "redirect_uri",
                            &format!("{}auth/callback", self.config.public_origin),
                        ),
                        ("code_verifier", &verifier.0),
                    ]),
            )
            .await?;
        if response.token_type != "Bearer"
            || response.access_token.len() < 43
            || response.access_token.len() > 128
            || !(1..=28800).contains(&response.expires_in)
        {
            return Err(Failure::Unavailable);
        }
        Ok(IdentityGrant(response.access_token))
    }
    async fn identity(&self, grant: &IdentityGrant) -> Result<Identity> {
        let identity: Identity = self.json(self.bearer("identity", grant)).await?;
        if identity.user_id.0.is_nil()
            || identity.display_name.is_empty()
            || identity.display_name.chars().count() > 120
        {
            return Err(Failure::Unavailable);
        }
        Ok(identity)
    }
    async fn companies(
        &self,
        grant: &IdentityGrant,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<CompanyPage> {
        if !(1..=100).contains(&limit) || cursor.is_some_and(|v| v.len() > 512) {
            return Err(Failure::Invalid);
        }
        let mut request = self.bearer("companies", grant).query(&[("limit", limit)]);
        if let Some(cursor) = cursor {
            request = request.query(&[("cursor", cursor)]);
        }
        let page: CompanyPage = self.json(request).await?;
        if page.items.len() > usize::from(limit)
            || page
                .next_cursor
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 512)
        {
            return Err(Failure::Unavailable);
        }
        for company in &page.items {
            company.validate(company.company_id)?;
        }
        Ok(page)
    }
    async fn access(&self, grant: &IdentityGrant, company: CompanyId) -> Result<CompanyAccess> {
        self.json(self.bearer(&format!("companies/{}/access", company.0), grant))
            .await
    }
    async fn status(&self, company: CompanyId) -> Result<CompanyStatus> {
        self.json(
            self.client
                .get(self.url(&format!("companies/{}/status", company.0)))
                .basic_auth("attribution", Some(&self.config.client_secret)),
        )
        .await
    }
    async fn revoke(&self, grant: &IdentityGrant) -> Result<()> {
        let response = self
            .client
            .post(self.url("revoke"))
            .bearer_auth(&grant.0)
            .send()
            .await
            .map_err(|_| Failure::Unavailable)?;
        if response.status() == StatusCode::NO_CONTENT {
            Ok(())
        } else {
            Err(Failure::Unavailable)
        }
    }
}
