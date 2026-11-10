//! Departments against PostgreSQL (01-departments p01): what the schema refuses outright, what the
//! use cases do over the real adapter, and the competing writers the company write lock settles.
use std::sync::Arc;

use serde_json::json;
use sqlx::PgPool;

use super::*;
use crate::adapters::persistence::test_support::{
    insert_company_with_owner, own_database, provisioning_templates, test_pool,
    wait_until_backends_are_blocked,
};
use crate::application::use_cases::department::DepartmentUseCases;
use crate::application::workflow::WorkflowActor;
use crate::entities::participant::GrantProvenance;

/// A company with an owner, an admin, a plain member (each with their person principal), an agent
/// principal and an external principal, plus a second company with an owner and a person.
struct Departments {
    persistence: PostgresPersistence,
    /// The department port over `persistence`, with the provisioning templates its desks need.
    departments: PostgresDepartments,
    use_cases: DepartmentUseCases,
    company: Uuid,
    owner: Uuid,
    admin: Uuid,
    member: Uuid,
    stranger: Uuid,
    owner_principal: PrincipalId,
    member_principal: PrincipalId,
    agent_principal: PrincipalId,
    external_principal: PrincipalId,
    system_principal: PrincipalId,
    foreign_company: Uuid,
    foreign_principal: PrincipalId,
}

async fn user(pool: &PgPool, label: &str) -> Uuid {
    let id = Uuid::new_v4();
    let unique = id.simple();
    sqlx::query("INSERT INTO users (id, username, email) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(format!("{label}-{unique}"))
        .bind(format!("{label}-{unique}@example.com"))
        .execute(pool)
        .await
        .expect("a user");
    id
}

async fn person(pool: &PgPool, company: Uuid, user: Uuid, label: &str) -> PrincipalId {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO principals (id, company_id, kind, user_id, display_label) \
         VALUES ($1, $2, 'person', $3, $4)",
    )
    .bind(id)
    .bind(company)
    .bind(user)
    .bind(label)
    .execute(pool)
    .await
    .expect("a person principal");
    PrincipalId::new(id)
}

async fn teammate(pool: &PgPool, company: Uuid, role: &str) -> (Uuid, PrincipalId) {
    let id = user(pool, role).await;
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role, access_level) \
         VALUES ($1, $2, $3, $4, 0)",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(id)
    .bind(role)
    .execute(pool)
    .await
    .expect("a company member");
    (id, person(pool, company, id, role).await)
}

/// A company agent made the way production makes them (01DEP-P03-D1): published, with its
/// default desk, so it can lead a department desk. The company gets a model connection for it.
async fn company_agent(
    persistence: &PostgresPersistence,
    company: Uuid,
    owner: Uuid,
    slug: &str,
) -> PrincipalId {
    use crate::use_cases::agent::AgentWrite;
    use crate::use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence};
    CompanyPersistence::replace_model_connections_for_user(
        persistence,
        owner,
        company,
        vec![CompanyModelConnectionWrite {
            provider: "openai".into(),
            api_key: Some("test-key".into()),
            models: vec!["gpt-4o-mini".into(), "gpt-4o".into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let created = crate::adapters::persistence::test_support::company_agents(persistence)
        .create(
            WorkflowActor::authenticated(owner).unwrap(),
            CompanyId::new(company),
            AgentWrite {
                name: format!("Agent {slug}"),
                slug: slug.into(),
                ..AgentWrite::default()
            },
            None,
        )
        .await
        .expect("a company agent");
    PrincipalId::new(
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2")
            .bind(company)
            .bind(created.agent.id)
            .fetch_one(persistence.pool())
            .await
            .unwrap(),
    )
}

/// A bare agent row and principal, never published: a company path makes none since
/// 01DEP-P03-D1, but deleting an agent still demotes its principal, and only an unpublished
/// agent deletes.
async fn agent_principal(pool: &PgPool, company: Uuid, slug: &str) -> PrincipalId {
    let agent = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO agents (id, company_id, name, slug, created_by) VALUES ($1, $2, $3, $3, $4)",
    )
    .bind(agent)
    .bind(company)
    .bind(slug)
    .bind(json!({"actor_type": "system", "actor_id": null, "actor_name": "Test"}))
    .execute(pool)
    .await
    .expect("an agent");
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO principals (id, company_id, kind, agent_id, display_label) \
         VALUES ($1, $2, 'agent', $3, $4)",
    )
    .bind(id)
    .bind(company)
    .bind(agent)
    .bind(slug)
    .execute(pool)
    .await
    .expect("an agent principal");
    PrincipalId::new(id)
}

async fn bare_principal(pool: &PgPool, company: Uuid, kind: &str) -> PrincipalId {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO principals (id, company_id, kind, display_label) VALUES ($1, $2, $3, $3)",
    )
    .bind(id)
    .bind(company)
    .bind(kind)
    .execute(pool)
    .await
    .expect("a principal");
    PrincipalId::new(id)
}

