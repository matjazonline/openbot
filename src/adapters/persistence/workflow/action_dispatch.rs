//! Trusted SQL composition: authority owners and local effects share the writer transaction.
//! Hooks may only access this database; network/provider I/O is forbidden here.
use super::*;
use crate::application::workflow::actions::*;
use crate::application::workflow::binding::ResourceStatus;
use serde_json::{Value, json};
use tokio::time::Instant;

pub(super) struct AuthorizedAction {
    pub action: FrozenAction,
    pub current_policy: Value,
    pub approval_required: bool,
}

pub struct LockedActionAuthority {
    pub resource: ResourceStatus,
    pub policy: CurrentActionPolicy,
}

#[async_trait]
pub trait SqlActionAuthority: Send + Sync {
    /// Lock actual resource/policy owners against revocation through commit. Missing
    /// or unsupported owners must return an error, never cached directory observations.
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        authority: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority>;
}

#[async_trait]
pub trait SqlLocalAction: Send + Sync {
    /// Registered same-database effect only. All writes must use the supplied transaction.
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        action: &FrozenAction,
    ) -> AppResult<Value>;
}

/// Production resource/policy owners are supplied by04.9/10. There is no fallback grant.
pub struct UnavailableActionAuthority;
#[async_trait]
impl SqlActionAuthority for UnavailableActionAuthority {
    async fn lock_current(
        &self,
        _: &mut Transaction<'_, Postgres>,
        _: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority> {
        Err(missing())
    }
}

pub struct PostgresActionDispatch<A, L> {
    pub(super) persistence: PostgresPersistence,
    pub(super) authority: A,
    local: L,
}
impl<A, L> PostgresActionDispatch<A, L> {
    pub fn new(persistence: PostgresPersistence, authority: A, local: L) -> Self {
        Self {
            persistence,
            authority,
            local,
        }
    }
}

impl<A: SqlActionAuthority, L: SqlLocalAction> PostgresActionDispatch<A, L> {
    pub(super) async fn authorize_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
    ) -> AppResult<AuthorizedAction> {
        if !lease::lock_fence(tx, request.fence, request.lease).await? {
            return Err(conflict());
        }
        // The existing attempt remains the sole ownership ledger.
        let attempt: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM task_attempts WHERE task_id=$1 AND attempt_number=$2 \
             AND execution_generation=$3 AND worker_id=$4 AND status='processing' FOR UPDATE",
        )
        .bind(request.fence.scope.job.0)
        .bind(request.fence.attempt.0)
        .bind(request.fence.generation.0)
        .bind(request.fence.worker.0)
        .fetch_optional(&mut **tx)
        .await?;
        if attempt.is_none()
            || lease::live_window(tx, request.fence, Instant::now())
                .await?
                .is_none()
        {
            return Err(conflict());
        }
        let authority = Box::pin(action_authority::load_on(
            tx,
            request.scope(),
            &request.subject,
        ))
        .await?;
        validate_run_authority(request.scope(), &request.subject, &authority)?;
        let current = self.authority.lock_current(tx, &authority).await?;
        validate_resource(authority.action.request(), &current.resource)?;
        let decision = validate_current_authority(authority.action.request(), &current.policy)?;
        // Lock the exact immutable intent after all authoritative owners. No caller
        // observation or stored marker can substitute for these current checks.
        let intent: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2 \
             AND execution_id=$3 AND id=$4 AND argument_digest=$5 FOR UPDATE",
        )
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .bind(request.subject.invocation.as_uuid())
        .bind(request.subject.argument_digest.as_str())
        .fetch_optional(&mut **tx)
        .await?;
        if intent.is_none() {
            return Err(missing());
        }
        let policy = json!({"tool":current.policy.tool,"approval_required":current.policy.approval_required});
        Ok(AuthorizedAction {
            action: authority.action,
            current_policy: policy,
            approval_required: decision == ActionAccessDecision::ApprovalRequired,
        })
    }

    pub(super) async fn existing(
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
    ) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM workflow_action_dispatches WHERE company_id=$1 AND invocation_id=$2)",
        ).bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid())
            .fetch_one(&mut **tx).await?)
    }

    pub(super) async fn insert(
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        marker: Uuid,
        kind: &str,
        policy: Value,
    ) -> AppResult<()> {
        Self::insert_subject(tx, request, marker, kind, policy, None).await
    }

    pub(super) async fn insert_subject(
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        marker: Uuid,
        kind: &str,
        policy: Value,
        subject: Option<Vec<u8>>,
    ) -> AppResult<()> {
        sqlx::query("INSERT INTO workflow_action_dispatches (company_id,run_id,execution_id,invocation_id,argument_digest,id,job_id,attempt_number,execution_generation,worker_id,effect_kind,current_policy,replay_subject) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
            .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
            .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
            .bind(request.subject.argument_digest.as_str()).bind(marker)
            .bind(request.fence.scope.job.0).bind(request.fence.attempt.0)
            .bind(request.fence.generation.0).bind(request.fence.worker.0)
            .bind(kind).bind(policy).bind(subject).execute(&mut **tx).await?;
        Ok(())
    }
}

