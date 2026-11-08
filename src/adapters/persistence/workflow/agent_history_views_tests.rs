use super::*;
use crate::adapters::http::pages::{HistoryAnchor, agent_history_detail, agent_history_section};
use crate::adapters::persistence::workflow::{
    agent_history_retention_fixture as clock, agent_history_views::testing,
};
use crate::application::workflow::{
    agent_history_views::*,
    run_views::{RunReadScope, RunViews},
};
use crate::entities::access_level::AccessLevel;

fn scope(f: &ManualHistory, claim: &ClaimedWorkflow, viewer: WorkflowActor) -> RunReadScope {
    RunReadScope {
        company: f.f.s.fixture.target.company,
        run: claim.fence.scope.run,
        viewer,
    }
}
fn anchor(f: &ManualHistory, claim: &ClaimedWorkflow) -> HistoryAnchor {
    HistoryAnchor {
        company: f.f.s.fixture.target.company.as_uuid(),
        run: claim.fence.scope.run.as_uuid(),
        execution: claim.fence.scope.execution.as_uuid(),
    }
}
fn id(claim: &ClaimedWorkflow) -> AgentExchangeId {
    AgentExchangeId::for_execution(claim.fence.scope.execution)
}

async fn detail(
    f: &ManualHistory,
    claim: &ClaimedWorkflow,
    viewer: WorkflowActor,
) -> HistoryDetail {
    f.f.s
        .p()
        .agent_history_exchange(
            scope(f, claim, viewer),
            claim.fence.scope.execution.as_uuid(),
            id(claim),
        )
        .await
        .unwrap()
        .unwrap()
}
async fn admin(f: &ManualHistory, level: i16) -> WorkflowActor {
    let user = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &f.f.s.fixture,
        "admin",
    )
    .await;
    sqlx::query("UPDATE company_members SET access_level=$3 WHERE company_id=$1 AND user_id=$2")
        .bind(f.f.s.fixture.target.company.as_uuid())
        .bind(user)
        .bind(level)
        .execute(f.f.s.pool())
        .await
        .unwrap();
    WorkflowActor::authenticated(user).unwrap()
}

