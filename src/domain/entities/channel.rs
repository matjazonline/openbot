use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    creation::CreationProvenance,
    participant::{
        ChannelPrincipalGrant, PrincipalAccessContext, PrincipalCapability, PrincipalGrantAuthority,
    },
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

/// Removes a leading quiet directive shared by email ingress and internal mail relays.
/// A match is a request to file the message without running an agent.
pub fn strip_quiet_prefix(body: &str) -> Option<&str> {
    let trimmed = body.trim_start();
    for suffix in RESERVED_SLUG_SUFFIXES {
        for prefix in [format!("[[{suffix}]]"), format!("[{suffix}]")] {
            if trimmed
                .get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(&prefix))
            {
                return Some(trimmed[prefix.len()..].trim_start());
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Channel {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    /// What this channel is for, in one line, as its owner wrote it.
    ///
    /// Shown to a teammate whose mail bounced off an address that does not exist, so the list of
    /// channels they *could* have written to explains itself.
    pub description: Option<String>,
    pub slug: ChannelSlug,
    /// Extra local parts this channel also answers on, canonical [`Channel::slug`] excluded.
    pub alias_slugs: Vec<ChannelSlug>,
    /// Email-only form/delivery projection of allowlist grants. Authorization never consults this
    /// field; the stable grant rows below are the source of truth.
    pub participant_emails: Option<Vec<EmailAddress>>,
    pub access_mode: ChannelAccessMode,
    pub principal_grants: Vec<ChannelPrincipalGrant>,
    /// Whether the channel takes traffic at all. A disabled channel keeps its threads but bounces
    /// inbound mail and cannot be an internal delivery target.
    pub enabled: bool,
    /// Whether a trusted sender may pull CC'd outsiders onto this channel's threads.
    ///
    /// Off means the channel is internal: a third party is neither added to
    /// `Thread::participant_emails` nor copied on the agent's reply. Because thread membership is
    /// itself an authorization grant, their later mail to the channel then bounces.
    pub add_3rd_party: bool,
    /// Moved on by every settings edit.
    ///
    /// Routing reads it in the statement that reads the settings it decides on, and the inbound
    /// commit refuses the message if the channel has moved past it, so a message is never stored
    /// under settings an edit replaced while it was being routed.
    pub settings_revision: i64,
    pub created_by: CreationProvenance,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The department this desk belongs to. Ownership never blocks deleting the desk.
    pub department_id: crate::entities::department::DepartmentId,
}

/// Participant list entry that opens a channel to any sender.
pub const PUBLIC_PARTICIPANT: &str = "@public";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelAccessMode {
    Members,
    Public,
}

impl ChannelAccessMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Members => "members",
            Self::Public => "public",
        }
    }
}

