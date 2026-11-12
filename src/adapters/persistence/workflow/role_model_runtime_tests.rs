use super::*;
use crate::application::workflow::reads::WorkflowReads;
use crate::application::workflow::run_views::{RunReadScope, RunViews};

#[tokio::test]
async fn workflow_role_admission_model_only_classification_and_decision_execute_without_tools_or_history()
 {
    for (kind, response) in [
        ("ai.classify", r#""support""#),
        (
            "decision.agent",
            r#"{"choice":"continue","data":"approved"}"#,
        ),
    ] {
        let mut d = Deployment::new(vec![LlmTurn::text(response)]).await;
        let a = agent(&d, "role-model").await;
        let desk = desk(&d, "model-team", &a).await;
        d.channel = desk.id;
        let binding = d.bind(source(kind), None).await;
        let run = admit(&d, binding, &desk, "model-only", Uuid::new_v4()).await;
        Box::pin(
            d.run_until("the role-selected model-only step completed", async || {
                settled(&d, run).await
            }),
        )
        .await;
        let requests = d.llm.observed();
        assert_eq!(requests.len(), 1);
        assert!(system_text(&requests[0]).contains("ROLE_INSTRUCTIONS_role-model"));
        assert!(
            requests[0]
                .get("tools")
                .is_none_or(|tools| tools.is_null() || tools.as_array().is_some_and(Vec::is_empty)),
            "{:?}",
            requests[0]
        );
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2",
        )
        .bind(binding.company.as_uuid())
        .bind(run.as_uuid())
        .fetch_one(d.p().pool())
        .await
        .unwrap();
        assert_eq!(
            count, 0,
            "role model-only steps allocate no agent conversation"
        );
        assert_inspection(&d, binding, run, &a).await;
        assert_eq!(d.llm.finish().await.unwrap(), 1);
    }
}

async fn assert_inspection(d: &Deployment, binding: BindingTarget, run: RunId, a: &RoleAgent) {
    let summary = d
        .p()
        .run_summary(binding.company, run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(summary.role_holders[0].principal, a.principal);
    let scope = RunReadScope {
        company: binding.company,
        run,
        viewer: binding.actor,
    };
    let execution: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND step_id='start'",
    )
    .bind(binding.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    let mut view = d.p().step_view(scope, execution).await.unwrap().unwrap();
    let selected = view.agent.as_ref().expect("role-selected agent is visible");
    assert_eq!(selected.via_role.as_ref().unwrap().principal, a.principal);
    assert!(selected.tools.is_empty());
    assert!(selected.skills.is_empty());
    let restricted = crate::application::knowledge::PayloadVisibility {
        viewer: crate::entities::access_level::AccessLevel::PUBLIC,
        content: crate::entities::access_level::AccessLevel::MAX,
    };
    view.project(restricted);
    assert!(view.agent.is_none());
    let mut run = d.p().run_view(scope).await.unwrap().unwrap();
    assert_eq!(run.detail.summary.role_holders[0].principal, a.principal);
    run.project(restricted);
    assert!(run.detail.summary.role_holders.is_empty());
    assert!(!run.detail.summary.role_holders_truncated);
    assert!(run.dependencies.is_none());
}
