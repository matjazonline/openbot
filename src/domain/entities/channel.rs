use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    creation::CreationProvenance,
    participant::{ChannelPrincipalGrant, PrincipalAccessContext, PrincipalCapability},
    transport::PrincipalId,
    value_objects::{ChannelSlug, CompanySlug, EmailAddress},
};
use std::str::FromStr;

/// Words a channel or company slug may neither be nor end with.
///
/// Transports use them as modifiers on an address that otherwise names a channel -- mail reads
/// `support+quiet@…` as "file it on the thread, do not run the agent" -- so a channel actually
/// *named* `quiet` could be addressed but never replied to. The list is a naming rule about
/// slugs, which is why it is stated here rather than inside whichever adapter happens to spell
/// the modifier first; the mail adapter's address grammar reads it, and so does slug validation.
pub const RESERVED_SLUG_SUFFIXES: &[&str] = &["noagent", "quiet", "message", "msg", "na"];

/// The separators a transport may put between a slug and one of [`RESERVED_SLUG_SUFFIXES`].
pub const RESERVED_SUFFIX_SEPARATORS: &[char] = &['.', '+', '-', '_'];

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
    /// Email-only form/delivery projection of allowlist grants. Authorization never consults this
    /// field; the stable grant rows below are the source of truth.
    pub participant_emails: Option<Vec<EmailAddress>>,
    pub access_mode: ChannelAccessMode,
    pub principal_grants: Vec<ChannelPrincipalGrant>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelAccessMode {
    Team,
    Allowlist,
    Public,
}

impl ChannelAccessMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Allowlist => "allowlist",
            Self::Public => "public",
        }
    }
}

