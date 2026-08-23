//! Shared, mostly-pure helpers for the inbound ingest and agent dispatch pipelines.
//!
//! Everything here is either a pure decision (no `self`, no I/O) or a memoizing wrapper around
//! persistence lookups. Keeping them out of the pipeline bodies is what lets those bodies read as
//! a sequence of named phases.

use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        agent::Agent,
        channel::Channel,
        company::Company,
        company_member::CompanyMembership,
        message::{AttachmentMetadata, Message},
        message_contract::NormalizedInboundMessage,
        value_objects::{CompanySlug, MessageId, ThreadIndex},
    },
    services::email_parser::{
        EmailParser, MAX_CHANNEL_HOPS, ParsedEmail, SMALL_INLINE_IMAGE_BYTES,
    },
};

use super::{InboundIngestResult, InternalChannelSource, ThreadUseCases};

/// Memoized directory lookups for a single ingest run.
///
/// The ingest pipeline resolves the same company, channel list and team membership repeatedly
/// while walking `To`/`Cc` pipelines. This owns those caches so the pipeline body never
/// hand-rolls a `get`/`insert` dance, and so every cache key is built in exactly one place.
pub(super) struct DirectoryCache<'a> {
    use_cases: &'a ThreadUseCases,
    companies: HashMap<String, Option<Company>>,
    channels: HashMap<Uuid, Vec<Channel>>,
    memberships: HashMap<(Uuid, String), CompanyMembership>,
    agents: HashMap<Uuid, Option<Agent>>,
}

impl<'a> DirectoryCache<'a> {
    pub(super) fn new(use_cases: &'a ThreadUseCases) -> Self {
        Self {
            use_cases,
            companies: HashMap::new(),
            channels: HashMap::new(),
            memberships: HashMap::new(),
            agents: HashMap::new(),
        }
    }

    pub(super) async fn company(&mut self, slug: &CompanySlug) -> AppResult<Option<Company>> {
        let key = slug.to_lowercase();
        if let Some(cached) = self.companies.get(&key) {
            return Ok(cached.clone());
        }
        let loaded = self.use_cases.company_persistence.get_by_slug(slug).await?;
        self.companies.insert(key, loaded.clone());
        Ok(loaded)
    }

    pub(super) async fn channels(&mut self, company_id: Uuid) -> AppResult<Vec<Channel>> {
        if let Some(cached) = self.channels.get(&company_id) {
            return Ok(cached.clone());
        }
        let loaded = self
            .use_cases
            .channel_persistence
            .list_by_company_id(company_id)
            .await?;
        self.channels.insert(company_id, loaded.clone());
        Ok(loaded)
    }

    /// What the sender is to the company, for `Channel::participant_access`.
    ///
    /// Membership feeds authorization decisions, so a persistence failure propagates rather than
    /// silently degrading the sender to a stranger.
    pub(super) async fn membership(
        &mut self,
        company_id: Uuid,
        sender: &str,
    ) -> AppResult<CompanyMembership> {
        let key = (company_id, sender.trim().to_lowercase());
        if let Some(cached) = self.memberships.get(&key) {
            return Ok(*cached);
        }
        let loaded = self
            .use_cases
            .company_persistence
            .membership_for_email(company_id, sender.trim())
            .await?;
        self.memberships.insert(key, loaded);
        Ok(loaded)
    }

    /// One configured agent, cached because several channel matches may reference the same
    /// library definition.
    pub(super) async fn agent(&mut self, id: Uuid) -> AppResult<Option<Agent>> {
        if let Some(cached) = self.agents.get(&id) {
            return Ok(cached.clone());
        }
        let Some(persistence) = self.use_cases.agent_persistence() else {
            return Ok(None);
        };
        let loaded = persistence.get_by_id(id).await?;
        self.agents.insert(id, loaded.clone());
        Ok(loaded)
    }
}

/// Message-IDs that point at *other* messages in the conversation (`In-Reply-To` + `References`).
pub(super) fn reference_ids(parsed: &ParsedEmail) -> Vec<MessageId> {
    let mut ids = Vec::with_capacity(parsed.references.len() + 1);
    if let Some(ref reply_id) = parsed.in_reply_to {
        ids.push(MessageId::from(reply_id.clone()));
    }
    ids.extend(parsed.references.iter().cloned().map(MessageId::from));
    ids
}

/// The `References` chain to put on a reply: the inbound chain, extended with the message it
/// answers. Ordering differs from [`reference_ids`] because this one goes out on the wire.
pub(super) fn outbound_reference_ids(parsed: &ParsedEmail) -> Vec<String> {
    let mut references = parsed.references.clone();
    if let Some(ref reply_to) = parsed.in_reply_to
        && !references.contains(reply_to)
    {
        references.push(reply_to.clone());
    }
    references
}

