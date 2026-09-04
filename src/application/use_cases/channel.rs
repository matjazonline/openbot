use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        channel::{
            Channel, PUBLIC_PARTICIPANT, RESERVED_SLUG_SUFFIXES, RESERVED_SUFFIX_SEPARATORS,
        },
        company::Company,
        creation::CreationProvenance,
        participant::{IdentityProvenance, PrincipalAccessContext},
        transport::{ChannelSelector, QualifiedIdentity},
        user::Viewer,
        value_objects::{ChannelSlug, CompanySlug},
    },
    infra::config::AppConfig,
    use_cases::agent::{AgentWrite, SpamScanning},
    use_cases::company::{CompanyPersistence, managed_company},
    use_cases::memory::{ActiveMemoryBinding, MemoryBindingPersistence},
    use_cases::participant::{ParticipantPersistence, observe_access_context},
};

/// Everything one channel write sets, so create and update cannot drift apart and so a caller
/// cannot transpose two same-typed arguments in a nine-parameter list.
///
/// Values reach persistence already normalized — see [`ChannelWrite::normalize`].
#[derive(Debug, Clone, Default)]
pub struct ChannelWrite {
    pub name: String,
    /// What the channel is for, in one line. `None` and a blank string mean the same thing, so
    /// every form boundary normalizes blank to `None` before building the write.
    pub description: Option<String>,
    pub slug: String,
    /// Extra local parts the channel also answers on. Replaced wholesale by every write.
    pub alias_slugs: Vec<String>,
    pub participant_emails: Option<Vec<String>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub enabled: bool,
    /// Whether a trusted sender may pull CC'd outsiders onto this channel's threads.
    pub add_3rd_party: bool,
    pub retrieve_company_memory: bool,
    pub retrieve_agent_memory: bool,
    pub retrieve_user_memory: bool,
    pub persist_company_memory: bool,
    pub persist_agent_memory: bool,
    pub persist_user_memory: bool,
    pub created_by: Option<CreationProvenance>,
}

impl ChannelWrite {
    /// Trim and lower-case the fields that have canonical forms, and drop the blanks. Runs once,
    /// in the use case, so every entry point stores the same shape.
    pub(crate) fn normalize(&mut self) -> AppResult<()> {
        self.normalize_with(ActiveAgent::InWrite)
    }

    /// [`normalize`](Self::normalize), told where the position-0 agent is coming from.
    ///
    /// The distinction exists because a caller creating the channel and its agent in one
    /// transaction has no agent id to put in `agent_ids` yet -- the write is still valid, the
    /// assignment just arrives with it. Saying that in an argument keeps the rule in one place;
    /// the alternative -- flipping `enabled` off around the call -- silently skips every future
    /// `enabled`-dependent check as well as this one.
    pub(crate) fn normalize_with(&mut self, active_agent: ActiveAgent) -> AppResult<()> {
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

        if let Some(emails) = self.participant_emails.as_mut() {
            emails.retain_mut(|email| {
                *email = email.trim().to_lowercase();
                !email.is_empty() && email.contains('@')
            });
        }

        if matches!(active_agent, ActiveAgent::InWrite)
            && self.enabled
            && self.agent_ids.as_ref().is_none_or(Vec::is_empty)
        {
            return Err(AppError::BadRequest(
                "An enabled channel must have an active agent at position 0.".into(),
            ));
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
    pub(crate) fn is_public(&self) -> bool {
        self.participant_emails
            .as_ref()
            .is_some_and(|emails| emails.iter().any(|e| e == PUBLIC_PARTICIPANT))
    }
}

/// A channel open to `@public` must not be saved while the server has no spam scanning at all,
/// unless the caller explicitly says it knows.
///
/// Free-standing because two use cases enforce it: the Channels workspace saving a channel, and
/// the Agents workspace creating an agent whose personal channel was written on the create form's
/// channel step. One copy means the two cannot come to refuse it in different words.
pub(crate) fn check_spam_interlock(
    write: &ChannelWrite,
    spam_scanning: SpamScanning,
    confirmed: bool,
) -> AppResult<()> {
    if write.is_public() && spam_scanning == SpamScanning::Unavailable && !confirmed {
        return Err(AppError::BadRequest(
            "Spam scanning is disabled in server configuration. Saving a public channel (@public) requires explicit confirmation (confirm_spam_disabled) that you are aware spam scanning is disabled.".into(),
        ));
    }
    Ok(())
}

/// Where a channel write's position-0 agent comes from, for [`ChannelWrite::normalize_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveAgent {
    /// `agent_ids` is the whole story: an enabled channel with none is rejected.
    InWrite,
    /// The caller creates the agent alongside the channel in one transaction, so an empty
    /// `agent_ids` is not yet a missing agent. The database's deferred
    /// `enabled_channel_active_agent_check` is what holds the invariant on this path.
    SuppliedByCaller,
}

/// A channel belonging to another company is reported exactly like a missing one, so an id probe
/// cannot tell a foreign channel from a nonexistent one. See [`managed_company`].
pub fn channel_not_found() -> AppError {
    AppError::NotFound("Channel not found in this company.".into())
}

#[async_trait]
pub trait ChannelPersistence: Send + Sync {
    async fn create(&self, company_id: Uuid, write: ChannelWrite) -> AppResult<Channel>;

