//! Pure nearest-holder resolution; capture facts belong to the admission adapter.
use crate::entities::department::{
    DepartmentId, DepartmentRole, MAX_DEPARTMENT_DEPTH, RoleRefusalKind,
};
use crate::entities::participant::PrincipalKind;
use crate::entities::transport::PrincipalId;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleNeed {
    Agent,
    People,
}

pub struct RoleHolder {
    pub role: DepartmentRole,
    pub principal: PrincipalId,
    pub kind: PrincipalKind,
}

pub struct RoleDepartment {
    pub department: DepartmentId,
    pub holders: Vec<RoleHolder>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedRole {
    pub department: DepartmentId,
    pub principals: Vec<PrincipalId>,
}

/// Scripted samples have no admitted department or immutable holder capture.
pub(crate) fn refuse_unresolved_sample_role(
    bundle: &super::publication::PublishedBundle,
    step: &crate::domain::workflow::StepId,
) -> crate::app_error::AppResult<()> {
    if let Some(reference) = bundle
        .compiled()
        .role_references()
        .iter()
        .find(|reference| &reference.step == step)
    {
        return Err(crate::app_error::AppError::WorkflowRole(
            crate::entities::department::RoleAdmissionRefusal::new(
                step.clone(),
                reference.role,
                RoleRefusalKind::MissingDepartment,
            ),
        ));
    }
    Ok(())
}

pub fn resolve_role(
    chain: &[RoleDepartment],
    role: DepartmentRole,
    need: RoleNeed,
) -> Result<ResolvedRole, RoleRefusalKind> {
    if chain.len() > MAX_DEPARTMENT_DEPTH {
        return Err(RoleRefusalKind::DepthBound);
    }
    let departments: BTreeSet<_> = chain
        .iter()
        .map(|department| department.department)
        .collect();
    if departments.len() != chain.len() {
        return Err(RoleRefusalKind::DepthBound);
    }
    for department in chain {
        let holders: Vec<_> = department
            .holders
            .iter()
            .filter(|holder| holder.role == role)
            .collect();
        if holders.is_empty() {
            continue;
        }
        let expected = match need {
            RoleNeed::Agent => PrincipalKind::Agent,
            RoleNeed::People => PrincipalKind::Person,
        };
        if holders.iter().any(|holder| holder.kind != expected) {
            return Err(RoleRefusalKind::WrongKind);
        }
        let principals: BTreeSet<_> = holders.iter().map(|holder| holder.principal).collect();
        if need == RoleNeed::Agent && principals.len() != 1 {
            return Err(RoleRefusalKind::Ambiguous);
        }
        if principals.len() > crate::domain::workflow::TokenAllowance::MAX_REVIEWERS {
            return Err(RoleRefusalKind::TooMany);
        }
        return Ok(ResolvedRole {
            department: department.department,
            principals: principals.into_iter().collect(),
        });
    }
    Err(RoleRefusalKind::NoHolder)
}

#[cfg(test)]
#[path = "roles_tests.rs"]
mod tests;
