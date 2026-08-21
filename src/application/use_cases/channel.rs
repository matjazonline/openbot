use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        channel::{Channel, PUBLIC_PARTICIPANT},
        company::{Company, CompanyAccess},
        user::Viewer,
        value_objects::{ChannelSlug, CompanySlug},
    },
    infra::config::AppConfig,
    use_cases::company::{CompanyPersistence, owned_company},
};
use serde::{Deserialize, Serialize};

/// Everything one channel write sets, so create and update cannot drift apart and so a caller
/// cannot transpose two same-typed arguments in a nine-parameter list.
///
/// Values reach persistence already normalized — see [`ChannelWrite::normalize`].
#[derive(Debug, Clone, Default)]
pub struct ChannelWrite {
    pub name: String,
    pub slug: String,
    /// Extra local parts the channel also answers on. Replaced wholesale by every write.
    pub alias_slugs: Vec<String>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<String>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub channel_config: Option<serde_json::Value>,
    pub enabled: bool,
    /// Whether a trusted sender may pull CC'd outsiders onto this channel's threads.
    pub add_3rd_party: bool,
}

impl ChannelWrite {
    /// Trim and lower-case the fields that have canonical forms, and drop the blanks. Runs once,
    /// in the use case, so every entry point stores the same shape.
    fn normalize(&mut self) -> AppResult<()> {
        self.name = self.name.trim().to_string();
        self.slug = self.slug.trim().to_lowercase().replace(' ', "-");

        if self.name.is_empty() {
            return Err(AppError::BadRequest(
                "The channel name cannot be empty.".into(),
            ));
        }
        if self.slug.is_empty() {
            return Err(AppError::BadRequest(
                "The channel address cannot be empty.".into(),
            ));
        }
        validate_slug(&self.slug, SlugKind::ChannelAddress)?;
        self.normalize_alias_slugs()?;

        blank_to_none(&mut self.api_key);
        blank_to_none(&mut self.provider);
        blank_to_none(&mut self.model);

        if let Some(emails) = self.participant_emails.as_mut() {
            emails.retain_mut(|email| {
                *email = email.trim().to_lowercase();
                !email.is_empty() && email.contains('@')
            });
        }

        Ok(())
    }

    /// Aliases go through the same canonicalization and reserved-suffix rules as the primary
    /// slug, then lose blanks, duplicates and any repeat of the primary slug — the database
    /// would reject that last one as a self-collision.
    fn normalize_alias_slugs(&mut self) -> AppResult<()> {
        let mut seen = std::collections::HashSet::new();
        let mut aliases = Vec::with_capacity(self.alias_slugs.len());

        for alias in std::mem::take(&mut self.alias_slugs) {
            let alias = alias.trim().to_lowercase().replace(' ', "-");
            if alias.is_empty() || alias == self.slug {
                continue;
            }
            validate_slug(&alias, SlugKind::ChannelAlias)?;
            if seen.insert(alias.clone()) {
                aliases.push(alias);
            }
        }

        self.alias_slugs = aliases;
        Ok(())
    }

    /// Whether the participant list opens this channel to anyone — the only case the spam
    /// interlock applies to.
    fn is_public(&self) -> bool {
        self.participant_emails
            .as_ref()
            .is_some_and(|emails| emails.iter().any(|e| e == PUBLIC_PARTICIPANT))
    }
}

fn blank_to_none(value: &mut Option<String>) {
    if let Some(inner) = value.as_mut() {
        *inner = inner.trim().to_string();
        if inner.is_empty() {
            *value = None;
        }
    }
}

/// A channel belonging to another company is reported exactly like a missing one, so an id probe
/// cannot tell a foreign channel from a nonexistent one. See [`owned_company`].
pub fn channel_not_found() -> AppError {
    AppError::NotFound("Channel not found in this company.".into())
}

#[async_trait]
pub trait ChannelPersistence: Send + Sync {
    async fn create(&self, company_id: Uuid, write: ChannelWrite) -> AppResult<Channel>;

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>>;

    async fn get_by_company_slug_and_channel_slug(
        &self,
        company_slug: &CompanySlug,
        channel_slug: &ChannelSlug,
    ) -> AppResult<Option<Channel>>;

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>>;

    async fn update(&self, id: Uuid, write: ChannelWrite) -> AppResult<Channel>;

    async fn delete(&self, id: Uuid) -> AppResult<()>;
}

#[derive(Clone)]
pub struct ChannelUseCases {
    company_persistence: Arc<dyn CompanyPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    config: Arc<AppConfig>,
}