async fn company(pool: &PgPool, owner: Uuid) -> (Uuid, PrincipalId) {
    let id = Uuid::new_v4();
    let slug = format!("dept-{}", id.simple());
    insert_company_with_owner(pool, id, "Departments", &slug, owner).await;
    (id, person(pool, id, owner, "Owner").await)
}

impl Departments {
    async fn on(pool: PgPool) -> Self {
        let persistence = PostgresPersistence::new(pool.clone());
        let owner = user(&pool, "owner").await;
        let (company_id, owner_principal) = company(&pool, owner).await;
        let (admin, _) = teammate(&pool, company_id, "admin").await;
        let (member, member_principal) = teammate(&pool, company_id, "member").await;
        let stranger = user(&pool, "stranger").await;
        let foreign_owner = user(&pool, "foreign").await;
        let (foreign_company, foreign_principal) = company(&pool, foreign_owner).await;
        let departments = PostgresDepartments::new(persistence.clone(), provisioning_templates());
        Self {
            use_cases: DepartmentUseCases::new(
                Arc::new(persistence.clone()),
                Arc::new(departments.clone()),
            ),
            departments,
            agent_principal: company_agent(&persistence, company_id, owner, "worker").await,
            external_principal: bare_principal(&pool, company_id, "external").await,
            system_principal: bare_principal(&pool, company_id, "system").await,
            persistence,
            company: company_id,
            owner,
            admin,
            member,
            stranger,
            owner_principal,
            member_principal,
            foreign_company,
            foreign_principal,
        }
    }

    async fn new() -> Option<Self> {
        Some(Self::on(test_pool().await?).await)
    }

    fn pool(&self) -> &PgPool {
        self.persistence.pool()
    }

    fn write(&self, name: &str, parent: Option<DepartmentId>) -> DepartmentWrite {
        DepartmentWrite {
            name: name.into(),
            slug: name.to_lowercase().replace(' ', "-"),
            parent_id: parent,
            ..DepartmentWrite::default()
        }
    }

    async fn create(&self, name: &str, parent: Option<DepartmentId>) -> Department {
        self.use_cases
            .create_department(self.owner, self.company, self.write(name, parent), None)
            .await
            .expect("the owner creates a department")
    }

    async fn reparent(
        &self,
        department: DepartmentId,
        parent: Option<DepartmentId>,
    ) -> AppResult<Department> {
        self.use_cases
            .update_department(
                self.owner,
                self.company,
                department,
                DepartmentEdit {
                    parent_id: Some(parent),
                    ..DepartmentEdit::default()
                },
            )
            .await
    }

    /// The department's own desk, as its `department:{id}` record names it (01DEP-P03-D8).
    async fn desk_of(&self, department: DepartmentId) -> Uuid {
        sqlx::query_scalar(
            "SELECT (result->'channel'->>'id')::uuid FROM workflow_provisioning_commands \
             WHERE company_id = $1 AND command_key = $2",
        )
        .bind(self.company)
        .bind(format!("department:{department}"))
        .fetch_one(self.pool())
        .await
        .expect("the department's desk is recorded")
    }

    /// Delete the department's own plain desk, which a department delete waits for (decision 9).
    async fn drop_desk(&self, department: DepartmentId) {
        use crate::application::ports::channel::ChannelPersistence;
        ChannelPersistence::delete(&self.persistence, self.desk_of(department).await)
            .await
            .expect("a plain department desk deletes");
    }

