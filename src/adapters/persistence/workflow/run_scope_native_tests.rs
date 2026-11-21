//! Actual workflow-origin native admission must use its sealed source projection.
use super::*;
use crate::application::memory::MemorySettingsStore;
use crate::application::ports::department::DepartmentWrite;
use crate::application::use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence};
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::entities::transport::PrincipalId;
use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};

struct NativeScopeSource {
    setup: Setup,
    native: NativeTurn,
    fixture: Fixture,
    binding: BindingTarget,
    principal: PrincipalId,
    other_report: DepartmentId,
}

async fn source_with_captured_harness() -> NativeScopeSource {
    let mut s = Box::pin(Setup::new()).await;
    CompanyPersistence::replace_model_connections_for_user(
        &s.fixture.persistence,
        s.fixture.target.actor.user_id(),
        s.fixture.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("native-scope-fixture-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    configure_with_agent(
        &mut s,
        "http://127.0.0.1:9",
        AgentWrite {
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            ..native_agent_write()
        },
    )
    .await;
    let fixture = Fixture {
        _db: s.fixture._db.clone(),
        persistence: s.fixture.persistence.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..s.fixture.target
        },
    };
    fixture
        .persistence
        .enable_memory(
            fixture.target.company,
            fixture.target.actor,
            0,
            Uuid::new_v4(),
        )
        .await
        .unwrap();
    let principal = crate::adapters::persistence::memory::agent_principal_on(
        &mut fixture.persistence.pool().acquire().await.unwrap(),
        fixture.target.company.as_uuid(),
        s.profile.agent_id.unwrap().as_uuid(),
    )
    .await
    .unwrap();
    let other_report = other_reader(&fixture).await;
    let mut source: Value = serde_json::from_str(&registry::example(KIND).unwrap().source).unwrap();
    source["workflow_id"] = json!(fixture.target.workflow.to_string());
    source["steps"]["start"]["with"] =
        json!({"profile_id":{"literal":s.profile.id},"prompt":{"literal":"native scope source"}});
    source["steps"]["start"]["routes"]["success"] = json!("later");
    // Actual later agent steps pin both acting principals. Neither executes:
    // the source remains parked on its first native operation.
    let model: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["steps"]["later"] = model["steps"]["start"].clone();
    source["steps"]["later"]["with"]["agent"] = json!({"literal":"workflow-native-mcp"});
    source["steps"]["later"]["routes"]["success"] = json!("other");
    source["steps"]["other"] = model["steps"]["start"].clone();
    source["steps"]["other"]["with"]["agent"] = json!({"literal":"other-reader"});
    let version = publish_scope_source(&fixture, source).await;
    let binding = bound(&fixture, version).await;
    s.scope = admitted_as(
        &fixture,
        binding,
        RelatedAssociation::Company,
        "captured-native-source",
    )
    .await;
    let claim = s.claim().await;
    s.park(&claim).await;
    let native = s.native_started().await;
    NativeScopeSource {
        setup: s,
        native,
        fixture,
        binding,
        principal,
        other_report,
    }
}

async fn publish_scope_source(f: &Fixture, source: Value) -> VersionId {
    let p = &f.persistence;
    let service = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        crate::adapters::persistence::workflow::agent_publication::AgentPublicationDirectory {
            persistence: p.clone(),
            inner: crate::adapters::persistence::workflow::step_builtins::StepBuiltinPublicationDirectory {
                persistence: p.clone(),
            },
        },
    );
    let version = VersionId::new(Uuid::new_v4());
    f.publish_source(&service, f.target.workflow, source.to_string(), version)
        .await
        .unwrap();
    version
}

async fn tuples(p: &PostgresPersistence, company: Uuid, run: RunId) -> Value {
    sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(scope) ORDER BY scope.acting_principal_id,scope.access,scope.unit_id,scope.principal_id),'[]'::jsonb)
        FROM workflow_scope_tuples($1,$2) AS scope")
        .bind(company).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap()
}

