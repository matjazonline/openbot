use std::sync::Arc;

use crate::application::ports::channel::{ChannelPersistence, ChannelUpdate, ChannelWrite};

#[cfg(test)]
use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        channel::{
            Channel, PUBLIC_PARTICIPANT, RESERVED_SLUG_SUFFIXES, RESERVED_SUFFIX_SEPARATORS,
        },
        company::Company,
        creation::CreationProvenance,
        participant::{IdentityProvenance, PrincipalAccessContext},
        transport::QualifiedIdentity,
        user::Viewer,
        value_objects::{ChannelSlug, CompanySlug},
    },
    use_cases::agent::SpamScanning,
    use_cases::company::{CompanyPersistence, managed_company},
    use_cases::participant::{ParticipantPersistence, observe_access_context},
};

impl ChannelWrite {
    /// Trim and lower-case the fields that have canonical forms, and drop the blanks. Runs once,
    /// in the use case, so every entry point stores the same shape. A channel needs no agent: its
    /// automation is a workflow binding (P08-D4 §D4.7).
    pub(crate) fn normalize(&mut self) -> AppResult<()> {
        self.name = self.name.trim().to_string();
        self.slug = self.slug.trim().to_lowercase().replace(' ', "-");

        if self.name.is_empty() {
            return Err(AppError::BadRequest(
                "The desk name cannot be empty.".into(),
            ));
        }
        if self.slug.is_empty() {
            return Err(AppError::BadRequest(
                "The desk address cannot be empty.".into(),
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
/// unless the caller explicitly says it knows. Create and update share this one copy, so the two
/// cannot come to refuse it in different words.
fn check_spam_interlock(
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

/// A channel belonging to another company is reported exactly like a missing one, so an id probe
/// cannot tell a foreign channel from a nonexistent one. See [`managed_company`].
pub fn channel_not_found() -> AppError {
    AppError::NotFound("Desk not found in this company.".into())
}

/// The ports the channel read rule needs, borrowed from whichever use case asks.
#[derive(Clone, Copy)]
pub(crate) struct ChannelReaders<'a> {
    pub(crate) companies: &'a dyn CompanyPersistence,
    pub(crate) channels: &'a dyn ChannelPersistence,
    pub(crate) participants: &'a dyn ParticipantPersistence,
}

/// What the viewer is to a company as a principal, or `None` if they are nothing to it.
async fn viewer_access(
    readers: &ChannelReaders<'_>,
    viewer: &Viewer,
    company_id: Uuid,
) -> AppResult<Option<PrincipalAccessContext>> {
    if readers
        .companies
        .company_access(viewer.user_id, company_id)
        .await?
        .is_none()
    {
        return Ok(None);
    }
    readers
        .participants
        .access_context_for_user(company_id, viewer.user_id)
        .await
}

/// The one rule for "may this viewer read this channel": a member of its company whom
/// [`Channel::viewer_access`] lets in. `None` for an unknown channel, another company's channel and
/// a channel the viewer may not read alike; a lookup failure is an error, never a `None`.
pub(crate) async fn readable_channel(
    readers: &ChannelReaders<'_>,
    viewer: &Viewer,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<Option<Channel>> {
    let Some(access) = viewer_access(readers, viewer, company_id).await? else {
        return Ok(None);
    };
    let channel = readers.channels.get_by_id(channel_id).await?;
    Ok(channel.filter(|channel| channel.company_id == company_id && channel.viewer_access(access)))
}

/// Every channel of the company this viewer may read, by the same rule as [`readable_channel`].
pub(crate) async fn readable_channels(
    readers: &ChannelReaders<'_>,
    viewer: &Viewer,
    company_id: Uuid,
) -> AppResult<Vec<Channel>> {
    let Some(access) = viewer_access(readers, viewer, company_id).await? else {
        return Ok(Vec::new());
    };
    Ok(readers
        .channels
        .list_by_company_id(company_id)
        .await?
        .into_iter()
        .filter(|channel| channel.viewer_access(access))
        .collect())
}

#[derive(Clone)]
pub struct ChannelUseCases {
    company_persistence: Arc<dyn CompanyPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    spam_scanning: SpamScanning,
    participant_persistence: Arc<dyn ParticipantPersistence>,
}

impl ChannelUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        participant_persistence: Arc<dyn ParticipantPersistence>,
        spam_scanning: SpamScanning,
    ) -> Self {
        Self {
            company_persistence,
            channel_persistence,
            participant_persistence,
            spam_scanning,
        }
    }

    async fn verify_company_manager(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        managed_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        Ok(())
    }

    #[instrument(skip_all)]
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

        info!(%company_id, "Creating channel");

        self.channel_persistence.create(company_id, write).await
    }

    fn check_spam_interlock(
        &self,
        write: &ChannelWrite,
        confirm_spam_disabled: bool,
    ) -> AppResult<()> {
        check_spam_interlock(write, self.spam_scanning, confirm_spam_disabled)
    }

    #[instrument(skip_all)]
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

    /// Manager-only reads use the same visibility decision as the mailbox.
    pub async fn list_managed_readable_channels(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
    ) -> AppResult<Vec<Channel>> {
        self.verify_company_manager(viewer.user_id, company_id)
            .await?;
        self.list_readable_channels(viewer, company_id).await
    }

    /// Every channel of `company_id` this viewer may read.
    ///
    /// The read counterpart of [`ChannelUseCases::list_company_channels`]: that one is for the
    /// pages that *configure* channels and stays owner-or-admin, this one is what the mailbox lists,
    /// and a restricted channel simply is not in it for a colleague who is not a participant.
    #[instrument(skip_all)]
    pub async fn list_readable_channels(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
    ) -> AppResult<Vec<Channel>> {
        readable_channels(&self.readers(), viewer, company_id).await
    }

    /// One channel, if this viewer may read it.
    ///
    /// Answers `None` for "no such channel", "not your company" and "not yours to read" alike:
    /// telling them apart would let anyone probe ids to learn which channels a company runs.
    #[instrument(skip_all)]
    pub async fn get_readable_channel(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<Channel>> {
        readable_channel(&self.readers(), viewer, company_id, channel_id).await
    }

    fn readers(&self) -> ChannelReaders<'_> {
        ChannelReaders {
            companies: self.company_persistence.as_ref(),
            channels: self.channel_persistence.as_ref(),
            participants: self.participant_persistence.as_ref(),
        }
    }

    #[instrument(skip_all)]
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

    #[instrument(skip_all)]
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

        write.normalize()?;
        self.check_spam_interlock(&write, confirm_spam_disabled)?;

        info!(%channel_id, %company_id, enabled = write.enabled, "Updating channel");

        self.channel_persistence
            .update(ChannelUpdate {
                company_id,
                channel_id,
                write,
            })
            .await
    }

    #[instrument(skip_all)]
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

        info!("Deleting channel {} for company {}", channel_id, company_id);
        self.channel_persistence.delete(channel_id).await
    }

    #[instrument(skip_all)]
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
    DepartmentAddress,
}

impl SlugKind {
    fn noun(self) -> &'static str {
        match self {
            SlugKind::ChannelAddress => "desk address",
            SlugKind::ChannelAlias => "desk alias",
            SlugKind::AgentSlug => "agent slug",
            SlugKind::DepartmentAddress => "department address",
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
    use crate::entities::company::Company;
    use crate::entities::company_member::CompanyMembership;
    use crate::entities::transport::ChannelSelector;
    use crate::entities::value_objects::EmailAddress;
    use crate::use_cases::company_test_support::{MockCompanyPersistence, TeamByAddress};
    use crate::use_cases::participant::test_support::{
        InMemoryParticipantDirectory, email_allowlist_policy,
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

        async fn list_departments(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::department::Department>> {
            Ok(Vec::new())
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

        async fn update(&self, request: ChannelUpdate) -> AppResult<Channel> {
            let mut list = self.channels.lock().unwrap();
            let existing = list
                .iter_mut()
                .find(|w| w.id == request.channel_id && w.company_id == request.company_id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;
            *existing = Channel {
                created_at: existing.created_at,
                ..channel_from_write(request.channel_id, existing.company_id, request.write)
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
            id,
            company_id,
            name: write.name,
            description: None,
            slug: write.slug.into(),
            alias_slugs: Vec::new(),
            participant_emails,
            access_mode,
            principal_grants,
            enabled: write.enabled,
            add_3rd_party: write.add_3rd_party,
            created_by: write.created_by.unwrap_or_else(CreationProvenance::system),
            created_at: Utc::now(),
            settings_revision: 0,
            department_id: None,
            members_only: false,
        }
    }

    fn test_scanning(spam_enabled: bool) -> SpamScanning {
        if spam_enabled {
            SpamScanning::Available
        } else {
            SpamScanning::Unavailable
        }
    }

    #[tokio::test]
    async fn security_email_identity_unverified_account_cannot_read_participant_channel() {
        let owner_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let company_persistence = Arc::new(
            MockCompanyPersistence::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme".into(),
                slug: "acme".into(),
                avatar_url: None,
                created_at: Utc::now(),
                channel_defaults: Default::default(),
            }])
            .with_memberships(vec![(member_id, company_id, CompanyMembership::Member)])
            .with_team(TeamByAddress::EveryoneIsMember),
        );
        let channel = channel_from_write(
            channel_id,
            company_id,
            ChannelWrite {
                name: "Private".into(),
                slug: "private".into(),
                participant_emails: Some(vec!["member@example.com".into()]),
                enabled: true,
                ..ChannelWrite::default()
            },
        );
        let directory = Arc::new(
            InMemoryParticipantDirectory::new()
                .with_team(company_persistence.clone())
                .with_user(company_id, member_id, "member@example.com"),
        );
        let cases = ChannelUseCases::new(
            company_persistence,
            Arc::new(MockChannelPersistence {
                channels: Mutex::new(vec![channel]),
            }),
            directory.clone(),
            test_scanning(false),
        );
        let mut viewer = Viewer {
            user_id: member_id,
            email: EmailAddress::from("member@example.com"),
            email_verified_at: Some(Utc::now()),
        };
        assert!(
            cases
                .get_readable_channel(&viewer, company_id, channel_id)
                .await
                .unwrap()
                .is_some()
        );
        directory.revoke_user_email_proof(company_id, member_id);
        viewer.email_verified_at = None;
        assert!(
            cases
                .get_readable_channel(&viewer, company_id, channel_id)
                .await
                .unwrap()
                .is_none()
        );
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
            avatar_url: None,
            created_at: Utc::now(),
        };
        let company_persistence = Arc::new(
            MockCompanyPersistence::new(vec![company(company_id), company(other_company_id)])
                .with_memberships(Vec::new())
                .with_team(TeamByAddress::EveryoneIsMember),
        );
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });
        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_scanning(true),
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
        let company_persistence = Arc::new(
            MockCompanyPersistence::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".into(),
                slug: "acme".into(),
                avatar_url: None,
                created_at: Utc::now(),
            }])
            .with_memberships(vec![
                (admin_id, company_id, CompanyMembership::Admin),
                (member_id, company_id, CompanyMembership::Member),
            ])
            .with_team(TeamByAddress::EveryoneIsMember),
        );
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });
        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_scanning(true),
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

        let company_persistence = Arc::new(
            MockCompanyPersistence::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                avatar_url: None,
                created_at: Utc::now(),
            }])
            .with_memberships(Vec::new())
            .with_team(TeamByAddress::EveryoneIsMember),
        );

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_scanning(true),
        );

        // 1. Owner creates channel with participant emails and config
        let emails = vec![
            "agent1@example.com".to_string(),
            "agent2@example.com".to_string(),
        ];

        let channel = use_cases
            .create_channel(
                owner_id,
                company_id,
                ChannelWrite {
                    name: "Support Flow".into(),
                    slug: "support-flow".into(),
                    participant_emails: Some(emails.clone()),
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
        assert!(message.contains("desk alias"), "{message}");
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
            address_message.contains("desk address"),
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
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: chrono::Utc::now(),
                settings_revision: 0,
                department_id: None,
                members_only: false,
            },
            Channel {
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
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: chrono::Utc::now(),
                settings_revision: 0,
                department_id: None,
                members_only: false,
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

        let company_persistence = Arc::new(
            MockCompanyPersistence::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                avatar_url: None,
                created_at: Utc::now(),
            }])
            .with_memberships(Vec::new())
            .with_team(TeamByAddress::EveryoneIsMember),
        );

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_scanning(true),
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
                crate::application::ports::thread::qualified_email_identity("customer@example.com")
                    .unwrap(),
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
                crate::application::ports::thread::qualified_email_identity("agent@example.com")
                    .unwrap(),
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
                crate::application::ports::thread::qualified_email_identity(
                    "stranger@external.com",
                )
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

        let company_persistence = Arc::new(
            MockCompanyPersistence::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                avatar_url: None,
                created_at: Utc::now(),
            }])
            .with_memberships(Vec::new())
            .with_team(TeamByAddress::EveryoneIsMember),
        );

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });

        // Config with spam scanning completely disabled
        let use_cases = ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            test_scanning(false),
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
