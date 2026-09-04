//! Where an inbound mail's authentication verdicts come from, and how raw MIME becomes a payload.
//!
//! Both halves used to live inside the SMTP listener, which made the HTTP webhooks import the
//! server module to reach them. They are email-protocol knowledge rather than session knowledge,
//! so they belong here with the rest of it, and every edge -- the listener and each provider
//! webhook -- now reads one copy.
//!
//! The verdicts are the security-relevant half. [`verify_email_authentication`] earns them from
//! DNS against the connecting IP and is what a listener that saw the connection uses.
//! [`AuthenticationResults`] is the other case: a provider that is this deployment's MX saw a
//! connection we never did, and its `Authentication-Results` header is the only account of it that
//! exists. That header is only trustworthy when the deployment can say which `authserv-id` may
//! write one, which is why reading it takes an expected id rather than a `bool`.

use std::net::IpAddr;

use mail_parser::MimeHeaders;

use crate::{
    adapters::protocols::email::parser::{RawAttachmentData, RawInboundPayload, extract_email},
    entities::auth::AuthVerdict,
};

/// DNS-backed authentication shared by direct SMTP and authenticated upstream ingress.
pub async fn verify_email_authentication(
    raw_mime: &[u8],
    mail_from: Option<&str>,
    client_ip: IpAddr,
) -> AuthResults {
    let mut results = AuthResults::default();
    let Ok(resolver) = mail_auth::MessageAuthenticator::new_quad9() else {
        return results;
    };

    let mut spf_output = None;
    if let Some(sender) = mail_from {
        let domain = sender.split('@').nth(1).unwrap_or(sender);
        let spf_res = resolver
            .verify_spf(mail_auth::spf::verify::SpfParameters::verify_mail_from(
                client_ip, domain, domain, sender,
            ))
            .await;
        results.spf = match spf_res.result() {
            mail_auth::SpfResult::Pass => AuthVerdict::Pass,
            mail_auth::SpfResult::Fail => AuthVerdict::Fail,
            mail_auth::SpfResult::SoftFail => AuthVerdict::SoftFail,
            mail_auth::SpfResult::Neutral => AuthVerdict::Neutral,
            mail_auth::SpfResult::TempError => AuthVerdict::TempError,
            mail_auth::SpfResult::PermError => AuthVerdict::PermError,
            _ => AuthVerdict::Unavailable,
        };
        spf_output = Some(spf_res);
    }

    let Some(auth_msg) = mail_auth::AuthenticatedMessage::parse(raw_mime) else {
        return results;
    };
    let dkim_outputs = resolver.verify_dkim(&auth_msg).await;
    if !dkim_outputs.is_empty() {
        results.dkim = if dkim_outputs
            .iter()
            .any(|output| matches!(output.result(), mail_auth::DkimResult::Pass))
        {
            AuthVerdict::Pass
        } else if dkim_outputs
            .iter()
            .any(|output| matches!(output.result(), mail_auth::DkimResult::Fail(_)))
        {
            AuthVerdict::Fail
        } else {
            AuthVerdict::Unavailable
        };
    }

    if let (Some(spf_output), Some(sender)) = (spf_output.as_ref(), mail_from) {
        let domain = sender.split('@').nth(1).unwrap_or(sender);
        let dmarc = resolver
            .verify_dmarc(mail_auth::dmarc::verify::DmarcParameters::new(
                &auth_msg,
                &dkim_outputs,
                domain,
                spf_output,
            ))
            .await;
        results.dmarc = if matches!(dmarc.dkim_result(), mail_auth::DmarcResult::Pass)
            || matches!(dmarc.spf_result(), mail_auth::DmarcResult::Pass)
        {
            AuthVerdict::Pass
        } else {
            AuthVerdict::Fail
        };
    }
    results
}

/// SPF/DKIM/DMARC verdicts from the local DNS verifier.
#[derive(Default)]
pub struct AuthResults {
    pub spf: AuthVerdict,
    pub dkim: AuthVerdict,
    pub dmarc: AuthVerdict,
}

fn extract_address_str(addr: &mail_parser::Address) -> Option<String> {
    match addr {
        mail_parser::Address::List(list) => list
            .first()
            .and_then(|a| a.address.as_deref())
            .map(extract_email),
        mail_parser::Address::Group(groups) => groups
            .first()
            .and_then(|g| g.addresses.first())
            .and_then(|a| a.address.as_deref())
            .map(extract_email),
    }
}

