//! Administration authority (D3, user decision 2026-11-08). A provisioner manages only the
//! tenants it created; because it can mint client credentials for them, it holds transitive data
//! authority over those tenants. It can never override an operator's safety action.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdminScope {
    Operator,
    Provisioner,
}

impl AdminScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Provisioner => "provisioner",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "operator" => Some(Self::Operator),
            "provisioner" => Some(Self::Provisioner),
            _ => None,
        }
    }
}

/// May `actor` manage a tenant whose `managed_by` is `managed_by`?
pub fn may_manage(actor_id: Uuid, actor_scope: AdminScope, managed_by: Option<Uuid>) -> bool {
    match actor_scope {
        AdminScope::Operator => true,
        AdminScope::Provisioner => managed_by == Some(actor_id),
    }
}

/// Operator-only actions: spend caps and admin credentials.
pub fn operator_only(actor_scope: AdminScope) -> bool {
    actor_scope == AdminScope::Operator
}

/// A provisioner may not enable a tenant an operator disabled.
pub fn may_enable(actor_scope: AdminScope, disabled_by_scope: Option<AdminScope>) -> bool {
    !(actor_scope == AdminScope::Provisioner && disabled_by_scope == Some(AdminScope::Operator))
}

/// Re-disabling an already-disabled tenant is a no-op that keeps the first recorded disabler,
/// so an operator disable stays sticky through any provisioner disable-then-enable sequence.
pub fn disabler_after_disable(
    current: Option<(Uuid, AdminScope)>,
    actor: (Uuid, AdminScope),
) -> (Uuid, AdminScope) {
    current.unwrap_or(actor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_authority_provisioner_bounds_are_pure_policy() {
        let me = Uuid::now_v7();
        let other = Uuid::now_v7();
        assert!(may_manage(me, AdminScope::Provisioner, Some(me)));
        assert!(!may_manage(me, AdminScope::Provisioner, Some(other)));
        assert!(!may_manage(me, AdminScope::Provisioner, None));
        assert!(may_manage(me, AdminScope::Operator, Some(other)));
        assert!(!operator_only(AdminScope::Provisioner));
        assert!(!may_enable(
            AdminScope::Provisioner,
            Some(AdminScope::Operator)
        ));
        assert!(may_enable(
            AdminScope::Provisioner,
            Some(AdminScope::Provisioner)
        ));
        assert!(may_enable(AdminScope::Operator, Some(AdminScope::Operator)));
        let operator = (other, AdminScope::Operator);
        assert_eq!(
            disabler_after_disable(Some(operator), (me, AdminScope::Provisioner)),
            operator
        );
    }
}
