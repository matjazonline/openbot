//! A company's one address namespace: every local part names exactly one desk or one principal.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    transport::PrincipalId,
    value_objects::{ChannelSlug, CompanySlug, EmailAddress},
};

/// What a slug in a company's address namespace names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum AddressTarget {
    Desk(Uuid),
    /// A person's or agent's direct inbox, independent of desks they may staff.
    Principal(PrincipalId),
}

impl AddressTarget {
    /// Construct a target only when exactly one addressable owner is present.
    pub const fn from_parts(desk: Option<Uuid>, principal: Option<PrincipalId>) -> Option<Self> {
        match (desk, principal) {
            (Some(id), None) => Some(Self::Desk(id)),
            (None, Some(id)) => Some(Self::Principal(id)),
            _ => None,
        }
    }

    pub const fn desk_id(self) -> Option<Uuid> {
        match self {
            Self::Desk(id) => Some(id),
            Self::Principal(_) => None,
        }
    }

    pub const fn principal_id(self) -> Option<PrincipalId> {
        match self {
            Self::Principal(id) => Some(id),
            Self::Desk(_) => None,
        }
    }
}

/// One addressed slug and what the namespace answered for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAddress {
    pub slug: ChannelSlug,
    pub target: AddressTarget,
}

/// The platform address `{slug}@{company}.{app domain}`, the one form shared by desks and
/// principals.
pub fn address_for(
    slug: &ChannelSlug,
    company: &CompanySlug,
    app_domain_name: &str,
) -> EmailAddress {
    EmailAddress::new(format!("{slug}@{company}.{app_domain_name}"))
}

/// A person's own address in one company they belong to, as their profile lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonAddress {
    pub company_id: Uuid,
    pub company_name: String,
    pub company_slug: CompanySlug,
    pub slug: ChannelSlug,
}

/// The desk targets among these slugs, for a test double that holds its desks in memory: each
/// slug a desk answers on (canonical or alias) names that desk.
#[cfg(test)]
pub(crate) fn desk_addresses(
    desks: &[crate::entities::channel::Channel],
    slugs: &[ChannelSlug],
) -> Vec<ResolvedAddress> {
    slugs
        .iter()
        .filter_map(|slug| {
            desks
                .iter()
                .find(|desk| desk.matches_slug(slug))
                .map(|desk| ResolvedAddress {
                    slug: slug.clone(),
                    target: AddressTarget::Desk(desk.id),
                })
        })
        .collect()
}

impl std::fmt::Display for AddressTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Desk(id) => write!(f, "desk:{id}"),
            Self::Principal(id) => write!(f, "principal:{id}"),
        }
    }
}

impl std::str::FromStr for AddressTarget {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (kind, id) = value
            .split_once(':')
            .ok_or("A target requires a kind and UUID")?;
        let id = Uuid::parse_str(id).map_err(|_| "A target requires a valid UUID")?;
        match kind {
            "desk" => Ok(Self::Desk(id)),
            "principal" => Ok(Self::Principal(PrincipalId::new(id))),
            _ => Err("Unknown address target kind"),
        }
    }
}
