//! The fenced lifecycle store (E2). A claim is one global `UPDATE … FROM (… FOR UPDATE SKIP
//! LOCKED)`; every completion requires the claim's fence while the row is still claimed, and
//! settling also its generation. Completions lock the tenant or namespace row before the
//! lifecycle row, the order every other memory path takes.
use super::*;
use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::credentials::envelope::CredentialContext;
use crate::application::memory::{
    CredentialReverification, IssuedClientCredential, LifecycleClaim, LifecycleCompletion,
    LifecycleDesired, LifecycleObserved, LifecycleResource, MAX_LIFECYCLE_ATTEMPTS,
    MemoryLifecycleStore, NamespaceExternalId, RetryBudget, TenantExternalId,
};
use async_trait::async_trait;
use sqlx::PgConnection;
use std::time::Duration;

/// The longest backoff (E2: 2ⁿ s to 5 min).
const MAX_BACKOFF: Duration = Duration::from_secs(300);

#[derive(sqlx::FromRow)]
struct ClaimRow {
    id: Uuid,
    company_id: Uuid,
    resource: String,
    namespace_id: Option<Uuid>,
    owner_kind: String,
    owner_id: Uuid,
    provisioner_credential_id: Uuid,
    desired: String,
    generation: i64,
    fence: i64,
    attempts: i32,
    transient_retries: i32,
}

fn invalid() -> AppError {
    AppError::Database("Invalid stored memory lifecycle".into())
}

impl ClaimRow {
    fn claim(&self) -> AppResult<LifecycleClaim> {
        let resource = match (self.resource.as_str(), self.namespace_id) {
            ("tenant", None) => LifecycleResource::Tenant,
            ("namespace", Some(id)) => LifecycleResource::Namespace(NamespaceExternalId(id)),
            _ => return Err(invalid()),
        };
        let desired = match self.desired.as_str() {
            "present" => LifecycleDesired::Present,
            "disabled" => LifecycleDesired::Disabled,
            "absent" => LifecycleDesired::Absent,
            _ => return Err(invalid()),
        };
        Ok(LifecycleClaim {
            id: self.id,
            tenant: TenantExternalId(self.company_id),
            resource,
            provisioner_credential_id: self.provisioner_credential_id,
            desired,
            generation: self.generation,
            fence: self.fence,
            attempts: self.attempts,
            transient_retries: self.transient_retries,
        })
    }
}

/// The owner a `present` row needs: the company for a tenant; for a namespace its live row and
/// its agent or member. Gone means the row must become `absent` (E2).
async fn owner_live(db: &mut PgConnection, row: &ClaimRow) -> AppResult<bool> {
    let live: bool = match (row.resource.as_str(), row.owner_kind.as_str()) {
        ("tenant", _) => {
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM companies WHERE id = $1)")
                .bind(row.company_id)
                .fetch_one(&mut *db)
                .await?
        }
        (_, kind) => {
            let owner = match kind {
                "company" => "SELECT 1 FROM companies AS company WHERE company.id = $1",
                "agent" => {
                    "SELECT 1 FROM agents AS agent WHERE agent.id = $3 AND agent.company_id = $1"
                }
                "user" => {
                    "SELECT 1 FROM company_members AS member \
                     WHERE member.company_id = $1 AND member.user_id = $3"
                }
                _ => return Err(invalid()),
            };
            sqlx::query_scalar(&format!(
                "SELECT EXISTS (SELECT 1 FROM memory_namespaces AS namespace \
                   WHERE namespace.company_id = $1 AND namespace.id = $2 \
                     AND namespace.state <> 'removing') AND EXISTS ({owner})"
            ))
            .bind(row.company_id)
            .bind(row.namespace_id)
            .bind(row.owner_id)
            .fetch_one(&mut *db)
            .await?
        }
    };
    Ok(live)
}

/// What the claimed row holds now, under its lock.
enum Fenced {
    Current { attempts: i32 },
    Superseded,
    Lost,
}

