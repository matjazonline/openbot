use super::polling::{CANDIDATES_SQL, CandidateRow, candidate};
use super::*;
use crate::application::workflow::{lease::WorkflowWorkerId, polling::*};
use std::time::Duration;

#[derive(sqlx::FromRow)]
struct JobCursor {
    after_job: Option<Uuid>,
    horizon: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
impl WorkflowFairPolling for PostgresPersistence {
    async fn poll_fair(&self, worker: WorkflowWorkerId, limit: u16) -> AppResult<FairPollPage> {
        if !(1..=128).contains(&limit) {
            return Err(invalid());
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
                .execute(&mut *tx).await?;
            let after: Option<Uuid> = sqlx::query_scalar("INSERT INTO workflow_poll_workers (worker_id) VALUES ($1) ON CONFLICT (worker_id) DO UPDATE SET touched_at=clock_timestamp() RETURNING after_company")
                .bind(worker.0).fetch_one(&mut *tx).await?;
            let sticky = sqlx::query_as::<_, StickyRow>(&format!("SELECT demand.ticket,demand.company_id,demand.run_id,demand.execution_id,demand.job_id,'job'::text AS work FROM workflow_dispatch_demand AS demand WHERE demand.worker_id=$1 AND {}", fairness::VALID_HINT))
                .bind(worker.0).fetch_optional(&mut *tx).await?;
            let (rows, demand) = match sticky {
                Some(row) => (vec![row.candidate], Some(DemandTicket(row.ticket))),
                None => (discover(&mut tx, worker, after, limit).await?, None),
            };
            clean_cursors(&mut tx, worker).await?;
            tx.commit().await?;
            Ok(FairPollPage { candidates: rows.into_iter().map(candidate).collect::<AppResult<_>>()?, demand })
        }).await.map_err(|_| lease::timed_out())?
    }

    async fn withdraw_demand(
        &self,
        worker: WorkflowWorkerId,
        ticket: DemandTicket,
    ) -> AppResult<()> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
                .execute(&mut *tx).await?;
            capacity::lock(&mut tx).await?;
            // Identity comparison cannot erase another requester's replacement ticket.
            sqlx::query("DELETE FROM workflow_dispatch_demand WHERE worker_id=$1 AND ticket=$2")
                .bind(worker.0).bind(ticket.0).execute(&mut *tx).await?;
            tx.commit().await?;
            Ok(())
        }).await.map_err(|_| lease::timed_out())?
    }
}

async fn discover(
    db: &mut PgConnection,
    worker: WorkflowWorkerId,
    after: Option<Uuid>,
    limit: u16,
) -> AppResult<Vec<CandidateRow>> {
    let company: Option<Uuid> = sqlx::query_scalar(&company_query())
        .bind(after)
        .fetch_optional(&mut *db)
        .await?;
    let Some(company) = company else {
        return Ok(vec![]);
    };
    sqlx::query("UPDATE workflow_poll_workers SET after_company=$2 WHERE worker_id=$1")
        .bind(worker.0)
        .bind(company)
        .execute(&mut *db)
        .await?;
    let cursor = sqlx::query_as::<_, JobCursor>("INSERT INTO workflow_poll_companies (worker_id,company_id) VALUES ($1,$2) ON CONFLICT (worker_id,company_id) DO UPDATE SET company_id=EXCLUDED.company_id RETURNING after_job,horizon")
        .bind(worker.0).bind(company).fetch_one(&mut *db).await?;
    let rows = sqlx::query_as::<_, CandidateRow>(&format!("{CANDIDATES_SQL} AND job.company_id=$1 AND ($2::uuid IS NULL OR job.id>$2) AND job.created_at<$3 ORDER BY job.id LIMIT $4"))
        .bind(company).bind(cursor.after_job).bind(cursor.horizon).bind(i64::from(limit))
        .fetch_all(&mut *db).await?;
    let next = rows.last().map(|row| row.job_id);
    // Strict horizon excludes timestamp ties/new arrivals until the next epoch.
    sqlx::query("UPDATE workflow_poll_companies SET after_job=$3,horizon=CASE WHEN $3::uuid IS NULL THEN clock_timestamp() ELSE horizon END WHERE worker_id=$1 AND company_id=$2")
        .bind(worker.0).bind(company).bind(next).execute(db).await?;
    Ok(rows)
}

pub(super) fn company_query() -> String {
    format!(
        "SELECT company_id FROM ({CANDIDATES_SQL}) AS eligible GROUP BY company_id ORDER BY ($1::uuid IS NOT NULL AND company_id<=$1),company_id LIMIT 1"
    )
}

async fn clean_cursors(db: &mut PgConnection, worker: WorkflowWorkerId) -> AppResult<()> {
    // Our parent is already locked. Retire deleted-company hints even for a
    // continuously active worker; they otherwise survive every stale-worker sweep.
    sqlx::query("DELETE FROM workflow_poll_companies WHERE worker_id=$1 AND company_id IN (SELECT cursor.company_id FROM workflow_poll_companies AS cursor WHERE cursor.worker_id=$1 AND NOT EXISTS (SELECT 1 FROM companies AS company WHERE company.id=cursor.company_id) ORDER BY cursor.company_id LIMIT 32)")
        .bind(worker.0).execute(&mut *db).await?;
    // Responsive workers touch within <=60s handler +30s poll, far below one day.
    // Delete a bounded number of children before empty parents; no large cascade.
    sqlx::query("WITH stale AS MATERIALIZED (SELECT worker.worker_id FROM workflow_poll_workers AS worker WHERE worker.touched_at<clock_timestamp()-interval '1 day' ORDER BY worker.touched_at,worker.worker_id LIMIT 32 FOR UPDATE SKIP LOCKED) DELETE FROM workflow_poll_companies WHERE (worker_id,company_id) IN (SELECT cursor.worker_id,cursor.company_id FROM workflow_poll_companies AS cursor JOIN stale ON stale.worker_id=cursor.worker_id ORDER BY cursor.worker_id,cursor.company_id LIMIT 32)")
        .execute(&mut *db).await?;
    sqlx::query("DELETE FROM workflow_poll_workers WHERE worker_id IN (SELECT worker.worker_id FROM workflow_poll_workers AS worker WHERE worker.touched_at<clock_timestamp()-interval '1 day' AND NOT EXISTS (SELECT 1 FROM workflow_poll_companies AS cursor WHERE cursor.worker_id=worker.worker_id) ORDER BY worker.touched_at,worker.worker_id LIMIT 32 FOR UPDATE SKIP LOCKED)")
        .execute(db).await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct StickyRow {
    ticket: i64,
    #[sqlx(flatten)]
    candidate: CandidateRow,
}
