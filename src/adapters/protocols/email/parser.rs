//! MIME and RFC 5322 parsing: the syntax only mail has.
//!
//! This lives in the email adapter rather than in `src/application/services` because everything it
//! produces is email-shaped. [`ParsedEmail`] is the adapter's own working value -- it never leaves
//! this module's crate neighbourhood, and no application or domain function accepts or returns
//! one. What crosses the boundary is the [`InboundEnvelope`](crate::transport::InboundEnvelope)
//! that [`super::ingress`] builds out of it.

use htmd::HtmlToMarkdown;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    entities::{
        auth::AuthVerdict,
        channel::RESERVED_SLUG_SUFFIXES,
        correlation::{CORRELATION_HEADER, CorrelationId},
        message::AttachmentMetadata,
        value_objects::{ObjectKey, ThreadIndex, ThreadIndexParseError},
    },
    transport::SMALL_INLINE_IMAGE_BYTES,
};

use serde::{Deserialize, Serialize};

/// Maximum untouched RFC 5322 message accepted by every public mail ingress.
pub const MAX_INBOUND_MESSAGE_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedEmail {
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// The canonicalized Outlook conversation index, when the header carried a usable one.
    pub thread_index: Option<ThreadIndex>,
    /// Why a `Thread-Index` header was discarded, so the caller can count it. Parsing is the
    /// adapter's job; deciding whether a rejected header is worth a metric is not.
    #[serde(skip)]
    pub thread_index_rejection: Option<ThreadIndexParseError>,
    /// How long the discarded header was, for the same gauge.
    #[serde(skip)]
    pub thread_index_raw_bytes: usize,
    /// The chain this message already belongs to, or a fresh one. Read from the correlation header
    /// here so nothing downstream parses headers a second time.
    pub correlation_id: CorrelationId,
    pub sender: String,
    pub recipients_to: Vec<String>,
    pub recipients_cc: Vec<String>,
    pub subject: String,
    pub clean_text_body: String,
    pub raw_text_body: Option<String>,
    pub raw_html_body: Option<String>,
    pub attachments: Vec<AttachmentMetadata>,
    pub prompt_text: String,
    pub is_auto_reply: bool,
    pub is_forwarded: bool,
    pub channel_id_header: Option<Uuid>,
    pub thread_id_header: Option<Uuid>,
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
    pub spf_status: AuthVerdict,
    pub dkim_status: AuthVerdict,
    pub dmarc_status: AuthVerdict,
    pub spam_score: Option<f64>,
    pub is_context_only: bool,
}

#[derive(Default)]
struct ParsedHeaders {
    message_id: Option<String>,
    in_reply_to: Option<String>,
    references: Vec<String>,
    thread_index: Option<String>,
    is_auto_reply: bool,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    hop_count: u32,
    trace_channels: Vec<Uuid>,
    is_context_only: bool,
    correlation_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RawInboundPayload {
    pub to: String,
    pub from: String,
    pub cc: Option<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub headers: Option<String>,
    pub spf: AuthVerdict,
    pub dkim: AuthVerdict,
    pub dmarc: AuthVerdict,
    pub spam_score: Option<f64>,
    pub attachments_data: Vec<RawAttachmentData>,
}

impl Default for RawInboundPayload {
    fn default() -> Self {
        Self {
            to: String::new(),
            from: String::new(),
            cc: None,
            subject: None,
            text: None,
            html: None,
            headers: None,
            spf: default_test_auth_verdict(),
            dkim: default_test_auth_verdict(),
            dmarc: default_test_auth_verdict(),
            spam_score: None,
            attachments_data: Vec::new(),
        }
    }
}

#[cfg(not(test))]
fn default_test_auth_verdict() -> AuthVerdict {
    AuthVerdict::Unknown
}

/// Existing unit fixtures represent mail that has already crossed the verifier boundary.
#[cfg(test)]
fn default_test_auth_verdict() -> AuthVerdict {
    AuthVerdict::Pass
}

#[derive(Debug, Clone)]
pub struct RawAttachmentData {
    pub filename: String,
    pub content_type: String,
    pub content: Vec<u8>,
    /// Where the bytes were put, once they have been; see
    /// [`crate::services::attachment_store::store_inbound_attachments`].
    pub stored_key: Option<ObjectKey>,
}

pub struct EmailParser;

impl EmailParser {
    pub fn check_body_context_trigger(text: &str) -> (bool, String) {
        let trimmed = text.trim_start();
        let lower = trimmed.to_lowercase();

        for suffix in RESERVED_SLUG_SUFFIXES {
            let double_bracket = format!("[[{}]]", suffix);
            let single_bracket = format!("[{}]", suffix);

            if lower.starts_with(&double_bracket) {
                let remaining = trimmed[double_bracket.len()..].trim_start();
                return (true, remaining.to_string());
            } else if lower.starts_with(&single_bracket) {
                let remaining = trimmed[single_bracket.len()..].trim_start();
                return (true, remaining.to_string());
            }
        }

        (false, text.to_string())
    }

