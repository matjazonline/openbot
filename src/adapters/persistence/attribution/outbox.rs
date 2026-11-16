use super::*;
#[async_trait]
impl AttributionOutbox for PostgresPersistence {
    async fn health(&self) -> IdentityResult<LifecycleHealth> {
        let row = sqlx::query("SELECT EXISTS(SELECT 1 FROM attribution_lifecycle_outbox WHERE delivery_state IN ('pending', 'delivering')) AS undelivered, EXISTS(SELECT 1 FROM attribution_lifecycle_outbox WHERE delivery_state = 'failed') AS failed")
            .fetch_one(&self.pool).await.map_err(database)?;
        Ok(LifecycleHealth {
            undelivered: row.try_get("undelivered").map_err(database)?,
            failed: row.try_get("failed").map_err(database)?,
        })
    }
    async fn claim(&self) -> IdentityResult<Vec<LifecycleClaim>> {
        let mut tx = self.pool.begin().await.map_err(database)?;
        sqlx::query(r#"WITH expired AS (
                SELECT id FROM attribution_lifecycle_outbox WHERE delivery_state = 'delivering'
                    AND lease_until <= clock_timestamp() AND attempts >= 12
                ORDER BY lease_until, id LIMIT 100 FOR UPDATE SKIP LOCKED
            ) UPDATE attribution_lifecycle_outbox AS delivery SET delivery_state = 'failed',
                execution_id = NULL, lease_until = NULL, completed_at = clock_timestamp(), last_failure = 'lease_expired'
            FROM expired WHERE delivery.id = expired.id"#).execute(&mut *tx).await.map_err(database)?;
        let rows = sqlx::query(r#"WITH due AS (
                SELECT id FROM attribution_lifecycle_outbox
                WHERE attempts < 12 AND ((delivery_state = 'pending' AND available_at <= clock_timestamp())
                    OR (delivery_state = 'delivering' AND lease_until <= clock_timestamp()))
                ORDER BY available_at, id LIMIT 4 FOR UPDATE SKIP LOCKED
            ) UPDATE attribution_lifecycle_outbox AS delivery SET delivery_state = 'delivering',
                attempts = delivery.attempts + 1, execution_id = gen_random_uuid(),
                lease_until = clock_timestamp() + interval '30 seconds'
            FROM due WHERE delivery.id = due.id
            RETURNING delivery.id, delivery.execution_id, delivery.company_id, delivery.version, delivery.state, delivery.changed_at"#)
            .fetch_all(&mut *tx).await.map_err(database)?;
        let claims = rows
            .iter()
            .map(|row| {
                Ok(LifecycleClaim {
                    id: row.try_get("id").map_err(database)?,
                    execution: row.try_get("execution_id").map_err(database)?,
                    payload: status(row)?,
                })
            })
            .collect::<IdentityResult<_>>()?;
        tx.commit().await.map_err(database)?;
        Ok(claims)
    }
    async fn complete(&self, claim: &LifecycleClaim, result: DeliveryResult) -> IdentityResult<()> {
        let (delivered, failure) = match result {
            DeliveryResult::Delivered => (true, None),
            DeliveryResult::Unavailable => (false, Some("unavailable")),
            DeliveryResult::Rejected => (false, Some("rejected")),
        };
        let changed = sqlx::query(r#"UPDATE attribution_lifecycle_outbox SET
                delivery_state = CASE WHEN $3 THEN 'delivered' WHEN attempts >= 12 THEN 'failed' ELSE 'pending' END,
                execution_id = NULL, lease_until = NULL,
                completed_at = CASE WHEN $3 OR attempts >= 12 THEN clock_timestamp() ELSE NULL END,
                available_at = clock_timestamp() + make_interval(secs => LEAST(60, power(2, attempts)::integer)),
                last_failure = $4
            WHERE id = $1 AND execution_id = $2 AND delivery_state = 'delivering' AND lease_until > clock_timestamp()"#)
            .bind(claim.id).bind(claim.execution).bind(delivered).bind(failure).execute(&self.pool).await.map_err(database)?.rows_affected();
        if changed != 1 {
            return Err(IdentityError::Conflict);
        }
        Ok(())
    }
    async fn cleanup(&self) -> IdentityResult<()> {
        for table in [
            "attribution_login_continuations",
            "attribution_identity_codes",
            "attribution_identity_grants",
        ] {
            // Fixed owned table names; no caller-supplied SQL identifiers.
            sqlx::query(&format!("DELETE FROM {table} WHERE verifier IN (SELECT verifier FROM {table} WHERE expires_at <= clock_timestamp() ORDER BY expires_at LIMIT 100 FOR UPDATE SKIP LOCKED)"))
                .execute(&self.pool).await.map_err(database)?;
        }
        Ok(())
    }
}
