//! Historical scopes survive actual parent-to-child admission after topology changes.
use super::*;
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::department::DepartmentWrite;
use crate::entities::department::{DepartmentId, DepartmentRole};

async fn report_unit(setup: &AgentSetup) -> DepartmentId {
    let f = &setup.fixture;
    let company = f.target.company;
    let departments =
        crate::adapters::persistence::test_support::department_use_cases(&f.persistence);
    let parent = departments
        .create_department(
            f.target.actor.user_id(),
            company.as_uuid(),
            DepartmentWrite {
                name: "Child team".into(),
                slug: "child-team".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    let principal = crate::adapters::persistence::memory::agent_principal_on(
        &mut f.persistence.pool().acquire().await.unwrap(),
        company.as_uuid(),
        setup.agent,
    )
    .await
    .unwrap();
    departments
        .assign_role(
            f.target.actor.user_id(),
            company.as_uuid(),
            parent,
            DepartmentRole::Lead,
            principal,
        )
        .await
        .unwrap();
    departments
        .create_department(
            f.target.actor.user_id(),
            company.as_uuid(),
            DepartmentWrite {
                name: "Child report".into(),
                slug: "child-report".into(),
                parent_id: Some(parent),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

async fn tuples(setup: &AgentSetup, run: RunId) -> Value {
    sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(scope) ORDER BY scope.acting_principal_id,scope.access,scope.unit_id,scope.principal_id),'[]'::jsonb)
        FROM workflow_scope_tuples($1,$2) AS scope")
        .bind(setup.fixture.target.company.as_uuid()).bind(run.as_uuid())
        .fetch_one(setup.fixture.persistence.pool()).await.unwrap()
}

async fn delete_report(setup: &AgentSetup, report: DepartmentId) {
    let f = &setup.fixture;
    for desk in ChannelPersistence::list_by_company_id(&f.persistence, f.target.company.as_uuid())
        .await
        .unwrap()
        .into_iter()
        .filter(|desk| desk.department_id == report)
    {
        ChannelPersistence::delete(&f.persistence, desk.id)
            .await
            .unwrap();
    }
    crate::adapters::persistence::test_support::department_use_cases(&f.persistence)
        .delete_department(f.target.actor.user_id(), f.target.company.as_uuid(), report)
        .await
        .unwrap();
}

#[tokio::test]
async fn memory_scope_admission_real_child_keeps_historical_report_ids_and_labels_after_rename_or_delete()
 {
    for change in ["rename", "delete"] {
        let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
        let report = report_unit(&setup).await;
        let graph = graph(&setup).await;
        let AdmissionResult::Created(run) = admit_graph(&setup, &graph, "historical-scope-child")
            .await
            .unwrap()
        else {
            panic!("new source root")
        };
        let expected = tuples(&setup, run).await;
        assert!(
            expected
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["unit_id"] == json!(report.as_uuid())
                    && row["report"] == true
                    && row["unit_slug"] == "child-report")
        );
        if change == "rename" {
            sqlx::query(
                "UPDATE departments SET slug='renamed-report' WHERE company_id=$1 AND id=$2",
            )
            .bind(setup.fixture.target.company.as_uuid())
            .bind(report.as_uuid())
            .execute(setup.fixture.persistence.pool())
            .await
            .unwrap();
        } else {
            delete_report(&setup, report).await;
        }
        let llm = scripted_llm(vec![LlmTurn::text("\"parent accepted\"")]).await;
        register_scripted_agent_base_url(setup.agent, &llm.base_url);
        let branch = Box::pin(begin_run(&setup, run, AccessLevel::PUBLIC)).await;
        assert_eq!(llm.finish().await.unwrap(), 1);
        assert_eq!(
            tuples(&setup, branch.child.fence.scope.run).await,
            expected,
            "{change}"
        );
        if change == "delete" {
            let p = &setup.fixture.persistence;
            let result = crate::adapters::persistence::workflow::memory_steps::namespace_on(
                p,
                &mut p.pool().acquire().await.unwrap(),
                setup.fixture.target.company,
                crate::application::memory::NamespaceOwner::Unit(report),
            )
            .await;
            assert!(
                matches!(result,Err(AppError::BadRequest(code)) if code=="memory.scope_unavailable")
            );
        }
    }
}

#[tokio::test]
async fn memory_scope_admission_child_seal_rejects_a_corrupted_copy_and_rolls_back_before_retry() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let graph = graph(&setup).await;
    let AdmissionResult::Created(run) = admit_graph(&setup, &graph, "forged-scope-child")
        .await
        .unwrap()
    else {
        panic!("new source root")
    };
    let expected = tuples(&setup, run).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"parent accepted\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let parent = Box::pin(complete_parent_to_child(&setup, run, AccessLevel::PUBLIC)).await;
    assert_eq!(llm.finish().await.unwrap(), 1);
    let p = &setup.fixture.persistence;
    // This isolated fixture corrupts the actual producer's INSERT before sealing;
    // production guards remain enabled and must reject the altered historical tuple.
    sqlx::raw_sql("CREATE FUNCTION test_forge_child_scope() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN IF NEW.unit_id IS NOT NULL AND EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=NEW.company_id
            AND id=NEW.run_id AND parent_run_id IS NOT NULL) THEN NEW.unit_slug:='forged-copy'; END IF; RETURN NEW; END $$;
        CREATE TRIGGER test_forge_child_scope BEFORE INSERT ON workflow_run_scopes
            FOR EACH ROW EXECUTE FUNCTION test_forge_child_scope();")
        .execute(p.pool()).await.unwrap();
    let error = p.park_child(parent).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("child scopes must exactly copy its sealed parent"),
        "{error}"
    );
    let children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2",
    )
    .bind(parent.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(children, 0, "failed sealing leaves no partial child");
    assert_eq!(tuples(&setup, run).await, expected);
    sqlx::raw_sql("DROP TRIGGER test_forge_child_scope ON workflow_run_scopes; DROP FUNCTION test_forge_child_scope();")
        .execute(p.pool()).await.unwrap();
    assert!(p.park_child(parent).await.unwrap().is_some());
    let child: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2")
            .bind(parent.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(tuples(&setup, RunId::new(child)).await, expected);
}
