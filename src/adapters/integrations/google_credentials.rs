//! Current Google credential schema. No login, consent, or network behavior.
use crate::{
    application::integrations::private::{
        CredentialAdapter, CredentialCacheUpdate, InjectionMethod, TypedInjection,
    },
    domain::integrations::*,
};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GooglePayload {
    pub subject: ExternalSubject,
    pub client: ExternalSubject,
    pub mailbox: VerifiedMailbox,
    pub scopes: Vec<Scope>,
    pub refresh_token: SecretString,
    pub access_token: Option<SecretString>,
    pub expires_at: Option<DateTime<Utc>>,
    pub generation: CredentialGeneration,
    pub refresh_retry_at: Option<DateTime<Utc>>,
}

pub(crate) struct GoogleCredentialAdapter {
    pub client: ExternalSubject,
}
impl GoogleCredentialAdapter {
    pub(super) fn decode(&self, raw: &SecretString) -> IntegrationResult<GooglePayload> {
        if raw.expose_secret().len() > MAX_CREDENTIAL_BYTES {
            return Err(IntegrationError::Invalid);
        }
        let mut payload: GooglePayload =
            serde_json::from_str(raw.expose_secret()).map_err(|_| IntegrationError::Credential)?;
        payload.scopes = canonical_scopes(payload.scopes)?;
        if payload.client != self.client
            || payload.scopes.len() > MAX_SCOPES
            || !super::google_authorization::opaque_token(&payload.refresh_token)
            || payload.access_token.is_some() != payload.expires_at.is_some()
            || payload
                .access_token
                .as_ref()
                .is_some_and(|token| !super::google_authorization::opaque_token(token))
        {
            return Err(IntegrationError::Credential);
        }
        // Both fields are part of the current schema even when a cache/retry is absent.
        let _ = (payload.generation, payload.refresh_retry_at);
        Ok(payload)
    }
}
impl CredentialAdapter for GoogleCredentialAdapter {
    fn policy_identity(
        &self,
        app: &crate::application::integrations::AppDefinition,
    ) -> IntegrationResult<String> {
        Ok(format!("google-bearer/v1/{:?}", required_scopes(app)?))
    }
    fn validate_use(
        &self,
        raw: &SecretString,
        app: &crate::application::integrations::AppDefinition,
    ) -> IntegrationResult<()> {
        let payload = self.decode(raw)?;
        if required_scopes(app)?
            .iter()
            .any(|scope| !payload.scopes.iter().any(|s| s.as_str() == *scope))
        {
            return Err(IntegrationError::Denied);
        }
        Ok(())
    }
    fn validate(&self, raw: &SecretString) -> IntegrationResult<Option<ExternalIdentity>> {
        let payload = self.decode(raw)?;
        let mut digest = Sha256::new();
        for field in [payload.subject.as_str(), payload.client.as_str()] {
            digest.update((field.len() as u32).to_be_bytes());
            digest.update(field.as_bytes());
        }
        Ok(Some(ExternalIdentity {
            kind: ExternalIdentityKind::parse("google-subject-client")?,
            subject: payload.subject,
            authorization_key: AuthorizationKey::parse(format!("{:x}", digest.finalize()))?,
        }))
    }
    fn validate_cache_update(&self, update: CredentialCacheUpdate<'_>) -> IntegrationResult<()> {
        let previous = self.decode(update.previous)?;
        let replacement = self.decode(update.replacement)?;
        let previous_scopes: std::collections::BTreeSet<_> = previous.scopes.into_iter().collect();
        let replacement_scopes: std::collections::BTreeSet<_> =
            replacement.scopes.into_iter().collect();
        if previous.subject != replacement.subject
            || previous.client != replacement.client
            || previous_scopes != replacement_scopes
            || previous.mailbox != replacement.mailbox
        {
            return Err(IntegrationError::Denied);
        }
        Ok(())
    }
    fn injection(&self, raw: &SecretString) -> IntegrationResult<TypedInjection> {
        let payload = self.decode(raw)?;
        Ok(TypedInjection {
            method: InjectionMethod::Bearer,
            value: payload.access_token.ok_or(IntegrationError::Credential)?,
        })
    }
}

pub(super) fn required_scopes(
    app: &crate::application::integrations::AppDefinition,
) -> IntegrationResult<Vec<&'static str>> {
    app.capabilities
        .iter()
        .map(|capability| match capability.as_str() {
            "gmail.read" => Ok("https://www.googleapis.com/auth/gmail.readonly"),
            "gmail.send" => Ok("https://www.googleapis.com/auth/gmail.send"),
            _ => Err(IntegrationError::Denied),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(super) struct VerifiedMailbox(String);
impl VerifiedMailbox {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for VerifiedMailbox {
    type Error = IntegrationError;
    fn try_from(value: String) -> IntegrationResult<Self> {
        if value.len() > 256
            || value.chars().any(char::is_control)
            || value.parse::<lettre::Address>().is_err()
        {
            return Err(IntegrationError::Credential);
        }
        Ok(Self(value))
    }
}
impl GooglePayload {
    pub fn encode(&self) -> IntegrationResult<SecretString> {
        let raw=serde_json::json!({"subject":self.subject,"client":self.client,"mailbox":self.mailbox.as_str(),"scopes":self.scopes,"refresh_token":self.refresh_token.expose_secret(),"access_token":self.access_token.as_ref().map(|s|s.expose_secret()),"expires_at":self.expires_at,"generation":self.generation,"refresh_retry_at":self.refresh_retry_at}).to_string();
        if raw.len() > MAX_CREDENTIAL_BYTES {
            return Err(IntegrationError::Limit);
        }
        Ok(SecretString::from(raw))
    }
}

/// Google's identity scope spellings express the same provider consent fact.
pub(super) fn canonical_scopes(scopes: Vec<Scope>) -> IntegrationResult<Vec<Scope>> {
    if scopes.len() > MAX_SCOPES {
        return Err(IntegrationError::Limit);
    }
    scopes
        .into_iter()
        .map(|scope| {
            if scope.as_str() == "email" {
                Scope::parse("https://www.googleapis.com/auth/userinfo.email")
            } else {
                Ok(scope)
            }
        })
        .collect::<IntegrationResult<std::collections::BTreeSet<_>>>()
        .map(|s| s.into_iter().collect())
}