    async fn roles(&self, department: DepartmentId) -> Vec<DepartmentRoleHolder> {
        self.departments
            .roles(CompanyId::new(self.company), department)
            .await
            .unwrap()
    }

    async fn members(&self, department: DepartmentId) -> Vec<PrincipalId> {
        self.departments
            .members(CompanyId::new(self.company), department)
            .await
            .unwrap()
            .into_iter()
            .map(|member| member.principal_id)
            .collect()
    }

    /// A raw department row, bypassing the application, as a writer that skips it would.
    async fn insert_raw(
        &self,
        company: Uuid,
        slug: &str,
        parent: Option<Uuid>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO departments (id, company_id, name, slug, parent_id, created_by) \
             VALUES ($1, $2, $3, $3, $4, $5)",
        )
        .bind(id)
        .bind(company)
        .bind(slug)
        .bind(parent)
        .bind(json!({"actor_type": "system", "actor_id": null, "actor_name": "Test"}))
        .execute(self.pool())
        .await
        .map(|_| id)
    }
}

/// The constraint or trigger-declared constraint a refused statement names.
fn refused_by<T: std::fmt::Debug>(result: Result<T, sqlx::Error>) -> String {
    let error = result.expect_err("the schema refuses this row");
    error
        .as_database_error()
        .and_then(|database| database.constraint().map(str::to_string))
        .unwrap_or_else(|| panic!("a named constraint, not {error:?}"))
}

fn raw_role(
    company: Uuid,
    department: Uuid,
    role: &str,
    principal: PrincipalId,
) -> sqlx::query::Query<'static, sqlx::Postgres, sqlx::postgres::PgArguments> {
    sqlx::query(
        "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(company)
    .bind(department)
    .bind(role.to_string())
    .bind(principal.as_uuid())
}

fn raw_member(
    company: Uuid,
    department: Uuid,
    principal: PrincipalId,
) -> sqlx::query::Query<'static, sqlx::Postgres, sqlx::postgres::PgArguments> {
    sqlx::query(
        "INSERT INTO department_members (company_id, department_id, principal_id) \
         VALUES ($1, $2, $3)",
    )
    .bind(company)
    .bind(department)
    .bind(principal.as_uuid())
}

// --- department_write_: the use cases over the real adapter ---------------------------------

#[tokio::test]
async fn department_write_round_trips_create_rename_reparent_and_delete() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let mut write = f.write("  Customer Care ", None);
    write.slug = " Customer Care ".into();
    write.description = Some("  ".into());
    write.charter = Some(" Answer every customer within a day. ".into());
    let care = f
        .use_cases
        .create_department(f.owner, f.company, write, None)
        .await
        .unwrap();
    assert_eq!(care.name, "Customer Care");
    assert_eq!(care.slug.as_str(), "customer-care");
    assert_eq!(care.description, None, "blank text is stored as none");
    assert_eq!(
        care.charter.as_deref(),
        Some("Answer every customer within a day.")
    );
    assert_eq!(care.created_by, CreationProvenance::user(f.owner));

    let support = f.create("Support", None).await;
    let renamed = f
        .use_cases
        .update_department(
            f.admin,
            f.company,
            care.id,
            DepartmentEdit {
                name: Some("Care".into()),
                slug: Some("care".into()),
                description: Some(Some("Front line".into())),
                ..DepartmentEdit::default()
            },
        )
        .await
        .expect("an admin edits a department");
    assert_eq!(
        (
            renamed.name.as_str(),
            renamed.slug.as_str(),
            renamed.description.as_deref()
        ),
        ("Care", "care", Some("Front line"))
    );
    assert_eq!(
        renamed.charter, care.charter,
        "an absent field is unchanged"
    );

    let moved = f.reparent(care.id, Some(support.id)).await.unwrap();
    assert_eq!(moved.parent_id, Some(support.id));
    let read = f
        .departments
        .get(CompanyId::new(f.company), care.id)
        .await
        .unwrap()
        .expect("the edit is stored");
    assert_eq!(read, moved);
    let rooted = f.reparent(care.id, None).await.unwrap();
    assert_eq!(rooted.parent_id, None);

    // Its own desk goes first: a department owning a desk is not deleted (decision 9).
    f.drop_desk(care.id).await;
    f.use_cases
        .delete_department(f.owner, f.company, care.id)
        .await
        .unwrap();
    assert!(
        f.departments
            .get(CompanyId::new(f.company), care.id)
            .await
            .unwrap()
            .is_none()
    );
}

