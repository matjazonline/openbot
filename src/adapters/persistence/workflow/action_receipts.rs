//! Receipts retain effect truth independently of current execution ownership.
use super::{action_dispatch::*, *};
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
        let receipt = tokio::time::timeout(entry.request.lease.persistence_timeout(), async {
            let mut tx = self.persistence.pool().begin().await?;
            // Same run-first order, deliberately no live-fence/access guard: late
            // bounded truth remains auditable after cancellation or ownership loss.
            sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
                .bind(entry.request.scope().company.as_uuid())
                .bind(entry.request.scope().run.as_uuid())
                .execute(&mut *tx)
                .await?;
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
            Self::insert_receipt_on(&mut tx, &entry, result).await?;
            let receipt = self
                .receipt_on(&mut tx, &entry.request, &entry.action)
                .await?
                .ok_or_else(invalid)?;
            tx.commit().await?;
            Ok::<_, AppError>(receipt)
        })
        .await
        .map_err(|_| lease::timed_out())??;
        if !self.return_authorized(&entry).await? {
            return Ok(RemoteDispatchObservation::Interrupted);
        }
        Ok(RemoteDispatchObservation::Committed(receipt))
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

    async fn return_authorized(&self, entry: &RemoteEntry) -> AppResult<bool> {
        tokio::time::timeout(entry.request.lease.persistence_timeout(), async {
            let started = Instant::now();
            let mut tx = self.persistence.pool().begin().await?;
            // Known ownership loss suppresses output; operational failures remain errors.
            if !lease::lock_fence(&mut tx, entry.request.fence, entry.request.lease).await?
                || lease::live_window(&mut tx, entry.request.fence, started)
                    .await?
                    .is_none()
            {
                return Ok(false);
            }
            let authorized = self.authorize_on(&mut tx, &entry.request).await?;
            if authorized.approval_required
                || action_uncertainty::conflicted_on(&mut tx, entry.request.scope()).await?
            {
                return Ok(false);
            }
            let window = lease::live_window(&mut tx, entry.request.fence, started).await?;
            tx.commit().await?;
            Ok::<_, AppError>(
                window
                    .is_some_and(|w| Instant::now() < w.expires && Instant::now() < w.run_deadline),
            )
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
}
