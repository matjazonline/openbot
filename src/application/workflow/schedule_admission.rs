//! Workflow admission for schedule occurrences (p08 D1.9). The cadence captures one occurrence
//! per due slot and run-now one per command key; this materializer admits one run of the
//! occurrence's binding for each, acting as that binding's run-as member, and records the run on
//! the occurrence under the claim that owns it. No thread or prompt message is created.

use super::message_admission::{BindingOutcome, Classified, WorkflowAdmitter, classify};
use super::triggers::ScheduleTriggerInput;
use super::{
    AdmissionClaim, AdmitWorkflowRequest, DeliveryPolicy, IdempotencyKey, RelatedAssociation,
    RelatedChannelId, WorkflowActor,
};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::entities::correlation::CorrelationId;
use crate::domain::entities::schedule::ClaimedScheduleRun;
use crate::domain::monitoring::MonitoringService;
use crate::domain::workflow::{
    BindingRevision, CompanyId, RunId, ScheduleId, ScheduleOccurrenceId, TriggerId, TriggerRef,
    TriggerSource, WorkflowBindingId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// How long one claim owns an occurrence. One occurrence's work is at most
/// [`MAX_STALE_RETRIES`] admissions.
pub const SCHEDULE_LEASE: Duration = Duration::from_secs(60);
/// Occurrences one claim takes.
pub const SCHEDULE_PAGE: i64 = 10;
/// Re-reads of a binding reconfigured while its admission was in flight.
const MAX_STALE_RETRIES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleWorkerId(pub Uuid);

/// One claim's fence: every write after the claim names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OccurrenceClaim {
    pub company: CompanyId,
    pub occurrence: Uuid,
    pub worker: ScheduleWorkerId,
    pub generation: Uuid,
}

/// An active binding's current configuration, as a schedule admits under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleBinding {
    pub id: WorkflowBindingId,
    pub revision: BindingRevision,
    pub run_as: WorkflowActor,
}

/// What materializing one claimed occurrence came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Materialized {
    Admitted(RunId),
    /// A deterministic refusal, recorded on the occurrence and the schedule.
    Refused(&'static str),
    /// A transient failure, recorded for a retry after backoff.
    Deferred,
    /// A transient failure on the last attempt: the occurrence failed for good.
    Failed,
    /// The claim was lost before its outcome was written; a later claim decides again.
    LeaseLost,
}

/// What a fenced transient-error write did to the occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorRecorded {
    /// Pending again after backoff.
    Retrying,
    /// That was the last attempt.
    Failed,
    /// The claim was lost; nothing was written.
    Lost,
}

/// An occurrence whose last attempt's lease lapsed, now failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailedOccurrence {
    pub company: CompanyId,
    pub occurrence: Uuid,
}

