//! Memory delivery intents (decisions E2, E6, E8, E9): the save freeze inside the producing
//! run's transaction, the fenced delivery queue, and the settlements disable and owner removal
//! make. Lock order for a freeze: the caller's prelocks and run lock → `memory_tenants` FOR
//! SHARE → `memory_namespaces` FOR SHARE → the intent. Disable locks the tenant FOR UPDATE and
//! then the intents; owner removal locks its namespaces and then their intents. Neither takes a
//! run lock, so no transaction waits run → tenant against tenant → run.
use super::super::PostgresPersistence;
use super::super::credentials::CredentialContext;
use crate::app_error::{AppError, AppResult};
use crate::application::memory::*;
use crate::domain::workflow::{CompanyId, ExecutionId, RunId};
use crate::entities::access_level::{
    AccessLevel, ExactnessTarget, HumanInputClass, human_input_exactness,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::time::Duration;
use uuid::Uuid;

/// The frozen request bound below the service's 4 MiB body (E4).
pub(crate) const MAX_FROZEN_SAVE_BYTES: usize = 3_670_016;
const MAX_TITLE_CHARS: usize = 256;
const MAX_BACKOFF: Duration = Duration::from_secs(300);

fn invalid() -> AppError {
    AppError::Internal("Invalid memory delivery state".into())
}

/// Which namespace a save targets. Steps have company and agent scope only; the save tool may
/// also name the user scope of a private session (E2), resolved by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveScope {
    Company,
    Agent(Uuid),
    User(Uuid),
}
impl SaveScope {
    fn columns(self) -> (&'static str, Option<Uuid>, Option<Uuid>) {
        match self {
            Self::Company => ("company", None, None),
            Self::Agent(agent) => ("agent", Some(agent), None),
            Self::User(user) => ("user", None, Some(user)),
        }
    }
    fn exactness_target(self) -> ExactnessTarget {
        match self {
            Self::Company => ExactnessTarget::CompanyScope,
            Self::Agent(_) => ExactnessTarget::AgentScope,
            Self::User(_) => ExactnessTarget::UserScope,
        }
    }
}

/// One logical save: its origin, its scope, its deterministic key and the facts to freeze.
pub(crate) struct SaveIntent<'a> {
    pub company: CompanyId,
    pub run: RunId,
    pub execution: ExecutionId,
    pub invocation: Option<Uuid>,
    pub wait: Option<Uuid>,
    pub scope: SaveScope,
    pub key: &'a str,
    pub facts: &'a [String],
    pub external_sources: Vec<ExternalSourceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveEnqueue {
    /// Committed with the caller's transaction; `replayed` when this key already had an intent.
    Enqueued { id: Uuid, replayed: bool },
    /// Memory is off for the company (terminal `memory.not_enabled`).
    NotEnabled,
    /// The scope's namespace is not provisioned yet; provision outside this transaction.
    NotReady,
    /// The frozen request would exceed the bound (terminal `memory.save_too_large`).
    TooLarge,
}

/// The producing run's facts, read from its locked row (E6).
#[derive(Debug, Clone, Copy)]
pub(crate) struct LockedRun {
    pub ceiling: AccessLevel,
    pub content: AccessLevel,
    pub content_exact: bool,
}

#[derive(sqlx::FromRow)]
struct LockedRunRow {
    knowledge_ceiling: i16,
    content_level: i16,
    content_exact: bool,
}

/// The consumed evidence a save sends (E3): the run's level while it is exact and the scope
/// accepts authored configuration as exact, else `unknown`, the producing ceiling's fallback.
pub(crate) fn consumed_for(
    content: AccessLevel,
    content_exact: bool,
    scope: SaveScope,
) -> ConsumedEvidence {
    let configuration_exact = human_input_exactness(
        HumanInputClass::AuthoredConfiguration,
        scope.exactness_target(),
    );
    if content_exact && configuration_exact {
        ConsumedEvidence::Exact {
            level: content.as_i16() as u8,
        }
    } else {
        ConsumedEvidence::Unknown
    }
}

/// The title is the fact's first line, cut at a character boundary (E4).
pub(crate) fn fact_title(fact: &str) -> String {
    let line = fact.lines().next().unwrap_or_default().trim();
    let line = if line.is_empty() { "Memory" } else { line };
    line.chars().take(MAX_TITLE_CHARS).collect()
}

