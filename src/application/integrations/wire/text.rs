use super::*;

/// Untrusted navigation data; decoding never produces SanitizedConnectUrl or approves an origin.
#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AuthorizationUrlData(String);
impl AuthorizationUrlData {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for AuthorizationUrlData {
    type Error = IntegrationError;
    fn try_from(value: String) -> IntegrationResult<Self> {
        if value.len() > 8192 {
            return Err(IntegrationError::Limit);
        }
        let url = url::Url::parse(&value).map_err(|_| IntegrationError::Invalid)?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query_pairs().any(|(key, _)| {
                matches!(
                    key.to_ascii_lowercase().as_str(),
                    "access_token"
                        | "refresh_token"
                        | "id_token"
                        | "client_secret"
                        | "code_verifier"
                        | "code"
                )
            })
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(Self(value))
    }
}
impl From<AuthorizationUrlData> for String {
    fn from(value: AuthorizationUrlData) -> Self {
        value.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CallbackError(String);
impl CallbackError {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for CallbackError {
    type Error = IntegrationError;
    fn try_from(value: String) -> IntegrationResult<Self> {
        if value.is_empty()
            || value.len() > 256
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(Self(value))
    }
}
impl From<CallbackError> for String {
    fn from(value: CallbackError) -> Self {
        value.0
    }
}
