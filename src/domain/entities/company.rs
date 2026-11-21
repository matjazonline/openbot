use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    company_member::CompanyMembership,
    value_objects::{AvatarUrl, CompanySlug, ModelName, ModelProvider},
};

/// Defaults copied into each new assistant channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompanyChannelDefaults {
    pub add_3rd_party: bool,
}

impl Default for CompanyChannelDefaults {
    fn default() -> Self {
        Self {
            add_3rd_party: true,
        }
    }
}

/// Non-secret company model configuration suitable for settings and agent-selection views.
/// Credentials are deliberately loaded through the narrow persistence method only at execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompanyModelConnection {
    pub id: Uuid,
    /// The unit that owns it (Plan07 02A); the root by default.
    pub unit_id: Uuid,
    /// That unit's name, for labelling choices.
    pub unit_name: String,
    /// Whether that unit is the root: the root's default is the company default.
    pub in_root: bool,
    pub provider: ModelProvider,
    pub models: Vec<ModelName>,
    /// The default of its own unit; the root's default is the company default.
    pub is_default: bool,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Company {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub slug: CompanySlug,
    #[serde(default)]
    pub channel_defaults: CompanyChannelDefaults,
    /// The company's picture; `None` falls back to the letter bubble.
    pub avatar_url: Option<AvatarUrl>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A company a signed-in account can reach, and what they are to it.
///
/// The two travel together because every read guard needs both: the company scopes the query, and
/// the membership decides which of its channels the caller may actually see.
#[derive(Debug, Clone, PartialEq)]
pub struct CompanyAccess {
    pub company: Company,
    pub membership: CompanyMembership,
    pub access_level: crate::entities::access_level::AccessLevel,
    /// Whether the person manages at least one unit (`any_unit_management_role`). A rendering
    /// hint for navigation only; every management action checks its own target.
    pub administers_units: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_defaults_match_database_defaults() {
        let defaults = CompanyChannelDefaults::default();
        assert!(defaults.add_3rd_party);
    }
}
