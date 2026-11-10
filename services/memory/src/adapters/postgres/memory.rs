//! Client memory storage (02B): sources, sessions, observations and their links, under the D13
//! lock order (tenant → namespace → credential `FOR SHARE`, then the idempotency record, then
//! content rows: observations → parts → links → sources, id order within each). Every write
//! claims its idempotency record before it executes (D5) and commits it with its rows and its
//! audit event.
use super::{PgStore, TxError, TxResult, conflict, stored, with_retry};
use crate::{
    application::{
        contract::*,
        ports::{ClientRead, ClientWrite, MemoryStore},
    },
    domain::{
        access_level::{
            AccessLevel, LabelInputs, LabelRefusal, SaveLabel, WatermarkKind, corrected_level,
            derive_label,
        },
        error::{ErrorCode, ServiceError},
    },
};
use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

pub(super) fn not_found() -> TxError {
    TxError::Service(ServiceError::not_found())
}

pub(super) fn level(value: i16) -> TxResult<AccessLevel> {
    AccessLevel::new(i64::from(value)).map_err(|_| stored("Stored level is invalid"))
}

fn refused(refusal: LabelRefusal) -> TxError {
    TxError::Service(match refusal {
        LabelRefusal::AboveWriteGrant { .. } => ServiceError::forbidden(),
        _ => ServiceError::new(ErrorCode::ClassificationRefused, refusal.to_string())
            .with_details(json!({ "reason": refusal.reason() })),
    })
}

pub(super) fn kind_str(kind: ObservationKind) -> &'static str {
    match kind {
        ObservationKind::Fact => "fact",
        ObservationKind::Decision => "decision",
        ObservationKind::Discovery => "discovery",
        ObservationKind::Fix => "fix",
        ObservationKind::Outcome => "outcome",
        ObservationKind::Note => "note",
        ObservationKind::Summary => "summary",
    }
}

pub(super) fn watermark_str(kind: WatermarkKind) -> &'static str {
    match kind {
        WatermarkKind::Exact => "exact",
        WatermarkKind::CeilingFallback => "ceiling_fallback",
    }
}

pub(super) fn parse<T: serde::de::DeserializeOwned>(value: &str) -> TxResult<T> {
    serde_json::from_value(Value::String(value.to_owned()))
        .map_err(|_| stored("Stored enumeration is invalid"))
}

pub(super) fn encode<T: serde::Serialize>(value: &T) -> TxResult<Value> {
    serde_json::to_value(value).map_err(|_| stored("Cannot encode the result"))
}

/// The content digest clients use to detect changed text: SHA-256 over title, NUL, body
/// (stored text cannot contain NUL, so the split is unambiguous), lowercase hex.
pub fn content_sha256(title: &str, body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update([0u8]);
    hasher.update(body.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// D13 parent locks for a client write, re-checking what authentication read outside the
/// transaction: the tenant is active, the namespace is active (its delete removes the grant),
/// and the credential is neither revoked nor expired.
pub(super) async fn lock_writer(db: &mut PgConnection, write: &ClientWrite) -> TxResult<()> {
    require_tenant(db, write).await?;
    let namespace = sqlx::query_scalar!(
        "SELECT state FROM namespaces WHERE tenant_id = $1 AND id = $2 FOR SHARE",
        write.tenant_id,
        write.namespace_id,
    )
    .fetch_optional(&mut *db)
    .await?;
    if namespace.as_deref() != Some("active") {
        return Err(TxError::Service(ServiceError::forbidden()));
    }
    require_credential(db, write).await
}

/// The writer's tenant `FOR SHARE`, which must be active (D13's first parent lock).
async fn require_tenant(db: &mut PgConnection, write: &ClientWrite) -> TxResult<()> {
    let tenant = sqlx::query_scalar!(
        "SELECT state FROM tenants WHERE id = $1 FOR SHARE",
        write.tenant_id
    )
    .fetch_optional(&mut *db)
    .await?;
    match tenant.as_deref() {
        Some("active") => {}
        Some("disabled") => {
            return Err(TxError::Service(ServiceError::new(
                ErrorCode::TenantDisabled,
                "The tenant is disabled",
            )));
        }
        _ => {
            return Err(TxError::Service(ServiceError::new(
                ErrorCode::CredentialRevoked,
                "The credential was revoked",
            )));
        }
    }
    Ok(())
}

/// Whether the writing credential holds the `purge` grant on its namespace now.
async fn purge_grant(db: &mut PgConnection, write: &ClientWrite) -> TxResult<bool> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM credential_grants
            WHERE tenant_id = $1 AND credential_id = $2 AND namespace_id = $3
              AND 'purge' = ANY(operations) AND write_max IS NOT NULL) AS "granted!""#,
        write.tenant_id,
        write.credential_id,
        write.namespace_id,
    )
    .fetch_one(db)
    .await?)
}

/// The writing credential `FOR SHARE`, which must be neither revoked nor expired.
async fn require_credential(db: &mut PgConnection, write: &ClientWrite) -> TxResult<()> {
    let credential = sqlx::query!(
        r#"SELECT revoked_at IS NOT NULL AS "revoked!",
                  coalesce(expires_at <= now(), false) AS "expired!"
             FROM credentials WHERE tenant_id = $1 AND id = $2 FOR SHARE"#,
        write.tenant_id,
        write.credential_id,
    )
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(|| TxError::Service(ServiceError::unauthenticated()))?;
    if credential.revoked {
        return Err(TxError::Service(ServiceError::new(
            ErrorCode::CredentialRevoked,
            "The credential was revoked",
        )));
    }
    if credential.expired {
        return Err(TxError::Service(ServiceError::new(
            ErrorCode::CredentialExpired,
            "The credential has expired",
        )));
    }
    Ok(())
}

