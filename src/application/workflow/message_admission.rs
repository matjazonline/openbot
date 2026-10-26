//! Workflow admission for accepted messages (p08 D1.4). Ingress commits one intent per message ×
//! channel association with the message; this worker first offers the message to a run awaiting
//! it as a reply (D1.5), and otherwise admits one run per eligible binding, each independently.
//! Eligibility belongs to the bindings' message triggers, never to channel settings.

use super::triggers::MessageTriggerInput;
use super::waits::{IngressReply, ReplyIngress, WorkflowEventId, WorkflowWaits};
use super::{
    AdmissionClaim, AdmissionResult, AdmitWorkflowRequest, DeliveryPolicy, IdempotencyKey,
    RelatedAssociation, RelatedChannelId, RelatedThreadId, WorkflowActor, WorkflowAdmission,
    WorkflowAuthorization, WorkflowBindings, WorkflowInspection, WorkflowRunTransitions,
    WorkflowService,
};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::entities::correlation::CorrelationId;
use crate::domain::entities::message::CanonicalMessageId;
use crate::domain::monitoring::MonitoringService;
use crate::domain::workflow::{
    BindingRevision, CompanyId, MessageFacts, MessageTrigger, RunId, TriggerId, TriggerRef,
    TriggerSource, WorkflowBindingId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Claims and attempts per intent; lease expiry and transient failures spend attempts.
pub const MAX_INTENT_ATTEMPTS: i16 = 5;
/// How long one claim owns an intent. One intent's work is bounded by the per-channel binding
/// cap times [`MAX_STALE_RETRIES`] admissions.
pub const MESSAGE_INTENT_LEASE: Duration = Duration::from_secs(60);
/// How long a reply may wait for its run to park the wait that awaits it (p08 D1.5.3).
pub const REPLY_PARK_GRACE: Duration = Duration::from_secs(5 * 60);
/// Intents one claim takes, and at most how many of one company's (p08 D1.4.1).
pub const MESSAGE_INTENT_PAGE: u16 = 32;
pub const MESSAGE_INTENTS_PER_COMPANY: u16 = 4;
/// The live worker starts every claimed lease immediately while retaining tenant spread.
const WORKER_CLAIM_PAGE: u16 = 8;
/// Re-reads of a binding reconfigured while its admission was in flight.
const MAX_STALE_RETRIES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageIntentWorkerId(pub Uuid);

/// One claim's fence: every write after the claim names it (p08 F008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntentClaim {
    pub company: CompanyId,
    pub intent: Uuid,
    pub worker: MessageIntentWorkerId,
    pub generation: Uuid,
}

/// A claimed intent: the facts ingress committed, and the claim that owns it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedIntent {
    pub claim: IntentClaim,
    pub message: CanonicalMessageId,
    pub channel: Uuid,
    pub thread: Uuid,
    pub facts: MessageFacts,
    /// The sender's stated disposition answers; a quiet message never admits.
    pub admissible: bool,
    /// Posted in-app by a member rather than received as mail.
    pub in_app: bool,
    pub reply_to: Option<CanonicalMessageId>,
    /// Reply correlation already found this message ordinary ingress; it is never offered as a
    /// reply again, so one message cannot both admit runs and answer a wait (p08 F024).
    pub reply_settled: bool,
    pub responder: Option<Uuid>,
    pub delivery_policy: DeliveryPolicy,
    pub correlation: CorrelationId,
    /// Including this claim.
    pub attempts: i16,
    pub accepted_at: DateTime<Utc>,
}

