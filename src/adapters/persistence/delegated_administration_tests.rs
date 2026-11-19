use super::*;
use crate::application::ports::administration::{
    AdministrationPersistence, ManagementRole, ManagementTarget, manages_unit,
};

struct Delegated {
    f: Departments,
    unit: Department,
    child: Department,
    sibling: Department,
    sibling_lead: Uuid,
}

impl Delegated {
    async fn on(pool: PgPool) -> Self {
        let f = Departments::on(pool).await;
        let unit = f.create("Sales", None).await;
        let child = f.create("Field", Some(unit.id)).await;
        let sibling = f.create("Support", None).await;
        let (sibling_lead, sibling_person) = teammate(f.pool(), f.company, "member").await;
        for (department, person) in [(unit.id, f.member_principal), (sibling.id, sibling_person)] {
            f.use_cases
                .assign_role(f.owner, f.company, department, DepartmentRole::Lead, person)
                .await
                .unwrap();
        }
        Self {
            f,
            unit,
            child,
            sibling,
            sibling_lead,
        }
    }

    fn company(&self) -> CompanyId {
        CompanyId::new(self.f.company)
    }

    async fn remote(&self) -> Department {
        self.f
            .use_cases
            .create_department(
                self.f.member,
                self.f.company,
                self.f.write("Remote", Some(self.child.id)),
                None,
            )
            .await
            .unwrap()
    }

    fn scope(&self, actor: Uuid) -> DepartmentScope {
        DepartmentScope {
            company: self.company(),
            actor: WorkflowActor::authenticated(actor).unwrap(),
        }
    }
}

fn hidden<T: std::fmt::Debug>(result: AppResult<T>) {
    assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
}

#[tokio::test]
async fn manages_unit_current_person_hierarchy_and_tenant_matrix() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let f = &d.f;
    let root = f.root_id().await;
    let foreign: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(f.foreign_company)
            .fetch_one(f.pool())
            .await
            .unwrap();
    for (actor, target, expected) in [
        (f.owner, root, Some(ManagementRole::Owner)),
        (f.owner, d.child.id, Some(ManagementRole::Owner)),
        (f.admin, d.child.id, Some(ManagementRole::Admin)),
        (f.member, d.unit.id, Some(ManagementRole::Lead(d.unit.id))),
        (f.member, d.child.id, Some(ManagementRole::Lead(d.unit.id))),
        (f.member, d.sibling.id, None),
        (f.member, root, None),
        (d.sibling_lead, d.child.id, None),
        (f.stranger, d.child.id, None),
        (f.owner, DepartmentId::random(), None),
        (f.owner, DepartmentId::new(foreign), None),
    ] {
        assert_eq!(
            f.persistence
                .management_role(actor, d.company(), ManagementTarget::Unit(target))
                .await
                .unwrap(),
            expected,
            "actor {actor}, unit {target}"
        );
        assert_eq!(
            manages_unit(&f.departments, actor, d.company(), target)
                .await
                .unwrap(),
            expected.is_some()
        );
    }
    assert_non_people_and_child_lead(&d).await;
}

