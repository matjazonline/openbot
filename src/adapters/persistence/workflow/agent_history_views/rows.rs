use super::*;
use chrono::{DateTime, Utc};

#[derive(sqlx::FromRow)]
pub(super) struct Anchor {
    pub conversation_id: Uuid,
    pub expired_count: i64,
    pub expired_through_order: i64,
    pub channel_id: Option<Uuid>,
    pub thread_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
pub(super) struct Metadata {
    pub id: Uuid,
    pub run_id: Uuid,
    pub execution_id: Uuid,
    pub checkpoint_id: Option<Uuid>,
    pub agent_id: Uuid,
    pub revision_id: Uuid,
    pub manifest_id: Uuid,
    pub position: i64,
    pub state: String,
    pub access_level: i16,
    pub payload_retained: bool,
    pub preparation_digest: Option<Vec<u8>>,
    pub output_digest: Option<Vec<u8>>,
    pub created_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub content_level: i16,
    pub channel_id: Option<Uuid>,
    pub thread_id: Option<Uuid>,
    pub workflow_state: String,
    pub waiting_reason: Option<String>,
    pub human_wait: bool,
    pub channel_response: Option<String>,
    pub delivery: Option<String>,
}

pub(super) const METADATA: &str = "SELECT exchange.id,exchange.run_id,exchange.execution_id,exchange.checkpoint_id,
    exchange.agent_id,exchange.revision_id,exchange.manifest_id,
    exchange.position,exchange.state,exchange.access_level::smallint AS access_level,exchange.payload_retained,
    exchange.preparation_digest,exchange.output_digest,exchange.created_at,exchange.activated_at,exchange.finished_at,
    run.content_level::smallint AS content_level,run.channel_id,run.thread_id,run.state AS workflow_state,run.waiting_reason,
    EXISTS(SELECT 1 FROM workflow_waits AS wait WHERE wait.company_id=exchange.company_id
      AND wait.run_id=exchange.run_id AND wait.execution_id=exchange.execution_id AND wait.state='waiting'
      AND (wait.reason='decision' OR (wait.reason='event' AND wait.notification_intent IS NOT NULL))) AS human_wait,
    final.state AS channel_response,delivery.status AS delivery
    FROM agent_history_exchanges AS exchange JOIN workflow_runs AS run
      ON run.company_id=exchange.company_id AND run.id=exchange.run_id
    LEFT JOIN channel_reply_decisions AS final ON final.company_id=run.company_id AND final.run_id=run.id
    LEFT JOIN message_deliveries AS delivery ON delivery.company_id=final.company_id AND delivery.id=final.delivery_id";

#[derive(sqlx::FromRow)]
pub(super) struct PayloadRow {
    pub input: Value,
    pub output: Option<Value>,
    pub frozen_preparation: Option<Value>,
    pub preparation_digest: Option<Vec<u8>>,
    pub output_digest: Option<Vec<u8>>,
    pub state: String,
}

pub(super) async fn payload_on(
    p: &PostgresPersistence,
    company: CompanyId,
    id: Uuid,
) -> AppResult<Option<PayloadRow>> {
    Ok(sqlx::query_as("SELECT execution.frozen_inputs->'context' AS input,exchange.output,exchange.frozen_preparation,
        exchange.preparation_digest,exchange.output_digest,exchange.state
        FROM agent_history_exchanges AS exchange JOIN workflow_executions AS execution
          ON execution.company_id=exchange.company_id AND execution.run_id=exchange.run_id AND execution.id=exchange.execution_id
        WHERE exchange.company_id=$1 AND exchange.id=$2 AND exchange.payload_retained")
        .bind(company.as_uuid()).bind(id).fetch_optional(p.pool()).await?)
}

pub(super) async fn metadata_on(
    db: &mut PgConnection,
    company: CompanyId,
    conversation: Uuid,
    ids: &[Uuid],
) -> AppResult<Vec<Metadata>> {
    let sql = format!(
        "{METADATA} WHERE exchange.company_id=$1 AND exchange.conversation_id=$2
        AND exchange.id=ANY($3) ORDER BY exchange.position DESC"
    );
    Ok(sqlx::query_as(&sql)
        .bind(company.as_uuid())
        .bind(conversation)
        .bind(ids)
        .fetch_all(db)
        .await?)
}

pub(super) async fn page_ids_on(
    db: &mut PgConnection,
    company: CompanyId,
    conversation: Uuid,
    request: &HistoryPageRequest,
) -> AppResult<Vec<Uuid>> {
    let before = request
        .after
        .as_ref()
        .map(HistoryCursor::parts)
        .transpose()?
        .map(|(_, order)| order.get() as i64);
    Ok(sqlx::query_scalar(
        "SELECT id FROM agent_history_exchanges WHERE company_id=$1 AND conversation_id=$2
        AND ($3::bigint IS NULL OR position<$3) ORDER BY position DESC LIMIT $4",
    )
    .bind(company.as_uuid())
    .bind(conversation)
    .bind(before)
    .bind(i64::from(request.limit) + 1)
    .fetch_all(db)
    .await?)
}
