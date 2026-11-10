//! Runtime consumption uses the actual live claim, filters by its ceiling, and commits the
//! observation with the content-level rise. It does not grant any human viewer access.
//! Reader of the run's frozen thread history for `context.history` (06-D6 A). One read in
//! the claim's transaction: the admission snapshot (`workflow_history_members`), with
//! internal notes excluded at capture; only the newest 256 members join their messages,
//! so a long thread never materializes bodies it cannot return.
use super::http::runtime;
use super::persistence_bound::{Attempt, End, WriteBound};
use super::*;
use crate::application::workflow::{
    actions::ActionScope,
    history::{FrozenHistory, HistoryMessage, KIND, MAX_HISTORY_ITEMS, MAX_HISTORY_TEXT},
    lease::{ClaimedWorkflow, LeasePolicy},
    polling::WorkflowStepKind,
};
use crate::entities::message::{MessageAudience, MessageRole, ThreadEntryKind};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::str::FromStr;

#[derive(sqlx::FromRow)]
struct HistoryRow {
    message_id: Uuid,
    access_level: i16,
    content_exact: Option<bool>,
    at: DateTime<Utc>,
    role: String,
    audience: String,
    entry_kind: String,
    author_label: String,
    author_subject: Option<String>,
    subject: String,
    body: String,
}

impl HistoryRow {
    fn message(self) -> AppResult<HistoryMessage> {
        let malformed = || AppError::BadRequest("Invalid stored workflow history".into());
        Ok(HistoryMessage {
            message_id: self.message_id,
            at: self.at,
            role: MessageRole::from_str(&self.role).map_err(|_| malformed())?,
            audience: MessageAudience::from_str(&self.audience).map_err(|_| malformed())?,
            entry_kind: ThreadEntryKind::from_str(&self.entry_kind).map_err(|_| malformed())?,
            // The legacy display rule: the label, else the identity handle.
            author: match self.author_label.trim() {
                "" => self.author_subject.unwrap_or_else(|| "Unknown".into()),
                _ => self.author_label,
            },
            subject: self.subject,
            body: self.body,
        })
    }
}

#[async_trait]
impl crate::application::workflow::history::WorkflowHistory for PostgresPersistence {
    async fn frozen_history(&self, claim: &ClaimedWorkflow) -> AppResult<FrozenHistory> {
        let policy = LeasePolicy::new(std::time::Duration::from_secs(6))?;
        let bound = WriteBound::within(
            "history.consume",
            policy,
            claim.window.expires.min(claim.window.run_deadline),
        );
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = consume_on(&mut tx, claim, &bound).await.map(End::Commit);
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(history) => Ok(history),
            Attempt::Failed(error) => Err(error),
            // A possibly committed rise grants neither history nor downstream execution.
            Attempt::Ambiguous => Err(bound.unresolved()),
        }
    }
}

async fn consume_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<FrozenHistory> {
    let scope = claim.fence.scope;
    let facts = runtime::facts(
        tx,
        ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
    )
    .await?;
    if !lease::lock_fence(tx, claim.fence, bound.server_timeout()).await? {
        return Err(ownership_lost());
    }
    live(tx, claim, bound).await?;
    let inputs = runtime::frozen_inputs(tx, &facts, claim, &WorkflowStepKind(KIND.into())).await?;
    let max_tokens = inputs
        .get("max_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(invalid)?;
    let eligible: i64 = sqlx::query_scalar(
        "SELECT eligible_count FROM workflow_history_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .unwrap_or(0);
    let rows = read_rows(tx, scope).await?;
    let highest = rows
        .iter()
        .map(|row| {
            crate::entities::access_level::AccessLevel::try_from(row.access_level)
                .map_err(|_| invalid())
        })
        .collect::<AppResult<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(crate::entities::access_level::AccessLevel::PUBLIC);
    // Every returned message counts, as for the level: human rows are conversational input,
    // and an inexact run's message carries its bit (E3).
    let mut exact = true;
    for row in &rows {
        exact &= super::knowledge::stored_message_exactness(&row.role, row.content_exact)?;
    }
    let history = FrozenHistory {
        max_tokens,
        eligible: u64::try_from(eligible).map_err(|_| invalid())?,
        newest: rows
            .into_iter()
            .map(HistoryRow::message)
            .collect::<AppResult<_>>()?,
    };
    super::knowledge::absorb_content_on(tx, scope.company, scope.run, highest, exact).await?;
    record_consumption(tx, claim, highest).await?;
    live(tx, claim, bound).await?;
    Ok(history)
}

async fn read_rows(
    tx: &mut Transaction<'_, Postgres>,
    scope: crate::application::workflow::activation::ActivationRequest,
) -> AppResult<Vec<HistoryRow>> {
    // Cut one character past the bound to mark truncation without unbounded bodies.
    let rows = sqlx::query_as(
            "SELECT newest.message_id, message.access_level::smallint AS access_level, message.content_exact, association.created_at AS at, message.role, message.audience, \
                    association.entry_kind, author.display_label AS author_label, \
                    author_identity.subject AS author_subject, \
                    left(message.subject, $3) AS subject, left(message.clean_text_body, $3) AS body \
               FROM (SELECT * FROM workflow_history_members WHERE company_id=$1 AND run_id=$2 \
                     ORDER BY ordinal DESC LIMIT $4) AS newest \
               JOIN thread_messages AS association \
                 ON (association.company_id, association.channel_id, association.thread_id, association.message_id) = \
                    ($1, newest.channel_id, newest.thread_id, newest.message_id) \
               JOIN messages AS message \
                 ON (message.company_id, message.id) = ($1, newest.message_id) \
               JOIN principals AS author \
                 ON (author.company_id, author.id) = (message.company_id, message.author_principal_id) \
               LEFT JOIN participant_identities AS author_identity \
                 ON (author_identity.company_id, author_identity.id) = (message.company_id, message.authored_identity_id) \
              ORDER BY newest.ordinal DESC",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(MAX_HISTORY_TEXT as i32 + 1)
        .bind(MAX_HISTORY_ITEMS as i64)
        .fetch_all(&mut **tx)
        .await?;
    Ok(rows)
}

async fn live(tx: &mut PgConnection, claim: &ClaimedWorkflow, bound: &WriteBound) -> AppResult<()> {
    if lease::live_window(tx, claim.fence, bound.started())
        .await?
        .is_none()
    {
        return Err(ownership_lost());
    }
    Ok(())
}

fn ownership_lost() -> AppError {
    AppError::Execution(crate::app_error::ExecutionFailure::OwnershipLost)
}

async fn record_consumption(
    tx: &mut PgConnection,
    claim: &ClaimedWorkflow,
    highest: crate::entities::access_level::AccessLevel,
) -> AppResult<()> {
    let fence = claim.fence;
    let scope = fence.scope;
    // An identical replay still schedules the deferred live-fence check at COMMIT.
    sqlx::query("INSERT INTO workflow_history_consumptions
        (company_id,run_id,execution_id,job_id,attempt_number,execution_generation,worker_id,content_level)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        ON CONFLICT (company_id,run_id,execution_id,job_id,attempt_number)
        DO UPDATE SET content_level=EXCLUDED.content_level")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.generation.0).bind(fence.worker.0)
        .bind(highest.as_i16()).execute(tx).await?;
    Ok(())
}
