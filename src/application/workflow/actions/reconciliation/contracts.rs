use super::super::{ActionScope, ApprovalSubject, FrozenAction};
use crate::application::app_error::{AppError, AppResult};
use crate::application::workflow::lease::WorkflowGeneration;
use crate::application::workflow::{IdempotencyKey, RunRevision, WorkerId, WorkflowActor};
use crate::domain::workflow::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const MAX_EVIDENCE_NOTE_BYTES: usize = 4096;
pub const MAX_EVIDENCE_ENVELOPE_BYTES: usize = 16_384;
pub const MAX_RECONCILIATION_COVERAGE: usize = 128;

/// A note may claim a disposition; the effective disposition is always Unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimedDisposition {
    Applied,
    NotApplied,
    Unknown,
}

/// Decodable caller input deliberately has no verdict, result, or quiescence field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceInput {
    UnknownNote {
        note: String,
        claimed: ClaimedDisposition,
    },
    VerifiedReference {
        registration: EvidenceVerifierId,
        reference: EvidenceRecordReference,
    },
}
impl EvidenceInput {
    pub fn validate(&self) -> AppResult<()> {
        if let Self::UnknownNote { note, .. } = self
            && note.len() > MAX_EVIDENCE_NOTE_BYTES
        {
            return Err(invalid());
        }
        Ok(())
    }
}

/// What one command attests (04.11-D3 §H.1): the remote marker's requests, or exactly one
/// destination delivery of a message invocation. Part of the request digest; a target whose
/// kind differs from the marker's effect kind is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReconciliationTarget {
    Remote,
    MessageDelivery { ordinal: u8, delivery: uuid::Uuid },
}
impl ReconciliationTarget {
    /// The marker effect kind this target attests.
    pub const fn effect_kind(self) -> &'static str {
        match self {
            Self::Remote => "remote",
            Self::MessageDelivery { .. } => "message",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileActionCommand {
    pub actor: WorkflowActor,
    pub scope: ActionScope,
    pub subject: ApprovalSubject,
    pub marker: ActionRemoteMarkerId,
    pub target: ReconciliationTarget,
    pub expected_revision: RunRevision,
    pub command_key: IdempotencyKey,
    pub input: EvidenceInput,
}
impl ReconcileActionCommand {
    pub fn request_digest(&self) -> AppResult<ReconciliationRequestDigest> {
        self.input.validate()?;
        if self.scope.company.as_uuid().is_nil()
            || self.scope.run.as_uuid().is_nil()
            || self.scope.execution.as_uuid().is_nil()
            || self.subject.invocation.as_uuid().is_nil()
            || self.marker.as_uuid().is_nil()
            || self.expected_revision.0 == 0
            || self.expected_revision.0 > i64::MAX as u64
            || matches!(self.target, ReconciliationTarget::MessageDelivery { ordinal, delivery }
                if usize::from(ordinal) >= crate::application::workflow::registry::MAX_MESSAGE_DESTINATIONS
                    || delivery.is_nil())
        {
            return Err(invalid());
        }
        let value = super::super::freeze::canonical(
            &json!({"version":2,"actor":self.actor.user_id(),"scope":self.scope,
                "subject":self.subject,"marker":self.marker,"target":self.target,
                "revision":self.expected_revision.0,
                "command_key":self.command_key.as_str(),"input":self.input}),
            MAX_EVIDENCE_ENVELOPE_BYTES,
        )?;
        ReconciliationRequestDigest::parse(hash(&value)?).map_err(|_| invalid())
    }
}

/// Exact prior request identity. Sorting never drops duplicates or truncates coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationEntry {
    pub id: ActionRemoteEntryId,
    pub attempt: ActionRemoteAttemptId,
    pub generation: WorkflowGeneration,
    pub worker: WorkerId,
    pub created_at: DateTime<Utc>,
}

/// The one outcome-unknown delivery a message reconciliation attests (04.11-D3 §H.2): what a
/// provider lookup can compare, and the facts settlement acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliverySubject {
    pub ordinal: u8,
    pub delivery: uuid::Uuid,
    pub part: uuid::Uuid,
    pub recipient: String,
    /// The predicted RFC Message-ID every request of this delivery carried.
    pub message_id: String,
    /// The frozen part's body digest.
    pub content_digest: String,
    /// The delivery's transport, which an installed registration's provider must equal.
    pub transport: String,
    pub source_binding: uuid::Uuid,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub request_started_at: DateTime<Utc>,
    pub cancellation: Option<String>,
}

