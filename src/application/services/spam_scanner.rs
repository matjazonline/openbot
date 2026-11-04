//! Application-owned spam-scanning boundary; network clients live in adapters.
use async_trait::async_trait;
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

/// Borrowed message facts; the scanner neither owns nor logs message bytes.
#[derive(Clone, Copy)]
pub struct SpamScanInput<'a> {
    pub raw_mime: &'a [u8],
    pub subject: Option<&'a str>,
    pub from: Option<&'a str>,
    pub body_text: Option<&'a str>,
    pub body_html: Option<&'a str>,
}
#[async_trait]
pub trait SpamScanner: Send + Sync {
    async fn scan(&self, input: SpamScanInput<'_>) -> SpamScanResult;
}
