use crate::{
    app_error::{AppError, AppResult},
    entities::mcp::McpEndpoint,
};
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

/// Exact IP-literal endpoints, supplied by the deployment, are the only private/HTTP exceptions.
/// Pinning the full URL prevents a tenant from widening an exception to another path or port.
#[derive(Clone, Default)]
pub struct EndpointPolicy {
    internal: HashSet<McpEndpoint>,
}
impl EndpointPolicy {
    pub fn new(endpoints: impl IntoIterator<Item = String>) -> AppResult<Self> {
        let mut internal = HashSet::new();
        for endpoint in endpoints {
            let endpoint = McpEndpoint::try_from(endpoint).map_err(AppError::BadRequest)?;
            let url = url::Url::parse(endpoint.as_str()).map_err(|_| denied())?;
            if !matches!(url.host(), Some(url::Host::Ipv4(_) | url::Host::Ipv6(_))) {
                return Err(AppError::BadRequest(
                    "MCP_INTERNAL_ENDPOINTS requires exact IP-literal URLs".into(),
                ));
            }
            internal.insert(endpoint);
        }
        Ok(Self { internal })
    }
    pub fn validate(&self, endpoint: &McpEndpoint) -> AppResult<()> {
        let url = url::Url::parse(endpoint.as_str()).map_err(|_| denied())?;
        if self.internal.contains(endpoint) {
            return Ok(());
        }
        if url.scheme() != "https" {
            return Err(denied());
        }
        match url.host() {
            Some(url::Host::Ipv4(ip)) if blocked(ip.into()) => Err(denied()),
            Some(url::Host::Ipv6(ip)) if blocked(ip.into()) => Err(denied()),
            Some(url::Host::Domain(host))
                if host.ends_with(".localhost")
                    || host == "localhost"
                    || host == "metadata.google.internal" =>
            {
                Err(denied())
            }
            _ => Ok(()),
        }
    }
    pub async fn client(&self, endpoint: &McpEndpoint) -> AppResult<reqwest::Client> {
        self.validate(endpoint)?;
        let url = url::Url::parse(endpoint.as_str()).map_err(|_| denied())?;
        let host = url.host_str().ok_or_else(denied)?.trim_matches(['[', ']']);
        let port = url.port_or_known_default().ok_or_else(denied)?;
        let addresses: Vec<SocketAddr> = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| denied())?
        .map_err(|_| denied())?
        .take(17)
        .collect();
        self.client_resolved(endpoint, addresses)
    }

    pub(crate) fn client_resolved(
        &self,
        endpoint: &McpEndpoint,
        addresses: Vec<SocketAddr>,
    ) -> AppResult<reqwest::Client> {
        self.validate(endpoint)?;
        let url = url::Url::parse(endpoint.as_str()).map_err(|_| denied())?;
        let host = url.host_str().ok_or_else(denied)?.trim_matches(['[', ']']);
        let port = url.port_or_known_default().ok_or_else(denied)?;
        if addresses.is_empty()
            || addresses.len() > 16
            || addresses.iter().any(|address| address.port() != port)
            || (!self.internal.contains(endpoint) && addresses.iter().any(|a| blocked(a.ip())))
        {
            return Err(denied());
        }
        pinned_client(host, &addresses)
    }
}
fn pinned_client(host: &str, addresses: &[SocketAddr]) -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(10))
        .resolve_to_addrs(host, addresses)
        .pool_max_idle_per_host(1)
        .build()
        .map_err(|_| denied())
}
fn denied() -> AppError {
    AppError::BadRequest("MCP destination is prohibited or unavailable".into())
}

// Conservative special-use ranges, matching the guarded web-fetch policy. Restrict IPv6 to
// ordinary global unicast and exclude transition/documentation ranges (including embedded IPv4).
fn blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let value = u32::from(ip);
            [
                (0x00000000, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 4),
                (0xf0000000, 4),
            ]
            .iter()
            .any(|(network, bits)| value >> (32 - bits) == network >> (32 - bits))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            segments[0] & 0xe000 != 0x2000
                || (segments[0] == 0x2001 && (segments[1] < 0x200 || segments[1] == 0xdb8))
                || matches!(segments[0], 0x2002 | 0x3ffe | 0x3fff)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_http_resolved_dns_rejects_any_prohibited_answer_and_rebinding() {
        let endpoint = McpEndpoint::try_from("https://public.invalid/".to_owned()).unwrap();
        let policy = EndpointPolicy::default();
        let public: SocketAddr = "8.8.8.8:443".parse().unwrap();
        assert!(policy.client_resolved(&endpoint, vec![public]).is_ok());
        for bad in [
            "127.0.0.1:443",
            "169.254.169.254:443",
            "10.0.0.1:443",
            "[::ffff:127.0.0.1]:443",
            "[2002:7f00:1::]:443",
            "8.8.8.8:80",
        ] {
            assert!(
                policy
                    .client_resolved(&endpoint, vec![public, bad.parse().unwrap()])
                    .is_err()
            );
            assert!(
                policy
                    .client_resolved(&endpoint, vec![bad.parse().unwrap()])
                    .is_err(),
                "a later rebound resolution cannot reuse approval"
            );
        }
        assert!(policy.client_resolved(&endpoint, vec![]).is_err());
        assert!(policy.client_resolved(&endpoint, vec![public; 17]).is_err());
    }
    #[tokio::test]
    async fn workflow_http_connector_uses_pinned_addresses_without_resolving_hostname_again() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let length = socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8(request[..length].to_vec()).unwrap()
        });
        // Exercise the actual connector with a deliberately unresolvable host. The policy
        // validator above is separately exercised; only this test directly supplies its pin.
        let client = pinned_client("rebind.invalid", &[address]).unwrap();
        let response = client
            .get(format!("http://rebind.invalid:{}/", address.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(task.await.unwrap().contains("host: rebind.invalid:"));
    }
    #[test]
    fn policy_exceptions_are_exact_and_operator_owned() {
        let endpoint = |v: &str| McpEndpoint::try_from(v.to_owned()).unwrap();
        let policy = EndpointPolicy::new(["http://127.0.0.1:8080/mcp".into()]).unwrap();
        assert!(
            policy
                .validate(&endpoint("http://127.0.0.1:8080/mcp"))
                .is_ok()
        );
        for url in [
            "http://127.0.0.1:8080/other",
            "http://example.com/mcp",
            "https://169.254.169.254/",
            "https://[::ffff:127.0.0.1]/",
        ] {
            assert!(policy.validate(&endpoint(url)).is_err());
        }
        assert!(EndpointPolicy::new(["http://internal.example/mcp".into()]).is_err());
        for ip in [
            "127.1.2.3",
            "100.64.0.1",
            "169.254.1.1",
            "::1",
            "64:ff9b::a00:1",
            "2002:7f00:1::",
        ] {
            assert!(blocked(ip.parse().unwrap()));
        }
        assert!(!blocked("8.8.8.8".parse().unwrap()));
    }
}
