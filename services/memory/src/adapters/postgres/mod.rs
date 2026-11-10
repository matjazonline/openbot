//! PostgreSQL implementation of `Store`. Every transaction follows the D13 lock order:
//! acting admin credential → tenant → namespace → credential (and grants) → schedule →
//! operations (id order) → … → spend rows.
//!
//! Every production statement is a compile-time-checked `sqlx::query*!` macro, verified offline
//! against the committed `.sqlx/` metadata (`cargo sqlx prepare --check` in scripts/verify.sh).
//! Columns of the `access_level` domain are cast to `smallint` on the way in and out, because
//! the macros type-check only the base type.
use crate::{
    application::{
        contract::*,
        ports::{
            Actor, AdminCredentialRecord, ClientCredentialRecord, CredentialPage, NewCredential,
            Store, TenantAction,
        },
        service::{grant_spec, invalid_target},
    },
    domain::{
        access_level::AccessLevel,
        admin_policy::{AdminScope, disabler_after_disable, may_enable, may_manage},
        error::{ErrorCode, ServiceError},
        grant::{NamespaceGrant, Operation},
    },
};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{
    PgConnection, PgPool, Postgres, Transaction,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{collections::BTreeMap, future::Future, str::FromStr, time::Duration};
use uuid::Uuid;

mod generation;
mod memory;
mod recall;
mod retention;
mod search;
#[cfg(test)]
pub mod test_support;

pub use generation::usage_exceeded_reservation_total;
pub use memory::content_sha256;
pub use search::semantic_partial_total;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Clone)]
pub struct PoolSettings {
    pub max_connections: u32,
    pub acquire_timeout: Duration,
    pub statement_timeout: Duration,
}

pub fn connect_options(url: &str, settings: &PoolSettings) -> Result<PgConnectOptions, String> {
    let timeout = settings.statement_timeout.as_millis().to_string();
    PgConnectOptions::from_str(url)
        .map(|options| {
            options
                .application_name("memory-service")
                .options([("statement_timeout", timeout.as_str())])
        })
        .map_err(|_| "MEMORY_DATABASE_URL is not a valid PostgreSQL URL".to_owned())
}

pub fn pool_options(settings: &PoolSettings) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(settings.max_connections)
        .acquire_timeout(settings.acquire_timeout)
}

#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
    exact_scan_cap: i64,
    /// Test-only fault injection: generation lease renewals fail without touching any row.
    #[cfg(test)]
    pub renew_fault: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl PgStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            exact_scan_cap: crate::domain::retrieval::EXACT_SCAN_CAP,
            #[cfg(test)]
            renew_fault: Default::default(),
        }
    }
    /// Lower the exact semantic scan cap (D7); configuration may only lower code maxima.
    pub fn with_exact_scan_cap(mut self, cap: i64) -> Self {
        self.exact_scan_cap = cap.clamp(0, crate::domain::retrieval::EXACT_SCAN_CAP);
        self
    }
    pub fn exact_scan_cap(&self) -> i64 {
        self.exact_scan_cap
    }
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

enum TxError {
    Service(ServiceError),
    Db(sqlx::Error),
}
impl From<sqlx::Error> for TxError {
    fn from(error: sqlx::Error) -> Self {
        Self::Db(error)
    }
}
impl From<ServiceError> for TxError {
    fn from(error: ServiceError) -> Self {
        Self::Service(error)
    }
}
type TxResult<T> = Result<T, TxError>;

/// Deadlocks and serialization failures, and a row that vanished between an `ON CONFLICT DO
/// NOTHING` and the read of the conflicting row (age-based retention can remove an
/// unreferenced source in between, 06A review B1 F2c): the whole transaction re-runs, and its
/// insert then succeeds.
fn retryable(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::RowNotFound)
        || error
            .as_database_error()
            .and_then(|db| db.code())
            .is_some_and(|code| code == "40P01" || code == "40001")
}

fn unavailable(error: sqlx::Error) -> ServiceError {
    tracing::warn!(error = %error, "database operation failed");
    ServiceError::unavailable("The memory database is unavailable")
}

fn service(error: TxError) -> ServiceError {
    match error {
        TxError::Service(error) => error,
        TxError::Db(error) => unavailable(error),
    }
}

/// Retry a whole transaction on deadlock or serialization failure, up to 3 times (D13).
async fn with_retry<T, F, Fut>(run: F) -> Result<T, ServiceError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = TxResult<T>>,
{
    let mut attempt = 0u32;
    loop {
        match run().await {
            Ok(value) => return Ok(value),
            Err(TxError::Service(error)) => return Err(error),
            Err(TxError::Db(error)) if retryable(&error) && attempt < 3 => {
                attempt += 1;
                let jitter = 10 + (Uuid::new_v4().as_u128() % 90) as u64;
                tracing::info!(attempt, "retrying a transaction after a lock conflict");
                tokio::time::sleep(Duration::from_millis(jitter)).await;
            }
            Err(TxError::Db(error)) => return Err(unavailable(error)),
        }
    }
}

fn stored(message: &str) -> TxError {
    TxError::Service(ServiceError::unavailable(message.to_owned()))
}

fn scope_of(value: &str) -> TxResult<AdminScope> {
    AdminScope::parse(value).ok_or_else(|| stored("Stored scope is invalid"))
}

fn tenant_state(value: &str) -> TxResult<TenantState> {
    match value {
        "active" => Ok(TenantState::Active),
        "disabled" => Ok(TenantState::Disabled),
        "deleted" => Ok(TenantState::Deleted),
        _ => Err(stored("Stored tenant state is invalid")),
    }
}

fn level(value: Option<i16>) -> TxResult<Option<AccessLevel>> {
    value
        .map(|value| {
            AccessLevel::new(i64::from(value)).map_err(|_| stored("Stored level is invalid"))
        })
        .transpose()
}