struct PagedHistory {
    newest: HistoryPage,
    older: HistoryPage,
}
async fn assert_pagination(f: &ManualHistory, claims: &[ClaimedWorkflow]) -> PagedHistory {
    let own = &claims[17];
    let p = f.f.s.p();
    let owner = f.f.s.owner();
    let read_scope = scope(f, own, owner);
    let page = p
        .agent_history_page(
            read_scope,
            own.fence.scope.execution.as_uuid(),
            HistoryPageRequest::new(None, None).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(page.entries.len(), 16);
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.order)
            .collect::<Vec<_>>(),
        (3..=18).rev().collect::<Vec<_>>()
    );
    let older = p
        .agent_history_page(
            read_scope,
            own.fence.scope.execution.as_uuid(),
            HistoryPageRequest::new(Some(page.next.as_ref().unwrap().as_str().into()), None)
                .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        older
            .entries
            .iter()
            .map(|entry| entry.order)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert!(older.next.is_none());
    let source = p
        .agent_history_exchange(
            read_scope,
            own.fence.scope.execution.as_uuid(),
            id(&claims[1]),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(source.entry.order, 2);
    assert_eq!(source.payload.as_ref().unwrap().output, Some(json!(1)));
    browser_fixture::write_fragment(anchor(f, own), "selected-outside-page.html", &source);
    PagedHistory {
        newest: page,
        older,
    }
}
async fn assert_frozen_copy(f: &ManualHistory, claims: &[ClaimedWorkflow]) -> HistoryDetail {
    let own = &claims[17];
    let p = f.f.s.p();
    let owner = f.f.s.owner();
    let before = detail(f, own, owner).await;
    let frozen_before =
        serde_json::to_value(before.payload.as_ref().unwrap().frozen.as_ref().unwrap()).unwrap();
    assert_eq!(before.payload.as_ref().unwrap().output, Some(Value::Null));
    assert_eq!(
        frozen_before["history"]["selected"]
            .as_array()
            .unwrap()
            .len(),
        16
    );
    assert_eq!(
        frozen_before["history"]["selected"][0]["id"],
        json!(id(&claims[1]))
    );
    let before_html = agent_history_detail(anchor(f, own), &before);
    assert!(
        before_html.contains(&format!("&amp;exchange={}", id(&claims[1]).as_uuid())),
        "selected source outside the current page has an exact native history action"
    );
    browser_fixture::write_fragment(anchor(f, own), "current-before-expiry.html", &before);
    clock::expire(p, claims[1].fence.scope).await;
    let after = detail(f, own, owner).await;
    let payload = after.payload.as_ref().unwrap();
    assert_eq!(
        serde_json::to_value(payload.frozen.as_ref().unwrap()).unwrap(),
        frozen_before
    );
    assert!(payload.selected_sources.iter().any(|source|matches!(source,HistorySelectedSource::Expired{exchange} if *exchange==id(&claims[1]))));
    assert!(matches!(
        after.entry.activity,
        Some(HistoryActivity::Produced)
    ));
    assert_eq!(after.status.as_ref().unwrap().workflow, "cancelled");
    let html = agent_history_detail(anchor(f, own), &after);
    assert!(!html.contains("<script>history-secret"));
    assert!(html.contains("&lt;/script&gt;"));
    assert!(html.contains("source payload expired; frozen copy retained"));
    after
}
async fn assert_api_snapshot(f: &ManualHistory, own: &ClaimedWorkflow, after: &HistoryDetail) {
    let p = f.f.s.p();
    let read_scope = scope(f, own, f.f.s.owner());
    let frozen_before =
        serde_json::to_value(after.payload.as_ref().unwrap().frozen.as_ref().unwrap()).unwrap();
    let engine = crate::infra::workflow::WorkflowEngine::for_tests(p.clone()).services();
    let response = crate::adapters::http::routes::workflows::history_test_request(
        engine.clone(),
        read_scope,
        own.fence.scope.execution.as_uuid(),
        &format!("exchange={}", id(own).as_uuid()),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["entry"]["activity"], "produced");
    assert_eq!(body["payload"]["frozen"], frozen_before);
    let bad = crate::adapters::http::routes::workflows::history_test_request(
        engine,
        read_scope,
        own.fence.scope.execution.as_uuid(),
        "limit=17",
    )
    .await;
    assert_eq!(bad.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn agent_history_views_maximum_page_reaches_last_source_outside_page_and_keeps_exact_frozen_copy_after_expiry()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(
        (0..18)
            .map(|index| {
                LlmTurn::text(if index == 17 {
                    "null".into()
                } else {
                    index.to_string()
                })
            })
            .collect(),
    )
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let mut claims = Vec::new();
    for index in 0..18 {
        let output = if index == 17 {
            Value::Null
        } else {
            json!(index)
        };
        claims.push(
            f.complete(
                json!({"index":index,"text":"</script><script>history-secret</script>"}),
                output,
            )
            .await,
        );
    }
    let own = &claims[17];
    let pages = assert_pagination(&f, &claims).await;
    let after = assert_frozen_copy(&f, &claims).await;
    assert_api_snapshot(&f, own, &after).await;
    assert!(agent_history_section(anchor(&f, own), &pages.newest).contains("Older exchanges"));
    let first_detail = detail(&f, &claims[0], f.f.s.owner()).await;
    browser_fixture::write(
        anchor(&f, own),
        &pages.newest,
        &pages.older,
        &after,
        &first_detail,
    );
    assert_eq!(
        llm.finish().await.unwrap(),
        18,
        "inspection never enters the model"
    );
}

#[path = "agent_history_views_authority_tests.rs"]
mod authority;
#[path = "agent_history_browser_fixture.rs"]
mod browser_fixture;