    pub fn parse(
        mut payload: RawInboundPayload,
        app_domain: &str,
    ) -> (ParsedEmail, Vec<RawAttachmentData>) {
        let attachments_data = std::mem::take(&mut payload.attachments_data);
        let ParsedHeaders {
            message_id: extracted_msg_id,
            in_reply_to,
            references,
            thread_index: raw_thread_index,
            is_auto_reply: is_auto_reply_from_headers,
            channel_id: channel_id_header,
            thread_id: thread_id_header,
            hop_count,
            trace_channels,
            is_context_only: is_context_from_headers,
            correlation_id: supplied_correlation_id,
        } = payload
            .headers
            .as_deref()
            .map(Self::parse_headers)
            .unwrap_or_default();

        let message_id = extracted_msg_id
            .unwrap_or_else(|| format!("<{}.inbound@{}>", Uuid::new_v4(), app_domain));
        let thread_index_raw_bytes = raw_thread_index.as_ref().map_or(0, String::len);
        let (thread_index, thread_index_rejection) = match raw_thread_index
            .as_deref()
            .map(ThreadIndex::parse)
            .transpose()
        {
            Ok(index) => (index, None),
            Err(error) => (None, Some(error)),
        };

        let sender = extract_email(&payload.from);
        let recipients_to = parse_email_list(&payload.to);
        let recipients_cc = payload
            .cc
            .as_deref()
            .map(parse_email_list)
            .unwrap_or_default();

        let subject = payload.subject.unwrap_or_else(|| "No Subject".to_string());
        let is_auto_reply_from_subject = Self::is_auto_reply_subject(&subject);
        let is_auto_reply = is_auto_reply_from_headers || is_auto_reply_from_subject;

        let raw_text = payload.text.clone();
        let raw_html = payload.html.clone();

        // Check if forwarded email
        let is_forwarded =
            Self::is_forwarded_email(&subject, raw_text.as_deref().unwrap_or_default());

        // Convert HTML to Markdown if text is missing or as fallback
        let base_text = if let Some(ref text) = raw_text {
            if !text.trim().is_empty() {
                text.clone()
            } else if let Some(ref html) = raw_html {
                Self::html_to_markdown(html)
            } else {
                String::new()
            }
        } else if let Some(ref html) = raw_html {
            Self::html_to_markdown(html)
        } else {
            String::new()
        };

        // Preserve full text in clean_text_body; quote stripping is applied during thread ingestion
        // if the email is a reply in an existing thread and not forwarded.
        let base_clean_text = base_text.trim().to_string();
        let (is_context_from_body, clean_text_body) =
            Self::check_body_context_trigger(&base_clean_text);
        let is_context_only = is_context_from_headers || is_context_from_body;

        // Process attachments - Filter small inline signature images (< 10KB images)
        let mut attachments = Vec::new();
        let mut attachment_prompts = Vec::new();

        for att in &attachments_data {
            let size = att.content.len();
            let is_small_image = att.content_type.to_lowercase().starts_with("image/")
                && size < SMALL_INLINE_IMAGE_BYTES;

            let mut hasher = Sha256::new();
            hasher.update(&att.content);
            let hash_hex = format!("{:x}", hasher.finalize());

            let meta = AttachmentMetadata {
                filename: att.filename.clone(),
                content_type: att.content_type.clone(),
                sha256_hash: hash_hex.clone(),
                size_bytes: size,
                storage_key: att.stored_key.clone(),
            };

            attachments.push(meta.clone());

            // Only add document/significant attachments to prompt context
            if !is_small_image {
                attachment_prompts.push(format!(
                    "[Attachment: {}, Type: {}, SHA256: {}, Size: {} bytes]",
                    meta.filename, meta.content_type, hash_hex, size
                ));
            }
        }

        let prompt_text = if attachment_prompts.is_empty() {
            clean_text_body.clone()
        } else {
            format!("{}\n\n{}", clean_text_body, attachment_prompts.join("\n"))
        };

        let parsed = ParsedEmail {
            message_id,
            in_reply_to,
            references,
            thread_index,
            thread_index_rejection,
            thread_index_raw_bytes,
            correlation_id: CorrelationId::parse_or_new(supplied_correlation_id.as_deref()),
            sender,
            recipients_to,
            recipients_cc,
            subject,
            clean_text_body,
            raw_text_body: raw_text,
            raw_html_body: raw_html,
            attachments,
            prompt_text,
            is_auto_reply,
            is_forwarded,
            channel_id_header,
            thread_id_header,
            hop_count,
            trace_channels,
            spf_status: payload.spf,
            dkim_status: payload.dkim,
            dmarc_status: payload.dmarc,
            spam_score: payload.spam_score,
            is_context_only,
        };
        (parsed, attachments_data)
    }

