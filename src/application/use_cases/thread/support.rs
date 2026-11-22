//! Shared, mostly-pure helpers for the inbound ingest pipeline.
//!
//! Everything here is either a pure decision (no `self`, no I/O) or a memoizing wrapper around
//! persistence lookups. Keeping them out of the pipeline bodies is what lets those bodies read as
//! a sequence of named phases.

use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        address::AddressTarget,
        channel::Channel,
        company::Company,
        message_view::AgentHistoryMessage,
        participant::PrincipalAccessContext,
        quoted_text,
        transport::QualifiedIdentity,
        value_objects::{ChannelSlug, CompanySlug},
    },
    transport::InboundDraft,
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
    departments: HashMap<Uuid, Vec<crate::entities::department::Department>>,
    access_contexts: HashMap<(Uuid, QualifiedIdentity), PrincipalAccessContext>,
    /// What each addressed slug names, per company, keyed by the lowercased slug.
    addresses: HashMap<Uuid, HashMap<String, AddressTarget>>,
}

impl<'a> DirectoryCache<'a> {
    pub(crate) fn new(use_cases: &'a ThreadUseCases) -> Self {
        Self {
            use_cases,
            companies: HashMap::new(),
            channels: HashMap::new(),
            departments: HashMap::new(),
            access_contexts: HashMap::new(),
            addresses: HashMap::new(),
        }
    }

    /// Read what these slugs name in the company's address namespace, in one statement. Routing
    /// calls it once per company with every slug the message names there, so all of them are
    /// answered from one snapshot.
    pub(crate) async fn load_addresses(
        &mut self,
        company_id: Uuid,
        slugs: &[ChannelSlug],
    ) -> AppResult<()> {
        let resolved = self
            .use_cases
            .channel_persistence
            .address_targets(company_id, slugs)
            .await?;
        let known = self.addresses.entry(company_id).or_default();
        for address in resolved {
            known.insert(address.slug.to_lowercase(), address.target);
        }
        Ok(())
    }

    /// What a loaded slug names, or `None` when it names nothing in the company.
    pub(crate) fn address_target(
        &self,
        company_id: Uuid,
        slug: &ChannelSlug,
    ) -> Option<AddressTarget> {
        self.addresses
            .get(&company_id)
            .and_then(|known| known.get(&slug.to_lowercase()))
            .copied()
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

    /// The company's active departments, to group its desk directory by: an archived unit's
    /// desks take no mail, so the directory leaves them out with it.
    pub(crate) async fn departments(
        &mut self,
        company_id: Uuid,
    ) -> AppResult<Vec<crate::entities::department::Department>> {
        if let Some(cached) = self.departments.get(&company_id) {
            return Ok(cached.clone());
        }
        let loaded = self
            .use_cases
            .channel_persistence
            .list_departments(
                company_id,
                crate::entities::department::ArchivedUnits::Exclude,
            )
            .await?;
        self.departments.insert(company_id, loaded.clone());
        Ok(loaded)
    }

    /// Resolve one transport identity without observing it. Unknown handles remain unknown until
    /// the accepted inbound transaction creates them.
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
            .participant_persistence
            .access_context_for_identity(company_id, identity)
            .await?;
        self.access_contexts.insert(key, loaded);
        Ok(loaded)
    }
}

/// Strip quoted history from a reply, falling back to matching against the thread's own stored
/// bodies when the heuristic can't find a quote marker.
pub(super) fn strip_quoted_history(
    draft: &InboundDraft,
    history: &[AgentHistoryMessage],
) -> String {
    let body = draft.content.body_text();
    if draft.directives.is_forwarded || history.is_empty() {
        return body.to_string();
    }
    let history_bodies: Vec<&str> = history
        .iter()
        .map(|message| message.body.as_str())
        .collect();
    quoted_text::strip(body, &history_bodies)
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
