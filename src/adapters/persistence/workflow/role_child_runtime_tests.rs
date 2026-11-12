use super::*;
use crate::application::workflow::reads::{PageRequest, RunFilter, RunLineage, WorkflowReads};
use crate::application::workflow::run_views::{RunReadScope, RunViews};

#[tokio::test]
async fn workflow_role_admission_pinned_child_reused_step_ids_execute_their_own_frozen_selection() {
    let mut d = Deployment::new(vec![
        LlmTurn::text(r#""root""#),
        LlmTurn::text(r#""child""#),
    ])
    .await;
    let lead = agent(&d, "child-lead").await;
    let reviewer = agent(&d, "root-reviewer").await;
    let desk = desk(&d, "nested-team", &lead).await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(d.p());
    departments
        .assign_role(
            d.fixture.target.actor.user_id(),
            d.fixture.target.company.as_uuid(),
            desk.department,
            DepartmentRole::Reviewer,
            reviewer.principal,
        )
        .await
        .unwrap();
    let child_workflow = WorkflowId::new(Uuid::new_v4());
    let child_version = d
        .fixture
        .publish_workflow(child_workflow, source("agent.run"))
        .await;
    let mut root = source("agent.run");
    root["steps"]["start"]["with"]["role"] = json!({"literal":"reviewer"});
    root["steps"]["start"]["routes"]["success"] = json!("child");
    root["steps"]["child"] = json!({"type":"workflow.call","child_workflow_id":child_workflow.to_string(),
        "child_version_id":child_version.to_string(),"with":{"input":{"literal":{}},"params":{"literal":{}}},
        "routes":{"success":"$end"}});
    d.channel = desk.id;
    let binding = d.bind(root, None).await;
    let run = admit(&d, binding, &desk, "nested", Uuid::new_v4()).await;
    departments
        .assign_role(
            binding.actor.user_id(),
            binding.company.as_uuid(),
            desk.department,
            DepartmentRole::Lead,
            reviewer.principal,
        )
        .await
        .unwrap();
    Box::pin(
        d.run_until("the root and frozen child completed", async || {
            settled(&d, run).await
        }),
    )
    .await;
    let child: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2")
            .bind(binding.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    assert_step_selection(&d, binding, run, &reviewer).await;
    assert_step_selection(&d, binding, RunId::new(child), &lead).await;
    let selections: Vec<(Uuid, String)> = sqlx::query_as("SELECT version_id,step FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 ORDER BY version_id")
        .bind(binding.company.as_uuid()).bind(run.as_uuid()).fetch_all(d.p().pool()).await.unwrap();
    assert_eq!(selections.len(), 2);
    assert_ne!(selections[0].0, selections[1].0);
    assert!(selections.iter().all(|row| row.1 == "start"));
    assert_role_agent_filter(&d, binding, &lead, run, RunId::new(child)).await;
    let requests = d.llm.observed();
    assert_eq!(requests.len(), 2);
    assert!(system_text(&requests[0]).contains("ROLE_INSTRUCTIONS_root-reviewer"));
    assert!(system_text(&requests[1]).contains("ROLE_INSTRUCTIONS_child-lead"));
    assert_eq!(d.llm.finish().await.unwrap(), 2);
}

async fn assert_step_selection(
    d: &Deployment,
    binding: BindingTarget,
    run: RunId,
    expected: &RoleAgent,
) {
    let execution: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND step_id='start'",
    )
    .bind(binding.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    let view = d
        .p()
        .step_view(
            RunReadScope {
                company: binding.company,
                run,
                viewer: binding.actor,
            },
            execution,
        )
        .await
        .unwrap()
        .unwrap();
    let selected = view.agent.unwrap();
    assert_eq!(selected.agent_id.as_uuid(), expected.id);
    assert_eq!(selected.revision_id.as_uuid(), expected.revision);
    assert_eq!(selected.via_role.unwrap().principal, expected.principal);
    let executed: Uuid = sqlx::query_scalar(
        "SELECT agent_id FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2",
    )
    .bind(binding.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(executed, expected.id);
}

async fn assert_role_agent_filter(
    d: &Deployment,
    binding: BindingTarget,
    agent: &RoleAgent,
    root: RunId,
    child: RunId,
) {
    let filter = RunFilter {
        agent: Some(agent.id),
        lineage: RunLineage::Any,
        ..Default::default()
    };
    let page = d
        .p()
        .list_runs(
            binding.company,
            &filter,
            PageRequest::new(None, Some(1)).unwrap(),
        )
        .await
        .unwrap();
    assert!(page.more);
    assert_eq!(page.items.len(), 1);
    let first = page.items[0].run;
    let next = d
        .p()
        .list_runs(
            binding.company,
            &filter,
            PageRequest::new(Some(first.as_uuid()), Some(1)).unwrap(),
        )
        .await
        .unwrap();
    assert!(!next.more);
    assert_eq!(next.items.len(), 1);
    let mut ids = vec![first, next.items[0].run];
    ids.sort();
    let mut expected = vec![root, child];
    expected.sort();
    assert_eq!(
        ids, expected,
        "children inherit the root's frozen agent footprint for filtering"
    );
    assert!(page.items[0].role_holders.len() == 2 && next.items[0].role_holders.len() == 2);
    let empty = d
        .p()
        .list_runs(
            CompanyId::new(Uuid::new_v4()),
            &filter,
            PageRequest::new(None, Some(1)).unwrap(),
        )
        .await
        .unwrap();
    assert!(
        empty.items.is_empty(),
        "role filtering remains tenant scoped"
    );
    assert_role_board(d, binding, agent, root).await;
}

async fn assert_role_board(d: &Deployment, binding: BindingTarget, agent: &RoleAgent, root: RunId) {
    let scope = crate::application::workflow::board::RunBoardScope {
        agent: Some(agent.id),
        ..Default::default()
    };
    let cutoff = chrono::Utc::now() - chrono::Duration::days(7);
    let counts = d
        .p()
        .run_board_counts(binding.company, &scope, cutoff)
        .await
        .unwrap();
    assert_eq!(
        counts.iter().map(|count| count.runs).sum::<u64>(),
        1,
        "the role board counts its root once"
    );
    let visible = crate::application::workflow::board::VisibleChannels {
        channels: vec![d.channel],
        company: true,
    };
    let cards = d
        .p()
        .run_board_cards(binding.company, &scope, cutoff, &visible)
        .await
        .unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].run, root);
    assert_eq!(cards[0].role_holders.len(), 2);
}