#[async_trait]
impl<A: SqlActionAuthority, L: SqlLocalAction> ActionDispatch for PostgresActionDispatch<A, L> {
    async fn local(&self, request: &ActionDispatchRequest) -> AppResult<LocalDispatchResult> {
        tokio::time::timeout(request.lease.persistence_timeout(), async {
            let started = Instant::now();
            let mut tx = self.persistence.pool().begin().await?;
            let AuthorizedAction { action, current_policy: policy, approval_required: protected } = self.authorize_on(&mut tx, request).await?;
            if protected { return Ok(LocalDispatchResult::ApprovalRequired); }
            if action_uncertainty::conflicted_on(&mut tx, request.scope()).await? { return Err(conflict()); }
            if Self::existing(&mut tx, request).await? {
                let saved: Option<Value> = sqlx::query_scalar("SELECT result FROM workflow_action_receipts WHERE company_id=$1 AND invocation_id=$2 AND argument_digest=$3")
                    .bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid())
                    .bind(request.subject.argument_digest.as_str()).fetch_optional(&mut *tx).await?;
                return match saved {
                    Some(result) => Ok(LocalDispatchResult::Committed(ActionReceipt { subject: request.subject.clone(), result: validate_action_result(&action, result)? })),
                    None => Ok(LocalDispatchResult::PossibleDispatchExists),
                };
            }
            let marker = Uuid::new_v4();
            Self::insert(&mut tx, request, marker, "local", policy).await?;
            // Box the local adapter seam to keep debug/test stacks bounded.
            let result = validate_action_result(&action, Box::pin(self.local.apply(&mut tx, &action)).await?)?;
            sqlx::query("INSERT INTO workflow_action_receipts (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) VALUES ($1,$2,$3,$4,$5,$6,'local',$7)")
                .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
                .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
                .bind(request.subject.argument_digest.as_str()).bind(marker).bind(&result).execute(&mut *tx).await?;
            lease::live_window(&mut tx, request.fence, started).await?.ok_or_else(conflict)?;
            tx.commit().await?;
            Ok(LocalDispatchResult::Committed(ActionReceipt { subject: request.subject.clone(), result }))
        }).await.map_err(|_| lease::timed_out())?
    }

    async fn remote(
        &self,
        request: &ActionDispatchRequest,
        provider: &dyn RemoteAction,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        supervise_remote(self, request, provider, cancel).await
    }
}

