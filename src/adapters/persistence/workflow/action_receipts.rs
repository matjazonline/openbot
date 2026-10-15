//! Receipts retain effect truth independently of current execution ownership.
use super::persistence_bound::{Attempt, End, Evidence, WriteBound};
use super::{action_dispatch::*, invalid, *};
use crate::application::workflow::actions::*;
use serde_json::Value;
use std::time::Duration;
use tokio::time::Instant;

impl<A: SqlActionAuthority, L: SqlLocalAction> PostgresActionDispatch<A, L> {
    pub(super) async fn receipt_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        action: &FrozenAction,
    ) -> AppResult<Option<ActionReceipt>> {
        let result: Option<Value> = sqlx::query_scalar(
            "SELECT result FROM workflow_action_receipts WHERE company_id=$1 AND run_id=$2 \
             AND execution_id=$3 AND invocation_id=$4 AND argument_digest=$5",
        )
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .bind(request.subject.invocation.as_uuid())
        .bind(request.subject.argument_digest.as_str())
        .fetch_optional(&mut **tx)
        .await?;
        result
            .map(|result| {
                Ok(ActionReceipt {
                    subject: request.subject.clone(),
                    result: validate_action_result(action, result)?,
                })
            })
            .transpose()
    }

    pub(super) async fn provider_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        action: &FrozenAction,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<ProviderInvocation> {
        // Read AFTER locking the run. LeaseWindow deliberately underestimates time
        // after lock waits; provider retention must cover the actual run horizon.
        let micros: i64 = sqlx::query_scalar(
            "SELECT ceil(extract(epoch FROM (deadline-clock_timestamp()))*1000000)::bigint \
             FROM workflow_runs WHERE company_id=$1 AND id=$2",
        )
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().run.as_uuid())
        .fetch_one(&mut **tx)
        .await?;
        let micros = u64::try_from(micros).map_err(|_| conflict())?;
        if micros == 0 {
            return Err(conflict());
        }
        prepare_provider(action, contract, Duration::from_micros(micros))
    }

    pub(super) async fn marker_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        action: &FrozenAction,
        policy: Value,
        provider: &ProviderInvocation,
    ) -> AppResult<Option<Uuid>> {
        let previous: Option<(Uuid, i32, bool)> = sqlx::query_as(
            "SELECT id,attempt_number,((current_policy->'provider_replay'=$3 AND \
             COALESCE(workflow_action_replay_supported(company_id,invocation_id),false)) OR \
             workflow_action_not_applied_available(company_id,invocation_id) IS NOT NULL) \
             FROM workflow_action_dispatches WHERE company_id=$1 AND invocation_id=$2 \
             AND effect_kind='remote' AND replay_subject=$4",
        )
        .bind(request.scope().company.as_uuid())
        .bind(request.subject.invocation.as_uuid())
        .bind(provider.proof()?)
        .bind(replay_subject(
            &action.request().contract,
            &action.request().target,
        )?)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some((marker, attempt, safe)) = previous {
            return Ok((attempt < request.fence.attempt.0 && safe).then_some(marker));
        }
        if Self::existing(tx, request).await? {
            return Ok(None);
        }
        let marker = Uuid::new_v4();
        Self::insert_subject(
            tx,
            request,
            marker,
            "remote",
            policy,
            Some(replay_subject(
                &action.request().contract,
                &action.request().target,
            )?),
        )
        .await?;
        Ok(Some(marker))
    }

    pub(super) async fn reserve_entry_on(
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        marker: Uuid,
        entry: Uuid,
    ) -> AppResult<bool> {
        let proof: Option<Uuid> =
            sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2)")
                .bind(request.scope().company.as_uuid())
                .bind(request.subject.invocation.as_uuid())
                .fetch_one(&mut **tx)
                .await?;
        let inserted = sqlx::query(
            "INSERT INTO workflow_action_remote_entries \
             (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,job_id,attempt_number,execution_generation,worker_id) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT (company_id,invocation_id,job_id,attempt_number) DO NOTHING",
        ).bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
            .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
            .bind(request.subject.argument_digest.as_str()).bind(marker).bind(entry)
            .bind(request.fence.scope.job.0).bind(request.fence.attempt.0)
            .bind(request.fence.generation.0).bind(request.fence.worker.0)
            .execute(&mut **tx).await?.rows_affected() == 1;
        if inserted && let Some(proof) = proof {
            sqlx::query("INSERT INTO workflow_action_evidence_consumptions(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
                .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid()).bind(request.scope().execution.as_uuid())
                .bind(request.subject.invocation.as_uuid()).bind(request.subject.argument_digest.as_str()).bind(marker).bind(proof).bind(entry)
                .execute(&mut **tx).await?;
        }
        Ok(inserted)
    }

    pub(super) async fn verify_entry_on(
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        marker: Uuid,
        entry: Uuid,
        action: &FrozenAction,
        provider: &ProviderInvocation,
    ) -> AppResult<()> {
        let exact: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry \
             JOIN workflow_action_dispatches AS marker ON marker.company_id=entry.company_id AND marker.id=entry.dispatch_id \
             JOIN workflow_action_intents AS intent ON intent.company_id=entry.company_id AND intent.id=entry.invocation_id \
             WHERE entry.company_id=$1 AND entry.run_id=$2 AND entry.execution_id=$3 AND entry.invocation_id=$4 \
             AND entry.argument_digest=$5 AND entry.dispatch_id=$6 AND entry.id=$7 AND entry.job_id=$8 \
             AND entry.attempt_number=$9 AND entry.execution_generation=$10 AND entry.worker_id=$11 \
             AND marker.effect_kind='remote' AND (marker.current_policy->'provider_replay'=$12 OR EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions AS consumption WHERE consumption.company_id=entry.company_id AND consumption.remote_entry_id=entry.id)) \
             AND marker.replay_subject=$13 AND intent.operation=$14)",
        ).bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
            .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
            .bind(request.subject.argument_digest.as_str()).bind(marker).bind(entry)
            .bind(request.fence.scope.job.0).bind(request.fence.attempt.0)
            .bind(request.fence.generation.0).bind(request.fence.worker.0).bind(provider.proof()?)
            .bind(replay_subject(&action.request().contract,&action.request().target)?)
            .bind(action.saved_operation()).fetch_one(&mut **tx).await?;
        if !exact {
            return Err(conflict());
        }
        Ok(())
    }

    pub(super) async fn commit_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation> {
        let first = WriteBound::start("action.commit_remote", entry.request.lease);
        let receipt = match Box::pin(self.commit_once(&first, &entry, result.clone())).await? {
            Some(receipt) => receipt,
            // The provider already applied the effect and this receipt definitely wrote
            // nothing: one more try within what remains of the same deadline T (both inserts
            // are idempotent). Still not applied, the error leaves the marker to the
            // reconciliation path, as before.
            None => {
                let retry = WriteBound::within(
                    "action.commit_remote",
                    entry.request.lease,
                    first.deadline(),
                );
                Box::pin(self.commit_once(&retry, &entry, result))
                    .await?
                    .ok_or_else(|| retry.not_applied())?
            }
        };
        if !self.return_authorized(&entry).await? {
            return Ok(RemoteDispatchObservation::Interrupted);
        }
        Ok(RemoteDispatchObservation::Committed(receipt))
    }

    /// One bounded receipt transaction: `None` when it definitely wrote nothing (the attempt
    /// elapsed or the server bound fired before COMMIT, or resolution proved it aborted).
    async fn commit_once(
        &self,
        bound: &WriteBound,
        entry: &RemoteEntry,
        result: Value,
    ) -> AppResult<Option<ActionReceipt>> {
        let raw = result.clone();
        let attempt = bound
            .attempt(Box::pin(async {
                // After the external effect: wait for a connection for the rest of the
                // attempt, not the pre-effect acquire bound.
                let mut conn = bound.acquire_for_attempt(self.persistence.pool()).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let receipt = async {
                    // Same run-first order, deliberately no live-fence/access guard: late
                    // bounded truth remains auditable after cancellation or ownership loss.
                    sqlx::query(
                        "SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE",
                    )
                    .bind(entry.request.scope().company.as_uuid())
                    .bind(entry.request.scope().run.as_uuid())
                    .execute(&mut *tx)
                    .await?;
                    super::persistence_bound::locks_held();
                    Self::verify_entry_on(
                        &mut tx,
                        &entry.request,
                        entry.marker,
                        entry.entry,
                        &entry.action,
                        &entry.provider,
                    )
                    .await?;
                    let result = validate_action_result(&entry.action, result)?;
                    Self::insert_receipt_on(&mut tx, entry, result).await?;
                    let receipt = self
                        .receipt_on(&mut tx, &entry.request, &entry.action)
                        .await?
                        .ok_or_else(invalid)?;
                    Ok(End::Commit(receipt))
                }
                .await;
                bound.end(tx, receipt).await
            }))
            .await;
        match attempt {
            Attempt::Done(receipt) => Ok(Some(receipt)),
            Attempt::Failed(AppError::DatabaseTimeout(_)) => Ok(None),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => {
                bound
                    .resolve_retryable(Box::pin(self.observed_on(bound, entry, raw)))
                    .await
            }
        }
    }

    async fn insert_receipt_on(
        tx: &mut Transaction<'_, Postgres>,
        entry: &RemoteEntry,
        result: Value,
    ) -> AppResult<()> {
        let request = &entry.request;
        // Record actual truth even when an earlier canonical receipt wins below.
        // The owning run lock serializes conflict detection with shared completion.
        sqlx::query("INSERT INTO workflow_action_actual_receipt_observations \
            (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,result) \
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (company_id,remote_entry_id,result_digest) DO NOTHING")
            .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
            .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
            .bind(request.subject.argument_digest.as_str()).bind(entry.marker).bind(entry.entry).bind(&result)
            .execute(&mut **tx).await?;
        sqlx::query("INSERT INTO workflow_action_receipts \
            (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,remote_entry_id,result) \
            VALUES ($1,$2,$3,$4,$5,$6,'remote',$7,$8) ON CONFLICT (company_id,invocation_id) DO NOTHING")
            .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
            .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
            .bind(request.subject.argument_digest.as_str()).bind(entry.marker).bind(entry.entry).bind(result)
            .execute(&mut **tx).await?;
        Ok(())
    }

    /// Resolution of an ambiguous receipt commit: the same run lock first, then the
    /// observation row of this exact entry and result, which the original inserted.
    async fn observed_on(
        &self,
        bound: &WriteBound,
        entry: &RemoteEntry,
        raw: Value,
    ) -> AppResult<Evidence<ActionReceipt>> {
        let result = validate_action_result(&entry.action, raw)?;
        let mut conn = bound.acquire(self.persistence.pool()).await?;
        let mut tx = bound.begin(&mut conn).await?;
        let receipt = async {
            sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
                .bind(entry.request.scope().company.as_uuid())
                .bind(entry.request.scope().run.as_uuid())
                .execute(&mut *tx)
                .await?;
            let observed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_action_actual_receipt_observations WHERE company_id=$1 AND remote_entry_id=$2 AND result=$3)")
                .bind(entry.request.scope().company.as_uuid()).bind(entry.entry).bind(&result)
                .fetch_one(&mut *tx).await?;
            if observed {
                self.receipt_on(&mut tx, &entry.request, &entry.action)
                    .await
            } else {
                Ok(None)
            }
        }
        .await;
        let receipt = bound.roll_back(tx, receipt).await?;
        Ok(receipt.map_or(Evidence::NotApplied, Evidence::Applied))
    }

    pub(super) async fn return_authorized(&self, entry: &RemoteEntry) -> AppResult<bool> {
        let bound = WriteBound::read_only("action.return_authorized", entry.request.lease);
        bound
            .read(Box::pin(async {
                let started = Instant::now();
                let mut conn = bound.acquire(self.persistence.pool()).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let window = async {
                    // Company/principal before run, including this ownership-only precheck.
                    let actor =
                        action_authority::prelock_on(&mut tx, entry.request.scope()).await?;
                    // Known ownership loss suppresses output; operational failures remain errors.
                    if !lease::lock_fence(&mut tx, entry.request.fence, bound.server_timeout())
                        .await?
                        || lease::live_window(&mut tx, entry.request.fence, started)
                            .await?
                            .is_none()
                    {
                        return Ok(None);
                    }
                    let authorized = self
                        .authorize_prelocked_on(
                            &mut tx,
                            &entry.request,
                            bound.server_timeout(),
                            actor,
                        )
                        .await?;
                    if authorized.approval_required
                        || action_uncertainty::conflicted_on(&mut tx, entry.request.scope()).await?
                    {
                        return Ok(None);
                    }
                    lease::live_window(&mut tx, entry.request.fence, started).await
                }
                .await;
                // Read-only: ROLLBACK on every path, including a known ownership loss, so the
                // run lock lock_fence took is released before the caller sees `false`.
                let window = bound.roll_back(tx, window).await?;
                Ok::<_, AppError>(
                    window.is_some_and(|w| {
                        Instant::now() < w.expires && Instant::now() < w.run_deadline
                    }),
                )
            }))
            .await
    }
}
