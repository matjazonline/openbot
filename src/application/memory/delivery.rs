//! Promised memory deliveries (decisions E6, E8): the fenced worker that sends a committed
//! save or capture (generation) intent's frozen request until the service answers definitely or
//! the intent expires.
//! A retry resends the same key and body, keeps the original deadline and never becomes a new
//! generation request. Disable and owner removal settle intents in their own transactions.
use super::MemoryServiceError;
use super::ports::{
    ClientCredential, MemoryClient, MemoryGenerationReceipt, MemoryGenerationRequest,
    MemorySaveReceipt, MemorySaveRequest,
};
use crate::app_error::AppResult;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const DELIVERY_LEASE: Duration = Duration::from_secs(60);
/// Below the lease, and above the HTTP timeout (at most 30 s), so a live attempt never outlives
/// its lease and needs no heartbeat.
pub const DELIVERY_DEADLINE: Duration = Duration::from_secs(45);
pub const MAX_DELIVERY_ATTEMPTS: i32 = 8;
/// A save intent's lifetime: inside the service's 7-day idempotency window (D5), so the record
/// exists while any attempt can execute; afterwards the service answers 410 (E8).
pub const SAVE_INTENT_LIFETIME: chrono::Duration = chrono::Duration::days(6);

/// An intent's frozen request, by its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrozenRequest {
    Save(MemorySaveRequest),
    Generation(MemoryGenerationRequest),
}

impl FrozenRequest {
    /// The intent's idempotency key, which is also its correlation id (p06 A2-2).
    pub fn key(&self) -> &str {
        match self {
            Self::Save(request) => &request.idempotency_key,
            Self::Generation(request) => &request.idempotency_key,
        }
    }
}

/// What a delivered intent stores as its `result`: a save keeps the receipt shape its effect
/// wait reads; a generation keeps the accepted operation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum DeliveryReceipt {
    Save(MemorySaveReceipt),
    Generation(MemoryGenerationReceipt),
}

impl DeliveryReceipt {
    fn replayed(&self) -> bool {
        match self {
            Self::Save(receipt) => receipt.replayed,
            Self::Generation(receipt) => receipt.replayed,
        }
    }
}

/// One claimed intent: its fence, the namespace's opened credential and the frozen request.
pub struct DeliveryClaim {
    pub company: Uuid,
    pub id: Uuid,
    pub fence: i64,
    pub worker: Uuid,
    pub transient_retries: i32,
    pub credential: ClientCredential,
    pub request: FrozenRequest,
}

pub enum DeliveryClaimOutcome {
    /// Nothing was due.
    Empty,
    /// A due intent was settled at the claim without a request: expired, disabled since it
    /// was committed (a stale `disable_epoch`), or its namespace is gone.
    Settled,
    Claimed(Box<DeliveryClaim>),
}