    pub fn html_to_markdown(html: &str) -> String {
        let converter = HtmlToMarkdown::new();
        converter.convert(html).unwrap_or_else(|_| html.to_string())
    }

    fn parse_headers(headers_str: &str) -> ParsedHeaders {
        let mut message_id = None;
        let mut in_reply_to = None;
        let mut references = Vec::new();
        let mut thread_index: Option<String> = None;
        let mut is_auto_reply = false;
        let mut channel_id_header = None;
        let mut thread_id_header = None;
        let mut hop_count = 0u32;
        let mut trace_channels = Vec::new();
        let mut is_context_only = false;
        let mut correlation_id = None;

        let correlation_header = CORRELATION_HEADER.to_lowercase();
        let mut collecting_thread_index = false;
        for raw_line in headers_str.split('\n') {
            if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
                if collecting_thread_index && let Some(value) = thread_index.as_mut() {
                    // Preserve folding whitespace for `ThreadIndex::parse`: its raw-length guard
                    // must run before RFC 2045 whitespace is removed.
                    value.push('\n');
                    value.push_str(raw_line);
                }
                continue;
            }

            collecting_thread_index = false;
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            let lower = line.to_lowercase();

            if lower.starts_with("message-id:") {
                message_id = Some(line["message-id:".len()..].trim().to_string());
            } else if lower.starts_with("in-reply-to:") {
                in_reply_to = Some(line["in-reply-to:".len()..].trim().to_string());
            } else if lower.starts_with("references:") {
                let refs_str = line["references:".len()..].trim();
                references = refs_str
                    .split_whitespace()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            } else if lower.starts_with("thread-index:") {
                // Keep the untouched field body. Its protocol parser owns both the raw bound and
                // RFC 2045 whitespace removal, in that order.
                thread_index = Some(raw_line["thread-index:".len()..].to_string());
                collecting_thread_index = true;
            } else if lower.starts_with("x-mailagents-channel-id:") {
                let val = line["x-mailagents-channel-id:".len()..].trim();
                channel_id_header = Uuid::from_str(val).ok();
            } else if lower.starts_with("x-mailagents-thread-id:") {
                let val = line["x-mailagents-thread-id:".len()..].trim();
                thread_id_header = Uuid::from_str(val).ok();
            } else if lower.starts_with("x-mailagents-hop-count:") {
                let val = line["x-mailagents-hop-count:".len()..].trim();
                hop_count = val.parse().unwrap_or(0);
            } else if lower.starts_with("x-mailagents-trace:") {
                let val = line["x-mailagents-trace:".len()..].trim();
                trace_channels = val
                    .split(',')
                    .filter_map(|s| Uuid::from_str(s.trim()).ok())
                    .collect();
            } else if lower.starts_with("auto-submitted:") {
                let val = line["auto-submitted:".len()..].trim().to_lowercase();
                if val != "no" {
                    is_auto_reply = true;
                }
            } else if lower.starts_with("x-autoreply:") || lower.starts_with("x-autorespond:") {
                is_auto_reply = true;
            } else if lower.starts_with("precedence:") || lower.starts_with("x-precedence:") {
                let val = line
                    .split(':')
                    .nth(1)
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase();
                if val == "bulk" || val == "junk" || val == "auto_reply" {
                    is_auto_reply = true;
                }
            } else if lower.starts_with("x-auto-response-suppress:") {
                is_auto_reply = true;
            } else if lower.starts_with(&format!("{correlation_header}:")) {
                correlation_id = Some(line[correlation_header.len() + 1..].trim().to_string());
            } else if lower.starts_with("x-mailagents-context-only:")
                || lower.starts_with("x-no-agent:")
                || lower.starts_with("x-context-only:")
                || lower.starts_with("x-quiet:")
            {
                let val = line
                    .split(':')
                    .nth(1)
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase();
                if val == "true" || val == "1" || val == "yes" || val.is_empty() {
                    is_context_only = true;
                }
            }
        }

        ParsedHeaders {
            message_id,
            in_reply_to,
            references,
            thread_index,
            is_auto_reply,
            channel_id: channel_id_header,
            thread_id: thread_id_header,
            hop_count,
            trace_channels,
            is_context_only,
            correlation_id,
        }
    }

