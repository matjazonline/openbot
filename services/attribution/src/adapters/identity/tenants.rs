use super::postgres::{IdentityPostgres, database};
use crate::application::{
    freshness::{Observation, Ticket},
    identity::*,
};
use async_trait::async_trait;
use sqlx::{Postgres, Row, Transaction};

/// Every tenant mutation uses this same row lock as lifecycle application.
pub async fn lock_tenant(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    ticket: &Ticket,
) -> Result<()> {
    let row = sqlx::query("SELECT version, state, status_valid_until > clock_timestamp() AS fresh FROM attribution_tenants WHERE company_id = $1 FOR UPDATE")
        .bind(company.0).fetch_optional(&mut **tx).await.map_err(database)?.ok_or(Failure::NotFound)?;
    active(&row, company, ticket)
}
fn active(row: &sqlx::postgres::PgRow, company: CompanyId, ticket: &Ticket) -> Result<()> {
    if row.try_get::<String, _>("state").map_err(database)? != "active" {
        return Err(Failure::Forbidden);
    }
    let version = row.try_get("version").map_err(database)?;
    ticket.check(company, version)?;
    if row.try_get::<Option<bool>, _>("fresh").map_err(database)? != Some(true) {
        return Err(Failure::Unavailable);
    }
    Ok(())
}
#[async_trait]
impl TenantStore for IdentityPostgres {
    async fn health(&self) -> Result<CompanyStatusHealth> {
        let row = sqlx::query("SELECT EXISTS(SELECT 1 FROM attribution_tenants WHERE state = 'active' AND (status_valid_until IS NULL OR status_valid_until <= clock_timestamp())) AS expired, EXISTS(SELECT 1 FROM attribution_tenants WHERE state <> 'deleted' AND last_reconcile_failure IS NOT NULL) AS reconciliation_failed")
            .fetch_one(&self.pool).await.map_err(database)?;
        Ok(CompanyStatusHealth {
            expired: row.try_get("expired").map_err(database)?,
            reconciliation_failed: row.try_get("reconciliation_failed").map_err(database)?,
        })
    }
    async fn observation(&self) -> Result<Observation> {
        let row = sqlx::query("SELECT nextval('attribution_observation_order') AS number, clock_timestamp() AS started_at").fetch_one(&self.pool).await.map_err(database)?;
        Ok(Observation {
            number: row.try_get("number").map_err(database)?,
            started_at: row.try_get("started_at").map_err(database)?,
        })
    }
    async fn observe(
        &self,
        status: &CompanyStatus,
        name: Option<&str>,
        observation: Option<&Observation>,
        claim: Option<&ReconcileClaim>,
    ) -> Result<()> {
        status.validate(status.company_id)?;
        if claim.is_some_and(|claim| claim.company_id != status.company_id) {
            return Err(Failure::Conflict);
        }
        let mut tx = self.pool.begin().await.map_err(database)?;
        sqlx::query("INSERT INTO attribution_tenants (company_id, version, state, changed_at, purge_requested) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (company_id) DO NOTHING")
            .bind(status.company_id.0).bind(status.version).bind(status.state.as_str()).bind(status.changed_at).bind(status.state == LifecycleState::Deleted)
            .execute(&mut *tx).await.map_err(database)?;
        let row = sqlx::query("SELECT version, state, observation_number FROM attribution_tenants WHERE company_id = $1 FOR UPDATE")
            .bind(status.company_id.0).fetch_one(&mut *tx).await.map_err(database)?;
        if let Some(claim) = claim {
            let live: bool = sqlx::query_scalar("SELECT COALESCE(reconcile_execution = $2 AND reconcile_lease_until > clock_timestamp(), false) FROM attribution_tenants WHERE company_id = $1")
                .bind(claim.company_id.0).bind(claim.execution).fetch_one(&mut *tx).await.map_err(database)?;
            if !live {
                return Err(Failure::Conflict);
            }
        }
        let version: i64 = row.try_get("version").map_err(database)?;
        let state = LifecycleState::parse(&row.try_get::<String, _>("state").map_err(database)?)?;
        let previous_observation: i64 = row.try_get("observation_number").map_err(database)?;
        if version == status.version && state != status.state {
            return Err(Failure::Conflict);
        }
        if state == LifecycleState::Deleted && status.state != LifecycleState::Deleted {
            return Err(Failure::Forbidden);
        }
        if status.version < version {
            return tx.commit().await.map_err(database);
        }
        let accepted = observation.filter(|value| value.number > previous_observation);
        let valid_until = accepted.map(|value| value.started_at + chrono::Duration::minutes(5));
        let number = accepted.map(|value| value.number);
        sqlx::query(r#"UPDATE attribution_tenants SET version = $2, state = $3, changed_at = $4,
                display_name = CASE WHEN $3 = 'deleted' THEN NULL WHEN $6::bigint IS NOT NULL THEN COALESCE($5, display_name) ELSE display_name END,
                observation_number = CASE WHEN $3 = 'deleted' THEN 0 ELSE COALESCE($6, observation_number) END,
                status_valid_until = CASE WHEN $3 <> 'active' THEN NULL WHEN $6::bigint IS NOT NULL THEN $7 WHEN version <> $2 THEN NULL ELSE status_valid_until END,
                purge_requested = purge_requested OR $3 = 'deleted'
            WHERE company_id = $1"#)
            .bind(status.company_id.0).bind(status.version).bind(status.state.as_str()).bind(status.changed_at).bind(name).bind(number).bind(valid_until)
            .execute(&mut *tx).await.map_err(database)?;
        tx.commit().await.map_err(database)
    }
    async fn check_tenant(&self, company: CompanyId, ticket: &Ticket) -> Result<()> {
        let row = sqlx::query("SELECT version, state, status_valid_until > clock_timestamp() AS fresh FROM attribution_tenants WHERE company_id = $1")
            .bind(company.0).fetch_optional(&self.pool).await.map_err(database)?.ok_or(Failure::NotFound)?;
        active(&row, company, ticket)
    }
    async fn claim_reconciliation(&self) -> Result<Vec<ReconcileClaim>> {
        let rows = sqlx::query(r#"WITH due AS (
                SELECT company_id FROM attribution_tenants
                WHERE state <> 'deleted' AND reconcile_at <= clock_timestamp()
                    AND (reconcile_lease_until IS NULL OR reconcile_lease_until <= clock_timestamp())
                ORDER BY reconcile_at, company_id LIMIT 4 FOR UPDATE SKIP LOCKED
            ) UPDATE attribution_tenants AS tenant SET reconcile_execution = gen_random_uuid(),
                reconcile_lease_until = clock_timestamp() + interval '30 seconds',
                reconcile_failures = LEAST(tenant.reconcile_failures + 1, 12)
            FROM due WHERE tenant.company_id = due.company_id
            RETURNING tenant.company_id, tenant.reconcile_execution"#)
            .fetch_all(&self.pool).await.map_err(database)?;
        rows.into_iter()
            .map(|row| {
                Ok(ReconcileClaim {
                    company_id: CompanyId(row.try_get("company_id").map_err(database)?),
                    execution: row.try_get("reconcile_execution").map_err(database)?,
                })
            })
            .collect()
    }
    async fn finish_reconciliation(&self, claim: &ReconcileClaim, success: bool) -> Result<()> {
        let rows = sqlx::query(r#"UPDATE attribution_tenants SET reconcile_execution = NULL, reconcile_lease_until = NULL,
                reconcile_at = clock_timestamp() + CASE WHEN $3 THEN interval '2 minutes' ELSE make_interval(secs => LEAST(60, power(2, reconcile_failures)::integer)) END,
                reconcile_failures = CASE WHEN $3 THEN 0 ELSE reconcile_failures END,
                last_reconcile_failure = CASE WHEN $3 THEN NULL ELSE clock_timestamp() END
            WHERE company_id = $1 AND reconcile_execution = $2 AND reconcile_lease_until > clock_timestamp()"#)
            .bind(claim.company_id.0).bind(claim.execution).bind(success).execute(&self.pool).await.map_err(database)?.rows_affected();
        if rows == 0 {
            return Err(Failure::Conflict);
        }
        Ok(())
    }
    async fn cleanup_sessions(&self) -> Result<()> {
        sqlx::query("DELETE FROM attribution_login_transactions WHERE id IN (SELECT id FROM attribution_login_transactions WHERE expires_at <= clock_timestamp() ORDER BY expires_at LIMIT 100 FOR UPDATE SKIP LOCKED)")
            .execute(&self.pool).await.map_err(database)?;
        sqlx::query("DELETE FROM attribution_sessions WHERE id IN (SELECT id FROM attribution_sessions WHERE absolute_expires_at <= clock_timestamp() ORDER BY absolute_expires_at LIMIT 100 FOR UPDATE SKIP LOCKED)")
            .execute(&self.pool).await.map_err(database)?;
        Ok(())
    }
}
