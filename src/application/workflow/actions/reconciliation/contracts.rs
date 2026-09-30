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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileActionCommand {
    pub actor: WorkflowActor,
    pub scope: ActionScope,
    pub subject: ApprovalSubject,
    pub marker: ActionRemoteMarkerId,
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
        {
            return Err(invalid());
        }
        let value = super::super::freeze::canonical(
            &json!({"version":1,"actor":self.actor.user_id(),"scope":self.scope,
                "subject":self.subject,"marker":self.marker,"revision":self.expected_revision.0,
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

/// Restored by the persistence snapshot transaction, including parked/terminal runs.
pub struct ReconciliationSnapshot {
    pub action: FrozenAction,
    pub subject: ApprovalSubject,
    pub marker: ActionRemoteMarkerId,
    pub marker_created_at: DateTime<Utc>,
    pub entries: Vec<ReconciliationEntry>,
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

    pub fn validate(&self, command: &ReconcileActionCommand) -> AppResult<()> {
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
            || self.coverage_digest
                != Self::coverage(
                    &self.action,
                    &self.subject,
                    self.marker,
                    self.marker_created_at,
                    &self.entries,
                )?
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
    AppliedRecorded { receipt: bool },
    NotAppliedRecorded,
    Scheduled { receipt_only: bool },
    Blocked { reason: ReconciliationBlockedReason },
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