    pub fn is_auto_reply_subject(subject: &str) -> bool {
        let lower = subject.trim().to_lowercase();
        lower.starts_with("auto-reply:")
            || lower.starts_with("autoreply:")
            || lower.starts_with("out of office:")
            || lower.starts_with("automatic reply:")
            || lower.starts_with("autoresponse:")
            || lower.contains("out of office response")
    }

    pub fn is_forwarded_email(subject: &str, body: &str) -> bool {
        let lower_subj = subject.trim().to_lowercase();
        if lower_subj.starts_with("fwd:")
            || lower_subj.starts_with("fw:")
            || lower_subj.starts_with("forward:")
        {
            return true;
        }

        let lower_body = body.to_lowercase();
        lower_body.contains("---------- forwarded message ---------")
            || lower_body.contains("-----original message-----")
    }
}

pub fn extract_email(input: &str) -> String {
    if let (Some(start), Some(end)) = (input.find('<'), input.rfind('>'))
        && start < end
    {
        return input[start + 1..end].trim().to_lowercase();
    }
    input.trim().to_lowercase()
}

pub fn parse_email_list(input: &str) -> Vec<String> {
    input
        .split([',', ';'])
        .map(extract_email)
        .filter(|e| !e.is_empty() && e.contains('@'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_headers_and_auto_reply() {
        let headers = r#"
Host: in.sendgrid.net
Message-ID: <CAGX123@mail.gmail.com>
In-Reply-To: <ORIGINAL456@mailagents.com>
References: <REF1@mailagents.com> <REF2@mailagents.com>
Auto-Submitted: auto-replied
X-MailAgents-Channel-ID: 00000000-0000-0000-0000-000000000001
X-MailAgents-Hop-Count: 2
Subject: Test Email
"#;

        let parsed = EmailParser::parse_headers(headers);
        assert_eq!(
            parsed.message_id.as_deref(),
            Some("<CAGX123@mail.gmail.com>")
        );
        assert_eq!(
            parsed.in_reply_to.as_deref(),
            Some("<ORIGINAL456@mailagents.com>")
        );
        assert_eq!(
            parsed.references,
            vec!["<REF1@mailagents.com>", "<REF2@mailagents.com>"]
        );
        assert!(parsed.thread_index.is_none());
        assert!(parsed.is_auto_reply);
        assert_eq!(
            parsed.channel_id,
            Some(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
        );
        assert_eq!(parsed.hop_count, 2);
        assert!(parsed.trace_channels.is_empty());
        assert!(!parsed.is_context_only);
    }

    #[test]
    fn thread_index_header_preserves_folding_for_bounded_canonicalization() {
        use base64::{Engine, engine::general_purpose::STANDARD};

        let mut bytes: Vec<u8> = (0..27).map(|value| value as u8).collect();
        bytes[0] = 0x01;
        let canonical = STANDARD.encode(bytes);
        let headers = format!(
            "Thread-Index: {}\r\n\t{}  \r\nSubject: reply",
            &canonical[..20],
            &canonical[20..]
        );
        let parsed = EmailParser::parse_headers(&headers);
        let raw = parsed.thread_index.unwrap();

        assert!(raw.contains("\r\n\t"));
        assert_eq!(
            ThreadIndex::parse(raw.as_str()).unwrap().as_str(),
            canonical
        );
    }

    #[test]
    fn test_body_and_header_context_trigger() {
        let text1 = "[[quiet]] This is background context for the thread.";
        let (is_ctx1, clean1) = EmailParser::check_body_context_trigger(text1);
        assert!(is_ctx1);
        assert_eq!(clean1, "This is background context for the thread.");

        let text2 = "[noagent] Additional note.";
        let (is_ctx2, clean2) = EmailParser::check_body_context_trigger(text2);
        assert!(is_ctx2);
        assert_eq!(clean2, "Additional note.");

        let text3 = "Normal message to agent.";
        let (is_ctx3, clean3) = EmailParser::check_body_context_trigger(text3);
        assert!(!is_ctx3);
        assert_eq!(clean3, "Normal message to agent.");

        let headers = "X-MailAgents-Context-Only: true\n";
        assert!(EmailParser::parse_headers(headers).is_context_only);
    }

    #[test]
    fn test_forwarded_email_detection() {
        assert!(EmailParser::is_forwarded_email(
            "Fwd: Customer Issue",
            "Original message text"
        ));
        assert!(EmailParser::is_forwarded_email(
            "FW: Urgent",
            "Original text"
        ));
        assert!(!EmailParser::is_forwarded_email(
            "Normal Subject",
            "Hello agent"
        ));
    }

    #[test]
    fn test_html_to_markdown() {
        let html = "<h1>Title</h1><p>This is a <strong>bold</strong> statement.</p>";
        let md = EmailParser::html_to_markdown(html);
        assert!(md.contains("# Title"));
        assert!(md.contains("**bold**"));
    }
}
