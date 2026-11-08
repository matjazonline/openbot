use super::*;

pub(super) async fn acquire_on(
    db: &mut PgConnection,
    company: CompanyId,
    exchange: AgentExchangeId,
) -> AppResult<bool> {
    let owned: bool = sqlx::query_scalar("SELECT agent_history_take_turn($1,$2)")
        .bind(company.as_uuid())
        .bind(exchange.as_uuid())
        .fetch_one(db)
        .await?;
    if owned {
        tracing::info!(company_id=%company.as_uuid(),exchange_id=%exchange.as_uuid(),
            durability="staged", outcome="owned_or_replayed", event="agent_history.acquired", "Agent turn verified in current transaction");
    }
    Ok(owned)
}

/// A read-only receipt proves the whole turn effect, never just matching input.
pub(super) async fn owned_on(
    db: &mut PgConnection,
    company: CompanyId,
    exchange: AgentExchangeId,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_history_exchanges AS exchange \
         JOIN agent_history_conversations AS conversation ON conversation.company_id=exchange.company_id \
             AND conversation.id=exchange.conversation_id \
         WHERE exchange.company_id=$1 AND exchange.id=$2 \
             AND (exchange.state='completed' OR (exchange.state IN ('queued','active') \
                 AND conversation.active_exchange_id=exchange.id)))",
    ).bind(company.as_uuid()).bind(exchange.as_uuid()).fetch_one(db).await?)
}