pub fn parse_raw_mime_to_payload(
    raw_mime: &[u8],
    smtp_mail_from: Option<&str>,
    smtp_rcpt_to: Option<&str>,
    _all_rcpts: &[String],
    spf_status: AuthVerdict,
    dkim_status: AuthVerdict,
    dmarc_status: AuthVerdict,
) -> RawInboundPayload {
    if let Some(msg) = mail_parser::MessageParser::new().parse(raw_mime) {
        // Extract sender
        let from = smtp_mail_from
            .filter(|s| !s.is_empty())
            .map(extract_email)
            .or_else(|| msg.from().and_then(extract_address_str))
            .unwrap_or_default();

        // Extract primary recipient ('to')
        let to = smtp_rcpt_to
            .filter(|s| !s.is_empty())
            .map(extract_email)
            .or_else(|| msg.to().and_then(extract_address_str))
            .unwrap_or_default();

        // Extract CC
        let cc = msg.cc().and_then(extract_address_str);

        // Extract subject
        let subject = msg.subject().map(|s| s.to_string());

        // Extract body text and html
        let text = msg.body_text(0).map(|t| t.to_string());
        let html = msg.body_html(0).map(|h| h.to_string());

        // Format headers string & fallback header-based SPF/DKIM extraction
        let headers = {
            let mut hdrs = String::new();
            for header in msg.headers() {
                let name = header.name();
                if let Some(val_str) = header.value().as_text() {
                    hdrs.push_str(name);
                    hdrs.push_str(": ");
                    hdrs.push_str(val_str);
                    hdrs.push('\n');
                } else if let Some(addr) = header.value().as_address()
                    && let Some(addr_str) = extract_address_str(addr)
                {
                    hdrs.push_str(name);
                    hdrs.push_str(": ");
                    hdrs.push_str(&addr_str);
                    hdrs.push('\n');
                }
            }
            if hdrs.is_empty() { None } else { Some(hdrs) }
        };

        // Extract attachments
        let mut attachments_data = Vec::new();
        for att in msg.attachments() {
            let filename = att.attachment_name().unwrap_or("attachment").to_string();
            let content_type = att
                .content_type()
                .map(|c| c.c_type.to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());
            let content = att.contents().to_vec();

            attachments_data.push(RawAttachmentData {
                filename,
                content_type,
                content,
                stored_key: None,
            });
        }

        RawInboundPayload {
            to,
            from,
            cc,
            subject,
            text,
            html,
            headers,
            spf: spf_status,
            dkim: dkim_status,
            dmarc: dmarc_status,
            spam_score: None,
            attachments_data,
        }
    } else {
        let raw_text = String::from_utf8_lossy(raw_mime).to_string();
        RawInboundPayload {
            to: smtp_rcpt_to.map(extract_email).unwrap_or_default(),
            from: smtp_mail_from.map(extract_email).unwrap_or_default(),
            text: Some(raw_text),
            spf: spf_status,
            dkim: dkim_status,
            dmarc: dmarc_status,
            ..Default::default()
        }
    }
}

/// The verdicts a trusted receiving MTA recorded on a message it accepted for us.
///
/// This exists because an inbound provider is a boundary this deployment cannot reproduce. SPF is
/// evaluated against the IP that connected, and when a provider is the MX, that connection
/// happened to the provider; by the time the mail reaches us over HTTPS there is no IP left to
/// check. The provider's `Authentication-Results` header is the only record of it.
///
/// What makes reading it safe is not that the provider is trustworthy in general -- it is that
/// exactly one header in the message is the provider's, and this type will read no other. A sender
/// can put `Authentication-Results: <anything>; dmarc=pass` into the message they compose, and a
/// reader that scanned for the first header claiming a pass would authenticate every forgery it
/// was shown. So: the *first* header wins, because a receiving MTA prepends its own above
/// everything the message arrived with, and its `authserv-id` must be the one this deployment
/// configured. Anything else leaves every verdict [`AuthVerdict::Unknown`], which
/// `guard_ingress` refuses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuthenticationResults {
    pub spf: AuthVerdict,
    pub dkim: AuthVerdict,
    pub dmarc: AuthVerdict,
}

impl AuthenticationResults {
    /// Read the verdicts `expected_authserv_id` recorded on this message, if it recorded any.
    pub fn from_raw_mime(raw_mime: &[u8], expected_authserv_id: &str) -> Self {
        let Some(message) = mail_parser::MessageParser::new().parse(raw_mime) else {
            return Self::default();
        };
        // `headers()` preserves wire order, so the first match is the topmost header -- the one the
        // last MTA to touch the message wrote. Later ones travelled with the message.
        let Some(header) = message
            .headers()
            .iter()
            .find(|header| header.name().eq_ignore_ascii_case("Authentication-Results"))
            .and_then(|header| header.value().as_text())
        else {
            return Self::default();
        };
        Self::parse_header(header, expected_authserv_id)
    }