/// Lock the lifecycle row and check the fence (and, for `Superseded`, re-pend it without an
/// attempt so the new desired state is reconciled next).
async fn fence_on(db: &mut PgConnection, claim: &LifecycleClaim) -> AppResult<Fenced> {
    let row: Option<(String, i64, i64, i32)> = sqlx::query_as(
        "SELECT lifecycle.state, lifecycle.fence, lifecycle.generation, lifecycle.attempts \
         FROM memory_lifecycles AS lifecycle WHERE lifecycle.id = $1 FOR UPDATE",
    )
    .bind(claim.id)
    .fetch_optional(&mut *db)
    .await?;
    let Some((state, fence, generation, attempts)) = row else {
        return Ok(Fenced::Lost);
    };
    if state != "claimed" || fence != claim.fence {
        return Ok(Fenced::Lost);
    }
    if generation != claim.generation {
        sqlx::query(
            "UPDATE memory_lifecycles AS lifecycle SET state = 'pending', lease_owner = NULL, \
               lease_until = NULL, attempts = 0, transient_retries = 0, \
               available_at = clock_timestamp(), updated_at = clock_timestamp() \
             WHERE lifecycle.id = $1",
        )
        .bind(claim.id)
        .execute(&mut *db)
        .await?;
        return Ok(Fenced::Superseded);
    }
    Ok(Fenced::Current { attempts })
}

/// The resource row first (tenant or namespace), then the lifecycle: the shared lock order.
async fn lock_resource(db: &mut PgConnection, claim: &LifecycleClaim) -> AppResult<()> {
    match claim.resource {
        LifecycleResource::Tenant => {
            sqlx::query("SELECT 1 FROM memory_tenants WHERE company_id = $1 FOR UPDATE")
                .bind(claim.tenant.0)
                .fetch_optional(&mut *db)
                .await?;
        }
        LifecycleResource::Namespace(namespace) => {
            sqlx::query(
                "SELECT 1 FROM memory_namespaces WHERE company_id = $1 AND id = $2 FOR UPDATE",
            )
            .bind(claim.tenant.0)
            .bind(namespace.0)
            .fetch_optional(&mut *db)
            .await?;
        }
    }
    Ok(())
}

/// 2ⁿ s, never below a `Retry-After` and never above five minutes. The exponential term is a
/// floor, so `Retry-After: 0` (or 1) cannot make an unbounded outage retry loop hot (r2 m4).
fn backoff(attempts: i32, retry_after: Option<Duration>) -> Duration {
    Duration::from_secs(1 << attempts.clamp(1, 9))
        .max(retry_after.unwrap_or_default())
        .min(MAX_BACKOFF)
}

#[cfg(test)]
#[test]
fn memory_integration_backoff_floors_retry_after_and_caps_at_five_minutes() {
    assert_eq!(backoff(1, Some(Duration::ZERO)), Duration::from_secs(2));
    assert_eq!(
        backoff(3, Some(Duration::from_secs(1))),
        Duration::from_secs(8)
    );
    assert_eq!(
        backoff(1, Some(Duration::from_secs(60))),
        Duration::from_secs(60)
    );
    assert_eq!(backoff(20, None), MAX_BACKOFF);
    assert_eq!(backoff(1, Some(Duration::from_secs(3_600))), MAX_BACKOFF);
}

async fn settle_on(
    db: &mut PgConnection,
    claim: &LifecycleClaim,
    observed: LifecycleObserved,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE memory_lifecycles AS lifecycle SET state = 'settled', observed = $2, \
           lease_owner = NULL, lease_until = NULL, attempts = 0, transient_retries = 0, \
           outcome_code = NULL, updated_at = clock_timestamp() \
         WHERE lifecycle.id = $1",
    )
    .bind(claim.id)
    .bind(observed.as_str())
    .execute(&mut *db)
    .await?;
    Ok(())
}

/// Expired leases out of attempts fail; the expiry scan is served by `memory_lifecycles_leased`.
pub(super) const EXPIRE_LEASES: &str = "UPDATE memory_lifecycles AS lifecycle SET state = 'failed', attempts = $1, \
               outcome_code = 'lease_expired', lease_owner = NULL, lease_until = NULL, \
               updated_at = clock_timestamp() \
             WHERE lifecycle.id IN (SELECT expired.id FROM memory_lifecycles AS expired \
               WHERE expired.state = 'claimed' AND expired.lease_until <= clock_timestamp() \
                 AND expired.attempts + 1 >= $1 FOR UPDATE SKIP LOCKED)";

