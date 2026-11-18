use super::*;
use crate::application::use_cases::administration::{
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