    /// Atomically creates an executable agent, its channel, and the position-0 assignment.
    async fn create_with_agent(
        &self,
        _company_id: Uuid,
        _agent: AgentWrite,
        _channel: ChannelWrite,
    ) -> AppResult<(Agent, Channel)> {
        Err(AppError::Internal(
            "Atomic agent/channel creation is unavailable.".into(),
        ))
    }

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
    memory_persistence: Option<Arc<dyn MemoryBindingPersistence>>,
    participant_persistence: Arc<dyn ParticipantPersistence>,
}

impl ChannelUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        participant_persistence: Arc<dyn ParticipantPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        Self {
            company_persistence,
            channel_persistence,
            participant_persistence,
            config,
            memory_persistence: None,
        }
    }

    pub fn with_memory_persistence(
        mut self,
        persistence: Arc<dyn MemoryBindingPersistence>,
    ) -> Self {
        self.memory_persistence = Some(persistence);
        self
    }

    async fn verify_company_manager(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        managed_company(self.company_persistence.as_ref(), user_id, company_id).await?;
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
        self.verify_company_manager(user_id, company_id).await?;
        write.created_by = Some(CreationProvenance::user(user_id));

        write.normalize()?;
        self.check_spam_interlock(&write, confirm_spam_disabled)?;

        info!(
            "Creating channel '{}' ({}) for company {}",
            write.name, write.slug, company_id
        );

        self.channel_persistence.create(company_id, write).await
    }

    #[instrument(skip(self, agent))]
    pub async fn create_channel_with_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        mut agent: AgentWrite,
        mut channel: ChannelWrite,
        confirm_spam_disabled: bool,
    ) -> AppResult<Channel> {
        self.verify_company_manager(user_id, company_id).await?;
        let provenance = CreationProvenance::user(user_id);
        agent.created_by = Some(provenance.clone());
        channel.created_by = Some(provenance);
        agent.normalize()?;

        channel.normalize_with(ActiveAgent::SuppliedByCaller)?;
        self.check_spam_interlock(&channel, confirm_spam_disabled)?;

        let (_, channel) = self
            .channel_persistence
            .create_with_agent(company_id, agent, channel)
            .await?;
        Ok(channel)
    }

    /// This deployment's spam scanning, as the shared [`check_spam_interlock`] states it.
    fn spam_scanning(&self) -> SpamScanning {
        if self.config.is_spam_scan_enabled() {
            SpamScanning::Available
        } else {
            SpamScanning::Unavailable
        }
    }

    fn check_spam_interlock(
        &self,
        write: &ChannelWrite,
        confirm_spam_disabled: bool,
    ) -> AppResult<()> {
        check_spam_interlock(write, self.spam_scanning(), confirm_spam_disabled)
    }

    pub async fn memory_ready(&self, user_id: Uuid, company_id: Uuid) -> AppResult<bool> {
        self.verify_company_manager(user_id, company_id).await?;
        let Some(persistence) = self.memory_persistence.as_ref() else {
            return Ok(false);
        };
        let binding = persistence.active_binding(company_id).await?;
        let configured = self.config.configured_memory_providers();
        Ok(matches!(binding, ActiveMemoryBinding::Ready(connection)
            if configured.contains(connection.provider)))
    }

    #[instrument(skip(self))]
    pub async fn list_company_channels(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Channel>> {
        self.verify_company_manager(user_id, company_id).await?;
        self.channel_persistence
            .list_by_company_id(company_id)
            .await
    }

    /// Every channel of `company_id` this viewer may read.
    ///
    /// The read counterpart of [`ChannelUseCases::list_company_channels`]: that one is for the
    /// pages that *configure* channels and stays owner-or-admin, this one is what the mailbox lists,
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
            .filter(|channel| channel.viewer_access(access))
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

        Ok(channel
            .filter(|channel| channel.company_id == company_id && channel.viewer_access(access)))
    }

    /// What the viewer is to a company, or `None` if they are nothing to it.
    async fn viewer_access(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
    ) -> AppResult<Option<PrincipalAccessContext>> {
        if self
            .company_persistence
            .company_access(viewer.user_id, company_id)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        self.participant_persistence
            .access_context_for_user(company_id, viewer.user_id)
            .await
    }

    #[instrument(skip(self))]
    pub async fn get_company_channel(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<Channel>> {
        self.verify_company_manager(user_id, company_id).await?;
        let channel = self.channel_persistence.get_by_id(channel_id).await?;
        if let Some(ref ch) = channel
            && ch.company_id != company_id
        {
            return Ok(None);
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
        self.verify_company_manager(user_id, company_id).await?;

        let channel = self
            .channel_persistence
            .get_by_id(channel_id)
            .await?
            .ok_or_else(channel_not_found)?;

        if channel.company_id != company_id {
            return Err(channel_not_found());
        }

        if let Some(owner_agent_id) = channel.owner_agent_id {
            write.slug = channel.slug.to_string();
            let mut agents = vec![owner_agent_id];
            agents.extend(
                write
                    .agent_ids
                    .take()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|agent_id| *agent_id != owner_agent_id),
            );
            write.agent_ids = Some(agents);
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
        self.verify_company_manager(user_id, company_id).await?;

        let channel = self
            .channel_persistence
            .get_by_id(channel_id)
            .await?
            .ok_or_else(channel_not_found)?;

        if channel.company_id != company_id {
            return Err(channel_not_found());
        }
        if channel.owner_agent_id.is_some() {
            return Err(AppError::Conflict(
                "This is an agent-owned personal channel. Delete the owning agent instead.".into(),
            ));
        }

        info!("Deleting channel {} for company {}", channel_id, company_id);
        self.channel_persistence.delete(channel_id).await
    }

    #[instrument(skip(self, sender))]
    pub async fn preview_inbound_route(
        &self,
        company_slug: CompanySlug,
        channel_slug: ChannelSlug,
        sender: QualifiedIdentity,
    ) -> AppResult<InboundRoutePreview> {
        let company = self.company_persistence.get_by_slug(&company_slug).await?;
        let channel = self
            .channel_persistence
            .get_by_company_slug_and_channel_slug(&company_slug, &channel_slug)
            .await?;
        let sender_authorized = match (company.as_ref(), channel.as_ref()) {
            (Some(company), Some(channel)) => {
                let context = observe_access_context(
                    self.participant_persistence.as_ref(),
                    company.id,
                    &sender,
                    IdentityProvenance::TransportIngress,
                )
                .await?;
                channel.participant_access(context).authorized
            }
            _ => false,
        };
        Ok(InboundRoutePreview {
            resolved: company.is_some() && channel.is_some() && sender_authorized,
            sender_authorized,
            company_slug,
            channel_slug,
            company,
            channel,
        })
    }
}

#[derive(Debug, Clone)]
pub struct InboundRoutePreview {
    pub resolved: bool,
    pub sender_authorized: bool,
    pub company_slug: CompanySlug,
    pub channel_slug: ChannelSlug,
    pub company: Option<Company>,
    pub channel: Option<Channel>,
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

    for suffix in RESERVED_SLUG_SUFFIXES {
        let reserved = slug_clean == *suffix
            || RESERVED_SUFFIX_SEPARATORS
                .iter()
                .any(|separator| slug_clean.ends_with(&format!("{separator}{suffix}")));
        if reserved {
            return Err(AppError::BadRequest(format!(
                "Invalid {} '{}': it cannot be, or end with, one of the reserved suffixes {} \
                 (optionally preceded by '.', '+', '-' or '_'). Those mark an address as \
                 context-only, so a channel named after one could never be replied to.",
                kind.noun(),
                slug_clean,
                RESERVED_SLUG_SUFFIXES.join(", ")
            )));
        }
    }

    Ok(())
}

/// Split `{local}@{company}.{app_domain}` into its company and its raw local part.
///
/// The local part comes back lowercased but otherwise **untouched** — no pipeline split, no
/// context-suffix stripping — because callers disagree about what it means. Channel routing expands
/// it; [`SystemAddress::parse`] must see it whole, before context-suffix handling could
/// eat a name like `_msg`.
/// Why a same-company channel cannot be called by another channel's agent.
///
/// Every variant is a rule the internal transport enforces anyway; naming them lets the caller
/// explain the refusal instead of returning a bare `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalTargetRejection {
    CrossCompany,
    SelfCall,
    Disabled,
    NoAgent,
}

