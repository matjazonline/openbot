//! One shared, bounded scanner client for the deployment's SMTP edge.
use crate::application::services::spam_scanner::{SpamScanInput, SpamScanResult, SpamScanner};
use anyhow::{Context, bail};
use async_trait::async_trait;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tracing::warn;
mod heuristic;
use heuristic::HeuristicScanner;

const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RESPONSE_BYTES: usize = 65_536;

pub enum ExternalScannerSettings {
    Rspamd { endpoint: String },
    Spamd { endpoint: String },
}
pub struct SpamScannerSettings {
    pub heuristic: bool,
    pub external: Option<ExternalScannerSettings>,
    pub threshold: f64,
}
enum Backend {
    Disabled,
    Rspamd {
        endpoint: reqwest::Url,
        client: reqwest::Client,
    },
    Spamd {
        endpoint: String,
    },
}
pub struct SpamScannerAdapter {
    heuristic: bool,
    backend: Backend,
    threshold: f64,
}
impl SpamScannerAdapter {
    pub fn new(settings: SpamScannerSettings) -> anyhow::Result<Self> {
        let backend = match settings.external {
            None => Backend::Disabled,
            Some(ExternalScannerSettings::Rspamd { endpoint }) => {
                let endpoint = reqwest::Url::parse(&endpoint).context("invalid Rspamd endpoint")?;
                if !matches!(endpoint.scheme(), "http" | "https") {
                    bail!("Rspamd endpoint must use HTTP or HTTPS");
                }
                let client = reqwest::Client::builder()
                    .pool_idle_timeout(Duration::from_secs(30))
                    .pool_max_idle_per_host(8)
                    .connect_timeout(CONNECT_TIMEOUT)
                    .timeout(NETWORK_TIMEOUT)
                    .build()
                    .context("could not build bounded spam-scanner client")?;
                Backend::Rspamd { endpoint, client }
            }
            Some(ExternalScannerSettings::Spamd { endpoint }) => {
                let address = endpoint
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .trim_start_matches("spamd://");
                let address = address.split('/').next().unwrap_or(address);
                if address.trim().is_empty() {
                    bail!("SpamAssassin endpoint must name a host");
                }
                let endpoint = if address.contains(':') {
                    address.to_string()
                } else {
                    format!("{address}:783")
                };
                Backend::Spamd { endpoint }
            }
        };
        Ok(Self {
            heuristic: settings.heuristic,
            backend,
            threshold: settings.threshold,
        })
    }

    async fn external(&self, raw_mime: &[u8]) -> anyhow::Result<SpamScanResult> {
        // Lease/drop cancellation reaches the actual network operation, with no detached task.
        tokio::time::timeout(NETWORK_TIMEOUT, async {
            match &self.backend {
                Backend::Disabled => Ok(SpamScanResult::new()),
                Backend::Rspamd { endpoint, client } => {
                    scan_rspamd(client, endpoint, raw_mime).await
                }
                Backend::Spamd { endpoint } => scan_spamd(endpoint, raw_mime).await,
            }
        })
        .await
        .context("spam scan exceeded its network deadline")?
    }
}
#[async_trait]
impl SpamScanner for SpamScannerAdapter {
    async fn scan(&self, input: SpamScanInput<'_>) -> SpamScanResult {
        let mut result = if self.heuristic {
            HeuristicScanner::scan(input.subject, input.from, input.body_text, input.body_html)
        } else {
            SpamScanResult::new()
        };
        match self.external(input.raw_mime).await {
            Ok(external) => {
                result.score += external.score;
                result.reasons.extend(external.reasons);
            }
            Err(error) => warn!(%error, "External spam scan failed; retaining local score"),
        }
        result.is_spam = result.score >= self.threshold;
        result
    }
}
async fn scan_rspamd(
    client: &reqwest::Client,
    endpoint: &reqwest::Url,
    raw_mime: &[u8],
) -> anyhow::Result<SpamScanResult> {
    let mut response = client
        .post(endpoint.clone())
        .body(raw_mime.to_vec())
        .send()
        .await
        .map_err(reqwest::Error::without_url)?;
    if !response.status().is_success() {
        bail!("Rspamd returned HTTP status {}", response.status());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            bail!("Rspamd response exceeds {MAX_RESPONSE_BYTES} bytes");
        }
        bytes.extend_from_slice(&chunk);
    }
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).context("invalid Rspamd response")?;
    let mut result = SpamScanResult::new();
    if let Some(score) = json.get("score").and_then(serde_json::Value::as_f64) {
        result.add(score, format!("Rspamd evaluation score: {score:.2}"));
    }
    if json.get("action").and_then(serde_json::Value::as_str) == Some("reject") {
        result.add(5.0, "Rspamd action flag: reject");
    }
    Ok(result)
}
async fn scan_spamd(endpoint: &str, raw_mime: &[u8]) -> anyhow::Result<SpamScanResult> {
    let mut stream = TcpStream::connect(endpoint).await?;
    let request = format!(
        "PROCESS SPAMC/1.2\r\nContent-length: {}\r\n\r\n",
        raw_mime.len()
    );
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(raw_mime).await?;
    stream.flush().await?;
    // PROCESS also returns a message body. Only its bounded headers carry the score we need.
    let mut reader = BufReader::new(stream).take(MAX_RESPONSE_BYTES as u64 + 1);
    let mut headers = Vec::new();
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line).await? == 0 {
            break;
        }
        headers.extend_from_slice(&line);
        if headers.len() > MAX_RESPONSE_BYTES {
            bail!("SpamAssassin headers exceed {MAX_RESPONSE_BYTES} bytes");
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let mut result = SpamScanResult::new();
    for line in String::from_utf8_lossy(&headers).lines() {
        if line.to_ascii_lowercase().starts_with("spam:")
            && let Some((_, scores)) = line.split_once(';')
            && let Some(score) = scores
                .split('/')
                .next()
                .and_then(|s| s.trim().parse::<f64>().ok())
            && score.is_finite()
        {
            result.add(score, format!("SpamAssassin score: {score:.2}"));
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests;
