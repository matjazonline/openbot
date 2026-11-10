//! Content removal of deleted namespaces and tenants, and age-based retention (06A.1, 06A.2;
//! p06 gate A1-1, A1-4, F1, F5, F7).
//!
//! Every step is its own transaction over at most `limit` rows. Candidates are locked
//! `FOR UPDATE SKIP LOCKED` (`FOR NO KEY UPDATE` for parents being marked), so competing
//! sweepers skip each other, and only content rows are locked while content is removed. A deleted
//! namespace's content is removed only once none of its operations is unfinished: delete and purge
//! cancel or flag them, and a flagged dispatch can no longer renew its lease. Its predicates only
//! ever become true (a deleted namespace admits no write, no acceptance and no save), so a step
//! needs no second look. Marking a namespace or tenant as removed is a separate transaction that
//! locks only those parent rows and re-checks, under that lock, that no child row is left.
//! Age-based steps act on live namespaces, so they re-check their predicate in the deleting
//! statement's fresh snapshot.
use super::{PgStore, TxResult};
use crate::{
    application::ports::{Aged, Removed},
    domain::retention::{
        AUDIT_RETENTION_DAYS, IDEMPOTENCY_RETENTION_DAYS, METADATA_RETENTION_DAYS, REMOVED_LABEL,
        SPEND_RETENTION_DAYS,
    },
};
use chrono::NaiveDate;
use uuid::Uuid;

fn days(count: u32) -> f64 {
    f64::from(count) * 86_400.0
}

fn split<A, B>(rows: impl IntoIterator<Item = (A, B)>) -> (Vec<A>, Vec<B>) {
    rows.into_iter().unzip()
}

impl PgStore {
    pub(super) async fn remove_deleted_once(&self, limit: i64) -> TxResult<Removed> {
        let mut removed = Removed {
            observations: self.remove_observations(limit).await?,
            ..Removed::default()
        };
        removed.operations = self.remove_operations(limit).await?;
        removed.sources = self.remove_sources(limit).await?;
        removed.sessions = self.remove_sessions(limit).await?;
        removed.namespaces_marked = self.mark_namespaces(limit).await?;
        removed.labels = self.blank_labels(limit).await?;
        removed.tenant_rows = self.remove_tenant_rows(limit).await?;
        removed.tenants_marked = self.mark_tenants(limit).await?;
        Ok(removed)
    }