/// Restored by the persistence snapshot transaction, including parked/terminal runs.
/// `entries` covers a remote marker; `delivery` is the subject of a message target, whose
/// marker has no remote entries.
pub struct ReconciliationSnapshot {
    pub action: FrozenAction,
    pub subject: ApprovalSubject,
    pub marker: ActionRemoteMarkerId,
    pub marker_created_at: DateTime<Utc>,
    pub entries: Vec<ReconciliationEntry>,
    pub delivery: Option<DeliverySubject>,
    pub revision: RunRevision,
    pub database_now: DateTime<Utc>,
    pub coverage_digest: ActionCoverageDigest,
}
impl ReconciliationSnapshot {
    pub fn coverage(
        action: &FrozenAction,
        subject: &ApprovalSubject,
        marker: ActionRemoteMarkerId,
        marker_created_at: DateTime<Utc>,
        entries: &[ReconciliationEntry],
    ) -> AppResult<ActionCoverageDigest> {
        if entries.len() > MAX_RECONCILIATION_COVERAGE
            || marker.as_uuid().is_nil()
            || subject.invocation.as_uuid().is_nil()
            || action.argument_digest() != &subject.argument_digest
        {
            return Err(invalid());
        }
        let mut prior = None;
        let mut coverage = Vec::with_capacity(entries.len());
        for entry in entries {
            if entry.id.as_uuid().is_nil()
                || entry.attempt.as_uuid().is_nil()
                || entry.worker.as_uuid().is_nil()
                || entry.generation.0.is_nil()
                || entry.created_at < marker_created_at
                || prior.is_some_and(|id| id >= entry.id)
            {
                return Err(invalid());
            }
            prior = Some(entry.id);
            coverage.push(json!({"entry":entry.id,"attempt":entry.attempt,
                "generation":entry.generation.0,"worker":entry.worker.as_uuid(),
                "created_at":entry.created_at}));
        }
        let value = super::super::freeze::canonical(
            &json!({"version":1,"scope":action.scope(),"subject":subject,"marker":marker,
                "marker_created_at":marker_created_at,"operation":action.operation_digest(),
                "entries":coverage}),
            super::super::freeze::MAX_OPERATION_BYTES,
        )?;
        ActionCoverageDigest::parse(hash(&value)?).map_err(|_| invalid())
    }

    /// Canonical, versioned coverage of one outcome-unknown delivery: every fact a stale
    /// snapshot could differ in, so settlement refuses rather than grants on changed truth.
    pub fn delivery_coverage(
        action: &FrozenAction,
        subject: &ApprovalSubject,
        marker: ActionRemoteMarkerId,
        delivery: &DeliverySubject,
    ) -> AppResult<ActionCoverageDigest> {
        if marker.as_uuid().is_nil()
            || subject.invocation.as_uuid().is_nil()
            || action.argument_digest() != &subject.argument_digest
            || delivery.delivery.is_nil()
            || delivery.part.is_nil()
            || usize::from(delivery.ordinal)
                >= crate::application::workflow::registry::MAX_MESSAGE_DESTINATIONS
        {
            return Err(invalid());
        }
        let value = super::super::freeze::canonical(
            &json!({"version":1,"kind":"message_delivery","scope":action.scope(),
                "subject":subject,"marker":marker,"operation":action.operation_digest(),
                "ordinal":delivery.ordinal,"delivery":delivery.delivery,"part":delivery.part,
                "message_id":delivery.message_id,"recipient":delivery.recipient,
                "content_digest":delivery.content_digest,"attempt_count":delivery.attempt_count,
                "request_started_at":delivery.request_started_at}),
            super::super::freeze::MAX_OPERATION_BYTES,
        )?;
        ActionCoverageDigest::parse(hash(&value)?).map_err(|_| invalid())
    }

    pub fn validate(&self, command: &ReconcileActionCommand) -> AppResult<()> {
        let coverage = match (&self.delivery, command.target) {
            (None, ReconciliationTarget::Remote) => Self::coverage(
                &self.action,
                &self.subject,
                self.marker,
                self.marker_created_at,
                &self.entries,
            )?,
            (
                Some(delivery),
                ReconciliationTarget::MessageDelivery {
                    ordinal,
                    delivery: id,
                },
            ) if self.entries.is_empty()
                && delivery.ordinal == ordinal
                && delivery.delivery == id
                && delivery.request_started_at <= self.database_now =>
            {
                Self::delivery_coverage(&self.action, &self.subject, self.marker, delivery)?
            }
            _ => return Err(invalid()),
        };
        if self.action.scope() != command.scope
            || self.subject != command.subject
            || self.marker != command.marker
            || self.revision.0 == 0
            || self.revision.0 > i64::MAX as u64
            || self.marker_created_at > self.database_now
            || self
                .entries
                .iter()
                .any(|entry| entry.created_at > self.database_now)
            || self.coverage_digest != coverage
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationBlockedReason {
    Unknown,
    MissingResult,
    InvalidRecoveredResult,
    BoundExceeded,
    StaleSnapshot,
    ExpiredProof,
    Terminal,
    IneligibleJob,
    EvidenceConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReconciliationOutcome {
    UnknownRecorded,
    AppliedRecorded {
        receipt: bool,
    },
    NotAppliedRecorded,
    Scheduled {
        receipt_only: bool,
    },
    /// A message delivery proven not applied returned to the delivery queue (04.11-D4 §R).
    /// It never reopens a workflow job: message outcomes stay out of the 04.7 continuation.
    DeliveryReopened,
    Blocked {
        reason: ReconciliationBlockedReason,
    },
    EvidenceConflict,
    RevisionConflict,
    IdempotencyConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationResult {
    pub outcome: ReconciliationOutcome,
    pub revision: RunRevision,
    pub evidence: Option<ActionEvidenceId>,
    pub replayed: bool,
}

pub(crate) fn invalid() -> AppError {
    AppError::BadRequest("Invalid workflow reconciliation evidence".into())
}
pub(crate) fn hash(value: &Value) -> AppResult<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
