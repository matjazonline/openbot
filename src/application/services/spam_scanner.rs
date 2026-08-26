use crate::infra::config::AppConfig;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, warn};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SpamScanResult {
    pub score: f64,
    pub is_spam: bool,
    pub reasons: Vec<String>,
}

impl SpamScanResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, score_delta: f64, reason: impl Into<String>) {
        self.score += score_delta;
        self.reasons.push(reason.into());
    }
}

/// Stage 1: Rust-Native Heuristic Scanner (Option B)
pub struct HeuristicScanner;

impl HeuristicScanner {
    pub fn scan(
        subject: Option<&str>,
        from: Option<&str>,
        body_text: Option<&str>,
        body_html: Option<&str>,
    ) -> SpamScanResult {
        let mut result = SpamScanResult::new();

        if let Some(subj) = subject {
            let subj_upper = subj.to_uppercase();

            // High urgency/spam keywords in subject
            let keywords = [
                ("URGENT", 1.5),
                ("CLAIM YOUR REWARD", 3.0),
                ("WINNER", 2.0),
                ("LOTTERY", 3.0),
                ("CASINO", 2.5),
                ("CRYPTO OFFER", 2.5),
                ("FREE GIFT", 2.0),
                ("ACT NOW", 1.5),
                ("BITCOIN", 2.0),
                ("CONGRATULATIONS", 1.5),
                ("100% FREE", 2.5),
                ("MAKE MONEY", 2.5),
            ];

            for (kw, score) in keywords {
                if subj_upper.contains(kw) {
                    result.add(score, format!("Subject contains spam keyword '{}'", kw));
                }
            }

            // Excess uppercase ratio check
            if subj.len() > 10 {
                let uppercase_count = subj.chars().filter(|c| c.is_uppercase()).count();
                let ratio = uppercase_count as f64 / subj.len() as f64;
                if ratio > 0.65 {
                    result.add(1.5, "Excessive uppercase letters in subject");
                }
            }

            // Punctuation check
            if subj.contains("!!!") || subj.contains("???") || subj.contains("$$$") {
                result.add(1.0, "Excessive punctuation in subject");
            }
        }

        // Body heuristics
        let combined_body = format!(
            "{}\n{}",
            body_text.unwrap_or_default(),
            body_html.unwrap_or_default()
        );

        if !combined_body.trim().is_empty() {
            let body_upper = combined_body.to_uppercase();

            // Shortener link detection
            let shorteners = [
                "bit.ly/",
                "tinyurl.com/",
                "t.co/",
                "goo.gl/",
                "ow.ly/",
                "is.gd/",
                "buff.ly/",
            ];
            for shortener in shorteners {
                if combined_body.contains(shortener) {
                    result.add(1.5, format!("Contains shortened link '{}'", shortener));
                }
            }

            // Financial/phishing triggers
            if body_upper.contains("SEND BITCOIN TO") || body_upper.contains("WALLET ADDRESS:") {
                result.add(3.0, "Crypto ransom or payment solicitation in body");
            }

            if body_upper.contains("VERIFY YOUR ACCOUNT IMMEDIATELY")
                || body_upper.contains("ACCOUNT SUSPENDED")
            {
                result.add(2.0, "Urgent account verification phishing language");
            }

            // Hidden text trick detection in HTML
            if let Some(html) = body_html {
                let html_lower = html.to_lowercase();
                if html_lower.contains("display:none")
                    || html_lower.contains("display: none")
                    || html_lower.contains("font-size:0")
                    || html_lower.contains("font-size: 0")
                {
                    result.add(2.0, "Hidden text elements detected in HTML body");
                }
            }
        }

        // Sender address sanity
        if let Some(from_addr) = from {
            if from_addr.contains("noreply@") && combined_body.contains("click here to reply") {
                result.add(1.0, "No-reply sender with reply request link");
            }
        }

        result
    }
}

/// Stage 2: External SpamScanner Client (Option A: Rspamd or SpamAssassin)
pub struct ExternalSpamScanner {
    config: Arc<AppConfig>,
    http_client: reqwest::Client,
}