impl ChannelUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        Self {
            company_persistence,
            channel_persistence,
            config,
        }
    }

    async fn verify_company_owner(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        owned_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn create_channel(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        mut write: ChannelWrite,
        confirm_spam_disabled: bool,
    ) -> AppResult<Channel> {
        self.verify_company_owner(user_id, company_id).await?;

        write.normalize()?;
        self.check_spam_interlock(&write, confirm_spam_disabled)?;

        info!(
            "Creating channel '{}' ({}) for company {}",
            write.name, write.slug, company_id
        );

        self.channel_persistence.create(company_id, write).await
    }

    /// A channel open to `@public` must not be saved while the server has no spam scanning at all,
    /// unless the caller explicitly says it knows.
    fn check_spam_interlock(
        &self,
        write: &ChannelWrite,
        confirm_spam_disabled: bool,
    ) -> AppResult<()> {
        if write.is_public() && !self.config.is_spam_scan_enabled() && !confirm_spam_disabled {
            return Err(AppError::BadRequest(
                "Spam scanning is disabled in server configuration. Saving a public channel (@public) requires explicit confirmation (confirm_spam_disabled) that you are aware spam scanning is disabled.".into(),
            ));
        }
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn list_company_channels(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Channel>> {
        self.verify_company_owner(user_id, company_id).await?;
        self.channel_persistence
            .list_by_company_id(company_id)
            .await
    }

    /// Every channel of `company_id` this viewer may read.
    ///
    /// The read counterpart of [`ChannelUseCases::list_company_channels`]: that one is for the
    /// pages that *configure* channels and stays owner-only, this one is what the mailbox lists,
    /// and a restricted channel simply is not in it for a colleague who is not a participant.
    #[instrument(skip(self))]
    pub async fn list_readable_channels(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
    ) -> AppResult<Vec<Channel>> {
        let Some(access) = self.viewer_access(viewer, company_id).await? else {
            return Ok(Vec::new());
        };

        Ok(self
            .channel_persistence
            .list_by_company_id(company_id)
            .await?
            .into_iter()
            .filter(|channel| channel.viewer_access(&viewer.email, access.membership))
            .collect())
    }

    /// One channel, if this viewer may read it.
    ///
    /// Answers `None` for "no such channel", "not your company" and "not yours to read" alike:
    /// telling them apart would let anyone probe ids to learn which channels a company runs.
    #[instrument(skip(self))]
    pub async fn get_readable_channel(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<Channel>> {
        let Some(access) = self.viewer_access(viewer, company_id).await? else {
            return Ok(None);
        };

        let channel = self.channel_persistence.get_by_id(channel_id).await?;

        Ok(channel.filter(|channel| {
            channel.company_id == company_id
                && channel.viewer_access(&viewer.email, access.membership)
        }))
    }

    /// What the viewer is to a company, or `None` if they are nothing to it.
    async fn viewer_access(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>> {
        self.company_persistence
            .company_access(viewer.user_id, company_id)
            .await
    }

    #[instrument(skip(self))]
    pub async fn get_company_channel(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<Channel>> {
        self.verify_company_owner(user_id, company_id).await?;
        let channel = self.channel_persistence.get_by_id(channel_id).await?;
        if let Some(ref ch) = channel {
            if ch.company_id != company_id {
                return Ok(None);
            }
        }
        Ok(channel)
    }

    pub fn channel_persistence(&self) -> &Arc<dyn ChannelPersistence> {
        &self.channel_persistence
    }

    #[instrument(skip(self))]
    pub async fn update_channel(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
        mut write: ChannelWrite,
        confirm_spam_disabled: bool,
    ) -> AppResult<Channel> {
        self.verify_company_owner(user_id, company_id).await?;

        let channel = self
            .channel_persistence
            .get_by_id(channel_id)
            .await?
            .ok_or_else(channel_not_found)?;

        if channel.company_id != company_id {
            return Err(channel_not_found());
        }

        write.normalize()?;
        self.check_spam_interlock(&write, confirm_spam_disabled)?;

        info!(
            "Updating channel {} for company {}: {} ({}), enabled={}",
            channel_id, company_id, write.name, write.slug, write.enabled
        );

        self.channel_persistence.update(channel_id, write).await
    }

    #[instrument(skip(self))]
    pub async fn delete_channel(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<()> {
        self.verify_company_owner(user_id, company_id).await?;

        let channel = self
            .channel_persistence
            .get_by_id(channel_id)
            .await?
            .ok_or_else(channel_not_found)?;

        if channel.company_id != company_id {
            return Err(channel_not_found());
        }

        info!("Deleting channel {} for company {}", channel_id, company_id);
        self.channel_persistence.delete(channel_id).await
    }

    #[instrument(skip(self, email))]
    pub async fn process_inbound_email(
        &self,
        provider: &str,
        email: InboundEmail,
        app_domain_name: &str,
    ) -> AppResult<InboundEmailResult> {
        info!(
            "Processing inbound email via provider '{}' for recipient: {}",
            provider, email.to
        );

        let parsed = parse_recipient_address(&email.to, app_domain_name);
        match parsed {
            Some((company_slug, channel_slug)) => {
                info!(
                    "Parsed recipient -> company_slug: '{}', channel_slug: '{}'",
                    company_slug, channel_slug
                );

                let company = self.company_persistence.get_by_slug(&company_slug).await?;
                let channel = self
                    .channel_persistence
                    .get_by_company_slug_and_channel_slug(&company_slug, &channel_slug)
                    .await?;

                let sender_email = extract_email_address(&email.from);

                // The same verdict the real inbound path reaches, from the same method: a preview
                // that re-implemented the participant rule would drift out of sync with it.
                let sender_authorized = match (company.as_ref(), channel.as_ref()) {
                    (Some(comp), Some(ch)) => {
                        let membership = self
                            .company_persistence
                            .membership_for_email(comp.id, &sender_email)
                            .await?;
                        ch.participant_access(&sender_email, membership).authorized
                    }
                    _ => false,
                };

                let resolved = company.is_some() && channel.is_some() && sender_authorized;

                if resolved {
                    info!(
                        "Successfully resolved channel '{}' for company '{}' with authorized sender '{}'",
                        channel_slug, company_slug, sender_email
                    );
                } else if !sender_authorized {
                    info!(
                        "Unauthorized sender '{}' for channel '{}@{}' (not in participant_emails)",
                        sender_email, channel_slug, company_slug
                    );
                } else {
                    info!(
                        "Channel or company not found for '{}@{}'",
                        channel_slug, company_slug
                    );
                }

                Ok(InboundEmailResult {
                    resolved,
                    sender_authorized,
                    company_slug: Some(company_slug),
                    channel_slug: Some(channel_slug),
                    company,
                    channel,
                    email,
                })
            }
            None => {
                info!("Could not parse recipient email address: '{}'", email.to);
                Ok(InboundEmailResult {
                    resolved: false,
                    sender_authorized: false,
                    company_slug: None,
                    channel_slug: None,
                    company: None,
                    channel: None,
                    email,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundEmail {
    pub to: String,
    pub from: String,
    pub subject: Option<String>,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub raw_payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundEmailResult {
    pub resolved: bool,
    pub sender_authorized: bool,
    pub company_slug: Option<CompanySlug>,
    pub channel_slug: Option<ChannelSlug>,
    pub company: Option<Company>,
    pub channel: Option<Channel>,
    pub email: InboundEmail,
}

pub fn extract_email_address(input: &str) -> String {
    let email_addr = if let (Some(start), Some(end)) = (input.find('<'), input.rfind('>')) {
        if start < end {
            &input[start + 1..end]
        } else {
            input
        }
    } else {
        input
    };
    email_addr.trim().to_lowercase()
}

/// Which field a slug came from, so a rejection can name the input the user has to fix instead of
/// saying "slug" for three different form fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlugKind {
    ChannelAddress,
    ChannelAlias,
    AgentSlug,
}

impl SlugKind {
    fn noun(self) -> &'static str {
        match self {
            SlugKind::ChannelAddress => "channel address",
            SlugKind::ChannelAlias => "channel alias",
            SlugKind::AgentSlug => "agent slug",
        }
    }
}

/// A malformed slug is user input, not a server fault, so every rejection here is a
/// [`AppError::BadRequest`] — `Internal` renders as a bare "Internal error" with the message
/// dropped, which is exactly what the person filling in the form needs to see.
pub fn validate_slug(slug: &str, kind: SlugKind) -> AppResult<()> {
    let slug_clean = slug.trim().to_lowercase().replace(' ', "-");
    if slug_clean.is_empty() {
        return Err(AppError::BadRequest(format!(
            "The {} cannot be empty.",
            kind.noun()
        )));
    }

    for suffix in crate::services::email_parser::RESERVED_CONTEXT_SUFFIXES {
        let dot_s = format!(".{}", suffix);
        let plus_s = format!("+{}", suffix);
        let dash_s = format!("-{}", suffix);
        let underscore_s = format!("_{}", suffix);

        if slug_clean == *suffix
            || slug_clean.ends_with(&dot_s)
            || slug_clean.ends_with(&plus_s)
            || slug_clean.ends_with(&dash_s)
            || slug_clean.ends_with(&underscore_s)
        {
            return Err(AppError::BadRequest(format!(
                "Invalid {} '{}': it cannot be, or end with, one of the reserved suffixes {} \
                 (optionally preceded by '.', '+', '-' or '_'). Those mark an address as \
                 context-only, so a channel named after one could never be replied to.",
                kind.noun(),
                slug_clean,
                crate::services::email_parser::RESERVED_CONTEXT_SUFFIXES.join(", ")
            )));
        }
    }

    Ok(())
}

pub fn strip_context_suffix_from_slug(raw_slug: &str) -> (String, bool) {
    let lower = raw_slug.trim().to_lowercase();
    for suffix in crate::services::email_parser::RESERVED_CONTEXT_SUFFIXES {
        let dot_s = format!(".{}", suffix);
        let plus_s = format!("+{}", suffix);
        let dash_s = format!("-{}", suffix);
        let underscore_s = format!("_{}", suffix);

        if lower.ends_with(&dot_s) {
            let base = lower[..lower.len() - dot_s.len()].trim();
            if !base.is_empty() {
                return (base.to_string(), true);
            }
        } else if lower.ends_with(&plus_s) {
            let base = lower[..lower.len() - plus_s.len()].trim();
            if !base.is_empty() {
                return (base.to_string(), true);
            }
        } else if lower.ends_with(&dash_s) {
            let base = lower[..lower.len() - dash_s.len()].trim();
            if !base.is_empty() {
                return (base.to_string(), true);
            }
        } else if lower.ends_with(&underscore_s) {
            let base = lower[..lower.len() - underscore_s.len()].trim();
            if !base.is_empty() {
                return (base.to_string(), true);
            }
        } else if lower == *suffix {
            return (String::new(), true);
        }
    }
    (raw_slug.trim().to_lowercase(), false)
}

pub fn parse_recipient_address_pipeline(
    to_str: &str,
    app_domain_name: &str,
) -> Option<(CompanySlug, Vec<ChannelSlug>, bool)> {
    let email_addr = if let (Some(start), Some(end)) = (to_str.find('<'), to_str.rfind('>')) {
        if start < end {
            &to_str[start + 1..end]
        } else {
            to_str
        }
    } else {
        to_str
    };

    let cleaned = email_addr.trim().to_lowercase();
    let parts: Vec<&str> = cleaned.split('@').collect();
    if parts.len() != 2 {
        return None;
    }

    let channel_part = parts[0].trim();
    let domain_part = parts[1].trim();

    if channel_part.is_empty() || domain_part.is_empty() {
        return None;
    }

    let domain_lower = app_domain_name.trim().to_lowercase();
    let expected_suffix = format!(".{}", domain_lower);

    let company_slug = if domain_part.ends_with(&expected_suffix) {
        &domain_part[..domain_part.len() - expected_suffix.len()]
    } else {
        return None;
    };

    if company_slug.is_empty() {
        return None;
    }

    let mut is_context_only = false;
    let mut channel_slugs: Vec<String> = Vec::new();

    for part in channel_part.split('+') {
        let (clean_slug, is_context) = strip_context_suffix_from_slug(part);
        if is_context {
            is_context_only = true;
        }
        if !clean_slug.is_empty() && !channel_slugs.contains(&clean_slug) {
            channel_slugs.push(clean_slug);
        }
    }

    if channel_slugs.is_empty() {
        return None;
    }

    Some((
        CompanySlug::new(company_slug),
        channel_slugs.into_iter().map(ChannelSlug::new).collect(),
        is_context_only,
    ))
}

pub fn parse_recipient_address(
    to_str: &str,
    app_domain_name: &str,
) -> Option<(CompanySlug, ChannelSlug)> {
    parse_recipient_address_pipeline(to_str, app_domain_name)
        .map(|(company, channels, _)| (company, channels[0].clone()))
}

pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut dp = vec![vec![0; n + 1]; m + 1];

    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[m][n]
}

pub fn find_similar_channel_slugs(target: &str, available: &[Channel]) -> Vec<ChannelSlug> {
    let target_clean = target.trim().to_lowercase();
    let mut matches: Vec<(usize, ChannelSlug)> = Vec::new();

    for slug in available.iter().flat_map(Channel::slugs) {
        let dist = levenshtein_distance(&target_clean, &slug.to_lowercase());
        let max_dist = (slug.len() / 2).max(2);
        if dist <= max_dist && dist > 0 {
            matches.push((dist, slug.clone()));
        }
    }

    matches.sort_by_key(|(d, _)| *d);
    matches.into_iter().map(|(_, slug)| slug).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::company::Company;
    use crate::entities::company_member::CompanyMembership;
    use crate::entities::value_objects::EmailAddress;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Mutex;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(
            &self,
            _user_id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
            unimplemented!()
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id)
                .cloned())
        }

        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.slug.eq_ignore_ascii_case(slug))
                .cloned())
        }

        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
            unimplemented!()
        }

        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
            unimplemented!()
        }

        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }

        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }

        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }
    }

    struct MockChannelPersistence {
        channels: Mutex<Vec<Channel>>,
    }

    #[async_trait]
    impl ChannelPersistence for MockChannelPersistence {
        async fn create(&self, company_id: Uuid, write: ChannelWrite) -> AppResult<Channel> {
            let channel = channel_from_write(Uuid::new_v4(), company_id, write);
            self.channels.lock().unwrap().push(channel.clone());
            Ok(channel)
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.id == id)
                .cloned())
        }

        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &CompanySlug,
            channel_slug: &ChannelSlug,
        ) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.slug.eq_ignore_ascii_case(channel_slug))
                .cloned())
        }

        async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .filter(|w| w.company_id == company_id)
                .cloned()
                .collect())
        }

        async fn update(&self, id: Uuid, write: ChannelWrite) -> AppResult<Channel> {
            let mut list = self.channels.lock().unwrap();
            let existing = list
                .iter_mut()
                .find(|w| w.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;

            *existing = Channel {
                created_at: existing.created_at,
                ..channel_from_write(id, existing.company_id, write)
            };
            Ok(existing.clone())
        }

        async fn delete(&self, id: Uuid) -> AppResult<()> {
            self.channels.lock().unwrap().retain(|w| w.id != id);
            Ok(())
        }
    }

    fn channel_from_write(id: Uuid, company_id: Uuid, write: ChannelWrite) -> Channel {
        Channel {
            id,
            company_id,
            name: write.name,
            slug: write.slug.into(),
            alias_slugs: Vec::new(),
            api_key: write.api_key,
            provider: write.provider,
            model: write.model,
            participant_emails: write
                .participant_emails
                .map(|emails| emails.into_iter().map(EmailAddress::from).collect()),
            agent_ids: write.agent_ids,
            channel_config: write.channel_config,
            enabled: write.enabled,
            add_3rd_party: write.add_3rd_party,
            created_at: Utc::now(),
        }
    }

    fn test_config(spam_enabled: bool) -> Arc<AppConfig> {
        Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
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
            enable_heuristic_scanner: spam_enabled,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
        })
    }

    /// `GET` and `PUT` on the same channel must agree, and neither may reveal whether an id it
    /// refuses belongs to another company or to nothing at all.
    #[tokio::test]
    async fn reads_and_writes_refuse_a_foreign_channel_exactly_like_a_missing_one() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();
        let other_company_id = Uuid::new_v4();

        let company = |id| Company {
            id,
            user_id: owner_id,
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now(),
        };
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![company(company_id), company(other_company_id)]),
        });
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });
        let use_cases =
            ChannelUseCases::new(company_persistence, channel_persistence, test_config(true));

        // The owner owns both companies, so this is a cross-tenant id, not an authorization miss.
        let foreign = use_cases
            .create_channel(
                owner_id,
                other_company_id,
                ChannelWrite {
                    name: "Elsewhere".into(),
                    slug: "elsewhere".into(),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .unwrap();
        let missing = Uuid::new_v4();

        // Reads collapse both to `None`, which the route layer renders as one 404.
        assert!(
            use_cases
                .get_company_channel(owner_id, company_id, foreign.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            use_cases
                .get_company_channel(owner_id, company_id, missing)
                .await
                .unwrap()
                .is_none()
        );

        let write = || ChannelWrite {
            name: "Hijack".into(),
            slug: "hijack".into(),
            enabled: true,
            ..ChannelWrite::default()
        };
        let foreign_err = use_cases
            .update_channel(owner_id, company_id, foreign.id, write(), false)
            .await
            .unwrap_err();
        let missing_err = use_cases
            .update_channel(owner_id, company_id, missing, write(), false)
            .await
            .unwrap_err();

        assert!(
            matches!(foreign_err, AppError::NotFound(_)),
            "{foreign_err:?}"
        );
        assert_eq!(
            foreign_err.to_string(),
            missing_err.to_string(),
            "a differing message would let an id probe map another company's channels"
        );
        assert_eq!(
            foreign_err.to_string(),
            channel_not_found().to_string(),
            "the write path must speak with the same voice as the read path"
        );

        // The foreign channel is untouched by the refused write.
        assert_eq!(
            use_cases
                .get_company_channel(owner_id, other_company_id, foreign.id)
                .await
                .unwrap()
                .unwrap()
                .name,
            "Elsewhere"
        );
    }

    #[tokio::test]
    async fn company_owner_channel_crud_flow_works() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        let use_cases =
            ChannelUseCases::new(company_persistence, channel_persistence, test_config(true));

        // 1. Owner creates channel with participant emails and config
        let emails = vec![
            "agent1@example.com".to_string(),
            "agent2@example.com".to_string(),
        ];
        let config = json!({ "trigger": "email_received", "action": "forward" });

        let agent_id1 = Uuid::new_v4();
        let agent_id2 = Uuid::new_v4();
        let agent_ids = vec![agent_id1, agent_id2];

        let channel = use_cases
            .create_channel(
                owner_id,
                company_id,
                ChannelWrite {
                    name: "Support Flow".into(),
                    slug: "support-flow".into(),
                    api_key: Some("key_123".into()),
                    provider: Some("openai".into()),
                    model: Some("gpt-4o".into()),
                    participant_emails: Some(emails.clone()),
                    agent_ids: Some(agent_ids.clone()),
                    channel_config: Some(config.clone()),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .unwrap();

        assert_eq!(channel.name, "Support Flow");
        assert_eq!(channel.slug, "support-flow");
        assert_eq!(channel.api_key.as_deref(), Some("key_123"));
        assert_eq!(channel.provider.as_deref(), Some("openai"));
        assert_eq!(channel.model.as_deref(), Some("gpt-4o"));
        assert_eq!(
            channel.participant_emails,
            Some(
                emails
                    .into_iter()
                    .map(EmailAddress::from)
                    .collect::<Vec<_>>()
            )
        );
        assert_eq!(channel.agent_ids, Some(agent_ids));
        assert_eq!(channel.channel_config, Some(config));

        // 2. Non-owner cannot create channel
        let non_owner_id = Uuid::new_v4();
        let err = use_cases
            .create_channel(
                non_owner_id,
                company_id,
                ChannelWrite {
                    name: "Hacker Flow".into(),
                    slug: "hacker-flow".into(),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await;
        assert!(err.is_err());

        // 3. List channels for company
        let list = use_cases
            .list_company_channels(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        // 4. Update channel
        let updated_config = json!({ "trigger": "webhook", "action": "notify" });
        let updated = use_cases
            .update_channel(
                owner_id,
                company_id,
                channel.id,
                ChannelWrite {
                    name: "Updated Flow".into(),
                    slug: "updated-flow".into(),
                    channel_config: Some(updated_config.clone()),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Updated Flow");
        assert_eq!(updated.slug, "updated-flow");
        assert_eq!(updated.participant_emails, None);
        assert_eq!(updated.agent_ids, None);
        assert_eq!(updated.channel_config, Some(updated_config));

        // 5. Delete channel
        use_cases
            .delete_channel(owner_id, company_id, channel.id)
            .await
            .unwrap();

        let list_after = use_cases
            .list_company_channels(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(list_after.len(), 0);
    }

    #[test]
    fn parse_recipient_address_works() {
        let app_domain = "mailagents.com";

        // Plain email
        let (company_slug, channel_slug) =
            parse_recipient_address("support@acme.mailagents.com", app_domain).unwrap();
        assert_eq!(company_slug, "acme");
        assert_eq!(channel_slug, "support");

        // Named email format
        let (company_slug, channel_slug) = parse_recipient_address(
            "Inbound Handler <inbound-email@wf-corp.mailagents.com>",
            app_domain,
        )
        .unwrap();
        assert_eq!(company_slug, "wf-corp");
        assert_eq!(channel_slug, "inbound-email");

        // Chained pipeline email format
        let (company_slug, channel_slugs, is_ctx) = parse_recipient_address_pipeline(
            "support+billing+legal@acme.mailagents.com",
            app_domain,
        )
        .unwrap();
        assert_eq!(company_slug, "acme");
        assert_eq!(channel_slugs, vec!["support", "billing", "legal"]);
        assert!(!is_ctx);

        // Quiet / Context-only subaddressing formats (.quiet, .noagent, .message, .msg, .na)
        let (comp1, ch1, is_ctx1) =
            parse_recipient_address_pipeline("support.quiet@acme.mailagents.com", app_domain)
                .unwrap();
        assert_eq!(comp1, "acme");
        assert_eq!(ch1, vec!["support"]);
        assert!(is_ctx1);

        let (comp2, ch2, is_ctx2) =
            parse_recipient_address_pipeline("support+noagent@acme.mailagents.com", app_domain)
                .unwrap();
        assert_eq!(comp2, "acme");
        assert_eq!(ch2, vec!["support"]);
        assert!(is_ctx2);

        // Localhost app domain
        let (company_slug, channel_slug) =
            parse_recipient_address("trigger@my-company.localhost", "localhost").unwrap();
        assert_eq!(company_slug, "my-company");
        assert_eq!(channel_slug, "trigger");

        // Invalid formats
        assert!(parse_recipient_address("invalid-email", app_domain).is_none());
        assert!(parse_recipient_address("support@mailagents.com", app_domain).is_none());
        assert!(parse_recipient_address("support@acme.example.com", app_domain).is_none());
    }

    #[test]
    fn normalize_canonicalizes_and_dedupes_alias_slugs() {
        let mut write = ChannelWrite {
            name: "Support".into(),
            slug: "support".into(),
            alias_slugs: vec![
                "  Sales  ".into(),
                "Customer Care".into(),
                "sales".into(),
                "support".into(),
                "   ".into(),
            ],
            enabled: true,
            ..ChannelWrite::default()
        };

        write.normalize().unwrap();

        // Lower-cased and space-hyphenated, blanks and duplicates dropped, and the alias that
        // repeats the canonical slug removed — the database would reject that as a self-collision.
        assert_eq!(write.alias_slugs, vec!["sales", "customer-care"]);
    }

    #[test]
    fn normalize_rejects_an_alias_ending_in_a_reserved_suffix() {
        let mut write = ChannelWrite {
            name: "Support".into(),
            slug: "support".into(),
            alias_slugs: vec!["sales-quiet".into()],
            enabled: true,
            ..ChannelWrite::default()
        };

        let err = write.normalize().unwrap_err().to_string();
        assert!(err.contains("sales-quiet"), "unexpected error: {err}");
    }

    #[test]
    fn test_slug_validation_reserved_suffixes() {
        let check = |slug| validate_slug(slug, SlugKind::ChannelAddress);

        assert!(check("support").is_ok());
        assert!(check("tech-help").is_ok());

        assert!(check("quiet").is_err());
        assert!(check("noagent").is_err());
        assert!(check("support.quiet").is_err());
        assert!(check("support-noagent").is_err());
        assert!(check("sales_message").is_err());
        assert!(check("bot+na").is_err());
    }

    /// A rejected slug is user input: the message must survive to the client, name the field the
    /// user typed into, and say which values are reserved.
    #[test]
    fn rejected_slugs_are_bad_requests_that_name_the_field_and_the_reason() {
        let alias_err = validate_slug("ops-quiet", SlugKind::ChannelAlias).unwrap_err();
        assert!(
            matches!(alias_err, AppError::BadRequest(_)),
            "Internal drops the message on the way out: {alias_err:?}"
        );
        let message = alias_err.to_string();
        assert!(message.contains("channel alias"), "{message}");
        assert!(message.contains("ops-quiet"), "{message}");
        assert!(
            message.contains("noagent, quiet, message, msg, na"),
            "{message}"
        );

        // The same value from the address field names that field instead.
        let address_message = validate_slug("ops-quiet", SlugKind::ChannelAddress)
            .unwrap_err()
            .to_string();
        assert!(
            address_message.contains("channel address"),
            "{address_message}"
        );

        let empty_message = validate_slug("  ", SlugKind::AgentSlug)
            .unwrap_err()
            .to_string();
        assert!(empty_message.contains("agent slug"), "{empty_message}");
    }

    #[test]
    fn test_levenshtein_and_fuzzy_channel_suggestions() {
        assert_eq!(levenshtein_distance("suppport", "support"), 1);
        assert_eq!(levenshtein_distance("biling", "billing"), 1);

        let company_id = uuid::Uuid::new_v4();
        let available = vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: uuid::Uuid::new_v4(),
                company_id,
                name: "Support".to_string(),
                slug: "support".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: chrono::Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: uuid::Uuid::new_v4(),
                company_id,
                name: "Billing".to_string(),
                slug: "billing".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: chrono::Utc::now(),
            },
        ];

        let suggestions = find_similar_channel_slugs("suppport", &available);
        assert_eq!(suggestions, vec!["support"]);

        let suggestions_biling = find_similar_channel_slugs("biling", &available);
        assert_eq!(suggestions_biling, vec!["billing"]);
    }

    #[tokio::test]
    async fn process_inbound_email_resolves_company_and_channel() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        let use_cases =
            ChannelUseCases::new(company_persistence, channel_persistence, test_config(true));

        let _ = use_cases
            .create_channel(
                owner_id,
                company_id,
                ChannelWrite {
                    name: "Support Flow".into(),
                    slug: "support".into(),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .unwrap();

        let email = InboundEmail {
            to: "Support <support@acme.mailagents.com>".to_string(),
            from: "customer@example.com".to_string(),
            subject: Some("Need help".to_string()),
            text_body: Some("Hello".to_string()),
            html_body: None,
            raw_payload: None,
        };

        let result = use_cases
            .process_inbound_email("sendgrid", email, "mailagents.com")
            .await
            .unwrap();

        assert!(result.resolved);
        assert!(result.sender_authorized);
        assert_eq!(result.company_slug.as_deref(), Some("acme"));
        assert_eq!(result.channel_slug.as_deref(), Some("support"));
        assert_eq!(result.company.unwrap().name, "Acme Corp");
        assert_eq!(result.channel.unwrap().name, "Support Flow");

        // Channel with participant_emails restriction
        let _ = use_cases
            .create_channel(
                owner_id,
                company_id,
                ChannelWrite {
                    name: "Restricted Flow".into(),
                    slug: "restricted".into(),
                    participant_emails: Some(vec!["agent@example.com".to_string()]),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .unwrap();

        // 1. Authorized sender
        let auth_email = InboundEmail {
            to: "restricted@acme.mailagents.com".to_string(),
            from: "Agent Smith <agent@example.com>".to_string(),
            subject: Some("Allowed".to_string()),
            text_body: None,
            html_body: None,
            raw_payload: None,
        };

        let auth_result = use_cases
            .process_inbound_email("sendgrid", auth_email, "mailagents.com")
            .await
            .unwrap();

        assert!(auth_result.resolved);
        assert!(auth_result.sender_authorized);

        // 2. Unauthorized sender
        let unauth_email = InboundEmail {
            to: "restricted@acme.mailagents.com".to_string(),
            from: "stranger@external.com".to_string(),
            subject: Some("Denied".to_string()),
            text_body: None,
            html_body: None,
            raw_payload: None,
        };

        let unauth_result = use_cases
            .process_inbound_email("sendgrid", unauth_email, "mailagents.com")
            .await
            .unwrap();

        assert!(!unauth_result.resolved);
        assert!(!unauth_result.sender_authorized);
    }

    #[tokio::test]
    async fn test_channel_creation_spam_disabled_confirmation() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        // Config with spam scanning completely disabled
        let use_cases =
            ChannelUseCases::new(company_persistence, channel_persistence, test_config(false));

        // 1. Trying to create public channel (@public) without confirmation fails when spam scanning is disabled
        let res = use_cases
            .create_channel(
                owner_id,
                company_id,
                ChannelWrite {
                    name: "Public Flow".into(),
                    slug: "public".into(),
                    participant_emails: Some(vec!["@public".to_string()]),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await;
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("Spam scanning is disabled")
        );

        // 2. Creating public channel (@public) BUT WITH confirm_spam_disabled=true succeeds
        let res_ok = use_cases
            .create_channel(
                owner_id,
                company_id,
                ChannelWrite {
                    name: "Public Flow".into(),
                    slug: "public".into(),
                    participant_emails: Some(vec!["@public".to_string()]),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                true,
            )
            .await;
        assert!(res_ok.is_ok());

        // 3. Creating channel WITH participants even without confirmation succeeds
        let res_participants = use_cases
            .create_channel(
                owner_id,
                company_id,
                ChannelWrite {
                    name: "Restricted Flow".into(),
                    slug: "restricted".into(),
                    participant_emails: Some(vec!["allowed@example.com".to_string()]),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await;
        assert!(res_participants.is_ok());
    }
}
