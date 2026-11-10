//! Progressive recall in PostgreSQL (03B): timeline neighbourhoods, context-pack candidates and
//! pack validation. Every statement carries the D4 predicate (tenant, named namespaces, active
//! namespace, ceiling, expiry) in its WHERE clause, ahead of any ORDER BY or LIMIT, so a record
//! the reader may not see never takes a slot, a count or a gap; provenance goes through the
//! filter fetch and search share.
use super::{
    PgStore, TxResult,
    memory::{level, parse, provenance},
    search::{RecallRow, RowShape},
    stored, with_retry,
};
use crate::{
    application::{
        contract::{
            ContextValidateResponse, PackDetail, PackItem, PackItemStatus, PackedRevision,
            SemanticState, TimelineResponse, ValidatedItem,
        },
        ports::{
            ClientRead, PackCandidate, PackCandidates, RecallStore, SearchQuery, TimelineQuery,
        },
        recall::sources_sha256,
    },
    domain::{access_level::AccessLevel, bounds::MAX_CONTEXT_ITEMS, retrieval::snippet},
};
use async_trait::async_trait;
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

/// Read statements are bounded at 2 s (D6).
const READ_TIMEOUT_MS: &str = "2000";

async fn read_snapshot(db: &mut PgConnection) -> TxResult<()> {
    sqlx::query!("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *db)
        .await?;
    sqlx::query!(
        "SELECT set_config('statement_timeout', $1, true)",
        READ_TIMEOUT_MS
    )
    .fetch_one(&mut *db)
    .await?;
    Ok(())
}

/// The visible rows among `ids`, in the order given, with their visible provenance. The
/// snippet without a query is the body's opening (at most 320 bytes).
async fn load_rows(
    db: &mut PgConnection,
    read: &ClientRead,
    ids: &[Uuid],
    shape: RowShape,
) -> TxResult<Vec<RecallRow>> {
    let rows = sqlx::query!(
        r#"SELECT o.id, n.external_id AS namespace, o.kind, o.title, o.occurred_at,
                  o.access_level::smallint AS "level!", o.revision, o.index_state, o.expires_at,
                  CASE WHEN $5::bool THEN left(o.body, 320) ELSE '' END AS "snippet!",
                  CASE WHEN $6::bool
                       THEN encode(sha256(convert_to(o.title, 'UTF8') || decode('00', 'hex')
                                          || convert_to(o.body, 'UTF8')), 'hex')
                  END AS digest,
                  CASE WHEN $7::bool THEN octet_length(to_json(o.body)::text) END
                    AS body_json_bytes
             FROM observations AS o
             JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
            WHERE o.tenant_id = $1 AND o.id = ANY($2) AND o.namespace_id = ANY($3)
              AND o.access_level <= $4::smallint AND n.state = 'active'
              AND (o.expires_at IS NULL OR o.expires_at > now())"#,
        read.tenant_id,
        ids,
        &read.namespace_ids,
        read.read_ceiling.as_i16(),
        shape.snippet,
        shape.digest,
        shape.body_size,
    )
    .fetch_all(&mut *db)
    .await?;
    let visible: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut links = provenance(db, read, &visible).await?;
    let mut by_id = HashMap::new();
    for row in rows {
        by_id.insert(row.id, row);
    }
    let mut ordered = Vec::with_capacity(by_id.len());
    for id in ids {
        let Some(row) = by_id.remove(id) else {
            continue;
        };
        ordered.push(RecallRow {
            id: row.id,
            namespace: row.namespace,
            kind: row.kind,
            title: row.title,
            snippet: snippet(&row.snippet),
            digest: row.digest,
            body_json_bytes: row.body_json_bytes,
            occurred_at: row.occurred_at,
            level: level(row.level)?,
            revision: row.revision,
            index_state: row.index_state,
            expires_at: row.expires_at,
            sources: links.remove(id).unwrap_or_default(),
        });
    }
    Ok(ordered)
}

/// Compact candidates carry their snippets; full candidates carry only their bodies' exact
/// JSON sizes, so selection never loads bodies that cannot be included (`pack_bodies`).
fn shape(detail: PackDetail) -> RowShape {
    RowShape {
        snippet: detail == PackDetail::Compact,
        digest: true,
        body_size: detail == PackDetail::Full,
    }
}

fn candidate(row: RecallRow, detail: PackDetail) -> TxResult<PackCandidate> {
    let exposed_level = row.exposed_level();
    let valid_until = row.valid_until();
    let content_sha256 = row
        .digest
        .ok_or_else(|| stored("A pack candidate has no digest"))?;
    let body_json_bytes = match detail {
        PackDetail::Full => Some(
            row.body_json_bytes
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or_else(|| stored("A full pack candidate has no body size"))?,
        ),
        PackDetail::Compact => None,
    };
    let sources: Vec<_> = row.sources.into_iter().map(|link| link.reference).collect();
    Ok(PackCandidate {
        item: PackItem {
            observation_id: row.id,
            namespace: row.namespace,
            kind: parse(&row.kind)?,
            title: row.title,
            text: row.snippet,
            occurred_at: row.occurred_at,
            access_level: row.level,
            revision: row.revision,
            content_sha256,
            sources_sha256: sources_sha256(&sources),
            sources,
        },
        exposed_level,
        valid_until,
        body_json_bytes,
    })
}

impl PgStore {
    async fn timeline_once(
        &self,
        read: &ClientRead,
        query: TimelineQuery,
    ) -> TxResult<TimelineResponse> {
        let mut tx = self.begin().await?;
        read_snapshot(&mut tx).await?;
        // The anchor passes the same predicate as everything else, or it is not found.
        let Some(anchor) = sqlx::query!(
            "SELECT o.occurred_at, o.session_id FROM observations AS o \
               JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id) \
              WHERE o.tenant_id = $1 AND o.id = $2 AND o.namespace_id = ANY($3) \
                AND o.access_level <= $4::smallint AND n.state = 'active' \
                AND (o.expires_at IS NULL OR o.expires_at > now())",
            read.tenant_id,
            query.anchor,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Err(super::memory::not_found());
        };
        // The nearest visible neighbours on each side by (occurred_at, id); the predicate
        // precedes each LIMIT, so invisible rows take no slot.
        let neighbours = sqlx::query_scalar!(
            r#"(SELECT o.id FROM observations AS o
                  JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
                 WHERE o.tenant_id = $1 AND o.namespace_id = ANY($2)
                   AND o.access_level <= $3::smallint AND n.state = 'active'
                   AND (o.expires_at IS NULL OR o.expires_at > now())
                   AND (o.occurred_at, o.id) < ($4, $5)
                   AND (NOT $8::bool OR o.session_id = $9)
                 ORDER BY o.occurred_at DESC, o.id DESC LIMIT $6)
               UNION ALL
               (SELECT o.id FROM observations AS o
                  JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
                 WHERE o.tenant_id = $1 AND o.namespace_id = ANY($2)
                   AND o.access_level <= $3::smallint AND n.state = 'active'
                   AND (o.expires_at IS NULL OR o.expires_at > now())
                   AND (o.occurred_at, o.id) > ($4, $5)
                   AND (NOT $8::bool OR o.session_id = $9)
                 ORDER BY o.occurred_at, o.id LIMIT $7)"#,
            read.tenant_id,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
            anchor.occurred_at,
            query.anchor,
            i64::from(query.before),
            i64::from(query.after),
            query.session_only,
            anchor.session_id,
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut ids: Vec<Uuid> = neighbours.into_iter().flatten().collect();
        ids.push(query.anchor);
        let mut rows = load_rows(&mut tx, read, &ids, RowShape::TIMELINE).await?;
        tx.commit().await?;
        rows.sort_by(|a, b| a.occurred_at.cmp(&b.occurred_at).then(a.id.cmp(&b.id)));
        let mut response = TimelineResponse {
            items: Vec::with_capacity(rows.len()),
            exposed_level: AccessLevel::PUBLIC,
        };
        for row in rows {
            response.exposed_level = response.exposed_level.max(row.exposed_level());
            response.items.push(row.index_item()?);
        }
        Ok(response)
    }

    /// The most recent visible observations: `occurred_at` descending, then id (D8).
    async fn recent_once(&self, read: &ClientRead, detail: PackDetail) -> TxResult<PackCandidates> {
        let mut tx = self.begin().await?;
        read_snapshot(&mut tx).await?;
        let mut ids = sqlx::query_scalar!(
            "SELECT o.id FROM observations AS o \
               JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id) \
              WHERE o.tenant_id = $1 AND o.namespace_id = ANY($2) \
                AND o.access_level <= $3::smallint AND n.state = 'active' \
                AND (o.expires_at IS NULL OR o.expires_at > now()) \
              ORDER BY o.occurred_at DESC, o.id LIMIT $4",
            read.tenant_id,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
            MAX_CONTEXT_ITEMS as i64 + 1,
        )
        .fetch_all(&mut *tx)
        .await?;
        let truncated = ids.len() > MAX_CONTEXT_ITEMS;
        ids.truncate(MAX_CONTEXT_ITEMS);
        let rows = load_rows(&mut tx, read, &ids, shape(detail)).await?;
        tx.commit().await?;
        Ok(PackCandidates {
            candidates: rows
                .into_iter()
                .map(|row| candidate(row, detail))
                .collect::<TxResult<_>>()?,
            truncated,
            semantic: SemanticState::Complete,
        })
    }

    async fn pack_bodies_once(
        &self,
        read: &ClientRead,
        items: &[(Uuid, i64)],
    ) -> TxResult<HashMap<Uuid, String>> {
        let mut tx = self.begin().await?;
        read_snapshot(&mut tx).await?;
        let (ids, revisions): (Vec<Uuid>, Vec<i64>) = items.iter().copied().unzip();
        // Only the selected bodies, at the revisions selected; a row corrected, deleted or
        // hidden since is simply absent.
        let rows = sqlx::query!(
            r#"SELECT o.id, o.body FROM observations AS o
                 JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
                 JOIN UNNEST($2::uuid[], $5::bigint[]) AS r(id, revision)
                   ON r.id = o.id AND r.revision = o.revision
                WHERE o.tenant_id = $1 AND o.namespace_id = ANY($3)
                  AND o.access_level <= $4::smallint AND n.state = 'active'
                  AND (o.expires_at IS NULL OR o.expires_at > now())"#,
            read.tenant_id,
            &ids,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
            &revisions,
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|row| (row.id, row.body)).collect())
    }

    async fn validate_once(
        &self,
        read: &ClientRead,
        items: &[PackedRevision],
    ) -> TxResult<ContextValidateResponse> {
        let mut tx = self.begin().await?;
        read_snapshot(&mut tx).await?;
        let ids: Vec<Uuid> = items.iter().map(|item| item.observation_id).collect();
        // Visible apart from expiry: tenant, named active namespace and ceiling.
        let rows = sqlx::query!(
            r#"SELECT o.id, o.revision, o.access_level::smallint AS "level!",
                      (o.expires_at IS NOT NULL AND o.expires_at <= now()) AS "expired!"
                 FROM observations AS o
                 JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
                WHERE o.tenant_id = $1 AND o.id = ANY($2) AND o.namespace_id = ANY($3)
                  AND o.access_level <= $4::smallint AND n.state = 'active'"#,
            read.tenant_id,
            &ids,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
        )
        .fetch_all(&mut *tx)
        .await?;
        let checked_at = sqlx::query_scalar!(r#"SELECT now() AS "now!""#)
            .fetch_one(&mut *tx)
            .await?;
        // The provenance the reader sees now, through the same filter the pack used: a link
        // whose target expired, was deleted or left the ceiling or the named namespaces makes
        // the frozen item `changed`.
        let current: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
        let links = provenance(&mut tx, read, &current).await?;
        tx.commit().await?;
        let digests: HashMap<Uuid, String> = current
            .iter()
            .map(|id| {
                let sources: Vec<_> = links
                    .get(id)
                    .map(|links| links.iter().map(|link| link.reference.clone()).collect())
                    .unwrap_or_default();
                (*id, sources_sha256(&sources))
            })
            .collect();
        let found: HashMap<Uuid, (i64, i16, bool)> = rows
            .into_iter()
            .map(|row| (row.id, (row.revision, row.level, row.expired)))
            .collect();
        let mut response = ContextValidateResponse {
            items: Vec::with_capacity(items.len()),
            exposed_level: AccessLevel::PUBLIC,
            checked_at,
        };
        for item in items {
            let status = match found.get(&item.observation_id) {
                None => PackItemStatus::Unavailable,
                Some((revision, current, expired)) => {
                    response.exposed_level = response.exposed_level.max(level(*current)?);
                    if *expired {
                        PackItemStatus::Expired
                    } else if *revision != item.revision
                        || digests.get(&item.observation_id) != Some(&item.sources_sha256)
                    {
                        PackItemStatus::Changed
                    } else {
                        PackItemStatus::Valid
                    }
                }
            };
            response.items.push(ValidatedItem {
                observation_id: item.observation_id,
                status,
            });
        }
        Ok(response)
    }
}

#[async_trait]
impl RecallStore for PgStore {
    async fn timeline(
        &self,
        read: &ClientRead,
        query: TimelineQuery,
    ) -> Result<TimelineResponse, crate::domain::error::ServiceError> {
        with_retry(|| self.timeline_once(read, query)).await
    }

    async fn pack_candidates(
        &self,
        read: &ClientRead,
        query: Option<&SearchQuery>,
        detail: PackDetail,
    ) -> Result<PackCandidates, crate::domain::error::ServiceError> {
        with_retry(|| async {
            let Some(query) = query else {
                return self.recent_once(read, detail).await;
            };
            let recalled = self.recall_once(read, query, shape(detail)).await?;
            Ok(PackCandidates {
                candidates: recalled
                    .rows
                    .into_iter()
                    .map(|row| candidate(row, detail))
                    .collect::<TxResult<_>>()?,
                truncated: recalled.truncated,
                semantic: recalled.semantic,
            })
        })
        .await
    }

    async fn pack_bodies(
        &self,
        read: &ClientRead,
        items: &[(Uuid, i64)],
    ) -> Result<HashMap<Uuid, String>, crate::domain::error::ServiceError> {
        with_retry(|| self.pack_bodies_once(read, items)).await
    }

    async fn validate_pack(
        &self,
        read: &ClientRead,
        items: &[PackedRevision],
    ) -> Result<ContextValidateResponse, crate::domain::error::ServiceError> {
        with_retry(|| self.validate_once(read, items)).await
    }
}