/// The durable occurrence queue. Every write after a claim is fenced by it; `false` means the
/// claim was lost and the caller must stop.
#[async_trait]
pub trait ScheduleOccurrences: Send + Sync {
    /// Fail claims whose lease lapsed on their last attempt, naming the failure on their
    /// schedules; they are never reclaimed.
    async fn fail_expired_occurrences(&self, page: i64) -> AppResult<Vec<FailedOccurrence>>;
    /// Capture every due slot and claim up to `page` claimable occurrences.
    async fn claim_due(
        &self,
        worker: ScheduleWorkerId,
        lease_until: DateTime<Utc>,
        page: i64,
    ) -> AppResult<Vec<ClaimedScheduleRun>>;
    /// Claim one occurrence if it is claimable now (run-now).
    async fn claim_occurrence(
        &self,
        company: CompanyId,
        occurrence: Uuid,
        worker: ScheduleWorkerId,
        lease_until: DateTime<Utc>,
    ) -> AppResult<Option<ClaimedScheduleRun>>;
    /// The binding's current configuration, or `None` when it is not active.
    async fn schedule_binding(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<ScheduleBinding>>;
    /// Materialized; clears the schedule's `last_error`.
    async fn record_run(&self, claim: OccurrenceClaim, run: RunId) -> AppResult<bool>;
    /// Failed with a refusal code, which the schedule's `last_error` names.
    async fn record_refusal(
        &self,
        claim: OccurrenceClaim,
        refusal: &'static str,
    ) -> AppResult<bool>;
    /// Pending again after backoff, or failed after the last attempt; either way the schedule's
    /// `last_error` names it.
    async fn record_error(&self, claim: OccurrenceClaim, error: &str) -> AppResult<ErrorRecorded>;
}

/// Run-now materializes the occurrence it captured inline, through the same path.
#[async_trait]
pub trait ScheduleRunNow: Send + Sync {
    /// `None` when the occurrence is not claimable now (already materialized, failed, or held).
    async fn materialize_now(
        &self,
        company: CompanyId,
        occurrence: Uuid,
    ) -> AppResult<Option<Materialized>>;
}

pub struct ScheduleMaterializer<Q, A> {
    queue: Q,
    admitter: A,
    monitoring: Option<Arc<dyn MonitoringService>>,
    /// Woken when a run was admitted, so workflow workers need not poll for it.
    admitted: Option<Arc<Notify>>,
}

impl<Q, A> ScheduleMaterializer<Q, A>
where
    Q: ScheduleOccurrences,
    A: WorkflowAdmitter,
{
    pub fn new(queue: Q, admitter: A) -> Self {
        Self {
            queue,
            admitter,
            monitoring: None,
            admitted: None,
        }
    }

    pub fn with_admitted(mut self, admitted: Arc<Notify>) -> Self {
        self.admitted = Some(admitted);
        self
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    pub async fn claim(&self, worker: ScheduleWorkerId) -> AppResult<Vec<ClaimedScheduleRun>> {
        self.claim_page(worker, SCHEDULE_PAGE).await
    }

    async fn claim_page(
        &self,
        worker: ScheduleWorkerId,
        page: i64,
    ) -> AppResult<Vec<ClaimedScheduleRun>> {
        for expired in self.queue.fail_expired_occurrences(page).await? {
            self.failed(expired.company, expired.occurrence, "lease_expired");
        }
        self.queue.claim_due(worker, lease_until()?, page).await
    }

    /// Admit one run for a claimed occurrence and record what happened under the claim.
    pub async fn materialize(
        &self,
        worker: ScheduleWorkerId,
        occurrence: &ClaimedScheduleRun,
    ) -> AppResult<Materialized> {
        let claim = OccurrenceClaim {
            company: CompanyId::new(occurrence.schedule.company_id),
            occurrence: occurrence.id,
            worker,
            generation: occurrence.materialization_generation,
        };
        let binding = WorkflowBindingId::new(occurrence.binding_id);
        let mut current = self.queue.schedule_binding(claim.company, binding).await?;
        for _ in 0..MAX_STALE_RETRIES {
            let Some(binding) = current else {
                return self.refuse(claim, "unavailable").await;
            };
            let result = self
                .admitter
                .admit(admission_request(occurrence, &binding, claim)?)
                .await;
            let error = result.as_ref().err().map(ToString::to_string);
            match classify(result) {
                Classified::Settled(BindingOutcome::Admitted(run)) => {
                    if !self.queue.record_run(claim, run).await? {
                        return Ok(self.lost(claim));
                    }
                    if let Some(admitted) = &self.admitted {
                        admitted.notify_waiters();
                    }
                    return Ok(Materialized::Admitted(run));
                }
                Classified::Settled(
                    BindingOutcome::Refused(reason) | BindingOutcome::Failed(reason),
                ) => return self.refuse(claim, reason).await,
                Classified::Transient => {
                    let error = error.unwrap_or_else(|| "transient admission failure".into());
                    return self.defer(claim, &error).await;
                }
                Classified::Stale => {
                    current = self
                        .queue
                        .schedule_binding(claim.company, binding.id)
                        .await?;
                }
            }
        }
        self.defer(claim, "The binding kept changing during admission")
            .await
    }

    async fn refuse(
        &self,
        claim: OccurrenceClaim,
        reason: &'static str,
    ) -> AppResult<Materialized> {
        tracing::warn!(
            company_id = %claim.company.as_uuid(),
            occurrence_id = %claim.occurrence,
            reason,
            "A workflow binding did not admit a schedule occurrence"
        );
        if let Some(monitoring) = &self.monitoring {
            monitoring.increment_counter(
                "workflow_schedule_admission_refusals_total",
                1,
                &[("reason", reason)],
            );
        }
        Ok(if self.queue.record_refusal(claim, reason).await? {
            Materialized::Refused(reason)
        } else {
            self.lost(claim)
        })
    }

    async fn defer(&self, claim: OccurrenceClaim, error: &str) -> AppResult<Materialized> {
        Ok(match self.queue.record_error(claim, error).await? {
            ErrorRecorded::Retrying => {
                tracing::warn!(
                    company_id = %claim.company.as_uuid(),
                    occurrence_id = %claim.occurrence,
                    error,
                    "A schedule occurrence could not be admitted yet"
                );
                Materialized::Deferred
            }
            ErrorRecorded::Failed => {
                self.failed(claim.company, claim.occurrence, "transient");
                Materialized::Failed
            }
            ErrorRecorded::Lost => self.lost(claim),
        })
    }

    /// An occurrence that will never be admitted (p08 F033).
    fn failed(&self, company: CompanyId, occurrence: Uuid, cause: &'static str) {
        tracing::error!(
            company_id = %company.as_uuid(),
            occurrence_id = %occurrence,
            cause,
            "A schedule occurrence failed terminally"
        );
        if let Some(monitoring) = &self.monitoring {
            monitoring.increment_counter(
                "workflow_schedule_occurrences_failed_total",
                1,
                &[("cause", cause)],
            );
        }
    }

    /// The claim was lost before its outcome was written; its next owner decides again.
    fn lost(&self, claim: OccurrenceClaim) -> Materialized {
        tracing::warn!(
            company_id = %claim.company.as_uuid(),
            occurrence_id = %claim.occurrence,
            "A schedule occurrence claim was lost before its outcome was recorded"
        );
        if let Some(monitoring) = &self.monitoring {
            monitoring.increment_counter("workflow_schedule_occurrence_leases_lost_total", 1, &[]);
        }
        Materialized::LeaseLost
    }
}

#[async_trait]
impl<Q, A> ScheduleRunNow for ScheduleMaterializer<Q, A>
where
    Q: ScheduleOccurrences,
    A: WorkflowAdmitter,
{
    async fn materialize_now(
        &self,
        company: CompanyId,
        occurrence: Uuid,
    ) -> AppResult<Option<Materialized>> {
        let worker = ScheduleWorkerId(Uuid::new_v4());
        let Some(claimed) = self
            .queue
            .claim_occurrence(company, occurrence, worker, lease_until()?)
            .await?
        else {
            return Ok(None);
        };
        self.materialize(worker, &claimed).await.map(Some)
    }
}

fn lease_until() -> AppResult<DateTime<Utc>> {
    chrono::Duration::from_std(SCHEDULE_LEASE)
        .map(|lease| Utc::now() + lease)
        .map_err(|_| AppError::Internal("schedule lease out of range".into()))
}

fn admission_request(
    occurrence: &ClaimedScheduleRun,
    binding: &ScheduleBinding,
    claim: OccurrenceClaim,
) -> AppResult<AdmitWorkflowRequest> {
    let schedule = &occurrence.schedule;
    let company = CompanyId::new(schedule.company_id);
    Ok(AdmitWorkflowRequest {
        company_id: company,
        actor: binding.run_as,
        association: RelatedAssociation::Channel(RelatedChannelId::new(schedule.channel_id)),
        trigger: TriggerRef::new(
            company,
            TriggerId::new(occurrence.id),
            TriggerSource::Schedule {
                schedule_id: ScheduleId::new(schedule.id),
                occurrence_id: ScheduleOccurrenceId::new(occurrence.id),
            },
        )
        .map_err(|error| AppError::Internal(error.to_string()))?,
        correlation_id: CorrelationId::new(),
        binding_id: binding.id,
        idempotency_key: IdempotencyKey::parse(format!("s:{}", occurrence.id))?,
        input: ScheduleTriggerInput {
            scheduled_for: occurrence.scheduled_for,
        }
        .to_value(),
        rerun_of: None,
        delivery_policy: DeliveryPolicy::Deliver,
        expected_binding_revision: Some(binding.revision),
        admission_claim: Some(AdmissionClaim::Schedule(claim)),
    })
}

/// The occurrence queue's polling loop. One occurrence at a time; dropping the loop drops the
/// in-flight occurrence, whose lease then expires and spends an attempt.
pub struct ScheduleMaterializerWorker<Q, A> {
    pub materializer: ScheduleMaterializer<Q, A>,
    pub worker: ScheduleWorkerId,
    pub poll: Duration,
}

impl<Q, A> ScheduleMaterializerWorker<Q, A>
where
    Q: ScheduleOccurrences,
    A: WorkflowAdmitter,
{
    pub async fn run(&self, shutdown: &CancellationToken) {
        loop {
            let page = tokio::select! { biased;
                _ = shutdown.cancelled() => return,
                page = self.materializer.claim_page(self.worker, 1) => page,
            };
            let mut full = false;
            match page {
                Ok(occurrences) => {
                    full = occurrences.len() == 1;
                    for occurrence in occurrences {
                        let id = occurrence.id;
                        let result = tokio::select! { biased;
                            _ = shutdown.cancelled() => return,
                            result = Box::pin(self.materializer.materialize(self.worker, &occurrence)) => result,
                        };
                        if let Err(error) = result {
                            tracing::warn!(occurrence_id = %id, error = %error,
                                "Schedule occurrence materialization failed; its lease will expire");
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "Schedule occurrence claim failed");
                }
            }
            if full {
                continue;
            }
            tokio::select! { biased;
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(self.poll) => {},
            }
        }
    }
}
