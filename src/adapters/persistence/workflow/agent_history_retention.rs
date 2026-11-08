//! History payload expiry shares the supervised workflow storage owner.
use super::*;
use crate::application::workflow::{agent_history::AgentExchangeId, storage::*};
use chrono::{DateTime, Utc};

#[derive(sqlx::FromRow)]
struct CandidateRow {
    company_id: Uuid,
    finished_at: DateTime<Utc>,
    id: Uuid,
}

pub(super) async fn page(
    pool: &sqlx::PgPool,
    after: Option<HistoryCandidate>,
) -> AppResult<HistoryPage> {
    let mut tx = super::storage::begin(pool).await?;
    // Both company discovery and record materialization are bounded. No payload columns.
    let rows: Vec<CandidateRow> = sqlx::query_as(
        "WITH companies AS MATERIALIZED (
          SELECT company_id FROM agent_history_exchanges
          WHERE payload_retained AND state IN ('completed','failed','cancelled')
            AND ($1::uuid IS NULL OR (company_id,finished_at,id)>($1,$2,$3))
          GROUP BY company_id ORDER BY company_id LIMIT $4)
        SELECT exchange.company_id,exchange.finished_at,exchange.id FROM agent_history_exchanges AS exchange
        JOIN companies ON companies.company_id=exchange.company_id
        WHERE exchange.payload_retained AND exchange.state IN ('completed','failed','cancelled')
          AND ($1::uuid IS NULL OR (exchange.company_id,exchange.finished_at,exchange.id)>($1,$2,$3))
        ORDER BY exchange.company_id,exchange.finished_at,exchange.id LIMIT $5",
    )
    .bind(after.map(|cursor| cursor.company.as_uuid()))
    .bind(after.map(|cursor| cursor.finished_at))
    .bind(after.map(|cursor| cursor.exchange.as_uuid()))
    .bind(RETENTION_PAGE_SIZE as i64)
    .bind(i64::from(HISTORY_EXPIRY_SWEEP_LIMIT))
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(HistoryPage {
        candidates: rows
            .into_iter()
            .map(|row| HistoryCandidate {
                company: CompanyId::new(row.company_id),
                finished_at: row.finished_at,
                exchange: AgentExchangeId::new(row.id),
            })
            .collect(),
    })
}

#[derive(sqlx::FromRow)]
struct Source {
    run_id: Uuid,
    execution_id: Uuid,
    job_id: Uuid,
}

