//! Generation operations in PostgreSQL (04B, D9, D13): durable acceptance with the window
//! recorded as source revisions, status and cancel, and the worker protocol (fair claim with
//! spend reservation, the dispatch compare-and-swap, lease renewal under the fence, settlement
//! in one transaction, the reaper) plus the retention sweep of generation inputs.
use super::{
    CancelScope, PgStore, TxError, TxResult, cancel_operations, conflict,
    memory::{
        Link, claim, encode, finish, insert_links, kind_str, level, lock_writer, not_found, parse,
        require_session, watermark_str,
    },
    stored, with_retry,
};
use crate::{
    application::{
        contract::{EventRole, GenerationState, GenerationStatus},
        generation::{GeneratedItem, PreparedEvent, PreparedGeneration, StoredWindow, restore},
        metrics::METRICS,
        ports::{
            Aged, ClientRead, ClientWrite, Dispatch, GenerationJob, GenerationStore, ProviderUsage,
            Reaped, Removed, Settled, Settlement, Swept,
        },
    },
    domain::{
        access_level::{AccessLevel, WatermarkKind},
        error::{ErrorCode, ServiceError},
        generation::{MAX_DISPATCH_ATTEMPTS, RETENTION_DAYS, RETRY_BACKOFF_SECS},
    },
};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use std::{
    collections::{BTreeMap, HashMap},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use uuid::Uuid;

static USAGE_EXCEEDED: AtomicU64 = AtomicU64::new(0);

/// `generation_usage_exceeded_reservation_total` (D9): settlements whose reported usage was
/// above the reservation. It is the early signal that the reserve formula no longer bounds the
/// structured-output overhead; exported with the 06A metrics.
pub fn usage_exceeded_reservation_total() -> u64 {
    USAGE_EXCEEDED.load(Ordering::Relaxed)
}

/// The stored, immutable request: the window's event list in order. Event text lives in the
/// linked source revisions.
#[derive(Debug, Serialize, Deserialize)]
struct StoredRequest {
    v: u32,
    description: Option<String>,
    events: Vec<StoredEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredEvent {
    event_id: String,
    source_id: Uuid,
    role: EventRole,
    occurred_at: DateTime<Utc>,
}

fn seconds(duration: Duration) -> f64 {
    duration.as_secs_f64()
}

fn service_error(code: ErrorCode, message: &str, reason: &str) -> TxError {
    TxError::Service(ServiceError::new(code, message).with_details(json!({ "reason": reason })))
}

fn watermark_kind(value: &str) -> TxResult<WatermarkKind> {
    match value {
        "exact" => Ok(WatermarkKind::Exact),
        "ceiling_fallback" => Ok(WatermarkKind::CeilingFallback),
        _ => Err(stored("Stored watermark kind is invalid")),
    }
}

fn status_of(state: &str) -> TxResult<GenerationState> {
    parse(state)
}

/// Replace a reservation on `day` with `used` tokens (0 releases it). Spend rows are the last
/// level of D13.
async fn settle_spend(
    db: &mut PgConnection,
    tenant: Uuid,
    day: Option<NaiveDate>,
    reserved: i64,
    used: i64,
) -> TxResult<()> {
    let Some(day) = day else {
        return Ok(());
    };
    if reserved == 0 && used == 0 {
        return Ok(());
    }
    sqlx::query!(
        "UPDATE tenant_spend_days SET reserved = greatest(reserved - $3, 0), used = used + $4 \
          WHERE tenant_id = $1 AND day = $2",
        tenant,
        day,
        reserved,
        used,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Finish an operation this transaction holds locked: terminal `state` with `code`, the lease
/// cleared, `charged` tokens moved from the reservation to usage and the rest released.
#[allow(clippy::too_many_arguments)]
async fn finish_operation(
    db: &mut PgConnection,
    tenant: Uuid,
    operation: Uuid,
    state: &str,
    code: &str,
    day: Option<NaiveDate>,
    reserved: i64,
    charged: i64,
) -> TxResult<()> {
    sqlx::query!(
        "UPDATE generation_operations SET state = $3, outcome_code = $4, finished_at = now(), \
                lease_owner = NULL, lease_until = NULL, reserved_tokens = 0, \
                used_tokens = used_tokens + $5 \
          WHERE tenant_id = $1 AND id = $2",
        tenant,
        operation,
        state,
        code,
        charged,
    )
    .execute(&mut *db)
    .await?;
    settle_spend(db, tenant, day, reserved, charged).await
}

/// Provider usage by token kind (06A A2-1).
fn count_usage(usage: &ProviderUsage) {
    for (kind, tokens) in [
        ("input", usage.input_tokens),
        ("output", usage.output_tokens),
        ("cache_read", usage.cache_read_input_tokens),
        ("cache_write", usage.cache_creation_input_tokens),
    ] {
        METRICS.inc("memory_provider_tokens_total", &[("kind", kind)], tokens);
    }
}

async fn system_audit(
    db: &mut PgConnection,
    tenant: Uuid,
    worker: Uuid,
    operation: Uuid,
    outcome: &str,
    details: Value,
) -> TxResult<()> {
    sqlx::query!(
        "INSERT INTO audit_events (id, tenant_id, actor_kind, actor_id, action, target_kind, \
         target_id, outcome, correlation_id, details) \
         SELECT $1, $2, 'system', $3, 'generation.settle', 'generation', $4, $5, \
                (SELECT correlation_id FROM generation_operations WHERE tenant_id = $2 AND id = $7), \
                $6",
        Uuid::now_v7(),
        tenant,
        worker,
        operation.to_string(),
        outcome,
        details,
        operation,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Why the parents of an operation no longer authorize it, re-checked at claim, dispatch and
/// commit (D13): the tenant, namespace and credential rows are locked `FOR SHARE` (with
/// `SKIP LOCKED` at claim, where a row being changed is simply skipped).
struct Authority {
    stop: Option<(&'static str, &'static str)>,
}

async fn recheck_authority(
    db: &mut PgConnection,
    tenant: Uuid,
    namespace: Uuid,
    credential: Uuid,
    input_level: i16,
    skip_locked: bool,
) -> TxResult<Option<Authority>> {
    let tenant_state = if skip_locked {
        sqlx::query_scalar!(
            "SELECT state FROM tenants WHERE id = $1 FOR SHARE SKIP LOCKED",
            tenant
        )
        .fetch_optional(&mut *db)
        .await?
    } else {
        sqlx::query_scalar!("SELECT state FROM tenants WHERE id = $1 FOR SHARE", tenant)
            .fetch_optional(&mut *db)
            .await?
    };
    let Some(tenant_state) = tenant_state else {
        return Ok(None);
    };
    let namespace_state = if skip_locked {
        sqlx::query_scalar!(
            "SELECT state FROM namespaces WHERE tenant_id = $1 AND id = $2 FOR SHARE SKIP LOCKED",
            tenant,
            namespace,
        )
        .fetch_optional(&mut *db)
        .await?
    } else {
        sqlx::query_scalar!(
            "SELECT state FROM namespaces WHERE tenant_id = $1 AND id = $2 FOR SHARE",
            tenant,
            namespace,
        )
        .fetch_optional(&mut *db)
        .await?
    };
    let Some(namespace_state) = namespace_state else {
        return Ok(None);
    };
    let credential_row = if skip_locked {
        sqlx::query!(
            r#"SELECT revoked_at IS NOT NULL AS "revoked!",
                      coalesce(expires_at <= now(), false) AS "expired!"
                 FROM credentials WHERE tenant_id = $1 AND id = $2 FOR SHARE SKIP LOCKED"#,
            tenant,
            credential,
        )
        .fetch_optional(&mut *db)
        .await?
        .map(|row| (row.revoked, row.expired))
    } else {
        sqlx::query!(
            r#"SELECT revoked_at IS NOT NULL AS "revoked!",
                      coalesce(expires_at <= now(), false) AS "expired!"
                 FROM credentials WHERE tenant_id = $1 AND id = $2 FOR SHARE"#,
            tenant,
            credential,
        )
        .fetch_optional(&mut *db)
        .await?
        .map(|row| (row.revoked, row.expired))
    };
    let Some((revoked, expired)) = credential_row else {
        return Ok(None);
    };
    // The grant is part of the credential level; it is never updated in place, only removed
    // by a namespace delete, which also cancels the namespace's operations.
    let grant = sqlx::query!(
        r#"SELECT write_max::smallint AS write_max, 'generate' = ANY(operations) AS "generate!"
             FROM credential_grants WHERE tenant_id = $1 AND credential_id = $2 AND namespace_id = $3"#,
        tenant,
        credential,
        namespace,
    )
    .fetch_optional(&mut *db)
    .await?;
    let authorized = grant
        .as_ref()
        .is_some_and(|g| g.generate && g.write_max.is_some_and(|max| max >= input_level));
    let stop = match (tenant_state.as_str(), namespace_state.as_str()) {
        ("disabled", _) => Some(("cancelled", "tenant_disabled")),
        ("deleted", _) => Some(("cancelled", "tenant_deleted")),
        (_, "deleted") => Some(("cancelled", "namespace_deleted")),
        _ if revoked => Some(("cancelled", "credential_revoked")),
        _ if expired => Some(("cancelled", "credential_expired")),
        _ if !authorized => Some(("cancelled", "authority_changed")),
        _ => None,
    };
    Ok(Some(Authority { stop }))
}

impl PgStore {
    async fn accept_once(
        &self,
        write: &ClientWrite,
        prepared: &PreparedGeneration,
    ) -> TxResult<Value> {
        let mut tx = self.begin().await?;
        lock_writer(&mut tx, write).await?;
        if let Some(replay) = claim(&mut tx, write, &[]).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        // The operator's daily cap (default 0) is the switch that keeps generation off.
        let cap = sqlx::query_scalar!(
            "SELECT generation_daily_token_cap FROM tenants WHERE id = $1",
            write.tenant_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        if cap == 0 {
            return Err(service_error(
                ErrorCode::GenerationDisabled,
                "Generation is disabled for this tenant",
                "daily_cap_zero",
            ));
        }
        let reservation = prepared.reserve_tokens as i64;
        if !prepared.events.is_empty() && reservation > cap {
            return Err(TxError::Service(
                ServiceError::new(
                    ErrorCode::SpendExhausted,
                    "The request's reservation exceeds the tenant's daily cap",
                )
                .with_details(json!({
                    "reason": "reservation_above_daily_cap",
                    "reserve_tokens": prepared.reserve_tokens,
                })),
            ));
        }
        require_session(&mut tx, write, prepared.session_id).await?;
        // Record the window as source revisions, one per (source_ref, revision), in one
        // order so concurrent acceptances lock in the same sequence. An identical stored
        // revision (same redacted content, level and session) is reused under `FOR SHARE`; a
        // different one conflicts, and one whose content retention nulled or that expired
        // refuses the request: an incomplete window is never accepted.
        let mut revisions: BTreeMap<(&str, &str), &PreparedEvent> = BTreeMap::new();
        for event in &prepared.events {
            revisions
                .entry((event.source_ref.as_str(), event.revision.as_str()))
                .or_insert(event);
        }
        let mut source_ids: HashMap<(&str, &str), Uuid> = HashMap::new();
        for ((reference, revision), event) in &revisions {
            let digest: [u8; 32] = Sha256::digest(event.text.as_bytes()).into();
            let inserted = sqlx::query_scalar!(
                "INSERT INTO sources (tenant_id, namespace_id, id, external_ref, revision, \
                 access_level, session_id, content, content_sha256, redaction_version, occurred_at, \
                 recorded_by) \
                 VALUES ($1, $2, $3, $4, $5, $6::smallint, $7, $8, $9, $10, $11, $12) \
                 ON CONFLICT (tenant_id, namespace_id, external_ref, revision) DO NOTHING RETURNING id",
                write.tenant_id,
                write.namespace_id,
                Uuid::now_v7(),
                reference,
                revision,
                event.access_level.as_i16(),
                prepared.session_id,
                event.text,
                &digest[..],
                prepared.redaction_version,
                event.occurred_at,
                write.credential_id,
            )
            .fetch_optional(&mut *tx)
            .await?;
            let id = match inserted {
                Some(id) => id,
                None => {
                    let existing = sqlx::query!(
                        r#"SELECT id, access_level::smallint AS "level!", content_sha256, session_id,
                                  recorded_by, content IS NULL AS "nulled!",
                                  (expires_at IS NOT NULL AND expires_at <= now()) AS "expired!"
                             FROM sources WHERE tenant_id = $1 AND namespace_id = $2
                              AND external_ref = $3 AND revision = $4 FOR SHARE"#,
                        write.tenant_id,
                        write.namespace_id,
                        reference,
                        revision,
                    )
                    .fetch_one(&mut *tx)
                    .await?;
                    // Above the caller's read grant (D3 allows write grants above read
                    // grants): only the recording credential may reuse it; anyone else gets a
                    // uniform conflict, whatever the content.
                    let probe = write
                        .read_max
                        .is_none_or(|max| existing.level > max.as_i16())
                        && existing.recorded_by != Some(write.credential_id);
                    if probe
                        || existing.level != event.access_level.as_i16()
                        || existing.content_sha256 != digest
                        || existing.session_id != prepared.session_id
                    {
                        return Err(conflict(
                            "A window revision was already recorded with other content, level or session",
                        ));
                    }
                    if existing.nulled || existing.expired {
                        return Err(service_error(
                            ErrorCode::Conflict,
                            "A window revision's recorded content is no longer available",
                            "source_unavailable",
                        ));
                    }
                    existing.id
                }
            };
            source_ids.insert((reference, revision), id);
        }
        let operation = Uuid::now_v7();
        let request = StoredRequest {
            v: 1,
            description: prepared.description.clone(),
            events: prepared
                .events
                .iter()
                .map(|event| StoredEvent {
                    event_id: event.event_id.clone(),
                    source_id: source_ids[&(event.source_ref.as_str(), event.revision.as_str())],
                    role: event.role,
                    occurred_at: event.occurred_at,
                })
                .collect(),
        };
        // Privacy left nothing to send: finished at once as a skip, and never dispatched.
        let (state, outcome) = if prepared.events.is_empty() {
            (GenerationState::Completed, Some("nothing_to_process"))
        } else {
            (GenerationState::Accepted, None)
        };
        let state_text = match state {
            GenerationState::Completed => "completed",
            _ => "accepted",
        };
        let label = prepared.label;
        sqlx::query!(
            "INSERT INTO generation_operations (tenant_id, namespace_id, id, credential_id, \
             idempotency_key, session_id, summary, state, outcome_code, request, producing_ceiling, \
             watermark_kind, watermark_level, source_max_level, input_level, allowance_tokens, \
             max_output_tokens, reservation, expires_at, finished_at, correlation_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::smallint, $12, $13::smallint, \
                     $14::smallint, $15::smallint, $16, $17, $18, $19, \
                     CASE WHEN $8 = 'completed' THEN now() END, $20)",
            write.tenant_id,
            write.namespace_id,
            operation,
            write.credential_id,
            write.key,
            prepared.session_id,
            prepared.summary,
            state_text,
            outcome,
            encode(&request)?,
            label.producing_ceiling().as_i16(),
            watermark_str(label.watermark_kind()),
            label.watermark_level().as_i16(),
            label.source_max_level().as_i16(),
            label.level().as_i16(),
            prepared.allowance_tokens as i64,
            prepared.max_output_tokens as i32,
            reservation,
            prepared.expires_at,
            write.correlation.as_ref().map(|id| id.as_str()),
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(db)
                if db.constraint() == Some("generation_operations_one_per_key") =>
            {
                TxError::Service(ServiceError::new(
                    ErrorCode::IdempotencyConflict,
                    "The idempotency key was used for a different request",
                ))
            }
            _ => error.into(),
        })?;
        let mut linked: Vec<Uuid> = source_ids.values().copied().collect();
        linked.sort();
        sqlx::query!(
            "INSERT INTO generation_operation_sources (tenant_id, namespace_id, operation_id, source_id) \
             SELECT $1, $2, $3, u.id FROM UNNEST($4::uuid[]) AS u(id)",
            write.tenant_id,
            write.namespace_id,
            operation,
            &linked,
        )
        .execute(&mut *tx)
        .await?;
        let result = encode(&GenerationStatus {
            operation_id: operation,
            state,
            outcome: outcome.map(str::to_owned),
            observations: Vec::new(),
            replayed: false,
        })?;
        let details = json!({
            "events": prepared.events.len(),
            "sources": linked.len(),
            "dropped": prepared.dropped,
            "redactions": prepared.redactions,
            "access_level": label.level(),
            "reserve_tokens": prepared.reserve_tokens,
        });
        finish(
            &mut tx,
            write,
            "generation.request",
            "generation",
            &operation.to_string(),
            &result,
            details,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn status_once(&self, read: &ClientRead, operation: Uuid) -> TxResult<GenerationStatus> {
        let mut tx = self.begin().await?;
        sqlx::query!("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query!(
            "SELECT o.state, o.outcome_code FROM generation_operations AS o \
               JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id) \
              WHERE o.tenant_id = $1 AND o.id = $2 AND o.namespace_id = ANY($3) \
                AND n.state = 'active' AND o.input_level <= $4::smallint",
            read.tenant_id,
            operation,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(not_found)?;
        let observations = sqlx::query_scalar!(
            "SELECT id FROM observations WHERE tenant_id = $1 AND generation_operation_id = $2 \
                AND access_level <= $3::smallint AND (expires_at IS NULL OR expires_at > now()) \
              ORDER BY output_ordinal",
            read.tenant_id,
            operation,
            read.read_ceiling.as_i16(),
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        // A pending cancel reason waits in `outcome_code`; it is an outcome only once final.
        let state = status_of(&row.state)?;
        let outcome = match state {
            GenerationState::Accepted | GenerationState::Claimed | GenerationState::Dispatched => {
                None
            }
            _ => row.outcome_code,
        };
        Ok(GenerationStatus {
            operation_id: operation,
            state,
            outcome,
            observations,
            replayed: false,
        })
    }

    async fn cancel_once(&self, write: &ClientWrite, operation: Uuid) -> TxResult<Value> {
        let mut tx = self.begin().await?;
        lock_writer(&mut tx, write).await?;
        // D13: operation rows before the idempotency record. An operation above the caller's
        // write grant is invisible to it.
        let found = sqlx::query_scalar!(
            r#"SELECT input_level::smallint AS "level!" FROM generation_operations
                WHERE tenant_id = $1 AND namespace_id = $2 AND id = $3 FOR NO KEY UPDATE"#,
            write.tenant_id,
            write.namespace_id,
            operation,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(not_found)?;
        if level(found)? > write.write_max {
            return Err(not_found());
        }
        if let Some(replay) = claim(&mut tx, write, &[]).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        let cancelled = cancel_operations(
            &mut tx,
            write.tenant_id,
            CancelScope::Operation(operation),
            "client_cancelled",
        )
        .await?;
        let row = sqlx::query!(
            "SELECT state, outcome_code FROM generation_operations WHERE tenant_id = $1 AND id = $2",
            write.tenant_id,
            operation,
        )
        .fetch_one(&mut *tx)
        .await?;
        let state = status_of(&row.state)?;
        let result = encode(&GenerationStatus {
            operation_id: operation,
            state,
            outcome: if state == GenerationState::Dispatched {
                None
            } else {
                row.outcome_code
            },
            observations: Vec::new(),
            replayed: false,
        })?;
        finish(
            &mut tx,
            write,
            "generation.cancel",
            "generation",
            &operation.to_string(),
            &result,
            json!({ "cancelled": cancelled, "state": row.state }),
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// One claim attempt on one candidate, in its own transaction. `Ok(None)` moves on to the
    /// next candidate (skipped, or finished without dispatch).
    async fn claim_candidate(
        &self,
        tenant: Uuid,
        operation: Uuid,
        owner: Uuid,
        lease: Duration,
    ) -> TxResult<Option<GenerationJob>> {
        let mut tx = self.begin().await?;
        let Some(ids) = sqlx::query!(
            r#"SELECT namespace_id, credential_id, input_level::smallint AS "level!"
                 FROM generation_operations WHERE tenant_id = $1 AND id = $2"#,
            tenant,
            operation,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(None);
        };
        // D13: tenant → namespace → credential FOR SHARE SKIP LOCKED (a tenant being
        // disabled, a namespace being deleted or a credential being revoked is skipped).
        let Some(authority) = recheck_authority(
            &mut tx,
            tenant,
            ids.namespace_id,
            ids.credential_id,
            ids.level,
            true,
        )
        .await?
        else {
            return Ok(None);
        };
        // Then the fairness cursor and the operation, both FOR UPDATE SKIP LOCKED.
        if sqlx::query_scalar!(
            r#"SELECT true AS "locked!" FROM tenant_generation_schedule WHERE tenant_id = $1
                FOR UPDATE SKIP LOCKED"#,
            tenant,
        )
        .fetch_optional(&mut *tx)
        .await?
        .is_none()
        {
            return Ok(None);
        }
        let Some(row) = sqlx::query!(
            r#"SELECT state, next_attempt_at <= now() AS "due!", expires_at <= now() AS "expired!",
                      reservation
                 FROM generation_operations WHERE tenant_id = $1 AND id = $2
                  FOR UPDATE SKIP LOCKED"#,
            tenant,
            operation,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(None);
        };
        if row.state != "accepted" || !row.due {
            return Ok(None);
        }
        let stop = authority
            .stop
            .or(row.expired.then_some(("expired", "request_expired")));
        if let Some((state, code)) = stop {
            finish_operation(&mut tx, tenant, operation, state, code, None, 0, 0).await?;
            system_audit(
                &mut tx,
                tenant,
                owner,
                operation,
                state,
                json!({ "code": code, "sent": false }),
            )
            .await?;
            tx.commit().await?;
            return Ok(None);
        }
        // The one in-flight slot: a concurrent claim for this tenant fails the partial unique
        // index, and this candidate is simply given up.
        let fence = match sqlx::query_scalar!(
            "UPDATE generation_operations SET state = 'claimed', lease_owner = $3, \
                    lease_until = now() + make_interval(secs => $4), fence = fence + 1 \
              WHERE tenant_id = $1 AND id = $2 RETURNING fence",
            tenant,
            operation,
            owner,
            seconds(lease),
        )
        .fetch_one(&mut *tx)
        .await
        {
            Ok(fence) => fence,
            Err(error)
                if error
                    .as_database_error()
                    .and_then(|db| db.code())
                    .is_some_and(|code| code == "23505") =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        sqlx::query!(
            "UPDATE tenant_generation_schedule SET last_claimed_at = now() WHERE tenant_id = $1",
            tenant,
        )
        .execute(&mut *tx)
        .await?;
        // Spend last (D13): reserve against the UTC day's cap; a day that cannot hold the
        // reservation fails the operation terminally (a new day never revives it).
        let day = sqlx::query_scalar!(r#"SELECT (now() AT TIME ZONE 'UTC')::date AS "day!""#)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query!(
            "INSERT INTO tenant_spend_days (tenant_id, day) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            tenant,
            day,
        )
        .execute(&mut *tx)
        .await?;
        let reserved = sqlx::query_scalar!(
            "UPDATE tenant_spend_days AS d SET reserved = d.reserved + $3 FROM tenants AS t \
              WHERE d.tenant_id = $1 AND d.day = $2 AND t.id = d.tenant_id \
                AND d.reserved + d.used + $3 <= t.generation_daily_token_cap \
             RETURNING d.day",
            tenant,
            day,
            row.reservation,
        )
        .fetch_optional(&mut *tx)
        .await?;
        if reserved.is_none() {
            finish_operation(
                &mut tx,
                tenant,
                operation,
                "failed",
                "spend_cap_reached",
                None,
                0,
                0,
            )
            .await?;
            system_audit(
                &mut tx,
                tenant,
                owner,
                operation,
                "failed",
                json!({ "code": "spend_cap_reached", "sent": false }),
            )
            .await?;
            tx.commit().await?;
            return Ok(None);
        }
        sqlx::query!(
            "UPDATE generation_operations SET spend_day = $3, reserved_tokens = reservation \
              WHERE tenant_id = $1 AND id = $2",
            tenant,
            operation,
            day,
        )
        .execute(&mut *tx)
        .await?;
        let correlation = sqlx::query_scalar!(
            "SELECT correlation_id FROM generation_operations WHERE tenant_id = $1 AND id = $2",
            tenant,
            operation,
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(GenerationJob {
            tenant_id: tenant,
            operation_id: operation,
            owner,
            fence,
            correlation,
        }))
    }

    async fn dispatch_once(&self, job: &GenerationJob, lease: Duration) -> TxResult<Dispatch> {
        let mut tx = self.begin().await?;
        let Some(ids) = sqlx::query!(
            r#"SELECT namespace_id, credential_id, input_level::smallint AS "level!"
                 FROM generation_operations WHERE tenant_id = $1 AND id = $2"#,
            job.tenant_id,
            job.operation_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(Dispatch::Stale);
        };
        let authority = recheck_authority(
            &mut tx,
            job.tenant_id,
            ids.namespace_id,
            ids.credential_id,
            ids.level,
            false,
        )
        .await?
        .ok_or_else(|| stored("An operation's parents are missing"))?;
        let row = sqlx::query!(
            r#"SELECT state, fence, lease_owner, lease_until > now() AS "live!",
                      expires_at <= now() AS "expired!", request, session_id, summary,
                      producing_ceiling::smallint AS "ceiling!", watermark_kind,
                      watermark_level::smallint AS "watermark!", max_output_tokens,
                      allowance_tokens, reserved_tokens, spend_day, expires_at
                 FROM generation_operations WHERE tenant_id = $1 AND id = $2 FOR UPDATE"#,
            job.tenant_id,
            job.operation_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        if row.state != "claimed"
            || row.fence != job.fence
            || row.lease_owner != Some(job.owner)
            || !row.live
        {
            return Ok(Dispatch::Stale);
        }
        let mut stopped = authority.stop;
        if stopped.is_none() && row.expired {
            stopped = Some(("expired", "request_expired"));
        }
        let mut window = None;
        if stopped.is_none() {
            match self.load_window(&mut tx, job, &row.request).await? {
                Ok(events) => window = Some(events),
                Err(code) => stopped = Some(("failed", code)),
            }
        }
        if let Some((state, code)) = stopped {
            finish_operation(
                &mut tx,
                job.tenant_id,
                job.operation_id,
                state,
                code,
                row.spend_day,
                row.reserved_tokens,
                0,
            )
            .await?;
            system_audit(
                &mut tx,
                job.tenant_id,
                job.owner,
                job.operation_id,
                state,
                json!({ "code": code, "sent": false }),
            )
            .await?;
            tx.commit().await?;
            return Ok(Dispatch::Stopped(code));
        }
        let (description, events) = window.expect("loaded");
        let restored = restore(StoredWindow {
            tenant_id: job.tenant_id,
            credential_id: ids.credential_id,
            namespace_id: ids.namespace_id,
            session_id: row.session_id,
            producing_ceiling: level(row.ceiling)?,
            watermark_kind: watermark_kind(&row.watermark_kind)?,
            watermark_level: level(row.watermark)?,
            input_level: level(ids.level)?,
            summary: row.summary,
            description,
            events,
            max_output_tokens: row.max_output_tokens as u32,
            allowance_tokens: row.allowance_tokens as u64,
            reserve_tokens: row.reserved_tokens as u64,
            expires_at: row.expires_at,
        });
        let Some(prepared) = restored else {
            finish_operation(
                &mut tx,
                job.tenant_id,
                job.operation_id,
                "failed",
                "stored_request_invalid",
                row.spend_day,
                row.reserved_tokens,
                0,
            )
            .await?;
            tx.commit().await?;
            return Ok(Dispatch::Stopped("stored_request_invalid"));
        };
        // Committed before the send: from here on the call may run, so it is never re-sent.
        sqlx::query!(
            "UPDATE generation_operations SET state = 'dispatched', attempts = attempts + 1, \
                    dispatched_at = now(), lease_until = now() + make_interval(secs => $3) \
              WHERE tenant_id = $1 AND id = $2",
            job.tenant_id,
            job.operation_id,
            seconds(lease),
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Dispatch::Send(Box::new(prepared)))
    }

    /// The window as stored: the event list from the request and each event's text from its
    /// recorded revision. Missing or expired content (`source_unavailable`) or an undecodable
    /// request (`stored_request_invalid`) refuses the dispatch: an incomplete window is never
    /// sent.
    async fn load_window(
        &self,
        db: &mut PgConnection,
        job: &GenerationJob,
        request: &Option<Value>,
    ) -> TxResult<Result<(Option<String>, Vec<PreparedEvent>), &'static str>> {
        let Some(request) = request else {
            return Ok(Err("source_unavailable"));
        };
        let Ok(request) = serde_json::from_value::<StoredRequest>(request.clone()) else {
            return Ok(Err("stored_request_invalid"));
        };
        let rows = sqlx::query!(
            r#"SELECT s.id, s.external_ref, s.revision, s.access_level::smallint AS "level!",
                      s.content, (s.expires_at IS NOT NULL AND s.expires_at <= now()) AS "expired!"
                 FROM generation_operation_sources AS g
                 JOIN sources AS s ON (s.tenant_id, s.id) = (g.tenant_id, g.source_id)
                WHERE g.tenant_id = $1 AND g.operation_id = $2"#,
            job.tenant_id,
            job.operation_id,
        )
        .fetch_all(&mut *db)
        .await?;
        let sources: HashMap<Uuid, _> = rows.into_iter().map(|row| (row.id, row)).collect();
        let mut events = Vec::with_capacity(request.events.len());
        for event in request.events {
            let Some(source) = sources.get(&event.source_id) else {
                return Ok(Err("stored_request_invalid"));
            };
            let (Some(text), false) = (&source.content, source.expired) else {
                return Ok(Err("source_unavailable"));
            };
            events.push(PreparedEvent {
                event_id: event.event_id,
                source_ref: source.external_ref.clone(),
                revision: source.revision.clone(),
                occurred_at: event.occurred_at,
                role: event.role,
                access_level: level(source.level)?,
                text: text.clone(),
            });
        }
        if events.is_empty() {
            return Ok(Err("stored_request_invalid"));
        }
        Ok(Ok((request.description, events)))
    }

    async fn settle_once(&self, job: &GenerationJob, settlement: &Settlement) -> TxResult<Settled> {
        let mut tx = self.begin().await?;
        let Some(ids) = sqlx::query!(
            r#"SELECT namespace_id, credential_id, input_level::smallint AS "level!"
                 FROM generation_operations WHERE tenant_id = $1 AND id = $2"#,
            job.tenant_id,
            job.operation_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(Settled::Stale);
        };
        // Saving output re-checks its authority under the D13 parent locks first.
        let authority = if matches!(settlement, Settlement::Saved(..)) {
            recheck_authority(
                &mut tx,
                job.tenant_id,
                ids.namespace_id,
                ids.credential_id,
                ids.level,
                false,
            )
            .await?
        } else {
            None
        };
        let row = sqlx::query!(
            r#"SELECT state, fence, lease_owner, cancel_requested_at IS NOT NULL AS "cancel!",
                      outcome_code, expires_at <= now() AS "expired!", reserved_tokens, spend_day,
                      attempts, request
                 FROM generation_operations WHERE tenant_id = $1 AND id = $2 FOR UPDATE"#,
            job.tenant_id,
            job.operation_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        let expected = if matches!(settlement, Settlement::Release) {
            "claimed"
        } else {
            "dispatched"
        };
        if row.state != expected || row.fence != job.fence || row.lease_owner != Some(job.owner) {
            METRICS.inc(
                "memory_generation_lease_lost_total",
                &[("reason", "settle_stale")],
                1,
            );
            return Ok(Settled::Stale);
        }
        let reserved = row.reserved_tokens;
        let charge = |usage: &ProviderUsage| -> i64 {
            let used = usage.total() as i64;
            if used > reserved {
                USAGE_EXCEEDED.fetch_add(1, Ordering::Relaxed);
                METRICS.inc("memory_generation_usage_exceeded_reservation_total", &[], 1);
                tracing::warn!(
                    operation = %job.operation_id,
                    used,
                    reserved,
                    "generation usage exceeded its reservation"
                );
            }
            used
        };
        let (state, code, charged): (&str, String, i64) = match settlement {
            Settlement::Release => {
                sqlx::query!(
                    "UPDATE generation_operations SET state = 'accepted', lease_owner = NULL, \
                            lease_until = NULL, reserved_tokens = 0 \
                      WHERE tenant_id = $1 AND id = $2",
                    job.tenant_id,
                    job.operation_id,
                )
                .execute(&mut *tx)
                .await?;
                settle_spend(&mut tx, job.tenant_id, row.spend_day, reserved, 0).await?;
                tx.commit().await?;
                return Ok(Settled::Done(GenerationState::Accepted));
            }
            Settlement::Retry(code) if !row.cancel && row.attempts < MAX_DISPATCH_ATTEMPTS => {
                // A definite rejection or nothing sent: release and back off, 30 s × 2ⁿ⁻¹.
                sqlx::query!(
                    "UPDATE generation_operations SET state = 'accepted', lease_owner = NULL, \
                            lease_until = NULL, reserved_tokens = 0, \
                            next_attempt_at = now() + make_interval(secs => $3 * power(2, greatest(attempts - 1, 0))) \
                      WHERE tenant_id = $1 AND id = $2",
                    job.tenant_id,
                    job.operation_id,
                    RETRY_BACKOFF_SECS,
                )
                .execute(&mut *tx)
                .await?;
                settle_spend(&mut tx, job.tenant_id, row.spend_day, reserved, 0).await?;
                system_audit(
                    &mut tx,
                    job.tenant_id,
                    job.owner,
                    job.operation_id,
                    "retry",
                    json!({ "code": code, "attempts": row.attempts }),
                )
                .await?;
                tx.commit().await?;
                METRICS.inc("memory_generation_retries_total", &[], 1);
                return Ok(Settled::Done(GenerationState::Accepted));
            }
            Settlement::Paused(code, delay) if !row.cancel => {
                // The provider refused the credentials (p06 A4-1): nothing ran, so the dispatch
                // is not counted, and the operation waits for the breaker's window.
                sqlx::query!(
                    "UPDATE generation_operations SET state = 'accepted', lease_owner = NULL, \
                            lease_until = NULL, reserved_tokens = 0, \
                            attempts = greatest(attempts - 1, 0), \
                            next_attempt_at = now() + make_interval(secs => $3) \
                      WHERE tenant_id = $1 AND id = $2",
                    job.tenant_id,
                    job.operation_id,
                    delay.as_secs_f64(),
                )
                .execute(&mut *tx)
                .await?;
                settle_spend(&mut tx, job.tenant_id, row.spend_day, reserved, 0).await?;
                system_audit(
                    &mut tx,
                    job.tenant_id,
                    job.owner,
                    job.operation_id,
                    "paused",
                    json!({ "code": code, "seconds": delay.as_secs() }),
                )
                .await?;
                tx.commit().await?;
                METRICS.inc(
                    "memory_generation_settled_total",
                    &[("state", "accepted"), ("outcome", code)],
                    1,
                );
                return Ok(Settled::Done(GenerationState::Accepted));
            }
            _ if row.cancel => {
                // Cancelled while dispatched: the output is discarded and what ran is charged.
                let charged = match settlement {
                    Settlement::Saved(_, usage)
                    | Settlement::Skipped(_, usage)
                    | Settlement::Failed(_, usage) => charge(usage),
                    Settlement::Retry(_) | Settlement::Paused(..) => 0,
                    _ => reserved,
                };
                let reason = row
                    .outcome_code
                    .clone()
                    .unwrap_or_else(|| "cancel_requested".into());
                ("cancelled", reason, charged)
            }
            // Out of dispatch attempts. (A `Paused` settlement is always handled above.)
            Settlement::Retry(code) | Settlement::Paused(code, _) => ("failed", code.clone(), 0),
            Settlement::Unknown(code) => ("unknown", (*code).to_owned(), reserved),
            Settlement::Failed(code, usage) => ("failed", code.clone(), charge(usage)),
            Settlement::Skipped(code, usage) => ("completed", (*code).to_owned(), charge(usage)),
            Settlement::Saved(items, usage) => {
                let used = charge(usage);
                // P6b n2, 06A A1-3: a missing tenant, namespace or credential row is a stop,
                // never a save (only a `Saved` settlement re-checked its authority).
                let stop = match authority {
                    Some(authority) => authority.stop,
                    None => Some(("cancelled", "authority_missing")),
                };
                if let Some((state, code)) = stop {
                    (state, code.to_owned(), used)
                } else if row.expired {
                    ("expired", "request_expired".to_owned(), used)
                } else {
                    self.save_output(&mut tx, job, &row.request, items).await?;
                    ("completed", "saved".to_owned(), used)
                }
            }
        };
        finish_operation(
            &mut tx,
            job.tenant_id,
            job.operation_id,
            state,
            &code,
            row.spend_day,
            reserved,
            charged,
        )
        .await?;
        system_audit(
            &mut tx,
            job.tenant_id,
            job.owner,
            job.operation_id,
            state,
            json!({ "code": code, "charged": charged }),
        )
        .await?;
        tx.commit().await?;
        METRICS.inc(
            "memory_generation_settled_total",
            &[("state", state), ("outcome", &code)],
            1,
        );
        if let Settlement::Saved(_, usage)
        | Settlement::Skipped(_, usage)
        | Settlement::Failed(_, usage) = settlement
        {
            count_usage(usage);
        }
        tracing::info!(
            operation_id = %job.operation_id,
            correlation_id = %job.correlation.as_deref().unwrap_or(""),
            state,
            outcome = %code,
            "generation settled"
        );
        Ok(Settled::Done(status_of(state)?))
    }

    /// Insert the generated observations and their source links, labelled with the
    /// operation's frozen evidence (never anything the worker or the model computed), with
    /// stable `(operation, output_ordinal)` identities. They enter the indexing pipeline as
    /// `pending`.
    async fn save_output(
        &self,
        db: &mut PgConnection,
        job: &GenerationJob,
        request: &Option<Value>,
        items: &[GeneratedItem],
    ) -> TxResult<()> {
        let request: StoredRequest = request
            .clone()
            .and_then(|value| serde_json::from_value(value).ok())
            .ok_or_else(|| stored("The stored generation request is unavailable"))?;
        let occurred_at = request
            .events
            .iter()
            .map(|event| event.occurred_at)
            .max()
            .ok_or_else(|| stored("The stored generation request is empty"))?;
        // D13 content order: observations, then their links (whose foreign keys take the
        // cited sources' key-share locks). The window's sources need no lock of their own: an
        // unfinished operation's evidence is never nulled, and revisions are immutable.
        let mut ids = Vec::with_capacity(items.len());
        for (ordinal, item) in items.iter().enumerate() {
            let id = Uuid::now_v7();
            ids.push(id);
            sqlx::query!(
                "INSERT INTO observations (tenant_id, namespace_id, id, session_id, kind, title, body, \
                 access_level, producing_ceiling, watermark_kind, watermark_level, source_max_level, \
                 origin, generation_operation_id, output_ordinal, occurred_at) \
                 SELECT o.tenant_id, o.namespace_id, $3::uuid, o.session_id, $4::text, $5::text, \
                        $6::text, o.input_level, o.producing_ceiling, o.watermark_kind, \
                        o.watermark_level, o.source_max_level, 'generated', o.id, $7::int, \
                        $8::timestamptz \
                   FROM generation_operations AS o WHERE o.tenant_id = $1 AND o.id = $2",
                job.tenant_id,
                job.operation_id,
                id,
                kind_str(item.kind),
                item.title,
                item.body,
                ordinal as i32,
                occurred_at,
            )
            .execute(&mut *db)
            .await?;
        }
        let sources: HashMap<(String, String), (Uuid, AccessLevel)> = sqlx::query!(
            r#"SELECT s.id, s.external_ref, s.revision, s.access_level::smallint AS "level!"
                 FROM generation_operation_sources AS g
                 JOIN sources AS s ON (s.tenant_id, s.id) = (g.tenant_id, g.source_id)
                WHERE g.tenant_id = $1 AND g.operation_id = $2"#,
            job.tenant_id,
            job.operation_id,
        )
        .fetch_all(&mut *db)
        .await?
        .into_iter()
        .map(|row| {
            Ok((
                (row.external_ref, row.revision),
                (row.id, level(row.level)?),
            ))
        })
        .collect::<TxResult<_>>()?;
        for (id, item) in ids.into_iter().zip(items) {
            let links = item
                .sources
                .iter()
                .map(|key| {
                    sources
                        .get(key)
                        .map(|(source, level)| (Link::Source(*source), *level))
                        .ok_or_else(|| stored("A cited revision is not in the window"))
                })
                .collect::<TxResult<Vec<_>>>()?;
            insert_links(db, job.tenant_id, id, &links).await?;
        }
        Ok(())
    }

    async fn reap_once(&self, grace: Duration, limit: i64) -> TxResult<Reaped> {
        let mut tx = self.begin().await?;
        // Expired claims: nothing was sent and nothing is spent, so they are due again.
        let released = sqlx::query!(
            "WITH due AS ( \
               SELECT tenant_id, id, spend_day, reserved_tokens FROM generation_operations \
                WHERE state = 'claimed' AND lease_until < now() \
                ORDER BY tenant_id, id LIMIT $1 FOR UPDATE SKIP LOCKED) \
             UPDATE generation_operations AS o SET state = 'accepted', lease_owner = NULL, \
                    lease_until = NULL, reserved_tokens = 0 \
               FROM due WHERE (o.tenant_id, o.id) = (due.tenant_id, due.id) \
             RETURNING due.tenant_id, due.spend_day, due.reserved_tokens",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        // Dispatched past lease + grace: every live worker has aborted by then, so the slot is
        // freed; the call may have run, so it is charged and never re-sent.
        let abandoned = sqlx::query!(
            "WITH due AS ( \
               SELECT tenant_id, id, spend_day, reserved_tokens, \
                      cancel_requested_at IS NOT NULL AS cancel, outcome_code \
                 FROM generation_operations \
                WHERE state = 'dispatched' AND lease_until + make_interval(secs => $2) < now() \
                ORDER BY tenant_id, id LIMIT $1 FOR UPDATE SKIP LOCKED) \
             UPDATE generation_operations AS o \
                SET state = CASE WHEN due.cancel THEN 'cancelled' ELSE 'unknown' END, \
                    outcome_code = CASE WHEN due.cancel \
                                        THEN coalesce(due.outcome_code, 'cancel_requested') \
                                        ELSE 'lease_expired' END, \
                    finished_at = now(), lease_owner = NULL, lease_until = NULL, \
                    reserved_tokens = 0, used_tokens = o.used_tokens + due.reserved_tokens \
               FROM due WHERE (o.tenant_id, o.id) = (due.tenant_id, due.id) \
             RETURNING due.tenant_id, due.spend_day, due.reserved_tokens",
            limit,
            seconds(grace),
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut spend: BTreeMap<(Uuid, NaiveDate), (i64, i64)> = BTreeMap::new();
        for row in &released {
            if let Some(day) = row.spend_day {
                spend.entry((row.tenant_id, day)).or_default().0 += row.reserved_tokens;
            }
        }
        for row in &abandoned {
            if let Some(day) = row.spend_day {
                let entry = spend.entry((row.tenant_id, day)).or_default();
                entry.0 += row.reserved_tokens;
                entry.1 += row.reserved_tokens;
            }
        }
        for ((tenant, day), (reserved, used)) in spend {
            settle_spend(&mut tx, tenant, Some(day), reserved, used).await?;
        }
        // Pending requests past their deadline (they hold no reservation).
        let expired = sqlx::query!(
            "UPDATE generation_operations SET state = 'expired', outcome_code = 'request_expired', \
                    finished_at = now() \
              WHERE (tenant_id, id) IN ( \
                    SELECT tenant_id, id FROM generation_operations \
                     WHERE state = 'accepted' AND expires_at <= now() \
                     ORDER BY expires_at LIMIT $1 FOR UPDATE SKIP LOCKED)",
            limit,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(Reaped {
            released: released.len() as u64,
            abandoned: abandoned.len() as u64,
            expired,
        })
    }

    async fn sweep_once(&self, limit: i64) -> TxResult<Swept> {
        let retention = f64::from(RETENTION_DAYS) * 86_400.0;
        // Generation requests 30 days after their operation finished.
        let mut tx = self.begin().await?;
        let requests = sqlx::query!(
            "SELECT tenant_id, id FROM generation_operations \
              WHERE request IS NOT NULL AND finished_at < now() - make_interval(secs => $2) \
              ORDER BY finished_at LIMIT $1 FOR NO KEY UPDATE SKIP LOCKED",
            limit,
            retention,
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids): (Vec<Uuid>, Vec<Uuid>) =
            requests.iter().map(|row| (row.tenant_id, row.id)).unzip();
        let nulled_requests = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "UPDATE generation_operations SET request = NULL, inputs_nulled_at = now() \
                  WHERE (tenant_id, id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[]))",
                &tenants,
                &ids,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        // Source content: the candidate scan applies the same retention predicate as the
        // null-out (so rows kept by a recent operation never fill the batch and stall the
        // sweep), locks candidates first, then nulls them in a second statement whose
        // fresh snapshot re-checks that no unfinished operation references them. An
        // acceptance that holds a candidate FOR SHARE is skipped; one that commits before the
        // lock is seen by the re-check; one that comes later waits for this transaction and
        // then finds the content gone and refuses.
        let mut tx = self.begin().await?;
        let candidates = sqlx::query!(
            "SELECT s.tenant_id, s.id FROM sources AS s \
              WHERE s.content IS NOT NULL \
                AND NOT EXISTS ( \
                    SELECT 1 FROM generation_operation_sources AS g \
                      JOIN generation_operations AS o \
                        ON (o.tenant_id, o.id) = (g.tenant_id, g.operation_id) \
                     WHERE g.tenant_id = s.tenant_id AND g.source_id = s.id \
                       AND o.finished_at IS NULL) \
                AND ((s.expires_at IS NOT NULL AND s.expires_at <= now()) \
                     OR (s.recorded_at < now() - make_interval(secs => $2) \
                         AND NOT EXISTS ( \
                            SELECT 1 FROM generation_operation_sources AS g \
                              JOIN generation_operations AS o \
                                ON (o.tenant_id, o.id) = (g.tenant_id, g.operation_id) \
                             WHERE g.tenant_id = s.tenant_id AND g.source_id = s.id \
                               AND o.finished_at > now() - make_interval(secs => $2)))) \
              ORDER BY s.recorded_at LIMIT $1 FOR NO KEY UPDATE OF s SKIP LOCKED",
            limit,
            retention,
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids): (Vec<Uuid>, Vec<Uuid>) =
            candidates.iter().map(|row| (row.tenant_id, row.id)).unzip();
        let nulled_sources = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "UPDATE sources AS s SET content = NULL, content_nulled_at = now() \
                  WHERE (s.tenant_id, s.id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[])) \
                    AND s.content IS NOT NULL \
                    AND NOT EXISTS ( \
                        SELECT 1 FROM generation_operation_sources AS g \
                          JOIN generation_operations AS o \
                            ON (o.tenant_id, o.id) = (g.tenant_id, g.operation_id) \
                         WHERE g.tenant_id = s.tenant_id AND g.source_id = s.id \
                           AND o.finished_at IS NULL) \
                    AND ((s.expires_at IS NOT NULL AND s.expires_at <= now()) \
                         OR (s.recorded_at < now() - make_interval(secs => $3) \
                             AND NOT EXISTS ( \
                                SELECT 1 FROM generation_operation_sources AS g \
                                  JOIN generation_operations AS o \
                                    ON (o.tenant_id, o.id) = (g.tenant_id, g.operation_id) \
                                 WHERE g.tenant_id = s.tenant_id AND g.source_id = s.id \
                                   AND o.finished_at > now() - make_interval(secs => $3))))",
                &tenants,
                &ids,
                retention,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(Swept {
            requests: nulled_requests,
            sources: nulled_sources,
        })
    }
}

#[async_trait]
impl GenerationStore for PgStore {
    async fn accept_generation(
        &self,
        write: &ClientWrite,
        prepared: &PreparedGeneration,
    ) -> Result<Value, ServiceError> {
        with_retry(|| self.accept_once(write, prepared)).await
    }

    async fn generation_status(
        &self,
        read: &ClientRead,
        operation: Uuid,
    ) -> Result<GenerationStatus, ServiceError> {
        with_retry(|| self.status_once(read, operation)).await
    }

    async fn cancel_generation(
        &self,
        write: &ClientWrite,
        operation: Uuid,
    ) -> Result<Value, ServiceError> {
        with_retry(|| self.cancel_once(write, operation)).await
    }

    async fn claim_generation(
        &self,
        owner: Uuid,
        lease: Duration,
    ) -> Result<Option<GenerationJob>, ServiceError> {
        // Due candidates without locks, one per tenant with nothing in flight, the tenant
        // claimed longest ago first (round-robin), then the oldest request (D9).
        let candidates = with_retry(|| async {
            Ok(sqlx::query!(
                "SELECT c.tenant_id, c.id FROM ( \
                   SELECT DISTINCT ON (o.tenant_id) o.tenant_id, o.id, o.created_at \
                     FROM generation_operations AS o JOIN tenants AS t ON t.id = o.tenant_id \
                    WHERE o.state = 'accepted' AND o.next_attempt_at <= now() \
                      AND t.state = 'active' \
                      AND NOT EXISTS (SELECT 1 FROM generation_operations AS f \
                                       WHERE f.tenant_id = o.tenant_id \
                                         AND f.state IN ('claimed', 'dispatched')) \
                    ORDER BY o.tenant_id, o.created_at, o.id) AS c \
                 LEFT JOIN tenant_generation_schedule AS s ON s.tenant_id = c.tenant_id \
                 ORDER BY s.last_claimed_at NULLS FIRST, c.created_at, c.id LIMIT 16",
            )
            .fetch_all(self.pool())
            .await?)
        })
        .await?;
        // A candidate that fails is skipped, so one poisoned row cannot stall every tenant;
        // the first error is reported only when no candidate could be claimed.
        let mut failure = None;
        for candidate in candidates {
            match self
                .claim_candidate(candidate.tenant_id, candidate.id, owner, lease)
                .await
            {
                Ok(Some(job)) => return Ok(Some(job)),
                Ok(None) => {}
                Err(TxError::Db(error))
                    if error
                        .as_database_error()
                        .and_then(|db| db.code())
                        .is_some_and(|code| code == "40P01" || code == "40001") => {}
                Err(error) => {
                    let error = super::service(error);
                    tracing::warn!(
                        operation = %candidate.id,
                        code = ?error.code,
                        "a generation claim failed; trying the next candidate"
                    );
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(None), Err)
    }

    async fn dispatch_generation(
        &self,
        job: &GenerationJob,
        lease: Duration,
    ) -> Result<Dispatch, ServiceError> {
        let dispatch = with_retry(|| self.dispatch_once(job, lease)).await?;
        if matches!(dispatch, Dispatch::Send(_)) {
            METRICS.inc("memory_generation_dispatched_total", &[], 1);
            tracing::info!(operation_id = %job.operation_id,
                correlation_id = %job.correlation.as_deref().unwrap_or(""), "generation dispatched");
        }
        Ok(dispatch)
    }

    async fn renew_generation(
        &self,
        job: &GenerationJob,
        lease: Duration,
    ) -> Result<bool, ServiceError> {
        #[cfg(test)]
        if self.renew_fault.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(ServiceError::unavailable("injected renewal failure"));
        }
        // Single statement, single level (D13). A cancel request fails it, so the worker
        // aborts the call at its next heartbeat.
        let renewed = with_retry(|| async {
            Ok(sqlx::query_scalar!(
                "UPDATE generation_operations SET lease_until = now() + make_interval(secs => $5) \
                  WHERE tenant_id = $1 AND id = $2 AND fence = $3 AND lease_owner = $4 \
                    AND state IN ('claimed', 'dispatched') AND cancel_requested_at IS NULL \
                    AND lease_until > now() \
                 RETURNING id",
                job.tenant_id,
                job.operation_id,
                job.fence,
                job.owner,
                seconds(lease),
            )
            .fetch_optional(self.pool())
            .await?
            .is_some())
        })
        .await?;
        if !renewed {
            // Why, for the lease-loss counter (06A A2 F4): a cancel request, or a lost lease.
            let cancelled = sqlx::query_scalar!(
                r#"SELECT cancel_requested_at IS NOT NULL AS "cancelled!" FROM generation_operations
                    WHERE tenant_id = $1 AND id = $2"#,
                job.tenant_id,
                job.operation_id,
            )
            .fetch_optional(self.pool())
            .await
            .ok()
            .flatten()
            .unwrap_or(false);
            let reason = if cancelled {
                "renewal_cancel_requested"
            } else {
                "renewal_lost"
            };
            METRICS.inc(
                "memory_generation_lease_lost_total",
                &[("reason", reason)],
                1,
            );
            tracing::info!(operation_id = %job.operation_id,
                correlation_id = %job.correlation.as_deref().unwrap_or(""), reason,
                "generation lease lost");
        }
        Ok(renewed)
    }

    async fn settle_generation(
        &self,
        job: &GenerationJob,
        settlement: &Settlement,
    ) -> Result<Settled, ServiceError> {
        with_retry(|| self.settle_once(job, settlement)).await
    }

    async fn reap_generations(&self, grace: Duration, limit: i64) -> Result<Reaped, ServiceError> {
        with_retry(|| self.reap_once(grace, limit)).await
    }

    async fn sweep_retention(&self, limit: i64) -> Result<Swept, ServiceError> {
        with_retry(|| self.sweep_once(limit)).await
    }

    async fn remove_deleted(&self, limit: i64) -> Result<Removed, ServiceError> {
        with_retry(|| self.remove_deleted_once(limit)).await
    }

    async fn sweep_aged(&self, limit: i64) -> Result<Aged, ServiceError> {
        with_retry(|| self.sweep_aged_once(limit)).await
    }

    async fn after_restore(&self) -> Result<u64, ServiceError> {
        with_retry(|| self.after_restore_once()).await
    }
}

impl PgStore {
    async fn after_restore_once(&self) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let unfinished = sqlx::query!(
            "SELECT tenant_id, id, spend_day, reserved_tokens FROM generation_operations \
              WHERE state IN ('accepted', 'claimed', 'dispatched') \
              ORDER BY tenant_id, id FOR UPDATE"
        )
        .fetch_all(&mut *tx)
        .await?;
        for row in &unfinished {
            finish_operation(
                &mut tx,
                row.tenant_id,
                row.id,
                "cancelled",
                "restored",
                row.spend_day,
                row.reserved_tokens,
                0,
            )
            .await?;
        }
        if !unfinished.is_empty() {
            sqlx::query!(
                "INSERT INTO audit_events (id, actor_kind, action, target_kind, target_id, \
                 outcome, details) VALUES ($1, 'system', 'service.after_restore', 'service', \
                 'generation_operations', 'ok', $2)",
                Uuid::now_v7(),
                json!({ "cancelled": unfinished.len() }),
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(unfinished.len() as u64)
    }
}