    /// Observations of removable namespaces; their parts and links (including links naming
    /// them as a parent) cascade.
    async fn remove_observations(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT o.tenant_id, o.id FROM namespaces AS n \
               JOIN observations AS o ON (o.tenant_id, o.namespace_id) = (n.tenant_id, n.id) \
              WHERE n.state = 'deleted' AND n.content_removed_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM generation_operations AS u \
                                 WHERE u.tenant_id = n.tenant_id AND u.namespace_id = n.id \
                                   AND u.finished_at IS NULL) \
              LIMIT $1 FOR UPDATE OF o SKIP LOCKED",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids) = split(rows.into_iter().map(|row| (row.tenant_id, row.id)));
        let count = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "DELETE FROM observations \
                  WHERE (tenant_id, id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[]))",
                &tenants,
                &ids,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(count)
    }

    /// Finished operations of removable namespaces that no observation names, with their
    /// source rows.
    async fn remove_operations(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT o.tenant_id, o.id FROM namespaces AS n \
               JOIN generation_operations AS o \
                 ON (o.tenant_id, o.namespace_id) = (n.tenant_id, n.id) \
              WHERE n.state = 'deleted' AND n.content_removed_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM generation_operations AS u \
                                 WHERE u.tenant_id = n.tenant_id AND u.namespace_id = n.id \
                                   AND u.finished_at IS NULL) \
                AND NOT EXISTS (SELECT 1 FROM observations AS x \
                                 WHERE x.tenant_id = o.tenant_id \
                                   AND x.generation_operation_id = o.id) \
              LIMIT $1 FOR UPDATE OF o SKIP LOCKED",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids) = split(rows.into_iter().map(|row| (row.tenant_id, row.id)));
        let count = delete_operations(&mut tx, &tenants, &ids).await?;
        tx.commit().await?;
        Ok(count)
    }

    /// Sources of removable namespaces that no link and no operation names.
    async fn remove_sources(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT s.tenant_id, s.id FROM namespaces AS n \
               JOIN sources AS s ON (s.tenant_id, s.namespace_id) = (n.tenant_id, n.id) \
              WHERE n.state = 'deleted' AND n.content_removed_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM generation_operations AS u \
                                 WHERE u.tenant_id = n.tenant_id AND u.namespace_id = n.id \
                                   AND u.finished_at IS NULL) \
                AND NOT EXISTS (SELECT 1 FROM observation_links AS l \
                                 WHERE l.tenant_id = s.tenant_id AND l.source_id = s.id) \
                AND NOT EXISTS (SELECT 1 FROM generation_operation_sources AS g \
                                 WHERE g.tenant_id = s.tenant_id AND g.source_id = s.id) \
              LIMIT $1 FOR UPDATE OF s SKIP LOCKED",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids) = split(rows.into_iter().map(|row| (row.tenant_id, row.id)));
        let count = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "DELETE FROM sources \
                  WHERE (tenant_id, id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[]))",
                &tenants,
                &ids,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(count)
    }

    /// Sessions of removable namespaces that no source, operation or observation names.
    async fn remove_sessions(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT c.tenant_id, c.id FROM namespaces AS n \
               JOIN sessions AS c ON (c.tenant_id, c.namespace_id) = (n.tenant_id, n.id) \
              WHERE n.state = 'deleted' AND n.content_removed_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM generation_operations AS u \
                                 WHERE u.tenant_id = n.tenant_id AND u.namespace_id = n.id \
                                   AND u.finished_at IS NULL) \
                AND NOT EXISTS (SELECT 1 FROM generation_operations AS g \
                                 WHERE (g.tenant_id, g.namespace_id) = (n.tenant_id, n.id) \
                                   AND g.session_id = c.id) \
                AND NOT EXISTS (SELECT 1 FROM sources AS s \
                                 WHERE (s.tenant_id, s.namespace_id) = (n.tenant_id, n.id) \
                                   AND s.session_id = c.id) \
                AND NOT EXISTS (SELECT 1 FROM observations AS o \
                                 WHERE (o.tenant_id, o.namespace_id) = (n.tenant_id, n.id) \
                                   AND o.session_id = c.id) \
              LIMIT $1 FOR UPDATE OF c SKIP LOCKED",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids) = split(rows.into_iter().map(|row| (row.tenant_id, row.id)));
        let count = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "DELETE FROM sessions \
                  WHERE (tenant_id, id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[]))",
                &tenants,
                &ids,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(count)
    }

    /// Mark deleted namespaces with no content left. Only the namespace rows are locked; no
    /// child can appear meanwhile, because writers, settles and indexer commits take the
    /// namespace `FOR SHARE` and refuse a deleted one. The candidates already have no children,
    /// so namespaces still being emptied never fill the batch; the update re-checks under the
    /// lock.
    async fn mark_namespaces(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT n.tenant_id, n.id FROM namespaces AS n \
              WHERE n.state = 'deleted' AND n.content_removed_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM observations AS o \
                                 WHERE (o.tenant_id, o.namespace_id) = (n.tenant_id, n.id)) \
                AND NOT EXISTS (SELECT 1 FROM generation_operations AS g \
                                 WHERE (g.tenant_id, g.namespace_id) = (n.tenant_id, n.id)) \
                AND NOT EXISTS (SELECT 1 FROM sources AS s \
                                 WHERE (s.tenant_id, s.namespace_id) = (n.tenant_id, n.id)) \
                AND NOT EXISTS (SELECT 1 FROM sessions AS c \
                                 WHERE (c.tenant_id, c.namespace_id) = (n.tenant_id, n.id)) \
              ORDER BY n.deleted_at LIMIT $1 FOR NO KEY UPDATE SKIP LOCKED",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids) = split(rows.into_iter().map(|row| (row.tenant_id, row.id)));
        let count = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "UPDATE namespaces AS n SET content_removed_at = now() \
                  WHERE (n.tenant_id, n.id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[])) \
                    AND NOT EXISTS (SELECT 1 FROM observations AS o \
                                     WHERE (o.tenant_id, o.namespace_id) = (n.tenant_id, n.id)) \
                    AND NOT EXISTS (SELECT 1 FROM generation_operations AS g \
                                     WHERE (g.tenant_id, g.namespace_id) = (n.tenant_id, n.id)) \
                    AND NOT EXISTS (SELECT 1 FROM sources AS s \
                                     WHERE (s.tenant_id, s.namespace_id) = (n.tenant_id, n.id)) \
                    AND NOT EXISTS (SELECT 1 FROM sessions AS c \
                                     WHERE (c.tenant_id, c.namespace_id) = (n.tenant_id, n.id))",
                &tenants,
                &ids,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(count)
    }

    /// Credential labels are free text: every credential of a deleted tenant gets the fixed
    /// label, keyed on the tenant's state rather than its mark (review A1 F7).
    async fn blank_labels(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT c.tenant_id, c.id FROM tenants AS t \
               JOIN credentials AS c ON c.tenant_id = t.id \
              WHERE t.state = 'deleted' AND c.label <> $2 \
              LIMIT $1 FOR NO KEY UPDATE OF c SKIP LOCKED",
            limit,
            REMOVED_LABEL,
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids) = split(rows.into_iter().map(|row| (row.tenant_id, row.id)));
        let count = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "UPDATE credentials SET label = $3 \
                  WHERE (tenant_id, id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[]))",
                &tenants,
                &ids,
                REMOVED_LABEL,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(count)
    }

    /// The tenant-level rows of a deleted tenant whose namespaces are all removed: grants,
    /// idempotency records, spend days and the schedule row, at most `limit` of each.
    async fn remove_tenant_rows(&self, limit: i64) -> TxResult<u64> {
        let mut total = 0;
        let mut tx = self.begin().await?;
        let grants = sqlx::query!(
            "SELECT g.tenant_id, g.credential_id, g.namespace_id FROM tenants AS t \
               JOIN credential_grants AS g ON g.tenant_id = t.id \
              WHERE t.state = 'deleted' AND t.content_removed_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM namespaces AS n \
                                 WHERE n.tenant_id = t.id AND n.content_removed_at IS NULL) \
              LIMIT $1 FOR UPDATE OF g SKIP LOCKED",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        if !grants.is_empty() {
            let tenants: Vec<Uuid> = grants.iter().map(|row| row.tenant_id).collect();
            let credentials: Vec<Uuid> = grants.iter().map(|row| row.credential_id).collect();
            let namespaces: Vec<Uuid> = grants.iter().map(|row| row.namespace_id).collect();
            total += sqlx::query!(
                "DELETE FROM credential_grants \
                  WHERE (tenant_id, credential_id, namespace_id) IN \
                        (SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::uuid[]))",
                &tenants,
                &credentials,
                &namespaces,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        let records = sqlx::query!(
            "SELECT r.tenant_id, r.kind, r.key FROM tenants AS t \
               JOIN idempotency_records AS r ON r.tenant_id = t.id \
              WHERE t.state = 'deleted' AND t.content_removed_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM namespaces AS n \
                                 WHERE n.tenant_id = t.id AND n.content_removed_at IS NULL) \
              LIMIT $1 FOR UPDATE OF r SKIP LOCKED",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        if !records.is_empty() {
            total += delete_records(
                &mut tx,
                records
                    .into_iter()
                    .map(|row| (row.tenant_id, row.kind, row.key)),
            )
            .await?;
        }
        let days = sqlx::query!(
            "SELECT d.tenant_id, d.day FROM tenants AS t \
               JOIN tenant_spend_days AS d ON d.tenant_id = t.id \
              WHERE t.state = 'deleted' AND t.content_removed_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM namespaces AS n \
                                 WHERE n.tenant_id = t.id AND n.content_removed_at IS NULL) \
              LIMIT $1 FOR UPDATE OF d SKIP LOCKED",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        if !days.is_empty() {
            total += delete_days(
                &mut tx,
                days.into_iter().map(|row| (row.tenant_id, row.day)),
            )
            .await?;
        }
        total += sqlx::query!(
            "DELETE FROM tenant_generation_schedule WHERE tenant_id IN ( \
               SELECT s.tenant_id FROM tenants AS t \
                 JOIN tenant_generation_schedule AS s ON s.tenant_id = t.id \
                WHERE t.state = 'deleted' AND t.content_removed_at IS NULL \
                  AND NOT EXISTS (SELECT 1 FROM namespaces AS n \
                                   WHERE n.tenant_id = t.id AND n.content_removed_at IS NULL) \
                LIMIT $1 FOR UPDATE OF s SKIP LOCKED)",
            limit,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(total)
    }

    /// Mark deleted tenants with nothing left but credentials (labels blanked) and tombstones.
    /// Only the tenant rows are locked.
    async fn mark_tenants(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let ids = sqlx::query_scalar!(
            "SELECT t.id FROM tenants AS t \
              WHERE t.state = 'deleted' AND t.content_removed_at IS NULL \
                AND NOT EXISTS (SELECT 1 FROM namespaces AS n \
                                 WHERE n.tenant_id = t.id AND n.content_removed_at IS NULL) \
                AND NOT EXISTS (SELECT 1 FROM credential_grants AS g WHERE g.tenant_id = t.id) \
                AND NOT EXISTS (SELECT 1 FROM idempotency_records AS r WHERE r.tenant_id = t.id) \
                AND NOT EXISTS (SELECT 1 FROM tenant_spend_days AS d WHERE d.tenant_id = t.id) \
                AND NOT EXISTS (SELECT 1 FROM tenant_generation_schedule AS s \
                                 WHERE s.tenant_id = t.id) \
                AND NOT EXISTS (SELECT 1 FROM credentials AS c \
                                 WHERE c.tenant_id = t.id AND c.label <> $2) \
              ORDER BY t.state_changed_at LIMIT $1 FOR NO KEY UPDATE SKIP LOCKED",
            limit,
            REMOVED_LABEL,
        )
        .fetch_all(&mut *tx)
        .await?;
        let count = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "UPDATE tenants AS t SET content_removed_at = now() \
                  WHERE t.id = ANY($1) \
                    AND NOT EXISTS (SELECT 1 FROM namespaces AS n \
                                     WHERE n.tenant_id = t.id AND n.content_removed_at IS NULL) \
                    AND NOT EXISTS (SELECT 1 FROM credential_grants AS g WHERE g.tenant_id = t.id) \
                    AND NOT EXISTS (SELECT 1 FROM idempotency_records AS r WHERE r.tenant_id = t.id) \
                    AND NOT EXISTS (SELECT 1 FROM tenant_spend_days AS d WHERE d.tenant_id = t.id) \
                    AND NOT EXISTS (SELECT 1 FROM tenant_generation_schedule AS s \
                                     WHERE s.tenant_id = t.id) \
                    AND NOT EXISTS (SELECT 1 FROM credentials AS c \
                                     WHERE c.tenant_id = t.id AND c.label <> $2)",
                &ids,
                REMOVED_LABEL,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(count)
    }

    pub(super) async fn sweep_aged_once(&self, limit: i64) -> TxResult<Aged> {
        let mut aged = Aged {
            observations: self.remove_expired_observations(limit).await?,
            ..Aged::default()
        };
        aged.idempotency_records = self.remove_old_records(limit).await?;
        aged.audit_events = self.remove_old_audit(limit).await?;
        aged.spend_days = self.remove_old_spend(limit).await?;
        aged.operations = self.remove_unreferenced_operations(limit).await?;
        aged.sources = self.remove_unreferenced_sources(limit).await?;
        Ok(aged)
    }

    /// Observations past their own `expires_at` (reads already hide them). The delete re-checks
    /// the expiry, so a correction that lands between the scan and the delete is kept.
    async fn remove_expired_observations(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT tenant_id, id FROM observations WHERE expires_at <= now() \
              ORDER BY expires_at LIMIT $1 FOR UPDATE SKIP LOCKED",
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids) = split(rows.into_iter().map(|row| (row.tenant_id, row.id)));
        let count = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "DELETE FROM observations \
                  WHERE (tenant_id, id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[])) \
                    AND expires_at <= now()",
                &tenants,
                &ids,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(count)
    }

    /// Idempotency records kept for 7 days and until their request deadline passed (D5): any
    /// later attempt carrying that deadline is refused `request_expired`.
    async fn remove_old_records(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT tenant_id, kind, key FROM idempotency_records \
              WHERE created_at < now() - make_interval(secs => $2) \
                AND request_expires_at < now() \
              ORDER BY created_at LIMIT $1 FOR UPDATE SKIP LOCKED",
            limit,
            days(IDEMPOTENCY_RETENTION_DAYS),
        )
        .fetch_all(&mut *tx)
        .await?;
        let count = delete_records(
            &mut tx,
            rows.into_iter()
                .map(|row| (row.tenant_id, row.kind, row.key)),
        )
        .await?;
        tx.commit().await?;
        Ok(count)
    }

    /// Audit events older than 400 days (the schema's trigger refuses a younger delete).
    async fn remove_old_audit(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let ids = sqlx::query_scalar!(
            "SELECT id FROM audit_events WHERE at < now() - make_interval(secs => $2) \
              ORDER BY at LIMIT $1 FOR UPDATE SKIP LOCKED",
            limit,
            days(AUDIT_RETENTION_DAYS),
        )
        .fetch_all(&mut *tx)
        .await?;
        let count = if ids.is_empty() {
            0
        } else {
            sqlx::query!("DELETE FROM audit_events WHERE id = ANY($1)", &ids)
                .execute(&mut *tx)
                .await?
                .rows_affected()
        };
        tx.commit().await?;
        Ok(count)
    }

    /// Daily spend rows older than 400 days (UTC days, as the spend accounting writes them).
    async fn remove_old_spend(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT tenant_id, day FROM tenant_spend_days \
              WHERE day < (now() AT TIME ZONE 'UTC')::date - $2::int \
              ORDER BY day LIMIT $1 FOR UPDATE SKIP LOCKED",
            limit,
            SPEND_RETENTION_DAYS as i32,
        )
        .fetch_all(&mut *tx)
        .await?;
        let count = delete_days(
            &mut tx,
            rows.into_iter().map(|row| (row.tenant_id, row.day)),
        )
        .await?;
        tx.commit().await?;
        Ok(count)
    }

    /// Finished operations no observation names, 30 days after they finished (their request
    /// was nulled then), with their source rows (review A1 F5).
    async fn remove_unreferenced_operations(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT o.tenant_id, o.id FROM generation_operations AS o \
              WHERE o.finished_at < now() - make_interval(secs => $2) \
                AND NOT EXISTS (SELECT 1 FROM observations AS x \
                                 WHERE x.tenant_id = o.tenant_id \
                                   AND x.generation_operation_id = o.id) \
              ORDER BY o.finished_at LIMIT $1 FOR UPDATE SKIP LOCKED",
            limit,
            days(METADATA_RETENTION_DAYS),
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids) = split(rows.into_iter().map(|row| (row.tenant_id, row.id)));
        let count = delete_operations(&mut tx, &tenants, &ids).await?;
        tx.commit().await?;
        Ok(count)
    }

    /// Sources whose content was nulled 30 days ago that no link and no operation names. The
    /// candidates are locked first, so a save citing one (its FK check takes a key-share lock)
    /// waits; the delete's fresh snapshot then re-checks the references committed before it.
    async fn remove_unreferenced_sources(&self, limit: i64) -> TxResult<u64> {
        let mut tx = self.begin().await?;
        let rows = sqlx::query!(
            "SELECT s.tenant_id, s.id FROM sources AS s \
              WHERE s.content IS NULL AND s.content_nulled_at < now() - make_interval(secs => $2) \
                AND NOT EXISTS (SELECT 1 FROM observation_links AS l \
                                 WHERE l.tenant_id = s.tenant_id AND l.source_id = s.id) \
                AND NOT EXISTS (SELECT 1 FROM generation_operation_sources AS g \
                                 WHERE g.tenant_id = s.tenant_id AND g.source_id = s.id) \
              ORDER BY s.content_nulled_at LIMIT $1 FOR UPDATE OF s SKIP LOCKED",
            limit,
            days(METADATA_RETENTION_DAYS),
        )
        .fetch_all(&mut *tx)
        .await?;
        let (tenants, ids) = split(rows.into_iter().map(|row| (row.tenant_id, row.id)));
        let count = if ids.is_empty() {
            0
        } else {
            sqlx::query!(
                "DELETE FROM sources AS s \
                  WHERE (s.tenant_id, s.id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[])) \
                    AND NOT EXISTS (SELECT 1 FROM observation_links AS l \
                                     WHERE l.tenant_id = s.tenant_id AND l.source_id = s.id) \
                    AND NOT EXISTS (SELECT 1 FROM generation_operation_sources AS g \
                                     WHERE g.tenant_id = s.tenant_id AND g.source_id = s.id)",
                &tenants,
                &ids,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected()
        };
        tx.commit().await?;
        Ok(count)
    }
}

async fn delete_operations(
    db: &mut sqlx::PgConnection,
    tenants: &[Uuid],
    ids: &[Uuid],
) -> TxResult<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    sqlx::query!(
        "DELETE FROM generation_operation_sources \
          WHERE (tenant_id, operation_id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[]))",
        tenants,
        ids,
    )
    .execute(&mut *db)
    .await?;
    Ok(sqlx::query!(
        "DELETE FROM generation_operations \
          WHERE (tenant_id, id) IN (SELECT * FROM UNNEST($1::uuid[], $2::uuid[]))",
        tenants,
        ids,
    )
    .execute(&mut *db)
    .await?
    .rows_affected())
}

async fn delete_records(
    db: &mut sqlx::PgConnection,
    rows: impl Iterator<Item = (Uuid, String, String)>,
) -> TxResult<u64> {
    let mut tenants = Vec::new();
    let mut kinds = Vec::new();
    let mut keys = Vec::new();
    for (tenant, kind, key) in rows {
        tenants.push(tenant);
        kinds.push(kind);
        keys.push(key);
    }
    if tenants.is_empty() {
        return Ok(0);
    }
    Ok(sqlx::query!(
        "DELETE FROM idempotency_records \
          WHERE (tenant_id, kind, key) IN \
                (SELECT * FROM UNNEST($1::uuid[], $2::text[], $3::text[]))",
        &tenants,
        &kinds,
        &keys,
    )
    .execute(&mut *db)
    .await?
    .rows_affected())
}

async fn delete_days(
    db: &mut sqlx::PgConnection,
    rows: impl Iterator<Item = (Uuid, NaiveDate)>,
) -> TxResult<u64> {
    let (tenants, days): (Vec<Uuid>, Vec<NaiveDate>) = rows.unzip();
    if tenants.is_empty() {
        return Ok(0);
    }
    Ok(sqlx::query!(
        "DELETE FROM tenant_spend_days \
          WHERE (tenant_id, day) IN (SELECT * FROM UNNEST($1::uuid[], $2::date[]))",
        &tenants,
        &days,
    )
    .execute(&mut *db)
    .await?
    .rows_affected())
}