/// One due row: pending and available, or claimed with an expired lease (the second arm is
/// served by `memory_lifecycles_leased`, p06 A3-2).
pub(super) const CLAIM: &str = "UPDATE memory_lifecycles AS lifecycle SET state = 'claimed', \
               fence = lifecycle.fence + 1, lease_owner = $1, \
               lease_until = clock_timestamp() + $2 * interval '1 millisecond', \
               attempts = lifecycle.attempts + CASE WHEN lifecycle.state = 'claimed' THEN 1 ELSE 0 END, \
               updated_at = clock_timestamp() \
             FROM (SELECT candidate.id FROM memory_lifecycles AS candidate \
                   WHERE (candidate.state = 'pending' AND candidate.available_at <= clock_timestamp()) \
                      OR (candidate.state = 'claimed' AND candidate.lease_until <= clock_timestamp()) \
                   ORDER BY candidate.available_at, candidate.id LIMIT 1 \
                   FOR UPDATE SKIP LOCKED) AS due \
             WHERE lifecycle.id = due.id \
             RETURNING lifecycle.id, lifecycle.company_id, lifecycle.resource, \
               lifecycle.namespace_id, lifecycle.owner_kind, lifecycle.owner_id, \
               lifecycle.provisioner_credential_id, lifecycle.desired, lifecycle.generation, \
               lifecycle.fence, lifecycle.attempts, lifecycle.transient_retries";

impl PostgresPersistence {
    /// One fenced completion: lock the resource, check the fence, then `apply`.
    async fn complete_lifecycle<F>(
        &self,
        claim: &LifecycleClaim,
        apply: F,
    ) -> AppResult<LifecycleCompletion>
    where
        F: for<'c> FnOnce(
                &'c mut PgConnection,
                i32,
            ) -> futures::future::BoxFuture<'c, AppResult<()>>
            + Send,
    {
        let mut tx = self.pool().begin().await?;
        lock_resource(&mut tx, claim).await?;
        let completion = match fence_on(&mut tx, claim).await? {
            Fenced::Lost => {
                tx.rollback().await?;
                return Ok(LifecycleCompletion::Lost);
            }
            Fenced::Superseded => LifecycleCompletion::Superseded,
            Fenced::Current { attempts } => {
                apply(&mut tx, attempts).await?;
                LifecycleCompletion::Applied
            }
        };
        tx.commit().await?;
        Ok(completion)
    }
}

#[async_trait]
impl MemoryLifecycleStore for PostgresPersistence {
    async fn claim_lifecycle(
        &self,
        worker: Uuid,
        lease: Duration,
    ) -> AppResult<Option<LifecycleClaim>> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '2s'")
            .execute(&mut *tx)
            .await?;
        // An expired lease consumes an attempt; one expiring on the last attempt is terminal.
        sqlx::query(EXPIRE_LEASES)
            .bind(MAX_LIFECYCLE_ATTEMPTS)
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query_as::<_, ClaimRow>(CLAIM)
            .bind(worker)
            .bind(i64::try_from(lease.as_millis()).map_err(|_| invalid())?)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(mut row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        if row.desired != "absent" && !owner_live(&mut tx, &row).await? {
            // The owner went without its hook (or before it): never provision for nobody.
            sqlx::query(
                "UPDATE memory_lifecycles AS lifecycle SET desired = 'absent', \
                   generation = lifecycle.generation + 1, updated_at = clock_timestamp() \
                 WHERE lifecycle.id = $1",
            )
            .bind(row.id)
            .execute(&mut *tx)
            .await?;
            row.desired = "absent".into();
            row.generation += 1;
        }
        let claim = row.claim()?;
        tx.commit().await?;
        Ok(Some(claim))
    }