fn micros(at: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    use chrono::DurationRound;
    at.duration_trunc(chrono::Duration::microseconds(1))
        .unwrap_or(at)
}

/// Freeze one save in the caller's live ownership/run transaction (E6). The caller has taken
/// its prelocks and run lock; the levels are re-read from the locked run row, never from a
/// claim or an earlier read. Nothing here contacts the service or provisions a namespace.
pub(crate) async fn enqueue_save_on(
    db: &mut PgConnection,
    intent: &SaveIntent<'_>,
) -> AppResult<SaveEnqueue> {
    let run = sqlx::query_as::<_, LockedRunRow>(
        "SELECT run.knowledge_ceiling::smallint AS knowledge_ceiling, \
           run.content_level::smallint AS content_level, run.content_exact \
         FROM workflow_runs AS run WHERE run.company_id = $1 AND run.id = $2 FOR UPDATE",
    )
    .bind(intent.company.as_uuid())
    .bind(intent.run.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(|| AppError::NotFound("Workflow run".into()))?;
    let run = LockedRun {
        ceiling: AccessLevel::try_from(run.knowledge_ceiling).map_err(|_| invalid())?,
        content: AccessLevel::try_from(run.content_level).map_err(|_| invalid())?,
        content_exact: run.content_exact,
    };
    freeze_save_on(db, intent, run).await
}

/// The freeze after the run read: tenant FOR SHARE (enabled, its epoch) → namespace FOR SHARE
/// (ready) → the intent with its frozen body.
pub(crate) async fn freeze_save_on(
    db: &mut PgConnection,
    intent: &SaveIntent<'_>,
    run: LockedRun,
) -> AppResult<SaveEnqueue> {
    let company = intent.company.as_uuid();
    let (ceiling, content) = (run.ceiling, run.content);
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT delivery.id FROM memory_deliveries AS delivery \
         WHERE delivery.company_id = $1 AND delivery.idempotency_key = $2",
    )
    .bind(company)
    .bind(intent.key)
    .fetch_optional(&mut *db)
    .await?;
    if let Some(id) = existing {
        // A retry replays its frozen intent and label; it never freezes again.
        return Ok(SaveEnqueue::Enqueued { id, replayed: true });
    }
    let tenant: Option<(String, i64)> = sqlx::query_as(
        "SELECT tenant.state, tenant.disable_epoch FROM memory_tenants AS tenant \
         WHERE tenant.company_id = $1 FOR SHARE",
    )
    .bind(company)
    .fetch_optional(&mut *db)
    .await?;
    let Some((state, epoch)) = tenant else {
        return Ok(SaveEnqueue::NotEnabled);
    };
    if state != "enabled" {
        return Ok(SaveEnqueue::NotEnabled);
    }
    let (kind, agent, user) = intent.scope.columns();
    let namespace: Option<Uuid> = sqlx::query_scalar(
        "SELECT namespace.id FROM memory_namespaces AS namespace \
         WHERE namespace.company_id = $1 AND namespace.scope_kind = $2 \
           AND namespace.agent_id IS NOT DISTINCT FROM $3 \
           AND namespace.user_id IS NOT DISTINCT FROM $4 AND namespace.state = 'ready' \
         FOR SHARE",
    )
    .bind(company)
    .bind(kind)
    .bind(agent)
    .bind(user)
    .fetch_optional(&mut *db)
    .await?;
    let Some(namespace) = namespace else {
        return Ok(SaveEnqueue::NotReady);
    };
    if let Some(above) = intent
        .external_sources
        .iter()
        .map(|source| source.access_level)
        .max()
        .filter(|level| i16::from(*level) > ceiling.as_i16())
    {
        // Known content above the producing ceiling refuses (E3); it is never stored lower.
        return Err(AppError::KnowledgeRelease {
            content: AccessLevel::try_from(i16::from(above)).map_err(|_| invalid())?,
            destination: ceiling,
        });
    }
    let now = micros(chrono::Utc::now());
    let request = MemorySaveRequest {
        idempotency_key: intent.key.to_owned(),
        expires_at: now + SAVE_INTENT_LIFETIME,
        namespace: namespace.hyphenated().to_string(),
        context: ProducingContext {
            consumed: consumed_for(content, run.content_exact, intent.scope),
            producing_ceiling: ceiling.as_i16() as u8,
            external_sources: intent.external_sources.clone(),
        },
        items: intent
            .facts
            .iter()
            .map(|fact| MemorySaveItem {
                kind: "fact".into(),
                title: fact_title(fact),
                body: fact.clone(),
                occurred_at: now,
            })
            .collect(),
    };
    let frozen = json!({"version": "1", "body": request});
    if frozen.to_string().len() > MAX_FROZEN_SAVE_BYTES {
        return Ok(SaveEnqueue::TooLarge);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO memory_deliveries (company_id, id, namespace_id, kind, run_id, execution_id, \
           invocation_id, wait_id, idempotency_key, expires_at, request, disable_epoch, state, \
           available_at) \
         VALUES ($1, $2, $3, 'save', $4, $5, $6, $7, $8, $9, $10, $11, 'pending', clock_timestamp())",
    )
    .bind(company)
    .bind(id)
    .bind(namespace)
    .bind(intent.run.as_uuid())
    .bind(intent.execution.as_uuid())
    .bind(intent.invocation)
    .bind(intent.wait)
    .bind(intent.key)
    .bind(request.expires_at)
    .bind(&frozen)
    .bind(epoch)
    .execute(&mut *db)
    .await?;
    Ok(SaveEnqueue::Enqueued {
        id,
        replayed: false,
    })
}

