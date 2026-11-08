//! One actual thread: private search, public CC, restored audience, and current inspection.
use super::*;
use crate::application::workflow::{
    WorkflowActor,
    agent_history::AgentExchangeId,
    agent_history_views::HistoryPayloadState,
    run_views::{RunReadScope, RunViews},
};

#[derive(sqlx::FromRow)]
struct Exchange {
    id: Uuid,
    run_id: Uuid,
    execution_id: Uuid,
    conversation_id: Uuid,
    position: i64,
    access_level: i16,
    input: Value,
    output: Value,
    frozen_preparation: Value,
}

async fn exchanges(j: &SalesJourney) -> Vec<Exchange> {
    sqlx::query_as("SELECT exchange.id,exchange.run_id,exchange.execution_id,exchange.conversation_id,exchange.position,exchange.access_level::smallint AS access_level,execution.frozen_inputs->'context' AS input,exchange.output,exchange.frozen_preparation FROM agent_history_exchanges AS exchange JOIN workflow_executions AS execution ON execution.company_id=exchange.company_id AND execution.id=exchange.execution_id WHERE exchange.company_id=$1 ORDER BY exchange.position LIMIT 4")
        .bind(j.d.fixture.target.company.as_uuid()).fetch_all(j.d.p().pool()).await.unwrap()
}

enum Turn {
    Private,
    Public,
    Restored,
}
fn incoming(
    j: &SalesJourney,
    member: &crate::entities::user::Viewer,
    turn: Turn,
) -> RawInboundPayload {
    let (key, body) = match turn {
        Turn::Private => (
            "p07-private",
            "P07_FLOOR_QUERY: What is the AX-4410 discount floor?",
        ),
        Turn::Public => (
            "p07-public",
            "P07_PUBLIC_QUERY: Summarise this thread for the customer.",
        ),
        Turn::Restored => (
            "p07-restored",
            "P07_RESTORED_QUERY: What was the private floor?",
        ),
    };
    let mut message = email(&j.d, member.email.as_str(), key);
    message.text = Some(body.into());
    if !matches!(turn, Turn::Private) {
        message.headers = Some(format!(
            "Message-ID: <{key}@partner.test>\nIn-Reply-To: <p07-private@partner.test>\n"
        ));
    }
    if matches!(turn, Turn::Public) {
        message.cc = Some(CUSTOMER.into());
    }
    message
}

async fn remove_customer(j: &SalesJourney, thread: Uuid) {
    assert_eq!(sqlx::query("DELETE FROM thread_principals AS participant USING participant_identities AS identity WHERE participant.company_id=$1 AND participant.channel_id=$2 AND participant.thread_id=$3 AND identity.company_id=participant.company_id AND identity.principal_id=participant.principal_id AND identity.transport='email' AND identity.subject=$4")
        .bind(j.d.fixture.target.company.as_uuid()).bind(j.d.channel).bind(thread).bind(CUSTOMER)
        .execute(j.d.p().pool()).await.unwrap().rows_affected(), 1);
}

async fn restricted_inspection(j: &SalesJourney, row: &Exchange, lower: WorkflowActor) {
    let scope = RunReadScope {
        company: j.d.fixture.target.company,
        run: RunId::new(row.run_id),
        viewer: lower,
    };
    let detail =
        j.d.p()
            .agent_history_exchange(
                scope,
                row.execution_id,
                AgentExchangeId::for_execution(ExecutionId::new(row.execution_id)),
            )
            .await
            .unwrap()
            .unwrap();
    assert_eq!(detail.entry.payload, HistoryPayloadState::Restricted);
    assert!(detail.payload.is_none() && detail.status.is_none() && detail.entry.exchange.is_none());
    let html = crate::adapters::http::pages::agent_history_detail(
        crate::adapters::http::pages::HistoryAnchor {
            company: scope.company.as_uuid(),
            run: row.run_id,
            execution: row.execution_id,
        },
        &detail,
    );
    assert!(html.contains("Restricted exchange (completed)"));
    assert!(!html.contains(PRIVATE) && !html.contains("P07_FLOOR_QUERY"));
}