/// Reference IDs plus this message's own ID, for locating the thread it belongs to.
pub(super) fn thread_lookup_ids(parsed: &ParsedEmail) -> Vec<MessageId> {
    let mut ids = vec![MessageId::from(parsed.message_id.clone())];
    ids.extend(reference_ids(parsed));
    ids
}

pub(super) fn thread_index_of(parsed: &ParsedEmail) -> Option<ThreadIndex> {
    parsed.thread_index.clone().map(ThreadIndex::from)
}

/// Cheap rejections that need no I/O: identity, authentication, and loop protection.
///
/// Runs before any persistence work so a forged or looping message costs a single parse.
pub(super) fn check_inbound_guards(
    parsed: &ParsedEmail,
    internal_source: Option<InternalChannelSource>,
) -> Option<InboundIngestResult> {
    let is_inter_channel = internal_source.is_some();

    if let Some(source) = internal_source
        && parsed.channel_id_header != Some(source.channel_id)
    {
        return Some(InboundIngestResult::rejected(
            "Internal source channel identity mismatch",
        ));
    }

    // Trusted internal transport is authenticated by channel identity, not by SPF/DKIM/DMARC.
    if !is_inter_channel {
        if external_dmarc_rejection(parsed.dmarc_status).is_some() {
            tracing::warn!(sender = %parsed.sender, verdict = ?parsed.dmarc_status,
                "External message rejected because DMARC did not pass");
            return Some(InboundIngestResult::rejected(
                "DMARC authentication did not pass",
            ));
        }
    }

    if is_inter_channel && parsed.hop_count >= MAX_CHANNEL_HOPS {
        tracing::warn!(
            "Max inter-channel hop count ({}) reached for Message-ID: {}",
            parsed.hop_count,
            parsed.message_id
        );
        return Some(InboundIngestResult::rejected(
            "Max inter-channel hop count reached",
        ));
    }

    if !is_inter_channel && parsed.is_auto_reply {
        tracing::warn!(
            "External auto-reply loop detected for Message-ID: {}, dropping message",
            parsed.message_id
        );
        return Some(InboundIngestResult::rejected(
            "External auto-reply loop detected",
        ));
    }

    None
}

fn external_dmarc_rejection(verdict: crate::entities::auth::AuthVerdict) -> Option<&'static str> {
    match verdict {
        crate::entities::auth::AuthVerdict::Pass => None,
        crate::entities::auth::AuthVerdict::Fail
        | crate::entities::auth::AuthVerdict::SoftFail
        | crate::entities::auth::AuthVerdict::Neutral
        | crate::entities::auth::AuthVerdict::TempError
        | crate::entities::auth::AuthVerdict::PermError
        | crate::entities::auth::AuthVerdict::Unavailable
        | crate::entities::auth::AuthVerdict::Unknown => Some("DMARC authentication did not pass"),
    }
}

/// Projection of the protocol-neutral inbound message onto the email-shaped view the rest of the
/// ingest pipeline works with.
pub(super) fn parsed_email_from_normalized(norm: &NormalizedInboundMessage) -> ParsedEmail {
    ParsedEmail {
        message_id: norm.message_id.clone().into_string(),
        in_reply_to: norm.thread_ref.clone().map(MessageId::into_string),
        references: norm
            .references
            .iter()
            .cloned()
            .map(MessageId::into_string)
            .collect(),
        thread_index: norm.thread_index.clone().map(ThreadIndex::into_string),
        sender: norm.sender.identity.clone(),
        recipients_to: norm
            .recipients_to
            .iter()
            .map(|p| p.identity.clone())
            .collect(),
        recipients_cc: norm
            .recipients_cc
            .iter()
            .map(|p| p.identity.clone())
            .collect(),
        subject: norm.subject.clone(),
        clean_text_body: norm.clean_text.clone(),
        raw_text_body: norm.raw_text.clone(),
        raw_html_body: norm.raw_html.clone(),
        attachments: norm.attachments.clone(),
        prompt_text: norm.clean_text.clone(),
        is_auto_reply: norm.is_auto_reply,
        is_forwarded: norm.is_forwarded,
        channel_id_header: norm.channel_id_header,
        hop_count: norm.hop_count,
        trace_channels: norm.trace_channels.clone(),
        spf_status: norm.spf_status.clone(),
        dkim_status: norm.dkim_status.clone(),
        dmarc_status: norm.dmarc_status.clone(),
        spam_score: norm.spam_score,
        is_context_only: norm.is_context_only,
    }
}

