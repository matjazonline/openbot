use super::*;
use crate::entities::department::RoleAdmissionRefusal;

fn constraint(error: &sqlx::Error, expected: &str) {
    let database = error
        .as_database_error()
        .expect("a concrete SQL guard failure");
    assert_eq!(database.code().as_deref(), Some("23514"), "{error}");
    assert_eq!(database.constraint(), Some(expected), "{error}");
}

#[tokio::test]
async fn workflow_role_admission_sql_refusal_codes_and_bounded_payload_match_every_domain_kind() {
    let f = RoleFixture::new().await;
    for role in DepartmentRole::ALL {
        for kind in [
            RoleRefusalKind::NoHolder,
            RoleRefusalKind::WrongKind,
            RoleRefusalKind::Ambiguous,
            RoleRefusalKind::DepthBound,
            RoleRefusalKind::TooMany,
            RoleRefusalKind::Unavailable,
            RoleRefusalKind::MissingDepartment,
        ] {
            let payload = serde_json::to_value(RoleAdmissionRefusal::new(
                StepId::parse("start").unwrap(),
                role,
                kind,
            ))
            .unwrap();
            let result: (bool, String) = sqlx::query_as(
                "SELECT valid_workflow_role_refusal($1),workflow_role_refusal_code($1)",
            )
            .bind(&payload)
            .fetch_one(f.p().pool())
            .await
            .unwrap();
            assert_eq!(result, (true, kind.code().into()));
            for changed in [
                json!({"format":"unknown"}),
                json!({"step":""}),
                json!({"step":"x".repeat(129)}),
                json!({"kind":"unknown"}),
                json!({"role":"owner"}),
                json!({"principals":[Uuid::new_v4()]}),
            ] {
                let mut invalid = payload.clone();
                invalid
                    .as_object_mut()
                    .unwrap()
                    .extend(changed.as_object().unwrap().clone());
                let valid: bool = sqlx::query_scalar("SELECT valid_workflow_role_refusal($1)")
                    .bind(&invalid)
                    .fetch_one(f.p().pool())
                    .await
                    .unwrap();
                assert!(!valid, "malformed role context must fail closed: {invalid}");
            }
        }
    }
}

