use super::*;
use crate::application::workflow::roles::{RoleHolder, RoleNeed, resolve_role};
use crate::entities::department::RoleRefusalKind;
use uuid::Uuid;

fn department(kinds: &[PrincipalKind]) -> RoleDepartment {
    RoleDepartment {
        department: DepartmentId::new(Uuid::new_v4()),
        holders: kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| RoleHolder {
                role: DepartmentRole::Reviewer,
                principal: PrincipalId::new(Uuid::from_u128(index as u128 + 1)),
                kind: *kind,
            })
            .collect(),
    }
}

#[test]
fn workflow_escalation_notification_selection_keeps_all_kinds_and_local_ineligible_shadows_ancestor()
 {
    for kinds in [
        vec![PrincipalKind::Person],
        vec![PrincipalKind::Person; 3],
        vec![PrincipalKind::Agent; 3],
        vec![
            PrincipalKind::Agent,
            PrincipalKind::Person,
            PrincipalKind::Agent,
        ],
        vec![PrincipalKind::External, PrincipalKind::System],
    ] {
        let local = department(&kinds);
        let selected = local.department;
        let chain = [department(&[]), local, department(&[PrincipalKind::Person])];
        let NotificationRecipients::Holders {
            department,
            holders,
        } = resolve_notification_recipients(&chain, DepartmentRole::Reviewer)
        else {
            panic!("all nearest holders must be retained for eligibility")
        };
        assert_eq!(department, selected);
        assert_eq!(holders.len(), kinds.len());
        for (index, holder) in holders.iter().enumerate() {
            assert_eq!(
                holder.principal.as_uuid(),
                Uuid::from_u128(index as u128 + 1)
            );
            assert_eq!(holder.kind, kinds[index]);
        }
    }
}

#[test]
fn workflow_escalation_notification_selection_bounds_the_whole_set_without_a_partial_delivery() {
    let permitted = department(&[PrincipalKind::Agent; 16]);
    assert!(
        matches!(resolve_notification_recipients(&[permitted],DepartmentRole::Reviewer),
        NotificationRecipients::Holders { holders,.. } if holders.len()==16)
    );
    let exceeded = department(&[PrincipalKind::Person; 17]);
    let selected = exceeded.department;
    assert_eq!(
        resolve_notification_recipients(
            &[exceeded, department(&[PrincipalKind::Person])],
            DepartmentRole::Reviewer
        ),
        NotificationRecipients::Refused {
            department: Some(selected),
            reason: NotificationRoleRefusal::RecipientLimit
        }
    );
    assert_eq!(
        resolve_notification_recipients(&[department(&[])], DepartmentRole::Reviewer),
        NotificationRecipients::Refused {
            department: None,
            reason: NotificationRoleRefusal::NoHolder
        }
    );
}

#[test]
fn workflow_escalation_notification_selection_does_not_change_agent_or_people_modes() {
    let agents = [department(&[PrincipalKind::Agent; 2])];
    assert_eq!(
        resolve_role(&agents, DepartmentRole::Reviewer, RoleNeed::Agent),
        Err(RoleRefusalKind::Ambiguous)
    );
    assert_eq!(
        resolve_role(&agents, DepartmentRole::Reviewer, RoleNeed::People),
        Err(RoleRefusalKind::WrongKind)
    );
    assert!(
        matches!(resolve_notification_recipients(&agents,DepartmentRole::Reviewer),NotificationRecipients::Holders { holders,.. } if holders.len()==2)
    );
    let mut chain: Vec<_> = (0..MAX_DEPARTMENT_DEPTH).map(|_| department(&[])).collect();
    assert_eq!(
        resolve_notification_recipients(&chain, DepartmentRole::Reviewer),
        NotificationRecipients::Refused {
            department: None,
            reason: NotificationRoleRefusal::NoHolder
        }
    );
    chain.push(department(&[]));
    assert_eq!(
        resolve_notification_recipients(&chain, DepartmentRole::Reviewer),
        NotificationRecipients::Refused {
            department: None,
            reason: NotificationRoleRefusal::InvalidAncestry
        }
    );
    let mut cycle = [department(&[]), department(&[])];
    cycle[1].department = cycle[0].department;
    assert_eq!(
        resolve_notification_recipients(&cycle, DepartmentRole::Reviewer),
        NotificationRecipients::Refused {
            department: None,
            reason: NotificationRoleRefusal::InvalidAncestry
        }
    );
}