fn conflict(message: &str) -> TxError {
    TxError::Service(ServiceError::new(ErrorCode::Conflict, message))
}

#[allow(clippy::too_many_arguments)]
async fn audit(
    db: &mut PgConnection,
    tenant: Option<Uuid>,
    actor: Actor,
    action: &str,
    target_kind: &str,
    target_id: &str,
    outcome: &str,
    details: serde_json::Value,
) -> TxResult<()> {
    sqlx::query!(
        "INSERT INTO audit_events (id, tenant_id, actor_kind, actor_id, action, target_kind, \
         target_id, outcome, request_id, correlation_id, details) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        Uuid::now_v7(),
        tenant,
        actor.scope.as_str(),
        actor.id,
        action,
        target_kind,
        target_id,
        outcome,
        actor.request_id,
        actor.correlation.as_ref().map(|id| id.as_str()),
        details,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Lock the acting admin credential `FOR SHARE` (D13 level 0) and re-check it. Authentication
/// read it outside this transaction; a revoke or expiry that committed since then wins, and a
/// revoke that commits later waits for this transaction (its update conflicts with the share
/// lock), so no admin write commits after its credential's revocation.
async fn lock_actor(db: &mut PgConnection, actor: Actor) -> TxResult<()> {
    let row = sqlx::query!(
        r#"SELECT revoked_at IS NOT NULL AS "revoked!",
                  coalesce(expires_at <= now(), false) AS "expired!"
             FROM admin_credentials WHERE id = $1 FOR SHARE"#,
        actor.id,
    )
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(|| TxError::Service(ServiceError::unauthenticated()))?;
    if row.revoked {
        Err(TxError::Service(ServiceError::new(
            ErrorCode::CredentialRevoked,
            "The credential was revoked",
        )))
    } else if row.expired {
        Err(TxError::Service(ServiceError::new(
            ErrorCode::CredentialExpired,
            "The credential has expired",
        )))
    } else {
        Ok(())
    }
}

struct TenantDb {
    id: Uuid,
    external_id: String,
    state: String,
    generation_daily_token_cap: i64,
    managed_by: Option<Uuid>,
    disabled_by: Option<Uuid>,
    disabled_by_scope: Option<String>,
}

struct TenantRow {
    id: Uuid,
    external_id: String,
    state: TenantState,
    cap: i64,
    managed_by: Option<Uuid>,
    disabled_by: Option<Uuid>,
    disabled_by_scope: Option<AdminScope>,
}

impl TenantRow {
    fn from_db(row: TenantDb) -> TxResult<Self> {
        Ok(Self {
            id: row.id,
            external_id: row.external_id,
            state: tenant_state(&row.state)?,
            cap: row.generation_daily_token_cap,
            managed_by: row.managed_by,
            disabled_by: row.disabled_by,
            disabled_by_scope: row
                .disabled_by_scope
                .map(|scope| scope_of(&scope))
                .transpose()?,
        })
    }

    fn view(&self, cancelled_operations: u64) -> TenantView {
        TenantView {
            tenant: self.external_id.clone(),
            state: self.state,
            generation_daily_token_cap: self.cap.max(0) as u64,
            disabled_by_scope: self.disabled_by_scope,
            cancelled_operations,
        }
    }
}

#[derive(Clone, Copy)]
enum TenantLock {
    Share,
    NoKeyUpdate,
}

/// Lock the tenant row (D13 level 1) and check the actor may manage it. A tenant outside the
/// actor's authority is indistinguishable from a missing one.
async fn lock_tenant(
    db: &mut PgConnection,
    actor: Actor,
    external_id: &str,
    lock: TenantLock,
) -> TxResult<TenantRow> {
    let row = match lock {
        TenantLock::Share => {
            sqlx::query_as!(
                TenantDb,
                "SELECT id, external_id, state, generation_daily_token_cap, managed_by, \
                 disabled_by, disabled_by_scope FROM tenants WHERE external_id = $1 FOR SHARE",
                external_id,
            )
            .fetch_optional(&mut *db)
            .await?
        }
        TenantLock::NoKeyUpdate => {
            sqlx::query_as!(
                TenantDb,
                "SELECT id, external_id, state, generation_daily_token_cap, managed_by, \
                 disabled_by, disabled_by_scope FROM tenants WHERE external_id = $1 \
                 FOR NO KEY UPDATE",
                external_id,
            )
            .fetch_optional(&mut *db)
            .await?
        }
    };
    let tenant =
        TenantRow::from_db(row.ok_or_else(|| TxError::Service(ServiceError::not_found()))?)?;
    if !may_manage(actor.id, actor.scope, tenant.managed_by) {
        return Err(TxError::Service(ServiceError::not_found()));
    }
    Ok(tenant)
}

fn require_active(tenant: &TenantRow) -> TxResult<()> {
    match tenant.state {
        TenantState::Active => Ok(()),
        TenantState::Disabled => Err(TxError::Service(ServiceError::new(
            ErrorCode::TenantDisabled,
            "The tenant is disabled",
        ))),
        TenantState::Deleted => Err(conflict("The tenant is deleted")),
    }
}

/// Which operations a state change cancels.
#[derive(Clone, Copy)]
enum CancelScope {
    Tenant,
    Namespace(Uuid),
    Credential(Uuid),
    /// One operation (`generations.cancel`).
    Operation(Uuid),
}

/// Cancel `accepted`/`claimed` operations and flag `dispatched` ones (D3, D9). Operation rows are
/// locked first, in id order, so concurrent state changes over overlapping rows (a namespace
/// delete and a credential revoke both hold only tenant `FOR SHARE`) wait instead of
/// deadlocking. Cancelled rows give their reservation back to the spend day they reserved on
/// and keep `reserved_tokens = 0`, so nothing can release it twice; spend rows are updated
/// last, in day order (D13). Dispatched rows keep their reservation: it is charged when the
/// worker or reaper settles them. Returns how many were cancelled outright.
/// Tombstone one active namespace the caller holds `FOR NO KEY UPDATE` (admin delete and client
/// purge, D3/06A A1-2): every credential's grant on it goes, its accepted/claimed operations are
/// cancelled and dispatched ones flagged, then the row becomes the permanent tombstone. Content is
/// removed later by maintenance, once none of its operations is unfinished. Returns the number
/// of cancelled or flagged operations.
async fn tombstone_namespace(
    db: &mut PgConnection,
    tenant: Uuid,
    namespace: Uuid,
) -> TxResult<u64> {
    sqlx::query!(
        "DELETE FROM credential_grants WHERE tenant_id = $1 AND namespace_id = $2",
        tenant,
        namespace,
    )
    .execute(&mut *db)
    .await?;
    let cancelled = cancel_operations(
        db,
        tenant,
        CancelScope::Namespace(namespace),
        "namespace_deleted",
    )
    .await?;
    sqlx::query!(
        "UPDATE namespaces SET state = 'deleted', deleted_at = now() \
         WHERE tenant_id = $1 AND id = $2",
        tenant,
        namespace,
    )
    .execute(&mut *db)
    .await?;
    Ok(cancelled)
}

async fn cancel_operations(
    db: &mut PgConnection,
    tenant: Uuid,
    scope: CancelScope,
    reason: &str,
) -> TxResult<u64> {
    let (namespace, credential, operation) = match scope {
        CancelScope::Tenant => (None, None, None),
        CancelScope::Namespace(id) => (Some(id), None, None),
        CancelScope::Credential(id) => (None, Some(id), None),
        CancelScope::Operation(id) => (None, None, Some(id)),
    };
    let rows = sqlx::query!(
        "SELECT id, state, spend_day, reserved_tokens, cancel_requested_at \
           FROM generation_operations \
          WHERE tenant_id = $1 AND state IN ('accepted', 'claimed', 'dispatched') \
            AND ($2::uuid IS NULL OR namespace_id = $2) \
            AND ($3::uuid IS NULL OR credential_id = $3) \
            AND ($4::uuid IS NULL OR id = $4) \
          ORDER BY id FOR NO KEY UPDATE",
        tenant,
        namespace,
        credential,
        operation,
    )
    .fetch_all(&mut *db)
    .await?;
    let mut cancel = Vec::new();
    let mut flag = Vec::new();
    let mut release: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for row in rows {
        if row.state == "dispatched" {
            if row.cancel_requested_at.is_none() {
                flag.push(row.id);
            }
            continue;
        }
        cancel.push(row.id);
        if let Some(day) = row.spend_day.filter(|_| row.reserved_tokens > 0) {
            *release.entry(day).or_default() += row.reserved_tokens;
        }
    }
    if !cancel.is_empty() {
        sqlx::query!(
            "UPDATE generation_operations SET state = 'cancelled', outcome_code = $3, \
                    finished_at = now(), lease_owner = NULL, lease_until = NULL, \
                    reserved_tokens = 0 \
              WHERE tenant_id = $1 AND id = ANY($2)",
            tenant,
            &cancel,
            reason,
        )
        .execute(&mut *db)
        .await?;
    }
    if !flag.is_empty() {
        // The reason waits in `outcome_code` until the worker or the reaper settles the row.
        sqlx::query!(
            "UPDATE generation_operations SET cancel_requested_at = now(), outcome_code = $3 \
              WHERE tenant_id = $1 AND id = ANY($2)",
            tenant,
            &flag,
            reason,
        )
        .execute(&mut *db)
        .await?;
    }
    for (day, tokens) in release {
        sqlx::query!(
            "UPDATE tenant_spend_days SET reserved = greatest(reserved - $3, 0) \
              WHERE tenant_id = $1 AND day = $2",
            tenant,
            day,
            tokens,
        )
        .execute(&mut *db)
        .await?;
    }
    Ok(cancel.len() as u64)
}

impl PgStore {
    async fn begin(&self) -> TxResult<Transaction<'static, Postgres>> {
        Ok(self.pool.begin().await?)
    }

    /// Move up to `limit` tenants from `from` to the provisioner `to` (06A A1-5). Both admin
    /// rows are taken `FOR SHARE` in id order (a concurrent revoke of `to` either commits first
    /// and the target is refused, or waits). The tenants are locked `FOR NO KEY UPDATE` in id
    /// order without skipping, so a request the old manager already holds a tenant lock for
    /// finishes first; its next request is `not_found`.
    async fn reassign_once(
        &self,
        actor: Actor,
        from: Uuid,
        to: Uuid,
        limit: u32,
    ) -> TxResult<TenantReassignResponse> {
        let mut tx = self.begin().await?;
        lock_actor(&mut tx, actor).await?;
        let mut pair = [from, to];
        pair.sort();
        let rows = sqlx::query!(
            r#"SELECT id, scope, revoked_at IS NOT NULL AS "revoked!",
                      coalesce(expires_at <= now(), false) AS "expired!"
                 FROM admin_credentials WHERE id = ANY($1) ORDER BY id FOR SHARE"#,
            &pair[..],
        )
        .fetch_all(&mut *tx)
        .await?;
        let target = rows.iter().find(|row| row.id == to);
        if !target.is_some_and(|row| row.scope == "provisioner" && !row.revoked && !row.expired) {
            return Err(TxError::Service(invalid_target()));
        }
        let moved = sqlx::query!(
            "SELECT id, external_id FROM tenants WHERE managed_by = $1 \
              ORDER BY id LIMIT $2 FOR NO KEY UPDATE",
            from,
            i64::from(limit),
        )
        .fetch_all(&mut *tx)
        .await?;
        let ids: Vec<Uuid> = moved.iter().map(|tenant| tenant.id).collect();
        if !ids.is_empty() {
            sqlx::query!(
                "UPDATE tenants SET managed_by = $2 WHERE id = ANY($1)",
                &ids,
                to,
            )
            .execute(&mut *tx)
            .await?;
        }
        for tenant in &moved {
            audit(
                &mut tx,
                Some(tenant.id),
                actor,
                "tenant.reassign",
                "tenant",
                &tenant.external_id,
                "ok",
                serde_json::json!({ "from": from, "to": to }),
            )
            .await?;
        }
        let remaining = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM tenants WHERE managed_by = $1) AS "remaining!""#,
            from,
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(TenantReassignResponse {
            moved: moved.len() as u32,
            remaining,
        })
    }

    async fn change_tenant_once(
        &self,
        actor: Actor,
        external_id: &str,
        action: TenantAction,
    ) -> TxResult<TenantView> {
        let mut tx = self.begin().await?;
        lock_actor(&mut tx, actor).await?;
        let mut tenant = lock_tenant(&mut tx, actor, external_id, TenantLock::NoKeyUpdate).await?;
        let mut cancelled = 0;
        let action_name;
        match (action, tenant.state) {
            (TenantAction::Disable | TenantAction::Enable, TenantState::Deleted) => {
                return Err(conflict("The tenant is deleted"));
            }
            (TenantAction::Delete, TenantState::Deleted) => {
                action_name = "tenant.delete.noop";
            }
            (TenantAction::Disable, TenantState::Disabled) => {
                // Re-disable is a no-op: the first disabler (operator scope) stays recorded.
                action_name = "tenant.disable.noop";
            }
            (TenantAction::Disable, TenantState::Active) => {
                let current = tenant.disabled_by.zip(tenant.disabled_by_scope);
                let (by, scope) = disabler_after_disable(current, (actor.id, actor.scope));
                sqlx::query!(
                    "UPDATE tenants SET state = 'disabled', disabled_by = $2, \
                     disabled_by_scope = $3, state_changed_at = now() WHERE id = $1",
                    tenant.id,
                    by,
                    scope.as_str(),
                )
                .execute(&mut *tx)
                .await?;
                cancelled =
                    cancel_operations(&mut tx, tenant.id, CancelScope::Tenant, "tenant_disabled")
                        .await?;
                tenant.state = TenantState::Disabled;
                tenant.disabled_by = Some(by);
                tenant.disabled_by_scope = Some(scope);
                action_name = "tenant.disable";
            }
            (TenantAction::Enable, TenantState::Active) => {
                action_name = "tenant.enable.noop";
            }
            (TenantAction::Enable, TenantState::Disabled) => {
                if !may_enable(actor.scope, tenant.disabled_by_scope) {
                    return Err(TxError::Service(ServiceError::new(
                        ErrorCode::Forbidden,
                        "An operator disabled this tenant; only an operator may enable it",
                    )));
                }
                // Enabling never revives cancelled work; it only flips the state.
                sqlx::query!(
                    "UPDATE tenants SET state = 'active', disabled_by = NULL, \
                     disabled_by_scope = NULL, state_changed_at = now() WHERE id = $1",
                    tenant.id,
                )
                .execute(&mut *tx)
                .await?;
                tenant.state = TenantState::Active;
                tenant.disabled_by = None;
                tenant.disabled_by_scope = None;
                action_name = "tenant.enable";
            }
            (TenantAction::Delete, _) => {
                sqlx::query!(
                    "UPDATE tenants SET state = 'deleted', disabled_by = NULL, \
                     disabled_by_scope = NULL, state_changed_at = now() WHERE id = $1",
                    tenant.id,
                )
                .execute(&mut *tx)
                .await?;
                // 06A A1-1: every namespace becomes a tombstone with the tenant, so one predicate
                // drives content removal. Writers, claims, settles and indexer commits take the
                // tenant FOR SHARE before any namespace (D13), so the tenant lock held here
                // already excludes them; the namespaces are still taken in id order, before
                // the operations (the D13 parent-then-child order, review B1 F2d).
                sqlx::query!(
                    "SELECT id FROM namespaces WHERE tenant_id = $1 AND state = 'active' \
                     ORDER BY id FOR NO KEY UPDATE",
                    tenant.id,
                )
                .fetch_all(&mut *tx)
                .await?;
                cancelled =
                    cancel_operations(&mut tx, tenant.id, CancelScope::Tenant, "tenant_deleted")
                        .await?;
                sqlx::query!(
                    "DELETE FROM credential_grants WHERE tenant_id = $1",
                    tenant.id
                )
                .execute(&mut *tx)
                .await?;
                sqlx::query!(
                    "UPDATE namespaces SET state = 'deleted', deleted_at = now() \
                     WHERE tenant_id = $1 AND state = 'active'",
                    tenant.id,
                )
                .execute(&mut *tx)
                .await?;
                tenant.state = TenantState::Deleted;
                tenant.disabled_by = None;
                tenant.disabled_by_scope = None;
                action_name = "tenant.delete";
            }
        }
        audit(
            &mut tx,
            Some(tenant.id),
            actor,
            action_name,
            "tenant",
            external_id,
            "ok",
            serde_json::json!({ "cancelled_operations": cancelled }),
        )
        .await?;
        tx.commit().await?;
        Ok(tenant.view(cancelled))
    }

    async fn issue_credential_once(
        &self,
        actor: Actor,
        external_id: &str,
        credential: &NewCredential,
    ) -> TxResult<()> {
        let mut tx = self.begin().await?;
        lock_actor(&mut tx, actor).await?;
        let tenant = lock_tenant(&mut tx, actor, external_id, TenantLock::Share).await?;
        require_active(&tenant)?;
        // Namespaces FOR SHARE in id order, re-checking they are active (R2): a concurrent
        // namespace delete either commits first (and this refuses) or waits for this grant and
        // then removes it.
        let names: Vec<String> = credential
            .grants
            .iter()
            .map(|g| g.namespace.clone())
            .collect();
        let rows = sqlx::query!(
            "SELECT id, external_id, state FROM namespaces WHERE tenant_id = $1 \
             AND external_id = ANY($2) ORDER BY id FOR SHARE",
            tenant.id,
            &names,
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut ids = Vec::with_capacity(names.len());
        for grant in &credential.grants {
            let row = rows
                .iter()
                .find(|row| row.external_id == grant.namespace)
                .filter(|row| row.state == "active")
                .ok_or_else(|| {
                    TxError::Service(ServiceError::invalid(format!(
                        "Namespace {} is not active in this tenant",
                        grant.namespace
                    )))
                })?;
            ids.push(row.id);
        }
        sqlx::query!(
            "INSERT INTO credentials (tenant_id, id, label, verifier, expires_at, issued_by) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            tenant.id,
            credential.id,
            credential.label,
            &credential.verifier[..],
            credential.expires_at,
            actor.id,
        )
        .execute(&mut *tx)
        .await?;
        for (grant, namespace_id) in credential.grants.iter().zip(ids) {
            let operations: Vec<String> = grant
                .operations
                .iter()
                .map(|op| op.as_str().to_owned())
                .collect();
            sqlx::query!(
                "INSERT INTO credential_grants (tenant_id, credential_id, namespace_id, read_max, \
                 write_max, operations) VALUES ($1, $2, $3, $4::smallint, $5::smallint, $6)",
                tenant.id,
                credential.id,
                namespace_id,
                grant.read_max.map(AccessLevel::as_i16),
                grant.write_max.map(AccessLevel::as_i16),
                &operations,
            )
            .execute(&mut *tx)
            .await?;
        }
        audit(
            &mut tx,
            Some(tenant.id),
            actor,
            "credential.issue",
            "credential",
            &credential.id.to_string(),
            "ok",
            serde_json::json!({ "grants": credential.grants.len() }),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn grants_for(
        db: &mut PgConnection,
        tenant: Uuid,
        credential: Uuid,
    ) -> TxResult<Vec<NamespaceGrant>> {
        let rows = sqlx::query!(
            "SELECT g.namespace_id, n.external_id, g.read_max::smallint AS read_max, \
                    g.write_max::smallint AS write_max, g.operations \
               FROM credential_grants AS g JOIN namespaces AS n \
                 ON (n.tenant_id, n.id) = (g.tenant_id, g.namespace_id) \
              WHERE g.tenant_id = $1 AND g.credential_id = $2 AND n.state = 'active' \
              ORDER BY n.external_id",
            tenant,
            credential,
        )
        .fetch_all(db)
        .await?;
        rows.into_iter()
            .map(|row| {
                grant_row(
                    row.namespace_id,
                    row.external_id,
                    row.read_max,
                    row.write_max,
                    &row.operations,
                )
            })
            .collect()
    }

    /// The active-namespace grants of a page of credentials in one query (06A A1 F6), keyed
    /// by credential.
    async fn grants_of_page(
        db: &mut PgConnection,
        tenant: Uuid,
        credentials: &[Uuid],
    ) -> TxResult<BTreeMap<Uuid, Vec<NamespaceGrant>>> {
        let rows = sqlx::query!(
            "SELECT g.credential_id, g.namespace_id, n.external_id, \
                    g.read_max::smallint AS read_max, g.write_max::smallint AS write_max, \
                    g.operations \
               FROM credential_grants AS g JOIN namespaces AS n \
                 ON (n.tenant_id, n.id) = (g.tenant_id, g.namespace_id) \
              WHERE g.tenant_id = $1 AND g.credential_id = ANY($2) AND n.state = 'active' \
              ORDER BY g.credential_id, n.external_id",
            tenant,
            credentials,
        )
        .fetch_all(db)
        .await?;
        let mut grants: BTreeMap<Uuid, Vec<NamespaceGrant>> = BTreeMap::new();
        for row in rows {
            grants.entry(row.credential_id).or_default().push(grant_row(
                row.namespace_id,
                row.external_id,
                row.read_max,
                row.write_max,
                &row.operations,
            )?);
        }
        Ok(grants)
    }
}

fn grant_row(
    namespace_id: Uuid,
    namespace: String,
    read_max: Option<i16>,
    write_max: Option<i16>,
    operations: &[String],
) -> TxResult<NamespaceGrant> {
    let operations = operations
        .iter()
        .map(|op| Operation::parse(op).ok_or_else(|| stored("Stored grant is invalid")))
        .collect::<TxResult<_>>()?;
    Ok(NamespaceGrant {
        namespace_id,
        namespace,
        read_max: level(read_max)?,
        write_max: level(write_max)?,
        operations,
    })
}

#[async_trait]
impl Store for PgStore {
    async fn metrics_gauges(&self) -> Result<crate::application::metrics::Gauges, ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            sqlx::query!("SET TRANSACTION READ ONLY")
                .execute(&mut *tx)
                .await?;
            sqlx::query!("SET LOCAL statement_timeout = '2s'")
                .execute(&mut *tx)
                .await?;
            let row = sqlx::query!(
                r#"SELECT
                     (SELECT count(*) FROM generation_operations WHERE state = 'accepted')
                       AS "accepted!",
                     (SELECT count(*) FROM generation_operations WHERE state = 'dispatched')
                       AS "dispatched!",
                     coalesce((SELECT extract(epoch FROM now() - min(created_at))::float8
                                 FROM generation_operations WHERE state = 'accepted'), 0)
                       AS "oldest_accepted!",
                     (SELECT count(*) FROM generation_operations
                       WHERE state = 'unknown' AND finished_at > now() - interval '1 day')
                       AS "unknown!",
                     (SELECT count(DISTINCT tenant_id) FROM generation_operations
                       WHERE state = 'accepted') AS "tenants_waiting!",
                     coalesce((SELECT max(queued) FROM (SELECT count(*) AS queued
                                 FROM generation_operations WHERE state = 'accepted'
                                GROUP BY tenant_id) AS per_tenant), 0) AS "max_tenant_queue!",
                     (SELECT count(*) FROM observations WHERE index_state = 'pending')
                       AS "index_pending!",
                     coalesce((SELECT extract(epoch FROM now() - min(updated_at))::float8
                                 FROM observations WHERE index_state = 'pending'), 0)
                       AS "oldest_index_pending!",
                     (SELECT count(*) FROM observations WHERE index_state = 'failed')
                       AS "index_failed!""#
            )
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(crate::application::metrics::Gauges {
                accepted: row.accepted,
                dispatched: row.dispatched,
                oldest_accepted_seconds: row.oldest_accepted,
                unknown_last_day: row.unknown,
                tenants_waiting: row.tenants_waiting,
                max_tenant_queue: row.max_tenant_queue,
                index_pending: row.index_pending,
                oldest_index_pending_seconds: row.oldest_index_pending,
                index_failed: row.index_failed,
            })
        })
        .await
    }

    async fn ping(&self) -> Result<(), ServiceError> {
        sqlx::query_scalar!(r#"SELECT 1 AS "one!""#)
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(unavailable)
    }

    async fn admin_credential(
        &self,
        id: Uuid,
    ) -> Result<Option<AdminCredentialRecord>, ServiceError> {
        let row = sqlx::query!(
            "SELECT id, scope, verifier, expires_at, revoked_at FROM admin_credentials WHERE id = $1",
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?;
        row.map(|row| {
            Ok(AdminCredentialRecord {
                id: row.id,
                scope: scope_of(&row.scope).map_err(service)?,
                verifier: row.verifier,
                expires_at: row.expires_at,
                revoked_at: row.revoked_at,
            })
        })
        .transpose()
    }

    async fn client_credential(
        &self,
        id: Uuid,
    ) -> Result<Option<ClientCredentialRecord>, ServiceError> {
        with_retry(|| async {
            let mut db = self.pool.acquire().await?;
            let Some(row) = sqlx::query!(
                "SELECT c.id, c.tenant_id, t.external_id, t.state, c.label, c.verifier, \
                        c.expires_at, c.revoked_at \
                   FROM credentials AS c JOIN tenants AS t ON t.id = c.tenant_id WHERE c.id = $1",
                id,
            )
            .fetch_optional(&mut *db)
            .await?
            else {
                return Ok(None);
            };
            let grants = Self::grants_for(&mut db, row.tenant_id, id).await?;
            Ok(Some(ClientCredentialRecord {
                id: row.id,
                tenant_id: row.tenant_id,
                tenant: row.external_id,
                tenant_state: tenant_state(&row.state)?,
                label: row.label,
                verifier: row.verifier,
                expires_at: row.expires_at,
                revoked_at: row.revoked_at,
                grants,
            }))
        })
        .await
    }

    async fn bootstrap_operator(
        &self,
        id: Uuid,
        label: &str,
        verifier: [u8; 32],
    ) -> Result<(), ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            sqlx::query_scalar!(
                r#"SELECT true AS "locked!"
                     FROM pg_advisory_xact_lock(hashtext('memory-service.bootstrap'))"#
            )
            .fetch_one(&mut *tx)
            .await?;
            // Only a usable operator blocks bootstrap: if every operator credential was revoked
            // or has expired, bootstrap recovers the service without database surgery.
            let usable = sqlx::query_scalar!(
                r#"SELECT count(*) AS "count!" FROM admin_credentials
                    WHERE scope = 'operator' AND revoked_at IS NULL
                      AND (expires_at IS NULL OR expires_at > now())"#
            )
            .fetch_one(&mut *tx)
            .await?;
            if usable > 0 {
                return Err(conflict("A usable operator credential already exists"));
            }
            sqlx::query!(
                "INSERT INTO admin_credentials (id, scope, label, verifier) \
                 VALUES ($1, 'operator', $2, $3)",
                id,
                label,
                &verifier[..],
            )
            .execute(&mut *tx)
            .await?;
            let actor = Actor {
                id,
                scope: AdminScope::Operator,
                request_id: None,
                correlation: None,
            };
            audit(
                &mut tx,
                None,
                actor,
                "admin.bootstrap",
                "admin_credential",
                &id.to_string(),
                "ok",
                serde_json::json!({}),
            )
            .await?;
            tx.commit().await?;
            Ok(())
        })
        .await
    }

    async fn issue_admin_credential(
        &self,
        actor: Actor,
        id: Uuid,
        scope: AdminScope,
        label: &str,
        expires_at: Option<DateTime<Utc>>,
        verifier: [u8; 32],
    ) -> Result<(), ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            lock_actor(&mut tx, actor).await?;
            sqlx::query!(
                "INSERT INTO admin_credentials (id, scope, label, verifier, expires_at, created_by) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
                id,
                scope.as_str(),
                label,
                &verifier[..],
                expires_at,
                actor.id,
            )
            .execute(&mut *tx)
            .await?;
            audit(
                &mut tx,
                None,
                actor,
                "admin_credential.issue",
                "admin_credential",
                &id.to_string(),
                "ok",
                serde_json::json!({ "scope": scope.as_str() }),
            )
            .await?;
            tx.commit().await?;
            Ok(())
        })
        .await
    }

    async fn revoke_admin_credential(
        &self,
        actor: Actor,
        id: Uuid,
    ) -> Result<AdminCredentialView, ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            // Two operators revoking each other can deadlock here (each holds its own row
            // FOR SHARE); the retry then sees the winner's revocation.
            lock_actor(&mut tx, actor).await?;
            let scope = sqlx::query_scalar!(
                "UPDATE admin_credentials SET revoked_at = coalesce(revoked_at, now()) \
                 WHERE id = $1 RETURNING scope",
                id,
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| TxError::Service(ServiceError::not_found()))?;
            let scope = scope_of(&scope)?;
            audit(
                &mut tx,
                None,
                actor,
                "admin_credential.revoke",
                "admin_credential",
                &id.to_string(),
                "ok",
                serde_json::json!({}),
            )
            .await?;
            tx.commit().await?;
            Ok(AdminCredentialView {
                credential_id: id,
                scope,
                revoked: true,
            })
        })
        .await
    }

    async fn create_tenant(&self, actor: Actor, tenant: &str) -> Result<TenantView, ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            lock_actor(&mut tx, actor).await?;
            let id = Uuid::now_v7();
            let inserted = sqlx::query_scalar!(
                "INSERT INTO tenants (id, external_id, managed_by) VALUES ($1, $2, $3) \
                 ON CONFLICT (external_id) DO NOTHING RETURNING id",
                id,
                tenant,
                actor.id,
            )
            .fetch_optional(&mut *tx)
            .await?;
            if inserted.is_none() {
                // Creation is idempotent by external id: a retry by an admin that may manage the
                // live tenant gets its current view. Tenant ids are one global namespace, so an
                // id that is tombstoned or managed by another admin is refused with one uniform
                // conflict that does not say which.
                let existing = sqlx::query_as!(
                    TenantDb,
                    "SELECT id, external_id, state, generation_daily_token_cap, managed_by, \
                     disabled_by, disabled_by_scope FROM tenants WHERE external_id = $1 FOR SHARE",
                    tenant,
                )
                .fetch_one(&mut *tx)
                .await?;
                let existing = TenantRow::from_db(existing)?;
                if existing.state == TenantState::Deleted
                    || !may_manage(actor.id, actor.scope, existing.managed_by)
                {
                    return Err(conflict("The tenant id is unavailable"));
                }
                audit(
                    &mut tx,
                    Some(existing.id),
                    actor,
                    "tenant.create.noop",
                    "tenant",
                    tenant,
                    "ok",
                    serde_json::json!({}),
                )
                .await?;
                tx.commit().await?;
                return Ok(existing.view(0));
            }
            sqlx::query!(
                "INSERT INTO tenant_generation_schedule (tenant_id) VALUES ($1)",
                id,
            )
            .execute(&mut *tx)
            .await?;
            audit(
                &mut tx,
                Some(id),
                actor,
                "tenant.create",
                "tenant",
                tenant,
                "ok",
                serde_json::json!({}),
            )
            .await?;
            tx.commit().await?;
            Ok(TenantView {
                tenant: tenant.to_owned(),
                state: TenantState::Active,
                generation_daily_token_cap: 0,
                disabled_by_scope: None,
                cancelled_operations: 0,
            })
        })
        .await
    }

    async fn change_tenant(
        &self,
        actor: Actor,
        tenant: &str,
        action: TenantAction,
    ) -> Result<TenantView, ServiceError> {
        with_retry(|| self.change_tenant_once(actor, tenant, action)).await
    }

    async fn set_tenant_cap(
        &self,
        actor: Actor,
        tenant: &str,
        cap: u64,
    ) -> Result<TenantView, ServiceError> {
        let cap =
            i64::try_from(cap).map_err(|_| ServiceError::invalid("The token cap is too large"))?;
        with_retry(|| async {
            let mut tx = self.begin().await?;
            lock_actor(&mut tx, actor).await?;
            let mut row = lock_tenant(&mut tx, actor, tenant, TenantLock::NoKeyUpdate).await?;
            if row.state == TenantState::Deleted {
                return Err(conflict("The tenant is deleted"));
            }
            sqlx::query!(
                "UPDATE tenants SET generation_daily_token_cap = $2 WHERE id = $1",
                row.id,
                cap,
            )
            .execute(&mut *tx)
            .await?;
            row.cap = cap;
            audit(
                &mut tx,
                Some(row.id),
                actor,
                "tenant.settings",
                "tenant",
                tenant,
                "ok",
                serde_json::json!({ "generation_daily_token_cap": cap }),
            )
            .await?;
            tx.commit().await?;
            Ok(row.view(0))
        })
        .await
    }

    async fn create_namespace(
        &self,
        actor: Actor,
        tenant: &str,
        namespace: &str,
    ) -> Result<NamespaceView, ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            lock_actor(&mut tx, actor).await?;
            let row = lock_tenant(&mut tx, actor, tenant, TenantLock::Share).await?;
            require_active(&row)?;
            let inserted = sqlx::query_scalar!(
                "INSERT INTO namespaces (tenant_id, id, external_id) VALUES ($1, $2, $3) \
                 ON CONFLICT (tenant_id, external_id) DO NOTHING RETURNING id",
                row.id,
                Uuid::now_v7(),
                namespace,
            )
            .fetch_optional(&mut *tx)
            .await?;
            let action = if inserted.is_some() {
                "namespace.create"
            } else {
                // Idempotent by external id within the tenant; a tombstone is never recreated.
                let state = sqlx::query_scalar!(
                    "SELECT state FROM namespaces WHERE tenant_id = $1 AND external_id = $2",
                    row.id,
                    namespace,
                )
                .fetch_one(&mut *tx)
                .await?;
                if state == "deleted" {
                    return Err(TxError::Service(ServiceError::new(
                        ErrorCode::NamespaceDeleted,
                        "The namespace was deleted and cannot be recreated",
                    )));
                }
                "namespace.create.noop"
            };
            audit(
                &mut tx,
                Some(row.id),
                actor,
                action,
                "namespace",
                namespace,
                "ok",
                serde_json::json!({}),
            )
            .await?;
            tx.commit().await?;
            Ok(NamespaceView {
                tenant: tenant.to_owned(),
                namespace: namespace.to_owned(),
                state: NamespaceState::Active,
            })
        })
        .await
    }

    async fn delete_namespace(
        &self,
        actor: Actor,
        tenant: &str,
        namespace: &str,
    ) -> Result<NamespaceView, ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            lock_actor(&mut tx, actor).await?;
            let row = lock_tenant(&mut tx, actor, tenant, TenantLock::Share).await?;
            let found = sqlx::query!(
                "SELECT id, state FROM namespaces WHERE tenant_id = $1 AND external_id = $2 \
                 FOR NO KEY UPDATE",
                row.id,
                namespace,
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| TxError::Service(ServiceError::not_found()))?;
            if found.state == "active" {
                let cancelled = tombstone_namespace(&mut tx, row.id, found.id).await?;
                audit(
                    &mut tx,
                    Some(row.id),
                    actor,
                    "namespace.delete",
                    "namespace",
                    namespace,
                    "ok",
                    serde_json::json!({ "cancelled_operations": cancelled }),
                )
                .await?;
            }
            tx.commit().await?;
            Ok(NamespaceView {
                tenant: tenant.to_owned(),
                namespace: namespace.to_owned(),
                state: NamespaceState::Deleted,
            })
        })
        .await
    }

    async fn issue_credential(
        &self,
        actor: Actor,
        tenant: &str,
        credential: NewCredential,
    ) -> Result<(), ServiceError> {
        with_retry(|| self.issue_credential_once(actor, tenant, &credential)).await
    }

    async fn list_credentials(
        &self,
        actor: Actor,
        tenant: &str,
        page: CredentialPage<'_>,
    ) -> Result<CredentialList, ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            lock_actor(&mut tx, actor).await?;
            // Any tenant state: a manager may list the credentials of its own tombstone, which
            // is how a client proves it manages a deleted tenant (06A A3 F5).
            let row = lock_tenant(&mut tx, actor, tenant, TenantLock::Share).await?;
            let rows = sqlx::query!(
                "SELECT id, label, expires_at, revoked_at FROM credentials \
                  WHERE tenant_id = $1 AND ($2::text IS NULL OR label = $2) \
                    AND (NOT $3 OR (revoked_at IS NULL \
                                    AND (expires_at IS NULL OR expires_at > now()))) \
                    AND ($4::uuid IS NULL OR id > $4) \
                  ORDER BY id LIMIT $5",
                row.id,
                page.label,
                page.active_only,
                page.after,
                i64::from(page.limit),
            )
            .fetch_all(&mut *tx)
            .await?;
            let ids: Vec<Uuid> = rows.iter().map(|credential| credential.id).collect();
            let mut grants = Self::grants_of_page(&mut tx, row.id, &ids).await?;
            let next_after = (ids.len() == page.limit as usize)
                .then(|| ids.last().copied())
                .flatten();
            let credentials = rows
                .into_iter()
                .map(|credential| CredentialView {
                    tenant: tenant.to_owned(),
                    credential_id: credential.id,
                    label: credential.label,
                    expires_at: credential.expires_at,
                    revoked_at: credential.revoked_at,
                    grants: grants
                        .remove(&credential.id)
                        .unwrap_or_default()
                        .iter()
                        .map(grant_spec)
                        .collect(),
                })
                .collect();
            tx.commit().await?;
            Ok(CredentialList {
                credentials,
                next_after,
            })
        })
        .await
    }

    async fn reassign_tenants(
        &self,
        actor: Actor,
        from: Uuid,
        to: Uuid,
        limit: u32,
    ) -> Result<TenantReassignResponse, ServiceError> {
        with_retry(|| self.reassign_once(actor, from, to, limit)).await
    }

    async fn revoke_credential(
        &self,
        actor: Actor,
        tenant: &str,
        id: Uuid,
    ) -> Result<CredentialView, ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            lock_actor(&mut tx, actor).await?;
            let row = lock_tenant(&mut tx, actor, tenant, TenantLock::Share).await?;
            let credential = sqlx::query!(
                "SELECT label, expires_at, revoked_at FROM credentials \
                 WHERE tenant_id = $1 AND id = $2 FOR NO KEY UPDATE",
                row.id,
                id,
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| TxError::Service(ServiceError::not_found()))?;
            let mut revoked_at = credential.revoked_at;
            if revoked_at.is_none() {
                revoked_at = Some(
                    sqlx::query_scalar!(
                        r#"UPDATE credentials SET revoked_at = now()
                            WHERE tenant_id = $1 AND id = $2 RETURNING revoked_at AS "revoked_at!""#,
                        row.id,
                        id,
                    )
                    .fetch_one(&mut *tx)
                    .await?,
                );
                let cancelled = cancel_operations(
                    &mut tx,
                    row.id,
                    CancelScope::Credential(id),
                    "credential_revoked",
                )
                .await?;
                audit(
                    &mut tx,
                    Some(row.id),
                    actor,
                    "credential.revoke",
                    "credential",
                    &id.to_string(),
                    "ok",
                    serde_json::json!({ "cancelled_operations": cancelled }),
                )
                .await?;
            }
            let grants = Self::grants_for(&mut tx, row.id, id).await?;
            tx.commit().await?;
            Ok(CredentialView {
                tenant: tenant.to_owned(),
                credential_id: id,
                label: credential.label,
                expires_at: credential.expires_at,
                revoked_at,
                grants: grants.iter().map(grant_spec).collect(),
            })
        })
        .await
    }
}

#[cfg(test)]
#[path = "authority_tests.rs"]
mod authority_tests;
#[cfg(test)]
#[path = "generation_tests.rs"]
mod generation_tests;
#[cfg(test)]
#[path = "index_tests.rs"]
mod index_tests;
#[cfg(test)]
#[path = "migration_tests.rs"]
mod migration_tests;
#[cfg(test)]
#[path = "observability_tests.rs"]
mod observability_tests;
#[cfg(test)]
#[path = "operations_tests.rs"]
mod operations_tests;
#[cfg(test)]
#[path = "recall_tests.rs"]
mod recall_tests;
#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;
#[cfg(test)]
#[path = "storage_tests.rs"]
mod storage_tests;