impl FromStr for ChannelAccessMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "members" => Ok(Self::Members),
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
    /// Mail admission reads the owning department's materialized grants and explicit grants.
    /// Public intake accepts outsiders without trusting them; the owner always has access.
    pub fn participant_access(&self, context: PrincipalAccessContext) -> ParticipantAccess {
        let listed = self.has_capability(context, PrincipalCapability::Participate);
        ParticipantAccess {
            authorized: self.access_mode == ChannelAccessMode::Public
                || listed
                || context.membership.is_owner(),
            trusted: listed || context.membership.is_team(),
        }
    }

    /// Reading requires current company membership. Public intake never grants outsider reads;
    /// Members desks additionally require a view grant, with the company owner exempted.
    pub fn viewer_access(&self, context: PrincipalAccessContext) -> bool {
        context.membership.is_team()
            && (context.membership.is_owner()
                || self.access_mode == ChannelAccessMode::Public
                || self.has_capability(context, PrincipalCapability::View))
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

    fn has_capability(
        &self,
        context: PrincipalAccessContext,
        capability: PrincipalCapability,
    ) -> bool {
        let Some(principal) = context.principal_id else {
            return false;
        };
        self.principal_grants.iter().any(|grant| {
            grant.principal_id == principal
                && grant.capability == capability
                && (context.grant_authority == PrincipalGrantAuthority::Available
                    || grant.provenance
                        == super::participant::GrantProvenance::DepartmentMembership)
        })
    }

    /// Every local part this channel answers on: the canonical slug first, then its aliases.
    pub fn slugs(&self) -> impl Iterator<Item = &ChannelSlug> {
        std::iter::once(&self.slug).chain(self.alias_slugs.iter())
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
        crate::entities::address::address_for(channel_slug, company_slug, app_domain_name)
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
            id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            name: "Support".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: aliases.iter().map(|a| ChannelSlug::from(*a)).collect(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Members,
            principal_grants: Vec::new(),
            enabled: true,
            add_3rd_party: true,
            created_by: CreationProvenance::system(),
            created_at: chrono::Utc::now(),
            settings_revision: 0,
            department_id: crate::entities::department::DepartmentId::random(),
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
                            provenance: GrantProvenance::ConfiguredAllowlist,
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
            access_level: crate::entities::access_level::fixture_level(membership),
            principal_id: Some(principal_id),
            membership,
            grant_authority: PrincipalGrantAuthority::Available,
        }
    }

    #[test]
    fn access_mode_root_members_read_through_their_materialized_grants() {
        let dana = PrincipalId::random();
        let channel = channel_for(ChannelAccessMode::Members, &[dana]);

        assert!(channel.viewer_access(actor(dana, CompanyMembership::Owner)));
        assert!(channel.viewer_access(actor(dana, CompanyMembership::Admin)));
        assert!(channel.viewer_access(actor(dana, CompanyMembership::Member)));
        assert!(!channel.viewer_access(actor(dana, CompanyMembership::None)));
    }

    #[test]
    fn access_mode_child_members_requires_a_view_grant_even_for_admins() {
        let dana = PrincipalId::random();
        let partner = PrincipalId::random();
        let sam = PrincipalId::random();
        let channel = channel_for(ChannelAccessMode::Members, &[dana, partner]);

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
    fn access_mode_public_opens_mail_but_never_outsider_reads() {
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
    fn access_mode_members_preserves_owner_and_named_participant_access() {
        let dana = PrincipalId::random();
        let sam = PrincipalId::random();
        let channel = channel_for(ChannelAccessMode::Members, &[dana]);

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
    fn access_mode_root_members_send_through_their_materialized_grants() {
        let dana = PrincipalId::random();
        let channel = channel_for(ChannelAccessMode::Members, &[dana]);

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
    fn access_mode_public_accepts_outsiders_without_trusting_them() {
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
            access_level: crate::entities::access_level::fixture_level(membership),
            principal_id: None,
            membership,
            grant_authority: PrincipalGrantAuthority::Available,
        }
    }

    #[test]
    fn access_mode_unresolved_actor_holds_no_grant() {
        let channel = channel_for(ChannelAccessMode::Members, &[PrincipalId::random()]);

        assert!(
            !channel
                .participant_access(unresolved(CompanyMembership::None))
                .authorized
        );
        assert!(!channel.viewer_access(unresolved(CompanyMembership::Member)));
    }

    #[test]
    fn access_mode_unverified_account_can_use_only_its_department_grants() {
        let principal = PrincipalId::random();
        let mut unverified = actor(principal, CompanyMembership::Member);
        unverified.grant_authority = PrincipalGrantAuthority::WithheldUnverifiedEmail;
        for provenance in GrantProvenance::ALL.iter().copied() {
            let mut channel = channel_for(ChannelAccessMode::Members, &[principal]);
            for grant in &mut channel.principal_grants {
                grant.provenance = provenance;
            }
            let allowed = provenance == GrantProvenance::DepartmentMembership;
            assert_eq!(
                channel.viewer_access(unverified),
                allowed,
                "view {provenance:?}"
            );
            assert_eq!(
                channel.participant_access(unverified).authorized,
                allowed,
                "participate {provenance:?}"
            );
            assert!(channel.viewer_access(actor(principal, CompanyMembership::Member)));
            let missing = PrincipalAccessContext {
                principal_id: None,
                ..unverified
            };
            assert!(!channel.viewer_access(missing));
            assert!(!channel.participant_access(missing).authorized);
        }
    }

    #[test]
    fn access_mode_parses_only_members_and_public() {
        for mode in [ChannelAccessMode::Members, ChannelAccessMode::Public] {
            assert_eq!(mode.as_str().parse::<ChannelAccessMode>().unwrap(), mode);
            assert_eq!(serde_json::to_value(mode).unwrap(), mode.as_str());
        }
        for invalid in ["team", "allowlist", "Members", "", " members"] {
            assert!(invalid.parse::<ChannelAccessMode>().is_err());
        }
    }

    #[test]
    fn the_preferred_approver_is_the_first_principal_granted_a_view() {
        let first = PrincipalId::random();
        let second = PrincipalId::random();
        let earlier = chrono::Utc::now() - chrono::Duration::minutes(5);
        let channel = Channel {
            access_mode: ChannelAccessMode::Members,
            principal_grants: vec![
                ChannelPrincipalGrant {
                    principal_id: second,
                    capability: PrincipalCapability::View,
                    provenance: GrantProvenance::ConfiguredAllowlist,
                    created_at: chrono::Utc::now(),
                },
                ChannelPrincipalGrant {
                    principal_id: first,
                    capability: PrincipalCapability::View,
                    provenance: GrantProvenance::ConfiguredAllowlist,
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