/// Claim the idempotency record (D5). A concurrent identical request blocks on the uncommitted
/// unique entry; afterwards it replays the stored result, or conflicts when the canonical
/// request differs. Only a new execution checks the request's own deadline.
pub(super) async fn claim(
    db: &mut PgConnection,
    write: &ClientWrite,
    cited: &[SourceRef],
) -> TxResult<Option<Value>> {
    let inserted = sqlx::query_scalar!(
        "INSERT INTO idempotency_records (tenant_id, kind, key, request_sha256, credential_id, \
         result, request_expires_at) VALUES ($1, $2, $3, $4, $5, '{}'::jsonb, $6) \
         ON CONFLICT (tenant_id, kind, key) DO NOTHING RETURNING key",
        write.tenant_id,
        write.kind,
        write.key,
        &write.request_sha256[..],
        write.credential_id,
        write.expires_at,
    )
    .fetch_optional(&mut *db)
    .await?;
    if inserted.is_none() {
        let existing = sqlx::query!(
            "SELECT request_sha256, credential_id, result FROM idempotency_records \
             WHERE tenant_id = $1 AND kind = $2 AND key = $3",
            write.tenant_id,
            write.kind,
            write.key,
        )
        .fetch_one(&mut *db)
        .await?;
        if existing.request_sha256 != write.request_sha256 {
            return Err(TxError::Service(ServiceError::new(
                ErrorCode::IdempotencyConflict,
                "The idempotency key was used for a different request",
            )));
        }
        if existing.credential_id != write.credential_id {
            replay_authority(db, write, cited, &existing.result).await?;
        }
        let mut result = existing.result;
        result["replayed"] = Value::Bool(true);
        return Ok(Some(result));
    }
    if write.expires_at <= write.now {
        return Err(TxError::Service(ServiceError::new(
            ErrorCode::RequestExpired,
            "The request expired before it executed",
        )));
    }
    #[cfg(test)]
    hold(&write.key).await;
    Ok(None)
}

/// A replay by a credential other than the one that executed the request re-runs this
/// credential's authority over what the stored result discloses, in a fresh execution's order.
/// Every cited parent must still exist, unexpired, in an active namespace this credential may
/// read at the parent's current level; otherwise the replay is `not_found`, exactly as a fresh
/// request citing it would be, so a replay is no oracle for a parent's existence. Then every
/// level the result reports must be within `write_max`, else 403.
async fn replay_authority(
    db: &mut PgConnection,
    write: &ClientWrite,
    cited: &[SourceRef],
    result: &Value,
) -> TxResult<()> {
    let parents: Vec<Uuid> = cited
        .iter()
        .filter_map(|r| match r {
            SourceRef::Observation { observation_id, .. } => Some(*observation_id),
            SourceRef::Source { .. } => None,
        })
        .collect();
    if !parents.is_empty() {
        let rows = sqlx::query!(
            r#"SELECT o.id, o.namespace_id, o.access_level::smallint AS "level!"
                 FROM observations AS o JOIN namespaces AS n
                   ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
                WHERE o.tenant_id = $1 AND o.id = ANY($2) AND n.state = 'active'
                  AND (o.expires_at IS NULL OR o.expires_at > now())"#,
            write.tenant_id,
            &parents,
        )
        .fetch_all(&mut *db)
        .await?;
        for parent in &parents {
            let row = rows
                .iter()
                .find(|row| row.id == *parent)
                .ok_or_else(not_found)?;
            let found = level(row.level)?;
            if !write
                .readable
                .iter()
                .any(|(namespace, max)| *namespace == row.namespace_id && found <= *max)
            {
                return Err(not_found());
            }
        }
    }
    let mut levels = Vec::new();
    if let Some(level) = result["access_level"].as_i64() {
        levels.push(level);
    }
    if let Some(items) = result["observations"].as_array() {
        levels.extend(
            items
                .iter()
                .filter_map(|item| item["access_level"].as_i64()),
        );
    }
    if levels
        .iter()
        .any(|level| *level > i64::from(write.write_max.as_i16()))
    {
        return Err(TxError::Service(ServiceError::forbidden()));
    }
    Ok(())
}

