//! Admission policy shared by memory and knowledge. Persistence supplies topology;
//! this module alone decides which of those facts confer read and write authority.
use std::collections::BTreeMap;

use crate::app_error::{AppError, AppResult};
use crate::entities::{
    department::{DepartmentId, MAX_DEPARTMENT_DEPTH},
    transport::PrincipalId,
};

pub const MAX_RUN_READ_UNITS: usize = 256;
pub const RUN_SCOPE_LIMIT_EXCEEDED: &str = "memory.scope_limit_exceeded";
pub const MAX_RUN_READ_NAMESPACES: usize = MAX_RUN_READ_UNITS + 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeUnit {
    pub id: DepartmentId,
    pub parent: Option<DepartmentId>,
    pub slug: String,
}

#[derive(Debug, Clone)]
pub struct LeadChildren {
    pub principal: PrincipalId,
    pub department: DepartmentId,
    pub children: Vec<ScopeUnit>,
}

#[derive(Debug, Clone)]
pub struct ScopeRoleFacts {
    pub root: ScopeUnit,
    pub leads: Vec<LeadChildren>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadUnit {
    pub unit: ScopeUnit,
    pub depth: Option<usize>,
    pub own: bool,
    pub ancestor: bool,
    pub report: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunScopes {
    pub units: Vec<ReadUnit>,
    pub personal: PrincipalId,
    pub write_unit: Option<DepartmentId>,
}

pub fn unavailable() -> AppError {
    AppError::BadRequest("memory.scope_unavailable".into())
}

fn valid_slug(slug: &str) -> bool {
    let alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    !slug.is_empty()
        && slug.len() <= 63
        && slug.bytes().all(|byte| alphanumeric(byte) || byte == b'-')
        && slug.bytes().next().is_some_and(alphanumeric)
        && slug.bytes().last().is_some_and(alphanumeric)
}

fn valid_unit(unit: &ScopeUnit) -> bool {
    valid_slug(&unit.slug)
}

/// `chain` is local first, root last. Empty means an absent or ambiguous local
/// placement, which confers root read only and never lead-report expansion.
pub fn run_scopes(
    chain: &[ScopeUnit],
    agent: PrincipalId,
    roles_held: &ScopeRoleFacts,
) -> AppResult<RunScopes> {
    if roles_held.root.parent.is_some()
        || !valid_unit(&roles_held.root)
        || chain.len() > MAX_DEPARTMENT_DEPTH
        || (!chain.is_empty() && chain.last() != Some(&roles_held.root))
    {
        return Err(unavailable());
    }
    let mut units = BTreeMap::new();
    for (depth, unit) in chain.iter().enumerate() {
        if !valid_unit(unit)
            || units.contains_key(&unit.id)
            || unit.parent != chain.get(depth + 1).map(|parent| parent.id)
        {
            return Err(unavailable());
        }
        units.insert(
            unit.id,
            ReadUnit {
                unit: unit.clone(),
                depth: Some(depth),
                own: depth == 0,
                ancestor: depth > 0,
                report: false,
            },
        );
    }
    if chain.is_empty() {
        units.insert(
            roles_held.root.id,
            ReadUnit {
                unit: roles_held.root.clone(),
                depth: None,
                own: false,
                ancestor: true,
                report: false,
            },
        );
    } else {
        add_reports(&mut units, agent, &roles_held.leads)?;
    }
    Ok(RunScopes {
        units: units.into_values().collect(),
        personal: agent,
        write_unit: chain.first().map(|unit| unit.id),
    })
}

fn add_reports(
    units: &mut BTreeMap<DepartmentId, ReadUnit>,
    agent: PrincipalId,
    leads: &[LeadChildren],
) -> AppResult<()> {
    for lead in leads.iter().filter(|lead| lead.principal == agent) {
        for child in &lead.children {
            if child.parent != Some(lead.department) || !valid_unit(child) {
                return Err(unavailable());
            }
            if let Some(existing) = units.get_mut(&child.id) {
                if existing.unit != *child {
                    return Err(unavailable());
                }
                existing.report = true;
            } else {
                if units.len() >= MAX_RUN_READ_UNITS {
                    return Err(AppError::BadRequest(RUN_SCOPE_LIMIT_EXCEEDED.into()));
                }
                units.insert(
                    child.id,
                    ReadUnit {
                        unit: child.clone(),
                        depth: None,
                        own: false,
                        ancestor: false,
                        report: true,
                    },
                );
            }
        }
    }
    Ok(())
}

/// Small host-authored topology context, frozen before history/recall budgeting.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunScopeContext {
    pub write_unit: Option<String>,
    pub readable_units: Vec<String>,
}

impl RunScopeContext {
    pub fn validate(&self) -> AppResult<()> {
        if self.readable_units.is_empty()
            || self.readable_units.len() > MAX_RUN_READ_UNITS
            || self.readable_units.iter().any(|slug| !valid_slug(slug))
            || self.write_unit.as_deref().is_some_and(|slug| {
                !valid_slug(slug) || !self.readable_units.iter().any(|read| read == slug)
            })
        {
            return Err(unavailable());
        }
        Ok(())
    }
    pub(super) fn render(&self, text: &mut String) {
        text.push_str("\n\nMemory write scope: ");
        if let Some(unit) = &self.write_unit {
            text.push_str("unit:");
            text.push_str(unit);
        } else {
            text.push_str("personal");
        }
        text.push_str("; searchable units for this run: ");
        for (index, unit) in self.readable_units.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            text.push_str("unit:");
            text.push_str(unit);
        }
        text.push('.');
    }
}

#[cfg(test)]
#[path = "scopes_tests.rs"]
mod tests;
