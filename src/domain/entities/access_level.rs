//! One access-level vocabulary for membership, stored content and release decisions.
use super::company_member::CompanyMembership;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
#[serde(try_from = "i16", into = "i16")]
pub enum AccessLevel {
    L0 = 0,
    L1,
    L2,
    L3,
    L4,
    L5,
    L6,
    L7,
    L8,
    L9,
}
impl AccessLevel {
    pub const PUBLIC: Self = Self::L0;
    pub const MAX: Self = Self::L9;
    pub const ALL: [Self; 10] = [
        Self::L0,
        Self::L1,
        Self::L2,
        Self::L3,
        Self::L4,
        Self::L5,
        Self::L6,
        Self::L7,
        Self::L8,
        Self::L9,
    ];

    pub const fn as_i16(self) -> i16 {
        self as i16
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::L0 => "Level 0",
            Self::L1 => "Level 1",
            Self::L2 => "Level 2",
            Self::L3 => "Level 3",
            Self::L4 => "Level 4",
            Self::L5 => "Level 5",
            Self::L6 => "Level 6",
            Self::L7 => "Level 7",
            Self::L8 => "Level 8",
            Self::L9 => "Level 9",
        }
    }
    pub fn lowest(readers: impl IntoIterator<Item = Self>) -> Self {
        readers.into_iter().min().unwrap_or(Self::PUBLIC)
    }
}
impl TryFrom<i16> for AccessLevel {
    type Error = &'static str;
    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::L0),
            1 => Ok(Self::L1),
            2 => Ok(Self::L2),
            3 => Ok(Self::L3),
            4 => Ok(Self::L4),
            5 => Ok(Self::L5),
            6 => Ok(Self::L6),
            7 => Ok(Self::L7),
            8 => Ok(Self::L8),
            9 => Ok(Self::L9),
            _ => Err("Access level must be an integer from 0 to 9"),
        }
    }
}
impl From<AccessLevel> for i16 {
    fn from(value: AccessLevel) -> Self {
        value.as_i16()
    }
}

/// Current membership and level, always resolved together rather than from a client claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelActor {
    pub membership: CompanyMembership,
    pub access_level: AccessLevel,
}
impl LevelActor {
    pub fn manages_levels(self) -> bool {
        self.membership.manages_company_operations()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelTarget {
    Owner,
    Assignable(AccessLevel),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelRefusal {
    NotAdministrator,
    OwnerFixed,
    TargetAboveActor,
    RequestedAboveActor,
}
impl std::fmt::Display for LevelRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotAdministrator => "Only an owner or admin may assign access levels.",
            Self::OwnerFixed => "The company owner's access level is fixed at 9.",
            Self::TargetAboveActor => {
                "You cannot change a teammate or invitation above your access level."
            }
            Self::RequestedAboveActor => "You cannot assign an access level above your own.",
        })
    }
}
pub fn may_assign_level(
    actor: LevelActor,
    target_current: LevelTarget,
    requested: AccessLevel,
) -> Result<(), LevelRefusal> {
    if !actor.manages_levels() {
        return Err(LevelRefusal::NotAdministrator);
    }
    let LevelTarget::Assignable(current) = target_current else {
        return Err(LevelRefusal::OwnerFixed);
    };
    if actor.membership.is_owner() {
        return Ok(());
    }
    if current > actor.access_level {
        return Err(LevelRefusal::TargetAboveActor);
    }
    if requested > actor.access_level {
        return Err(LevelRefusal::RequestedAboveActor);
    }
    Ok(())
}

#[cfg(test)]
#[path = "access_level_tests.rs"]
mod tests;

/// Existing tests create fresh actors at the ordinary joining level, with the fixed owner rule.
#[cfg(test)]
pub(crate) fn fixture_level(membership: CompanyMembership) -> AccessLevel {
    if membership.is_owner() {
        AccessLevel::MAX
    } else {
        AccessLevel::PUBLIC
    }
}

/// The least cleared reader determines the clearance of a shared audience.
pub fn audience_level(readers: impl IntoIterator<Item = AccessLevel>) -> AccessLevel {
    AccessLevel::lowest(readers)
}

/// A release never lowers the classification of the content it carries.
pub const fn may_release(content: AccessLevel, destination: AccessLevel) -> bool {
    content.as_i16() <= destination.as_i16()
}

/// Knowledge classification never grants channel or company access on its own.
pub const fn visible_to(viewer: AccessLevel, stamp: AccessLevel) -> bool {
    viewer.as_i16() >= stamp.as_i16()
}

/// Immutable admission ceiling and the monotone classification of data already consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RunKnowledge {
    knowledge_ceiling: AccessLevel,
    content_level: AccessLevel,
}
impl RunKnowledge {
    pub fn new(
        knowledge_ceiling: AccessLevel,
        content_level: AccessLevel,
    ) -> Result<Self, &'static str> {
        if content_level > knowledge_ceiling {
            return Err("Run content exceeds its knowledge ceiling");
        }
        Ok(Self {
            knowledge_ceiling,
            content_level,
        })
    }
    pub const fn ceiling(self) -> AccessLevel {
        self.knowledge_ceiling
    }
    pub const fn content(self) -> AccessLevel {
        self.content_level
    }
}

#[cfg(test)]
#[path = "knowledge_level_tests.rs"]
mod knowledge_tests;
