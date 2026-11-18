use super::*;

#[path = "memory_subject_visibility_tests.rs"]
mod subject_visibility;

#[path = "run_scope_seal_tests.rs"]
mod seals;

#[path = "run_scope_race_tests.rs"]
mod races;

async fn report(f: &RoleFixture) -> DepartmentId {
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .create_department(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            DepartmentWrite {
                name: "Report".into(),
                slug: "report".into(),
                parent_id: Some(f.department),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

async fn tuples(f: &RoleFixture, run: RunId) -> Value {
    sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(scope) ORDER BY scope.acting_principal_id,scope.access,scope.unit_id,scope.principal_id),'[]'::jsonb)
        FROM workflow_scope_tuples($1,$2) AS scope")
        .bind(f.company().as_uuid()).bind(run.as_uuid()).fetch_one(f.p().pool()).await.unwrap()
}

#[tokio::test]
async fn memory_scope_admission_company_wide_own_binding_refuses_activation() {
    use crate::application::workflow::lifecycle::LifecycleError;
    let mut source: Value =
        serde_json::from_str(&registry::example("memory.save").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["scope"] = json!({"literal":{"unit":"own"}});
    let binding = BindingFixture::from_source(source).await;
    let configured = binding
        .service()
        .configure(binding.request(None))
        .await
        .unwrap();
    let result = binding
        .service()
        .activate(SetBindingActivity {
            target: binding.target,
            expected: configured.revision,
        })
        .await;
    assert!(
        matches!(&result, Err(LifecycleError::Application(AppError::BadRequest(code)))
            if code.starts_with("binding.memory_scope_needs_desk")),
        "unexpected activation result: {:?}",
        result.as_ref().err()
    );
}

#[tokio::test]
async fn memory_scope_admission_ancestor_read_never_authorizes_a_root_save_effect() {
    use crate::application::workflow::lease::{WorkflowLeases, WorkflowWorkerId};
    use crate::application::workflow::memory::MemoryStepStore;
    let mut source: Value =
        serde_json::from_str(&registry::example("memory.save").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["scope"] = json!({"literal":{"unit":"root"}});
    let f = RoleFixture::from_binding(BindingFixture::from_source(source).await).await;
    let run = f.admitted("refuse-ancestor-save").await;
    let scope = super::super::child_support::entry(&f.admission, run).await;
    let claim = f
        .p()
        .claim_io(
            scope,
            WorkflowWorkerId(Uuid::new_v4()),
            crate::adapters::persistence::workflow::tests::message_tests::support::lease(),
        )
        .await
        .unwrap()
        .unwrap();
    let result = f.p().park_memory_save(&claim).await;
    assert!(matches!(result,Err(AppError::BadRequest(code)) if code=="memory.scope_unavailable"));
    let effects: i64 =
        sqlx::query_scalar("SELECT count(*) FROM memory_deliveries WHERE company_id=$1")
            .bind(f.company().as_uuid())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    assert_eq!(effects, 0);
}

async fn own_direct_binding(f: &RoleFixture) -> BindingFixture {
    use crate::application::memory::MemorySettingsStore;
    f.p()
        .enable_memory(
            f.company(),
            f.admission.binding.target.actor,
            0,
            Uuid::new_v4(),
        )
        .await
        .unwrap();
    let fixture = Fixture {
        _db: f.admission.binding.fixture._db.clone(),
        persistence: f.p().clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..f.admission.binding.fixture.target
        },
    };
    let mut source: Value =
        serde_json::from_str(&registry::example("memory.save").unwrap().source).unwrap();
    source["workflow_id"] = json!(fixture.target.workflow.to_string());
    source["steps"]["start"]["with"]["scope"] = json!({"literal":{"unit":"own"}});
    let service = DefinitionService::new(
        Preflight,
        f.p().clone(),
        WorkflowSourceDecoder,
        crate::adapters::persistence::workflow::step_builtins::StepBuiltinPublicationDirectory {
            persistence: f.p().clone(),
        },
    );
    let version = VersionId::new(Uuid::new_v4());
    fixture
        .publish_source(
            &service,
            fixture.target.workflow,
            source.to_string(),
            version,
        )
        .await
        .unwrap();
    let binding = BindingFixture::from_version(fixture, version);
    let mut configuration = binding.request(None);
    configuration.association = RelatedAssociation::Principal(f.principal);
    let configured = binding.service().configure(configuration).await.unwrap();
    binding
        .service()
        .activate(SetBindingActivity {
            target: binding.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    binding
}

#[tokio::test]
async fn memory_scope_admission_active_own_binding_refuses_zero_or_ambiguous_placement() {
    let f = RoleFixture::new().await;
    let _binding = own_direct_binding(&f).await;
    let other = report(&f).await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    let actor = f.admission.binding.target.actor.user_id();
    let error = departments
        .add_member(actor, f.company().as_uuid(), other, f.principal)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("binding.memory_scope_needs_desk"),
        "{error}"
    );
    let error = departments
        .clear_role(
            actor,
            f.company().as_uuid(),
            f.department,
            DepartmentRole::Lead,
            f.principal,
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("binding.memory_scope_needs_desk"),
        "{error}"
    );
    let units: Vec<Uuid> = sqlx::query_scalar(
        "SELECT department FROM workflow_target_departments($1,NULL,$2) AS department",
    )
    .bind(f.company().as_uuid())
    .bind(f.principal.as_uuid())
    .fetch_all(f.p().pool())
    .await
    .unwrap();
    assert_eq!(
        units,
        vec![f.department.as_uuid()],
        "both mutations roll back atomically"
    );
}

async fn other_desk(f: &RoleFixture) -> Uuid {
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    let unit = departments
        .create_department(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            DepartmentWrite {
                name: "Other".into(),
                slug: "other".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    departments
        .assign_role(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            unit,
            DepartmentRole::Lead,
            f.principal,
        )
        .await
        .unwrap();
    crate::adapters::persistence::test_support::create_channel(
        f.p(),
        f.company().as_uuid(),
        ChannelWrite {
            name: "Other intake".into(),
            slug: "other-intake".into(),
            department_id: Some(unit),
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn memory_scope_admission_same_agent_on_two_desks_keeps_their_local_units_separate() {
    use crate::application::workflow::memory::{MemoryStepScope, UnitSelector};
    let mut f = RoleFixture::new().await;
    let report = report(&f).await;
    let channel = other_desk(&f).await;
    let first = f.admitted("first-desk").await;
    let original = tuples(&f, first).await;
    f.admission.binding.target.binding = WorkflowBindingId::new(Uuid::new_v4());
    let mut configuration = f.admission.binding.request(None);
    configuration.association = RelatedAssociation::Channel(RelatedChannelId::new(channel));
    let configured = f
        .admission
        .binding
        .service()
        .configure(configuration)
        .await
        .unwrap();
    f.admission
        .binding
        .service()
        .activate(SetBindingActivity {
            target: f.admission.binding.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    f.channel = channel;
    let second = f.admitted("second-desk").await;
    let mut db = f.p().pool().acquire().await.unwrap();
    let first_rows = run_scopes::rows_on(&mut db, f.company(), first, Some(f.principal))
        .await
        .unwrap();
    let second_rows = run_scopes::rows_on(&mut db, f.company(), second, Some(f.principal))
        .await
        .unwrap();
    let own = |rows: &[run_scopes::ScopeRow]| {
        rows.iter()
            .find(|row| row.access == "write")
            .unwrap()
            .unit_id
    };
    assert_eq!(own(&first_rows), Some(f.department.as_uuid()));
    assert_ne!(own(&second_rows), own(&first_rows));
    assert!(
        !second_rows
            .iter()
            .any(|row| row.access == "read" && row.unit_id == Some(f.department.as_uuid()))
    );
    assert!(
        second_rows
            .iter()
            .any(|row| row.access == "read" && row.unit_id == Some(report.as_uuid()) && row.report)
    );
    let scope = super::super::child_support::entry(&f.admission, second).await;
    let root = crate::adapters::persistence::workflow::memory_steps::step_scope_access_on(
        &mut db,
        scope,
        MemoryStepScope::Unit(UnitSelector::Root),
    )
    .await
    .unwrap();
    assert!(!root.writable, "an ancestor read never grants a root write");
    assert_eq!(tuples(&f, first).await, original);
}

#[tokio::test]
async fn memory_scope_admission_freezes_per_actor_reports_and_retained_labels() {
    let f = RoleFixture::new().await;
    let report = report(&f).await;
    let old = f.admitted("before-report-change").await;
    let before = tuples(&f, old).await;
    let mut db = f.p().pool().acquire().await.unwrap();
    let actor = run_scopes::rows_on(&mut db, f.company(), old, Some(f.principal))
        .await
        .unwrap();
    assert!(actor.iter().any(|row| row.unit_id == Some(report.as_uuid())
        && row.report
        && row.unit_slug.as_deref() == Some("report")));
    let base = run_scopes::rows_on(&mut db, f.company(), old, None)
        .await
        .unwrap();
    assert!(!base.iter().any(|row| row.unit_id == Some(report.as_uuid())));
    assert!(
        actor
            .iter()
            .any(|row| row.unit_id == Some(f.department.as_uuid())
                && row.own
                && row.access == "write")
    );
    assert!(
        run_scopes::rows_on(
            &mut db,
            f.company(),
            old,
            Some(PrincipalId::new(Uuid::new_v4()))
        )
        .await
        .is_err()
    );
    drop(db);
    sqlx::query("UPDATE departments SET slug='renamed' WHERE company_id=$1 AND id=$2")
        .bind(f.company().as_uuid())
        .bind(report.as_uuid())
        .execute(f.p().pool())
        .await
        .unwrap();
    assert_eq!(tuples(&f, old).await, before);
    let fresh = f.admitted("after-report-change").await;
    assert_ne!(tuples(&f, fresh).await, before);
    assert!(
        tuples(&f, fresh)
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["unit_slug"] == "renamed" && row["report"] == true)
    );
    for desk in ChannelPersistence::list_by_company_id(f.p(), f.company().as_uuid())
        .await
        .unwrap()
        .into_iter()
        .filter(|desk| desk.department_id == report)
    {
        ChannelPersistence::delete(
            f.p(),
            crate::adapters::persistence::test_support::company_owner_actor(
                f.p(),
                f.company().as_uuid(),
            )
            .await,
            f.company().as_uuid(),
            desk.id,
        )
        .await
        .unwrap();
    }
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .delete_department(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            report,
        )
        .await
        .unwrap();
    assert_eq!(tuples(&f, old).await, before);
}

#[tokio::test]
async fn memory_scope_admission_seal_refuses_updates_deletes_late_rows_and_missing_or_foreign_authority()
 {
    let f = RoleFixture::new().await;
    let run = f.admitted("scope-immutable").await;
    let before = tuples(&f, run).await;
    for statement in [
        "UPDATE workflow_run_scopes SET report=true WHERE company_id=$1 AND run_id=$2",
        "DELETE FROM workflow_run_scopes WHERE company_id=$1 AND run_id=$2",
        "UPDATE workflow_run_scope_snapshots SET reason=reason WHERE company_id=$1 AND run_id=$2",
        "DELETE FROM workflow_run_scope_snapshots WHERE company_id=$1 AND run_id=$2",
        "INSERT INTO workflow_run_scopes(company_id,run_id,acting_principal_id,principal_id,access) VALUES($1,$2,NULL,gen_random_uuid(),'personal')",
    ] {
        assert!(
            sqlx::query(statement)
                .bind(f.company().as_uuid())
                .bind(run.as_uuid())
                .execute(f.p().pool())
                .await
                .is_err(),
            "{statement}"
        );
    }
    assert_eq!(tuples(&f, run).await, before);
    let mut db = f.p().pool().acquire().await.unwrap();
    assert!(
        run_scopes::require_available_on(&mut db, CompanyId::new(Uuid::new_v4()), run)
            .await
            .is_err()
    );
    assert!(
        run_scopes::require_available_on(&mut db, f.company(), RunId::new(Uuid::new_v4()))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn memory_scope_admission_competing_append_and_seal_share_the_run_mutex() {
    let f = RoleFixture::new().await;
    let run =
        super::super::storage_tests::autonomous_agent_storage_tests::native_runs::unsealed_root(
            &f.admission,
            "scope-race",
        )
        .await;
    let mut writer = f.p().pool().begin().await.unwrap();
    let root = crate::adapters::persistence::company::root_on(&mut writer, f.company())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO workflow_run_scopes(company_id,run_id,unit_id,access,unit_slug,ancestor)
        SELECT $1,$2,id,'read',slug::text,true FROM departments WHERE company_id=$1 AND id=$3",
    )
    .bind(f.company().as_uuid())
    .bind(run.run.as_uuid())
    .bind(root.as_uuid())
    .execute(&mut *writer)
    .await
    .unwrap();
    let result=sqlx::query("INSERT INTO workflow_run_scope_snapshots(company_id,run_id,state) VALUES($1,$2,'available')")
        .bind(f.company().as_uuid()).bind(run.run.as_uuid()).execute(f.p().pool()).await.unwrap_err();
    assert_eq!(
        result
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("55P03")
    );
    sqlx::query("INSERT INTO workflow_run_scope_snapshots(company_id,run_id,state) VALUES($1,$2,'available')")
        .bind(f.company().as_uuid()).bind(run.run.as_uuid()).execute(&mut *writer).await.unwrap();
    writer.commit().await.unwrap();
    assert!(
        sqlx::query(
            "INSERT INTO workflow_run_scopes(company_id,run_id,unit_id,access,unit_slug,ancestor)
        SELECT $1,$2,id,'read',slug::text,true FROM departments WHERE company_id=$1 AND id=$3"
        )
        .bind(f.company().as_uuid())
        .bind(run.run.as_uuid())
        .bind(root.as_uuid())
        .execute(f.p().pool())
        .await
        .is_err()
    );
    let seal: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_run_scope_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.company().as_uuid())
    .bind(run.run.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(seal, 1);
}
