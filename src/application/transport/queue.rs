//! The durable delivery queue, as the worker that drains it needs it.
//!
//! One state machine for every transport. A claimant takes a bounded batch, mints a fresh
//! execution id, and every write it makes afterwards names that id in its `WHERE` clause -- so a
//! run that lost its lease cannot report a result over the execution that replaced it. The parent
//! delivery is the only leased object; its parts carry the provider results and are transitioned
//! through the parent's fence.
//!
//! The port lives here, beside [`crate::services::delivery_worker`], rather than in the SQLx
//! adapter that happens to implement the claim. What the adapter owes is stated as method
//! signatures with no defaults: a double that silently succeeded at a lease renewal or a
//! completion would make every concurrency test prove nothing.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        correlation::CorrelationId,
        message::CanonicalMessageId,
        transport::{
            ChannelBindingId, DeliveryId, DeliveryPartId, DeliveryPartStatus, DeliveryPurpose,
            DeliveryStatus, ExternalDestination, ExternalMessageKey, FailureClass, TransportKind,
        },
    },
    transport::{
        bounded::{BoundedVec, BoundsError},
        delivery::{
            DeliveryKey, FailureDetail, MAX_DELIVERY_PARTS, ProviderSendOutcome, RenderedPart,
        },
        lease::{ExecutionLease, WorkerId},
    },
};

/// How many attempts one delivery gets before it is dead-lettered.
///
/// Every way an attempt can end costs one: a reported failure, and a lease that lapsed without a
/// report. While expiry was free, a delivery that reliably outlived its lease was redelivered
/// every lease period for ever and never reached the cap.
pub const MAX_DELIVERY_ATTEMPTS: i32 = 5;

/// How long a claim is held before it has to be renewed.
///
/// Long enough for one provider call with its own deadline inside it, short enough that a crashed
/// worker's rows come back within a maintenance sweep.
pub const DELIVERY_LEASE_SECONDS: i64 = 120;

/// The most deliveries one claim takes. Bounded so a backlog is drained in batches whose memory
/// cost is known, rather than in one unbounded read of every frozen payload in the queue.
pub const DELIVERY_CLAIM_BATCH: i64 = 20;

/// How a failed attempt ages before it becomes claimable again.
///
/// Stated once, in Rust, and formatted into the SQL of both paths that end an attempt -- the
/// reported failure and the lease sweep -- so a row that only ever expires ages at the same rate
/// as one that failed outright. Two copies of this arithmetic is how those two drift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeliveryBackoff {
    /// The first delay. Doubles per attempt from here.
    pub base: Duration,
    /// The ceiling, before jitter. Without one, the fifth attempt of a five-attempt budget would
    /// sit for hours and the dead-letter that follows would arrive long after anyone was looking.
    pub cap: Duration,
    /// The fraction of the delay that is randomised, either way. Jitter is what stops a provider
    /// outage that failed a thousand deliveries at once from retrying all thousand in the same
    /// second, over and over.
    pub jitter: f64,
}

impl DeliveryBackoff {
    pub const DEFAULT: Self = Self {
        base: Duration::from_secs(2),
        cap: Duration::from_secs(600),
        jitter: 0.2,
    };

    /// The unjittered delay after `attempts` failed attempts, clamped to [`Self::cap`].
    ///
    /// Exposed for the test that pins the curve; the queue itself applies this through SQL so the
    /// sweep can age a whole batch in one statement.
    pub fn delay_after(&self, attempts: i32) -> Duration {
        let exponent = attempts.clamp(0, 16) as u32;
        self.base
            .saturating_mul(2u32.saturating_pow(exponent))
            .min(self.cap)
    }
}

impl Default for DeliveryBackoff {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One delivery to create, with everything that will be sent already frozen.
///
/// Parts are rendered by the producer, inside the transaction that creates the durable state the
/// delivery answers for. That ordering is the point: a retry sends the bytes that were frozen
/// rather than re-rendering against a display name, a participant list or a policy that has
/// changed since, and a resumed multi-part send addresses the parts it already has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDelivery {
    pub id: DeliveryId,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    /// The canonical message being exposed. Even a platform notice has one: an approval request
    /// is written as a system-authored message in the thread it concerns, so the queue holds
    /// identifiers rather than the only copy of what was sent.
    pub message_id: CanonicalMessageId,
    pub source_binding_id: ChannelBindingId,
    pub destination_binding_id: ChannelBindingId,
    /// The recipient named within the destination interface's namespace, or `None` when the
    /// interface itself is the destination.
    pub external_destination: Option<ExternalDestination>,
    pub task_id: Option<Uuid>,
    /// The delivery that must land first. The claim refuses this row until that one is delivered.
    pub depends_on_delivery_id: Option<DeliveryId>,
    pub correlation_id: CorrelationId,
    pub transport: TransportKind,
    pub purpose: DeliveryPurpose,
    pub idempotency_key: DeliveryKey,
    pub max_attempts: i32,
    pub parts: BoundedVec<RenderedPart, MAX_DELIVERY_PARTS>,
}

