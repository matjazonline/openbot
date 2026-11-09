//! A department groups desks and principals inside one company, nests under a parent department,
//! and assigns roles to principals (01-departments).
//!
//! Membership grants nothing by itself: it is a fact other rules read. A role holder need not be a
//! member — a department's lead is usually not one of its workers.

use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    creation::CreationProvenance,
    transport::{PrincipalId, uuid_id},
    value_objects::DepartmentSlug,
};

uuid_id!(DepartmentId);

/// The deepest a department tree may nest: a root department is level 1. The schema's
/// `department_shape_problem` enforces the same number.
pub const MAX_DEPARTMENT_DEPTH: usize = 8;

/// The most a charter may hold, in bytes, as `departments_charter_size` checks.
pub const MAX_DEPARTMENT_CHARTER_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Department {
    pub id: DepartmentId,
    pub company_id: Uuid,
    pub name: String,
    /// Also the address of the department's own desk, so it follows channel slug rules.
    pub slug: DepartmentSlug,
    pub description: Option<String>,
    /// What the department is for, shown to its members and leads so they share one framing.
    pub charter: Option<String>,
    pub parent_id: Option<DepartmentId>,
    pub created_by: CreationProvenance,
    pub created_at: DateTime<Utc>,
}

/// A role a principal holds in a department. `Lead` has exactly one holder; later plans add
/// roles that may have several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepartmentRole {
    Lead,
}

impl DepartmentRole {
    pub const ALL: [Self; 1] = [Self::Lead];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
        }
    }

    /// Whether a department holds at most one principal in this role. Assigning a single-holder
    /// role replaces its current holder rather than adding a second.
    pub const fn is_single_holder(self) -> bool {
        match self {
            Self::Lead => true,
        }
    }
}

impl fmt::Display for DepartmentRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidDepartmentRole(String);

impl fmt::Display for InvalidDepartmentRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown department role '{}'", self.0)
    }
}

impl std::error::Error for InvalidDepartmentRole {}

impl FromStr for DepartmentRole {
    type Err = InvalidDepartmentRole;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "lead" => Ok(Self::Lead),
            _ => Err(InvalidDepartmentRole(value.to_string())),
        }
    }
}

/// One role and the principal holding it, as a department's role list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepartmentRoleHolder {
    pub role: DepartmentRole,
    pub principal_id: PrincipalId,
}

/// One role a principal holds, as that principal's role list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldDepartmentRole {
    pub department_id: DepartmentId,
    pub role: DepartmentRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepartmentMember {
    pub principal_id: PrincipalId,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_department_role_round_trips_through_its_stored_string() {
        for role in DepartmentRole::ALL {
            assert_eq!(role.as_str().parse::<DepartmentRole>(), Ok(role));
            assert_eq!(
                serde_json::to_value(role).unwrap(),
                serde_json::Value::String(role.as_str().into())
            );
        }
    }

    #[test]
    fn an_unknown_department_role_is_refused_by_name() {
        let error = "manager".parse::<DepartmentRole>().unwrap_err();
        assert_eq!(error.to_string(), "unknown department role 'manager'");
        assert!("Lead".parse::<DepartmentRole>().is_err(), "case matters");
    }

    #[test]
    fn the_lead_has_a_single_holder() {
        assert!(DepartmentRole::Lead.is_single_holder());
    }
}