impl InternalTargetRejection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CrossCompany => "Cross-company channel calls are not allowed",
            Self::SelfCall => "A channel cannot call itself",
            Self::Disabled => "Target channel is disabled",
            Self::NoAgent => "Target channel has no configured agent",
        }
    }
}

impl std::fmt::Display for InternalTargetRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Whether `target` may receive an internal channel call from the caller's channel.
///
/// One decision, one place: the outreach tool's send path and the agent directory both consult
/// this, so the directory can never advertise a channel the send path would refuse.
///
/// Address-shape rules (direct address, no `+` pipeline, no context-only suffix) are *not* checked
/// here — those belong to the address parser, and a resolved [`Channel`] no longer carries them.
pub fn check_internal_target(
    target: &Channel,
    caller_company_id: Uuid,
    caller_channel_id: Uuid,
) -> Result<(), InternalTargetRejection> {
    if target.company_id != caller_company_id {
        return Err(InternalTargetRejection::CrossCompany);
    }
    if target.id == caller_channel_id {
        return Err(InternalTargetRejection::SelfCall);
    }
    if !target.enabled {
        return Err(InternalTargetRejection::Disabled);
    }
    if target.agent_ids.as_ref().is_none_or(Vec::is_empty) {
        return Err(InternalTargetRejection::NoAgent);
    }
    Ok(())
}