/// A durable notification with no canonical-message attribution.
///
/// The frozen payload is still bounded and the destination is still explicit. Only the tenant,
/// channel, message and binding tuple is absent, which is the honest state for a bounce caused by
/// an address that resolved to no company.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewStandaloneDelivery {
    pub id: DeliveryId,
    pub external_destination: ExternalDestination,
    pub correlation_id: CorrelationId,
    pub transport: TransportKind,
    pub purpose: DeliveryPurpose,
    pub idempotency_key: DeliveryKey,
    pub max_attempts: i32,
    pub parts: BoundedVec<RenderedPart, MAX_DELIVERY_PARTS>,
}

impl NewDelivery {
    /// Refuses a delivery that renders nothing.
    ///
    /// An empty part list would be written as a row the worker can never finish: the parent is
    /// delivered only when every part is, and no part is ever delivered. Caught here rather than
    /// aggregated into a dead letter, because it is a renderer defect and not a send failure.
    pub fn frozen_parts(
        parts: Vec<RenderedPart>,
    ) -> Result<BoundedVec<RenderedPart, MAX_DELIVERY_PARTS>, BoundsError> {
        if parts.is_empty() {
            return Err(BoundsError::Empty {
                field: "delivery parts",
            });
        }
        BoundedVec::parse("delivery parts", parts)
    }
}

/// What creating a delivery did.
///
/// `Absorbed` is the unique index doing its job rather than a failure: two workers racing the same
/// logical delivery compute the same idempotency key, and the loser must not enqueue a second
/// send. It carries the id of the delivery that already exists so the caller can still point at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryCreation {
    Created(DeliveryId),
    Absorbed(DeliveryId),
}

impl DeliveryCreation {
    pub const fn delivery_id(self) -> DeliveryId {
        match self {
            Self::Created(id) | Self::Absorbed(id) => id,
        }
    }

    pub const fn was_created(self) -> bool {
        matches!(self, Self::Created(_))
    }
}

/// The durable identity of one claimed delivery: every fact the worker needs that is not the
/// payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryRecord {
    pub id: DeliveryId,
    pub attribution: Option<DeliveryAttribution>,
    pub external_destination: Option<ExternalDestination>,
    pub task_id: Option<Uuid>,
    pub correlation_id: CorrelationId,
    pub transport: TransportKind,
    pub purpose: DeliveryPurpose,
    pub idempotency_key: DeliveryKey,
    pub attempt_count: i32,
    pub max_attempts: i32,
}

/// The canonical rows an ordinary delivery is attributed to.
///
/// One option around one struct mirrors the database's all-or-none check and prevents Rust from
/// representing a half-attributed delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryAttribution {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub message_id: CanonicalMessageId,
    pub source_binding_id: ChannelBindingId,
    pub destination_binding_id: ChannelBindingId,
}

impl DeliveryRecord {
    /// Which company this delivery is being sent on behalf of, when it is being sent on behalf of
    /// one at all.
    ///
    /// `None` is a platform notice -- a confirmation code, an approval link -- which belongs to
    /// the deployment rather than to a tenant, and so goes out over the deployment's own relay.
    pub const fn company_id(&self) -> Option<Uuid> {
        match self.attribution {
            Some(attribution) => Some(attribution.company_id),
            None => None,
        }
    }

    /// Whether this attempt is the delivery's last. Read to decide whether a failure dead-letters
    /// now or comes back once more.
    pub const fn attempts_exhausted(&self) -> bool {
        self.attempt_count >= self.max_attempts
    }
}

/// One frozen part as stored, with what has already happened to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPart {
    pub id: DeliveryPartId,
    pub rendered: RenderedPart,
    pub status: DeliveryPartStatus,
    pub attempt_count: i32,
    /// Set immediately before the provider call, and the whole reason a crash is classifiable.
    pub request_started_at: Option<DateTime<Utc>>,
    pub provider_message_key: Option<ExternalMessageKey>,
}