#[async_trait]
impl<A: SqlActionAuthority, L: SqlLocalAction> RemoteDispatch for PostgresActionDispatch<A, L> {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        tokio::time::timeout(request.lease.persistence_timeout(), async {
            let started = Instant::now();
            let mut tx = self.persistence.pool().begin().await?;
            let AuthorizedAction {
                action,
                current_policy: mut policy,
                approval_required: protected,
            } = self.authorize_on(&mut tx, request).await?;
            if protected {
                return Ok(RemoteReservationResult::ApprovalRequired);
            }
            if action_uncertainty::conflicted_on(&mut tx, request.scope()).await? {
                return Err(conflict());
            }
            if let Some(receipt) = self.receipt_on(&mut tx, request, &action).await? {
                let window = lease::live_window(&mut tx, request.fence, started)
                    .await?
                    .ok_or_else(conflict)?;
                tx.commit().await?;
                if Instant::now() >= window.expires.min(window.run_deadline) {
                    return Err(conflict());
                }
                return Ok(RemoteReservationResult::Committed(receipt));
            }
            let window = lease::live_window(&mut tx, request.fence, started)
                .await?
                .ok_or_else(conflict)?;
            let provider = self
                .provider_on(&mut tx, request, &action, contract)
                .await?;
            policy["provider_replay"] = provider.proof()?;
            let Some(marker) = self
                .marker_on(&mut tx, request, &action, policy, &provider)
                .await?
            else {
                return Ok(RemoteReservationResult::PossibleDispatchExists);
            };
            let entry = Uuid::new_v4();
            if !Self::reserve_entry_on(&mut tx, request, marker, entry).await? {
                return Ok(RemoteReservationResult::PossibleDispatchExists);
            }
            // An ambiguous acknowledgement returns no reservation. A committed marker
            // still forbids any subsequent permit, even if this process never sent.
            tx.commit().await?;
            Ok(RemoteReservationResult::Reserved(Box::new(
                RemoteReservation {
                    marker,
                    entry,
                    action,
                    request: request.clone(),
                    window,
                    provider,
                },
            )))
        })
        .await
        .map_err(|_| lease::timed_out())?
    }

    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry> {
        let request = &reservation.request;
        let window = tokio::time::timeout(request.lease.persistence_timeout(), async {
            let started = Instant::now();
            let mut tx = self.persistence.pool().begin().await?;
            let AuthorizedAction {
                approval_required: protected,
                ..
            } = self.authorize_on(&mut tx, request).await?;
            if protected {
                return Err(missing());
            }
            Self::verify_entry_on(
                &mut tx,
                request,
                reservation.marker,
                reservation.entry,
                &reservation.action,
                &reservation.provider,
            )
            .await?;
            let allowed: bool = sqlx::query_scalar("SELECT NOT workflow_action_has_evidence_conflict($1,$2) AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=$1 AND invocation_id=$3 AND disposition='applied') AND (NOT EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions WHERE company_id=$1 AND remote_entry_id=$4) OR workflow_action_not_applied_available($1,$3,$4) IS NOT NULL)")
                .bind(request.scope().company.as_uuid()).bind(request.scope().execution.as_uuid())
                .bind(request.subject.invocation.as_uuid()).bind(reservation.entry).fetch_one(&mut *tx).await?;
            if !allowed { return Err(conflict()); }
            let window = lease::live_window(&mut tx, request.fence, started)
                .await?
                .ok_or_else(conflict)?;
            tx.commit().await?;
            Ok(window)
        })
        .await
        .map_err(|_| lease::timed_out())??;
        Ok(RemoteEntry {
            marker: reservation.marker,
            entry: reservation.entry,
            request: reservation.request,
            action: reservation.action,
            provider: reservation.provider,
            deadline: window
                .expires
                .min(window.run_deadline)
                .min(reservation.window.expires)
                .min(reservation.window.run_deadline),
        })
    }

    async fn finish_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation> {
        self.commit_remote(entry, result).await
    }

    async fn owns_remote(&self, request: &ActionDispatchRequest) -> AppResult<bool> {
        tokio::time::timeout(request.lease.persistence_timeout(), async {
            let mut tx = self.persistence.pool().begin().await?;
            if !lease::lock_fence(&mut tx, request.fence, request.lease).await?
                || action_uncertainty::conflicted_on(&mut tx, request.scope()).await?
            {
                return Ok(false);
            }
            let live = lease::live_window(&mut tx, request.fence, Instant::now())
                .await?
                .is_some();
            tx.commit().await?;
            Ok(live)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
}