/// PATCH semantics (01DEP-P01-G1-F3): an edit that names only one field keeps every other, and an
/// explicit `None` parent makes the department a root.
#[tokio::test]
async fn department_write_a_name_only_edit_keeps_every_other_field() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let parent = f.create("Holding", None).await;
    let department = f
        .use_cases
        .create_department(
            f.owner,
            f.company,
            DepartmentWrite {
                description: Some("The one with the plants".into()),
                charter: Some("Water everything on Mondays.".into()),
                ..f.write("Garden", Some(parent.id))
            },
            None,
        )
        .await
        .unwrap();
    let renamed = f
        .use_cases
        .update_department(
            f.owner,
            f.company,
            department.id,
            DepartmentEdit {
                name: Some("Greenhouse".into()),
                ..DepartmentEdit::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        renamed,
        Department {
            name: "Greenhouse".into(),
            ..department.clone()
        },
        "slug, description, charter and parent are untouched"
    );
    let rooted = f.reparent(department.id, None).await.unwrap();
    assert_eq!(
        rooted,
        Department {
            name: "Greenhouse".into(),
            parent_id: None,
            ..department
        }
    );
}

#[tokio::test]
async fn department_write_assigns_replaces_and_clears_the_lead() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = f
        .use_cases
        .create_department(
            f.owner,
            f.company,
            f.write("Research", None),
            Some(f.agent_principal),
        )
        .await
        .unwrap();
    let lead = |principal| {
        vec![DepartmentRoleHolder {
            role: DepartmentRole::Lead,
            principal_id: principal,
        }]
    };
    assert_eq!(f.roles(department.id).await, lead(f.agent_principal));

    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            department.id,
            DepartmentRole::Lead,
            f.member_principal,
        )
        .await
        .expect("assigning the lead replaces the holder");
    assert_eq!(f.roles(department.id).await, lead(f.member_principal));
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            department.id,
            DepartmentRole::Lead,
            f.member_principal,
        )
        .await
        .expect("assigning the holder again changes nothing");
    assert_eq!(f.roles(department.id).await, lead(f.member_principal));
    assert!(
        f.members(department.id).await.is_empty(),
        "a role holder need not be a member"
    );

    let not_holder = f
        .use_cases
        .clear_role(
            f.owner,
            f.company,
            department.id,
            DepartmentRole::Lead,
            f.agent_principal,
        )
        .await;
    assert!(
        matches!(not_holder, Err(AppError::NotFound(_))),
        "{not_holder:?}"
    );
    f.use_cases
        .clear_role(
            f.owner,
            f.company,
            department.id,
            DepartmentRole::Lead,
            f.member_principal,
        )
        .await
        .unwrap();
    assert!(f.roles(department.id).await.is_empty());
    assert_eq!(
        f.departments
            .roles_held(CompanyId::new(f.company), f.member_principal)
            .await
            .unwrap(),
        Vec::new()
    );
}