async fn assert_non_people_and_child_lead(d: &Delegated) {
    let f = &d.f;
    let agent: Uuid = sqlx::query_scalar("SELECT agent_id FROM principals WHERE id=$1")
        .bind(f.agent_principal.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap();
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            d.child.id,
            DepartmentRole::Lead,
            f.agent_principal,
        )
        .await
        .unwrap();
    for actor in [
        agent,
        f.agent_principal.as_uuid(),
        f.external_principal.as_uuid(),
        f.system_principal.as_uuid(),
    ] {
        assert!(
            !manages_unit(&f.persistence, actor, d.company(), d.child.id)
                .await
                .unwrap()
        );
    }
    let (child_lead, child_person) = teammate(f.pool(), f.company, "member").await;
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            d.child.id,
            DepartmentRole::Lead,
            child_person,
        )
        .await
        .unwrap();
    assert!(
        !manages_unit(&f.persistence, child_lead, d.company(), d.unit.id)
            .await
            .unwrap()
    );
    assert!(
        manages_unit(&f.persistence, child_lead, d.company(), d.child.id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn manages_unit_reaches_the_eighth_level_and_rejects_a_ninth() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let mut parent = d.child.id.as_uuid(); // root1, Sales2, Field3
    for depth in 4..=8 {
        parent =
            d.f.insert_raw(d.f.company, &format!("depth-{depth}"), Some(parent))
                .await
                .unwrap();
    }
    assert!(
        manages_unit(
            &d.f.persistence,
            d.f.member,
            d.company(),
            DepartmentId::new(parent)
        )
        .await
        .unwrap()
    );
    let rejected =
        d.f.insert_raw(d.f.company, "depth-9", Some(parent))
            .await
            .unwrap_err();
    assert_eq!(refused_by::<()>(Err(rejected)), "departments_shape");
}

#[tokio::test]
async fn manages_unit_agent_placements_use_any_managed_unit_and_no_unit_uses_root() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let agent: Uuid = sqlx::query_scalar("SELECT agent_id FROM principals WHERE id=$1")
        .bind(d.f.agent_principal.as_uuid())
        .fetch_one(d.f.pool())
        .await
        .unwrap();
    let target = ManagementTarget::Agent(crate::entities::agent_capability::AgentId::new(agent));
    assert_eq!(
        d.f.persistence
            .management_role(d.f.member, d.company(), target)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        d.f.persistence
            .management_role(d.f.admin, d.company(), target)
            .await
            .unwrap(),
        Some(ManagementRole::Admin)
    );
    d.f.use_cases
        .add_member(d.f.owner, d.f.company, d.child.id, d.f.agent_principal)
        .await
        .unwrap();
    d.f.use_cases
        .add_member(d.f.owner, d.f.company, d.sibling.id, d.f.agent_principal)
        .await
        .unwrap();
    assert_eq!(
        d.f.persistence
            .management_role(d.f.member, d.company(), target)
            .await
            .unwrap(),
        Some(ManagementRole::Lead(d.unit.id))
    );
    assert_eq!(
        d.f.persistence
            .management_role(d.sibling_lead, d.company(), target)
            .await
            .unwrap(),
        Some(ManagementRole::Lead(d.sibling.id))
    );
    let missing = ManagementTarget::Agent(crate::entities::agent_capability::AgentId::new(
        Uuid::new_v4(),
    ));
    assert_eq!(
        d.f.persistence
            .management_role(d.f.owner, d.company(), missing)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn delegated_admin_department_parent_member_and_role_boundaries() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let f = &d.f;
    let made = f
        .use_cases
        .create_department(
            f.member,
            f.company,
            f.write("Delegated", Some(d.unit.id)),
            None,
        )
        .await
        .unwrap();
    f.use_cases
        .update_department(
            f.member,
            f.company,
            made.id,
            DepartmentEdit {
                name: Some("Managed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    hidden(
        f.use_cases
            .update_department(
                f.member,
                f.company,
                d.unit.id,
                DepartmentEdit {
                    name: Some("Own parent".into()),
                    ..Default::default()
                },
            )
            .await,
    );
    hidden(
        f.use_cases
            .update_department(
                f.member,
                f.company,
                made.id,
                DepartmentEdit {
                    parent_id: Some(Some(d.sibling.id)),
                    ..Default::default()
                },
            )
            .await,
    );
    assert_eq!(
        f.departments
            .get(d.company(), made.id)
            .await
            .unwrap()
            .unwrap()
            .parent_id,
        Some(d.unit.id)
    );
    assert_member_and_role_boundaries(&d, made.id).await;
    f.drop_desk(made.id).await;
    f.use_cases
        .delete_department(f.member, f.company, made.id)
        .await
        .unwrap();
}

async fn assert_member_and_role_boundaries(d: &Delegated, unit: DepartmentId) {
    let f = &d.f;
    f.use_cases
        .add_member(f.member, f.company, d.unit.id, f.owner_principal)
        .await
        .unwrap();
    f.use_cases
        .remove_member(f.member, f.company, d.unit.id, f.owner_principal)
        .await
        .unwrap();
    f.use_cases
        .assign_role(
            f.member,
            f.company,
            unit,
            DepartmentRole::Reviewer,
            f.owner_principal,
        )
        .await
        .unwrap();
    f.use_cases
        .clear_role(
            f.member,
            f.company,
            unit,
            DepartmentRole::Reviewer,
            f.owner_principal,
        )
        .await
        .unwrap();
    hidden(
        f.use_cases
            .assign_role(
                f.member,
                f.company,
                d.unit.id,
                DepartmentRole::Lead,
                f.owner_principal,
            )
            .await,
    );
    hidden(
        f.use_cases
            .clear_role(
                f.member,
                f.company,
                d.unit.id,
                DepartmentRole::Lead,
                f.member_principal,
            )
            .await,
    );
    for actor in [f.member, f.admin] {
        hidden(
            f.use_cases
                .assign_role(
                    actor,
                    f.company,
                    f.root_id().await,
                    DepartmentRole::Reviewer,
                    f.member_principal,
                )
                .await,
        );
    }
}

#[tokio::test]
async fn delegated_admin_department_persistence_refuses_every_sibling_write() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let p = &d.f.departments;
    let scope = d.scope(d.sibling_lead);
    hidden(
        p.create(scope, d.f.write("Forbidden", Some(d.unit.id)), None)
            .await,
    );
    hidden(
        p.update(
            scope,
            d.child.id,
            DepartmentEdit {
                name: Some("Forbidden".into()),
                ..Default::default()
            },
        )
        .await,
    );
    hidden(p.delete(scope, d.child.id).await);
    hidden(
        p.assign_role(
            scope,
            d.child.id,
            DepartmentRole::Reviewer,
            d.f.owner_principal,
        )
        .await,
    );
    hidden(
        p.clear_role(scope, d.unit.id, DepartmentRole::Lead, d.f.member_principal)
            .await,
    );
    hidden(p.add_member(scope, d.unit.id, d.f.owner_principal).await);
    hidden(
        p.remove_member(scope, d.unit.id, d.f.member_principal)
            .await,
    );
    assert_eq!(
        d.f.roles(d.unit.id)
            .await
            .iter()
            .find(|holder| holder.role == DepartmentRole::Lead)
            .unwrap()
            .principal_id,
        d.f.member_principal
    );
}

#[path = "delegated_administration_flow_tests.rs"]
mod flows;

#[path = "delegated_desk_tests.rs"]
mod desks;
#[path = "delegated_schedule_management_tests.rs"]
mod schedules;

#[path = "delegated_agent_management_tests.rs"]
mod agents;

fn ids(departments: &[Department]) -> std::collections::BTreeSet<DepartmentId> {
    departments.iter().map(|department| department.id).collect()
}

/// Plan06 02A.2/D1 r2-3: a lead sees the departments it belongs to and every one it manages,
/// pickers offer only managed destinations, and a plain member or sibling reaches none of it.
#[tokio::test]
async fn delegated_admin_lists_departments_visible_managed_and_picker_scopes() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let remote = d.remote().await;
    let cases = &d.f.use_cases;
    let (plain, _) = teammate(d.f.pool(), d.f.company, "member").await;
    let managed = std::collections::BTreeSet::from([d.unit.id, d.child.id, remote.id]);

    let visible = ids(&cases
        .visible_departments(d.f.member, d.f.company)
        .await
        .unwrap());
    assert!(managed.is_subset(&visible), "{visible:?}");
    assert!(!visible.contains(&d.sibling.id));
    assert_eq!(
        cases
            .visible_department(d.f.member, d.f.company, remote.id)
            .await
            .unwrap()
            .department
            .id,
        remote.id
    );
    hidden(
        cases
            .visible_department(d.f.member, d.f.company, d.sibling.id)
            .await,
    );

    assert_eq!(
        ids(&cases
            .list_departments(d.f.member, d.f.company)
            .await
            .unwrap()),
        managed
    );
    assert_eq!(
        cases
            .managed_units(d.f.member, d.f.company)
            .await
            .unwrap()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        managed
    );
    assert_eq!(
        ids(&cases
            .list_departments(d.sibling_lead, d.f.company)
            .await
            .unwrap()),
        std::collections::BTreeSet::from([d.sibling.id])
    );
    let all = ids(&cases
        .list_departments(d.f.owner, d.f.company)
        .await
        .unwrap());
    assert!(
        managed.is_subset(&all) && all.contains(&d.sibling.id) && all.len() > managed.len() + 1
    );
    assert!(
        cases
            .principal_choices(d.f.member, d.f.company)
            .await
            .is_ok()
    );
    hidden(cases.list_departments(plain, d.f.company).await);
    hidden(cases.principal_choices(plain, d.f.company).await);
    assert!(
        cases
            .managed_units(plain, d.f.company)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !ids(&cases.visible_departments(plain, d.f.company).await.unwrap()).contains(&d.unit.id)
    );
}

/// The unit entry gate and its rendering hint: owner, root admin and a unit lead enter the unit
/// workspaces; a plain member does not, and neither does anyone at company-level administration
/// unless they administer the company (Plan06 D1 r2-1).
#[tokio::test]
async fn delegated_admin_lists_unit_entry_gate_and_hint_follow_the_predicate() {
    use crate::use_cases::company::CompanyUseCases;
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool.clone()).await;
    let companies = CompanyUseCases::new(std::sync::Arc::new(PostgresPersistence::new(pool)));
    let (plain, _) = teammate(d.f.pool(), d.f.company, "member").await;
    for (user, units, company_level) in [
        (d.f.owner, true, true),
        (d.f.admin, true, true),
        (d.f.member, true, false),
        (plain, false, false),
    ] {
        let access = companies.readable_company(user, d.f.company).await.unwrap();
        assert_eq!(access.administers_units, units, "{user}");
        assert_eq!(
            companies
                .administered_company(user, d.f.company)
                .await
                .is_ok(),
            units
        );
        assert_eq!(
            companies.managed_company(user, d.f.company).await.is_ok(),
            company_level
        );
        if !units {
            hidden(companies.administered_company(user, d.f.company).await);
        }
        if !company_level {
            hidden(companies.managed_company(user, d.f.company).await);
        }
        assert_eq!(
            companies
                .list_administered_companies(user)
                .await
                .unwrap()
                .iter()
                .any(|access| access.company.id == d.f.company),
            units
        );
    }
    hidden(
        companies
            .administered_company(d.f.stranger, d.f.company)
            .await,
    );

    // U5: a person who leads the root manages every unit yet does not administer the company.
    let (root_lead, principal) = teammate(d.f.pool(), d.f.company, "member").await;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(d.f.company)
            .fetch_one(d.f.pool())
            .await
            .unwrap();
    d.f.use_cases
        .assign_role(
            d.f.owner,
            d.f.company,
            DepartmentId::new(root),
            DepartmentRole::Lead,
            principal,
        )
        .await
        .unwrap();
    assert!(
        companies
            .administered_company(root_lead, d.f.company)
            .await
            .is_ok()
    );
    hidden(companies.managed_company(root_lead, d.f.company).await);
    assert_eq!(
        ids(&d
            .f
            .use_cases
            .list_departments(root_lead, d.f.company)
            .await
            .unwrap()),
        ids(&d
            .f
            .use_cases
            .list_departments(d.f.owner, d.f.company)
            .await
            .unwrap())
    );
}
