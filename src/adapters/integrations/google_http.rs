//! Fixed Google credential endpoints. Raw provider errors never escape this adapter.
use crate::{application::integrations::ConnectStatus, domain::integrations::MAX_CREDENTIAL_BYTES};
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;
#[derive(Debug, Clone, Copy)]
pub(super) enum GoogleHttpError {
    Cancelled,
    Unavailable,
    Malformed,
}
impl From<GoogleHttpError> for ConnectStatus {
    fn from(error: GoogleHttpError) -> Self {
        match error {
            GoogleHttpError::Cancelled => Self::Cancelled,
            _ => Self::ProviderError,
        }
    }
}
pub(super) struct GoogleReply {
    pub status: u16,
    pub body: SecretString,
}
pub(crate) struct GoogleHttp {
    client: reqwest::Client,
    #[cfg(test)]
    endpoints: Option<(String, String)>,
}
impl GoogleHttp {
    pub fn new() -> Result<Self, ConnectStatus> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|_| ConnectStatus::Configuration)?,
            #[cfg(test)]
            endpoints: None,
        })
    }
    #[cfg(test)]
    pub fn scripted(token: String, identity: String) -> Self {
        for endpoint in [&token, &identity] {
            let url = url::Url::parse(endpoint).unwrap();
            assert_eq!(url.scheme(), "http");
            assert_eq!(url.host_str(), Some("127.0.0.1"));
        }
        Self {
            endpoints: Some((token, identity)),
            ..Self::new().unwrap()
        }
    }
    pub(super) async fn token(
        &self,
        form: &[(&str, &str)],
        deadline: DateTime<Utc>,
        cancel: &CancellationToken,
    ) -> Result<GoogleReply, GoogleHttpError> {
        let endpoint = "https://oauth2.googleapis.com/token";
        #[cfg(test)]
        let endpoint = self
            .endpoints
            .as_ref()
            .map(|p| p.0.as_str())
            .unwrap_or(endpoint);
        self.bounded(self.client.post(endpoint).form(form), deadline, cancel)
            .await
    }
    pub(super) async fn identity(
        &self,
        access: &str,
        deadline: DateTime<Utc>,
        cancel: &CancellationToken,
    ) -> Result<GoogleReply, GoogleHttpError> {
        let endpoint = "https://openidconnect.googleapis.com/v1/userinfo";
        #[cfg(test)]
        let endpoint = self
            .endpoints
            .as_ref()
            .map(|p| p.1.as_str())
            .unwrap_or(endpoint);
        self.bounded(
            self.client.get(endpoint).bearer_auth(access),
            deadline,
            cancel,
        )
        .await
    }
    async fn bounded(
        &self,
        request: reqwest::RequestBuilder,
        deadline: DateTime<Utc>,
        cancel: &CancellationToken,
    ) -> Result<GoogleReply, GoogleHttpError> {
        let remaining = (deadline - Utc::now())
            .to_std()
            .map_err(|_| GoogleHttpError::Unavailable)?;
        tokio::select! {biased;
            _=cancel.cancelled()=>Err(GoogleHttpError::Cancelled),
            _=tokio::time::sleep(remaining)=>Err(GoogleHttpError::Unavailable),
            result=read(request)=>result,
        }
    }
}
async fn read(request: reqwest::RequestBuilder) -> Result<GoogleReply, GoogleHttpError> {
    let mut response = request
        .send()
        .await
        .map_err(|_| GoogleHttpError::Unavailable)?;
    if response
        .content_length()
        .is_some_and(|v| v > MAX_CREDENTIAL_BYTES as u64)
    {
        return Err(GoogleHttpError::Malformed);
    }
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| GoogleHttpError::Unavailable)?
    {
        if bytes.len() + chunk.len() > MAX_CREDENTIAL_BYTES {
            return Err(GoogleHttpError::Malformed);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(GoogleReply {
        status,
        body: SecretString::from(String::from_utf8(bytes).map_err(|_| GoogleHttpError::Malformed)?),
    })
}
pub(super) fn failure(reply: &GoogleReply) -> ConnectStatus {
    use secrecy::ExposeSecret;
    let value: serde_json::Value =
        serde_json::from_str(reply.body.expose_secret()).unwrap_or(serde_json::Value::Null);
    match value.get("error").and_then(|v| v.as_str()) {
        Some("invalid_client" | "unauthorized_client") => ConnectStatus::Configuration,
        Some("admin_policy_enforced" | "access_blocked") => ConnectStatus::AdminBlocked,
        Some("access_denied") => ConnectStatus::Denied,
        _ => ConnectStatus::ProviderError,
    }
}