/// A delivery this worker owns, its identity, and its parts in index order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedDelivery {
    pub lease: ExecutionLease<DeliveryId>,
    pub record: DeliveryRecord,
    pub parts: Vec<StoredPart>,
}

impl ClaimedDelivery {
    /// The next part that still owes a provider call, in order.
    ///
    /// Resuming here is what stops a partially delivered multi-part send from re-posting the
    /// parts a provider already accepted.
    pub fn next_unfinished_part(&self) -> Option<&StoredPart> {
        self.parts.iter().find(|part| part.status.is_unfinished())
    }
}

/// One part result to commit, fenced on the parent's execution.
#[derive(Debug, Clone, Copy)]
pub struct PartResult<'a> {
    pub fence: &'a ExecutionLease<DeliveryId>,
    pub part_id: DeliveryPartId,
    pub outcome: &'a ProviderSendOutcome,
}

/// A failure that ends the whole delivery attempt without a provider having spoken: no adapter
/// registered, a payload that will not decode, a dependency that can never land.
#[derive(Debug, Clone)]
pub struct DeliveryFailure<'a> {
    pub fence: &'a ExecutionLease<DeliveryId>,
    pub class: FailureClass,
    pub detail: FailureDetail,
    /// Whether this failure can come out differently next time.
    ///
    /// [`Disposition::Terminal`] goes to dead letter immediately rather than spending five
    /// backoffs to reach the same verdict, which is what a payload that will not deserialize does.
    pub disposition: Disposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Retry,
    Terminal,
}

/// Where a fenced write left the delivery.
///
/// `LeaseLost` is not an error: it is the fence working. A run that finds it must stop touching
/// the row and must not treat its own provider call as unreported -- the replacement execution
/// owns the outcome now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Applied(DeliveryStatus),
    LeaseLost,
}

impl DeliveryOutcome {
    pub const fn lease_held(self) -> bool {
        matches!(self, Self::Applied(_))
    }
}

/// What one sweep of the queue's overdue rows did.
///
/// The two counts are separate because they are separate faults. An expired lease is a worker that
/// died or was descheduled; an orphaned dependency is a delivery that can never land because the
/// one it waits on cannot. Reporting them as one number hides whichever is smaller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeliveryReaping {
    pub leases_expired: u64,
    pub dependencies_orphaned: u64,
}

impl DeliveryReaping {
    pub const fn is_quiet(self) -> bool {
        self.leases_expired == 0 && self.dependencies_orphaned == 0
    }
}

/// Claiming, fencing and ending deliveries.
///
/// No method has a default body. Every one of them is a durable state transition or an ownership
/// check, which `src/application/AGENTS.md` forbids defaulting: a test double that quietly
/// returned "yes, renewed" would let a lease-loss test pass while the protocol was broken.
#[async_trait]
pub trait DeliveryQueue: Send + Sync {
    /// Take up to `limit` claimable deliveries whose time has come, oldest first.
    ///
    /// One atomic `UPDATE ... FROM (SELECT ... FOR UPDATE SKIP LOCKED)`, ordered deterministically
    /// so two claimants never contend for the same row. The claim is global rather than
    /// tenant-scoped on purpose -- that is what a worker is -- and fairness across tenants is the
    /// scheduling decision the worker makes with what it is given, not a filter in this query.
    ///
    /// A row whose dependency is not yet delivered is excluded, so a mirror cannot overtake the
    /// root post it threads under.
    async fn claim_deliveries(
        &self,
        owner: WorkerId,
        lease_for: Duration,
        limit: i64,
    ) -> AppResult<Vec<ClaimedDelivery>>;

    /// Extend a claim this execution still holds. `false` means it does not.
    async fn renew_delivery_lease(
        &self,
        fence: &ExecutionLease<DeliveryId>,
        until: DateTime<Utc>,
    ) -> AppResult<bool>;

    /// Record that the provider call for one part is about to be made.
    ///
    /// Committed before the call, not after, because it is what separates "this crashed before
    /// the provider saw it" from "this may already have been accepted". A part with no
    /// `request_started_at` is safely retryable; one with it set is not.
    async fn begin_part(
        &self,
        fence: &ExecutionLease<DeliveryId>,
        part_id: DeliveryPartId,
    ) -> AppResult<DeliveryOutcome>;

