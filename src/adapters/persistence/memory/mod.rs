//! OpenBot's side of the memory service: connection state, per-namespace client credentials and
//! the remote lifecycle (memory decisions E1–E2). The memory corpus is never stored here.
mod capture;
pub(crate) mod deliveries;
pub(crate) mod inventory;
mod lifecycle;
mod namespaces;
mod retention;
mod tenants;

pub(crate) use namespaces::retire_namespaces_on;
pub(crate) use tenants::retire_company_on;

use crate::app_error::{AppError, AppResult};
use uuid::Uuid;

/// A remote lifecycle in one of `states`, or stuck in an outage (at least
/// `STUCK_TRANSIENT_RETRIES` outage retries, review p7b r2 m2): reported at startup, so
/// neither an unset service URL nor a terminal failure strands remote data silently (E1, review
/// p7b r1 M2). `docs/deploy.md` ("Memory service") gives the query and the manual re-drive.
#[derive(Debug, sqlx::FromRow)]
pub struct PendingMemoryLifecycle {
    pub id: Uuid,
    pub company_id: Uuid,
    pub resource: String,
    pub desired: String,
    pub state: String,
    pub outcome_code: Option<String>,
}

/// At most `limit` unfinished rows plus whether more exist.
pub async fn pending_lifecycles(
    pool: &sqlx::PgPool,
    states: &[&str],
    limit: i64,
) -> AppResult<(Vec<PendingMemoryLifecycle>, bool)> {
    let mut rows = sqlx::query_as::<_, PendingMemoryLifecycle>(
        "SELECT lifecycle.id, lifecycle.company_id, lifecycle.resource, lifecycle.desired, \
           lifecycle.state, lifecycle.outcome_code \
         FROM memory_lifecycles AS lifecycle \
         WHERE lifecycle.state = ANY($2) \
            OR (lifecycle.state IN ('pending', 'claimed') AND lifecycle.transient_retries >= $3) \
         ORDER BY lifecycle.id LIMIT $1",
    )
    .bind(limit + 1)
    .bind(states)
    .bind(crate::application::memory::STUCK_TRANSIENT_RETRIES)
    .fetch_all(pool)
    .await?;
    let more = rows.len() as i64 > limit;
    rows.truncate(limit as usize);
    Ok((rows, more))
}

/// Whether the company has memory enabled now; a missing tenant row is not enabled (U1).
pub(crate) async fn memory_enabled_on(
    db: &mut sqlx::PgConnection,
    company: crate::domain::workflow::CompanyId,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM memory_tenants AS tenant \
         WHERE tenant.company_id = $1 AND tenant.state = 'enabled')",
    )
    .bind(company.as_uuid())
    .fetch_one(db)
    .await?)
}

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod delivery_tests;
#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[cfg(test)]
#[path = "live_tests.rs"]
pub(crate) mod live_tests;
#[cfg(test)]
#[path = "retention_tests.rs"]
mod retention_tests;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
