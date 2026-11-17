use super::*;
use crate::application::ports::department::DepartmentRoleAssignment;
use crate::entities::department::RoleCapacity;

#[tokio::test]
async fn workflow_role_sponsor_admission_agent_decider_captures_its_revision_and_informed_person() {
    let f = Box::pin(RoleFixture::with_role(DepartmentRole::Sponsor)).await;
    let owner = f.admission.binding.target.actor.user_id();
    let person = PrincipalId::new(f.admission.binding.fixture.principal(owner).await);
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    for (principal, decider) in [(f.principal, true), (person, false)] {
        departments
            .assign_role(
                owner,
                f.company().as_uuid(),
                f.department,
                DepartmentRoleAssignment {
                    role: DepartmentRole::Sponsor,
                    principal,
                    decider,
                },
            )
            .await
            .unwrap();
    }
    let run = f.admitted("agent-sponsor-with-information").await;
    let manifest = f.manifest(run).await;
    let role = &manifest.roles[0];
    assert_eq!(role.role, DepartmentRole::Sponsor);
    assert_eq!(role.principals.len(), 2);
    let deciding = role
        .principals
        .iter()
        .find(|holder| holder.capacity == RoleCapacity::Decides)
        .unwrap();
    assert_eq!(deciding.principal_id, f.principal);
    assert_eq!(
        deciding.agent_id,
        Some(crate::entities::agent_capability::AgentId::new(f.agent))
    );
    assert_eq!(deciding.revision_id, Some(f.revision));
    let informed = role
        .principals
        .iter()
        .find(|holder| holder.capacity == RoleCapacity::Informed)
        .unwrap();
    assert_eq!(informed.principal_id, person);
    assert_eq!((informed.agent_id, informed.revision_id), (None, None));
    assert_eq!(manifest.agents.len(), 1);
}
