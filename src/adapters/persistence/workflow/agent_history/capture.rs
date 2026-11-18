//! Capture at exchange acceptance (phase 05 E8, U3): which accepted windows may become a
//! generation intent, and into which scope. Only a completed `agent.run` exchange qualifies:
//! a thread conversation under the agent's `threads` switch (into the agent's memory), or a
//! private session under its `private_sessions` switch when the session is not attached to a
//! channel or thread and the run is its creator's own manual root (E2/U4 user-scope
//! eligibility, into that user's memory). Child branches and attached sessions are never
//! captured. Only the logical input and the accepted output are sent: no tool arguments or
//! results, recalled memory or internal notes. The freeze itself (tenant, namespace, switch,
//! intent) is `memory::deliveries::enqueue_capture_on`.
use super::*;
use crate::adapters::persistence::memory::deliveries::{
    CaptureEnqueue, CaptureIntent, enqueue_capture_on,
};
use crate::application::memory::MemoryCaptureKind;
use crate::application::workflow::memory_tools::{MemorySaveTarget, memory_write_scope};
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct ConversationRow {
    scope_kind: String,
    attached: Option<bool>,
}

/// Runs inside the accepting transaction, after the exchange became `completed` under the run
/// lock. Never fails the acceptance for an ineligible window: it answers why nothing was done.
pub(super) async fn capture_on(
    db: &mut PgConnection,
    scope: ActionScope,
    exchange: Uuid,
    agent: Uuid,
    input: &Value,
    output: &Value,
) -> AppResult<CaptureEnqueue> {
    let conversation = sqlx::query_as::<_, ConversationRow>(
        "SELECT conversation.scope_kind, \
           (session.channel_id IS NOT NULL OR session.thread_id IS NOT NULL) AS attached \
         FROM agent_history_exchanges AS exchange \
         JOIN agent_history_conversations AS conversation \
           ON conversation.company_id = exchange.company_id \
          AND conversation.id = exchange.conversation_id \
         LEFT JOIN agent_history_sessions AS session \
           ON session.company_id = conversation.company_id AND session.id = conversation.session_id \
         WHERE exchange.company_id = $1 AND exchange.id = $2",
    )
    .bind(scope.company.as_uuid())
    .bind(exchange)
    .fetch_one(&mut *db)
    .await?;
    let eligible =
        super::super::checkpoints::memory_tools::eligible_on(db, scope.company, scope.run, agent)
            .await?;
    let (kind, target) = match (conversation.scope_kind.as_str(), conversation.attached) {
        ("thread", _) => {
            let Some(target) = memory_write_scope(&eligible, MemorySaveTarget::Default) else {
                return Ok(CaptureEnqueue::Skipped("scope_unavailable"));
            };
            (MemoryCaptureKind::Threads, target.owner)
        }
        ("private_session", Some(false)) => {
            let Some(target) = eligible.iter().find(|scope| scope.private_person) else {
                return Ok(CaptureEnqueue::Skipped("ineligible"));
            };
            (MemoryCaptureKind::PrivateSessions, target.owner)
        }
        _ => return Ok(CaptureEnqueue::Skipped("ineligible")),
    };
    let (input, output) = (input.to_string(), output.to_string());
    enqueue_capture_on(
        db,
        &CaptureIntent {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
            exchange,
            agent,
            kind,
            scope: target,
            input: &input,
            output: &output,
        },
    )
    .await
}
