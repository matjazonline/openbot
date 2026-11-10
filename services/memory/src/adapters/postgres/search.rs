//! Hybrid retrieval and the indexing state machine in PostgreSQL (D7). Both legs filter by
//! tenant, named namespaces, ceiling, active namespace and expiry before any limit; the
//! semantic leg also compares only vectors of the configured model. Fetch's provenance filter
//! (D4) supplies the items' source references.
use super::{
    PgStore, TxError, TxResult,
    memory::{VisibleLink, level, parse, provenance},
    stored, with_retry,
};
use crate::{
    application::{
        contract::{IndexItem, SearchResponse, SemanticState},
        ports::{
            ClientRead, IndexCommit, IndexFailure, IndexJob, IndexedPart, SearchQuery, SearchStore,
        },
    },
    domain::{
        access_level::AccessLevel,
        error::ServiceError,
        retrieval::{
            DIMENSIONS, MAX_QUERY_LEXEMES, MAX_SCAN_TUPLES, Ranked, VectorPlan, VectorStep, fuse,
            snippet,
        },
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use uuid::Uuid;

/// Searches whose exact semantic scan exceeded the cap and answered `semantic: partial`; the
/// counter phase 06's metrics export.
static SEMANTIC_PARTIAL_TOTAL: AtomicU64 = AtomicU64::new(0);

pub fn semantic_partial_total() -> u64 {
    SEMANTIC_PARTIAL_TOTAL.load(Ordering::Relaxed)
}

/// Read statements of a search are bounded at 2 s (D6).
const READ_TIMEOUT_MS: &str = "2000";

#[derive(Debug, Clone, Copy)]
struct Hit {
    id: Uuid,
    occurred_at: DateTime<Utc>,
    revision: i64,
    byte_start: i32,
    byte_end: i32,
}

/// Which optional columns a recall row carries.
#[derive(Debug, Clone, Copy)]
pub(super) struct RowShape {
    pub snippet: bool,
    pub digest: bool,
    /// The body's exact JSON-string size (`to_json(body)`, quotes included) instead of the body.
    pub body_size: bool,
}

impl RowShape {
    pub const SEARCH: Self = Self {
        snippet: true,
        digest: false,
        body_size: false,
    };
    pub const TIMELINE: Self = Self::SEARCH;
}

/// One visible observation as recall returns it, with its visible provenance and each link
/// target's current level.
pub(super) struct RecallRow {
    pub id: Uuid,
    pub namespace: String,
    pub kind: String,
    pub title: String,
    /// At most 320 bytes; empty unless the shape asked for it.
    pub snippet: String,
    pub digest: Option<String>,
    pub body_json_bytes: Option<i32>,
    pub occurred_at: DateTime<Utc>,
    pub level: AccessLevel,
    pub revision: i64,
    pub index_state: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub sources: Vec<VisibleLink>,
}

impl RecallRow {
    /// Its own level and every provenance target it carries (D4: metadata is exposure).
    pub fn exposed_level(&self) -> AccessLevel {
        self.sources
            .iter()
            .map(|link| link.level)
            .fold(self.level, AccessLevel::max)
    }

    /// The earliest expiry of the row and of every provenance target it carries: past it, the
    /// row or a link disappears, so a frozen pack holding it is no longer current.
    pub fn valid_until(&self) -> Option<DateTime<Utc>> {
        self.sources
            .iter()
            .filter_map(|link| link.expires_at)
            .chain(self.expires_at)
            .min()
    }

    pub fn index_item(self) -> TxResult<IndexItem> {
        Ok(IndexItem {
            observation_id: self.id,
            namespace: self.namespace,
            kind: parse(&self.kind)?,
            title: self.title,
            snippet: self.snippet,
            occurred_at: self.occurred_at,
            access_level: self.level,
            revision: self.revision,
            sources: self
                .sources
                .into_iter()
                .map(|link| link.reference)
                .collect(),
            index_state: parse(&self.index_state)?,
        })
    }
}

pub(super) struct Recalled {
    pub rows: Vec<RecallRow>,
    pub semantic: SemanticState,
    /// Some leg filled its window or more visible candidates were fused than kept.
    pub truncated: bool,
}

fn seconds(duration: Duration) -> f64 {
    duration.as_secs_f64()
}

/// pgvector's text form; Rust's shortest round-trip float formatting keeps every bit.
fn vector_text(vector: &[f32]) -> String {
    let values: Vec<String> = vector.iter().map(|value| value.to_string()).collect();
    format!("[{}]", values.join(","))
}

async fn read_only(db: &mut PgConnection) -> TxResult<()> {
    sqlx::query!("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// The keyword leg, in the caller's snapshot.
pub(super) async fn keyword_leg(
    db: &mut PgConnection,
    read: &ClientRead,
    query: &SearchQuery,
    limit: usize,
) -> TxResult<Vec<Ranked>> {
    // The keyword leg (D7): observations holding all of the query's first 32 'simple'
    // lexemes, or any identifier-like raw token as a phrase (so `idempotency_key` or
    // `foo.bar.baz` match although the parser splits them), ranked by ts_rank_cd over any
    // lexeme, under the same predicate. 'simple' keeps function words, so an OR of every
    // word would match nearly every row. It covers pending and failed-index observations.
    let keyword = sqlx::query!(
            r#"WITH lexemes AS (
                 SELECT t.lexeme,
                        '''' || replace(replace(t.lexeme, '\', '\\'), '''', '''''') || '''' AS quoted
                   FROM unnest(to_tsvector('simple', $5)) AS t
                  ORDER BY t.positions[1], t.lexeme LIMIT $6
               ), phrases AS (
                 SELECT phraseto_tsquery('simple', token) AS phrase FROM unnest($7::text[]) AS token
               ), q AS (
                 SELECT (SELECT string_agg(quoted, ' & ')::tsquery FROM lexemes) AS every,
                        (SELECT string_agg(quoted, ' | ')::tsquery FROM lexemes) AS any,
                        (SELECT string_agg('(' || phrase::text || ')', ' | ')::tsquery
                           FROM phrases WHERE numnode(phrase) > 0) AS ids
               )
               SELECT o.id, o.occurred_at
                 FROM q, observations AS o
                 JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
                WHERE q.every IS NOT NULL
                  AND (o.search_tsv @@ q.every OR (q.ids IS NOT NULL AND o.search_tsv @@ q.ids))
                  AND o.tenant_id = $1 AND o.namespace_id = ANY($2)
                  AND o.access_level <= $3::smallint AND n.state = 'active'
                  AND (o.expires_at IS NULL OR o.expires_at > now())
                ORDER BY (o.search_tsv @@ q.every) DESC, ts_rank_cd(o.search_tsv, q.any) DESC,
                         o.occurred_at DESC, o.id
                LIMIT $4"#,
            read.tenant_id,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
            limit as i64,
            query.text,
            MAX_QUERY_LEXEMES as i64,
            &query.identifiers,
        )
        .fetch_all(&mut *db)
        .await?;
    Ok(keyword
        .into_iter()
        .map(|row| Ranked {
            id: row.id,
            occurred_at: row.occurred_at,
        })
        .collect())
}

impl PgStore {
    /// The semantic leg in its own read-only transaction: a filtered HNSW iterative scan with
    /// one `SET LOCAL` block, retried with 4× the parts while it fills its limit with too few
    /// observations, and settled by the exact scan when it stops short (D7).
    async fn vector_leg(
        &self,
        read: &ClientRead,
        query: &SearchQuery,
    ) -> TxResult<(Vec<Hit>, SemanticState)> {
        let mut tx = self.begin().await?;
        read_only(&mut tx).await?;
        let mut plan = VectorPlan::new(query.limit);
        let vector = &query.vector;
        loop {
            sqlx::query!(
                "SELECT set_config('statement_timeout', $1, true) AS timeout, \
                        set_config('hnsw.iterative_scan', 'strict_order', true) AS iterative, \
                        set_config('hnsw.ef_search', $2, true) AS ef_search, \
                        set_config('hnsw.max_scan_tuples', $3, true) AS max_scan, \
                        set_config('hnsw.scan_mem_multiplier', '2', true) AS memory",
                READ_TIMEOUT_MS,
                plan.ef_search().to_string(),
                MAX_SCAN_TUPLES.to_string(),
            )
            .fetch_one(&mut *tx)
            .await?;
            let rows = sqlx::query!(
                r#"SELECT p.observation_id, p.revision, p.byte_start, p.byte_end, o.occurred_at
                     FROM observation_parts AS p
                     JOIN observations AS o ON (o.tenant_id, o.id) = (p.tenant_id, p.observation_id)
                     JOIN namespaces AS n ON (n.tenant_id, n.id) = (p.tenant_id, p.namespace_id)
                    WHERE p.tenant_id = $1 AND p.namespace_id = ANY($2)
                      AND p.access_level <= $3::smallint AND p.model_id = $6
                      AND n.state = 'active' AND (o.expires_at IS NULL OR o.expires_at > now())
                    ORDER BY p.embedding <=> $5::real[]::vector
                    LIMIT $4"#,
                read.tenant_id,
                &read.namespace_ids,
                read.read_ceiling.as_i16(),
                plan.nearest_parts as i64,
                vector,
                query.model_id,
            )
            .fetch_all(&mut *tx)
            .await?;
            let parts = rows.len();
            let mut seen = HashSet::new();
            let mut hits = Vec::new();
            for row in rows {
                if seen.insert(row.observation_id) {
                    hits.push(Hit {
                        id: row.observation_id,
                        occurred_at: row.occurred_at,
                        revision: row.revision,
                        byte_start: row.byte_start,
                        byte_end: row.byte_end,
                    });
                }
            }
            match plan.next(hits.len(), parts) {
                VectorStep::Complete => {
                    tx.commit().await?;
                    hits.truncate(plan.leg_candidates);
                    return Ok((hits, SemanticState::Complete));
                }
                VectorStep::Retry(next) => plan = next,
                VectorStep::Exact => {
                    let exact = self.exact_leg(&mut tx, read, query, plan).await?;
                    tx.commit().await?;
                    return Ok(match exact {
                        Some(exact) => (exact, SemanticState::Complete),
                        None => {
                            SEMANTIC_PARTIAL_TOTAL.fetch_add(1, Ordering::Relaxed);
                            crate::application::metrics::METRICS.inc(
                                "memory_search_semantic_partial_total",
                                &[],
                                1,
                            );
                            tracing::warn!(
                                metric = "memory_search_semantic_partial_total",
                                "the exact semantic scan exceeded its cap; answering partial"
                            );
                            (hits, SemanticState::Partial)
                        }
                    });
                }
            }
        }
    }

    /// The true answer when at most the exact-scan cap of parts is eligible: a bounded count,
    /// then a `MATERIALIZED` scan over the same predicate ordered by distance. `None` above it.
    async fn exact_leg(
        &self,
        db: &mut PgConnection,
        read: &ClientRead,
        query: &SearchQuery,
        plan: VectorPlan,
    ) -> TxResult<Option<Vec<Hit>>> {
        let cap = self.exact_scan_cap();
        let eligible = sqlx::query_scalar!(
            r#"SELECT count(*) AS "count!" FROM (
                 SELECT 1 FROM observation_parts AS p
                   JOIN observations AS o ON (o.tenant_id, o.id) = (p.tenant_id, p.observation_id)
                   JOIN namespaces AS n ON (n.tenant_id, n.id) = (p.tenant_id, p.namespace_id)
                  WHERE p.tenant_id = $1 AND p.namespace_id = ANY($2)
                    AND p.access_level <= $3::smallint AND p.model_id = $4
                    AND n.state = 'active' AND (o.expires_at IS NULL OR o.expires_at > now())
                  LIMIT $5) AS eligible"#,
            read.tenant_id,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
            query.model_id,
            cap + 1,
        )
        .fetch_one(&mut *db)
        .await?;
        if eligible > cap {
            return Ok(None);
        }
        let rows = sqlx::query!(
            r#"WITH eligible AS MATERIALIZED (
                 SELECT p.observation_id, p.ordinal, p.revision, p.byte_start, p.byte_end,
                        o.occurred_at, p.embedding <=> $5::real[]::vector AS distance
                   FROM observation_parts AS p
                   JOIN observations AS o ON (o.tenant_id, o.id) = (p.tenant_id, p.observation_id)
                   JOIN namespaces AS n ON (n.tenant_id, n.id) = (p.tenant_id, p.namespace_id)
                  WHERE p.tenant_id = $1 AND p.namespace_id = ANY($2)
                    AND p.access_level <= $3::smallint AND p.model_id = $4
                    AND n.state = 'active' AND (o.expires_at IS NULL OR o.expires_at > now())
               ), best AS (
                 SELECT DISTINCT ON (observation_id) * FROM eligible
                  ORDER BY observation_id, distance, ordinal
               )
               SELECT observation_id AS "observation_id!", revision AS "revision!",
                      byte_start AS "byte_start!", byte_end AS "byte_end!",
                      occurred_at AS "occurred_at!"
                 FROM best ORDER BY distance, observation_id LIMIT $6"#,
            read.tenant_id,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
            query.model_id,
            &query.vector,
            plan.leg_candidates as i64,
        )
        .fetch_all(&mut *db)
        .await?;
        Ok(Some(
            rows.into_iter()
                .map(|row| Hit {
                    id: row.observation_id,
                    occurred_at: row.occurred_at,
                    revision: row.revision,
                    byte_start: row.byte_start,
                    byte_end: row.byte_end,
                })
                .collect(),
        ))
    }

    /// The fused, re-filtered search candidates with the columns `shape` asks for, their
    /// provenance through the shared D4 filter, in fused order (search and context packs).
    pub(super) async fn recall_once(
        &self,
        read: &ClientRead,
        query: &SearchQuery,
        shape: RowShape,
    ) -> TxResult<Recalled> {
        if query.vector.len() != DIMENSIONS {
            return Err(stored("The query embedding has the wrong width"));
        }
        let (semantic_hits, semantic) = self.vector_leg(read, query).await?;
        let plan = VectorPlan::new(query.limit);
        let mut tx = self.begin().await?;
        read_only(&mut tx).await?;
        sqlx::query!(
            "SELECT set_config('statement_timeout', $1, true)",
            READ_TIMEOUT_MS
        )
        .fetch_one(&mut *tx)
        .await?;
        let keyword = keyword_leg(&mut tx, read, query, plan.leg_candidates).await?;
        let semantic_ranked: Vec<Ranked> = semantic_hits
            .iter()
            .map(|hit| Ranked {
                id: hit.id,
                occurred_at: hit.occurred_at,
            })
            .collect();
        let fused = fuse(&keyword, &semantic_ranked);
        let candidates: Vec<Uuid> = fused.iter().map(|(ranked, _)| ranked.id).collect();
        // The semantic leg ran in another snapshot: re-apply the predicate here, so nothing
        // deleted, expired or relabelled since is returned, then keep the best `limit`.
        let eligible: HashSet<Uuid> = sqlx::query_scalar!(
            "SELECT o.id FROM observations AS o \
               JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id) \
              WHERE o.tenant_id = $1 AND o.id = ANY($2) AND o.namespace_id = ANY($3) \
                AND o.access_level <= $4::smallint AND n.state = 'active' \
                AND (o.expires_at IS NULL OR o.expires_at > now())",
            read.tenant_id,
            &candidates,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
        let best: HashMap<Uuid, Hit> = semantic_hits.iter().map(|hit| (hit.id, *hit)).collect();
        let visible: Vec<Uuid> = candidates
            .into_iter()
            .filter(|id| eligible.contains(id))
            .collect();
        // Either leg filled its bounded window, or more visible candidates were fused than
        // the limit keeps: more may exist (only visible rows are ever counted).
        let truncated = visible.len() > query.limit
            || keyword.len() >= plan.leg_candidates
            || semantic_hits.len() >= plan.leg_candidates;
        let chosen: Vec<Uuid> = visible.into_iter().take(query.limit).collect();
        let (mut starts, mut ends, mut revisions) = (Vec::new(), Vec::new(), Vec::new());
        for id in &chosen {
            let hit = best.get(id);
            starts.push(hit.map_or(0, |hit| hit.byte_start));
            ends.push(hit.map_or(0, |hit| hit.byte_end));
            revisions.push(hit.map_or(0, |hit| hit.revision));
        }
        // Snippet: the best part's byte range at the part's revision (its keyword fragment when
        // the query's words occur in it), else one ts_headline fragment of the body; at most
        // 40 words, on at most `limit` final rows (≤ 100 for search, ≤ 200 for a compact
        // pack), then cut to 320 bytes. The content digest and body size only when asked for.
        let rows = sqlx::query!(
            r#"WITH lexemes AS (
                 SELECT t.lexeme FROM unnest(to_tsvector('simple', $5)) AS t
                  ORDER BY t.positions[1], t.lexeme LIMIT $9
               ), q AS (
                 SELECT string_agg('''' || replace(replace(lexeme, '\', '\\'), '''', '''''') || '''',
                                   ' | ')::tsquery AS query FROM lexemes
               )
               SELECT o.id, n.external_id AS namespace, o.kind, o.title, o.occurred_at,
                      o.access_level::smallint AS "level!", o.revision, o.index_state,
                      o.expires_at,
                      CASE WHEN $11::bool
                           THEN encode(sha256(convert_to(o.title, 'UTF8') || decode('00', 'hex')
                                              || convert_to(o.body, 'UTF8')), 'hex')
                      END AS digest,
                      CASE WHEN $12::bool THEN octet_length(to_json(o.body)::text) END
                        AS body_json_bytes,
                      CASE WHEN NOT $10::bool THEN ''
                           WHEN pt.part IS NOT NULL AND q.query IS NOT NULL
                                AND to_tsvector('simple', pt.part) @@ q.query
                           THEN ts_headline('simple', pt.part, q.query,
                                'MaxFragments=1, MaxWords=40, MinWords=5, StartSel="", StopSel=""')
                           WHEN pt.part IS NOT NULL THEN pt.part
                           WHEN q.query IS NULL OR o.body = '' THEN ''
                           ELSE ts_headline('simple', o.body, q.query,
                                'MaxFragments=1, MaxWords=40, MinWords=5, StartSel="", StopSel=""')
                      END AS "snippet!"
                 FROM UNNEST($2::uuid[], $6::int[], $7::int[], $8::bigint[])
                      AS r(id, byte_start, byte_end, revision)
                CROSS JOIN q
                 JOIN observations AS o ON o.tenant_id = $1 AND o.id = r.id
                 JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
                CROSS JOIN LATERAL (
                      SELECT CASE WHEN r.byte_end > r.byte_start AND o.revision = r.revision
                                  THEN convert_from(substring(convert_to(o.body, 'UTF8')
                                       FROM r.byte_start + 1 FOR r.byte_end - r.byte_start), 'UTF8')
                             END AS part) AS pt
                WHERE o.namespace_id = ANY($3) AND o.access_level <= $4::smallint
                  AND n.state = 'active' AND (o.expires_at IS NULL OR o.expires_at > now())"#,
            read.tenant_id,
            &chosen,
            &read.namespace_ids,
            read.read_ceiling.as_i16(),
            query.text,
            &starts,
            &ends,
            &revisions,
            MAX_QUERY_LEXEMES as i64,
            shape.snippet,
            shape.digest,
            shape.body_size,
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut links = provenance(&mut tx, read, &chosen).await?;
        tx.commit().await?;
        let mut by_id = HashMap::new();
        for row in rows {
            by_id.insert(row.id, row);
        }
        let mut recalled = Recalled {
            rows: Vec::with_capacity(chosen.len()),
            semantic,
            truncated,
        };
        for id in chosen {
            let Some(row) = by_id.remove(&id) else {
                continue;
            };
            recalled.rows.push(RecallRow {
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
                sources: links.remove(&id).unwrap_or_default(),
            });
        }
        Ok(recalled)
    }

    async fn search_once(
        &self,
        read: &ClientRead,
        query: &SearchQuery,
    ) -> TxResult<SearchResponse> {
        let recalled = self.recall_once(read, query, RowShape::SEARCH).await?;
        let mut response = SearchResponse {
            items: Vec::with_capacity(recalled.rows.len()),
            exposed_level: AccessLevel::PUBLIC,
            semantic: recalled.semantic,
        };
        for row in recalled.rows {
            response.exposed_level = response.exposed_level.max(row.exposed_level());
            response.items.push(row.index_item()?);
        }
        Ok(response)
    }

    async fn commit_index_once(
        &self,
        job: &IndexJob,
        model_id: &str,
        parts: &[IndexedPart],
    ) -> TxResult<IndexCommit> {
        let mut tx = self.begin().await?;
        // An observation never changes namespace, so this unlocked lookup is stable.
        let Some(namespace) = sqlx::query_scalar!(
            "SELECT namespace_id FROM observations WHERE tenant_id = $1 AND id = $2",
            job.tenant_id,
            job.observation_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(IndexCommit::Stale);
        };
        // D13: parents FOR SHARE before the child row.
        let tenant = sqlx::query_scalar!(
            "SELECT state FROM tenants WHERE id = $1 FOR SHARE",
            job.tenant_id
        )
        .fetch_optional(&mut *tx)
        .await?;
        let namespace_state = sqlx::query_scalar!(
            "SELECT state FROM namespaces WHERE tenant_id = $1 AND id = $2 FOR SHARE",
            job.tenant_id,
            namespace,
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = sqlx::query!(
            r#"SELECT revision, index_fence, access_level::smallint AS "level!",
                      (expires_at IS NOT NULL AND expires_at <= now()) AS "expired!"
                 FROM observations WHERE tenant_id = $1 AND id = $2 FOR NO KEY UPDATE"#,
            job.tenant_id,
            job.observation_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(IndexCommit::Stale);
        };
        if row.revision != job.revision || row.index_fence != job.fence {
            return Ok(IndexCommit::Stale);
        }
        if tenant.as_deref() != Some("active")
            || namespace_state.as_deref() != Some("active")
            || row.expired
        {
            sqlx::query!(
                "UPDATE observations SET index_lease_until = NULL, \
                        index_attempts = greatest(index_attempts - 1, 0), \
                        next_index_at = now() + interval '30 seconds' \
                  WHERE tenant_id = $1 AND id = $2",
                job.tenant_id,
                job.observation_id,
            )
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(IndexCommit::Deferred);
        }
        let (mut ordinals, mut starts, mut ends, mut vectors) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (ordinal, part) in parts.iter().enumerate() {
            if part.vector.len() != DIMENSIONS
                || part.byte_end > job.body.len()
                || part.byte_start > part.byte_end
            {
                return Err(stored("An indexed part is invalid"));
            }
            ordinals.push(ordinal as i32);
            starts.push(part.byte_start as i32);
            ends.push(part.byte_end as i32);
            vectors.push(vector_text(&part.vector));
        }
        sqlx::query!(
            "DELETE FROM observation_parts WHERE tenant_id = $1 AND observation_id = $2",
            job.tenant_id,
            job.observation_id,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO observation_parts (tenant_id, observation_id, namespace_id, access_level, \
             revision, ordinal, byte_start, byte_end, model_id, embedding) \
             SELECT $1, $2, $3, $4::smallint, $5, u.ordinal, u.byte_start, u.byte_end, $6, \
                    u.embedding::vector \
               FROM UNNEST($7::int[], $8::int[], $9::int[], $10::text[]) \
                    AS u(ordinal, byte_start, byte_end, embedding)",
            job.tenant_id,
            job.observation_id,
            namespace,
            row.level,
            job.revision,
            model_id,
            &ordinals,
            &starts,
            &ends,
            &vectors,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE observations SET index_state = 'indexed', index_error = NULL, \
                    index_lease_until = NULL, index_attempts = 0 \
              WHERE tenant_id = $1 AND id = $2",
            job.tenant_id,
            job.observation_id,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(IndexCommit::Indexed)
    }
}

#[async_trait]
impl SearchStore for PgStore {
    async fn search_observations(
        &self,
        read: &ClientRead,
        query: &SearchQuery,
    ) -> Result<SearchResponse, ServiceError> {
        with_retry(|| self.search_once(read, query)).await
    }

    async fn claim_index_jobs(
        &self,
        limit: usize,
        lease: Duration,
    ) -> Result<Vec<IndexJob>, ServiceError> {
        with_retry(|| async {
            // One statement, one level (D13): rows whose last attempt's lease expired become
            // terminal failures; due rows of active tenants and namespaces are claimed.
            let rows = sqlx::query!(
                r#"WITH exhausted AS (
                     UPDATE observations SET index_state = 'failed', index_error = 'lease_expired',
                            index_lease_until = NULL
                      WHERE (tenant_id, id) IN (
                            SELECT tenant_id, id FROM observations
                             WHERE index_state = 'pending' AND index_attempts >= 8
                               AND index_lease_until < now()
                               FOR NO KEY UPDATE SKIP LOCKED)
                     RETURNING id
                   ), due AS (
                     SELECT o.tenant_id, o.id FROM observations AS o
                       JOIN tenants AS t ON t.id = o.tenant_id
                       JOIN namespaces AS n ON (n.tenant_id, n.id) = (o.tenant_id, o.namespace_id)
                      WHERE o.index_state IN ('pending', 'failed') AND o.index_attempts < 8
                        AND o.next_index_at <= now()
                        AND (o.index_lease_until IS NULL OR o.index_lease_until < now())
                        AND t.state = 'active' AND n.state = 'active'
                        AND (o.expires_at IS NULL OR o.expires_at > now())
                      ORDER BY o.next_index_at, o.id
                      LIMIT $1
                        FOR NO KEY UPDATE OF o SKIP LOCKED
                   )
                   UPDATE observations AS o
                      SET index_fence = o.index_fence + 1, index_attempts = o.index_attempts + 1,
                          index_lease_until = now() + make_interval(secs => $2)
                     FROM due WHERE (o.tenant_id, o.id) = (due.tenant_id, due.id)
                   RETURNING o.tenant_id, o.id, o.revision, o.index_fence, o.title, o.body"#,
                limit as i64,
                seconds(lease),
            )
            .fetch_all(self.pool())
            .await?;
            Ok(rows
                .into_iter()
                .map(|row| IndexJob {
                    tenant_id: row.tenant_id,
                    observation_id: row.id,
                    revision: row.revision,
                    fence: row.index_fence,
                    title: row.title,
                    body: row.body,
                })
                .collect())
        })
        .await
    }

    async fn renew_index_leases(
        &self,
        jobs: &[IndexJob],
        lease: Duration,
    ) -> Result<Vec<Uuid>, ServiceError> {
        let tenants: Vec<Uuid> = jobs.iter().map(|job| job.tenant_id).collect();
        let ids: Vec<Uuid> = jobs.iter().map(|job| job.observation_id).collect();
        let fences: Vec<i64> = jobs.iter().map(|job| job.fence).collect();
        with_retry(|| async {
            Ok(sqlx::query_scalar!(
                "UPDATE observations AS o SET index_lease_until = now() + make_interval(secs => $4) \
                   FROM UNNEST($1::uuid[], $2::uuid[], $3::bigint[]) AS h(tenant_id, id, fence) \
                  WHERE (o.tenant_id, o.id, o.index_fence) = (h.tenant_id, h.id, h.fence) \
                    AND o.index_lease_until IS NOT NULL \
                 RETURNING o.id",
                &tenants,
                &ids,
                &fences,
                seconds(lease),
            )
            .fetch_all(self.pool())
            .await?)
        })
        .await
    }

    async fn commit_index(
        &self,
        job: &IndexJob,
        model_id: &str,
        parts: &[IndexedPart],
    ) -> Result<IndexCommit, ServiceError> {
        let commit = with_retry(|| self.commit_index_once(job, model_id, parts)).await?;
        let result = match commit {
            IndexCommit::Indexed => "indexed",
            IndexCommit::Stale => "stale",
            IndexCommit::Deferred => "deferred",
        };
        crate::application::metrics::METRICS.inc(
            "memory_index_commits_total",
            &[("result", result)],
            1,
        );
        Ok(commit)
    }

    async fn fail_index(
        &self,
        job: &IndexJob,
        failure: IndexFailure,
    ) -> Result<bool, ServiceError> {
        with_retry(|| async {
            let updated = sqlx::query_scalar!(
                "UPDATE observations SET index_state = 'failed', index_error = $5, \
                        index_lease_until = NULL, \
                        next_index_at = now() + least(interval '30 seconds' \
                            * power(2, greatest(index_attempts - 1, 0)), interval '1 hour') \
                  WHERE tenant_id = $1 AND id = $2 AND index_fence = $3 AND revision = $4 \
                 RETURNING id",
                job.tenant_id,
                job.observation_id,
                job.fence,
                job.revision,
                failure.as_str(),
            )
            .fetch_optional(self.pool())
            .await?;
            Ok(updated.is_some())
        })
        .await
        .inspect(|failed| {
            if *failed {
                crate::application::metrics::METRICS.inc(
                    "memory_index_commits_total",
                    &[("result", "failed")],
                    1,
                );
            }
        })
    }

    async fn reindex_failed(&self) -> Result<u64, ServiceError> {
        with_retry(|| async {
            let mut tx = self.begin().await?;
            let count = sqlx::query_scalar!(
                r#"WITH requeued AS (
                     UPDATE observations SET index_state = 'pending', index_error = NULL,
                            index_attempts = 0, next_index_at = now(), index_lease_until = NULL,
                            index_fence = index_fence + 1
                      WHERE index_state = 'failed' AND index_attempts >= 8
                     RETURNING 1)
                   SELECT count(*) AS "count!" FROM requeued"#
            )
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query!(
                "INSERT INTO audit_events (id, actor_kind, action, target_kind, target_id, outcome, \
                 details) VALUES ($1, 'system', 'index.reindex_failed', 'service', 'observations', \
                 'ok', $2)",
                Uuid::now_v7(),
                serde_json::json!({ "requeued": count }),
            )
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(count as u64)
        })
        .await
    }

    async fn adopt_embedding_model(&self, model_id: &str) -> Result<(), ServiceError> {
        with_retry(|| async {
            sqlx::query!(
                "UPDATE service_settings SET embedding_model_id = $1 WHERE embedding_model_id IS NULL",
                model_id
            )
            .execute(self.pool())
            .await?;
            let stored_model = sqlx::query_scalar!("SELECT embedding_model_id FROM service_settings")
                .fetch_one(self.pool())
                .await?;
            if stored_model.as_deref() == Some(model_id) {
                Ok(())
            } else {
                Err(TxError::Service(ServiceError::unavailable(format!(
                    "the database was indexed with embedding model {:?}, but {model_id:?} is \
                     configured; vectors of different models are never compared, so reset the \
                     service database (no backward compatibility)",
                    stored_model.unwrap_or_default()
                ))))
            }
        })
        .await
    }

    async fn vector_capability(&self) -> Result<(), ServiceError> {
        with_retry(|| async {
            let row = sqlx::query!(
                r#"SELECT (SELECT extversion FROM pg_extension WHERE extname = 'vector') AS version,
                          (SELECT format_type(atttypid, atttypmod) FROM pg_attribute
                            WHERE attrelid = 'observation_parts'::regclass
                              AND attname = 'embedding' AND NOT attisdropped) AS column_type"#
            )
            .fetch_one(self.pool())
            .await?;
            let version: Vec<u32> = row
                .version
                .as_deref()
                .unwrap_or("0")
                .split('.')
                .map(|part| part.parse().unwrap_or(0))
                .collect();
            let recent = version.as_slice() >= [0, 8, 0].as_slice();
            let width = row.column_type.as_deref() == Some(&format!("vector({DIMENSIONS})"));
            if recent && width {
                Ok(())
            } else {
                Err(TxError::Service(ServiceError::unavailable(
                    "the vector capability is missing: pgvector >= 0.8.0 and a vector(384) \
                     part column are required; there is no keyword-only mode",
                )))
            }
        })
        .await
    }
}
