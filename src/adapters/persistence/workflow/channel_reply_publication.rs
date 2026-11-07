use super::*;
use crate::application::workflow::channel_reply::*;
use chrono::{DateTime, Utc};

#[derive(sqlx::FromRow)]
struct ClaimRow {
    company_id: Uuid,
    run_id: Uuid,
    proposal_id: Uuid,
    integrity: String,
    generation: Uuid,
    lease_expires_at: DateTime<Utc>,
    attempts: i32,
}
impl ClaimRow {
    fn claim(self, worker: ReplyPublicationWorker) -> AppResult<ReplyPublicationClaim> {
        Ok(ReplyPublicationClaim {
            company: CompanyId::new(self.company_id),
            run: RunId::new(self.run_id),
            proposal: ReplyProposalId::new(self.proposal_id),
            integrity: self.integrity.try_into().map_err(|_| invalid())?,
            worker,
            generation: self.generation,
            expires_at: self.lease_expires_at,
            attempt: u8::try_from(self.attempts).map_err(|_| invalid())?,
        })
    }
}

#[async_trait]
impl ReplyPublications for PostgresPersistence {
    async fn claim_replies(
        &self,
        worker: ReplyPublicationWorker,
        page: ReplyPublicationPage,
    ) -> AppResult<Vec<ReplyPublicationClaim>> {
        if worker.0.is_nil() {
            return Err(AppError::BadRequest(
                "Invalid reply publication worker".into(),
            ));
        }
        tokio::time::timeout(std::time::Duration::from_secs(7),async {
        let mut tx = self.pool().begin().await?;
        bounds(&mut tx).await?;
        let rows: Vec<ClaimRow>=sqlx::query_as(
            "WITH ranked AS (SELECT company_id,run_id,available_at, \
             row_number() OVER(PARTITION BY company_id ORDER BY available_at,run_id) AS ordinal \
             FROM channel_reply_decisions WHERE (state='pending' AND available_at<=clock_timestamp()) \
             OR (state='claimed' AND lease_expires_at<=clock_timestamp())), \
             chosen AS (SELECT decision.company_id,decision.run_id FROM channel_reply_decisions AS decision \
             JOIN ranked USING(company_id,run_id) WHERE (decision.state='pending' AND decision.available_at<=clock_timestamp()) \
             OR (decision.state='claimed' AND decision.lease_expires_at<=clock_timestamp()) ORDER BY ranked.ordinal, \
             COALESCE((SELECT served.last_claimed_at FROM channel_reply_decisions AS served WHERE served.company_id=decision.company_id \
             AND served.last_claimed_at IS NOT NULL ORDER BY served.last_claimed_at DESC LIMIT 1),'-infinity'::timestamptz), \
             ranked.available_at,decision.company_id,decision.run_id \
             LIMIT $1 FOR UPDATE OF decision SKIP LOCKED), \
             exhausted AS (UPDATE channel_reply_decisions AS decision SET state='failed',diagnostic_code='attempts_exhausted', \
             generation=NULL,worker_id=NULL,lease_expires_at=NULL,updated_at=clock_timestamp() \
             FROM chosen WHERE decision.company_id=chosen.company_id AND decision.run_id=chosen.run_id AND decision.attempts=8) \
             UPDATE channel_reply_decisions AS decision SET state='claimed',attempts=attempts+1,generation=gen_random_uuid(),last_claimed_at=clock_timestamp(), \
             worker_id=$2,lease_expires_at=clock_timestamp()+interval '30 seconds',updated_at=clock_timestamp() \
             FROM chosen WHERE decision.company_id=chosen.company_id AND decision.run_id=chosen.run_id AND decision.attempts<8 \
             RETURNING decision.company_id,decision.run_id,decision.proposal_id,decision.integrity,decision.generation,decision.lease_expires_at,decision.attempts")
            .bind(i64::from(page.get())).bind(worker.0).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        rows.into_iter().map(|row| row.claim(worker)).collect()
        }).await.map_err(|_|storage_timeout())?
    }

    async fn release_reply(
        &self,
        claim: ReplyPublicationClaim,
        retry: ReplyPublicationRetry,
    ) -> AppResult<bool> {
        tokio::time::timeout(std::time::Duration::from_secs(7), async {
            let mut tx = self.pool().begin().await?;
            bounds(&mut tx).await?;
            let changed = settle_on(&mut tx, claim, retry).await?;
            tx.commit().await?;
            Ok(changed)
        })
        .await
        .map_err(|_| storage_timeout())?
    }
}

pub(super) async fn bounds(db: &mut PgConnection) -> AppResult<()> {
    sqlx::query(
        "SELECT set_config('lock_timeout','2s',true),set_config('statement_timeout','5s',true)",
    )
    .execute(db)
    .await?;
    Ok(())
}

pub(super) fn storage_timeout() -> AppError {
    AppError::Conflict("Reply publication storage timed out".into())
}

/// Both queue release and atomic publication refusal use this exact claim witness.
pub(super) const CLAIM_FILTER: &str =
    "company_id=$1 AND run_id=$2 AND proposal_id=$3 AND integrity=$4
    AND worker_id=$5 AND generation=$6 AND lease_expires_at=$7 AND attempts=$8
    AND state='claimed' AND lease_expires_at>clock_timestamp()";

pub(super) fn bind_claim<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    claim: ReplyPublicationClaim,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    query
        .bind(claim.company.as_uuid())
        .bind(claim.run.as_uuid())
        .bind(claim.proposal.as_uuid())
        .bind(claim.integrity.as_hex().to_owned())
        .bind(claim.worker.0)
        .bind(claim.generation)
        .bind(claim.expires_at)
        .bind(i32::from(claim.attempt))
}

pub(super) async fn settle_on(
    db: &mut PgConnection,
    claim: ReplyPublicationClaim,
    retry: ReplyPublicationRetry,
) -> AppResult<bool> {
    let failure = match retry {
        ReplyPublicationRetry::Terminal(reason) => Some(reason.as_str()),
        ReplyPublicationRetry::Retry if claim.attempt >= 8 => Some("attempts_exhausted"),
        ReplyPublicationRetry::Retry => None,
    };
    let query=format!("UPDATE channel_reply_decisions SET state=CASE WHEN $9::text IS NULL THEN 'pending' ELSE 'failed' END,
        diagnostic_code=$9,generation=NULL,worker_id=NULL,lease_expires_at=NULL,
        available_at=clock_timestamp()+make_interval(secs=>LEAST(300,2^attempts)::integer),updated_at=clock_timestamp()
        WHERE {CLAIM_FILTER}");
    Ok(bind_claim(sqlx::query(&query), claim)
        .bind(failure)
        .execute(db)
        .await?
        .rows_affected()
        == 1)
}
