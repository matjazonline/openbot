//! Bounded completed-pair selection. Hidden levels never enter either query.
use super::*;
use crate::application::workflow::agent::PreparedAgent;
use crate::entities::access_level::AccessLevel;
use crate::services::harness::runs::{
    LogicalSeed, LogicalSeedPair, MAX_LOGICAL_SEED_BYTES, MAX_LOGICAL_SEED_PAIRS,
};
use serde_json::Value;

pub(in crate::adapters::persistence::workflow) const CANDIDATES_SQL: &str =
    include_str!("selection_candidates.sql");
pub(in crate::adapters::persistence::workflow) const COUNT_SQL: &str =
    include_str!("selection_count.sql");
const PAYLOAD_SQL: &str = include_str!("selection_payload.sql");

#[derive(sqlx::FromRow)]
struct Source {
    conversation_id: Uuid,
    position: i64,
    ceiling: i16,
    content: i16,
}
#[derive(sqlx::FromRow)]
struct Candidate {
    id: Uuid,
    position: i64,
    access_level: i16,
    input_bytes: i32,
    output_bytes: i32,
}
#[derive(sqlx::FromRow)]
struct Payload {
    input: Value,
    output: Value,
}

pub(super) async fn select_on(
    db: &mut PgConnection,
    scope: ActionScope,
    prepared: &PreparedAgent,
) -> AppResult<FrozenAgentHistory> {
    // Mandatory content is checked before fetching any history payload.
    prepared.history_budget(&LogicalSeed::default())?;
    let source = source_on(db, scope).await?;
    let candidates = candidates_on(db, scope, &source).await?;
    let mut selected = Vec::new();
    let mut pairs = Vec::new();
    let mut raw_bytes = 0usize;
    for candidate in candidates {
        let raw = usize::try_from(candidate.input_bytes + candidate.output_bytes)
            .map_err(|_| invalid())?
            + 32;
        if selected.len() == MAX_LOGICAL_SEED_PAIRS || raw_bytes + raw > MAX_LOGICAL_SEED_BYTES {
            break;
        }
        let payload = payload_on(db, scope, &candidate).await?;
        let mut proposed = pairs.clone();
        proposed.push(LogicalSeedPair {
            input: payload.input,
            output: payload.output,
        });
        match LogicalSeed::new(proposed.clone()).and_then(|seed| prepared.history_budget(&seed)) {
            Ok(_) => (),
            Err(AppError::Execution(crate::app_error::ExecutionFailure::Budget)) => break,
            Err(error) => return Err(error),
        }
        pairs = proposed;
        raw_bytes += raw;
        selected.push(SelectedAgentExchange {
            id: AgentExchangeId::new(candidate.id),
            position: AgentExchangeOrder::try_from(candidate.position as u64)?,
            access_level: AccessLevel::try_from(candidate.access_level).map_err(|_| invalid())?,
        });
    }
    selected.reverse();
    pairs.reverse();
    // Reversal preserves serialized size/cost, but rebuild from chronological material.
    let seed = LogicalSeed::new(pairs)?;
    let budget = prepared.history_budget(&seed)?;
    let count = count_on(db, scope, &source).await?;
    let selected_level = selected
        .iter()
        .map(|row| row.access_level)
        .max()
        .unwrap_or(AccessLevel::PUBLIC);
    let history = FrozenAgentHistory {
        omitted_eligible: count
            .checked_sub(selected.len() as u64)
            .ok_or_else(invalid)?,
        selected,
        seed,
        selected_level,
        content_level: AccessLevel::try_from(source.content)
            .map_err(|_| invalid())?
            .max(selected_level),
        budget,
    };
    history.validate()?;
    tracing::info!(company_id=%scope.company.as_uuid(),exchange_id=%scope.execution.as_uuid(),
        selected=history.selected.len(), omitted_eligible=history.omitted_eligible,
        selected_level=history.selected_level.as_i16(), content_level=history.content_level.as_i16(),
        durability="staged", event="agent_history.selected", "Agent history suffix selected in current transaction");
    Ok(history)
}

async fn source_on(db: &mut PgConnection, scope: ActionScope) -> AppResult<Source> {
    sqlx::query_as("SELECT exchange.conversation_id,exchange.position,run.knowledge_ceiling::smallint AS ceiling,run.content_level::smallint AS content FROM agent_history_exchanges AS exchange JOIN workflow_runs AS run ON run.company_id=exchange.company_id AND run.id=exchange.run_id WHERE exchange.company_id=$1 AND exchange.run_id=$2 AND exchange.execution_id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .fetch_optional(db).await?.ok_or_else(missing)
}

async fn candidates_on(
    db: &mut PgConnection,
    scope: ActionScope,
    source: &Source,
) -> AppResult<Vec<Candidate>> {
    sqlx::query_as(CANDIDATES_SQL)
        .bind(scope.company.as_uuid())
        .bind(source.conversation_id)
        .bind(source.position)
        .bind(source.ceiling)
        .bind((MAX_LOGICAL_SEED_PAIRS + 1) as i64)
        .fetch_all(db)
        .await
        .map_err(Into::into)
}

async fn count_on(db: &mut PgConnection, scope: ActionScope, source: &Source) -> AppResult<u64> {
    // One scalar, no payload materialization; index scan is bounded by WriteBound time.
    let count: i64 = sqlx::query_scalar(COUNT_SQL)
        .bind(scope.company.as_uuid())
        .bind(source.conversation_id)
        .bind(source.position)
        .bind(source.ceiling)
        .fetch_one(db)
        .await?;
    u64::try_from(count).map_err(|_| invalid())
}

async fn payload_on(
    db: &mut PgConnection,
    scope: ActionScope,
    candidate: &Candidate,
) -> AppResult<Payload> {
    sqlx::query_as(PAYLOAD_SQL)
        .bind(scope.company.as_uuid())
        .bind(candidate.id)
        .bind(candidate.input_bytes)
        .bind(candidate.output_bytes)
        .bind(MAX_LOGICAL_SEED_BYTES as i64)
        .fetch_optional(db)
        .await?
        .ok_or_else(missing)
}
