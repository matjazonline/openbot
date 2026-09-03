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
        email_message::EmailMessageMetadata,
        message::{AttachmentMetadata, Message},
        participant::PrincipalAccessContext,
        quoted_text,
        transport::QualifiedIdentity,
        value_objects::{CompanySlug, MessageId},
    },
    transport::{InboundDraft, InboundEnvelope, SMALL_INLINE_IMAGE_BYTES},
};

use super::ThreadUseCases;

/// Memoized directory lookups for a single ingest run.
///
/// The ingest pipeline resolves the same company, channel list and team membership repeatedly
/// while walking `To`/`Cc` pipelines. This owns those caches so the pipeline body never
/// hand-rolls a `get`/`insert` dance, and so every cache key is built in exactly one place.
pub(crate) struct DirectoryCache<'a> {
    use_cases: &'a ThreadUseCases,
    companies: HashMap<String, Option<Company>>,
    channels: HashMap<Uuid, Vec<Channel>>,
    access_contexts: HashMap<(Uuid, QualifiedIdentity), PrincipalAccessContext>,
    agents: HashMap<Uuid, Option<Agent>>,
}

impl<'a> DirectoryCache<'a> {
    pub(crate) fn new(use_cases: &'a ThreadUseCases) -> Self {
        Self {
            use_cases,
            companies: HashMap::new(),
            channels: HashMap::new(),
            access_contexts: HashMap::new(),
            agents: HashMap::new(),
        }
    }

    pub(crate) async fn company(&mut self, slug: &CompanySlug) -> AppResult<Option<Company>> {
        let key = slug.to_lowercase();
        if let Some(cached) = self.companies.get(&key) {
            return Ok(cached.clone());
        }
        let loaded = self.use_cases.company_persistence.get_by_slug(slug).await?;
        self.companies.insert(key, loaded.clone());
        Ok(loaded)
    }

    pub(crate) async fn channels(&mut self, company_id: Uuid) -> AppResult<Vec<Channel>> {
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

    /// Resolve one transport identity to its stable principal and membership. The observation is
    /// idempotent and confers no grant; it only ensures all later decisions use the same actor id.
    pub(crate) async fn access_context(
        &mut self,
        company_id: Uuid,
        identity: &QualifiedIdentity,
    ) -> AppResult<PrincipalAccessContext> {
        let key = (company_id, identity.clone());
        if let Some(cached) = self.access_contexts.get(&key) {
            return Ok(*cached);
        }
        let loaded = self
            .use_cases
            .observe_ingress_identity(company_id, identity)
            .await?;
        self.access_contexts.insert(key, loaded);
        Ok(loaded)
    }

    /// One configured agent, cached because several channel matches may reference the same
    /// library definition.
    pub(crate) async fn agent(&mut self, id: Uuid) -> AppResult<Option<Agent>> {
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

/// The `References` chain to put on a reply: the inbound chain, extended with the message it
/// answers.
///
/// Ordered for the wire, oldest first, which is the opposite of the nearest-ancestor order
/// [`EmailMessageMetadata::reference_candidates`] uses for thread lookup.
pub(super) fn outbound_reference_ids(envelope: &InboundEnvelope) -> Vec<MessageId> {
    let Some(metadata) = envelope.extension.email_metadata() else {
        return Vec::new();
    };
    let mut references = metadata.references.clone();
    if let Some(in_reply_to) = metadata.in_reply_to.as_ref()
        && !references.contains(in_reply_to)
    {
        references.push(in_reply_to.clone());
    }
    references
}

/// The RFC id this message will be answered `In-Reply-To`, when mail carried it.
pub(super) fn rfc_message_id(envelope: &InboundEnvelope) -> Option<&MessageId> {
    envelope
        .extension
        .email_metadata()
        .map(|metadata: &EmailMessageMetadata| &metadata.rfc_message_id)
}

/// Strip quoted history from a reply, falling back to matching against the thread's own stored
/// bodies when the heuristic can't find a quote marker.
pub(super) fn strip_quoted_history(draft: &InboundDraft, history: &[Message]) -> String {
    let body = draft.content.body_text();
    if draft.directives.is_forwarded || history.is_empty() {
        return body.to_string();
    }
    let history_bodies: Vec<&str> = history
        .iter()
        .map(|message| message.clean_text_body.as_str())
        .collect();
    quoted_text::strip(body, &history_bodies)
}

/// The prompt handed to the agent: the cleaned body plus a description of every attachment worth
/// mentioning (tiny inline images are signature decorations, not content).
pub(super) fn build_prompt_text(envelope: &InboundEnvelope) -> String {
    prompt_text(envelope.content.body_text(), &envelope.attachments)
}

/// The prompt handed to the agent: the cleaned body plus a description of every attachment worth
/// mentioning.
fn prompt_text(clean_body: &str, attachments: &[AttachmentMetadata]) -> String {
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

impl ThreadUseCases {
    /// Whether this address belongs to someone outside the platform.
    ///
    /// The single definition of "third party" on the *egress* side: it decides who is dropped from
    /// an agent reply's `Cc` when a channel has `add_3rd_party` off. Ingress no longer asks -- the
    /// adapter already classified every recipient before the application saw it -- and this goes
    /// with the reply renderer in step 9.
    pub(super) async fn is_third_party_address(
        &self,
        address: &str,
        sender: &str,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<bool> {
        let address = address.trim();
        if address.is_empty() || address.eq_ignore_ascii_case(sender) {
            return Ok(false);
        }
        let Some((company_slug, _, _)) =
            crate::use_cases::channel::parse_recipient_address_pipeline(
                address,
                &self.config.app_domain_name,
            )
        else {
            return Ok(true);
        };
        Ok(directory.company(&company_slug).await?.is_none())
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
    use super::{body_mentions_email, body_mentions_slug};

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
