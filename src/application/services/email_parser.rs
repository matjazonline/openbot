use htmd::HtmlToMarkdown;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use uuid::Uuid;

use crate::entities::message::AttachmentMetadata;

use serde::{Deserialize, Serialize};

pub const MAX_CHANNEL_HOPS: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedEmail {
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub thread_index: Option<String>,
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
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
    pub spf_status: Option<String>,
    pub dkim_status: Option<String>,
    pub dmarc_status: Option<String>,
    pub spam_score: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct RawInboundPayload {
    pub to: String,
    pub from: String,
    pub cc: Option<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub headers: Option<String>,
    pub spf: Option<String>,
    pub dkim: Option<String>,
    pub dmarc: Option<String>,
    pub spam_score: Option<f64>,
    pub attachments_data: Vec<RawAttachmentData>,
}

#[derive(Debug, Clone)]
pub struct RawAttachmentData {
    pub filename: String,
    pub content_type: String,
    pub content: Vec<u8>,
}

pub struct EmailParser;

impl EmailParser {
    pub fn parse(payload: RawInboundPayload, app_domain: &str) -> ParsedEmail {
        let (
            extracted_msg_id,
            in_reply_to,
            references,
            thread_index,
            is_auto_reply_from_headers,
            channel_id_header,
            hop_count,
            trace_channels,
        ) = if let Some(ref hdrs) = payload.headers {
            Self::parse_headers(hdrs)
        } else {
            (None, None, Vec::new(), None, false, None, 0, Vec::new())
        };

        let message_id = extracted_msg_id
            .unwrap_or_else(|| format!("<{}.inbound@{}>", Uuid::new_v4(), app_domain));

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
        let clean_text_body = base_text.trim().to_string();

        // Process attachments - Filter small inline signature images (< 10KB images)
        let mut attachments = Vec::new();
        let mut attachment_prompts = Vec::new();

        for att in payload.attachments_data {
            let size = att.content.len();
            let is_small_image =
                att.content_type.to_lowercase().starts_with("image/") && size < 10_000;

            let mut hasher = Sha256::new();
            hasher.update(&att.content);
            let hash_hex = format!("{:x}", hasher.finalize());

            let meta = AttachmentMetadata {
                filename: att.filename.clone(),
                content_type: att.content_type.clone(),
                sha256_hash: hash_hex.clone(),
                size_bytes: size,
                storage_url: None,
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

        ParsedEmail {
            message_id,
            in_reply_to,
            references,
            thread_index,
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
            hop_count,
            trace_channels,
            spf_status: payload.spf,
            dkim_status: payload.dkim,
            dmarc_status: payload.dmarc,
            spam_score: payload.spam_score,
        }
    }

    pub fn html_to_markdown(html: &str) -> String {
        let converter = HtmlToMarkdown::new();
        converter.convert(html).unwrap_or_else(|_| html.to_string())
    }

    pub fn parse_headers(
        headers_str: &str,
    ) -> (
        Option<String>,
        Option<String>,
        Vec<String>,
        Option<String>,
        bool,
        Option<Uuid>,
        u32,
        Vec<Uuid>,
    ) {
        let mut message_id = None;
        let mut in_reply_to = None;
        let mut references = Vec::new();
        let mut thread_index = None;
        let mut is_auto_reply = false;
        let mut channel_id_header = None;
        let mut hop_count = 0u32;
        let mut trace_channels = Vec::new();

        for line in headers_str.lines() {
            let line = line.trim();
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
                thread_index = Some(line["thread-index:".len()..].trim().to_string());
            } else if lower.starts_with("x-mailagents-channel-id:") {
                let val = line["x-mailagents-channel-id:".len()..].trim();
                channel_id_header = Uuid::from_str(val).ok();
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
            }
        }

        (
            message_id,
            in_reply_to,
            references,
            thread_index,
            is_auto_reply,
            channel_id_header,
            hop_count,
            trace_channels,
        )
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

    pub fn strip_quotes_heuristic(text: &str) -> String {
        let mut result_lines = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();

            // Blockquote marker
            if trimmed.starts_with('>') {
                break;
            }

            // Divider / Original Message splitters
            let lower = trimmed.to_lowercase();
            if lower.contains("-----original message-----")
                || lower.contains("-----forwarded message-----")
                || lower.starts_with("___________")
            {
                break;
            }

            // Pattern: On <date>, <user> wrote:
            if (lower.starts_with("on ") || lower.starts_with("am ") || lower.starts_with("le "))
                && lower.ends_with("wrote:")
            {
                break;
            }

            // Header blocks in forwards/replies
            if lower.starts_with("from:")
                && (lower.contains("sent:") || lower.contains("date:") || lower.contains("to:"))
            {
                break;
            }

            result_lines.push(line);
        }

        result_lines.join("\n").trim().to_string()
    }

    /// Fallback quote stripping by subtracting previous DB thread message lines from new message
    pub fn strip_historical_quotes_fallback(
        clean_text: &str,
        history_clean_bodies: &[String],
    ) -> String {
        if history_clean_bodies.is_empty() {
            return clean_text.to_string();
        }

        let new_lines: Vec<&str> = clean_text.lines().collect();
        let mut filtered_lines = Vec::new();

        for line in new_lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                filtered_lines.push(line);
                continue;
            }

            // Check if this line exists verbatim in previous messages
            let exists_in_history = history_clean_bodies.iter().any(|prev_body| {
                prev_body
                    .lines()
                    .any(|prev_line| prev_line.trim() == trimmed)
            });

            if exists_in_history {
                // We reached quoted text from history
                break;
            }

            filtered_lines.push(line);
        }

        filtered_lines.join("\n").trim().to_string()
    }
}

pub fn extract_email(input: &str) -> String {
    if let (Some(start), Some(end)) = (input.find('<'), input.rfind('>')) {
        if start < end {
            return input[start + 1..end].trim().to_lowercase();
        }
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

        let (msg_id, in_reply, refs, thread_idx, is_auto, wf_id, hop, trace) =
            EmailParser::parse_headers(headers);
        assert_eq!(msg_id.as_deref(), Some("<CAGX123@mail.gmail.com>"));
        assert_eq!(in_reply.as_deref(), Some("<ORIGINAL456@mailagents.com>"));
        assert_eq!(refs, vec!["<REF1@mailagents.com>", "<REF2@mailagents.com>"]);
        assert!(thread_idx.is_none());
        assert!(is_auto);
        assert_eq!(
            wf_id,
            Some(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
        );
        assert_eq!(hop, 2);
        assert!(trace.is_empty());
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
    fn test_quote_stripping_heuristic() {
        let text = r#"Hello agent!
Can you summarize this report?

On Mon, Aug 10, 2026 at 10:00 AM User <user@example.com> wrote:
> Older email content here...
> Multi line blockquote
"#;

        let cleaned = EmailParser::strip_quotes_heuristic(text);
        assert_eq!(cleaned, "Hello agent!\nCan you summarize this report?");
    }

    #[test]
    fn test_quote_stripping_fallback() {
        let new_text = "Thanks for the update!\n\nHello agent!\nCan you summarize this report?";
        let history = vec!["Hello agent!\nCan you summarize this report?".to_string()];

        let cleaned = EmailParser::strip_historical_quotes_fallback(new_text, &history);
        assert_eq!(cleaned, "Thanks for the update!");
    }

    #[test]
    fn test_html_to_markdown() {
        let html = "<h1>Title</h1><p>This is a <strong>bold</strong> statement.</p>";
        let md = EmailParser::html_to_markdown(html);
        assert!(md.contains("# Title"));
        assert!(md.contains("**bold**"));
    }
}