/// Strip quoted history from a reply, falling back to matching against the thread's own stored
/// bodies when the heuristic can't find a quote marker.
pub(super) fn strip_quoted_history(parsed: &ParsedEmail, history: &[Message]) -> String {
    if parsed.is_forwarded || history.is_empty() {
        return parsed.clean_text_body.clone();
    }
    let history_bodies: Vec<String> = history.iter().map(|m| m.clean_text_body.clone()).collect();
    let heuristic_clean = EmailParser::strip_quotes_heuristic(&parsed.clean_text_body);
    EmailParser::strip_historical_quotes_fallback(&heuristic_clean, &history_bodies)
}

/// The prompt handed to the agent: the cleaned body plus a description of every attachment worth
/// mentioning (tiny inline images are signature decorations, not content).
pub(super) fn build_prompt_text(clean_body: &str, attachments: &[AttachmentMetadata]) -> String {
    let attachment_prompts: Vec<String> = attachments
        .iter()
        .filter(|att| {
            !(att.content_type.to_lowercase().starts_with("image/")
                && att.size_bytes < SMALL_INLINE_IMAGE_BYTES)
        })
        .map(|meta| {
            format!(
                "[Attachment: {}, Type: {}, SHA256: {}, Size: {} bytes]",
                meta.filename, meta.content_type, meta.sha256_hash, meta.size_bytes
            )
        })
        .collect();

    if attachment_prompts.is_empty() {
        clean_body.to_string()
    } else {
        format!("{}\n\n{}", clean_body, attachment_prompts.join("\n"))
    }
}

/// Whether a body contains an email address as a complete address-like token.
pub(super) fn body_mentions_email(body: &str, address: &str) -> bool {
    let body = body.to_lowercase();
    let address = address.trim().to_lowercase();
    if address.is_empty() {
        return false;
    }

    body.match_indices(&address).any(|(start, matched)| {
        let before = body[..start].chars().next_back();
        let end = start + matched.len();
        let mut after = body[end..].chars();
        let next = after.next();
        let left_bounded = before.is_none_or(|ch| !is_email_token_char(ch));
        let right_bounded = match next {
            Some('.') => after.next().is_none_or(|ch| !ch.is_ascii_alphanumeric()),
            Some(ch) => !is_email_token_char(ch),
            None => true,
        };
        left_bounded && right_bounded
    })
}

/// Whether a body contains a plain `@slug` mention as a complete mention token.
pub(super) fn body_mentions_slug(body: &str, slug: &str) -> bool {
    let slug = slug.trim();
    if slug.is_empty() {
        return false;
    }
    contains_bounded_case_insensitive(body, &format!("@{slug}"), is_slug_token_char)
}

fn contains_bounded_case_insensitive(
    haystack: &str,
    needle: &str,
    is_token_char: fn(char) -> bool,
) -> bool {
    let haystack = haystack.to_lowercase();
    let needle = needle.to_lowercase();
    if needle.is_empty() {
        return false;
    }

    haystack.match_indices(&needle).any(|(start, matched)| {
        let before = haystack[..start].chars().next_back();
        let end = start + matched.len();
        let after = haystack[end..].chars().next();
        before.is_none_or(|ch| !is_token_char(ch)) && after.is_none_or(|ch| !is_token_char(ch))
    })
}

fn is_email_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-' | '@')
}

fn is_slug_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

#[cfg(test)]
mod mention_tests {
    use super::{body_mentions_email, body_mentions_slug, external_dmarc_rejection};
    use crate::entities::auth::AuthVerdict;

    #[test]
    fn only_a_dmarc_pass_authorizes_external_mail() {
        assert_eq!(external_dmarc_rejection(AuthVerdict::Pass), None);
        for verdict in [
            AuthVerdict::Fail,
            AuthVerdict::SoftFail,
            AuthVerdict::Neutral,
            AuthVerdict::TempError,
            AuthVerdict::PermError,
            AuthVerdict::Unavailable,
            AuthVerdict::Unknown,
        ] {
            assert!(external_dmarc_rejection(verdict).is_some(), "{verdict:?}");
        }
    }

    #[test]
    fn email_mentions_are_case_insensitive_and_bounded() {
        assert!(body_mentions_email(
            "Please ask SUPPORT@ACME.MAILAGENTS.COM.",
            "support@acme.mailagents.com"
        ));
        assert!(!body_mentions_email(
            "xsupport@acme.mailagents.com",
            "support@acme.mailagents.com"
        ));
    }

    #[test]
    fn slug_mentions_do_not_match_longer_slugs_or_bracket_syntax() {
        assert!(body_mentions_slug("Please ask @Support.", "support"));
        assert!(!body_mentions_slug("Please ask @supporting.", "support"));
        assert!(!body_mentions_slug("Please ask @[support].", "support"));
    }
}