    async fn tenant_ready(&self, claim: &LifecycleClaim) -> AppResult<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM memory_tenants AS tenant \
               JOIN memory_lifecycles AS lifecycle ON lifecycle.company_id = tenant.company_id \
                 AND lifecycle.resource = 'tenant' \
               WHERE tenant.company_id = $1 AND tenant.state = 'enabled' \
                 AND lifecycle.desired = 'present' AND lifecycle.observed = 'present' \
                 AND lifecycle.state = 'settled')",
        )
        .bind(claim.tenant.0)
        .fetch_one(self.pool())
        .await?)
    }

    async fn stored_credential(&self, claim: &LifecycleClaim) -> AppResult<Option<Uuid>> {
        let LifecycleResource::Namespace(namespace) = claim.resource else {
            return Err(invalid());
        };
        Ok(sqlx::query_scalar(
            "SELECT namespace.credential_id FROM memory_namespaces AS namespace \
             WHERE namespace.company_id = $1 AND namespace.id = $2",
        )
        .bind(claim.tenant.0)
        .bind(namespace.0)
        .fetch_optional(self.pool())
        .await?
        .flatten())
    }

    async fn settle_lifecycle(
        &self,
        claim: &LifecycleClaim,
        observed: LifecycleObserved,
        generation_cap: Option<u64>,
    ) -> AppResult<LifecycleCompletion> {
        let claim_for = claim.clone();
        let cap = generation_cap
            .map(i64::try_from)
            .transpose()
            .map_err(|_| invalid())?;
        self.complete_lifecycle(claim, move |db, _| {
            Box::pin(async move {
                settle_on(db, &claim_for, observed).await?;
                match claim_for.resource {
                    LifecycleResource::Tenant => {
                        // A cap-cache refresh changes neither `disable_epoch` nor the settings
                        // form's `revision`, so a form rendered by enable stays current.
                        if let Some(cap) = cap {
                            sqlx::query(
                                "UPDATE memory_tenants AS tenant SET generation_cap = $2 \
                                 WHERE tenant.company_id = $1 \
                                   AND tenant.generation_cap IS DISTINCT FROM $2",
                            )
                            .bind(claim_for.tenant.0)
                            .bind(cap)
                            .execute(&mut *db)
                            .await?;
                        }
                    }
                    LifecycleResource::Namespace(namespace) => {
                        let statement = match observed {
                            LifecycleObserved::Absent => {
                                "DELETE FROM memory_namespaces AS namespace \
                                 WHERE namespace.company_id = $1 AND namespace.id = $2"
                            }
                            _ => {
                                "UPDATE memory_namespaces AS namespace SET state = 'ready' \
                                 WHERE namespace.company_id = $1 AND namespace.id = $2 \
                                   AND namespace.state = 'provisioning' \
                                   AND namespace.credential_id IS NOT NULL"
                            }
                        };
                        sqlx::query(statement)
                            .bind(claim_for.tenant.0)
                            .bind(namespace.0)
                            .execute(&mut *db)
                            .await?;
                    }
                }
                Ok(())
            })
        })
        .await
    }

    async fn store_credential(
        &self,
        claim: &LifecycleClaim,
        credential: IssuedClientCredential,
        replaces: Option<Uuid>,
    ) -> AppResult<LifecycleCompletion> {
        let LifecycleResource::Namespace(namespace) = claim.resource else {
            return Err(invalid());
        };
        let envelope = self.credential_cipher()?.seal_envelope(
            &CredentialContext::memory_client_credential(claim.tenant.0, namespace.0),
            &credential.token,
        )?;
        let claim_for = claim.clone();
        self.complete_lifecycle(claim, move |db, _| {
            Box::pin(async move {
                // A first credential needs a provisioning row; a replacement needs the row to
                // still hold the credential it replaces (p06 A4-8), and keeps a ready row ready.
                let stored = sqlx::query(
                    "UPDATE memory_namespaces AS namespace SET credential_id = $3, \
                       credential_envelope = $4, state = 'ready' \
                     WHERE namespace.company_id = $1 AND namespace.id = $2 \
                       AND CASE WHEN $5::uuid IS NULL THEN namespace.state = 'provisioning' \
                                ELSE namespace.state IN ('provisioning', 'ready') \
                                     AND namespace.credential_id = $5 END",
                )
                .bind(claim_for.tenant.0)
                .bind(namespace.0)
                .bind(credential.credential_id)
                .bind(envelope)
                .bind(replaces)
                .execute(&mut *db)
                .await?;
                if stored.rows_affected() != 1 {
                    // Under the fence and a `present` generation only this lifecycle writes the
                    // credential, so the row must be as the step read it.
                    return Err(invalid());
                }
                settle_on(db, &claim_for, LifecycleObserved::Present).await
            })
        })
        .await
    }

    async fn retry_lifecycle(
        &self,
        claim: &LifecycleClaim,
        outcome: &'static str,
        retry_after: Option<Duration>,
        budget: RetryBudget,
    ) -> AppResult<LifecycleCompletion> {
        let id = claim.id;
        self.complete_lifecycle(claim, move |db, _| {
            Box::pin(async move {
                let (attempts, transient): (i32, i32) = sqlx::query_as(
                    "SELECT attempts, transient_retries FROM memory_lifecycles WHERE id = $1",
                )
                .bind(id)
                .fetch_one(&mut *db)
                .await?;
                let (attempts, transient, exponent) = match budget {
                    RetryBudget::Bounded => (attempts + 1, transient, attempts + 1),
                    RetryBudget::Outage => (attempts, transient + 1, transient + 1),
                };
                let delay = backoff(exponent, retry_after);
                sqlx::query(
                    "UPDATE memory_lifecycles AS lifecycle SET \
                       state = CASE WHEN $2 >= $6 THEN 'failed' ELSE 'pending' END, \
                       attempts = LEAST($2, $6), transient_retries = $3, outcome_code = $4, \
                       lease_owner = NULL, lease_until = NULL, \
                       available_at = clock_timestamp() + $5 * interval '1 millisecond', \
                       updated_at = clock_timestamp() \
                     WHERE lifecycle.id = $1",
                )
                .bind(id)
                .bind(attempts)
                .bind(transient)
                .bind(outcome)
                .bind(delay.as_millis() as i64)
                .bind(MAX_LIFECYCLE_ATTEMPTS)
                .execute(&mut *db)
                .await?;
                Ok(())
            })
        })
        .await
    }

    async fn fail_lifecycle(
        &self,
        claim: &LifecycleClaim,
        outcome: &'static str,
    ) -> AppResult<LifecycleCompletion> {
        let id = claim.id;
        self.complete_lifecycle(claim, move |db, _| {
            Box::pin(async move {
                sqlx::query(
                    "UPDATE memory_lifecycles AS lifecycle SET state = 'failed', \
                       outcome_code = $2, lease_owner = NULL, lease_until = NULL, \
                       updated_at = clock_timestamp() \
                     WHERE lifecycle.id = $1",
                )
                .bind(id)
                .bind(outcome)
                .execute(&mut *db)
                .await?;
                Ok(())
            })
        })
        .await
    }

    async fn adopt_provisioner(&self, claim: &LifecycleClaim, credential: Uuid) -> AppResult<()> {
        // The E2 order: the company's tenant row first (absent once the company is deleted),
        // then its lifecycle rows in id order, so two adoptions never deadlock.
        let mut tx = self.pool().begin().await?;
        sqlx::query("SELECT 1 FROM memory_tenants WHERE company_id = $1 FOR UPDATE")
            .bind(claim.tenant.0)
            .fetch_optional(&mut *tx)
            .await?;
        sqlx::query("SELECT 1 FROM memory_lifecycles WHERE company_id = $1 ORDER BY id FOR UPDATE")
            .bind(claim.tenant.0)
            .fetch_all(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE memory_tenants SET provisioner_credential_id = $2 \
             WHERE company_id = $1 AND provisioner_credential_id <> $2",
        )
        .bind(claim.tenant.0)
        .bind(credential)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE memory_lifecycles SET provisioner_credential_id = $2 \
             WHERE company_id = $1 AND provisioner_credential_id <> $2",
        )
        .bind(claim.tenant.0)
        .bind(credential)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn postpone_lifecycle(
        &self,
        claim: &LifecycleClaim,
        outcome: &'static str,
        delay: Duration,
    ) -> AppResult<LifecycleCompletion> {
        let id = claim.id;
        // A reclaimed expired lease already consumed its attempt; the refusal itself does not.
        self.complete_lifecycle(claim, move |db, _| {
            Box::pin(async move {
                sqlx::query(
                    "UPDATE memory_lifecycles AS lifecycle SET state = 'pending', \
                       outcome_code = $2, lease_owner = NULL, lease_until = NULL, \
                       available_at = clock_timestamp() + $3 * interval '1 millisecond', \
                       updated_at = clock_timestamp() \
                     WHERE lifecycle.id = $1",
                )
                .bind(id)
                .bind(outcome)
                .bind(delay.as_millis() as i64)
                .execute(&mut *db)
                .await?;
                Ok(())
            })
        })
        .await
    }
}

/// p06 A4-9: the compare-and-set re-pend of a refused credential's namespace lifecycle.
#[async_trait]
impl CredentialReverification for PostgresPersistence {
    async fn credential_refused(
        &self,
        tenant: TenantExternalId,
        credential: Uuid,
    ) -> AppResult<bool> {
        // Only a settled `present` row whose namespace still stores this credential moves; its
        // present step then verifies the credential or issues a new one (A4-8).
        Ok(sqlx::query(
            "UPDATE memory_lifecycles AS lifecycle SET state = 'pending', \
               generation = lifecycle.generation + 1, attempts = 0, transient_retries = 0, \
               outcome_code = 'credential_unverified', available_at = clock_timestamp(), \
               updated_at = clock_timestamp() \
             FROM memory_namespaces AS namespace \
             WHERE namespace.company_id = $1 AND namespace.credential_id = $2 \
               AND lifecycle.company_id = namespace.company_id \
               AND lifecycle.resource = 'namespace' AND lifecycle.namespace_id = namespace.id \
               AND lifecycle.desired = 'present' AND lifecycle.state = 'settled'",
        )
        .bind(tenant.0)
        .bind(credential)
        .execute(self.pool())
        .await?
        .rows_affected()
            == 1)
    }
}
