//! One company's Resend account, in the three shapes the product needs it in.
//!
//! Resend is a per-tenant integration here, not a deployment one. The key that posts a company's
//! mail is the same key that fetches the mail its webhook announces, so both directions travel as
//! one record -- and a company without one simply has no Resend, rather than falling back to a
//! shared credential with which one tenant's mail could be read under another's account.
//!
//! The split between the types below is the point of the module: [`CompanyResendApiIntegration`] is
//! what a settings page may hold and therefore carries no secret at all, while
//! [`ResendApiInboundCredentials`] and [`ResendApiAccountCredentials`] are nothing *but* secrets and are
//! loaded through narrow lookups at the moment they are used.

use base64::Engine;
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::value_objects::{AuthservId, ResendApiWebhookToken};

/// The path every company's Resend webhook hangs off, with the token as its last segment.
///
/// Named once because two places have to agree on it: the router that serves the endpoint and the
/// settings page that tells an operator what to paste into a Resend dashboard. A URL shown but
/// not served is worse than no URL at all.
pub const RESEND_API_WEBHOOK_PATH: &str = "/webhooks/email/resend_api";

/// The prefix Svix writes on a signing secret. The key is what follows it, base64-decoded.
const SIGNING_SECRET_PREFIX: &str = "whsec_";

/// The signing key behind a stored secret, or `None` when the secret is not one.
///
/// Separate from verification so a settings form can refuse a mistyped secret where it is entered
/// rather than discovering it one 401 at a time, and so the base64 decode happens once per stored
/// value rather than once per request.
pub fn decode_signing_secret(secret: &str) -> Option<Vec<u8>> {
    let encoded = secret
        .trim()
        .strip_prefix(SIGNING_SECRET_PREFIX)
        .unwrap_or(secret.trim());
    if encoded.is_empty() {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()
        .filter(|key| !key.is_empty())
}

/// Which of a company's two Resend secrets is being addressed.
///
/// The kind is part of each credential's authenticated context, so the signing secret cannot be
/// read back through a request for the API key even by someone who swaps the two columns by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResendApiCredentialKind {
    ApiKey,
    SigningSecret,
}

impl ResendApiCredentialKind {
    /// Every variant, so the rotation job can enumerate what a row holds rather than listing the
    /// columns a second time.
    pub const ALL: &'static [Self] = &[Self::ApiKey, Self::SigningSecret];

    /// The name bound into the credential's associated data. It is also the column the secret
    /// lives in, and deliberately so: one string names the secret everywhere it is referred to.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::SigningSecret => "signing_secret",
        }
    }
}

/// One company's Resend integration as a settings page may see it: the switch, the endpoint, and
/// the account identity. No secret appears here, and none may be added -- this is the shape that
/// gets serialized, logged and rendered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyResendApiIntegration {
    pub company_id: Uuid,
    pub webhook_token: ResendApiWebhookToken,
    pub authserv_id: AuthservId,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CompanyResendApiIntegration {
    /// The absolute URL to register in this company's Resend dashboard.
    ///
    /// Built from the deployment's own origin rather than from the request that happens to be
    /// asking, so the value an operator copies is the one Resend can actually reach even when the
    /// page was opened through a tunnel or a private host name.
    pub fn webhook_url(&self, base_url: &str) -> String {
        format!(
            "{origin}{RESEND_API_WEBHOOK_PATH}/{token}",
            origin = base_url.trim_end_matches('/'),
            token = self.webhook_token,
        )
    }
}

/// A settings-form write.
///
/// `None` on either secret means "keep the stored one", which is what lets the form render a
/// blank password field instead of round-tripping a credential through a browser. A first write
/// therefore has to carry both; the store is what refuses one that does not.
#[derive(Debug)]
pub struct CompanyResendApiIntegrationWrite {
    pub api_key: Option<SecretString>,
    pub signing_secret: Option<SecretString>,
    pub authserv_id: AuthservId,
    pub enabled: bool,
}

/// What the webhook route needs to answer one unauthenticated request.
///
/// The token found the row; this is what proves the request. Loaded together because the tenant
/// and the secret that authenticates it are one decision: verifying against any other company's
/// secret would authenticate nothing at all.
#[derive(Debug)]
pub struct ResendApiInboundCredentials {
    pub company_id: Uuid,
    pub signing_secret: SecretString,
}

/// What the runtime needs to act as one company at Resend: the key its API calls carry, and the
/// `authserv-id` whose verdicts may be believed on the mail those calls return.
#[derive(Debug)]
pub struct ResendApiAccountCredentials {
    pub api_key: SecretString,
    pub authserv_id: AuthservId,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integration(token: &str) -> CompanyResendApiIntegration {
        CompanyResendApiIntegration {
            company_id: Uuid::new_v4(),
            webhook_token: ResendApiWebhookToken::new(token),
            authserv_id: AuthservId::new("resend.com"),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn the_webhook_url_is_the_served_path_with_the_token_appended() {
        let token = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            integration(token).webhook_url("https://example.com"),
            format!("https://example.com{RESEND_API_WEBHOOK_PATH}/{token}")
        );
    }

    #[test]
    fn a_base_url_with_a_trailing_slash_does_not_double_it() {
        let url =
            integration("0123456789abcdef0123456789abcdef").webhook_url("https://example.com/");
        assert!(!url.contains("com//"), "{url}");
    }

    #[test]
    fn every_credential_kind_has_its_own_context_name() {
        let mut names: Vec<&str> = ResendApiCredentialKind::ALL
            .iter()
            .map(|kind| kind.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ResendApiCredentialKind::ALL.len());
    }
}
