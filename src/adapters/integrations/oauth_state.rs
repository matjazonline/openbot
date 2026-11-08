use crate::{
    application::integrations::connect::{AttemptFacts, ConnectStateCodec},
    domain::integrations::*,
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
const PURPOSE: &str = "openbot-integration-connect/v1";
pub(crate) struct SignedConnectState {
    encoding: EncodingKey,
    decoding: DecodingKey,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    iss: String,
    aud: String,
    purpose: String,
    iat: i64,
    exp: i64,
    facts: AttemptFacts,
}
impl SignedConnectState {
    pub fn new(secret: &str) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
        }
    }
}
impl ConnectStateCodec for SignedConnectState {
    fn sign(&self, facts: &AttemptFacts) -> IntegrationResult<String> {
        let c = Claims {
            iss: PURPOSE.into(),
            aud: PURPOSE.into(),
            purpose: PURPOSE.into(),
            iat: facts.issued_at.timestamp(),
            exp: facts.expires_at.timestamp(),
            facts: facts.clone(),
        };
        let value = jsonwebtoken::encode(&Header::new(Algorithm::HS256), &c, &self.encoding)
            .map_err(|_| IntegrationError::Invalid)?;
        if value.len() > 4096 {
            return Err(IntegrationError::Limit);
        }
        Ok(value)
    }
    fn verify(&self, state: &str) -> IntegrationResult<AttemptFacts> {
        if state.len() > 4096 {
            return Err(IntegrationError::Limit);
        }
        let mut v = Validation::new(Algorithm::HS256);
        v.leeway = 0;
        v.set_audience(&[PURPOSE]);
        v.set_issuer(&[PURPOSE]);
        let c = jsonwebtoken::decode::<Claims>(state, &self.decoding, &v)
            .map_err(|_| IntegrationError::Denied)?
            .claims;
        let now = chrono::Utc::now().timestamp();
        if c.purpose != PURPOSE
            || c.iat > now
            || c.exp <= now
            || c.exp - c.iat > 600
            || c.exp <= c.iat
            || c.facts.issued_at.timestamp() != c.iat
            || c.facts.expires_at.timestamp() != c.exp
        {
            return Err(IntegrationError::Denied);
        }
        Ok(c.facts)
    }
}
