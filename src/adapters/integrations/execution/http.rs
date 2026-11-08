use super::*;
use crate::{
    adapters::guarded_http,
    application::{
        integrations::private::InjectionMethod,
        workflow::http::{HttpOrigin, HttpRequest},
    },
};
use std::sync::Mutex;

fn request(i: &AuthorizedInvocation) -> IntegrationResult<(HttpOrigin, HttpRequest)> {
    let origin = HttpOrigin::parse(i.authorization.destination.as_str())
        .map_err(|_| IntegrationError::Invalid)?;
    let request = HttpRequest::parse(i.arguments.value()).map_err(|_| IntegrationError::Invalid)?;
    Ok((origin, request))
}
pub(super) fn url(i: &AuthorizedInvocation) -> IntegrationResult<url::Url> {
    let (origin, request) = request(i)?;
    let url =
        guarded_http::request_url(&origin, &request).map_err(|_| IntegrationError::Invalid)?;
    if url.query_pairs().any(|(name, _)| {
        matches!(
            name.to_ascii_lowercase().replace('-', "_").as_str(),
            "access_token"
                | "refresh_token"
                | "id_token"
                | "api_key"
                | "apikey"
                | "authorization"
                | "key"
                | "token"
                | "auth"
        )
    }) {
        return Err(IntegrationError::Denied);
    }
    Ok(url)
}
pub(super) async fn prepare(
    transports: &IntegrationTransports,
    i: &AuthorizedInvocation,
    c: &ExecutionContext,
    injection: TypedInjection,
) -> IntegrationResult<Box<dyn PreparedBusinessCall>> {
    url(i)?;
    let (origin, request) = request(i)?;
    // Box the client/DNS seam so factoring the owned request does not grow native frames.
    Box::pin(prepare_origin(transports, i, c, injection, origin, request)).await
}
#[cfg(test)]
pub(super) async fn prepare_loopback(
    transports: &IntegrationTransports,
    i: &AuthorizedInvocation,
    c: &ExecutionContext,
    injection: TypedInjection,
    origin: HttpOrigin,
) -> IntegrationResult<Box<dyn PreparedBusinessCall>> {
    url(i)?;
    let (_, request) = request(i)?;
    let parsed = url::Url::parse(origin.as_str()).map_err(|_| IntegrationError::Invalid)?;
    if parsed.scheme() != "http"
        || !parsed
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback())
    {
        return Err(IntegrationError::Denied);
    }
    Box::pin(prepare_origin(transports, i, c, injection, origin, request)).await
}
async fn prepare_origin(
    transports: &IntegrationTransports,
    i: &AuthorizedInvocation,
    c: &ExecutionContext,
    injection: TypedInjection,
    origin: HttpOrigin,
    request: HttpRequest,
) -> IntegrationResult<Box<dyn PreparedBusinessCall>> {
    let client = preflight(i.deadline, &c.cancellation, async {
        guarded_http::client(&transports.http, &origin)
            .await
            .map_err(|_| IntegrationError::Unavailable)
    })
    .await?;
    Ok(Box::new(HttpCall {
        client,
        origin,
        request: Mutex::new(Some(request)),
        injection,
    }))
}
struct HttpCall {
    client: reqwest::Client,
    origin: HttpOrigin,
    request: Mutex<Option<HttpRequest>>,
    injection: TypedInjection,
}
#[async_trait]
impl PreparedBusinessCall for HttpCall {
    async fn call(&self) -> IntegrationResult<ProviderData> {
        let request = self
            .request
            .lock()
            .map_err(|_| IntegrationError::Unavailable)?
            .take()
            .ok_or(IntegrationError::Denied)?;
        let auth = match &self.injection.method {
            InjectionMethod::Bearer => {
                guarded_http::HttpAuthentication::Bearer(&self.injection.value)
            }
            InjectionMethod::ApiKeyHeader(header) => {
                guarded_http::HttpAuthentication::Header(header, &self.injection.value)
            }
        };
        let data =
            guarded_http::send_authenticated(self.client.clone(), &self.origin, auth, request)
                .await
                .map_err(|_| IntegrationError::Unavailable)?;
        ProviderData::from_redacted(data)
    }
    async fn close(&self) -> IntegrationResult<()> {
        Ok(())
    }
}