#[tokio::test]
async fn department_write_adds_and_removes_members() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = f.create("Field", None).await;
    for principal in [f.member_principal, f.agent_principal, f.member_principal] {
        f.use_cases
            .add_member(f.owner, f.company, department.id, principal)
            .await
            .expect("people and agents join; joining twice changes nothing");
    }
    let mut members = f.members(department.id).await;
    members.sort();
    let mut expected = vec![f.member_principal, f.agent_principal];
    expected.sort();
    assert_eq!(members, expected);
    assert_eq!(
        f.departments
            .memberships(CompanyId::new(f.company), f.agent_principal)
            .await
            .unwrap(),
        vec![department.id]
    );

    let external = f
        .use_cases
        .add_member(f.owner, f.company, department.id, f.external_principal)
        .await;
    assert!(
        matches!(external, Err(AppError::BadRequest(_))),
        "{external:?}"
    );
    let foreign = f
        .use_cases
        .add_member(f.owner, f.company, department.id, f.foreign_principal)
        .await;
    assert!(matches!(foreign, Err(AppError::NotFound(_))), "{foreign:?}");

    f.use_cases
        .remove_member(f.owner, f.company, department.id, f.agent_principal)
        .await
        .unwrap();
    assert_eq!(f.members(department.id).await, vec![f.member_principal]);
    let again = f
        .use_cases
        .remove_member(f.owner, f.company, department.id, f.agent_principal)
        .await;
    assert!(matches!(again, Err(AppError::NotFound(_))), "{again:?}");
}

#[tokio::test]
async fn department_write_refuses_a_taken_slug_with_conflict() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let billing = f.create("Billing", None).await;
    let duplicate = f
        .use_cases
        .create_department(f.owner, f.company, f.write("Billing", None), None)
        .await;
    assert!(
        matches!(duplicate, Err(AppError::Conflict(_))),
        "{duplicate:?}"
    );
    let other = f.create("Invoices", None).await;
    let rename = f
        .use_cases
        .update_department(
            f.owner,
            f.company,
            other.id,
            DepartmentEdit {
                slug: Some(billing.slug.to_string()),
                ..DepartmentEdit::default()
            },
        )
        .await;
    assert!(matches!(rename, Err(AppError::Conflict(_))), "{rename:?}");
    let malformed = f
        .use_cases
        .create_department(
            f.owner,
            f.company,
            DepartmentWrite {
                slug: "-bad-".into(),
                ..f.write("Bad", None)
            },
            None,
        )
        .await;
    assert!(
        matches!(malformed, Err(AppError::BadRequest(_))),
        "{malformed:?}"
    );
}

#[tokio::test]
async fn department_write_preflight_names_the_departments_in_a_cycle_or_too_deep() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let top = f.create("Top", None).await;
    let middle = f.create("Middle", Some(top.id)).await;
    let cycle = f.reparent(top.id, Some(middle.id)).await;
    let Err(AppError::BadRequest(message)) = cycle else {
        panic!("a cycle is a bad request, not {cycle:?}");
    };
    assert!(message.contains("'Middle' sits inside 'Top'"), "{message}");
    let own = f.reparent(top.id, Some(top.id)).await;
    assert!(matches!(own, Err(AppError::BadRequest(_))), "{own:?}");

    let mut parent = middle.id;
    for level in 3..=MAX_DEPARTMENT_DEPTH {
        parent = f.create(&format!("Level {level}"), Some(parent)).await.id;
    }
    let too_deep = f
        .use_cases
        .create_department(f.owner, f.company, f.write("Level 9", Some(parent)), None)
        .await;
    let Err(AppError::BadRequest(message)) = too_deep else {
        panic!("a ninth level is a bad request, not {too_deep:?}");
    };
    assert!(message.contains("under 'Level 8'"), "{message}");
    // `middle` heads a subtree seven deep (levels 2..8): under a second root it stays at eight,
    // under that root's child it would reach nine.
    let other = f.create("Other Root", None).await;
    let other_child = f.create("Other Child", Some(other.id)).await;
    let deeper = f.reparent(middle.id, Some(other_child.id)).await;
    assert!(matches!(deeper, Err(AppError::BadRequest(_))), "{deeper:?}");
    f.reparent(middle.id, Some(other.id))
        .await
        .expect("a seven-deep subtree fits under a root");
    let missing = f
        .use_cases
        .create_department(
            f.owner,
            f.company,
            f.write("Orphan", Some(DepartmentId::random())),
            None,
        )
        .await;
    assert!(matches!(missing, Err(AppError::NotFound(_))), "{missing:?}");
}

#[tokio::test]
async fn department_write_refuses_deleting_a_department_with_children() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let parent = f.create("Parent", None).await;
    f.create("Child", Some(parent.id)).await;
    let refused = f
        .use_cases
        .delete_department(f.owner, f.company, parent.id)
        .await;
    assert!(matches!(refused, Err(AppError::Conflict(_))), "{refused:?}");
}

