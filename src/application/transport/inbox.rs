//! The application contract for the authenticated, durable inbound-event inbox.
//!
//! The HTTP boundary stores exact bounded bytes through [`InboundEventInbox`]. A worker later
//! claims them through [`InboundEventQueue`], selects a decoder by transport, and gives the
//! resulting canonical commit the same execution fence. Raw bytes never enter a task payload.

use std::{collections::BTreeMap, fmt, str::FromStr, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    domain::monitoring::MonitoringService,
    entities::{
        correlation::CorrelationId,
        transport::{
            ExternalEventKey, InboundEventErrorClass, InboundEventId, InboundEventIgnoreReason,
            InboundEventStatus, InstallationId, TransportKind, bounded_string,
        },
    },
    transport::{ExecutionLease, InboundCommitRequest, WorkerId},
};

pub const MAX_INBOUND_EVENT_PAYLOAD_BYTES: usize = 1024 * 1024;
pub const MAX_INBOUND_CONTENT_TYPE_BYTES: usize = 255;
pub const MAX_INBOUND_ERROR_DETAIL_BYTES: usize = 512;
pub const MAX_SAFE_HEADER_FACTS: usize = 16;
pub const MAX_SAFE_HEADER_NAME_BYTES: usize = 64;
pub const MAX_SAFE_HEADER_VALUE_BYTES: usize = 256;
pub const MAX_INBOUND_EVENT_ATTEMPTS: i32 = 5;
pub const INBOUND_EVENT_CLAIM_BATCH: i64 = 8;
pub const INBOUND_EVENT_LEASE_SECONDS: i64 = 120;

bounded_string!(InboundContentType, MAX_INBOUND_CONTENT_TYPE_BYTES);
bounded_string!(InboundFailureDetail, MAX_INBOUND_ERROR_DETAIL_BYTES);

/// Exact request bytes, bounded before they can be allocated into a durable row.
#[derive(Clone, PartialEq, Eq)]
pub struct InboundEventPayload(Vec<u8>);

