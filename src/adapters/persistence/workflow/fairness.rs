//! Fair admission uses hints, never a second ownership ledger.
use super::*;
use crate::application::workflow::{
    activation::ActivationRequest, lease::WorkflowWorkerId, polling::FAIR_OPPORTUNITY,
};
use std::collections::HashMap;

#[derive(sqlx::FromRow)]
struct Demand {
    company_id: Uuid,
    ticket: i64,
    expired: bool,
}

/// Own run first, then the same singleton used by ALL claims and renewals.
pub(super) async fn available(
    db: &mut PgConnection,
    scope: ActivationRequest,
    worker: WorkflowWorkerId,
) -> AppResult<bool> {
    let policy = capacity::lock(db).await?;
    remove_invalid(db).await?;
    sqlx::query("INSERT INTO workflow_dispatch_demand (company_id,worker_id,run_id,execution_id,job_id) VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
        .bind(scope.company.as_uuid()).bind(worker.0).bind(scope.run.as_uuid())
        .bind(scope.execution.as_uuid()).bind(scope.job.0).execute(&mut *db).await?;
    // Fresh READ COMMITTED visibility after singleton serialization includes renewals.
    let live = capacity::live_companies(db, policy.global()).await?;
    if live.len() >= usize::from(policy.global()) {
        return Ok(false);
    }
    let mut counts = HashMap::<Uuid, usize>::new();
    for company in live {
        *counts.entry(company).or_default() += 1;
    }
    let full: Vec<Uuid> = counts
        .into_iter()
        .filter_map(|(company, count)| (count >= usize::from(policy.company())).then_some(company))
        .collect();
    let head = sqlx::query_as::<_, Demand>(&format!("SELECT demand.company_id,demand.ticket,COALESCE(demand.opportunity_until<=clock_timestamp(),false) AS expired FROM workflow_dispatch_demand AS demand WHERE NOT (demand.company_id=ANY($1)) AND {VALID_HINT} ORDER BY (demand.opportunity_until IS NULL),demand.ticket LIMIT 1"))
        .bind(full).fetch_optional(&mut *db).await?;
    let Some(head) = head else {
        return Ok(false);
    };
    if head.expired {
        remove(db, head.ticket).await?;
        return Ok(false);
    }
    if head.company_id == scope.company.as_uuid() {
        remove(db, head.ticket).await?;
        return Ok(true);
    }
    // No room -> no timer. Once offered, ALL newer claims protect this room.
    sqlx::query("UPDATE workflow_dispatch_demand SET opportunity_until=clock_timestamp()+$2*interval '1 second' WHERE ticket=$1 AND opportunity_until IS NULL")
        .bind(head.ticket).bind(FAIR_OPPORTUNITY.as_secs_f64()).execute(db).await?;
    Ok(false)
}

async fn remove(db: &mut PgConnection, ticket: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM workflow_dispatch_demand WHERE ticket=$1")
        .bind(ticket)
        .execute(db)
        .await?;
    Ok(())
}

async fn remove_invalid(db: &mut PgConnection) -> AppResult<()> {
    sqlx::query(&format!("DELETE FROM workflow_dispatch_demand WHERE ticket IN (SELECT demand.ticket FROM workflow_dispatch_demand AS demand WHERE NOT ({VALID_HINT}) ORDER BY (demand.opportunity_until IS NULL),demand.ticket LIMIT 32)"))
        .execute(db).await?;
    Ok(())
}

/// Plain scoped reads only: taking another run lock here inverts transition order.
pub(super) const VALID_HINT: &str = r#"
    EXISTS (SELECT 1 FROM background_tasks AS job
      JOIN workflow_executions AS execution
        ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id
      JOIN workflow_runs AS run
        ON run.company_id=execution.company_id AND run.id=execution.run_id
      WHERE job.company_id=demand.company_id AND job.id=demand.job_id
        AND execution.id=demand.execution_id AND run.id=demand.run_id
        AND job.queue_kind='workflow' AND job.status='pending'
        AND job.run_at<=clock_timestamp() AND job.retry_count<job.max_retries
        AND execution.completed_at IS NULL AND run.state IN ('queued','running')
        AND run.deadline>clock_timestamp()
        AND NOT EXISTS (SELECT 1 FROM workflow_waits AS wait
          WHERE wait.company_id=execution.company_id AND wait.execution_id=execution.id
            AND wait.state='waiting'))
"#;