/// As `schedule_management_requires_an_owner_or_admin_who_sees_the_channel`: a plain member and a
/// stranger get `NotFound` on every write, and a foreign department is missing to the owner.
#[tokio::test]
async fn department_write_refuses_members_and_strangers_with_not_found() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = f.create("Legal", None).await;
    let all = async |user: Uuid, company: Uuid, department: DepartmentId| -> Vec<AppResult<()>> {
        let uc = &f.use_cases;
        vec![
            uc.create_department(user, company, f.write("New", None), None)
                .await
                .map(drop),
            uc.update_department(
                user,
                company,
                department,
                DepartmentEdit {
                    name: Some("Renamed".into()),
                    ..DepartmentEdit::default()
                },
            )
            .await
            .map(drop),
            uc.assign_role(
                user,
                company,
                department,
                DepartmentRole::Lead,
                f.member_principal,
            )
            .await,
            uc.clear_role(
                user,
                company,
                department,
                DepartmentRole::Lead,
                f.member_principal,
            )
            .await,
            uc.add_member(user, company, department, f.member_principal)
                .await,
            uc.remove_member(user, company, department, f.member_principal)
                .await,
            uc.list_departments(user, company).await.map(drop),
            uc.delete_department(user, company, department).await,
        ]
    };
    for user in [f.member, f.stranger] {
        for result in all(user, f.company, department.id).await {
            assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
        }
    }
    let foreign = DepartmentId::new(
        f.insert_raw(f.foreign_company, "abroad", None)
            .await
            .unwrap(),
    );
    // Everything but `create` and `list` names the department; the owner must not find it.
    let about_the_department = [1, 2, 3, 4, 5, 7];
    let results = all(f.owner, f.company, foreign).await;
    for index in about_the_department {
        assert!(
            matches!(results[index], Err(AppError::NotFound(_))),
            "{index}: {:?}",
            results[index]
        );
    }
    let read = f
        .use_cases
        .visible_department(f.owner, f.company, foreign)
        .await;
    assert!(matches!(read, Err(AppError::NotFound(_))), "{read:?}");
    assert!(
        f.departments
            .get(CompanyId::new(f.foreign_company), foreign)
            .await
            .unwrap()
            .is_some(),
        "nothing happened to the foreign department"
    );
    assert_eq!(
        f.departments
            .get(CompanyId::new(f.company), department.id)
            .await
            .unwrap()
            .map(|department| department.name),
        Some("Legal".into()),
        "nothing happened to this one"
    );
}

#[tokio::test]
async fn department_write_shows_a_member_only_their_departments() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let joined = f.create("Joined", None).await;
    let led = f.create("Led", None).await;
    let other = f.create("Other", None).await;
    f.use_cases
        .add_member(f.owner, f.company, joined.id, f.member_principal)
        .await
        .unwrap();
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            led.id,
            DepartmentRole::Lead,
            f.member_principal,
        )
        .await
        .unwrap();
    let ids = |departments: Vec<Department>| {
        let mut ids: Vec<_> = departments.into_iter().map(|d| d.id).collect();
        ids.sort();
        ids
    };
    let mut theirs = vec![joined.id, led.id];
    theirs.sort();
    assert_eq!(
        ids(f
            .use_cases
            .visible_departments(f.member, f.company)
            .await
            .unwrap()),
        theirs
    );
    let mut every = vec![joined.id, led.id, other.id];
    every.sort();
    for manager in [f.owner, f.admin] {
        assert_eq!(
            ids(f
                .use_cases
                .visible_departments(manager, f.company)
                .await
                .unwrap()),
            every
        );
    }
    let detail = f
        .use_cases
        .visible_department(f.member, f.company, joined.id)
        .await
        .unwrap();
    assert_eq!(detail.members.len(), 1);
    let hidden = f
        .use_cases
        .visible_department(f.member, f.company, other.id)
        .await;
    assert!(matches!(hidden, Err(AppError::NotFound(_))), "{hidden:?}");
    let stranger = f.use_cases.visible_departments(f.stranger, f.company).await;
    assert!(
        matches!(stranger, Err(AppError::NotFound(_))),
        "{stranger:?}"
    );
}

