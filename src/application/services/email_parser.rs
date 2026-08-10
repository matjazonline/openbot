use htmd::HtmlToMarkdown;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entities::message::AttachmentMetadata;

#[derive(Debug, Clone)]
pub struct ParsedEmail {
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub sender: String,
    pub recipients_to: Vec<String>,
    pub recipients_cc: Vec<String>,
    pub subject: String,
    pub clean_text_body: String,
    pub raw_text_body: Option<String>,
    pub raw_html_body: Option<String>,
    pub attachments: Vec<AttachmentMetadata>,
    pub prompt_text: String,
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
        let (extracted_msg_id, in_reply_to, references) = if let Some(ref hdrs) = payload.headers {
            Self::parse_headers(hdrs)
        } else {
            (None, None, Vec::new())
        };

        let message_id = extracted_msg_id.unwrap_or_else(|| {
            format!("<{}.inbound@{}>", Uuid::new_v4(), app_domain)
        });

        let sender = extract_email(&payload.from);
        let recipients_to = parse_email_list(&payload.to);
        let recipients_cc = payload
            .cc
            .as_deref()
            .map(parse_email_list)
            .unwrap_or_default();

        let subject = payload.subject.unwrap_or_else(|| "No Subject".to_string());
        let raw_text = payload.text.clone();
        let raw_html = payload.html.clone();

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

        // Strip quotes via heuristic
        let clean_text_body = Self::strip_quotes_heuristic(&base_text);

        // Process attachments
        let mut attachments = Vec::new();
        let mut attachment_prompts = Vec::new();

        for att in payload.attachments_data {
            let mut hasher = Sha256::new();
            hasher.update(&att.content);
            let hash_hex = format!("{:x}", hasher.finalize());
            let size = att.content.len();

            let meta = AttachmentMetadata {
                filename: att.filename.clone(),
                content_type: att.content_type.clone(),
                sha256_hash: hash_hex.clone(),
                size_bytes: size,
                storage_url: None,
            };

            attachment_prompts.push(format!(
                "[Attachment: {}, Type: {}, SHA256: {}, Size: {} bytes]",
                meta.filename, meta.content_type, hash_hex, size
            ));

            attachments.push(meta);
        }

        let prompt_text = if attachment_prompts.is_empty() {
            clean_text_body.clone()
        } else {
            format!(
                "{}\n\n{}",
                clean_text_body,
                attachment_prompts.join("\n")
            )
        };

        ParsedEmail {
            message_id,
            in_reply_to,
            references,
            sender,
            recipients_to,
            recipients_cc,
            subject,
            clean_text_body,
            raw_text_body: raw_text,
            raw_html_body: raw_html,
            attachments,
            prompt_text,
        }
    }

    pub fn html_to_markdown(html: &str) -> String {
        let converter = HtmlToMarkdown::new();
        converter.convert(html).unwrap_or_else(|_| html.to_string())
    }

    pub fn parse_headers(headers_str: &str) -> (Option<String>, Option<String>, Vec<String>) {
        let mut message_id = None;
        let mut in_reply_to = None;
        let mut references = Vec::new();

        for line in headers_str.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if line.to_lowercase().starts_with("message-id:") {
                message_id = Some(line["message-id:".len()..].trim().to_string());
            } else if line.to_lowercase().starts_with("in-reply-to:") {
                in_reply_to = Some(line["in-reply-to:".len()..].trim().to_string());
            } else if line.to_lowercase().starts_with("references:") {
                let refs_str = line["references:".len()..].trim();
                references = refs_str
                    .split_whitespace()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        (message_id, in_reply_to, references)
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
    pub fn strip_historical_quotes_fallback(clean_text: &str, history_clean_bodies: &[String]) -> String {
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
                prev_body.lines().any(|prev_line| prev_line.trim() == trimmed)
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
    fn test_parse_headers() {
        let headers = r#"
Host: in.sendgrid.net
Message-ID: <CAGX123@mail.gmail.com>
In-Reply-To: <ORIGINAL456@mailagents.com>
References: <REF1@mailagents.com> <REF2@mailagents.com>
Subject: Test Email
"#;

        let (msg_id, in_reply, refs) = EmailParser::parse_headers(headers);
        assert_eq!(msg_id.as_deref(), Some("<CAGX123@mail.gmail.com>"));
        assert_eq!(in_reply.as_deref(), Some("<ORIGINAL456@mailagents.com>"));
        assert_eq!(refs, vec!["<REF1@mailagents.com>", "<REF2@mailagents.com>"]);
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