/// An active message-triggered binding that saw the message accepted, and the configuration
/// revision its eligibility was evaluated against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateBinding {
    pub id: WorkflowBindingId,
    pub revision: BindingRevision,
    pub run_as: WorkflowActor,
    pub trigger: MessageTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingOutcome {
    Admitted(RunId),
    /// A deterministic refusal; the code is bounded and names no foreign record.
    Refused(&'static str),
    Failed(&'static str),
}

/// An intent whose last attempt's lease lapsed, now failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailedIntent {
    pub company: CompanyId,
    pub intent: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentDeferral {
    /// Something failed that may succeed later; spends the claim's attempt.
    Transient,
    /// A reply's wait has not parked yet; attempt-neutral and bounded by the park grace.
    NotYet,
}

/// The durable intent queue. Every write after a claim is fenced by it; `false` means the claim
/// was lost (lease expired and reclaimed) and the caller must stop.
#[async_trait]
pub trait MessageIntentQueue: Send + Sync {
    /// Fail claims whose lease lapsed on their last attempt; they are never reclaimed.
    async fn fail_expired_intents(&self, page: u16) -> AppResult<Vec<FailedIntent>>;
    async fn claim_intents(
        &self,
        worker: MessageIntentWorkerId,
        page: u16,
    ) -> AppResult<Vec<ClaimedIntent>>;
    /// Active bindings with a message trigger, on the intent's channel or thread, activated no
    /// later than the message was accepted, without an outcome for this intent yet.
    async fn candidate_bindings(&self, intent: &ClaimedIntent) -> AppResult<Vec<CandidateBinding>>;
    async fn candidate_binding(
        &self,
        intent: &ClaimedIntent,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<CandidateBinding>>;
    /// The stored message as the standard trigger input.
    async fn trigger_input(&self, intent: &ClaimedIntent) -> AppResult<MessageTriggerInput>;
    async fn record_binding_outcome(
        &self,
        claim: IntentClaim,
        binding: &CandidateBinding,
        outcome: BindingOutcome,
    ) -> AppResult<bool>;
    /// Record that reply correlation found the message ordinary ingress (p08 F024).
    async fn settle_reply(&self, claim: IntentClaim) -> AppResult<bool>;
    /// `matched` for a resumed wait; otherwise the outcome follows the binding outcomes.
    async fn finish_intent(&self, claim: IntentClaim, matched: Option<RunId>) -> AppResult<bool>;
    async fn defer_intent(&self, claim: IntentClaim, deferral: IntentDeferral) -> AppResult<bool>;
}

/// The admission entry point the worker needs; [`WorkflowService::admit`] in production.
#[async_trait]
pub trait WorkflowAdmitter: Send + Sync {
    async fn admit(&self, request: AdmitWorkflowRequest) -> AppResult<AdmissionResult>;
}

#[async_trait]
impl<D, A, T, I, H> WorkflowAdmitter for WorkflowService<D, A, T, I, H>
where
    D: WorkflowBindings,
    A: WorkflowAdmission,
    T: WorkflowRunTransitions,
    I: WorkflowInspection,
    H: WorkflowAuthorization,
{
    async fn admit(&self, request: AdmitWorkflowRequest) -> AppResult<AdmissionResult> {
        WorkflowService::admit(self, request).await
    }
}

/// What one binding's admission attempt came to.
enum Attempt {
    Outcome(BindingOutcome),
    /// The binding no longer wants the message after a re-read.
    Ineligible,
    Transient,
}

pub struct MessageAdmissions<Q, A, W> {
    queue: Q,
    admitter: A,
    waits: W,
    monitoring: Option<Arc<dyn MonitoringService>>,
    /// Woken when a run was admitted or a wait resumed, so workflow workers need not poll for it.
    admitted: Option<Arc<Notify>>,
}

impl<Q, A, W> MessageAdmissions<Q, A, W>
where
    Q: MessageIntentQueue,
    A: WorkflowAdmitter,
    W: WorkflowWaits,
{
    pub fn new(queue: Q, admitter: A, waits: W) -> Self {
        Self {
            queue,
            admitter,
            waits,
            monitoring: None,
            admitted: None,
        }
    }

    pub fn with_admitted(mut self, admitted: Arc<Notify>) -> Self {
        self.admitted = Some(admitted);
        self
    }

    fn wake_workers(&self) {
        if let Some(admitted) = &self.admitted {
            admitted.notify_waiters();
        }
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    pub fn queue(&self) -> &Q {
        &self.queue
    }

    /// Fail, observably, the intents whose last lease lapsed, then claim a fair page.
    pub async fn claim(
        &self,
        worker: MessageIntentWorkerId,
        page: u16,
    ) -> AppResult<Vec<ClaimedIntent>> {
        for expired in self.queue.fail_expired_intents(page).await? {
            self.failed(expired.company, expired.intent, "lease_expired");
        }
        self.queue.claim_intents(worker, page).await
    }

    /// Process one claimed intent to an outcome, a deferral, or a lost claim.
    pub async fn process(&self, intent: ClaimedIntent) -> AppResult<()> {
        if let Some(answers) = intent.reply_to.filter(|_| !intent.reply_settled)
            && !self.correlate(&intent, answers).await?
        {
            return Ok(());
        }
        if !intent.admissible {
            self.queue.finish_intent(intent.claim, None).await?;
            return Ok(());
        }
        let mut transient = false;
        for binding in self.queue.candidate_bindings(&intent).await? {
            if !binding.trigger.admits(intent.facts) {
                continue;
            }
            match self.admit_binding(&intent, binding).await? {
                Attempt::Outcome(outcome) => {
                    if !self.record(&intent, &binding, outcome).await? {
                        return Ok(()); // The claim was lost; its new owner continues.
                    }
                    if matches!(outcome, BindingOutcome::Admitted(_)) {
                        self.wake_workers();
                    }
                }
                Attempt::Ineligible => {}
                Attempt::Transient => transient = true,
            }
        }
        if transient && intent.attempts < MAX_INTENT_ATTEMPTS {
            return self.defer(&intent, IntentDeferral::Transient).await;
        }
        if transient {
            // The last attempt: every binding still without an outcome failed transiently.
            for binding in self.queue.candidate_bindings(&intent).await? {
                if binding.trigger.admits(intent.facts)
                    && !self
                        .record(
                            &intent,
                            &binding,
                            BindingOutcome::Failed("transient_exhausted"),
                        )
                        .await?
                {
                    return Ok(());
                }
            }
        }
        self.queue.finish_intent(intent.claim, None).await?;
        Ok(())
    }

    /// Offer a reply to the run awaiting it. `true`: ordinary admission follows, decided durably
    /// so no retry offers it again; `false`: matched, deferred, or the claim was lost.
    async fn correlate(
        &self,
        intent: &ClaimedIntent,
        answers: CanonicalMessageId,
    ) -> AppResult<bool> {
        match self.offer_reply(intent, answers).await? {
            Some(Ok(run)) => {
                if self.queue.finish_intent(intent.claim, Some(run)).await? {
                    self.wake_workers();
                }
                Ok(false)
            }
            Some(Err(deferral)) => {
                self.defer(intent, deferral).await?;
                Ok(false)
            }
            None => self.queue.settle_reply(intent.claim).await,
        }
    }

    /// Defer the intent; a transient failure on its last attempt ends it, observably.
    async fn defer(&self, intent: &ClaimedIntent, deferral: IntentDeferral) -> AppResult<()> {
        let terminal =
            deferral == IntentDeferral::Transient && intent.attempts >= MAX_INTENT_ATTEMPTS;
        if self.queue.defer_intent(intent.claim, deferral).await? && terminal {
            self.failed(intent.claim.company, intent.claim.intent, "transient");
        }
        Ok(())
    }

    /// An intent ended neither matched nor offered to every binding (p08 F023): distinct from
    /// per-binding refusals, which `record` counts.
    fn failed(&self, company: CompanyId, intent: Uuid, cause: &'static str) {
        tracing::error!(
            company_id = %company.as_uuid(),
            intent_id = %intent,
            cause,
            "A workflow message intent failed terminally"
        );
        if let Some(monitoring) = &self.monitoring {
            monitoring.increment_counter(
                "workflow_message_intents_failed_total",
                1,
                &[("cause", cause)],
            );
        }
    }

    /// `Some(Ok(run))`: the reply resumed that run's wait. `Some(Err(_))`: try again later.
    /// `None`: not a reply any run awaits; ordinary admission follows.
    async fn offer_reply(
        &self,
        intent: &ClaimedIntent,
        answers: CanonicalMessageId,
    ) -> AppResult<Option<Result<RunId, IntentDeferral>>> {
        let payload = self.queue.trigger_input(intent).await?.to_value();
        let grace = chrono::Duration::from_std(REPLY_PARK_GRACE)
            .map_err(|_| AppError::Internal("Reply park grace out of range".into()))?;
        let reply = IngressReply {
            company: intent.claim.company,
            thread: intent.thread,
            answers,
            responder: intent.responder,
            in_app: intent.in_app,
            event: WorkflowEventId(intent.claim.intent),
            payload,
            park_grace_until: intent.accepted_at + grace,
        };
        Ok(match self.waits.deliver_reply(reply).await {
            Ok(ReplyIngress::Recorded(run) | ReplyIngress::Duplicate(run)) => Some(Ok(run)),
            Ok(ReplyIngress::NotYet) => Some(Err(IntentDeferral::NotYet)),
            Ok(ReplyIngress::Unmatched) => None,
            // The run can never take this answer, so it is ordinary ingress rather than lost.
            Ok(ReplyIngress::Undeliverable(cause)) => {
                tracing::error!(
                    company_id = %intent.claim.company.as_uuid(),
                    intent_id = %intent.claim.intent,
                    cause,
                    "A workflow reply could not be delivered to its run"
                );
                None
            }
            // Anything else may succeed later, running out of time included (p08 F028).
            Err(error) => {
                tracing::warn!(
                    intent_id = %intent.claim.intent,
                    attempts = intent.attempts,
                    error = %error,
                    "Workflow reply delivery failed"
                );
                Some(Err(IntentDeferral::Transient))
            }
        })
    }

    /// Admit one binding, re-reading it when it was reconfigured meanwhile (p08 D1.4.5).
    async fn admit_binding(
        &self,
        intent: &ClaimedIntent,
        mut binding: CandidateBinding,
    ) -> AppResult<Attempt> {
        let input = self.queue.trigger_input(intent).await?.to_value();
        for _ in 0..MAX_STALE_RETRIES {
            let result = self
                .admitter
                .admit(admission_request(intent, &binding, input.clone())?)
                .await;
            match classify(result) {
                Classified::Settled(outcome) => return Ok(Attempt::Outcome(outcome)),
                Classified::Transient => return Ok(Attempt::Transient),
                Classified::Stale => {
                    match self.queue.candidate_binding(intent, binding.id).await? {
                        Some(current) if current.trigger.admits(intent.facts) => binding = current,
                        _ => return Ok(Attempt::Ineligible),
                    }
                }
            }
        }
        Ok(Attempt::Transient)
    }

    async fn record(
        &self,
        intent: &ClaimedIntent,
        binding: &CandidateBinding,
        outcome: BindingOutcome,
    ) -> AppResult<bool> {
        if let BindingOutcome::Refused(reason) | BindingOutcome::Failed(reason) = outcome {
            tracing::warn!(
                company_id = %intent.claim.company.as_uuid(),
                binding_id = %binding.id.as_uuid(),
                intent_id = %intent.claim.intent,
                reason,
                "A workflow binding did not admit an accepted message"
            );
            if let Some(monitoring) = &self.monitoring {
                monitoring.increment_counter(
                    "workflow_message_admission_refusals_total",
                    1,
                    &[("reason", reason)],
                );
            }
        }
        self.queue
            .record_binding_outcome(intent.claim, binding, outcome)
            .await
    }
}

pub(super) enum Classified {
    Settled(BindingOutcome),
    Stale,
    Transient,
}

/// By producing condition (p08 F005): a concurrent reconfiguration is `Stale`, a refusal of
/// this binding or input is permanent, and anything else may succeed later.
pub(super) fn classify(result: AppResult<AdmissionResult>) -> Classified {
    match result {
        Ok(AdmissionResult::Created(run) | AdmissionResult::Replayed(run)) => {
            Classified::Settled(BindingOutcome::Admitted(run))
        }
        Ok(AdmissionResult::Stale) => Classified::Stale,
        Ok(AdmissionResult::Conflict) => {
            tracing::error!("A message admission key collided with a different admission");
            Classified::Settled(BindingOutcome::Failed("key_conflict"))
        }
        // Inactive or archived binding, run-as no longer owner/admin or not visible, unready
        // resources: all non-disclosing NotFound.
        Err(AppError::NotFound(_)) => Classified::Settled(BindingOutcome::Refused("unavailable")),
        Err(AppError::BadRequest(_)) => {
            Classified::Settled(BindingOutcome::Refused("input_rejected"))
        }
        Err(_) => Classified::Transient,
    }
}

fn admission_request(
    intent: &ClaimedIntent,
    binding: &CandidateBinding,
    input: serde_json::Value,
) -> AppResult<AdmitWorkflowRequest> {
    let company = intent.claim.company;
    Ok(AdmitWorkflowRequest {
        company_id: company,
        actor: binding.run_as,
        association: RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(intent.channel),
            thread_id: RelatedThreadId::new(intent.thread),
        },
        trigger: TriggerRef::new(
            company,
            TriggerId::new(intent.claim.intent),
            TriggerSource::Message {
                message_id: intent.message,
            },
        )
        .map_err(|error| AppError::Internal(error.to_string()))?,
        correlation_id: intent.correlation,
        binding_id: binding.id,
        idempotency_key: IdempotencyKey::parse(format!(
            "m:{}:{}",
            intent.claim.intent,
            binding.id.as_uuid()
        ))?,
        input,
        rerun_of: None,
        delivery_policy: intent.delivery_policy,
        expected_binding_revision: Some(binding.revision),
        admission_claim: Some(AdmissionClaim::Message(intent.claim)),
    })
}

/// The intent queue's polling loop. Each claimed page is processed concurrently so no
/// intent spends its lease waiting behind another. Shutdown drops the in-flight page.
pub struct MessageAdmissionWorker<Q, A, W> {
    pub admissions: MessageAdmissions<Q, A, W>,
    pub worker: MessageIntentWorkerId,
    pub poll: Duration,
}

impl<Q, A, W> MessageAdmissionWorker<Q, A, W>
where
    Q: MessageIntentQueue,
    A: WorkflowAdmitter,
    W: WorkflowWaits,
{
    pub async fn run(&self, wakeup: &Notify, shutdown: &CancellationToken) {
        loop {
            let page = tokio::select! { biased;
                _ = shutdown.cancelled() => return,
                page = self.admissions.claim(self.worker, WORKER_CLAIM_PAGE) => page,
            };
            let mut full = false;
            match page {
                Ok(intents) => {
                    // A full page moved every row out of the claimable set, so it is evidence of
                    // more work; deferred rows were moved into the future and are not reclaimed.
                    full = intents.len() == usize::from(WORKER_CLAIM_PAGE);
                    let results = tokio::select! { biased;
                        _ = shutdown.cancelled() => return,
                        results = futures::future::join_all(intents.into_iter().map(|intent| async {
                            let id = intent.claim.intent;
                            (id, self.admissions.process(intent).await)
                        })) => results,
                    };
                    for (id, result) in results {
                        if let Err(error) = result {
                            tracing::warn!(intent_id = %id, error = %error,
                                "Workflow message intent processing failed; its lease will expire");
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "Workflow message intent claim failed");
                }
            }
            if full {
                continue;
            }
            tokio::select! { biased;
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(self.poll) => {},
                _ = wakeup.notified() => {},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_admission_classifies_by_producing_condition() {
        let run = RunId::new(Uuid::new_v4());
        assert!(matches!(
            classify(Ok(AdmissionResult::Created(run))),
            Classified::Settled(BindingOutcome::Admitted(admitted)) if admitted == run
        ));
        assert!(matches!(
            classify(Ok(AdmissionResult::Replayed(run))),
            Classified::Settled(BindingOutcome::Admitted(_))
        ));
        assert!(matches!(
            classify(Ok(AdmissionResult::Stale)),
            Classified::Stale
        ));
        assert!(matches!(
            classify(Ok(AdmissionResult::Conflict)),
            Classified::Settled(BindingOutcome::Failed("key_conflict"))
        ));
        assert!(matches!(
            classify(Err(AppError::NotFound("binding".into()))),
            Classified::Settled(BindingOutcome::Refused("unavailable"))
        ));
        assert!(matches!(
            classify(Err(AppError::BadRequest("input".into()))),
            Classified::Settled(BindingOutcome::Refused("input_rejected"))
        ));
        assert!(matches!(
            classify(Err(AppError::Database("down".into()))),
            Classified::Transient
        ));
    }
}
