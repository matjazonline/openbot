use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::company_member::CompanyMembership;
use crate::entities::{
    creation::CreationProvenance,
    value_objects::{ChannelSlug, CompanySlug, EmailAddress},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Email,
    WebChat,
    Slack,
    WhatsApp,
    Api,
}

impl ChannelType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChannelType::Email => "email",
            ChannelType::WebChat => "webchat",
            ChannelType::Slack => "slack",
            ChannelType::WhatsApp => "whatsapp",
            ChannelType::Api => "api",
        }
    }
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ParticipantIdentity {
    pub identity: String,
    pub protocol: ChannelType,
}

impl ParticipantIdentity {
    pub fn new(identity: impl Into<String>, protocol: ChannelType) -> Self {
        Self {
            identity: identity.into().trim().to_lowercase(),
            protocol,
        }
    }

    pub fn email(addr: impl Into<String>) -> Self {
        Self::new(addr, ChannelType::Email)
    }

    pub fn matches(&self, other_raw: &str) -> bool {
        let clean = other_raw.trim().to_lowercase();
        self.identity == clean
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Channel {
    pub id: Uuid,
    pub company_id: Uuid,
    /// The agent whose personal address this channel is, or `None` for standalone channels.
    #[serde(default)]
    pub owner_agent_id: Option<Uuid>,
    pub name: String,
    /// What this channel is for, in one line, as its owner wrote it.
    ///
    /// Shown to a teammate whose mail bounced off an address that does not exist, so the list of
    /// channels they *could* have written to explains itself. Defaulted on deserialize for the
    /// same reason as `enabled`: durable `background_tasks` payloads written before this field
    /// existed must still re-hydrate.
    #[serde(default)]
    pub description: Option<String>,
    pub slug: ChannelSlug,
    /// Extra local parts this channel also answers on, canonical [`Channel::slug`] excluded.
    ///
    /// Defaulted on deserialize for the same reason as `enabled`: durable `background_tasks`
    /// payloads written before aliases existed must still re-hydrate.
    #[serde(default)]
    pub alias_slugs: Vec<ChannelSlug>,
    pub participant_emails: Option<Vec<EmailAddress>>,
    pub agent_ids: Option<Vec<Uuid>>,
    /// Whether the channel takes traffic at all. A disabled channel keeps its threads and tasks
    /// but bounces inbound mail and cannot be an internal delivery target.
    ///
    /// Defaulted on deserialize because `Channel` is stored inside durable `background_tasks`
    /// payloads: tasks queued before this field existed must still re-hydrate.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether a trusted sender may pull CC'd outsiders onto this channel's threads.
    ///
    /// Off means the channel is internal: a third party is neither added to
    /// `Thread::participant_emails` nor copied on the agent's reply. Because thread membership is
    /// itself an authorization grant, their later mail to the channel then bounces.
    ///
    /// Defaulted on deserialize for the same reason as `enabled`.
    #[serde(default = "default_true")]
    pub add_3rd_party: bool,
    #[serde(default)]
    pub retrieve_company_memory: bool,
    #[serde(default)]
    pub retrieve_agent_memory: bool,
    #[serde(default)]
    pub retrieve_user_memory: bool,
    #[serde(default)]
    pub persist_company_memory: bool,
    #[serde(default)]
    pub persist_agent_memory: bool,
    #[serde(default)]
    pub persist_user_memory: bool,
    pub created_by: CreationProvenance,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

fn default_true() -> bool {
    true
}

/// Participant list entry that opens a channel to any sender.
pub const PUBLIC_PARTICIPANT: &str = "@public";

/// What a channel's participant list says about one sender.
///
/// `authorized` decides whether the message may enter the channel at all; `trusted` additionally
/// waives spam scoring and permits pulling third-party recipients into the thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticipantAccess {
    pub authorized: bool,
    pub trusted: bool,
}

impl Channel {
    /// The single definition of "may this sender use this channel".
    ///
    /// An empty or absent participant list means "company team members only". A list containing
    /// `@public` opens the channel to anyone, but only explicitly listed senders (or team members)
    /// are *trusted*.
    ///
    /// `membership` is what the account behind `sender` is to the company, so the owner exemption
    /// this shares with [`Channel::viewer_access`] can be applied here too: a restricted channel
    /// never shuts its own company's owner out, whether they are reading it or writing to it.
    /// Mail from an address that belongs to no account is simply [`CompanyMembership::None`].
    pub fn participant_access(
        &self,
        sender: &str,
        membership: CompanyMembership,
    ) -> ParticipantAccess {
        match &self.participant_emails {
            Some(allowed) if !allowed.is_empty() => {
                let is_public = allowed
                    .iter()
                    .any(|e| e.trim().eq_ignore_ascii_case(PUBLIC_PARTICIPANT));
                let explicitly_listed = allowed.iter().any(|e| {
                    !e.trim().eq_ignore_ascii_case(PUBLIC_PARTICIPANT)
                        && e.eq_ignore_ascii_case(sender)
                });
                ParticipantAccess {
                    authorized: is_public || explicitly_listed || membership.is_owner(),
                    trusted: explicitly_listed || membership.is_team(),
                }
            }
            _ => ParticipantAccess {
                authorized: membership.is_team(),
                trusted: membership.is_team(),
            },
        }
    }

    /// The single definition of "may this signed-in account *read* this channel".
    ///
    /// The mirror of [`Channel::participant_access`], which answers the same question for mail
    /// arriving from outside. The two differ in one deliberate way: `@public` decides what may
    /// *enter* a channel and never grants a *read*, so an open channel's traffic is still only
    /// visible to the company's own team. The owner exemption below is *not* one of the
    /// differences -- it holds on both sides, so the owner of a restricted channel they are not
    /// listed on can write to it as well as read it.
    ///
    /// | participant list | who may read it |
    /// | --- | --- |
    /// | absent or empty | the company team |
    /// | contains `@public` | the company team -- never a stranger |
    /// | specific addresses | those addresses, plus the company owner |
    ///
    /// A restricted channel is therefore *narrower* than the team: a colleague who is not a
    /// participant does not see it. The owner is the one exception, so a company cannot lock
    /// itself out of its own data.
    pub fn viewer_access(&self, viewer: &EmailAddress, membership: CompanyMembership) -> bool {
        if !membership.is_team() {
            return false;
        }

        if membership.is_owner() {
            return true;
        }

        match &self.participant_emails {
            Some(allowed) if !allowed.is_empty() => allowed.iter().any(|participant| {
                participant.trim().eq_ignore_ascii_case(PUBLIC_PARTICIPANT)
                    || participant.eq_ignore_case(viewer)
            }),
            _ => true,
        }
    }

    /// Who should be asked to approve an action on this channel: the first listed participant
    /// that names an actual person rather than the `@public` wildcard.
    pub fn preferred_approver(&self) -> Option<EmailAddress> {
        self.participant_emails
            .as_ref()?
            .iter()
            .find(|email| !email.eq_ignore_ascii_case(PUBLIC_PARTICIPANT))
            .cloned()
    }

    /// Every local part this channel answers on: the canonical slug first, then its aliases.
    pub fn slugs(&self) -> impl Iterator<Item = &ChannelSlug> {
        std::iter::once(&self.slug).chain(self.alias_slugs.iter())
    }

    /// Whether this channel is the given agent's personal address.
    ///
    /// The single definition of the ownership test: an owned channel's slug follows its owner's
    /// handle, its owner is pinned at position 0, and it can only be deleted through that agent —
    /// so the pages and use cases that special-case any of those all ask this rather than
    /// re-comparing `owner_agent_id` themselves.
    pub fn is_owned_by(&self, agent_id: Uuid) -> bool {
        self.owner_agent_id == Some(agent_id)
    }

    /// The single definition of "does this addressed slug reach this channel".
    pub fn matches_slug(&self, slug: &str) -> bool {
        self.slugs().any(|own| own.eq_ignore_ascii_case(slug))
    }

    /// The address inbound mail reaches this channel on: `{channel}@{company}.{app domain}`.
    ///
    /// The same address the simulator sends to and the mailbox composes to, so it lives here
    /// rather than being re-`format!`ed per page.
    pub fn inbound_address(
        &self,
        company_slug: &CompanySlug,
        app_domain_name: &str,
    ) -> EmailAddress {
        Self::address_for(&self.slug, company_slug, app_domain_name)
    }

    /// Every address this channel is reachable at, canonical first.
    pub fn inbound_addresses(
        &self,
        company_slug: &CompanySlug,
        app_domain_name: &str,
    ) -> Vec<EmailAddress> {
        self.slugs()
            .map(|slug| Self::address_for(slug, company_slug, app_domain_name))
            .collect()
    }

    /// The address form, for a slug that may be an alias rather than [`Channel::slug`].
    pub fn address_for(
        channel_slug: &ChannelSlug,
        company_slug: &CompanySlug,
        app_domain_name: &str,
    ) -> EmailAddress {
        EmailAddress::new(format!("{channel_slug}@{company_slug}.{app_domain_name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A channel whose participant list is the only thing under test.
    fn channel_for(participants: Option<&[&str]>) -> Channel {
        Channel {
            owner_agent_id: None,
            participant_emails: participants.map(|list| {
                list.iter()
                    .map(|email| EmailAddress::from(*email))
                    .collect()
            }),
            ..channel_with_aliases(&[])
        }
    }

    #[test]
    fn an_unrestricted_channel_is_readable_by_the_team_and_nobody_else() {
        let viewer = EmailAddress::from("dana@acme.test");

        for channel in [channel_for(None), channel_for(Some(&[]))] {
            assert!(channel.viewer_access(&viewer, CompanyMembership::Owner));
            assert!(channel.viewer_access(&viewer, CompanyMembership::Admin));
            assert!(channel.viewer_access(&viewer, CompanyMembership::Member));
            assert!(!channel.viewer_access(&viewer, CompanyMembership::None));
        }
    }

    #[test]
    fn a_restricted_channel_is_narrower_than_the_team() {
        let channel = channel_for(Some(&["dana@acme.test", "ops@partner.test"]));

        // A listed colleague sees it...
        assert!(channel.viewer_access(
            &EmailAddress::from("dana@acme.test"),
            CompanyMembership::Member
        ));
        // ...and an unlisted one does not, even though they are on the team.
        assert!(!channel.viewer_access(
            &EmailAddress::from("sam@acme.test"),
            CompanyMembership::Member
        ));
        assert!(!channel.viewer_access(
            &EmailAddress::from("sam@acme.test"),
            CompanyMembership::Admin
        ));
        // The owner is the one exception, so a company cannot lock itself out of its own data.
        assert!(channel.viewer_access(
            &EmailAddress::from("sam@acme.test"),
            CompanyMembership::Owner
        ));
        // Being listed is not a way into a company you do not belong to.
        assert!(!channel.viewer_access(
            &EmailAddress::from("ops@partner.test"),
            CompanyMembership::None
        ));
    }

    #[test]
    fn public_opens_a_channel_to_mail_not_to_readers() {
        let channel = channel_for(Some(&[PUBLIC_PARTICIPANT, "dana@acme.test"]));
        let stranger = EmailAddress::from("anyone@example.test");

        // `@public` lets anyone write in, so the whole team may read what arrives...
        assert!(channel.viewer_access(&stranger, CompanyMembership::Member));
        assert!(channel.viewer_access(&stranger, CompanyMembership::Admin));
        assert!(channel.viewer_access(&stranger, CompanyMembership::Owner));
        // ...but it never makes the traffic readable to the public that sent it.
        assert!(!channel.viewer_access(&stranger, CompanyMembership::None));
    }

    #[test]
    fn a_restricted_channel_still_takes_its_owner_s_mail() {
        let channel = channel_for(Some(&["dana@acme.test", "ops@partner.test"]));

        // The listed participant writes in, as they always could.
        let listed = channel.participant_access("dana@acme.test", CompanyMembership::Member);
        assert!(listed.authorized);
        assert!(listed.trusted);

        // The owner is not on the list, and still gets in -- the same exemption `viewer_access`
        // makes, so the mailbox cannot show them a channel they may not answer in.
        let owner = channel.participant_access("sam@acme.test", CompanyMembership::Owner);
        assert!(owner.authorized);
        assert!(owner.trusted);
        assert!(channel.viewer_access(
            &EmailAddress::from("sam@acme.test"),
            CompanyMembership::Owner
        ));

        // An unlisted colleague is not the owner: a restricted channel is still narrower than the
        // team, on the way in as on the way out.
        let colleague = channel.participant_access("sam@acme.test", CompanyMembership::Member);
        assert!(!colleague.authorized);
        assert!(!channel.viewer_access(
            &EmailAddress::from("sam@acme.test"),
            CompanyMembership::Member
        ));

        // A stranger stays a stranger, whatever address they claim.
        assert!(
            !channel
                .participant_access("anyone@example.test", CompanyMembership::None)
                .authorized
        );
    }

    #[test]
    fn an_unrestricted_channel_takes_the_team_s_mail_and_no_one_else_s() {
        for channel in [channel_for(None), channel_for(Some(&[]))] {
            for membership in [
                CompanyMembership::Owner,
                CompanyMembership::Admin,
                CompanyMembership::Member,
            ] {
                let access = channel.participant_access("dana@acme.test", membership);
                assert!(access.authorized);
                assert!(access.trusted);
            }
            let stranger =
                channel.participant_access("anyone@example.test", CompanyMembership::None);
            assert!(!stranger.authorized);
            assert!(!stranger.trusted);
        }
    }

    #[test]
    fn public_lets_a_stranger_write_without_trusting_them() {
        let channel = channel_for(Some(&[PUBLIC_PARTICIPANT, "dana@acme.test"]));

        // Anyone may write in...
        let stranger = channel.participant_access("anyone@example.test", CompanyMembership::None);
        assert!(stranger.authorized);
        // ...but `@public` alone never waives spam scoring or opens the thread to third parties.
        assert!(!stranger.trusted);

        // Being on the team is what earns that, listed or not.
        assert!(
            channel
                .participant_access("sam@acme.test", CompanyMembership::Member)
                .trusted
        );
    }

    #[test]
    fn a_participant_is_matched_the_way_a_mailbox_is() {
        let channel = channel_for(Some(&["  Dana@Acme.test  "]));

        assert!(channel.viewer_access(
            &EmailAddress::from("dana@acme.test"),
            CompanyMembership::Member
        ));
    }

    fn channel_with_aliases(aliases: &[&str]) -> Channel {
        Channel {
            owner_agent_id: None,
            id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            name: "Support".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: aliases.iter().map(|a| ChannelSlug::from(*a)).collect(),
            participant_emails: None,
            agent_ids: None,
            enabled: true,
            add_3rd_party: true,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn matches_slug_accepts_canonical_and_alias_case_insensitively() {
        let channel = channel_with_aliases(&["sales", "help"]);

        assert!(channel.matches_slug("support"));
        assert!(channel.matches_slug("SALES"));
        assert!(channel.matches_slug("Help"));
        assert!(!channel.matches_slug("billing"));
    }

    #[test]
    fn addresses_list_the_canonical_slug_first() {
        let channel = channel_with_aliases(&["sales"]);
        let addresses = channel.inbound_addresses(&CompanySlug::from("acme"), "mailagents.com");

        assert_eq!(
            addresses,
            vec![
                EmailAddress::from("support@acme.mailagents.com"),
                EmailAddress::from("sales@acme.mailagents.com"),
            ]
        );
    }

    #[test]
    fn a_channel_without_aliases_answers_on_one_address_only() {
        let channel = channel_with_aliases(&[]);

        assert!(channel.matches_slug("support"));
        assert!(!channel.matches_slug("sales"));
        assert_eq!(
            channel.inbound_addresses(&CompanySlug::from("acme"), "mailagents.com"),
            vec![channel.inbound_address(&CompanySlug::from("acme"), "mailagents.com")]
        );
    }
}
