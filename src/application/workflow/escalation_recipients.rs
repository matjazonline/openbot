//! Notification role selection keeps every nearest holder; delivery eligibility is a later check.
use super::MAX_ESCALATION_RECIPIENTS;
use crate::application::workflow::roles::RoleDepartment;
use crate::entities::department::{DepartmentId, DepartmentRole, MAX_DEPARTMENT_DEPTH};
use crate::entities::participant::PrincipalKind;
use crate::entities::transport::PrincipalId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NotificationRoleHolder {
    pub principal: PrincipalId,
    pub kind: PrincipalKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationRoleRefusal {
    NoHolder,
    MissingContext,
    RecipientLimit,
    InvalidAncestry,
    InvalidIdentity,
}

impl NotificationRoleRefusal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoHolder => "no_holder",
            Self::MissingContext => "missing_context",
            Self::RecipientLimit => "recipient_limit",
            Self::InvalidAncestry => "invalid_ancestry",
            Self::InvalidIdentity => "invalid_identity",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NotificationRecipients {
    Holders {
        department: DepartmentId,
        holders: Vec<NotificationRoleHolder>,
    },
    Refused {
        department: Option<DepartmentId>,
        reason: NotificationRoleRefusal,
    },
}

pub(crate) fn resolve_notification_recipients(
    chain: &[RoleDepartment],
    role: DepartmentRole,
) -> NotificationRecipients {
    let departments: BTreeSet<_> = chain
        .iter()
        .map(|department| department.department)
        .collect();
    if chain.len() > MAX_DEPARTMENT_DEPTH || departments.len() != chain.len() {
        return NotificationRecipients::Refused {
            department: None,
            reason: NotificationRoleRefusal::InvalidAncestry,
        };
    }
    for department in chain {
        let mut selected = BTreeMap::new();
        for holder in department
            .holders
            .iter()
            .filter(|holder| holder.role == role)
        {
            if let Some(prior) = selected.insert(holder.principal, holder.kind)
                && prior != holder.kind
            {
                return NotificationRecipients::Refused {
                    department: Some(department.department),
                    reason: NotificationRoleRefusal::InvalidIdentity,
                };
            }
            if selected.len() > MAX_ESCALATION_RECIPIENTS {
                return NotificationRecipients::Refused {
                    department: Some(department.department),
                    reason: NotificationRoleRefusal::RecipientLimit,
                };
            }
        }
        if !selected.is_empty() {
            return NotificationRecipients::Holders {
                department: department.department,
                holders: selected
                    .into_iter()
                    .map(|(principal, kind)| NotificationRoleHolder { principal, kind })
                    .collect(),
            };
        }
    }
    NotificationRecipients::Refused {
        department: None,
        reason: NotificationRoleRefusal::NoHolder,
    }
}

#[cfg(test)]
#[path = "escalation_recipient_tests.rs"]
mod tests;