pub(super) async fn expire(pool: &sqlx::PgPool, candidate: HistoryCandidate) -> AppResult<bool> {
    let mut tx = super::storage::begin(pool).await?;
    let outcome = expire_on(&mut tx, candidate).await;
    match outcome {
        Ok(expired) => {
            tx.commit().await?;
            if expired {
                tracing::info!(company_id=%candidate.company.as_uuid(), exchange_id=%candidate.exchange.as_uuid(),
                    event="agent_history.expired", "Agent history payload expired");
            }
            Ok(expired)
        }
        Err(error)
            if error
                .as_database_error()
                .is_some_and(|error| error.code().as_deref() == Some("55P03")) =>
        {
            tx.rollback().await?;
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

async fn expire_on(
    db: &mut PgConnection,
    candidate: HistoryCandidate,
) -> Result<bool, sqlx::Error> {
    let company = candidate.company.as_uuid();
    if sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE SKIP LOCKED",
    )
    .bind(company)
    .fetch_optional(&mut *db)
    .await?
    .is_none()
    {
        return Ok(false);
    }
    let source: Option<Source> = sqlx::query_as(
        "SELECT run_id,execution_id,job_id FROM agent_history_exchanges
        WHERE company_id=$1 AND id=$2 AND payload_retained AND state IN ('completed','failed','cancelled')",
    ).bind(company).bind(candidate.exchange.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(source) = source else {
        return Ok(false);
    };
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR SHARE NOWAIT")
        .bind(company)
        .bind(source.run_id)
        .execute(&mut *db)
        .await?;
    sqlx::query(
        "SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR SHARE NOWAIT",
    )
    .bind(company)
    .bind(source.execution_id)
    .execute(&mut *db)
    .await?;
    sqlx::query("SELECT id FROM background_tasks WHERE company_id=$1 AND id=$2 FOR SHARE NOWAIT")
        .bind(company)
        .bind(source.job_id)
        .execute(&mut *db)
        .await?;
    let count = sqlx::query(
        "UPDATE agent_history_exchanges AS exchange SET payload_retained=false,
            payload_expired_at=clock_timestamp(),output=NULL,frozen_preparation=NULL
        WHERE exchange.company_id=$1 AND exchange.id=$2 AND agent_history_payload_expirable(exchange)",
    ).bind(company).bind(candidate.exchange.as_uuid()).execute(db).await?.rows_affected();
    Ok(count == 1)
}

pub(super) async fn scope_page(
    pool: &sqlx::PgPool,
    after: Option<CompanyId>,
) -> AppResult<Vec<CompanyId>> {
    let mut tx = super::storage::begin(pool).await?;
    let companies: Vec<Uuid> = sqlx::query_scalar(
        "SELECT company_id FROM (
          SELECT company_id FROM agent_history_conversations WHERE $1::uuid IS NULL OR company_id>$1
          UNION SELECT company_id FROM agent_history_sessions WHERE $1::uuid IS NULL OR company_id>$1
        ) AS scopes ORDER BY company_id LIMIT $2",
    ).bind(after.map(|company| company.as_uuid())).bind(RETENTION_PAGE_SIZE as i64)
        .fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(companies.into_iter().map(CompanyId::new).collect())
}

pub(super) async fn prune(pool: &sqlx::PgPool, company: CompanyId, limit: u16) -> AppResult<u64> {
    if !(1..=STORAGE_SWEEP_LIMIT).contains(&limit) {
        return Err(AppError::BadRequest(
            "Invalid agent history cleanup limit".into(),
        ));
    }
    let company = company.as_uuid();
    let mut tx = super::storage::begin(pool).await?;
    if sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE SKIP LOCKED",
    )
    .bind(company)
    .fetch_optional(&mut *tx)
    .await?
    .is_none()
    {
        return Ok(0);
    }
    let conversations = sqlx::query(
        "WITH unused AS MATERIALIZED (
          SELECT conversation.id FROM agent_history_conversations AS conversation
          WHERE conversation.company_id=$1 AND agent_history_conversation_unused(conversation)
          ORDER BY conversation.id LIMIT $2 FOR UPDATE OF conversation SKIP LOCKED)
        DELETE FROM agent_history_conversations AS conversation USING unused
        WHERE conversation.company_id=$1 AND conversation.id=unused.id",
    )
    .bind(company)
    .bind(i64::from(limit))
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let sessions = sqlx::query(
        "WITH unused AS MATERIALIZED (
          SELECT session.id FROM agent_history_sessions AS session WHERE session.company_id=$1
            AND NOT EXISTS(SELECT 1 FROM workflow_agent_sessions AS mapping WHERE mapping.company_id=$1 AND mapping.session_id=session.id)
            AND NOT EXISTS(SELECT 1 FROM agent_history_conversations AS conversation WHERE conversation.company_id=$1 AND conversation.session_id=session.id)
          ORDER BY session.id LIMIT $2 FOR UPDATE OF session SKIP LOCKED)
        DELETE FROM agent_history_sessions AS session USING unused WHERE session.company_id=$1 AND session.id=unused.id",
    ).bind(company).bind(i64::from(limit)-conversations as i64).execute(&mut *tx).await?.rows_affected();
    tx.commit().await?;
    let count = conversations + sessions;
    if count > 0 {
        tracing::info!(company_id=%company, count, event="agent_history.pruned", "Unused agent history scopes pruned");
    }
    Ok(count)
}
