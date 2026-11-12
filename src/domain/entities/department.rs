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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepartmentRole {
    Lead,
    Reviewer,
}

impl DepartmentRole {
    pub const ALL: [Self; 2] = [Self::Lead, Self::Reviewer];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Reviewer => "reviewer",
        }
    }

    /// Whether a department holds at most one principal in this role. Assigning a single-holder
    /// role replaces its current holder rather than adding a second.
    pub const fn is_single_holder(self) -> bool {
        match self {
            Self::Lead => true,
            Self::Reviewer => false,
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
            "reviewer" => Ok(Self::Reviewer),
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

/// A bounded admission refusal. It deliberately carries no holder identities or labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleRefusalKind {
    NoHolder,
    WrongKind,
    Ambiguous,
    DepthBound,
    TooMany,
    Unavailable,
    MissingDepartment,
}

impl RoleRefusalKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::NoHolder => "workflow.role_no_holder",
            Self::WrongKind => "workflow.role_wrong_kind",
            Self::Ambiguous => "workflow.role_ambiguous",
            Self::DepthBound => "workflow.role_depth_bound",
            Self::TooMany => "workflow.role_too_many",
            Self::Unavailable => "workflow.role_unavailable",
            Self::MissingDepartment => "workflow.role_needs_department",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleAdmissionRefusal {
    format: RoleRefusalFormat,
    pub step: crate::domain::workflow::StepId,
    pub role: DepartmentRole,
    pub kind: RoleRefusalKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum RoleRefusalFormat {
    #[serde(rename = "workflow-role-refusal-v1")]
    V1,
}

impl RoleAdmissionRefusal {
    pub fn new(
        step: crate::domain::workflow::StepId,
        role: DepartmentRole,
        kind: RoleRefusalKind,
    ) -> Self {
        Self {
            format: RoleRefusalFormat::V1,
            step,
            role,
            kind,
        }
    }
}

impl fmt::Display for RoleAdmissionRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: step {} requires role {}",
            self.kind.code(),
            self.step,
            self.role
        )
    }
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

/// A department a principal belongs to or holds a role in, with the roles it holds there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HeldDepartment {
    pub id: DepartmentId,
    pub name: String,
    pub slug: String,
    pub roles: Vec<DepartmentRole>,
}

/// The most people and agents the department forms offer as members or leads; a longer list is
/// cut and says so.
pub const MAX_DEPARTMENT_PRINCIPAL_CHOICES: usize = 500;

/// A person or agent of the company as the department pages name it: an agent by its name and
/// handle, a person by their username and email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartmentPrincipal {
    pub principal_id: PrincipalId,
    pub kind: crate::entities::participant::PrincipalKind,
    pub name: String,
    pub detail: String,
}

/// A desk a department owns, at its primary address slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartmentDesk {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    /// The department's own desk, the one its `department:{id}` record names. A slug edit does
    /// not move it, so its address is this slug, not the department's.
    pub own: bool,
}

/// Which department owns each desk, for the surfaces that group by department (01-departments
/// 04B.4, 04C). Names are company-visible: everyone may see which department a desk belongs to;
/// who is in a department stays with the Departments tab.
#[derive(Debug, Clone, Default)]
pub struct DeskDepartments {
    pub departments: Vec<Department>,
    /// Desk id to its owning department.
    pub owners: std::collections::HashMap<Uuid, DepartmentId>,
}

impl DeskDepartments {
    pub fn department(&self, id: DepartmentId) -> Option<&Department> {
        self.departments
            .iter()
            .find(|department| department.id == id)
    }

    /// The department that owns `desk`, if any.
    pub fn of_desk(&self, desk: Option<Uuid>) -> Option<&Department> {
        desk.and_then(|desk| self.owners.get(&desk))
            .and_then(|id| self.department(*id))
    }

    /// `items` under their desk's department, departments in tree order (a root before its
    /// children), then those without one under "No department". When no item has a department
    /// there is one group without a heading, so a company that does not use departments sees its
    /// list as before. Order within a group is the input order.
    pub fn group<'t, T>(
        &self,
        items: &'t [T],
        desk: impl Fn(&T) -> Option<Uuid>,
    ) -> Vec<(Option<String>, Vec<&'t T>)> {
        let mut groups: Vec<(Option<String>, Vec<&'t T>)> = department_tree(&self.departments)
            .into_iter()
            .map(|(_, department)| {
                let owned = items
                    .iter()
                    .filter(|item| {
                        self.of_desk(desk(item)).map(|owner| owner.id) == Some(department.id)
                    })
                    .collect::<Vec<_>>();
                (Some(department.name.clone()), owned)
            })
            .filter(|(_, owned)| !owned.is_empty())
            .collect();
        let rest: Vec<&T> = items
            .iter()
            .filter(|item| self.of_desk(desk(item)).is_none())
            .collect();
        if groups.is_empty() {
            return vec![(None, rest)];
        }
        if !rest.is_empty() {
            groups.push((Some("No department".to_owned()), rest));
        }
        groups
    }
}

/// The departments in tree order with their depth (a root is 0): each department, then its
/// children, siblings by name and then id. A department whose parent is not in `departments`
/// (a member sees only their own) is placed as a root, so nothing is dropped.
pub fn department_tree(departments: &[Department]) -> Vec<(usize, &Department)> {
    let present: std::collections::HashSet<DepartmentId> =
        departments.iter().map(|department| department.id).collect();
    let mut sorted: Vec<&Department> = departments.iter().collect();
    sorted.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.id.as_uuid().cmp(&b.id.as_uuid()))
    });
    let children = |parent: Option<DepartmentId>| -> Vec<&Department> {
        sorted
            .iter()
            .copied()
            .filter(|department| match parent {
                None => department.parent_id.is_none_or(|id| !present.contains(&id)),
                Some(parent) => department.parent_id == Some(parent),
            })
            .collect()
    };
    let mut ordered = Vec::with_capacity(departments.len());
    let mut stack: Vec<(usize, &Department)> = children(None)
        .into_iter()
        .rev()
        .map(|root| (0, root))
        .collect();
    while let Some((depth, department)) = stack.pop() {
        ordered.push((depth, department));
        // The schema keeps the tree acyclic and at most `MAX_DEPARTMENT_DEPTH` deep; the bound
        // only keeps a corrupt row from looping.
        if depth + 1 < MAX_DEPARTMENT_DEPTH * 2 {
            stack.extend(
                children(Some(department.id))
                    .into_iter()
                    .rev()
                    .map(|child| (depth + 1, child)),
            );
        }
    }
    ordered
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
        assert!(!DepartmentRole::Reviewer.is_single_holder());
    }
}
