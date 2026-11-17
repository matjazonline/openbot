use super::*;
use crate::application::ports::department::{DepartmentRoleAssignment, DepartmentRoleRemoval};

fn assignment(principal: PrincipalId, decider: bool) -> DepartmentRoleAssignment {
    DepartmentRoleAssignment {
        role: DepartmentRole::Sponsor,
        principal,
        decider,
    }
}
fn removal(principal: PrincipalId, successor: Option<PrincipalId>) -> DepartmentRoleRemoval {
    DepartmentRoleRemoval {
        role: DepartmentRole::Sponsor,
        principal,
        successor,
    }
}

#[tokio::test]
async fn department_sponsor_first_moves_and_requires_an_explicit_successor() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Sponsors", None).await;
    for principal in [f.owner_principal, f.member_principal, f.agent_principal] {
        f.use_cases
            .assign_role(f.owner, f.company, unit.id, assignment(principal, false))
            .await
            .unwrap();
    }
    let holders = f.roles(unit.id).await;
    assert_eq!(holders.len(), 3);
    assert_eq!(holders.iter().filter(|holder| holder.decider).count(), 1);
    assert!(
        holders
            .iter()
            .find(|holder| holder.principal_id == f.owner_principal)
            .unwrap()
            .decider
    );
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            unit.id,
            assignment(f.agent_principal, true),
        )
        .await
        .unwrap();
    assert!(
        f.roles(unit.id)
            .await
            .iter()
            .find(|holder| holder.principal_id == f.agent_principal)
            .unwrap()
            .decider
    );
    let before = f.roles(unit.id).await;
    let error = f
        .use_cases
        .clear_role(
            f.owner,
            f.company,
            unit.id,
            removal(f.agent_principal, None),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("department.sponsor_needs_decider")
    );
    assert_eq!(f.roles(unit.id).await, before);
    f.use_cases
        .clear_role(
            f.owner,
            f.company,
            unit.id,
            removal(f.agent_principal, Some(f.member_principal)),
        )
        .await
        .unwrap();
    assert!(
        f.roles(unit.id)
            .await
            .iter()
            .find(|holder| holder.principal_id == f.member_principal)
            .unwrap()
            .decider
    );
    f.use_cases
        .clear_role(
            f.owner,
            f.company,
            unit.id,
            removal(f.owner_principal, None),
        )
        .await
        .unwrap();
    f.use_cases
        .clear_role(
            f.owner,
            f.company,
            unit.id,
            removal(f.member_principal, None),
        )
        .await
        .unwrap();
    assert!(f.roles(unit.id).await.is_empty());
}

#[tokio::test]
async fn department_sponsor_constraints_reject_a_second_decider_and_other_roles() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Sponsor constraints", None).await;
    for principal in [f.owner_principal, f.member_principal] {
        f.use_cases
            .assign_role(f.owner, f.company, unit.id, assignment(principal, false))
            .await
            .unwrap();
    }
    let error=sqlx::query("UPDATE department_roles SET decider=true WHERE company_id=$1 AND department_id=$2 AND principal_id=$3")
        .bind(f.company).bind(unit.id.as_uuid()).bind(f.member_principal.as_uuid()).execute(f.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("department_roles_one_sponsor_decider")
    );
    let error=sqlx::query("INSERT INTO department_roles(company_id,department_id,role,principal_id,decider) VALUES($1,$2,'lead',$3,true)")
        .bind(f.company).bind(unit.id.as_uuid()).bind(f.member_principal.as_uuid()).execute(f.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert!(
        f.use_cases
            .assign_role(
                f.owner,
                f.company,
                unit.id,
                DepartmentRoleAssignment {
                    role: DepartmentRole::Reviewer,
                    principal: f.member_principal,
                    decider: true,
                }
            )
            .await
            .is_err()
    );
    assert!(
        f.use_cases
            .assign_role(
                f.owner,
                f.company,
                unit.id,
                assignment(f.foreign_principal, true)
            )
            .await
            .is_err()
    );
    assert!(
        f.use_cases
            .assign_role(
                f.owner,
                f.company,
                unit.id,
                assignment(f.external_principal, true)
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn department_sponsor_retirement_never_promotes_and_person_repairs_the_gap() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Retiring sponsor", None).await;
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            unit.id,
            assignment(f.member_principal, false),
        )
        .await
        .unwrap();
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            unit.id,
            assignment(f.owner_principal, false),
        )
        .await
        .unwrap();
    CompanyInvitePersistence::remove_member(&f.persistence, f.company, f.member)
        .await
        .unwrap();
    let holders = f.roles(unit.id).await;
    assert_eq!(holders.len(), 1);
    assert!(!holders[0].decider);
    let error = f
        .use_cases
        .assign_role(
            f.owner,
            f.company,
            unit.id,
            assignment(f.owner_principal, false),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("department.sponsor_needs_decider")
    );
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            unit.id,
            assignment(f.owner_principal, true),
        )
        .await
        .unwrap();
    assert!(f.roles(unit.id).await[0].decider);
}