    /// Commit what the provider said about one part, and re-aggregate the parent from its parts.
    ///
    /// The parent becomes delivered only when every part is; one ambiguous part holds it at
    /// [`DeliveryStatus::OutcomeUnknown`]; one terminal part dead-letters it. Where the parent
    /// stays claimable, the attempt is counted and the backoff applied here rather than by the
    /// caller, so the two paths that end an attempt cannot age differently.
    async fn complete_part(&self, result: PartResult<'_>) -> AppResult<DeliveryOutcome>;

    /// End this delivery attempt without a provider result.
    async fn fail_delivery(&self, failure: DeliveryFailure<'_>) -> AppResult<DeliveryOutcome>;

    /// Release a claim this execution holds without spending an attempt, for a shutdown that
    /// interrupted the work before the provider was called.
    ///
    /// The row comes back immediately rather than after a lease period, which is what keeps a
    /// deploy from stranding queued mail for two minutes.
    async fn release_delivery(
        &self,
        fence: &ExecutionLease<DeliveryId>,
    ) -> AppResult<DeliveryOutcome>;

    /// Give back every claim whose lease ran out, charging each an attempt, and dead-letter the
    /// descendants of dependencies that can never be delivered.
    ///
    /// A part whose request had already started becomes [`DeliveryPartStatus::OutcomeUnknown`]
    /// rather than retryable: the provider may hold it, and an automatic resend is where a
    /// duplicate comes from.
    async fn reap_expired_deliveries(&self) -> AppResult<DeliveryReaping>;
}

/// Producer-side creation of a standalone notification on the generic delivery queue.
///
/// Separate from [`DeliveryQueue`] because producers may enqueue but must never claim, fence, or
/// settle work. The Postgres adapter implements both views over the same tables.
#[async_trait]
pub trait StandaloneDeliveryEnqueuer: Send + Sync {
    async fn enqueue_standalone_delivery(
        &self,
        delivery: NewStandaloneDelivery,
    ) -> AppResult<DeliveryCreation>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_then_stops_at_its_ceiling() {
        let backoff = DeliveryBackoff::DEFAULT;
        assert_eq!(Duration::from_secs(2), backoff.delay_after(0));
        assert_eq!(Duration::from_secs(4), backoff.delay_after(1));
        assert_eq!(Duration::from_secs(16), backoff.delay_after(3));
        // Clamped rather than growing without bound: a dead letter nobody is awake for is worse
        // than an earlier one.
        assert_eq!(backoff.cap, backoff.delay_after(20));
        assert_eq!(backoff.cap, backoff.delay_after(i32::MAX));
    }

    #[test]
    fn a_delivery_that_renders_nothing_is_refused_rather_than_queued() {
        assert!(NewDelivery::frozen_parts(Vec::new()).is_err());
    }

    #[test]
    fn a_claimed_delivery_resumes_at_its_first_unfinished_part() {
        let parts = vec![
            (DeliveryPartStatus::Delivered, 0),
            (DeliveryPartStatus::Retryable, 1),
            (DeliveryPartStatus::Prepared, 2),
        ];
        let claimed = ClaimedDelivery {
            lease: ExecutionLease::new(DeliveryId::random(), WorkerId::random(), Utc::now()),
            record: record(),
            parts: parts
                .into_iter()
                .map(|(status, index)| stored_part(status, index))
                .collect(),
        };

        let next = claimed
            .next_unfinished_part()
            .expect("one part is unfinished");
        assert_eq!(1, next.rendered.index.get());
    }

    fn record() -> DeliveryRecord {
        DeliveryRecord {
            id: DeliveryId::random(),
            attribution: Some(DeliveryAttribution {
                company_id: Uuid::new_v4(),
                channel_id: Uuid::new_v4(),
                message_id: CanonicalMessageId::random(),
                source_binding_id: ChannelBindingId::random(),
                destination_binding_id: ChannelBindingId::random(),
            }),
            external_destination: None,
            task_id: None,
            correlation_id: CorrelationId::new(),
            transport: TransportKind::Email,
            purpose: DeliveryPurpose::Notification,
            idempotency_key: DeliveryKey::parse("notification:key").unwrap(),
            attempt_count: 0,
            max_attempts: MAX_DELIVERY_ATTEMPTS,
        }
    }

    fn stored_part(status: DeliveryPartStatus, index: u16) -> StoredPart {
        use crate::transport::delivery::{ContentDigest, PartIndex, PartKey, TransportPayload};

        StoredPart {
            id: DeliveryPartId::random(),
            rendered: RenderedPart {
                index: PartIndex::new(index),
                key: PartKey::parse(format!("part-{index}")).unwrap(),
                payload: TransportPayload::encode(TransportKind::Email, 1, &index).unwrap(),
                digest: ContentDigest::sha256_of(b"body"),
            },
            status,
            attempt_count: 0,
            request_started_at: None,
            provider_message_key: None,
        }
    }
}