/// Test hook: a key starting with `hold-` keeps its claimed, uncommitted record for 400 ms, so
/// competing-claimant tests can observe the losers waiting on it in `pg_stat_activity`.
#[cfg(test)]
async fn hold(key: &str) {
    if key.starts_with("hold-") {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
}

pub(super) async fn finish(
    db: &mut PgConnection,
    write: &ClientWrite,
    action: &str,
    target_kind: &str,
    target_id: &str,
    result: &Value,
    details: Value,
) -> TxResult<()> {
    sqlx::query!(
        "UPDATE idempotency_records SET result = $4 \
         WHERE tenant_id = $1 AND kind = $2 AND key = $3",
        write.tenant_id,
        write.kind,
        write.key,
        result,
    )
    .execute(&mut *db)
    .await?;
    sqlx::query!(
        "INSERT INTO audit_events (id, tenant_id, actor_kind, actor_id, action, target_kind, \
         target_id, outcome, request_id, correlation_id, details) \
         VALUES ($1, $2, 'client', $3, $4, $5, $6, 'ok', $7, $8, $9)",
        Uuid::now_v7(),
        write.tenant_id,
        write.credential_id,
        action,
        target_kind,
        target_id,
        write.request_id,
        write.correlation.as_ref().map(|id| id.as_str()),
        details,
    )
    .execute(&mut *db)
    .await?;
    Ok(())
}

pub(super) async fn require_session(
    db: &mut PgConnection,
    write: &ClientWrite,
    session: Option<Uuid>,
) -> TxResult<()> {
    let Some(session) = session else {
        return Ok(());
    };
    let found = sqlx::query_scalar!(
        r#"SELECT true AS "found!" FROM sessions WHERE tenant_id = $1 AND namespace_id = $2 AND id = $3"#,
        write.tenant_id,
        write.namespace_id,
        session,
    )
    .fetch_optional(&mut *db)
    .await?;
    found.map(|_| ()).ok_or_else(not_found)
}

pub(super) enum Link {
    Source(Uuid),
    Parent(Uuid, i64),
}

struct Resolved {
    levels: Vec<AccessLevel>,
    links: Vec<(Link, AccessLevel)>,
}

/// Resolve service sources inside the transaction, locking parent observations and then source
/// revisions `FOR SHARE` in id order. A reference this credential cannot read (foreign,
/// missing, expired, in an ungranted namespace or above that namespace's `read_max`) is
/// `not_found`, revealing nothing. A readable parent at another revision is a
/// `revision_conflict`; a readable source above the producing ceiling is refused by the label.
async fn resolve(
    db: &mut PgConnection,
    write: &ClientWrite,
    refs: &[SourceRef],
) -> TxResult<Resolved> {
    let readable = |namespace: Uuid, level: AccessLevel| {
        write
            .readable
            .iter()
            .any(|(granted, max)| *granted == namespace && level <= *max)
    };
    let parents: Vec<Uuid> = refs
        .iter()
        .filter_map(|r| match r {
            SourceRef::Observation { observation_id, .. } => Some(*observation_id),
            SourceRef::Source { .. } => None,
        })
        .collect();
    let sources: Vec<Uuid> = refs
        .iter()
        .filter_map(|r| match r {
            SourceRef::Source { source_id, .. } => Some(*source_id),
            SourceRef::Observation { .. } => None,
        })
        .collect();
    let mut parent_rows = HashMap::new();
    if !parents.is_empty() {
        // Lock and re-check every other namespace a parent lives in (D13: after the target
        // namespace, in id order; an observation never changes namespace, so the unlocked
        // lookup is stable). A namespace deleted since authentication hides its parents.
        let mut cited: Vec<Uuid> = sqlx::query_scalar!(
            "SELECT DISTINCT namespace_id FROM observations WHERE tenant_id = $1 AND id = ANY($2)",
            write.tenant_id,
            &parents,
        )
        .fetch_all(&mut *db)
        .await?;
        cited.retain(|namespace| *namespace != write.namespace_id);
        let mut active = std::collections::HashSet::from([write.namespace_id]);
        if !cited.is_empty() {
            active.extend(
                sqlx::query_scalar!(
                    "SELECT id FROM namespaces WHERE tenant_id = $1 AND id = ANY($2) \
                     AND state = 'active' ORDER BY id FOR SHARE",
                    write.tenant_id,
                    &cited,
                )
                .fetch_all(&mut *db)
                .await?,
            );
        }
        for row in sqlx::query!(
            r#"SELECT id, namespace_id, access_level::smallint AS "level!", revision,
                      (expires_at IS NOT NULL AND expires_at <= now()) AS "expired!"
                 FROM observations WHERE tenant_id = $1 AND id = ANY($2) ORDER BY id FOR SHARE"#,
            write.tenant_id,
            &parents,
        )
        .fetch_all(&mut *db)
        .await?
        {
            let hidden = row.expired || !active.contains(&row.namespace_id);
            parent_rows.insert(
                row.id,
                (row.namespace_id, level(row.level)?, row.revision, hidden),
            );
        }
    }
    let mut source_rows = HashMap::new();
    if !sources.is_empty() {
        // Sources are evidence of the target namespace only (D4); others are not found.
        for row in sqlx::query!(
            r#"SELECT id, revision, access_level::smallint AS "level!",
                      (expires_at IS NOT NULL AND expires_at <= now()) AS "expired!"
                 FROM sources WHERE tenant_id = $1 AND namespace_id = $2 AND id = ANY($3)
                ORDER BY id FOR SHARE"#,
            write.tenant_id,
            write.namespace_id,
            &sources,
        )
        .fetch_all(&mut *db)
        .await?
        {
            source_rows.insert(row.id, (level(row.level)?, row.revision, row.expired));
        }
    }
    let mut resolved = Resolved {
        levels: Vec::with_capacity(refs.len()),
        links: Vec::with_capacity(refs.len()),
    };
    for reference in refs {
        let (link, found) = match reference {
            SourceRef::Observation {
                observation_id,
                revision,
            } => {
                let &(namespace, found, current, hidden) =
                    parent_rows.get(observation_id).ok_or_else(not_found)?;
                if hidden || !readable(namespace, found) {
                    return Err(not_found());
                }
                if current != *revision {
                    return Err(TxError::Service(ServiceError::new(
                        ErrorCode::RevisionConflict,
                        "A parent observation is not at the named revision",
                    )));
                }
                (Link::Parent(*observation_id, *revision), found)
            }
            SourceRef::Source {
                source_id,
                revision,
            } => {
                let (found, current, expired) = source_rows.get(source_id).ok_or_else(not_found)?;
                if *expired || !readable(write.namespace_id, *found) || current != revision {
                    return Err(not_found());
                }
                (Link::Source(*source_id), *found)
            }
        };
        resolved.levels.push(found);
        resolved.links.push((link, found));
    }
    Ok(resolved)
}

fn label(
    write: &ClientWrite,
    context: &ProducingContext,
    resolved: &Resolved,
) -> TxResult<SaveLabel> {
    derive_label(
        &LabelInputs {
            producing_ceiling: context.producing_ceiling,
            consumed: context.consumed,
            service_sources: resolved.levels.clone(),
            external_sources: context
                .external_sources
                .iter()
                .map(|source| source.access_level)
                .collect(),
            expected: context.expected_access_level,
        },
        Some(write.write_max),
    )
    .map_err(refused)
}

/// Insert an observation's links: source links carry the source's real level (the composite
/// FK refuses any other), parent links the parent's level and revision at derivation.
pub(super) async fn insert_links(
    db: &mut PgConnection,
    tenant: Uuid,
    observation: Uuid,
    links: &[(Link, AccessLevel)],
) -> TxResult<()> {
    let (mut s_ord, mut s_id, mut s_level) = (Vec::new(), Vec::new(), Vec::new());
    let (mut p_ord, mut p_id, mut p_rev, mut p_level) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (ordinal, (link, level)) in links.iter().enumerate() {
        match link {
            Link::Source(id) => {
                s_ord.push(ordinal as i32);
                s_id.push(*id);
                s_level.push(level.as_i16());
            }
            Link::Parent(id, revision) => {
                p_ord.push(ordinal as i32);
                p_id.push(*id);
                p_rev.push(*revision);
                p_level.push(level.as_i16());
            }
        }
    }
    if !s_ord.is_empty() {
        sqlx::query!(
            "INSERT INTO observation_links (tenant_id, observation_id, ordinal, source_id, source_level) \
             SELECT $1, $2, u.ordinal, u.source_id, u.level::smallint \
               FROM UNNEST($3::int[], $4::uuid[], $5::smallint[]) AS u(ordinal, source_id, level)",
            tenant,
            observation,
            &s_ord,
            &s_id,
            &s_level,
        )
        .execute(&mut *db)
        .await?;
    }
    if !p_ord.is_empty() {
        sqlx::query!(
            "INSERT INTO observation_links (tenant_id, observation_id, ordinal, \
             parent_observation_id, parent_revision, source_level) \
             SELECT $1, $2, u.ordinal, u.parent, u.revision, u.level::smallint \
               FROM UNNEST($3::int[], $4::uuid[], $5::bigint[], $6::smallint[]) \
                    AS u(ordinal, parent, revision, level)",
            tenant,
            observation,
            &p_ord,
            &p_id,
            &p_rev,
            &p_level,
        )
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}

/// The visible observation a correction or delete targets, locked `FOR NO KEY UPDATE`. Foreign,
/// missing, expired and above-ceiling ids are the same `not_found` (D4).
async fn lock_target(
    db: &mut PgConnection,
    write: &ClientWrite,
    id: Uuid,
    ceiling: AccessLevel,
    deleting: bool,
) -> TxResult<(AccessLevel, i64)> {
    // A delete locks FOR UPDATE directly (it removes the row); a correction FOR NO KEY UPDATE.
    let row = if deleting {
        sqlx::query!(
            r#"SELECT access_level::smallint AS "level!", revision,
                      (expires_at IS NOT NULL AND expires_at <= now()) AS "expired!"
                 FROM observations WHERE tenant_id = $1 AND namespace_id = $2 AND id = $3
                  FOR UPDATE"#,
            write.tenant_id,
            write.namespace_id,
            id,
        )
        .fetch_optional(&mut *db)
        .await?
        .map(|row| (row.level, row.revision, row.expired))
    } else {
        sqlx::query!(
            r#"SELECT access_level::smallint AS "level!", revision,
                      (expires_at IS NOT NULL AND expires_at <= now()) AS "expired!"
                 FROM observations WHERE tenant_id = $1 AND namespace_id = $2 AND id = $3
                  FOR NO KEY UPDATE"#,
            write.tenant_id,
            write.namespace_id,
            id,
        )
        .fetch_optional(&mut *db)
        .await?
        .map(|row| (row.level, row.revision, row.expired))
    };
    let (found, revision, expired) = row.ok_or_else(not_found)?;
    let found = level(found)?;
    if expired || found > ceiling {
        return Err(not_found());
    }
    // Correcting or deleting a record is a write at its level (D3, D4).
    if found > write.write_max {
        return Err(TxError::Service(ServiceError::forbidden()));
    }
    Ok((found, revision))
}

/// One provenance link the reader may see, with its target's current level and expiry.
#[derive(Debug, Clone)]
pub(super) struct VisibleLink {
    pub reference: SourceRef,
    pub level: AccessLevel,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Provenance links of `ids` that the reader may see, with each target's current level (D4):
/// a source in a named namespace, at or below the ceiling and unexpired; a parent observation
/// in a named, active namespace, at or below the ceiling at its current level and unexpired.
/// Everything else is omitted without a marker. Fetch and search share this filter.
pub(super) async fn provenance(
    db: &mut PgConnection,
    read: &ClientRead,
    ids: &[Uuid],
) -> TxResult<HashMap<Uuid, Vec<VisibleLink>>> {
    let mut sources: HashMap<Uuid, Vec<VisibleLink>> = HashMap::new();
    if ids.is_empty() {
        return Ok(sources);
    }
    for link in sqlx::query!(
        r#"SELECT l.observation_id, l.source_id, s.revision AS "source_revision?",
                  l.parent_observation_id, l.parent_revision,
                  coalesce(s.access_level, p.access_level)::smallint AS "target_level!",
                  CASE WHEN s.id IS NOT NULL THEN s.expires_at ELSE p.expires_at END
                    AS target_expires_at
             FROM observation_links AS l
             LEFT JOIN sources AS s ON (s.tenant_id, s.id) = (l.tenant_id, l.source_id)
             LEFT JOIN observations AS p
               ON (p.tenant_id, p.id) = (l.tenant_id, l.parent_observation_id)
             LEFT JOIN namespaces AS pn ON (pn.tenant_id, pn.id) = (p.tenant_id, p.namespace_id)
            WHERE l.tenant_id = $1 AND l.observation_id = ANY($2)
              AND ((s.id IS NOT NULL AND s.namespace_id = ANY($3)
                    AND s.access_level <= $4::smallint
                    AND (s.expires_at IS NULL OR s.expires_at > now()))
                OR (p.id IS NOT NULL AND p.namespace_id = ANY($3) AND pn.state = 'active'
                    AND p.access_level <= $4::smallint
                    AND (p.expires_at IS NULL OR p.expires_at > now())))
            ORDER BY l.observation_id, l.ordinal"#,
        read.tenant_id,
        ids,
        &read.namespace_ids,
        read.read_ceiling.as_i16(),
    )
    .fetch_all(&mut *db)
    .await?
    {
        let reference = match (
            link.source_id,
            link.source_revision,
            link.parent_observation_id,
            link.parent_revision,
        ) {
            (Some(source_id), Some(revision), _, _) => SourceRef::Source {
                source_id,
                revision,
            },
            (_, _, Some(observation_id), Some(revision)) => SourceRef::Observation {
                observation_id,
                revision,
            },
            _ => return Err(stored("Stored link is invalid")),
        };
        sources
            .entry(link.observation_id)
            .or_default()
            .push(VisibleLink {
                reference,
                level: level(link.target_level)?,
                expires_at: link.target_expires_at,
            });
    }
    Ok(sources)
}

impl PgStore {
    async fn record_sources_once(
        &self,
        write: &ClientWrite,
        request: &SourcesRecordRequest,
    ) -> TxResult<Value> {
        let mut tx = self.begin().await?;
        lock_writer(&mut tx, write).await?;
        if let Some(replay) = claim(&mut tx, write, &[]).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        require_session(&mut tx, write, request.session_id).await?;
        let mut recorded = Vec::with_capacity(request.sources.len());
        for source in &request.sources {
            let digest: [u8; 32] = Sha256::digest(source.content.as_bytes()).into();
            let id = sqlx::query_scalar!(
                "INSERT INTO sources (tenant_id, namespace_id, id, external_ref, revision, \
                 access_level, session_id, content, content_sha256, occurred_at, expires_at, \
                 recorded_by) \
                 VALUES ($1, $2, $3, $4, $5, $6::smallint, $7, $8, $9, $10, $11, $12) \
                 ON CONFLICT (tenant_id, namespace_id, external_ref, revision) DO NOTHING RETURNING id",
                write.tenant_id,
                write.namespace_id,
                Uuid::now_v7(),
                source.external_ref,
                source.revision,
                source.access_level.as_i16(),
                request.session_id,
                source.content,
                &digest[..],
                source.occurred_at,
                source.expires_at,
                write.credential_id,
            )
            .fetch_optional(&mut *tx)
            .await?;
            let id = match id {
                Some(id) => id,
                None => {
                    // A revision is immutable: re-recording it identically names the same row.
                    let existing = sqlx::query!(
                        r#"SELECT id, access_level::smallint AS "level!", content_sha256, session_id,
                                  occurred_at, expires_at, recorded_by
                             FROM sources WHERE tenant_id = $1 AND namespace_id = $2
                              AND external_ref = $3 AND revision = $4"#,
                        write.tenant_id,
                        write.namespace_id,
                        source.external_ref,
                        source.revision,
                    )
                    .fetch_one(&mut *tx)
                    .await?;
                    // A stored revision above what this credential may read matches only
                    // the credential that recorded it; for anyone else it is a conflict
                    // whatever its content, so a write-up grant cannot confirm guesses (D3).
                    let probe = write
                        .read_max
                        .is_none_or(|max| existing.level > max.as_i16())
                        && existing.recorded_by != Some(write.credential_id);
                    if probe
                        || existing.level != source.access_level.as_i16()
                        || existing.content_sha256 != digest
                        || existing.session_id != request.session_id
                        || existing.occurred_at != source.occurred_at
                        || existing.expires_at != source.expires_at
                    {
                        return Err(conflict(
                            "This source revision was already recorded with different content",
                        ));
                    }
                    existing.id
                }
            };
            recorded.push(RecordedSource {
                source_id: id,
                external_ref: source.external_ref.clone(),
                revision: source.revision.clone(),
            });
        }
        let result = encode(&SourcesRecordResponse {
            sources: recorded,
            replayed: false,
        })?;
        let details = json!({ "sources": request.sources.len() });
        finish(
            &mut tx,
            write,
            "sources.record",
            "namespace",
            &write.namespace_id.to_string(),
            &result,
            details,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn open_session_once(
        &self,
        write: &ClientWrite,
        request: &SessionOpenRequest,
    ) -> TxResult<Value> {
        let mut tx = self.begin().await?;
        lock_writer(&mut tx, write).await?;
        if let Some(replay) = claim(&mut tx, write, &[]).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        sqlx::query!(
            "INSERT INTO sessions (tenant_id, namespace_id, id, external_id) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (tenant_id, namespace_id, external_id) DO NOTHING",
            write.tenant_id,
            write.namespace_id,
            Uuid::now_v7(),
            request.external_id,
        )
        .execute(&mut *tx)
        .await?;
        // Sessions are also idempotent by external id: another key opening it gets its view.
        let row = sqlx::query!(
            "SELECT id, state FROM sessions WHERE tenant_id = $1 AND namespace_id = $2 \
             AND external_id = $3",
            write.tenant_id,
            write.namespace_id,
            request.external_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        let result = encode(&SessionView {
            session_id: row.id,
            external_id: request.external_id.clone(),
            state: parse(&row.state)?,
            replayed: false,
        })?;
        finish(
            &mut tx,
            write,
            "session.open",
            "session",
            &row.id.to_string(),
            &result,
            json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn close_session_once(
        &self,
        write: &ClientWrite,
        request: &SessionCloseRequest,
    ) -> TxResult<Value> {
        let mut tx = self.begin().await?;
        lock_writer(&mut tx, write).await?;
        if let Some(replay) = claim(&mut tx, write, &[]).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        let row = sqlx::query!(
            "SELECT external_id, state FROM sessions WHERE tenant_id = $1 AND namespace_id = $2 \
             AND id = $3 FOR NO KEY UPDATE",
            write.tenant_id,
            write.namespace_id,
            request.session_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(not_found)?;
        if row.state == "open" {
            sqlx::query!(
                "UPDATE sessions SET state = 'closed', closed_at = now() \
                 WHERE tenant_id = $1 AND id = $2",
                write.tenant_id,
                request.session_id,
            )
            .execute(&mut *tx)
            .await?;
        }
        let result = encode(&SessionView {
            session_id: request.session_id,
            external_id: row.external_id,
            state: SessionState::Closed,
            replayed: false,
        })?;
        finish(
            &mut tx,
            write,
            "session.close",
            "session",
            &request.session_id.to_string(),
            &result,
            json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn save_once(&self, write: &ClientWrite, request: &SaveRequest) -> TxResult<Value> {
        let mut tx = self.begin().await?;
        lock_writer(&mut tx, write).await?;
        if let Some(replay) = claim(&mut tx, write, &request.context.sources).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        require_session(&mut tx, write, request.session_id).await?;
        let resolved = resolve(&mut tx, write, &request.context.sources).await?;
        let label = label(write, &request.context, &resolved)?;
        let mut saved = Vec::with_capacity(request.items.len());
        for item in &request.items {
            let id = Uuid::now_v7();
            sqlx::query!(
                "INSERT INTO observations (tenant_id, namespace_id, id, session_id, kind, title, body, \
                 access_level, producing_ceiling, watermark_kind, watermark_level, source_max_level, \
                 origin, occurred_at, expires_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8::smallint, $9::smallint, $10, $11::smallint, \
                         $12::smallint, 'explicit', $13, $14)",
                write.tenant_id,
                write.namespace_id,
                id,
                request.session_id,
                kind_str(item.kind),
                item.title,
                item.body,
                label.level().as_i16(),
                label.producing_ceiling().as_i16(),
                watermark_str(label.watermark_kind()),
                label.watermark_level().as_i16(),
                label.source_max_level().as_i16(),
                item.occurred_at,
                item.expires_at,
            )
            .execute(&mut *tx)
            .await?;
            insert_links(&mut tx, write.tenant_id, id, &resolved.links).await?;
            saved.push(SavedObservation {
                observation_id: id,
                revision: 1,
                access_level: label.level(),
                index_state: IndexState::Pending,
            });
        }
        let result = encode(&SaveResponse {
            observations: saved,
            replayed: false,
        })?;
        let details = json!({
            "observations": request.items.len(),
            "access_level": label.level(),
            "watermark_kind": watermark_str(label.watermark_kind()),
        });
        finish(
            &mut tx,
            write,
            "observation.save",
            "namespace",
            &write.namespace_id.to_string(),
            &result,
            details,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn correct_once(&self, write: &ClientWrite, request: &CorrectRequest) -> TxResult<Value> {
        let mut tx = self.begin().await?;
        lock_writer(&mut tx, write).await?;
        if let Some(replay) = claim(&mut tx, write, &request.context.sources).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        let (existing, revision) = lock_target(
            &mut tx,
            write,
            request.observation_id,
            request.read_ceiling,
            false,
        )
        .await?;
        #[cfg(test)]
        hold(&write.key).await;
        if revision != request.expected_revision {
            return Err(TxError::Service(
                ServiceError::new(ErrorCode::RevisionConflict, "The observation changed")
                    .with_details(json!({ "current_revision": revision })),
            ));
        }
        let resolved = resolve(&mut tx, write, &request.context.sources).await?;
        let label = label(write, &request.context, &resolved)?;
        // A correction never lowers the label (D4).
        let stored_level = corrected_level(existing, label);
        // The prior level is carried only when this correction's own evidence derives less.
        let carried = (stored_level > label.level()).then_some(stored_level.as_i16());
        if stored_level > write.write_max {
            return Err(TxError::Service(ServiceError::forbidden()));
        }
        // Derived searchable parts belong to the old text and revision: drop them and requeue
        // indexing under a new fence, so a running indexer cannot commit stale parts.
        sqlx::query!(
            "DELETE FROM observation_parts WHERE tenant_id = $1 AND observation_id = $2",
            write.tenant_id,
            request.observation_id,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM observation_links WHERE tenant_id = $1 AND observation_id = $2",
            write.tenant_id,
            request.observation_id,
        )
        .execute(&mut *tx)
        .await?;
        let revision = sqlx::query_scalar!(
            "UPDATE observations SET title = $3, body = $4, revision = revision + 1, \
                    access_level = $5::smallint, producing_ceiling = $6::smallint, \
                    watermark_kind = $7, watermark_level = $8::smallint, \
                    source_max_level = $9::smallint, carried_level = $10::smallint, \
                    updated_at = now(), index_state = 'pending', index_error = NULL, \
                    index_attempts = 0, next_index_at = now(), index_lease_until = NULL, \
                    index_fence = index_fence + 1 \
              WHERE tenant_id = $1 AND id = $2 RETURNING revision",
            write.tenant_id,
            request.observation_id,
            request.title,
            request.body,
            stored_level.as_i16(),
            label.producing_ceiling().as_i16(),
            watermark_str(label.watermark_kind()),
            label.watermark_level().as_i16(),
            label.source_max_level().as_i16(),
            carried,
        )
        .fetch_one(&mut *tx)
        .await?;
        insert_links(
            &mut tx,
            write.tenant_id,
            request.observation_id,
            &resolved.links,
        )
        .await?;
        let result = encode(&SavedObservation {
            observation_id: request.observation_id,
            revision,
            access_level: stored_level,
            index_state: IndexState::Pending,
        })?;
        let details = json!({ "revision": revision, "access_level": stored_level });
        finish(
            &mut tx,
            write,
            "observation.correct",
            "observation",
            &request.observation_id.to_string(),
            &result,
            details,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn delete_once(&self, write: &ClientWrite, request: &DeleteRequest) -> TxResult<Value> {
        let mut tx = self.begin().await?;
        lock_writer(&mut tx, write).await?;
        if let Some(replay) = claim(&mut tx, write, &[]).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        lock_target(
            &mut tx,
            write,
            request.observation_id,
            request.read_ceiling,
            true,
        )
        .await?;
        // Parts (vectors) and links cascade; links naming it as a parent go with it.
        sqlx::query!(
            "DELETE FROM observations WHERE tenant_id = $1 AND id = $2",
            write.tenant_id,
            request.observation_id,
        )
        .execute(&mut *tx)
        .await?;
        let result = encode(&DeleteResponse {
            observation_id: request.observation_id,
            deleted: true,
            replayed: false,
        })?;
        finish(
            &mut tx,
            write,
            "observation.delete",
            "observation",
            &request.observation_id.to_string(),
            &result,
            json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Client purge (06A A1-2, review F2, F8, F9). D13 order: tenant `FOR SHARE`, namespace
    /// `FOR NO KEY UPDATE`, calling credential `FOR SHARE`, then the idempotency claim, *before*
    /// the namespace and grant re-check, so an identical purge that waited on the namespace
    /// replays the winner's record. A record another credential executed replays only while the
    /// caller holds the `purge` grant, which the winner's commit removed: 403, as a fresh
    /// execution by that credential would be.
    async fn purge_once(&self, write: &ClientWrite, request: &PurgeRequest) -> TxResult<Value> {
        let mut tx = self.begin().await?;
        require_tenant(&mut tx, write).await?;
        let namespace = sqlx::query_scalar!(
            "SELECT state FROM namespaces WHERE tenant_id = $1 AND id = $2 FOR NO KEY UPDATE",
            write.tenant_id,
            write.namespace_id,
        )
        .fetch_optional(&mut *tx)
        .await?;
        require_credential(&mut tx, write).await?;
        let inserted = sqlx::query_scalar!(
            "INSERT INTO idempotency_records (tenant_id, kind, key, request_sha256, credential_id, \
             result, request_expires_at) VALUES ($1, $2, $3, $4, $5, '{}'::jsonb, $6) \
             ON CONFLICT (tenant_id, kind, key) DO NOTHING RETURNING key",
            write.tenant_id,
            write.kind,
            write.key,
            &write.request_sha256[..],
            write.credential_id,
            write.expires_at,
        )
        .fetch_optional(&mut *tx)
        .await?;
        if inserted.is_none() {
            let existing = sqlx::query!(
                "SELECT request_sha256, credential_id, result FROM idempotency_records \
                 WHERE tenant_id = $1 AND kind = $2 AND key = $3",
                write.tenant_id,
                write.kind,
                write.key,
            )
            .fetch_one(&mut *tx)
            .await?;
            if existing.request_sha256 != write.request_sha256 {
                return Err(TxError::Service(ServiceError::new(
                    ErrorCode::IdempotencyConflict,
                    "The idempotency key was used for a different request",
                )));
            }
            if existing.credential_id != write.credential_id && !purge_grant(&mut tx, write).await?
            {
                return Err(TxError::Service(ServiceError::forbidden()));
            }
            let mut result = existing.result;
            result["replayed"] = Value::Bool(true);
            return Ok(result);
        }
        if write.expires_at <= write.now {
            return Err(TxError::Service(ServiceError::new(
                ErrorCode::RequestExpired,
                "The request expired before it executed",
            )));
        }
        #[cfg(test)]
        hold(&write.key).await;
        // A delete or purge that committed while this request waited on the namespace removed
        // its grants: uniformly 403 (D3).
        if namespace.as_deref() != Some("active") || !purge_grant(&mut tx, write).await? {
            return Err(TxError::Service(ServiceError::forbidden()));
        }
        let cancelled =
            super::tombstone_namespace(&mut tx, write.tenant_id, write.namespace_id).await?;
        let result = encode(&PurgeResponse {
            namespace: request.namespace.clone(),
            purged: true,
            replayed: false,
        })?;
        finish(
            &mut tx,
            write,
            "namespace.purge",
            "namespace",
            &request.namespace,
            &result,
            json!({ "cancelled_operations": cancelled }),
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }
}

#[async_trait]
impl MemoryStore for PgStore {
    async fn record_sources(
        &self,
        write: &ClientWrite,
        request: &SourcesRecordRequest,
    ) -> Result<Value, ServiceError> {
        with_retry(|| self.record_sources_once(write, request)).await
    }
    async fn open_session(
        &self,
        write: &ClientWrite,
        request: &SessionOpenRequest,
    ) -> Result<Value, ServiceError> {
        with_retry(|| self.open_session_once(write, request)).await
    }
    async fn close_session(
        &self,
        write: &ClientWrite,
        request: &SessionCloseRequest,
    ) -> Result<Value, ServiceError> {
        with_retry(|| self.close_session_once(write, request)).await
    }
    async fn save_observations(
        &self,
        write: &ClientWrite,
        request: &SaveRequest,
    ) -> Result<Value, ServiceError> {
        with_retry(|| self.save_once(write, request)).await
    }
    async fn correct_observation(
        &self,
        write: &ClientWrite,
        request: &CorrectRequest,
    ) -> Result<Value, ServiceError> {
        with_retry(|| self.correct_once(write, request)).await
    }
    async fn delete_observation(
        &self,
        write: &ClientWrite,
        request: &DeleteRequest,
    ) -> Result<Value, ServiceError> {
        with_retry(|| self.delete_once(write, request)).await
    }

    async fn purge_namespace(
        &self,
        write: &ClientWrite,
        request: &PurgeRequest,
    ) -> Result<Value, ServiceError> {
        with_retry(|| self.purge_once(write, request)).await
    }

    async fn purge_replay(
        &self,
        tenant: Uuid,
        credential: Uuid,
        key: &str,
        request_sha256: [u8; 32],
    ) -> Result<Option<Value>, ServiceError> {
        let row = with_retry(|| async {
            Ok(sqlx::query!(
                "SELECT request_sha256, credential_id, result FROM idempotency_records \
                 WHERE tenant_id = $1 AND kind = 'namespaces.purge' AND key = $2",
                tenant,
                key,
            )
            .fetch_optional(&self.pool)
            .await?)
        })
        .await?;
        Ok(row
            .filter(|row| row.credential_id == credential && row.request_sha256 == request_sha256)
            .map(|row| {
                let mut result = row.result;
                result["replayed"] = Value::Bool(true);
                result
            }))
    }

    /// Selected details in one REPEATABLE READ snapshot, filtered by tenant, granted active
    /// namespaces, ceiling and expiry before anything is returned (D4). Ids that are filtered
    /// out are `unavailable`, indistinguishably; details past the byte budget are
    /// `omitted_for_budget`.
    async fn fetch_observations(
        &self,
        read: &ClientRead,
        ids: &[Uuid],
    ) -> Result<FetchResponse, ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            sqlx::query!("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
                .execute(&mut *tx)
                .await?;
            let rows = sqlx::query!(
                r#"SELECT o.id, n.external_id AS namespace, o.kind, o.title, o.body, o.occurred_at,
                          o.access_level::smallint AS "level!", o.revision, o.index_state
                     FROM observations AS o JOIN namespaces AS n
                       ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
                    WHERE o.tenant_id = $1 AND o.namespace_id = ANY($2) AND o.id = ANY($3)
                      AND o.access_level <= $4::smallint AND n.state = 'active'
                      AND (o.expires_at IS NULL OR o.expires_at > now())"#,
                read.tenant_id,
                &read.namespace_ids,
                ids,
                read.read_ceiling.as_i16(),
            )
            .fetch_all(&mut *tx)
            .await?;
            let visible: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
            let mut sources = provenance(&mut tx, read, &visible).await?;
            tx.commit().await?;
            let mut details = HashMap::new();
            for row in rows {
                let detail = ObservationDetail {
                    observation_id: row.id,
                    namespace: row.namespace,
                    kind: parse(&row.kind)?,
                    content_sha256: content_sha256(&row.title, &row.body),
                    title: row.title,
                    body: row.body,
                    occurred_at: row.occurred_at,
                    access_level: level(row.level)?,
                    revision: row.revision,
                    sources: Vec::new(),
                    index_state: parse(&row.index_state)?,
                };
                let links = sources.remove(&row.id).unwrap_or_default();
                details.insert(detail.observation_id, (detail, links));
            }
            let mut response = FetchResponse {
                items: Vec::new(),
                unavailable: Vec::new(),
                omitted_for_budget: Vec::new(),
                exposed_level: AccessLevel::PUBLIC,
            };
            let mut bytes = 0usize;
            for id in ids {
                let Some((mut detail, links)) = details.remove(id) else {
                    response.unavailable.push(*id);
                    continue;
                };
                let link_level = links.iter().map(|link| link.level).max();
                detail.sources = links.into_iter().map(|link| link.reference).collect();
                let size = serde_json::to_vec(&detail).map_or(usize::MAX, |encoded| encoded.len());
                if bytes.saturating_add(size) > crate::application::memory::MAX_FETCH_BYTES {
                    response.omitted_for_budget.push(*id);
                    continue;
                }
                bytes += size;
                // Provenance is exposed metadata too: a parent's id at its current level counts.
                response.exposed_level = response
                    .exposed_level
                    .max(detail.access_level)
                    .max(link_level.unwrap_or(AccessLevel::PUBLIC));
                response.items.push(detail);
            }
            Ok(response)
        })
        .await
    }
}
