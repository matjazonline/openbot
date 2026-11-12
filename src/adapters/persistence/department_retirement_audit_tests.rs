use super::*;
use crate::adapters::persistence::test_support::own_database;
use crate::use_cases::company_invite::CompanyInvitePersistence;
use serde_json::{Value, json};

struct AuditFixture {
    _database: crate::adapters::persistence::test_support::OwnDatabase,
    departments: Departments,
}
impl AuditFixture {
    async fn new() -> Self {
        let database = own_database().await.unwrap();
        let departments = Box::pin(Departments::on(database.pool.clone())).await;
        Self {
            _database: database,
            departments,
        }
    }
    async fn held_department(&self, slug: &str, name: &str) -> DepartmentId {
        let f = &self.departments;
        let department = Box::pin(f.use_cases.create_department(
            f.owner,
            f.company,
            DepartmentWrite {
                name: "Audit boundary".into(),
                slug: slug.into(),
                ..Default::default()
            },
            None,
        ))
        .await
        .unwrap();
        f.use_cases
            .update_department(
                f.owner,
                f.company,
                department.id,
                DepartmentEdit {
                    name: Some(name.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        f.use_cases
            .add_member(f.owner, f.company, department.id, f.member_principal)
            .await
            .unwrap();
        f.use_cases
            .assign_role(
                f.owner,
                f.company,
                department.id,
                DepartmentRole::Reviewer,
                f.member_principal,
            )
            .await
            .unwrap();
        department.id
    }
    async fn events(&self) -> Vec<Value> {
        sqlx::query_scalar("SELECT to_jsonb(event) FROM department_administration_events AS event WHERE company_id=$1 ORDER BY department_id,role")
            .bind(self.departments.company).fetch_all(self.departments.pool()).await.unwrap()
    }
    async fn demote(&self) {
        self.departments
            .persistence
            .remove_member(self.departments.company, self.departments.member)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_retirement_audit_name_representation_never_blocks_valid_long_names()
 {
    let f = Box::pin(AuditFixture::new()).await;
    let names = vec![
        "x".repeat(127),
        "x".repeat(128),
        "x".repeat(129),
        "界".repeat(127),
        "界".repeat(128),
        "界".repeat(129),
        "😀".repeat(129),
        "x".repeat(5000),
        "😀".repeat(1500),
        "\u{1}".repeat(5000),
    ];
    let mut expected = std::collections::BTreeMap::new();
    for (index, name) in names.into_iter().enumerate() {
        let department = f.held_department(&format!("audit-{index}"), &name).await;
        expected.insert(department.as_uuid(), name);
    }
    let principal_label = "\u{1}".repeat(255);
    sqlx::query("UPDATE principals SET display_label=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.departments.company)
        .bind(f.departments.member_principal.as_uuid())
        .bind(&principal_label)
        .execute(f.departments.pool())
        .await
        .unwrap();
    f.demote().await;
    let events = f.events().await;
    assert_eq!(events.len(), expected.len());
    let operations: std::collections::BTreeSet<_> = events
        .iter()
        .map(|event| event["operation_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        operations.len(),
        1,
        "one principal transition captures all held roles"
    );
    for event in &events {
        let id = Uuid::parse_str(event["department_id"].as_str().unwrap()).unwrap();
        let name = &expected[&id];
        assert_eq!(
            event["department_label"],
            json!({"prefix":name.chars().take(128).collect::<String>(),"truncated":name.chars().nth(128).is_some()})
        );
        assert_eq!(event["principal_label"], principal_label);
        assert_eq!(event["principal_id"], json!(f.departments.member_principal));
        assert_eq!(event["role"], "reviewer");
    }
    let largest:i32=sqlx::query_scalar("SELECT max(octet_length(to_jsonb(event)::text)) FROM department_administration_events AS event WHERE company_id=$1")
        .bind(f.departments.company).fetch_one(f.departments.pool()).await.unwrap();
    assert!(largest <= 4096, "maximum encoded audit row was {largest}");
    let stored: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'name',name) FROM departments WHERE company_id=$1",
    )
    .bind(f.departments.company)
    .fetch_all(f.departments.pool())
    .await
    .unwrap();
    for row in stored {
        let id = Uuid::parse_str(row["id"].as_str().unwrap()).unwrap();
        if let Some(name) = expected.get(&id) {
            assert_eq!(row["name"], *name, "the source name must never be clipped");
        }
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_retirement_audit_history_survives_principal_department_delete_and_regrant()
 {
    let f = Box::pin(AuditFixture::new()).await;
    let department = f.held_department("historical-audit", "History").await;
    let mut tx = f.departments.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.departments.company)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE principals SET kind='external',user_id=NULL WHERE company_id=$1 AND id=$2")
        .bind(f.departments.company)
        .bind(f.departments.member_principal.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let original = f.events().await;
    assert_eq!(original.len(), 1);
    let mut tx = f.departments.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.departments.company)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE principals SET kind='person',user_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.departments.company)
        .bind(f.departments.member_principal.as_uuid())
        .bind(f.departments.member)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    f.departments
        .use_cases
        .add_member(
            f.departments.owner,
            f.departments.company,
            department,
            f.departments.member_principal,
        )
        .await
        .unwrap();
    f.departments
        .use_cases
        .assign_role(
            f.departments.owner,
            f.departments.company,
            department,
            DepartmentRole::Reviewer,
            f.departments.member_principal,
        )
        .await
        .unwrap();
    assert_eq!(
        f.events().await,
        original,
        "later committed re-grant must preserve history"
    );
    let mut tx = f.departments.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.departments.company)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND id=$2")
        .bind(f.departments.company)
        .bind(f.departments.member_principal.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let deleted = f.events().await;
    assert_eq!(deleted.len(), 2);
    assert!(
        deleted
            .iter()
            .any(|event| event["transition_kind"] == "deleted")
    );
    f.departments.drop_desk(department).await;
    f.departments
        .use_cases
        .delete_department(f.departments.owner, f.departments.company, department)
        .await
        .unwrap();
    assert_eq!(
        f.events().await,
        deleted,
        "historical IDs/labels survive permitted department deletion"
    );
    for statement in [
        "UPDATE department_administration_events SET principal_label='forged' WHERE company_id=$1",
        "DELETE FROM department_administration_events WHERE company_id=$1",
    ] {
        let result = sqlx::query(statement)
            .bind(f.departments.company)
            .execute(f.departments.pool())
            .await;
        assert_eq!(refused_by(result), "department_retirement_audit_immutable");
    }
    let forged=sqlx::query("INSERT INTO department_administration_events(company_id,id,operation_id,principal_id,principal_kind,principal_label,department_id,department_label,role,transition_kind,actor_kind,created_at) \
        SELECT company_id,id,operation_id,principal_id,principal_kind,principal_label,department_id,department_label,role,transition_kind,actor_kind,created_at \
        FROM department_administration_events WHERE company_id=$1 ON CONFLICT(company_id,id) DO UPDATE SET principal_label='forged'")
        .bind(f.departments.company).execute(f.departments.pool()).await;
    assert_eq!(refused_by(forged), "department_retirement_audit_source");
    assert_eq!(f.events().await, deleted);
}

async fn install_fault(f: &AuditFixture, foreign_department: Uuid) {
    sqlx::raw_sql("CREATE TABLE retirement_audit_test_fault(field text NOT NULL,foreign_company uuid,foreign_department uuid,foreign_principal uuid); \
        CREATE FUNCTION corrupt_retirement_audit_test() RETURNS trigger LANGUAGE plpgsql AS $$ DECLARE fault retirement_audit_test_fault; BEGIN \
        SELECT field,foreign_company,foreign_department,foreign_principal INTO fault FROM retirement_audit_test_fault; \
        CASE fault.field WHEN 'prefix' THEN NEW.department_label:=jsonb_set(NEW.department_label,'{prefix}','\"forged\"'); \
          WHEN 'truncation' THEN NEW.department_label:=jsonb_set(NEW.department_label,'{truncated}','true'); \
          WHEN 'department' THEN NEW.department_id:=fault.foreign_department; \
          WHEN 'principal' THEN NEW.principal_id:=fault.foreign_principal; \
          WHEN 'company' THEN NEW.company_id:=fault.foreign_company; \
          WHEN 'principal_label' THEN NEW.principal_label:='forged'; ELSE NULL; END CASE; RETURN NEW; END $$; \
        CREATE TRIGGER a00_corrupt_retirement_audit_test BEFORE INSERT ON department_administration_events \
          FOR EACH ROW EXECUTE FUNCTION corrupt_retirement_audit_test();")
        .execute(f.departments.pool()).await.unwrap();
    sqlx::query("INSERT INTO retirement_audit_test_fault VALUES('none',$1,$2,$3)")
        .bind(f.departments.foreign_company)
        .bind(foreign_department)
        .bind(f.departments.foreign_principal.as_uuid())
        .execute(f.departments.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_reviewer_follow_retirement_audit_rejects_corrupt_actual_source_and_rolls_back() {
    let f = Box::pin(AuditFixture::new()).await;
    let department = f.held_department("source-guard", "Actual label").await;
    let foreign = f
        .departments
        .insert_raw(f.departments.foreign_company, "foreign-audit", None)
        .await
        .unwrap();
    install_fault(&f, foreign).await;
    for field in [
        "prefix",
        "truncation",
        "department",
        "principal",
        "company",
        "principal_label",
    ] {
        sqlx::query("UPDATE retirement_audit_test_fault SET field=$1")
            .bind(field)
            .execute(f.departments.pool())
            .await
            .unwrap();
        let error = f
            .departments
            .persistence
            .remove_member(f.departments.company, f.departments.member)
            .await
            .unwrap_err();
        assert!(
            matches!(error,AppError::Database(ref message) if message.contains("retirement audit must snapshot")),
            "{field}: {error:?}"
        );
        assert!(
            f.events().await.is_empty(),
            "{field}: failed retirement left audit history"
        );
        assert_eq!(
            f.departments.roles(department).await.len(),
            1,
            "{field}: failed retirement cleared a role"
        );
        let kind: String =
            sqlx::query_scalar("SELECT kind FROM principals WHERE company_id=$1 AND id=$2")
                .bind(f.departments.company)
                .bind(f.departments.member_principal.as_uuid())
                .fetch_one(f.departments.pool())
                .await
                .unwrap();
        assert_eq!(
            kind, "person",
            "{field}: failed retirement demoted the person"
        );
    }
    sqlx::query("UPDATE retirement_audit_test_fault SET field='none'")
        .execute(f.departments.pool())
        .await
        .unwrap();
    f.demote().await;
    assert_eq!(f.events().await.len(), 1);
}

#[tokio::test]
async fn workflow_reviewer_follow_retirement_audit_requires_material_clear_only_in_its_creation_transaction()
 {
    let f = Box::pin(AuditFixture::new()).await;
    let department = f.held_department("material-clear", "Material").await;
    let mut tx = f.departments.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.departments.company)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE principals SET kind='external',user_id=NULL WHERE company_id=$1 AND id=$2")
        .bind(f.departments.company)
        .bind(f.departments.member_principal.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE principals SET kind='person',user_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.departments.company)
        .bind(f.departments.member_principal.as_uuid())
        .bind(f.departments.member)
        .execute(&mut *tx)
        .await
        .unwrap();
    raw_member(
        f.departments.company,
        department.as_uuid(),
        f.departments.member_principal,
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    raw_role(
        f.departments.company,
        department.as_uuid(),
        "reviewer",
        f.departments.member_principal,
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        refused_by(tx.commit().await),
        "department_retirement_role_cleared"
    );
    assert!(f.events().await.is_empty());
    assert_eq!(
        f.departments.roles(department).await.len(),
        1,
        "the failed commit restored the original role"
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_retirement_audit_agent_nonholder_and_tenant_cascade_use_actual_lifecycle()
 {
    use crate::application::ports::account_deletion::HostAccountDeletion;
    use crate::application::use_cases::agent::AgentPersistence;
    let f = Box::pin(AuditFixture::new()).await;
    f.demote().await;
    f.demote().await;
    assert!(
        f.events().await.is_empty(),
        "a nonholder/replayed removal must not invent history"
    );
    let unpublished = AgentPersistence::create(
        &f.departments.persistence,
        f.departments.company,
        crate::application::use_cases::agent::AgentWrite {
            name: "Unpublished retiree".into(),
            slug: "unpublished-retiree".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let retiring = PrincipalId::new(
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
            .bind(f.departments.company)
            .bind(unpublished.id)
            .fetch_one(f.departments.pool())
            .await
            .unwrap(),
    );
    let department = f
        .departments
        .use_cases
        .create_department(
            f.departments.owner,
            f.departments.company,
            DepartmentWrite {
                name: "Agent retirement".into(),
                slug: "agent-retirement".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    f.departments
        .use_cases
        .add_member(
            f.departments.owner,
            f.departments.company,
            department.id,
            retiring,
        )
        .await
        .unwrap();
    f.departments
        .use_cases
        .assign_role(
            f.departments.owner,
            f.departments.company,
            department.id,
            DepartmentRole::Reviewer,
            retiring,
        )
        .await
        .unwrap();
    let agent: Uuid =
        sqlx::query_scalar("SELECT agent_id FROM principals WHERE company_id=$1 AND id=$2")
            .bind(f.departments.company)
            .bind(retiring.as_uuid())
            .fetch_one(f.departments.pool())
            .await
            .unwrap();
    AgentPersistence::delete(&f.departments.persistence, agent)
        .await
        .unwrap();
    let events = f.events().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["principal_id"], json!(retiring));
    assert_eq!(events[0]["principal_kind"], "agent");
    assert!(f.departments.roles(department.id).await.is_empty());
    f.departments
        .persistence
        .delete_user_account(f.departments.owner)
        .await
        .unwrap();
    assert!(
        f.events().await.is_empty(),
        "complete tenant deletion must cascade audit history"
    );
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM companies WHERE id=$1)")
        .bind(f.departments.company)
        .fetch_one(f.departments.pool())
        .await
        .unwrap();
    assert!(!exists, "the actual owning account teardown committed");
}