/// What one outreach recipient turns out to be.
#[derive(Debug, Clone)]
pub enum InternalTargetOutcome {
    /// A same-company channel the caller is allowed to call.
    Callable(Box<Channel>),
    /// An address under the application domain that cannot be called, and why.
    Rejected(String),
}

/// Resolve a transport-neutral business-channel intent as callable or refused.
///
/// Shared by the outreach send path and the approval policy so that "is this a colleague or a
/// stranger?" is answered the same way in both. Persistence errors propagate rather than
/// degrading into `External` — misclassifying a channel as a stranger would mail the outside
/// world under a policy meant for internal traffic.
pub async fn resolve_internal_target(
    selector: &ChannelSelector,
    caller_company_id: Uuid,
    caller_channel_id: Uuid,
    channel_persistence: &dyn ChannelPersistence,
) -> AppResult<InternalTargetOutcome> {
    let channel = match selector {
        ChannelSelector::CurrentCompany(channel_slug) => channel_persistence
            .list_by_company_id(caller_company_id)
            .await?
            .into_iter()
            .find(|channel| channel.matches_slug(channel_slug)),
        ChannelSelector::Qualified { company, channel } => {
            channel_persistence
                .get_by_company_slug_and_channel_slug(company, channel)
                .await?
        }
    };
    let Some(channel) = channel else {
        return Ok(InternalTargetOutcome::Rejected(format!(
            "Selected platform channel does not exist: {selector}"
        )));
    };

    match check_internal_target(&channel, caller_company_id, caller_channel_id) {
        Ok(()) => Ok(InternalTargetOutcome::Callable(Box::new(channel))),
        Err(rejection) => Ok(InternalTargetOutcome::Rejected(format!(
            "{rejection}: {selector}"
        ))),
    }
}

pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut dp = vec![vec![0; n + 1]; m + 1];

    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate().take(n + 1) {
        *cell = j;
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
    use crate::adapters::protocols::email::EmailChannelSelectorParser;
    use crate::entities::channel::ChannelAccessMode;
    use crate::entities::company::{Company, CompanyAccess};
    use crate::entities::company_member::CompanyMembership;
    use crate::entities::value_objects::EmailAddress;
    use crate::use_cases::company::CompanyWrite;
    use crate::use_cases::participant::test_support::{
        InMemoryParticipantDirectory, TeamFixture, email_allowlist_policy,
    };
    use chrono::Utc;
    use std::sync::Mutex;

    fn parse_platform_address(value: &str, domain: &str) -> Option<(CompanySlug, String)> {
        EmailChannelSelectorParser::new(domain).parse_platform_address(value)
    }

    fn parse_recipient_address_pipeline(
        value: &str,
        domain: &str,
    ) -> Option<(CompanySlug, Vec<ChannelSlug>, bool)> {
        let selection = EmailChannelSelectorParser::new(domain).parse(value)?;
        let company = selection.primary().company()?.clone();
        let channels = selection
            .selectors()
            .iter()
            .map(ChannelSelector::channel)
            .cloned()
            .collect();
        Some((company, channels, selection.delivery().is_context_only()))
    }

    fn parse_recipient_address(value: &str, domain: &str) -> Option<(CompanySlug, ChannelSlug)> {
        parse_recipient_address_pipeline(value, domain)
            .and_then(|(company, channels, _)| channels.into_iter().next().map(|c| (company, c)))
    }

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
        memberships: Mutex<Vec<(Uuid, Uuid, CompanyMembership)>>,
    }

    /// The signed-in half is what these tests drive, so `company_access` carries the memberships
    /// this double was built with and the by-address half is never reached.
    #[async_trait]
    impl TeamFixture for MockCompanyPersistence {
        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }

        async fn company_access(
            &self,
            user_id: Uuid,
            company_id: Uuid,
        ) -> AppResult<Option<CompanyAccess>> {
            CompanyPersistence::company_access(self, user_id, company_id).await
        }
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
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

        async fn list_accessible_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
            let companies = self.companies.lock().unwrap();
            let memberships = self.memberships.lock().unwrap();
            Ok(companies
                .iter()
                .filter_map(|company| {
                    let membership = if company.user_id == user_id {
                        CompanyMembership::Owner
                    } else if let Some((_, _, membership)) =
                        memberships.iter().find(|(member_id, company_id, _)| {
                            *member_id == user_id && *company_id == company.id
                        })
                    {
                        *membership
                    } else {
                        return None;
                    };
                    Some(CompanyAccess {
                        company: company.clone(),
                        membership,
                    })
                })
                .collect())
        }

        async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }

        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }

        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }

        async fn list_company_team_accounts(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyTeamAccount>> {
            unimplemented!("this double is not exercised on the team-account path")
        }

        /// Model connections are not part of what these tests drive; a call here is a wiring mistake
        /// rather than a state worth simulating.
        async fn list_model_connections(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
            unimplemented!("this double is not exercised on the model-connection path")
        }

        async fn model_api_key(
            &self,
            _company_id: Uuid,
            _provider: &crate::entities::value_objects::ModelProvider,
        ) -> AppResult<Option<String>> {
            unimplemented!("this double is not exercised on the model-connection path")
        }

        async fn replace_model_connections_for_user(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
            _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
        ) -> AppResult<()> {
            unimplemented!("this double is not exercised on the model-connection path")
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
                owner_agent_id: None,
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
        let participant_emails: Option<Vec<EmailAddress>> = write
            .participant_emails
            .map(|emails| emails.into_iter().map(EmailAddress::from).collect());
        // The mock resolves the entered addresses the way the SQL writer does, so a form
        // round-trip in these tests exercises the same projection the real one stores.
        let (access_mode, principal_grants) =
            email_allowlist_policy(company_id, participant_emails.as_deref());
        Channel {
            owner_agent_id: None,
            id,
            company_id,
            name: write.name,
            description: None,
            slug: write.slug.into(),
            alias_slugs: Vec::new(),
            participant_emails,
            access_mode,
            principal_grants,
            agent_ids: write.agent_ids,
            enabled: write.enabled,
            add_3rd_party: write.add_3rd_party,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: write.created_by.unwrap_or_else(CreationProvenance::system),
            created_at: Utc::now(),
        }
    }

    fn test_config(spam_enabled: bool) -> Arc<AppConfig> {
        Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: false,
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
            channel_defaults: Default::default(),
            id,
            user_id: owner_id,
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![company(company_id), company(other_company_id)]),
            memberships: Mutex::new(Vec::new()),
        });
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });
        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_config(true),
        );

        // The owner owns both companies, so this is a cross-tenant id, not an authorization miss.
        let foreign = use_cases
            .create_channel(
                owner_id,
                other_company_id,
                ChannelWrite {
                    name: "Elsewhere".into(),
                    slug: "elsewhere".into(),
                    enabled: false,
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
            enabled: false,
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
    async fn admins_manage_channels_while_members_cannot() {
        let owner_id = Uuid::new_v4();
        let admin_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".into(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
            memberships: Mutex::new(vec![
                (admin_id, company_id, CompanyMembership::Admin),
                (member_id, company_id, CompanyMembership::Member),
            ]),
        });
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });
        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_config(true),
        );

        assert!(
            use_cases
                .create_channel(
                    member_id,
                    company_id,
                    ChannelWrite {
                        name: "Member Channel".into(),
                        slug: "member-channel".into(),
                        ..ChannelWrite::default()
                    },
                    false,
                )
                .await
                .is_err()
        );
        assert!(
            use_cases
                .list_company_channels(member_id, company_id)
                .await
                .is_err()
        );

        let channel = use_cases
            .create_channel(
                admin_id,
                company_id,
                ChannelWrite {
                    name: "Admin Channel".into(),
                    slug: "admin-channel".into(),
                    enabled: false,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .expect("an admin creates a channel");
        assert_eq!(
            use_cases
                .list_company_channels(admin_id, company_id)
                .await
                .expect("an admin lists channels")
                .len(),
            1
        );
        let channel = use_cases
            .update_channel(
                admin_id,
                company_id,
                channel.id,
                ChannelWrite {
                    name: "Managed Channel".into(),
                    slug: "managed-channel".into(),
                    enabled: false,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .expect("an admin updates a channel");
        assert_eq!(channel.name, "Managed Channel");
        use_cases
            .delete_channel(admin_id, company_id, channel.id)
            .await
            .expect("an admin deletes a channel");
    }

    #[tokio::test]
    async fn company_owner_channel_crud_flow_works() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
            memberships: Mutex::new(Vec::new()),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_config(true),
        );

        // 1. Owner creates channel with participant emails and config
        let emails = vec![
            "agent1@example.com".to_string(),
            "agent2@example.com".to_string(),
        ];

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
                    participant_emails: Some(emails.clone()),
                    agent_ids: Some(agent_ids.clone()),
                    enabled: false,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .unwrap();

        assert_eq!(channel.name, "Support Flow");
        assert_eq!(channel.slug, "support-flow");
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

        // 2. Non-owner cannot create channel
        let non_owner_id = Uuid::new_v4();
        let err = use_cases
            .create_channel(
                non_owner_id,
                company_id,
                ChannelWrite {
                    name: "Hacker Flow".into(),
                    slug: "hacker-flow".into(),
                    enabled: false,
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
        let updated = use_cases
            .update_channel(
                owner_id,
                company_id,
                channel.id,
                ChannelWrite {
                    name: "Updated Flow".into(),
                    slug: "updated-flow".into(),
                    enabled: false,
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
            enabled: false,
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
            enabled: false,
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
                owner_agent_id: None,
                enabled: false,
                add_3rd_party: true,
                id: uuid::Uuid::new_v4(),
                company_id,
                name: "Support".to_string(),
                description: None,
                slug: "support".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                agent_ids: None,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: chrono::Utc::now(),
            },
            Channel {
                owner_agent_id: None,
                enabled: false,
                add_3rd_party: true,
                id: uuid::Uuid::new_v4(),
                company_id,
                name: "Billing".to_string(),
                description: None,
                slug: "billing".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                agent_ids: None,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
                created_by: crate::entities::creation::CreationProvenance::system(),
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
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
            memberships: Mutex::new(Vec::new()),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_config(true),
        );

        let _ = use_cases
            .create_channel(
                owner_id,
                company_id,
                ChannelWrite {
                    name: "Support Flow".into(),
                    slug: "support".into(),
                    enabled: false,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .unwrap();

        let result = use_cases
            .preview_inbound_route(
                "acme".into(),
                "support".into(),
                crate::use_cases::thread::qualified_email_identity("customer@example.com").unwrap(),
            )
            .await
            .unwrap();

        assert!(result.resolved);
        assert!(result.sender_authorized);
        assert_eq!(result.company_slug, "acme");
        assert_eq!(result.channel_slug, "support");
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
                    enabled: false,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await
            .unwrap();

        // 1. Authorized sender
        let auth_result = use_cases
            .preview_inbound_route(
                "acme".into(),
                "restricted".into(),
                crate::use_cases::thread::qualified_email_identity("agent@example.com").unwrap(),
            )
            .await
            .unwrap();

        assert!(auth_result.resolved);
        assert!(auth_result.sender_authorized);

        // 2. Unauthorized sender
        let unauth_result = use_cases
            .preview_inbound_route(
                "acme".into(),
                "restricted".into(),
                crate::use_cases::thread::qualified_email_identity("stranger@external.com")
                    .unwrap(),
            )
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
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
            memberships: Mutex::new(Vec::new()),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        // Config with spam scanning completely disabled
        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_config(false),
        );

        // 1. Trying to create public channel (@public) without confirmation fails when spam scanning is disabled
        let res = use_cases
            .create_channel(
                owner_id,
                company_id,
                ChannelWrite {
                    name: "Public Flow".into(),
                    slug: "public".into(),
                    participant_emails: Some(vec!["@public".to_string()]),
                    enabled: false,
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
                    enabled: false,
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
                    enabled: false,
                    ..ChannelWrite::default()
                },
                false,
            )
            .await;
        assert!(res_participants.is_ok());
    }

    #[test]
    fn a_platform_address_splits_into_its_company_and_untouched_local_part() {
        let domain = "mailagents.com";

        let (company, local) =
            parse_platform_address("_help@acme.mailagents.com", domain).expect("ours");
        assert_eq!(company, "acme");
        assert_eq!(
            local, "_help",
            "the local part must survive whole -- no pipeline split, no suffix stripping"
        );

        let (_, piped) =
            parse_platform_address("Support Desk <SUPPORT+Billing@acme.mailagents.com>", domain)
                .expect("ours");
        assert_eq!(
            piped, "support+billing",
            "a display name is stripped and the address lowercased, but nothing is expanded"
        );

        assert!(parse_platform_address("someone@elsewhere.com", domain).is_none());
        assert!(
            parse_platform_address("someone@mailagents.com", domain).is_none(),
            "the bare application domain names no company"
        );
        assert!(parse_platform_address("not-an-address", domain).is_none());
    }

    #[test]
    fn the_platform_domain_test_is_wider_than_the_address_parser() {
        // Email destination classification relies on this gap: `someone@mailagents.com` is ours
        // but malformed, and must not be classified as an external stranger.
        let parser = EmailChannelSelectorParser::new("mailagents.com");
        assert!(parser.is_platform_domain("mailagents.com"));
        assert!(parser.is_platform_domain("acme.mailagents.com"));
        assert!(!parser.is_platform_domain("elsewhere.com"));
        assert!(!parser.is_platform_domain("notmailagents.com"));
    }
}