#[tokio::test]
async fn workflow_role_admission_sql_rows_and_future_resolution_revision_are_immutable() {
    let f = RoleFixture::new().await;
    let run = f.admitted("immutable").await;
    for statement in [
        "UPDATE workflow_run_role_bindings SET step='replacement' WHERE company_id=$1 AND run_id=$2",
        "DELETE FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2",
        "UPDATE workflow_run_role_bindings SET resolution_revision=1 WHERE company_id=$1 AND run_id=$2",
    ] {
        let error = sqlx::query(statement)
            .bind(f.company().as_uuid())
            .bind(run.as_uuid())
            .execute(f.p().pool())
            .await
            .unwrap_err();
        constraint(&error, "workflow_guard:guard_workflow_role_binding");
    }
    let error = sqlx::query("INSERT INTO workflow_run_role_bindings(company_id,run_id,manifest_id,version_id,step,role,resolution_revision,department_id,principal_id,agent_id,agent_revision_id,created_at) SELECT company_id,run_id,manifest_id,version_id,step,role,1,department_id,principal_id,agent_id,agent_revision_id,created_at FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2")
        .bind(f.company().as_uuid()).bind(run.as_uuid()).execute(f.p().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    // A later resolution is now legal only for People with its own canonical proof.
    // An admission's agent cannot manufacture one or change its frozen revision zero.
    constraint(&error, "workflow_guard:guard_workflow_role_binding");
    let error = sqlx::query("UPDATE agent_dependency_manifests SET content=content||' ' WHERE company_id=$1 AND root_run_id=$2")
        .bind(f.company().as_uuid()).bind(run.as_uuid()).execute(f.p().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
}

#[tokio::test]
async fn workflow_role_admission_sql_missing_members_and_canonical_requirements_roll_back_the_root()
{
    let f = RoleFixture::new().await;
    // Inject omissions before the normal writer inserts. All production deferred validators stay
    // enabled: an internally self-consistent omission still must fail against frozen source roles.
    sqlx::raw_sql("CREATE FUNCTION omit_role_members() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; \
        CREATE TRIGGER omit_role_members BEFORE INSERT ON workflow_run_role_bindings FOR EACH ROW EXECUTE FUNCTION omit_role_members()")
        .execute(f.p().pool()).await.unwrap();
    let command = f.prepared("missing-members").await;
    let error = f.p().admit(&command).await.unwrap_err();
    assert!(
        matches!(error, AppError::WorkflowGuard(ref violation) if violation.guard() == "check_workflow_role_manifest" && violation.message().contains("role members must exactly match")),
        "{error:?}"
    );
    sqlx::query("DROP TRIGGER omit_role_members ON workflow_run_role_bindings")
        .execute(f.p().pool())
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION omit_role_requirement() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
        NEW.content := jsonb_set(NEW.content::jsonb,'{roles}','[]')::text; NEW.digest := sha256(convert_to(NEW.content,'UTF8')); RETURN NEW; END $$; \
        CREATE TRIGGER omit_role_requirement BEFORE INSERT ON agent_dependency_manifests FOR EACH ROW EXECUTE FUNCTION omit_role_requirement(); \
        CREATE TRIGGER omit_role_members BEFORE INSERT ON workflow_run_role_bindings FOR EACH ROW EXECUTE FUNCTION omit_role_members()")
        .execute(f.p().pool()).await.unwrap();
    let command = f.prepared("missing-requirement").await;
    let error = f.p().admit(&command).await.unwrap_err();
    assert!(
        matches!(error, AppError::WorkflowGuard(ref violation) if violation.guard() == "check_workflow_role_manifest" && violation.message().contains("exactly cover frozen workflow requirements")),
        "{error:?}"
    );
    for table in [
        "workflow_runs",
        "workflow_executions",
        "workflow_run_role_bindings",
        "agent_dependency_manifests",
    ] {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE company_id=$1"))
                .bind(f.company().as_uuid())
                .fetch_one(f.p().pool())
                .await
                .unwrap();
        assert_eq!(count, 0, "the failed complete-set guard rolls back {table}");
    }
}

#[tokio::test]
async fn workflow_role_admission_sql_company_teardown_releases_the_complete_capture() {
    let f = RoleFixture::new().await;
    f.admitted("teardown").await;
    CompanyPersistence::delete(f.p(), f.company().as_uuid())
        .await
        .unwrap();
    for table in [
        "workflow_run_role_bindings",
        "agent_dependency_manifests",
        "agent_manifest_revisions",
        "agent_manifest_workflows",
    ] {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE company_id=$1"))
                .bind(f.company().as_uuid())
                .fetch_one(f.p().pool())
                .await
                .unwrap();
        assert_eq!(
            count, 0,
            "owned capture table {table} cascades during whole-company teardown"
        );
    }
}

#[tokio::test]
async fn workflow_role_admission_sql_malformed_frozen_role_requirements_fail_closed() {
    let f = RoleFixture::new().await;
    let state = f.admission.binding.state().await;
    let version = state
        .configuration
        .bundle()
        .compiled()
        .graph()
        .definition()
        .version_id
        .as_uuid();
    let document: Value = sqlx::query_scalar("SELECT convert_from(bundle,'UTF8')::jsonb FROM workflow_versions WHERE company_id=$1 AND id=$2")
        .bind(f.company().as_uuid()).bind(version).fetch_one(f.p().pool()).await.unwrap();
    for location in ["root", "versions", "roles"] {
        let mut changed = document.clone();
        match location {
            "root" => changed["root"] = json!(Uuid::new_v4()),
            "versions" => changed["versions"] = json!([]),
            "roles" => changed["versions"][0]["compiled"]["roles"] = json!({}),
            _ => unreachable!(),
        }
        let error = sqlx::query("SELECT workflow_publication_role_requirements($1,$2)")
            .bind(serde_json::to_vec(&changed).unwrap())
            .bind(version)
            .execute(f.p().pool())
            .await
            .unwrap_err();
        constraint(&error, "workflow_guard:check_workflow_role_manifest");
    }
}

#[tokio::test]
async fn workflow_role_admission_retained_department_refuses_delete_with_a_history_conflict() {
    use crate::application::ports::channel::ChannelUpdate;
    let f = RoleFixture::new().await;
    let run = f.admitted("retained-department").await;
    let binding = &f.admission.binding;
    binding
        .service()
        .deactivate(SetBindingActivity {
            target: binding.target,
            expected: binding.state().await.revision,
        })
        .await
        .unwrap();
    ChannelPersistence::update(
        f.p(),
        ChannelUpdate {
            company_id: f.company().as_uuid(),
            channel_id: f.channel,
            write: ChannelWrite {
                name: "Role intake".into(),
                slug: "role-intake".into(),
                enabled: true,
                ownership: Some(ChannelOwnershipWrite {
                    department_id: Some(None),
                    members_only: Some(false),
                }),
                ..Default::default()
            },
        },
    )
    .await
    .unwrap();
    let other_desks: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM channels WHERE company_id=$1 AND department_id=$2")
            .bind(f.company().as_uuid())
            .bind(f.department.as_uuid())
            .fetch_all(f.p().pool())
            .await
            .unwrap();
    for desk in other_desks {
        let current = ChannelPersistence::get_by_id(f.p(), desk)
            .await
            .unwrap()
            .unwrap();
        ChannelPersistence::update(
            f.p(),
            ChannelUpdate {
                company_id: f.company().as_uuid(),
                channel_id: desk,
                write: ChannelWrite {
                    name: current.name,
                    slug: current.slug.to_string(),
                    enabled: current.enabled,
                    ownership: Some(ChannelOwnershipWrite {
                        department_id: Some(None),
                        members_only: Some(false),
                    }),
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();
    }
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    let error = departments
        .delete_department(
            binding.target.actor.user_id(),
            f.company().as_uuid(),
            f.department,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::Conflict(ref message) if message.contains("retained by workflow role history")),
        "{error:?}"
    );
    assert_eq!(f.manifest(run).await.roles[0].department_id, f.department);
}

#[tokio::test]
async fn workflow_role_admission_wrong_kind_and_ambiguous_real_holders_refuse_before_run_creation()
{
    let f = RoleFixture::new().await;
    let owner: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(f.company().as_uuid())
            .bind(f.admission.binding.target.actor.user_id())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    departments
        .assign_role(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            f.department,
            DepartmentRole::Lead,
            PrincipalId::new(owner),
        )
        .await
        .unwrap();
    let command = f.prepared("person-lead").await;
    let error = f.p().admit(&command).await.unwrap_err();
    assert!(
        matches!(error, AppError::WorkflowRole(ref refusal) if refusal.kind == RoleRefusalKind::WrongKind && refusal.role == DepartmentRole::Lead)
    );
    assert_eq!(f.admission.counts().await["runs"], 0);
    let f = RoleFixture::with_role(DepartmentRole::Reviewer).await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    let (other, principal) = create_agent(&f.admission.binding, "other-reviewer").await;
    f.admission.binding.fixture.publish_agent(other).await;
    for principal in [f.principal, principal] {
        departments
            .assign_role(
                f.admission.binding.target.actor.user_id(),
                f.company().as_uuid(),
                f.department,
                DepartmentRole::Reviewer,
                principal,
            )
            .await
            .unwrap();
    }
    let command = f.prepared("ambiguous-reviewer").await;
    let error = f.p().admit(&command).await.unwrap_err();
    assert!(
        matches!(error, AppError::WorkflowRole(ref refusal) if refusal.kind == RoleRefusalKind::Ambiguous && refusal.role == DepartmentRole::Reviewer)
    );
    assert_eq!(f.admission.counts().await["runs"], 0);
}

#[tokio::test]
async fn workflow_role_admission_sql_capture_refuses_foreign_principals_and_unselected_revisions() {
    let f = RoleFixture::new().await;
    let foreign = RoleFixture::new().await;
    let run = f.admitted("foreign").await;
    let error = sqlx::query("INSERT INTO workflow_run_role_bindings(company_id,run_id,manifest_id,version_id,step,role,department_id,principal_id,agent_id,agent_revision_id) \
        SELECT company_id,run_id,manifest_id,version_id,'extra',role,department_id,$3,agent_id,agent_revision_id \
        FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2")
        .bind(f.company().as_uuid()).bind(run.as_uuid()).bind(foreign.principal.as_uuid()).execute(f.p().pool()).await.unwrap_err();
    constraint(&error, "workflow_guard:guard_workflow_role_binding");
    let revision_fk: String = sqlx::query_scalar("SELECT conname FROM pg_constraint WHERE conrelid='workflow_run_role_bindings'::regclass AND contype='f' AND confrelid='agent_revisions'::regclass")
        .fetch_one(f.p().pool()).await.unwrap();
    let mut tx = f.p().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO workflow_run_role_bindings(company_id,run_id,manifest_id,version_id,step,role,department_id,principal_id,agent_id,agent_revision_id) \
        SELECT company_id,run_id,manifest_id,version_id,'extra',role,department_id,principal_id,agent_id,$3 \
        FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2")
        .bind(f.company().as_uuid()).bind(run.as_uuid()).bind(foreign.revision.as_uuid()).execute(&mut *tx).await.unwrap();
    let error = sqlx::query(&format!(
        "SET CONSTRAINTS \"{}\" IMMEDIATE",
        revision_fk.replace('"', "\"\"")
    ))
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some(revision_fk.as_str())
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        f.manifest(run).await.roles[0].principals[0].revision_id,
        Some(f.revision)
    );
}