async fn place(f: &Fixture, principal: PrincipalId, slug: &str) -> DepartmentId {
    let departments =
        crate::adapters::persistence::test_support::department_use_cases(&f.persistence);
    let unit = departments
        .create_department(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            DepartmentWrite {
                name: slug.into(),
                slug: slug.into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    departments
        .assign_role(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            unit,
            crate::application::ports::department::DepartmentRoleAssignment {
                role: DepartmentRole::Lead,
                principal: principal,
                decider: false,
            },
        )
        .await
        .unwrap();
    unit
}

async fn member_placement(f: &Fixture, principal: PrincipalId) -> DepartmentId {
    let departments =
        crate::adapters::persistence::test_support::department_use_cases(&f.persistence);
    let unit = departments
        .create_department(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            DepartmentWrite {
                name: "Member placement".into(),
                slug: "member-placement".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    departments
        .add_member(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            unit,
            principal,
        )
        .await
        .unwrap();
    unit
}

async fn other_reader(f: &Fixture) -> DepartmentId {
    let agent = AgentPersistence::create(
        &f.persistence,
        f.target.company.as_uuid(),
        AgentWrite {
            name: "Other reader".into(),
            slug: "other-reader".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    f.publish_agent(agent.id).await;
    let principal = crate::adapters::persistence::memory::agent_principal_on(
        &mut f.persistence.pool().acquire().await.unwrap(),
        f.target.company.as_uuid(),
        agent.id,
    )
    .await
    .unwrap();
    let parent = place(f, principal, "other-team").await;
    crate::adapters::persistence::test_support::department_use_cases(&f.persistence)
        .create_department(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            DepartmentWrite {
                name: "Other report".into(),
                slug: "other-report".into(),
                parent_id: Some(parent),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

async fn fresh(source: &NativeScopeSource, key: &str) -> Value {
    let admitted = admitted_as(
        &source.fixture,
        source.binding,
        RelatedAssociation::Principal(source.principal),
        key,
    )
    .await;
    tuples(
        &source.fixture.persistence,
        source.fixture.target.company.as_uuid(),
        admitted.run,
    )
    .await
}

async fn assert_fresh_placement_matrix(source: &NativeScopeSource, unit: DepartmentId) {
    let f = &source.fixture;
    let departments =
        crate::adapters::persistence::test_support::department_use_cases(&f.persistence);
    let report = departments
        .create_department(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            DepartmentWrite {
                name: "Member report".into(),
                slug: "member-report".into(),
                parent_id: Some(unit),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    let singleton = fresh(source, "fresh-single-placement").await;
    let actor_rows = |rows: &Value| {
        rows.as_array()
            .unwrap()
            .iter()
            .filter(|row| row["acting_principal_id"] == json!(source.principal.as_uuid()))
            .cloned()
            .collect::<Vec<_>>()
    };
    let member = actor_rows(&singleton);
    assert!(
        member
            .iter()
            .any(|row| row["unit_id"] == json!(unit.as_uuid()) && row["access"] == "write")
    );
    assert!(
        !member
            .iter()
            .any(|row| row["unit_id"] == json!(report.as_uuid())),
        "membership alone grants no child reports"
    );
    departments
        .assign_role(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            unit,
            crate::application::ports::department::DepartmentRoleAssignment {
                role: DepartmentRole::Lead,
                principal: source.principal,
                decider: false,
            },
        )
        .await
        .unwrap();
    let lead = actor_rows(&fresh(source, "fresh-lead-placement").await);
    assert!(
        lead.iter()
            .any(|row| row["unit_id"] == json!(report.as_uuid()) && row["report"] == true)
    );
    place(f, source.principal, "ambiguous-placement").await;
    let ambiguous = actor_rows(&fresh(source, "fresh-ambiguous-placement").await);
    let root = crate::adapters::persistence::company::root_on(
        &mut f.persistence.pool().acquire().await.unwrap(),
        f.target.company,
    )
    .await
    .unwrap();
    let reads: Vec<_> = ambiguous
        .iter()
        .filter(|row| row["access"] == "read")
        .collect();
    assert_eq!(reads.len(), 1, "ambiguous no-desk scope is root only");
    assert_eq!(reads[0]["unit_id"], json!(root.as_uuid()));
    assert!(ambiguous.iter().all(|row| row["access"] != "write"));
}

#[tokio::test]
async fn memory_scope_admission_native_workflow_keeps_the_source_projection_after_placement_changes()
 {
    let source = Box::pin(source_with_captured_harness()).await;
    let p = &source.fixture.persistence;
    let company = source.fixture.target.company;
    let agent = source.setup.profile.agent_id.unwrap();
    let expected: Value = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(scope) ORDER BY scope.acting_principal_id,scope.access,scope.unit_id,scope.principal_id),'[]'::jsonb)
        FROM workflow_native_scope_tuples($1,$2,$3,$4) AS scope")
        .bind(company.as_uuid()).bind(source.setup.scope.run.as_uuid()).bind(source.principal.as_uuid())
        .bind(source.fixture.target.actor.user_id()).fetch_one(p.pool()).await.unwrap();
    let rows = expected.as_array().unwrap();
    assert!(rows.iter().any(
        |row| row["acting_principal_id"] == json!(source.principal.as_uuid())
            && row["access"] == "personal"
    ));
    assert!(
        rows.iter()
            .any(|row| row["acting_principal_id"].is_null() && row["access"] == "personal")
    );
    assert!(
        !rows
            .iter()
            .any(|row| row["unit_id"] == json!(source.other_report.as_uuid()))
    );
    assert!(
        tuples(p, company.as_uuid(), source.setup.scope.run)
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["unit_id"] == json!(source.other_report.as_uuid())
                && row["report"] == true)
    );
    let unit = member_placement(&source.fixture, source.principal).await;
    let broker = p
        .native_mcp(&request(&source.setup, &source.native))
        .await
        .unwrap();
    assert_eq!(tuples(p, company.as_uuid(), broker.run_id).await, expected);
    let mut db = p.pool().acquire().await.unwrap();
    let scopes = crate::adapters::persistence::workflow::checkpoints::memory_tools::eligible_on(
        &mut db,
        company,
        broker.run_id,
        agent.as_uuid(),
    )
    .await
    .unwrap();
    assert!(
        scopes.iter().any(|scope| scope.private_person),
        "the broker keeps genuine source own-session proof"
    );
    assert!(
        !scopes
            .iter()
            .any(|scope| scope.owner == crate::application::memory::NamespaceOwner::Unit(unit))
    );
    drop(db);
    assert_fresh_placement_matrix(&source, unit).await;
    assert_eq!(tuples(p, company.as_uuid(), broker.run_id).await, expected);
    let claim = Box::pin(broker_claim(&source.setup, broker.run_id)).await;
    assert!(
        p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into()))
            .await
            .is_ok()
    );
}
