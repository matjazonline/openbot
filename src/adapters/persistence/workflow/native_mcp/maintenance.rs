//! Run-first invalidation. Native owner transactions never cancel a workflow inline.
use super::*;

pub(in crate::adapters::persistence) async fn sweep(
    p: &PostgresPersistence,
    limit: u16,
) -> AppResult<u16> {
    if !(1..=100).contains(&limit) {
        return Err(AppError::BadRequest(
            "Invalid native MCP sweep bound".into(),
        ));
    }
    let candidates:Vec<(Uuid,Uuid,Uuid)> = sqlx::query_as("SELECT call.company_id,call.id,call.run_id FROM native_mcp_calls AS call
        JOIN workflow_runs AS run ON run.company_id=call.company_id AND run.id=call.run_id
        WHERE call.maintenance_after<=clock_timestamp() AND run.state IN ('queued','running','waiting')
            AND NOT public.native_mcp_run_live(run.company_id,run.id)
        ORDER BY call.maintenance_after,call.id LIMIT $1")
        .bind(i64::from(limit)).fetch_all(p.pool()).await?;
    let mut retired = 0;
    for (company, call, run) in candidates {
        let result = retire(p, company, run).await;
        if matches!(result, Ok(true)) {
            retired += 1;
        }
        if result.is_err() {
            tracing::warn!(%company,%call,"native MCP retirement failed; delaying this candidate");
        }
        // Separate transaction: a poison cancellation rolls back without erasing its backoff.
        let mut tx = p.pool().begin().await?;
        sqlx::query("SELECT set_config('statement_timeout','250ms',true),set_config('lock_timeout','100ms',true)")
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE native_mcp_calls SET maintenance_after=clock_timestamp()+make_interval(secs=>$3) WHERE company_id=$1 AND id=$2")
            .bind(company).bind(call).bind(if result.is_err(){30_f64}else{1_f64}).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(retired)
}

async fn retire(p: &PostgresPersistence, company: Uuid, run: Uuid) -> AppResult<bool> {
    let mut tx = p.pool().begin().await?;
    sqlx::query("SELECT set_config('statement_timeout','250ms',true),set_config('lock_timeout','100ms',true),set_config('transaction_timeout','500ms',true)")
        .execute(&mut *tx).await?;
    let owned: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2
        AND native_mcp_call_id IS NOT NULL AND state IN ('queued','running','waiting')
        AND NOT public.native_mcp_run_live(company_id,id) FOR UPDATE SKIP LOCKED",
    )
    .bind(company)
    .bind(run)
    .fetch_optional(&mut *tx)
    .await?;
    if owned.is_none() {
        return Ok(false);
    }
    super::super::controls::cancel_run_on(&mut tx, CompanyId::new(company), RunId::new(run))
        .await?;
    tx.commit().await?;
    Ok(true)
}
