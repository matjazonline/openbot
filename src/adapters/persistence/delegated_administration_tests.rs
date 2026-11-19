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

#[derive(Debug, PartialEq, Eq)]
struct AuditRow {
    kind: String,
    acting_role: Option<String>,
    actor_label: Option<String>,
    subject: Option<Uuid>,
    role: Option<String>,
}

async fn audit_rows(pool: &PgPool, company: Uuid, department: DepartmentId) -> Vec<AuditRow> {
    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<Uuid>,
        Option<String>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT transition_kind, acting_role, actor_label, principal_id, role \
             FROM department_administration_events WHERE company_id=$1 AND department_id=$2 \
             ORDER BY created_at, id",
    )
    .bind(company)
    .bind(department.as_uuid())
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(kind, acting_role, actor_label, subject, role)| AuditRow {
            kind,
            acting_role,
            actor_label,
            subject,
            role,
        })
        .collect()
}

async fn label_of(pool: &PgPool, company: Uuid, user: Uuid) -> String {
    sqlx::query_scalar(
        "SELECT display_label FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(company)
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Plan06 02B: every department write a lead makes leaves exactly one history row carrying the
/// role that admitted it — the parent's manager for the department's own lifecycle and roles, the
/// destination's for a move, the department's for members — and nothing for a refused write.
#[tokio::test]
async fn administration_audit_each_write_kind_records_its_admitting_role() {
    use crate::application::ports::channel::ChannelPersistence;
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let cases = &d.f.use_cases;
    let (company, lead) = (d.f.company, d.f.member);
    let lead_role = format!("lead:{}", d.unit.id);
    let lead_label = label_of(d.f.pool(), company, lead).await;
    let (worker, worker_principal) = teammate(d.f.pool(), company, "member").await;
    let _ = worker;

    let team = cases
        .create_department(lead, company, d.f.write("Team", Some(d.unit.id)), None)
        .await
        .unwrap();
    cases
        .update_department(
            lead,
            company,
            team.id,
            DepartmentEdit {
                name: Some("Team renamed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    cases
        .update_department(
            lead,
            company,
            team.id,
            DepartmentEdit {
                parent_id: Some(Some(d.child.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    cases
        .add_member(lead, company, team.id, worker_principal)
        .await
        .unwrap();
    // Adding again changes nothing and records nothing.
    cases
        .add_member(lead, company, team.id, worker_principal)
        .await
        .unwrap();
    cases
        .assign_role(
            lead,
            company,
            team.id,
            DepartmentRole::Lead,
            worker_principal,
        )
        .await
        .unwrap();
    cases
        .clear_role(
            lead,
            company,
            team.id,
            DepartmentRole::Lead,
            worker_principal,
        )
        .await
        .unwrap();
    cases
        .remove_member(lead, company, team.id, worker_principal)
        .await
        .unwrap();
    // A sibling lead's refused write leaves no row.
    hidden(
        cases
            .add_member(d.sibling_lead, company, team.id, worker_principal)
            .await,
    );

    let expected: Vec<(&str, Option<Uuid>, Option<&str>)> = vec![
        ("department_created", None, None),
        ("department_updated", None, None),
        ("department_moved", None, None),
        ("member_added", Some(worker_principal.as_uuid()), None),
        (
            "role_assigned",
            Some(worker_principal.as_uuid()),
            Some("lead"),
        ),
        (
            "role_cleared",
            Some(worker_principal.as_uuid()),
            Some("lead"),
        ),
        ("member_removed", Some(worker_principal.as_uuid()), None),
    ];
    let rows = audit_rows(d.f.pool(), company, team.id).await;
    assert_eq!(
        rows,
        expected
            .into_iter()
            .map(|(kind, subject, role)| AuditRow {
                kind: kind.into(),
                acting_role: Some(lead_role.clone()),
                actor_label: Some(lead_label.clone()),
                subject,
                role: role.map(str::to_owned),
            })
            .collect::<Vec<_>>()
    );

    // The owner deletes it: the row is written before the department goes, as the owner.
    let owner = WorkflowActor::authenticated(d.f.owner).unwrap();
    let desks: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM channels WHERE company_id=$1 AND department_id=$2")
            .bind(company)
            .bind(team.id.as_uuid())
            .fetch_all(d.f.pool())
            .await
            .unwrap();
    for desk in desks {
        ChannelPersistence::delete(&d.f.persistence, owner, company, desk)
            .await
            .unwrap();
    }
    cases
        .delete_department(d.f.owner, company, team.id)
        .await
        .unwrap();
    let last = audit_rows(d.f.pool(), company, team.id)
        .await
        .pop()
        .unwrap();
    assert_eq!(last.kind, "department_deleted");
    assert_eq!(last.acting_role.as_deref(), Some("owner"));
    // A root admin acting is recorded as such.
    let admin_made = cases
        .create_department(d.f.admin, company, d.f.write("By admin", None), None)
        .await
        .unwrap();
    assert_eq!(
        audit_rows(d.f.pool(), company, admin_made.id).await[0]
            .acting_role
            .as_deref(),
        Some("admin")
    );
    // The pane's read: newest first, for the department's managers only.
    let history = cases
        .administration_history(lead, company, d.unit.id)
        .await
        .unwrap();
    assert!(history.len() <= crate::entities::department::DEPARTMENT_HISTORY_ENTRIES);
    hidden(
        cases
            .administration_history(d.sibling_lead, company, d.unit.id)
            .await,
    );
}

/// Rows are append-only and must snapshot the actual actor and subject; a forged person row is
/// refused, and company deletion still removes the history with its tenant.
#[tokio::test]
async fn administration_audit_rows_are_immutable_and_forgery_is_refused() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool.clone()).await;
    let company = d.f.company;
    let rows = audit_rows(&pool, company, d.unit.id).await;
    assert!(
        rows.iter().any(|row| row.kind == "role_assigned"),
        "{rows:?}"
    );
    for statement in [
        "UPDATE department_administration_events SET acting_role='owner' WHERE company_id=$1",
        "DELETE FROM department_administration_events WHERE company_id=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(company)
            .execute(&pool)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("department retirement history is immutable"),
            "{error}"
        );
    }
    let forged = sqlx::query(
        "INSERT INTO department_administration_events (company_id, operation_id, department_id, \
           department_label, transition_kind, actor_kind, actor_principal_id, actor_label, acting_role) \
         SELECT $1, gen_random_uuid(), $2, department_audit_label('Sales'), 'department_updated', \
           'person', principal.id, 'Someone else', 'owner' \
         FROM principals AS principal WHERE principal.company_id=$1 AND principal.user_id=$3",
    )
    .bind(company)
    .bind(d.unit.id.as_uuid())
    .bind(d.f.owner)
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(
        forged
            .to_string()
            .contains("administration audit must snapshot"),
        "{forged}"
    );
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM department_administration_events WHERE company_id=$1",
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(left, 0);
}

/// Plan06 02B.1/r1 item 14: a lead's write racing the removal of its lead role either commits
/// with its history row under the old role, or is refused with no row; never a write without
/// its row or a row without its write.
#[tokio::test]
async fn administration_audit_competing_role_removal_and_management_write_in_both_orders() {
    use crate::adapters::persistence::test_support::{
        WriteBarrier, competing_writes, own_database,
    };
    for write_first in [true, false] {
        // An own database: the barrier is a table lock no concurrent test may share.
        let Some(db) = own_database().await else {
            return;
        };
        let d = Delegated::on(db.pool.clone()).await;
        let (company, lead) = (d.f.company, d.f.member);
        let (_, worker) = teammate(d.f.pool(), company, "member").await;
        let write = d.f.use_cases.add_member(lead, company, d.unit.id, worker);
        let revoke = d.f.use_cases.clear_role(
            d.f.owner,
            company,
            d.unit.id,
            DepartmentRole::Lead,
            d.f.member_principal,
        );
        let added = if write_first {
            let result =
                competing_writes(d.f.pool(), WriteBarrier::DepartmentMembers, write, revoke).await;
            result.second.unwrap();
            result.first
        } else {
            let result =
                competing_writes(d.f.pool(), WriteBarrier::DeskGrants, revoke, write).await;
            result.first.unwrap();
            result.second
        };
        let rows = audit_rows(d.f.pool(), company, d.unit.id).await;
        let member_rows: Vec<_> = rows
            .iter()
            .filter(|row| row.kind == "member_added")
            .collect();
        let is_member: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM department_members WHERE company_id=$1 AND department_id=$2 AND principal_id=$3)",
        )
        .bind(company)
        .bind(d.unit.id.as_uuid())
        .bind(worker.as_uuid())
        .fetch_one(d.f.pool())
        .await
        .unwrap();
        if write_first {
            added.unwrap();
            assert!(is_member);
            assert_eq!(member_rows.len(), 1);
            assert_eq!(
                member_rows[0].acting_role.as_deref(),
                Some(format!("lead:{}", d.unit.id).as_str())
            );
        } else {
            hidden(added);
            assert!(!is_member && member_rows.is_empty(), "{rows:?}");
        }
        assert!(
            rows.iter().any(
                |row| row.kind == "role_cleared" && row.acting_role.as_deref() == Some("owner")
            )
        );
    }
}

/// B2-F1: replacing a single-holder role records the holder it was taken from, then the new one,
/// in one operation with the same acting role.
#[tokio::test]
async fn administration_audit_replacing_a_lead_records_the_replaced_holder() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let (_, successor) = teammate(d.f.pool(), d.f.company, "member").await;
    d.f.use_cases
        .assign_role(
            d.f.owner,
            d.f.company,
            d.unit.id,
            DepartmentRole::Lead,
            successor,
        )
        .await
        .unwrap();
    type Row = (String, Option<Uuid>, Option<String>, Option<String>, Uuid);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT transition_kind, principal_id, role, acting_role, operation_id \
         FROM department_administration_events WHERE company_id=$1 AND department_id=$2 \
           AND actor_kind='person' ORDER BY created_at, id",
    )
    .bind(d.f.company)
    .bind(d.unit.id.as_uuid())
    .fetch_all(d.f.pool())
    .await
    .unwrap();
    let last: Vec<_> = rows.iter().rev().take(2).rev().collect();
    assert_eq!(
        last.iter()
            .map(|(kind, principal, role, acting, _)| (
                kind.as_str(),
                *principal,
                role.as_deref(),
                acting.as_deref()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "role_cleared",
                Some(d.f.member_principal.as_uuid()),
                Some("lead"),
                Some("owner")
            ),
            (
                "role_assigned",
                Some(successor.as_uuid()),
                Some("lead"),
                Some("owner")
            ),
        ]
    );
    assert_eq!(last[0].4, last[1].4, "one operation");
}

/// B2-F2/F3: removing members from the root on the Departments pane is the root's history — an
/// agent as a root admin, a person as the owner — and a person's departure records every
/// department they leave, written before their principal is retired.
#[tokio::test]
async fn administration_audit_root_removal_records_every_department_left() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let (company, cases) = (d.f.company, &d.f.use_cases);
    let root = d.f.root_id().await;
    let (leaver, leaver_principal) = teammate(d.f.pool(), company, "member").await;
    let _ = leaver;
    cases
        .add_member(d.f.owner, company, d.child.id, leaver_principal)
        .await
        .unwrap();
    cases
        .add_member(d.f.owner, company, root, d.f.agent_principal)
        .await
        .unwrap();
    cases
        .remove_member(d.f.admin, company, root, d.f.agent_principal)
        .await
        .unwrap();
    cases
        .remove_member(d.f.owner, company, root, leaver_principal)
        .await
        .unwrap();
    let rows: Vec<(Uuid, Option<Uuid>, Option<String>)> = sqlx::query_as(
        "SELECT department_id, principal_id, acting_role FROM department_administration_events \
         WHERE company_id=$1 AND transition_kind='member_removed' ORDER BY created_at, id",
    )
    .bind(company)
    .fetch_all(d.f.pool())
    .await
    .unwrap();
    assert!(rows.contains(&(
        root.as_uuid(),
        Some(d.f.agent_principal.as_uuid()),
        Some("admin".to_owned())
    )));
    for department in [root.as_uuid(), d.child.id.as_uuid()] {
        assert!(
            rows.contains(&(
                department,
                Some(leaver_principal.as_uuid()),
                Some("owner".to_owned())
            )),
            "{rows:?}"
        );
    }
    let still_member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM department_members WHERE company_id=$1 AND principal_id=$2)",
    )
    .bind(company)
    .bind(leaver_principal.as_uuid())
    .fetch_one(d.f.pool())
    .await
    .unwrap();
    assert!(!still_member, "the departure itself happened");
}

/// B2-F2: a placement whose actor manages no unit there is refused, and the transaction it ran in
/// keeps nothing.
#[tokio::test]
async fn administration_audit_unmanaged_placement_is_refused_without_history() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let (outsider, _) = teammate(d.f.pool(), d.f.company, "member").await;
    let before = audit_rows(d.f.pool(), d.f.company, d.sibling.id).await;
    let mut tx = d.f.pool().begin().await.unwrap();
    let refused = crate::adapters::persistence::department::place_in_department_on(
        &mut tx,
        &crate::adapters::persistence::test_support::provisioning_templates(),
        d.company(),
        WorkflowActor::authenticated(outsider).unwrap(),
        d.sibling.id,
        d.f.agent_principal,
        None,
    )
    .await;
    hidden(refused);
    tx.rollback().await.unwrap();
    assert_eq!(
        audit_rows(d.f.pool(), d.f.company, d.sibling.id).await,
        before
    );
}