/// What one attempt decided. Every variant moves the intent out of the claimable set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryResult {
    Delivered(DeliveryReceipt),
    /// An outage: back off and resend the same request, without spending an attempt.
    Retry {
        code: &'static str,
        retry_after: Option<Duration>,
    },
    Failed(&'static str),
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryCompletion {
    Applied,
    /// The fence moved (lease expiry, disable, owner removal): this attempt changed nothing.
    Lost,
}

/// Persistence for the delivery queue. Every method is a durable, fenced transition.
#[async_trait]
pub trait MemoryDeliveryStore: Send + Sync {
    async fn claim_delivery(
        &self,
        worker: Uuid,
        lease: Duration,
    ) -> AppResult<DeliveryClaimOutcome>;
    async fn complete_delivery(
        &self,
        claim: &DeliveryClaim,
        result: DeliveryResult,
    ) -> AppResult<DeliveryCompletion>;
}

/// The service's answer, classified once (E1, E8): 409 is terminal, 410 expired, a refused grant
/// or classification terminal, `generation_disabled` terminal, and only outages and a refused
/// credential retry, always with the same key. A `401` on a credential OpenBot holds as live is
/// transient (p06 A4-9): the client re-pends the namespace's lifecycle, which verifies the
/// credential or issues a new one, and the intent retries until its own deadline.
pub fn delivery_result(answer: Result<DeliveryReceipt, MemoryServiceError>) -> DeliveryResult {
    match answer {
        Ok(receipt) => DeliveryResult::Delivered(receipt),
        Err(MemoryServiceError::Unavailable) => DeliveryResult::Retry {
            code: "service_unavailable",
            retry_after: None,
        },
        Err(MemoryServiceError::Ambiguous) => DeliveryResult::Retry {
            code: "outcome_unknown",
            retry_after: None,
        },
        Err(MemoryServiceError::Capacity { retry_after_secs }) => DeliveryResult::Retry {
            code: "service_capacity",
            retry_after: retry_after_secs.map(Duration::from_secs),
        },
        Err(MemoryServiceError::Expired) => DeliveryResult::Expired,
        Err(MemoryServiceError::Conflict { .. }) => DeliveryResult::Failed("conflict"),
        Err(MemoryServiceError::Forbidden { code }) if code == "generation_disabled" => {
            DeliveryResult::Failed("generation_disabled")
        }
        Err(MemoryServiceError::Unauthorized) => DeliveryResult::Retry {
            code: "credential_unverified",
            retry_after: None,
        },
        Err(MemoryServiceError::Forbidden { .. }) => DeliveryResult::Failed("authority"),
        Err(MemoryServiceError::Refused { .. }) => DeliveryResult::Failed("refused"),
        Err(MemoryServiceError::NotFound) => DeliveryResult::Failed("not_found"),
        Err(MemoryServiceError::Protocol) => DeliveryResult::Failed("protocol_error"),
    }
}

pub struct MemoryDeliveryWorker {
    store: Arc<dyn MemoryDeliveryStore>,
    client: Arc<dyn MemoryClient>,
    worker: Uuid,
    poll: Duration,
    lanes: usize,
    /// Serializes the lanes' store calls, so the worker holds at most one database connection
    /// at a time (the one `RESERVED_DATABASE_CONNECTIONS` counts) however many sends are in flight.
    database: tokio::sync::Mutex<()>,
    monitoring: Option<Arc<dyn crate::domain::monitoring::MonitoringService>>,
}

impl MemoryDeliveryWorker {
    pub fn new(store: Arc<dyn MemoryDeliveryStore>, client: Arc<dyn MemoryClient>) -> Self {
        Self {
            store,
            client,
            worker: Uuid::new_v4(),
            poll: Duration::from_secs(1),
            lanes: 1,
            database: tokio::sync::Mutex::new(()),
            monitoring: None,
        }
    }

    /// Count delivery outcomes (p06 A2-4).
    pub fn with_monitoring(
        mut self,
        monitoring: Arc<dyn crate::domain::monitoring::MonitoringService>,
    ) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    /// Send up to `lanes` intents at once (at least one). The claim leaves out companies that
    /// already have a save in flight (their share of the Delivery pool), so one company's
    /// backlog holds at most one lane and other companies' saves proceed in the rest. Only the
    /// sends overlap: the lanes take turns on the store (one short claim or completion each).
    pub fn with_lanes(mut self, lanes: usize) -> Self {
        self.lanes = lanes.max(1);
        self
    }

    /// Until `stop`: `lanes` concurrent loops, each working while intents are due, polling when
    /// none are and backing off on store errors.
    pub async fn run(&self, stop: &CancellationToken) {
        futures::future::join_all((0..self.lanes).map(|_| self.lane(stop))).await;
    }

    async fn lane(&self, stop: &CancellationToken) {
        loop {
            let delay = match self.pass(stop).await {
                Ok(true) => Duration::ZERO,
                Ok(false) => self.poll,
                Err(error) => {
                    tracing::warn!(error = %error, "Memory delivery pass failed; retrying");
                    Duration::from_secs(5)
                }
            };
            tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = tokio::time::sleep(delay) => {}
            }
        }
    }

    /// One due intent, sent and completed. `false` when nothing was due. A busy pass is
    /// evidence of progress only because claims and completions move rows out of the due set.
    pub async fn pass(&self, stop: &CancellationToken) -> AppResult<bool> {
        let claimed = {
            let _database = self.database.lock().await;
            // A lane that waited out shutdown on the mutex claims nothing (review
            // MEM-P05-B2-F16): a claim released as `interrupted` would spend a transient retry.
            if stop.is_cancelled() {
                return Ok(false);
            }
            self.store
                .claim_delivery(self.worker, DELIVERY_LEASE)
                .await?
        };
        let claim = match claimed {
            DeliveryClaimOutcome::Empty => return Ok(false),
            DeliveryClaimOutcome::Settled => return Ok(true),
            DeliveryClaimOutcome::Claimed(claim) => claim,
        };
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => DeliveryResult::Retry { code: "interrupted", retry_after: None },
            answer = tokio::time::timeout(DELIVERY_DEADLINE, self.send(&claim)) => match answer {
                Ok(answer) => delivery_result(answer),
                Err(_) => DeliveryResult::Retry { code: "deadline_exceeded", retry_after: None },
            },
        };
        let kind = match &claim.request {
            FrozenRequest::Save(_) => "save",
            FrozenRequest::Generation(_) => "generation",
        };
        let state = match &result {
            DeliveryResult::Delivered(_) => "delivered",
            DeliveryResult::Retry { .. } => "retry",
            DeliveryResult::Failed(_) => "failed",
            DeliveryResult::Expired => "expired",
        };
        let summary = match &result {
            DeliveryResult::Delivered(receipt) => {
                if receipt.replayed() {
                    "replayed"
                } else {
                    "delivered"
                }
            }
            DeliveryResult::Retry { code, .. } | DeliveryResult::Failed(code) => code,
            DeliveryResult::Expired => "expired",
        };
        let completion = {
            let _database = self.database.lock().await;
            self.store.complete_delivery(&claim, result).await?
        };
        if let Some(monitoring) = &self.monitoring {
            match completion {
                DeliveryCompletion::Lost => {
                    monitoring.increment_counter("memory_delivery_completions_lost_total", 1, &[]);
                }
                DeliveryCompletion::Applied if state == "retry" => {
                    monitoring.increment_counter("memory_delivery_transient_retries_total", 1, &[]);
                }
                DeliveryCompletion::Applied => monitoring.increment_counter(
                    "memory_deliveries_settled_total",
                    1,
                    &[("kind", kind), ("state", state)],
                ),
            }
        }
        tracing::info!(
            company_id = %claim.company,
            delivery_id = %claim.id,
            correlation_id = claim.request.key(),
            fence = claim.fence,
            outcome = summary,
            completion = ?completion,
            "Memory delivery pass"
        );
        Ok(true)
    }

    async fn send(&self, claim: &DeliveryClaim) -> Result<DeliveryReceipt, MemoryServiceError> {
        // p06 A2-2: an intent's correlation is its idempotency key (`ob.s.…`, `ob.g.…`).
        super::with_correlation(claim.request.key().to_owned(), self.send_frozen(claim)).await
    }

    async fn send_frozen(
        &self,
        claim: &DeliveryClaim,
    ) -> Result<DeliveryReceipt, MemoryServiceError> {
        match &claim.request {
            FrozenRequest::Save(request) => self
                .client
                .save(&claim.credential, request)
                .await
                .map(DeliveryReceipt::Save),
            FrozenRequest::Generation(request) => self
                .client
                .request_generation(&claim.credential, request)
                .await
                .map(DeliveryReceipt::Generation),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_integration_delivery_classifies_every_service_answer() {
        use MemoryServiceError as E;
        let retry = |error| matches!(delivery_result(Err(error)), DeliveryResult::Retry { .. });
        assert!(retry(E::Unavailable));
        assert!(retry(E::Ambiguous));
        assert_eq!(
            delivery_result(Err(E::Capacity {
                retry_after_secs: Some(7)
            })),
            DeliveryResult::Retry {
                code: "service_capacity",
                retry_after: Some(Duration::from_secs(7))
            }
        );
        assert_eq!(delivery_result(Err(E::Expired)), DeliveryResult::Expired);
        assert_eq!(
            delivery_result(Err(E::Unauthorized)),
            DeliveryResult::Retry {
                code: "credential_unverified",
                retry_after: None
            },
            "p06 A4-9: a refused credential is re-verified, not terminal"
        );
        for (error, code) in [
            (
                E::Conflict {
                    code: "idempotency_conflict".into(),
                },
                "conflict",
            ),
            (
                E::Forbidden {
                    code: "tenant_disabled".into(),
                },
                "authority",
            ),
            (
                E::Forbidden {
                    code: "generation_disabled".into(),
                },
                "generation_disabled",
            ),
            (
                E::Refused {
                    reason: "classification_refused".into(),
                },
                "refused",
            ),
            (E::NotFound, "not_found"),
            (E::Protocol, "protocol_error"),
        ] {
            assert_eq!(delivery_result(Err(error)), DeliveryResult::Failed(code));
        }
    }
}