impl FromStr for ChannelAccessMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "team" => Ok(Self::Team),
            "allowlist" => Ok(Self::Allowlist),
            "public" => Ok(Self::Public),
            _ => Err(format!("invalid channel access mode '{value}'")),
        }
    }
}

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
    pub fn participant_access(&self, context: PrincipalAccessContext) -> ParticipantAccess {
        let explicitly_listed = context.principal_id.is_some_and(|principal_id| {
            self.has_capability(principal_id, PrincipalCapability::Participate)
        });
        match self.access_mode {
            ChannelAccessMode::Public => ParticipantAccess {
                authorized: true,
                trusted: explicitly_listed || context.membership.is_team(),
            },
            ChannelAccessMode::Allowlist => ParticipantAccess {
                authorized: explicitly_listed || context.membership.is_owner(),
                trusted: explicitly_listed || context.membership.is_team(),
            },
            ChannelAccessMode::Team => ParticipantAccess {
                authorized: context.membership.is_team(),
                trusted: context.membership.is_team(),
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
    pub fn viewer_access(&self, context: PrincipalAccessContext) -> bool {
        if !context.membership.is_team() {
            return false;
        }

        if context.membership.is_owner() {
            return true;
        }

        match self.access_mode {
            ChannelAccessMode::Team | ChannelAccessMode::Public => true,
            ChannelAccessMode::Allowlist => context.principal_id.is_some_and(|principal_id| {
                self.has_capability(principal_id, PrincipalCapability::View)
            }),
        }
    }

    /// The stable principal to ask for approval. Resolving a delivery identity is an application
    /// projection and deliberately does not happen in channel policy.
    pub fn preferred_approver(&self) -> Option<PrincipalId> {
        self.principal_grants
            .iter()
            .filter(|grant| grant.capability == PrincipalCapability::View)
            .min_by_key(|grant| (grant.created_at, grant.principal_id))
            .map(|grant| grant.principal_id)
    }

    fn has_capability(&self, principal_id: PrincipalId, capability: PrincipalCapability) -> bool {
        self.principal_grants
            .iter()
            .any(|grant| grant.principal_id == principal_id && grant.capability == capability)
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
    use crate::entities::{
        company_member::CompanyMembership,
        participant::{GrantProvenance, PrincipalAccessContext},
    };

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
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
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

    /// A channel whose access policy is the only thing under test. The allowlist form grants both
    /// capabilities at once, which is what the persistence writer does for an entered address.
    fn channel_for(mode: ChannelAccessMode, granted: &[PrincipalId]) -> Channel {
        Channel {
            access_mode: mode,
            principal_grants: granted
                .iter()
                .flat_map(|principal_id| {
                    [PrincipalCapability::Participate, PrincipalCapability::View].map(
                        |capability| ChannelPrincipalGrant {
                            principal_id: *principal_id,
                            capability,
                            provenance: GrantProvenance::EmailAllowlist,
                            created_at: chrono::Utc::now(),
                        },
                    )
                })
                .collect(),
            ..channel_with_aliases(&[])
        }
    }

    fn actor(principal_id: PrincipalId, membership: CompanyMembership) -> PrincipalAccessContext {
        PrincipalAccessContext {
            principal_id: Some(principal_id),
            membership,
        }
    }

    #[test]
    fn a_team_channel_is_readable_by_the_team_and_nobody_else() {
        let channel = channel_for(ChannelAccessMode::Team, &[]);
        let dana = PrincipalId::random();

        assert!(channel.viewer_access(actor(dana, CompanyMembership::Owner)));
        assert!(channel.viewer_access(actor(dana, CompanyMembership::Admin)));
        assert!(channel.viewer_access(actor(dana, CompanyMembership::Member)));
        assert!(!channel.viewer_access(actor(dana, CompanyMembership::None)));
    }

    #[test]
    fn an_allowlist_channel_is_narrower_than_the_team() {
        let dana = PrincipalId::random();
        let partner = PrincipalId::random();
        let sam = PrincipalId::random();
        let channel = channel_for(ChannelAccessMode::Allowlist, &[dana, partner]);

        // A granted colleague sees it...
        assert!(channel.viewer_access(actor(dana, CompanyMembership::Member)));
        // ...and an ungranted one does not, even though they are on the team.
        assert!(!channel.viewer_access(actor(sam, CompanyMembership::Member)));
        assert!(!channel.viewer_access(actor(sam, CompanyMembership::Admin)));
        // The owner is the one exception, so a company cannot lock itself out of its own data.
        assert!(channel.viewer_access(actor(sam, CompanyMembership::Owner)));
        // Being granted is not a way into a company you do not belong to.
        assert!(!channel.viewer_access(actor(partner, CompanyMembership::None)));
    }

    #[test]
    fn public_opens_a_channel_to_mail_not_to_readers() {
        let channel = channel_for(ChannelAccessMode::Public, &[]);
        let stranger = PrincipalId::random();

        // `@public` lets anyone write in, so the whole team may read what arrives...
        assert!(channel.viewer_access(actor(stranger, CompanyMembership::Member)));
        assert!(channel.viewer_access(actor(stranger, CompanyMembership::Admin)));
        assert!(channel.viewer_access(actor(stranger, CompanyMembership::Owner)));
        // ...but it never makes the traffic readable to the public that sent it.
        assert!(!channel.viewer_access(actor(stranger, CompanyMembership::None)));
    }

    #[test]
    fn an_allowlist_channel_still_takes_its_owner_s_mail() {
        let dana = PrincipalId::random();
        let sam = PrincipalId::random();
        let channel = channel_for(ChannelAccessMode::Allowlist, &[dana]);

        // The granted participant writes in, as they always could.
        let granted = channel.participant_access(actor(dana, CompanyMembership::Member));
        assert!(granted.authorized);
        assert!(granted.trusted);

        // The owner holds no grant, and still gets in -- the same exemption `viewer_access`
        // makes, so the mailbox cannot show them a channel they may not answer in.
        let owner = channel.participant_access(actor(sam, CompanyMembership::Owner));
        assert!(owner.authorized);
        assert!(owner.trusted);
        assert!(channel.viewer_access(actor(sam, CompanyMembership::Owner)));

        // An ungranted colleague is not the owner: an allowlist channel is still narrower than
        // the team, on the way in as on the way out.
        let colleague = channel.participant_access(actor(sam, CompanyMembership::Member));
        assert!(!colleague.authorized);
        assert!(!channel.viewer_access(actor(sam, CompanyMembership::Member)));

        // A stranger stays a stranger, whatever handle they arrive on.
        let stranger = PrincipalId::random();
        assert!(
            !channel
                .participant_access(actor(stranger, CompanyMembership::None))
                .authorized
        );
    }

    #[test]
    fn a_team_channel_takes_the_team_s_mail_and_no_one_else_s() {
        let channel = channel_for(ChannelAccessMode::Team, &[]);
        let dana = PrincipalId::random();

        for membership in [
            CompanyMembership::Owner,
            CompanyMembership::Admin,
            CompanyMembership::Member,
        ] {
            let access = channel.participant_access(actor(dana, membership));
            assert!(access.authorized);
            assert!(access.trusted);
        }
        let stranger =
            channel.participant_access(actor(PrincipalId::random(), CompanyMembership::None));
        assert!(!stranger.authorized);
        assert!(!stranger.trusted);
    }

    #[test]
    fn public_lets_a_stranger_write_without_trusting_them() {
        let dana = PrincipalId::random();
        let channel = channel_for(ChannelAccessMode::Public, &[dana]);

        // Anyone may write in...
        let stranger =
            channel.participant_access(actor(PrincipalId::random(), CompanyMembership::None));
        assert!(stranger.authorized);
        // ...but `@public` alone never waives spam scoring or opens the thread to third parties.
        assert!(!stranger.trusted);

        // Being on the team is what earns that, granted or not.
        assert!(
            channel
                .participant_access(actor(PrincipalId::random(), CompanyMembership::Member))
                .trusted
        );
    }

    /// A sender whose transport handle resolved to no principal at all is not "everyone else's"
    /// principal: an allowlist channel must not let them in on an empty match.
    fn unresolved(membership: CompanyMembership) -> PrincipalAccessContext {
        PrincipalAccessContext {
            principal_id: None,
            membership,
        }
    }

    #[test]
    fn an_unresolved_actor_holds_no_grant() {
        let channel = channel_for(ChannelAccessMode::Allowlist, &[PrincipalId::random()]);

        assert!(
            !channel
                .participant_access(unresolved(CompanyMembership::None))
                .authorized
        );
        assert!(!channel.viewer_access(unresolved(CompanyMembership::Member)));
    }

    #[test]
    fn the_preferred_approver_is_the_first_principal_granted_a_view() {
        let first = PrincipalId::random();
        let second = PrincipalId::random();
        let earlier = chrono::Utc::now() - chrono::Duration::minutes(5);
        let channel = Channel {
            access_mode: ChannelAccessMode::Allowlist,
            principal_grants: vec![
                ChannelPrincipalGrant {
                    principal_id: second,
                    capability: PrincipalCapability::View,
                    provenance: GrantProvenance::EmailAllowlist,
                    created_at: chrono::Utc::now(),
                },
                ChannelPrincipalGrant {
                    principal_id: first,
                    capability: PrincipalCapability::View,
                    provenance: GrantProvenance::EmailAllowlist,
                    created_at: earlier,
                },
            ],
            ..channel_with_aliases(&[])
        };

        assert_eq!(channel.preferred_approver(), Some(first));
        // A public channel grants participation to the world without naming an approver.
        assert_eq!(
            channel_for(ChannelAccessMode::Public, &[]).preferred_approver(),
            None
        );
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