    /// The header grammar, reduced to what this deployment acts on.
    ///
    /// RFC 8601 allows properties, comments and quoting that no verdict here depends on, so this
    /// reads the leading `authserv-id` and then the `method=result` tokens, and ignores the rest.
    /// A parser that tried to be complete would be a second, larger attack surface for no gain.
    fn parse_header(header: &str, expected_authserv_id: &str) -> Self {
        let mut fields = header.split(';').map(str::trim);
        let Some(authserv_id) = fields.next() else {
            return Self::default();
        };
        // `resend.com 1` -- the optional version follows the id.
        let authserv_id = authserv_id.split_whitespace().next().unwrap_or_default();
        if !authserv_id.eq_ignore_ascii_case(expected_authserv_id) {
            return Self::default();
        }

        let mut results = Self::default();
        for field in fields {
            let mut token = field.split_whitespace();
            let Some((method, verdict)) = token.next().and_then(|pair| pair.split_once('=')) else {
                continue;
            };
            let verdict = AuthVerdict::parse(verdict);
            match method.trim().to_ascii_lowercase().as_str() {
                "spf" => results.spf = verdict,
                "dkim" => results.dkim = verdict,
                "dmarc" => results.dmarc = verdict,
                _ => {}
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHSERV: &str = "resend.com";

    fn mime(headers: &str) -> Vec<u8> {
        format!("{headers}From: sender@example.com\r\nSubject: hi\r\n\r\nbody\r\n").into_bytes()
    }

    #[test]
    fn the_configured_authserv_ids_verdicts_are_read() {
        let results = AuthenticationResults::from_raw_mime(
            &mime(
                "Authentication-Results: resend.com; spf=pass smtp.mailfrom=example.com; dkim=pass header.d=example.com; dmarc=pass header.from=example.com\r\n",
            ),
            AUTHSERV,
        );
        assert_eq!(
            results,
            AuthenticationResults {
                spf: AuthVerdict::Pass,
                dkim: AuthVerdict::Pass,
                dmarc: AuthVerdict::Pass,
            }
        );
    }

    /// The case the whole type exists for. A sender composes a message already carrying a header
    /// that claims every check passed; the receiving MTA prepends its own verdict above it. Only
    /// the top one may be read, or every forgery authenticates itself.
    #[test]
    fn a_forged_header_beneath_the_receiving_mtas_own_is_never_read() {
        let results = AuthenticationResults::from_raw_mime(
            &mime(
                "Authentication-Results: resend.com; spf=fail; dkim=fail; dmarc=fail\r\n\
                 Authentication-Results: resend.com; spf=pass; dkim=pass; dmarc=pass\r\n",
            ),
            AUTHSERV,
        );
        assert_eq!(results.dmarc, AuthVerdict::Fail);
    }

    #[test]
    fn a_header_from_any_other_authserv_id_asserts_nothing() {
        for header in [
            "Authentication-Results: mx.attacker.example; spf=pass; dkim=pass; dmarc=pass\r\n",
            "Authentication-Results: resend.com.attacker.example; dmarc=pass\r\n",
            "Authentication-Results: ; dmarc=pass\r\n",
        ] {
            assert_eq!(
                AuthenticationResults::from_raw_mime(&mime(header), AUTHSERV),
                AuthenticationResults::default(),
                "{header} must assert nothing"
            );
        }
    }

    #[test]
    fn a_missing_or_unreadable_header_leaves_every_verdict_unknown() {
        assert_eq!(
            AuthenticationResults::from_raw_mime(&mime(""), AUTHSERV),
            AuthenticationResults::default()
        );
        assert_eq!(
            AuthenticationResults::from_raw_mime(b"", AUTHSERV),
            AuthenticationResults::default()
        );
        // Unknown is the default, so a partial header reports only what it actually said.
        let partial = AuthenticationResults::from_raw_mime(
            &mime("Authentication-Results: resend.com; dkim=pass\r\n"),
            AUTHSERV,
        );
        assert_eq!(partial.dkim, AuthVerdict::Pass);
        assert_eq!(partial.dmarc, AuthVerdict::Unknown);
        assert_eq!(partial.spf, AuthVerdict::Unknown);
    }

    #[test]
    fn an_authserv_id_may_carry_the_optional_version_token() {
        let results = AuthenticationResults::from_raw_mime(
            &mime("Authentication-Results: resend.com 1; dmarc=pass\r\n"),
            AUTHSERV,
        );
        assert_eq!(results.dmarc, AuthVerdict::Pass);
    }
}