#[tokio::test]
async fn department_sponsor_competing_writers_leave_exactly_one_decider() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Competing sponsors", None).await;
    let barrier = tokio::sync::Barrier::new(2);
    let compete = |principal| {
        let barrier = &barrier;
        let f = &f;
        async move {
            barrier.wait().await;
            f.use_cases
                .assign_role(f.owner, f.company, unit.id, assignment(principal, true))
                .await
        }
    };
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(compete(f.owner_principal), compete(f.member_principal))
    })
    .await
    .unwrap();
    a.unwrap();
    b.unwrap();
    let holders = f.roles(unit.id).await;
    assert_eq!(holders.len(), 2);
    assert_eq!(holders.iter().filter(|holder| holder.decider).count(), 1);
}

#[tokio::test]
async fn department_sponsor_escalation_create_update_and_sql_rust_shape_validation_agree() {
    use crate::entities::workflow_escalation::{EscalationHops, EscalationPolicy};
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let initial = EscalationPolicy {
        notify: DepartmentRole::Sponsor,
        hops: EscalationHops::try_from(2).unwrap(),
    };
    let mut write = f.write("Sponsor policy", None);
    write.escalation = Some(initial);
    let unit = f
        .use_cases
        .create_department(f.owner, f.company, write, None)
        .await
        .unwrap();
    assert_eq!(unit.escalation, Some(initial));
    assert_eq!(
        f.use_cases
            .visible_department(f.owner, f.company, unit.id)
            .await
            .unwrap()
            .department
            .escalation,
        Some(initial)
    );
    let next = EscalationPolicy {
        notify: DepartmentRole::Lead,
        hops: EscalationHops::try_from(1).unwrap(),
    };
    f.use_cases
        .update_department(
            f.owner,
            f.company,
            unit.id,
            DepartmentEdit {
                escalation: Some(Some(next)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.use_cases
            .visible_department(f.owner, f.company, unit.id)
            .await
            .unwrap()
            .department
            .escalation,
        Some(next)
    );
    for role in DepartmentRole::ALL {
        for hops in [0, 1, 2, 3] {
            let value = json!({"notify":role.as_str(),"hops":hops});
            let sql: bool = sqlx::query_scalar("SELECT workflow_escalation_policy_valid($1)")
                .bind(&value)
                .fetch_one(f.pool())
                .await
                .unwrap();
            assert_eq!(
                sql,
                serde_json::from_value::<EscalationPolicy>(value.clone()).is_ok(),
                "{value}"
            );
        }
    }
    let invalid = json!({"notify":"sponsor","hops":3});
    let error = sqlx::query("UPDATE departments SET escalation=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(unit.id.as_uuid())
        .bind(invalid)
        .execute(f.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("departments_escalation_policy")
    );
}
