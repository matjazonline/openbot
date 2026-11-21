//! Managed cleanup uses the existing administrator's ownership boundary. It does
//! not issue runtime grants or make expired content visible to model clients.
use super::*;

impl PgStore {
    pub(super) async fn list_subject_once(
        &self,
        actor: Actor,
        request: &SubjectListRequest,
    ) -> TxResult<SubjectPage> {
        let mut tx = self.begin().await?;
        lock_actor(&mut tx, actor).await?;
        let tenant = lock_tenant(&mut tx, actor, &request.tenant, TenantLock::Share).await?;
        let namespace = sqlx::query_scalar!(
            "SELECT id FROM namespaces WHERE tenant_id=$1 AND external_id=$2 FOR SHARE",
            tenant.id,
            request.namespace
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| TxError::Service(ServiceError::not_found()))?;
        let limit = i64::from(request.limit) + 1;
        let rows = sqlx::query!(
            "SELECT id,revision,access_level::smallint AS access_level FROM observations
            WHERE tenant_id=$1 AND namespace_id=$2 AND subject=$3 AND ($4::uuid IS NULL OR id>$4)
            ORDER BY id LIMIT $5",
            tenant.id,
            namespace,
            request.subject,
            request.after,
            limit
        )
        .fetch_all(&mut *tx)
        .await?;
        let more = rows.len() > usize::from(request.limit);
        let mut items = Vec::new();
        for row in rows.into_iter().take(usize::from(request.limit)) {
            items.push(SubjectObservation {
                observation_id: row.id,
                revision: row.revision,
                access_level: level(row.access_level)?
                    .ok_or_else(|| stored("Subject observation level is missing"))?,
            });
        }
        let next_after = if more {
            items.last().map(|item| item.observation_id)
        } else {
            None
        };
        tx.commit().await?;
        Ok(SubjectPage { items, next_after })
    }

    pub(super) async fn delete_subject_once(
        &self,
        actor: Actor,
        request: &SubjectDeleteRequest,
    ) -> TxResult<SubjectDeleted> {
        let mut tx = self.begin().await?;
        lock_actor(&mut tx, actor).await?;
        let tenant = lock_tenant(&mut tx, actor, &request.tenant, TenantLock::Share).await?;
        // Saves hold SHARE on this exact namespace. No delete can cross an
        // admitted save or another purger, and no lock is acquired in reverse order.
        let namespace = sqlx::query_scalar!(
            "SELECT id FROM namespaces WHERE tenant_id=$1 AND external_id=$2 FOR NO KEY UPDATE",
            tenant.id,
            request.namespace
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| TxError::Service(ServiceError::not_found()))?;
        let mut deleted = sqlx::query_scalar!(
            "DELETE FROM observations WHERE tenant_id=$1 AND namespace_id=$2
            AND subject=$3 AND id=ANY($4) RETURNING id",
            tenant.id,
            namespace,
            request.subject,
            &request.observation_ids
        )
        .fetch_all(&mut *tx)
        .await?;
        deleted.sort_unstable();
        audit(
            &mut tx,
            Some(tenant.id),
            actor,
            "observation.subject_delete",
            "namespace",
            &request.namespace,
            "ok",
            serde_json::json!({"subject":request.subject,"deleted":deleted.len()}),
        )
        .await?;
        tx.commit().await?;
        Ok(SubjectDeleted { deleted })
    }
}
