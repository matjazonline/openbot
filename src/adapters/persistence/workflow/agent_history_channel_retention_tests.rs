use super::*;
use crate::adapters::persistence::workflow::agent_history_retention_fixture as clock;
use crate::application::workflow::{
    activation::{ActivationRequest, WorkflowJobId},
    agent_history::AgentExchangeId,
    agent_history_views::{HistoryActivity, HistoryPayloadState},
    run_views::{RunReadScope, RunViews},
    storage::WorkflowStorageMaintenance,
};

async fn scope(d: &Deployment, row: &Value) -> ActivationRequest {
    let company = d.fixture.target.company;
    let execution = ExecutionId::new(row["id"].as_str().unwrap().parse().unwrap());
    let job = sqlx::query_scalar(
        "SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2",
    )
    .bind(company.as_uuid())
    .bind(execution.as_uuid())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    ActivationRequest {
        company,
        execution,
        run: RunId::new(row["run"].as_str().unwrap().parse().unwrap()),
        job: WorkflowJobId(job),
    }
}

pub(super) async fn inspect_dead_delivery(d: &Deployment, row: &Value) {
    let source = scope(d, row).await;
    let read = RunReadScope {
        company: source.company,
        run: source.run,
        viewer: d.fixture.target.actor,
    };
    let detail = d
        .p()
        .agent_history_exchange(
            read,
            source.execution.as_uuid(),
            AgentExchangeId::for_execution(source.execution),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.entry.activity, Some(HistoryActivity::Produced));
    assert_eq!(detail.status.as_ref().unwrap().workflow, "succeeded");
    assert_eq!(
        detail.status.as_ref().unwrap().delivery.as_deref(),
        Some("dead_letter")
    );
    clock::expire(d.p(), source).await;
    let expired = d
        .p()
        .agent_history_exchange(
            read,
            source.execution.as_uuid(),
            AgentExchangeId::for_execution(source.execution),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(expired.entry.activity, Some(HistoryActivity::Produced));
    assert_eq!(expired.entry.payload, HistoryPayloadState::Expired);
    assert!(expired.payload.is_none());
    assert_eq!(
        expired.status.as_ref().unwrap().delivery.as_deref(),
        Some("dead_letter")
    );
}

#[tokio::test]
async fn agent_history_retention_real_pending_channel_delivery_pins_the_produced_payload_even_after_age()
 {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"produced and pending"}"#,
    )]))
    .await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    d.email(customer_email(&d, "history-pending-retention", None))
        .await;
    Box::pin(d.run_until(
        "published output awaiting real delivery owner",
        async || responses_published(&d, 1).await,
    ))
    .await;
    let rows = exchanges(&d).await;
    assert_eq!(rows.len(), 1);
    let source = scope(&d, &rows[0]).await;
    clock::age(d.p(), source).await;
    assert!(
        !d.p()
            .expire_history(clock::candidate(d.p(), source).await)
            .await
            .unwrap()
    );
    let detail = d
        .p()
        .agent_history_exchange(
            RunReadScope {
                company: source.company,
                run: source.run,
                viewer: d.fixture.target.actor,
            },
            source.execution.as_uuid(),
            AgentExchangeId::for_execution(source.execution),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.entry.activity, Some(HistoryActivity::Produced));
    assert_eq!(detail.entry.payload, HistoryPayloadState::Retained);
    assert_eq!(
        detail.status.as_ref().unwrap().delivery.as_deref(),
        Some("pending")
    );
    assert_eq!(
        detail.payload.as_ref().unwrap().output,
        Some(json!({"body":"produced and pending"}))
    );
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}