fn restored_context(rows: &[Exchange], request: &Value) {
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter().map(|row| row.position).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(
        rows.iter()
            .all(|row| row.conversation_id == rows[0].conversation_id)
    );
    assert_eq!(
        rows.iter().map(|row| row.access_level).collect::<Vec<_>>(),
        vec![5, 0, 5]
    );
    assert_eq!(
        rows[2].frozen_preparation["history"]["seed"],
        json!([
            {"input":rows[0].input,"output":rows[0].output},
            {"input":rows[1].input,"output":rows[1].output},
        ])
    );
    let actual = request.to_string();
    for marker in [
        "P07_FLOOR_QUERY",
        PRIVATE,
        "P07_PUBLIC_QUERY",
        "P07_RESTORED_QUERY",
    ] {
        assert_eq!(actual.matches(marker).count(), 1, "{marker}");
    }
    assert_ne!(rows[0].id, rows[1].id);
    assert_ne!(rows[1].id, rows[2].id);
    assert!(!rows[2].input.to_string().contains(PRIVATE));
}

#[tokio::test]
async fn agent_history_e2e_real_knowledge_cc_removal_restores_private_seed_and_restricts_inspection()
 {
    let mut scripted = turns("PRIVATE FLOOR 60");
    scripted.extend(turns("Public information only."));
    scripted.push(LlmTurn::text("\"Restored floor 60\""));
    // Box the real workflow/provider journey at the stock 2 MiB libtest stack.
    let mut j = Box::pin(SalesJourney::new(scripted, Audience::Team)).await;
    let member = j.member(AccessLevel::L5).await;
    let lower = j.member(AccessLevel::PUBLIC).await;
    sqlx::query("UPDATE company_members SET role='admin' WHERE company_id=$1 AND user_id=$2")
        .bind(j.d.fixture.target.company.as_uuid())
        .bind(lower.user_id)
        .execute(j.d.p().pool())
        .await
        .unwrap();
    j.d.bind(answering_source(), Some(MessageTrigger::Direct))
        .await;
    let first =
        j.d.email(incoming(&j, &member, Turn::Private))
            .await
            .thread
            .unwrap()
            .id;
    j.drive(1).await;
    assert_eq!(j.d.llm.observed().len(), 2);
    assert_eq!(j.facts().await["content"], 5);
    let second =
        j.d.email(incoming(&j, &member, Turn::Public))
            .await
            .thread
            .unwrap()
            .id;
    assert_eq!(second, first);
    j.drive(2).await;
    let public = j.d.llm.observed();
    assert_eq!(public.len(), 2);
    assert!(public.iter().all(|request| {
        let value = request.to_string();
        !value.contains(PRIVATE) && !value.contains("P07_FLOOR_QUERY")
    }));
    assert_eq!(j.facts().await["reply_levels"], json!([0]));
    let sent = j.transport.payloads.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    assert!(!sent[1].to_string().contains(PRIVATE));
    remove_customer(&j, first).await;
    assert_eq!(
        j.d.email(incoming(&j, &member, Turn::Restored))
            .await
            .thread
            .unwrap()
            .id,
        first
    );
    j.drive(3).await;
    let restored = j.d.llm.observed();
    assert_eq!(restored.len(), 1);
    let rows = exchanges(&j).await;
    restored_context(&rows, &restored[0]);
    restricted_inspection(
        &j,
        &rows[0],
        WorkflowActor::authenticated(lower.user_id).unwrap(),
    )
    .await;
    assert_eq!(j.transport.calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        j.d.count("SELECT count(*) FROM workflow_knowledge_consumptions WHERE company_id=$1")
            .await,
        2,
        "two real persisted knowledge consumptions"
    );
    j.d.knowledge.shutdown().await.unwrap();
    assert_eq!(j.d.llm.finish().await.unwrap(), 5);
}