/// The service's window bound on all event texts (`ActivityEvent.text`, 512 KiB).
pub(crate) const MAX_CAPTURE_TEXT_BYTES: usize = 512 * 1024;

/// One accepted exchange window to capture (E8): its origin, the policy kind and target scope
/// eligibility chose, and the logical input and accepted output as text.
pub(crate) struct CaptureIntent<'a> {
    pub company: CompanyId,
    pub run: RunId,
    pub execution: ExecutionId,
    pub exchange: Uuid,
    pub agent: Uuid,
    pub kind: MemoryCaptureKind,
    pub scope: SaveScope,
    pub input: &'a str,
    pub output: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureEnqueue {
    Enqueued {
        id: Uuid,
        replayed: bool,
    },
    /// Nothing captured, for this traced reason.
    Skipped(&'static str),
}

#[derive(sqlx::FromRow)]
struct CapturePolicyRow {
    enabled: bool,
    allowance_tokens: i64,
    max_output_tokens: i32,
    expiry_hours: i32,
}

/// The capture in the accepting transaction (E8): the levels are re-read from the locked run
/// row, as a save's are (E6).
pub(crate) async fn enqueue_capture_on(
    db: &mut PgConnection,
    intent: &CaptureIntent<'_>,
) -> AppResult<CaptureEnqueue> {
    let run = sqlx::query_as::<_, LockedRunRow>(
        "SELECT run.knowledge_ceiling::smallint AS knowledge_ceiling, \
           run.content_level::smallint AS content_level, run.content_exact \
         FROM workflow_runs AS run WHERE run.company_id = $1 AND run.id = $2 FOR UPDATE",
    )
    .bind(intent.company.as_uuid())
    .bind(intent.run.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(|| AppError::NotFound("Workflow run".into()))?;
    let run = LockedRun {
        ceiling: AccessLevel::try_from(run.knowledge_ceiling).map_err(|_| invalid())?,
        content: AccessLevel::try_from(run.content_level).map_err(|_| invalid())?,
        content_exact: run.content_exact,
    };
    freeze_capture_on(db, intent, run).await
}

/// The capture freeze after the run read (E2, E8). Lock order: run → `memory_tenants` FOR SHARE
/// (enabled, epoch, operator cap) → `memory_namespaces` FOR SHARE (ready) →
/// `memory_agent_capture` FOR SHARE (the switch) → the intent. A disable, removal or switch
/// change racing it therefore either waits for this commit and then settles the intent, or
/// commits first and this skips. A replayed acceptance finds the key and freezes nothing again.
pub(crate) async fn freeze_capture_on(
    db: &mut PgConnection,
    intent: &CaptureIntent<'_>,
    run: LockedRun,
) -> AppResult<CaptureEnqueue> {
    let company = intent.company.as_uuid();
    let key = format!("ob.g.{}", intent.exchange.simple());
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT delivery.id FROM memory_deliveries AS delivery \
         WHERE delivery.company_id = $1 AND delivery.idempotency_key = $2",
    )
    .bind(company)
    .bind(&key)
    .fetch_optional(&mut *db)
    .await?;
    if let Some(id) = existing {
        return Ok(CaptureEnqueue::Enqueued { id, replayed: true });
    }
    let (ceiling, content) = (run.ceiling, run.content);
    let tenant: Option<(String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT tenant.state, tenant.disable_epoch, tenant.generation_cap \
         FROM memory_tenants AS tenant WHERE tenant.company_id = $1 FOR SHARE",
    )
    .bind(company)
    .fetch_optional(&mut *db)
    .await?;
    let Some((state, epoch, cap)) = tenant else {
        return Ok(CaptureEnqueue::Skipped("not_enabled"));
    };
    if state != "enabled" {
        return Ok(CaptureEnqueue::Skipped("not_enabled"));
    }
    if cap.unwrap_or(0) == 0 {
        // The operator's tenant cap (cached at the last create) leaves generation off.
        return Ok(CaptureEnqueue::Skipped("generation_off"));
    }
    let (kind, agent, user) = intent.scope.columns();
    let namespace: Option<Uuid> = sqlx::query_scalar(
        "SELECT namespace.id FROM memory_namespaces AS namespace \
         WHERE namespace.company_id = $1 AND namespace.scope_kind = $2 \
           AND namespace.agent_id IS NOT DISTINCT FROM $3 \
           AND namespace.user_id IS NOT DISTINCT FROM $4 AND namespace.state = 'ready' \
         FOR SHARE",
    )
    .bind(company)
    .bind(kind)
    .bind(agent)
    .bind(user)
    .fetch_optional(&mut *db)
    .await?;
    let Some(namespace) = namespace else {
        return Ok(CaptureEnqueue::Skipped("not_ready"));
    };
    let policy = sqlx::query_as::<_, CapturePolicyRow>(
        "SELECT policy.enabled, policy.allowance_tokens, policy.max_output_tokens, \
           policy.expiry_hours FROM memory_agent_capture AS policy \
         WHERE policy.company_id = $1 AND policy.agent_id = $2 AND policy.kind = $3 FOR SHARE",
    )
    .bind(company)
    .bind(intent.agent)
    .bind(intent.kind.as_str())
    .fetch_optional(&mut *db)
    .await?;
    let Some(policy) = policy.filter(|policy| policy.enabled) else {
        return Ok(CaptureEnqueue::Skipped("off"));
    };
    if content.as_i16() > ceiling.as_i16() {
        // Never labelled lower than what the window carries (E3); the service would refuse it.
        return Ok(CaptureEnqueue::Skipped("above_ceiling"));
    }
    if intent.input.len() + intent.output.len() > MAX_CAPTURE_TEXT_BYTES {
        return Ok(CaptureEnqueue::Skipped("too_large"));
    }
    let now = micros(chrono::Utc::now());
    let level = content.as_i16() as u8;
    let exchange = intent.exchange.simple();
    // Each part is its own source: the service refuses one (source_ref, revision) carrying two
    // different texts in a window (409), so input and output never share one.
    let event = |role: MemoryEventRole, part: &str, text: &str| MemoryActivityEvent {
        event_id: format!("ob.e.{exchange}.{part}"),
        source_ref: format!("openbot.exchange.{exchange}.{part}"),
        revision: "1".into(),
        occurred_at: now,
        access_level: level,
        role,
        text: text.to_owned(),
    };
    let request = MemoryGenerationRequest {
        idempotency_key: key.clone(),
        expires_at: now + chrono::Duration::hours(i64::from(policy.expiry_hours)),
        namespace: namespace.hyphenated().to_string(),
        producing_ceiling: ceiling.as_i16() as u8,
        consumed: consumed_for(content, run.content_exact, intent.scope),
        events: vec![
            event(MemoryEventRole::User, "input", intent.input),
            event(MemoryEventRole::Assistant, "output", intent.output),
        ],
        allowance_tokens: u64::try_from(policy.allowance_tokens).map_err(|_| invalid())?,
        max_output_tokens: u32::try_from(policy.max_output_tokens).map_err(|_| invalid())?,
    };
    let frozen = json!({"version": "1", "body": request});
    if frozen.to_string().len() > MAX_FROZEN_SAVE_BYTES {
        return Ok(CaptureEnqueue::Skipped("too_large"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO memory_deliveries (company_id, id, namespace_id, kind, run_id, execution_id, \
           exchange_id, idempotency_key, expires_at, request, disable_epoch, state, available_at) \
         VALUES ($1, $2, $3, 'generation', $4, $5, $6, $7, $8, $9, $10, 'pending', \
           clock_timestamp())",
    )
    .bind(company)
    .bind(id)
    .bind(namespace)
    .bind(intent.run.as_uuid())
    .bind(intent.execution.as_uuid())
    .bind(intent.exchange)
    .bind(&key)
    .bind(request.expires_at)
    .bind(&frozen)
    .bind(epoch)
    .execute(&mut *db)
    .await?;
    Ok(CaptureEnqueue::Enqueued {
        id,
        replayed: false,
    })
}

/// Disable (E2, E9), after its `memory_tenants … FOR UPDATE` and epoch bump: every pending or
/// in-flight intent fails as `disabled`, so its wait wakes with a terminal failure. An
/// in-flight attempt's fenced completion then finds nothing to complete.
pub(crate) async fn fail_disabled_on(db: &mut PgConnection, company: Uuid) -> AppResult<u64> {
    settle_open_on(db, company, None, "failed", "disabled").await
}

/// Owner removal (E9), after its namespace rows moved to `removing`: their open intents are
/// cancelled in the owner's transaction.
pub(crate) async fn cancel_removed_on(
    db: &mut PgConnection,
    company: Uuid,
    namespaces: &[Uuid],
) -> AppResult<u64> {
    settle_open_on(db, company, Some(namespaces), "cancelled", "owner_removed").await
}

/// Settles every open intent of a company (or of its given namespaces). The rows are locked in
/// id order before the update, like the run-cancel trigger, so a disable, an owner removal and a
/// run cancel settling overlapping intents wait on each other instead of deadlocking. A settled
/// intent drops its frozen body (05A.6).
async fn settle_open_on(
    db: &mut PgConnection,
    company: Uuid,
    namespaces: Option<&[Uuid]>,
    state: &str,
    outcome: &str,
) -> AppResult<u64> {
    let settled = sqlx::query(
        "WITH open AS (SELECT delivery.company_id, delivery.id FROM memory_deliveries AS delivery \
           WHERE delivery.company_id = $1 AND delivery.state IN ('pending', 'claimed') \
             AND ($2::uuid[] IS NULL OR delivery.namespace_id = ANY($2)) \
           ORDER BY delivery.id FOR UPDATE) \
         UPDATE memory_deliveries AS delivery SET state = $3, outcome_code = $4, request = NULL, \
           lease_owner = NULL, lease_until = NULL, updated_at = clock_timestamp() \
         FROM open WHERE delivery.company_id = open.company_id AND delivery.id = open.id",
    )
    .bind(company)
    .bind(namespaces)
    .bind(state)
    .bind(outcome)
    .execute(db)
    .await?;
    Ok(settled.rows_affected())
}

#[derive(sqlx::FromRow)]
struct DueRow {
    company_id: Uuid,
    id: Uuid,
    kind: String,
    state: String,
    attempts: i32,
    transient_retries: i32,
    request: Option<Value>,
    expired: bool,
    epoch_current: bool,
    namespace_id: Uuid,
    credential_id: Option<Uuid>,
    credential_envelope: Option<String>,
}

fn backoff(retries: i32, retry_after: Option<Duration>) -> Duration {
    Duration::from_secs(1 << retries.clamp(1, 9))
        .max(retry_after.unwrap_or_default())
        .min(MAX_BACKOFF)
}

impl PostgresPersistence {
    async fn settle_claimed_on(
        db: &mut PgConnection,
        row: &DueRow,
        state: &str,
        outcome: &str,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE memory_deliveries AS delivery SET state = $3, outcome_code = $4, \
               request = NULL, lease_owner = NULL, lease_until = NULL, \
               updated_at = clock_timestamp() \
             WHERE delivery.company_id = $1 AND delivery.id = $2",
        )
        .bind(row.company_id)
        .bind(row.id)
        .bind(state)
        .bind(outcome)
        .execute(db)
        .await?;
        Ok(())
    }

    /// The intent's credential and frozen request. `Ok(None)` when the frozen body is not a
    /// valid request (a terminal `invalid_intent`); `Err` when the envelope cannot be opened.
    fn open_request(&self, row: &DueRow) -> AppResult<Option<(ClientCredential, FrozenRequest)>> {
        let (Some(envelope), Some(credential_id), Some(frozen)) = (
            row.credential_envelope.as_deref(),
            row.credential_id,
            row.request.as_ref(),
        ) else {
            return Ok(None);
        };
        if frozen["version"] != json!("1") {
            return Ok(None);
        }
        // The intent's kind decides how its frozen body decodes; any mismatch is invalid.
        let body = frozen["body"].clone();
        let request = match row.kind.as_str() {
            "save" => serde_json::from_value(body).ok().map(FrozenRequest::Save),
            "generation" => serde_json::from_value(body)
                .ok()
                .map(FrozenRequest::Generation),
            _ => None,
        };
        let Some(request) = request else {
            return Ok(None);
        };
        let token = self.credential_cipher()?.open_envelope(
            &CredentialContext::memory_client_credential(row.company_id, row.namespace_id),
            envelope,
        )?;
        Ok(Some((
            ClientCredential {
                tenant: TenantExternalId(row.company_id),
                id: credential_id,
                token,
            },
            request,
        )))
    }
}

#[async_trait]
impl MemoryDeliveryStore for PostgresPersistence {
    async fn claim_delivery(
        &self,
        worker: Uuid,
        lease: Duration,
    ) -> AppResult<DeliveryClaimOutcome> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '2s'")
            .execute(&mut *tx)
            .await?;
        // One due intent: pending and due, or claimed with an expired lease. The tenant and
        // namespace are read, not locked, so a disable or removal waits only on this row. A
        // company with a save in flight (a live lease) is skipped, so its backlog holds at most
        // one lane and other companies' intents are claimed in the rest (F7).
        let row = sqlx::query_as::<_, DueRow>(
            "SELECT delivery.company_id, delivery.id, delivery.kind, delivery.state, delivery.attempts, \
               delivery.transient_retries, delivery.request, \
               delivery.expires_at <= clock_timestamp() AS expired, \
               COALESCE(tenant.state = 'enabled' AND tenant.disable_epoch = delivery.disable_epoch, false) \
                 AS epoch_current, \
               delivery.namespace_id, \
               CASE WHEN namespace.state = 'ready' THEN namespace.credential_id END \
                 AS credential_id, \
               CASE WHEN namespace.state = 'ready' THEN namespace.credential_envelope END \
                 AS credential_envelope \
             FROM memory_deliveries AS delivery \
             LEFT JOIN memory_tenants AS tenant ON tenant.company_id = delivery.company_id \
             LEFT JOIN memory_namespaces AS namespace ON namespace.company_id = delivery.company_id \
               AND namespace.id = delivery.namespace_id \
             WHERE ((delivery.state = 'pending' AND delivery.available_at <= clock_timestamp()) \
                OR (delivery.state = 'claimed' AND delivery.lease_until <= clock_timestamp())) \
             AND NOT EXISTS (SELECT 1 FROM memory_deliveries AS busy \
               WHERE busy.company_id = delivery.company_id AND busy.state = 'claimed' \
                 AND busy.lease_until > clock_timestamp()) \
             ORDER BY delivery.available_at, delivery.id LIMIT 1 \
             FOR UPDATE OF delivery SKIP LOCKED",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(DeliveryClaimOutcome::Empty);
        };
        let reclaimed = row.state == "claimed";
        let settled = if row.expired {
            Some(("expired", "expired"))
        } else if !row.epoch_current {
            // A stale epoch never silently skips: it fails, which wakes the wait (E2).
            Some(("failed", "disabled"))
        } else if reclaimed && row.attempts + 1 >= MAX_DELIVERY_ATTEMPTS {
            Some(("failed", "lease_expired"))
        } else if row.credential_envelope.is_none() {
            // Removed without its owner hook (or the hook raced the claim): nothing can serve it.
            Some(("failed", "namespace_unavailable"))
        } else {
            None
        };
        if let Some((state, outcome)) = settled {
            Self::settle_claimed_on(&mut tx, &row, state, outcome).await?;
            tx.commit().await?;
            tracing::info!(company_id = %row.company_id, delivery_id = %row.id, state, outcome,
                "Memory delivery settled at claim");
            return Ok(DeliveryClaimOutcome::Settled);
        }
        let (credential, request) = match self.open_request(&row) {
            Ok(Some(opened)) => opened,
            Ok(None) => {
                Self::settle_claimed_on(&mut tx, &row, "failed", "invalid_intent").await?;
                tx.commit().await?;
                return Ok(DeliveryClaimOutcome::Settled);
            }
            Err(error) => {
                // An envelope this process cannot open would fail every pass and head the
                // queue forever (F6): it settles terminally and the next intent is served.
                Self::settle_claimed_on(&mut tx, &row, "failed", "credential_unavailable").await?;
                tx.commit().await?;
                tracing::warn!(company_id = %row.company_id, delivery_id = %row.id,
                    error = %error, "Memory delivery credential unavailable; intent failed");
                return Ok(DeliveryClaimOutcome::Settled);
            }
        };
        let fence: i64 = sqlx::query_scalar(
            "UPDATE memory_deliveries AS delivery SET state = 'claimed', \
               fence = delivery.fence + 1, lease_owner = $3, \
               lease_until = clock_timestamp() + $4 * interval '1 millisecond', \
               attempts = delivery.attempts + CASE WHEN delivery.state = 'claimed' THEN 1 ELSE 0 END, \
               updated_at = clock_timestamp() \
             WHERE delivery.company_id = $1 AND delivery.id = $2 RETURNING delivery.fence",
        )
        .bind(row.company_id)
        .bind(row.id)
        .bind(worker)
        .bind(i64::try_from(lease.as_millis()).map_err(|_| invalid())?)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(DeliveryClaimOutcome::Claimed(Box::new(DeliveryClaim {
            company: row.company_id,
            id: row.id,
            fence,
            worker,
            transient_retries: row.transient_retries,
            credential,
            request,
        })))
    }

    async fn complete_delivery(
        &self,
        claim: &DeliveryClaim,
        result: DeliveryResult,
    ) -> AppResult<DeliveryCompletion> {
        let (state, outcome, receipt, delay, retried) = match &result {
            DeliveryResult::Delivered(receipt) => (
                "delivered",
                None,
                Some(serde_json::to_value(receipt).map_err(|_| invalid())?),
                Duration::ZERO,
                0,
            ),
            DeliveryResult::Retry { retry_after, .. } => (
                "pending",
                None,
                None,
                backoff(claim.transient_retries + 1, *retry_after),
                1,
            ),
            DeliveryResult::Failed(code) => ("failed", Some(*code), None, Duration::ZERO, 0),
            DeliveryResult::Expired => ("expired", Some("expired"), None, Duration::ZERO, 0),
        };
        // Fenced: a moved fence (lease expiry, disable, owner removal) completes nothing. Bounded
        // like the claim (review MEM-P05-B2-F16): a settlement holding this row would otherwise
        // stall every lane behind the worker's store mutex; on the timeout the attempt is left to
        // lease-expiry reclaim, which resends the same key and body.
        let mut tx = self.pool().begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '2s'")
            .execute(&mut *tx)
            .await?;
        let changed = sqlx::query(
            "UPDATE memory_deliveries AS delivery SET state = $5, outcome_code = $6, result = $7, \
               request = CASE WHEN $5 = 'pending' THEN delivery.request END, \
               available_at = clock_timestamp() + $8 * interval '1 millisecond', \
               transient_retries = delivery.transient_retries + $9, \
               lease_owner = NULL, lease_until = NULL, updated_at = clock_timestamp() \
             WHERE delivery.company_id = $1 AND delivery.id = $2 AND delivery.state = 'claimed' \
               AND delivery.fence = $3 AND delivery.lease_owner = $4",
        )
        .bind(claim.company)
        .bind(claim.id)
        .bind(claim.fence)
        .bind(claim.worker)
        .bind(state)
        .bind(outcome)
        .bind(receipt)
        .bind(i64::try_from(delay.as_millis()).map_err(|_| invalid())?)
        .bind(retried)
        .execute(&mut *tx)
        .await;
        let changed = match changed {
            Ok(changed) => changed,
            Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("55P03") => {
                tracing::warn!(company_id = %claim.company, delivery_id = %claim.id,
                    "Memory delivery completion timed out on a row lock; left to lease reclaim");
                return Ok(DeliveryCompletion::Lost);
            }
            Err(error) => return Err(error.into()),
        };
        tx.commit().await?;
        Ok(if changed.rows_affected() == 1 {
            DeliveryCompletion::Applied
        } else {
            DeliveryCompletion::Lost
        })
    }
}