impl InboundEventPayload {
    pub fn parse(bytes: Vec<u8>) -> Result<Self, InboundPayloadError> {
        if bytes.is_empty() {
            return Err(InboundPayloadError::Empty);
        }
        if bytes.len() > MAX_INBOUND_EVENT_PAYLOAD_BYTES {
            return Err(InboundPayloadError::TooLarge {
                actual: bytes.len(),
                max: MAX_INBOUND_EVENT_PAYLOAD_BYTES,
            });
        }
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for InboundEventPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InboundEventPayload")
            .field("bytes", &self.0.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InboundPayloadError {
    #[error("inbound event payload must not be empty")]
    Empty,
    #[error("inbound event payload is {actual} bytes; maximum is {max}")]
    TooLarge { actual: usize, max: usize },
}

/// The SHA-256 of the exact authenticated bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundPayloadDigest([u8; 32]);

impl InboundPayloadDigest {
    pub fn sha256(payload: &InboundEventPayload) -> Self {
        Self(Sha256::digest(payload.as_bytes()).into())
    }

    pub fn parse(bytes: Vec<u8>) -> Result<Self, InboundDigestError> {
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|bytes: Vec<u8>| InboundDigestError {
                actual: bytes.len(),
            })?;
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("inbound payload digest is {actual} bytes; expected 32")]
pub struct InboundDigestError {
    actual: usize,
}

/// A small allowlist selected by the authenticating adapter.
///
/// This type enforces shape and size, but the adapter still decides which headers are safe. Raw
/// authorization and signature headers must never be passed here.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct SafeHeaderFacts(BTreeMap<String, String>);

impl SafeHeaderFacts {
    pub fn parse(facts: BTreeMap<String, String>) -> Result<Self, SafeHeaderFactsError> {
        if facts.len() > MAX_SAFE_HEADER_FACTS {
            return Err(SafeHeaderFactsError::TooMany {
                actual: facts.len(),
                max: MAX_SAFE_HEADER_FACTS,
            });
        }
        for (name, value) in &facts {
            validate_header_component(name, MAX_SAFE_HEADER_NAME_BYTES, true)?;
            validate_header_component(value, MAX_SAFE_HEADER_VALUE_BYTES, false)?;
        }
        Ok(Self(facts))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn into_inner(self) -> BTreeMap<String, String> {
        self.0
    }
}

fn validate_header_component(
    value: &str,
    max: usize,
    require_token: bool,
) -> Result<(), SafeHeaderFactsError> {
    if value.is_empty() {
        return Err(SafeHeaderFactsError::Empty);
    }
    if value.len() > max {
        return Err(SafeHeaderFactsError::TooLong { max });
    }
    if value.chars().any(char::is_control) {
        return Err(SafeHeaderFactsError::ControlCharacter);
    }
    if require_token
        && !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(SafeHeaderFactsError::InvalidName);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SafeHeaderFactsError {
    #[error("too many safe header facts: {actual}; maximum is {max}")]
    TooMany { actual: usize, max: usize },
    #[error("safe header fact must not be empty")]
    Empty,
    #[error("safe header fact exceeds its {max}-byte limit")]
    TooLong { max: usize },
    #[error("safe header fact contains a control character")]
    ControlCharacter,
    #[error("safe header fact names use lowercase letters, digits, and underscores only")]
    InvalidName,
}

/// One request that has already passed its transport's authentication boundary.
#[derive(Debug, Clone)]
pub struct AuthenticatedInboundEvent {
    pub transport: TransportKind,
    pub company_id: Uuid,
    pub installation_id: Option<InstallationId>,
    pub external_event_key: ExternalEventKey,
    pub correlation_id: CorrelationId,
    pub payload: InboundEventPayload,
    pub content_type: Option<InboundContentType>,
    pub safe_header_facts: SafeHeaderFacts,
    pub received_at: DateTime<Utc>,
}

impl AuthenticatedInboundEvent {
    pub fn digest(&self) -> InboundPayloadDigest {
        InboundPayloadDigest::sha256(&self.payload)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundEventStoreOutcome {
    Stored(InboundEventId),
    Duplicate(InboundEventId),
}

impl InboundEventStoreOutcome {
    pub const fn event_id(self) -> InboundEventId {
        match self {
            Self::Stored(id) | Self::Duplicate(id) => id,
        }
    }

    pub const fn was_stored(self) -> bool {
        matches!(self, Self::Stored(_))
    }
}

#[derive(Debug, Clone)]
pub struct InboundEventRecord {
    pub id: InboundEventId,
    pub company_id: Uuid,
    pub installation_id: Option<InstallationId>,
    pub transport: TransportKind,
    pub external_event_key: ExternalEventKey,
    pub correlation_id: CorrelationId,
    pub payload: InboundEventPayload,
    pub payload_digest: InboundPayloadDigest,
    pub content_type: Option<InboundContentType>,
    pub safe_header_facts: SafeHeaderFacts,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ClaimedInboundEvent {
    pub lease: ExecutionLease<InboundEventId>,
    pub record: InboundEventRecord,
}

#[derive(Debug, Clone)]
pub struct InboundEventFailure<'a> {
    pub fence: &'a ExecutionLease<InboundEventId>,
    pub class: InboundEventErrorClass,
    pub detail: InboundFailureDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundEventTransition {
    Applied(InboundEventStatus),
    LeaseLost,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InboundEventReaping {
    pub leases_expired: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InboundEventRetention {
    pub completed_deleted: u64,
    pub ignored_deleted: u64,
    pub dead_letters_deleted: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundRetentionPolicy {
    pub completed_for: Duration,
    pub ignored_for: Duration,
    pub dead_letters_for: Duration,
    pub batch_size: i64,
}

impl Default for InboundRetentionPolicy {
    fn default() -> Self {
        Self {
            completed_for: Duration::from_secs(24 * 60 * 60),
            ignored_for: Duration::from_secs(24 * 60 * 60),
            dead_letters_for: Duration::from_secs(30 * 24 * 60 * 60),
            batch_size: 1_000,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct InboundEventCensus {
    pub pending: u64,
    pub processing: u64,
    pub retryable: u64,
    pub completed: u64,
    pub ignored: u64,
    pub dead_letter: u64,
    pub oldest_ready_age: Option<Duration>,
}

#[async_trait]
pub trait InboundEventInbox: Send + Sync {
    async fn store_authenticated(
        &self,
        event: AuthenticatedInboundEvent,
    ) -> AppResult<InboundEventStoreOutcome>;
}

/// Counts what every authenticating adapter stores, so arrival volume and provider redelivery are
/// visible before any route exists to interpret them.
///
/// This wraps the port rather than living in the persistence adapter: storage is a database
/// concern, while "how many events did this transport deliver twice" is an operational one, and
/// the adapter has no monitoring handle to answer it with.
pub struct MonitoredInboundEventInbox {
    inner: Arc<dyn InboundEventInbox>,
    monitoring: Arc<dyn MonitoringService>,
}

impl MonitoredInboundEventInbox {
    pub fn new(inner: Arc<dyn InboundEventInbox>, monitoring: Arc<dyn MonitoringService>) -> Self {
        Self { inner, monitoring }
    }
}

#[async_trait]
impl InboundEventInbox for MonitoredInboundEventInbox {
    async fn store_authenticated(
        &self,
        event: AuthenticatedInboundEvent,
    ) -> AppResult<InboundEventStoreOutcome> {
        let transport = event.transport;
        let outcome = self.inner.store_authenticated(event).await?;
        let labels = [("transport", transport.as_str())];
        self.monitoring
            .increment_counter("inbound_events_received_total", 1, &labels);
        if !outcome.was_stored() {
            self.monitoring
                .increment_counter("inbound_events_duplicate_total", 1, &labels);
        }
        Ok(outcome)
    }
}

#[async_trait]
pub trait InboundEventQueue: Send + Sync {
    async fn claim_inbound_events(
        &self,
        owner: WorkerId,
        lease_for: Duration,
        limit: i64,
    ) -> AppResult<Vec<ClaimedInboundEvent>>;

    async fn renew_inbound_event_lease(
        &self,
        fence: &ExecutionLease<InboundEventId>,
        until: DateTime<Utc>,
    ) -> AppResult<bool>;

    async fn complete_inbound_event(
        &self,
        fence: &ExecutionLease<InboundEventId>,
    ) -> AppResult<InboundEventTransition>;

    async fn ignore_inbound_event(
        &self,
        fence: &ExecutionLease<InboundEventId>,
        reason: InboundEventIgnoreReason,
    ) -> AppResult<InboundEventTransition>;

    async fn retry_inbound_event(
        &self,
        failure: InboundEventFailure<'_>,
    ) -> AppResult<InboundEventTransition>;

    async fn dead_letter_inbound_event(
        &self,
        failure: InboundEventFailure<'_>,
    ) -> AppResult<InboundEventTransition>;

    async fn reap_expired_inbound_events(&self) -> AppResult<InboundEventReaping>;

    async fn inbound_event_census(&self) -> AppResult<InboundEventCensus>;

    async fn purge_inbound_events(
        &self,
        policy: InboundRetentionPolicy,
    ) -> AppResult<InboundEventRetention>;
}

/// A decoder classifies every claimed payload; malformed and unsupported shapes are data, not a
/// generic error that the worker has to guess about.
#[async_trait]
pub trait InboundEventDecoder: Send + Sync {
    fn transport(&self) -> TransportKind;

    async fn decode(&self, event: &InboundEventRecord) -> InboundEventDecodeOutcome;
}

pub enum InboundEventDecodeOutcome {
    /// A fully planned canonical commit. The decoder leaves `claimed_event` empty; the worker is
    /// the sole owner allowed to inject the live fence before calling `commit_inbound`.
    Message(Box<InboundCommitRequest>),
    Ignore(InboundEventIgnoreReason),
    Retry {
        class: InboundEventErrorClass,
        detail: InboundFailureDetail,
    },
    Terminal {
        class: InboundEventErrorClass,
        detail: InboundFailureDetail,
    },
}

#[derive(Default)]
pub struct InboundEventDecoderRegistry {
    decoders: BTreeMap<&'static str, Arc<dyn InboundEventDecoder>>,
}

impl InboundEventDecoderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        mut self,
        decoder: Arc<dyn InboundEventDecoder>,
    ) -> Result<Self, DuplicateInboundDecoder> {
        let transport = decoder.transport();
        if self.decoders.insert(transport.as_str(), decoder).is_some() {
            return Err(DuplicateInboundDecoder(transport));
        }
        Ok(self)
    }

    pub fn get(&self, transport: TransportKind) -> Option<Arc<dyn InboundEventDecoder>> {
        self.decoders.get(transport.as_str()).cloned()
    }

    pub fn registered(&self) -> impl Iterator<Item = TransportKind> + '_ {
        self.decoders
            .keys()
            .filter_map(|transport| TransportKind::from_str(transport).ok())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an inbound decoder for {0} is already registered")]
pub struct DuplicateInboundDecoder(TransportKind);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_debug_never_contains_its_bytes() {
        let payload = InboundEventPayload::parse(b"top-secret-message".to_vec()).unwrap();
        let rendered = format!("{payload:?}");
        assert!(!rendered.contains("top-secret-message"));
        assert!(rendered.contains("18"));
    }

    #[test]
    fn payload_and_safe_facts_are_bounded() {
        assert!(InboundEventPayload::parse(Vec::new()).is_err());
        assert!(InboundEventPayload::parse(vec![0; MAX_INBOUND_EVENT_PAYLOAD_BYTES + 1]).is_err());
        assert!(
            SafeHeaderFacts::parse(BTreeMap::from([("x-signature".into(), "secret".into())]))
                .is_err()
        );
        assert!(
            SafeHeaderFacts::parse(BTreeMap::from([("retry_number".into(), "1".into())])).is_ok()
        );
    }
}