impl ExternalSpamScanner {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        Self {
            config,
            http_client,
        }
    }

    pub async fn scan_mime(&self, raw_mime: &[u8]) -> SpamScanResult {
        let result = SpamScanResult::new();

        if !self.config.enable_spam_scanner {
            return result;
        }

        match self.config.spam_scanner_type.to_lowercase().as_str() {
            "spamassassin" | "spamd" => match self.scan_spamassassin(raw_mime).await {
                Ok(res) => return res,
                Err(err) => {
                    warn!("SpamAssassin scan failed: {err}");
                }
            },
            _ => {
                // Default to Rspamd HTTP
                match self.scan_rspamd(raw_mime).await {
                    Ok(res) => return res,
                    Err(err) => {
                        warn!("Rspamd scan failed: {err}");
                    }
                }
            }
        }

        result
    }

    async fn scan_rspamd(&self, raw_mime: &[u8]) -> anyhow::Result<SpamScanResult> {
        let response = self
            .http_client
            .post(&self.config.spam_scanner_url)
            .body(raw_mime.to_vec())
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Rspamd server returned HTTP status {}", response.status());
        }

        let json: serde_json::Value = response.json().await?;
        let mut result = SpamScanResult::new();

        if let Some(score) = json.get("score").and_then(|v| v.as_f64()) {
            result.score = score;
            result
                .reasons
                .push(format!("Rspamd evaluation score: {:.2}", score));
        }

        if let Some(action) = json.get("action").and_then(|v| v.as_str()) {
            if action == "reject" {
                result.add(5.0, "Rspamd action flag: reject");
            }
        }

        Ok(result)
    }

    async fn scan_spamassassin(&self, raw_mime: &[u8]) -> anyhow::Result<SpamScanResult> {
        let url_str = &self.config.spam_scanner_url;
        let addr = url_str
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_start_matches("spamd://");

        let host_port = if addr.contains('/') {
            addr.split('/').next().unwrap_or(addr)
        } else {
            addr
        };

        let host_port = if !host_port.contains(':') {
            format!("{}:783", host_port)
        } else {
            host_port.to_string()
        };

        debug!("Connecting to SpamAssassin spamd at {}", host_port);
        let mut stream = TcpStream::connect(&host_port).await?;

        let request = format!(
            "PROCESS SPAMC/1.2\r\nContent-length: {}\r\n\r\n",
            raw_mime.len()
        );

        stream.write_all(request.as_bytes()).await?;
        stream.write_all(raw_mime).await?;
        stream.flush().await?;

        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await?;

        let resp_str = String::from_utf8_lossy(&buf);
        let mut result = SpamScanResult::new();

        // Parse SPAMD response line: SPAMD/1.1 0 EX_OK\r\nSpam: True ; 8.5 / 5.0
        for line in resp_str.lines() {
            if line.to_lowercase().starts_with("spam:") {
                let parts: Vec<&str> = line.split(';').collect();
                if parts.len() >= 2 {
                    let score_part = parts[1].split('/').next().unwrap_or("0").trim();
                    if let Ok(score) = score_part.parse::<f64>() {
                        result.score = score;
                        result
                            .reasons
                            .push(format!("SpamAssassin score: {:.2}", score));
                    }
                }
            }
        }

        Ok(result)
    }
}

/// Composite Multi-Stage Spam Scanner Service
pub struct SpamScannerService {
    config: Arc<AppConfig>,
    external_scanner: ExternalSpamScanner,
}

impl SpamScannerService {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let external_scanner = ExternalSpamScanner::new(config.clone());
        Self {
            config,
            external_scanner,
        }
    }

    pub async fn scan(
        &self,
        raw_mime: &[u8],
        subject: Option<&str>,
        from: Option<&str>,
        body_text: Option<&str>,
        body_html: Option<&str>,
    ) -> SpamScanResult {
        let mut total_result = SpamScanResult::new();

        // Stage 1: Option B (Local Heuristic Scanner)
        if self.config.enable_heuristic_scanner {
            let heuristic_res = HeuristicScanner::scan(subject, from, body_text, body_html);
            total_result.score += heuristic_res.score;
            total_result.reasons.extend(heuristic_res.reasons);
        }

        // Stage 2: Option A (External SpamScanner)
        if self.config.enable_spam_scanner {
            let external_res = self.external_scanner.scan_mime(raw_mime).await;
            total_result.score += external_res.score;
            total_result.reasons.extend(external_res.reasons);
        }

        total_result.is_spam = total_result.score >= self.config.max_spam_score;
        total_result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_scanner_spam_keywords() {
        let result = HeuristicScanner::scan(
            Some("URGENT: CLAIM YOUR REWARD NOW!!!"),
            Some("spammer@evil.com"),
            Some(
                "Send bitcoin to wallet address: 1ABC123 and get 100% free cash! Check bit.ly/123",
            ),
            None,
        );

        assert!(result.score >= 5.0);
        assert!(result.reasons.iter().any(|r| r.contains("URGENT")));
        assert!(
            result
                .reasons
                .iter()
                .any(|r| r.contains("CLAIM YOUR REWARD"))
        );
        assert!(result.reasons.iter().any(|r| r.contains("bit.ly")));
    }

    #[tokio::test]
    async fn test_spam_scanner_service_composite() {
        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
        });

        let service = SpamScannerService::new(config);
        let raw_mime = b"From: spammer@evil.com\r\nTo: user@target.com\r\nSubject: URGENT: WINNER LOTTERY!!!\r\n\r\nCheck bit.ly/free-cash to claim $1000000";

        let result = service
            .scan(
                raw_mime,
                Some("URGENT: WINNER LOTTERY!!!"),
                Some("spammer@evil.com"),
                Some("Check bit.ly/free-cash to claim $1000000"),
                None,
            )
            .await;

        assert!(result.score >= 5.0);
        assert!(result.is_spam);
    }
}