/// As `schedule_tests.rs`' direct persistence check: the adapter re-authorizes every write under
/// the company lock, whatever the use case decided, so a member's or a stranger's scope reaching
/// persistence is refused as if the company did not exist, and nothing changes.
#[tokio::test]
async fn department_write_persistence_reauthorizes_every_write_under_the_lock() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = f.create("Audit", None).await;
    f.use_cases
        .add_member(f.owner, f.company, department.id, f.agent_principal)
        .await
        .unwrap();
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            department.id,
            DepartmentRole::Lead,
            f.agent_principal,
        )
        .await
        .unwrap();
    let before = (
        f.departments
            .list_by_company(CompanyId::new(f.company))
            .await
            .unwrap(),
        f.members(department.id).await,
        f.roles(department.id).await,
    );
    for user in [f.member, f.stranger] {
        let scope = DepartmentScope {
            actor: WorkflowActor::authenticated(user).unwrap(),
            company: CompanyId::new(f.company),
        };
        let p = &f.departments;
        let results: Vec<AppResult<()>> = vec![
            p.create(scope, f.write("Sneaky", None), None)
                .await
                .map(drop),
            p.update(
                scope,
                department.id,
                DepartmentEdit {
                    name: Some("Taken".into()),
                    ..DepartmentEdit::default()
                },
            )
            .await
            .map(drop),
            p.delete(scope, department.id).await,
            p.assign_role(
                scope,
                department.id,
                DepartmentRole::Lead,
                f.member_principal,
            )
            .await,
            p.clear_role(
                scope,
                department.id,
                DepartmentRole::Lead,
                f.agent_principal,
            )
            .await,
            p.add_member(scope, department.id, f.member_principal).await,
            p.remove_member(scope, department.id, f.agent_principal)
                .await,
        ];
        for result in results {
            assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
        }
    }
    let after = (
        f.departments
            .list_by_company(CompanyId::new(f.company))
            .await
            .unwrap(),
        f.members(department.id).await,
        f.roles(department.id).await,
    );
    assert_eq!(after, before, "no refused write changed anything");
}

/// The production paths that take a principal out of a company demote it rather than delete it;
/// each takes it out of its departments too.
#[tokio::test]
async fn department_write_removing_a_teammate_or_deleting_an_agent_leaves_their_departments() {
    use crate::use_cases::agent::AgentPersistence;
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let Some(f) = Departments::new().await else {
        return;
    };
    let department = f
        .use_cases
        .create_department(
            f.owner,
            f.company,
            f.write("Crew", None),
            Some(f.member_principal),
        )
        .await
        .unwrap();
    let unpublished = agent_principal(f.pool(), f.company, "draft-only").await;
    for principal in [f.member_principal, unpublished] {
        f.use_cases
            .add_member(f.owner, f.company, department.id, principal)
            .await
            .unwrap();
    }
    CompanyInvitePersistence::remove_member(&f.persistence, f.company, f.member)
        .await
        .expect("the member is removed from the company");
    assert_eq!(f.members(department.id).await, vec![unpublished]);
    assert!(
        f.roles(department.id).await.is_empty(),
        "the lead left with them"
    );

    let agent: Uuid = sqlx::query_scalar("SELECT agent_id FROM principals WHERE id = $1")
        .bind(unpublished.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap();
    AgentPersistence::delete(&f.persistence, agent)
        .await
        .expect("an agent that never spoke deletes");
    assert!(f.members(department.id).await.is_empty());
}

#[path = "department_schema_tests.rs"]
mod schema;

#[path = "department_competing_tests.rs"]
mod competing;

#[path = "department_desks_tests.rs"]
mod desks;

#[path = "department_agent_desks_tests.rs"]
mod agent_desks;

#[path = "department_lead_desk_tests.rs"]
mod lead_desks;

#[path = "department_roster_tests.rs"]
mod roster;

#[path = "department_ui_reads_tests.rs"]
mod ui_reads;
