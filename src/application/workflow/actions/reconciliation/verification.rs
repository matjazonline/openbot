use super::super::{ActionTarget, ArgumentDigest};
use super::contracts::*;
use crate::application::app_error::AppResult;
use crate::application::workflow::publication::ToolSnapshot;
use crate::domain::workflow::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// Trusted host registration for one company/provider/version and exact tool/target.
/// No Deserialize: discovery metadata and command input cannot register a verifier.
#[derive(Clone)]
pub struct EvidenceVerifierRegistration {
    id: EvidenceVerifierId,
    version: EvidenceVerifierVersion,
    provider: EvidenceProviderId,
    company: CompanyId,
    operation: ArgumentDigest,
}
impl EvidenceVerifierRegistration {
    pub fn approve(
        id: EvidenceVerifierId,
        version: EvidenceVerifierVersion,
        provider: EvidenceProviderId,
        tool: &ToolSnapshot,
        target: &ActionTarget,
    ) -> AppResult<Self> {
        if tool.company_id.as_uuid().is_nil() || tool.policy.policy_revision == 0 {
            return Err(invalid());
        }
        let subject = super::super::replay_subject(tool, target)?;
        use sha2::{Digest, Sha256};
        Ok(Self {
            id,
            version,
            provider,
            company: tool.company_id,
            operation: ArgumentDigest(format!("{:x}", Sha256::digest(subject))),
        })
    }

    pub fn id(&self) -> &EvidenceVerifierId {
        &self.id
    }

    pub fn version(&self) -> &EvidenceVerifierVersion {
        &self.version
    }
    pub fn provider(&self) -> &EvidenceProviderId {
        &self.provider
    }
    pub fn company(&self) -> CompanyId {
        self.company
    }
    pub fn operation(&self) -> &ArgumentDigest {
        &self.operation
    }

    pub fn matches_snapshot(&self, snapshot: &ReconciliationSnapshot) -> AppResult<bool> {
        self.matches(snapshot)
    }

    pub(super) fn matches(&self, snapshot: &ReconciliationSnapshot) -> AppResult<bool> {
        let action = snapshot.action.request();
        let expected = Self::approve(
            self.id.clone(),
            self.version.clone(),
            self.provider.clone(),
            &action.contract,
            &action.target,
        )?;
        Ok(self.company == action.scope.company && self.operation == expected.operation)
    }
}

/// Only a registered verifier returns this observation. FinalNotApplied asserts that
/// EVERY covered request, including a zero-entry marker, is permanently quiescent.
/// Providers lacking that operation-specific contract must return Unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppliedEvidenceRequest {
    RemoteEntry(ActionRemoteEntryId),
    MarkerReservation,
    Unattributed,
}

pub enum VerifiedDisposition {
    Applied {
        recovered_result: Option<Value>,
        request: AppliedEvidenceRequest,
    },
    FinalNotApplied,
    Unknown,
}

/// Trusted adapter output, never decoded from command JSON or arbitrary notes.
pub struct EvidenceAttestation {
    pub disposition: VerifiedDisposition,
    pub authoritative_reference: EvidenceRecordReference,
    pub observed_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub diagnostic: Value,
}

#[async_trait]
pub trait ActionEvidenceVerifier: Send + Sync {
    fn registration(&self) -> &EvidenceVerifierRegistration;
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveEvidenceDisposition {
    Applied,
    FinalNotApplied,
    Unknown,
}

/// Service-issued settlement token. No public constructor, mutation or Deserialize.
/// Persistence still rechecks scope, coverage, registration and DB-clock validity.
pub struct VerifiedEvidence {
    request_digest: ReconciliationRequestDigest,
    coverage_digest: ActionCoverageDigest,
    registration: Option<EvidenceVerifierRegistration>,
    reference: Option<EvidenceRecordReference>,
    disposition: EffectiveEvidenceDisposition,
    observed_at: DateTime<Utc>,
    verified_at: DateTime<Utc>,
    valid_until: DateTime<Utc>,
    diagnostic: Value,
    recovered_result: Option<Value>,
    applied_request: Option<AppliedEvidenceRequest>,
}
impl VerifiedEvidence {
    pub(super) fn unknown(
        command: &ReconcileActionCommand,
        snapshot: &ReconciliationSnapshot,
    ) -> AppResult<Self> {
        let EvidenceInput::UnknownNote { note, claimed } = &command.input else {
            return Err(invalid());
        };
        Ok(Self {
            request_digest: command.request_digest()?,
            coverage_digest: snapshot.coverage_digest.clone(),
            registration: None,
            reference: None,
            disposition: EffectiveEvidenceDisposition::Unknown,
            observed_at: snapshot.database_now,
            verified_at: snapshot.database_now,
            valid_until: snapshot.database_now,
            diagnostic: json!({"version":1,"note":note,"claimed":claimed}),
            recovered_result: None,
            applied_request: None,
        })
    }

    pub(super) fn issue(
        command: &ReconcileActionCommand,
        snapshot: &ReconciliationSnapshot,
        registration: &EvidenceVerifierRegistration,
        attestation: EvidenceAttestation,
        verified_at: DateTime<Utc>,
    ) -> AppResult<Self> {
        let EvidenceInput::VerifiedReference {
            registration: requested,
            ..
        } = &command.input
        else {
            return Err(invalid());
        };
        if requested != registration.id()
            || !registration.matches(snapshot)?
            || verified_at < snapshot.database_now
            || verified_at > snapshot.database_now + chrono::Duration::seconds(5)
            || attestation.observed_at > verified_at
            || attestation.observed_at < snapshot.marker_created_at
            || snapshot
                .entries
                .iter()
                .any(|entry| entry.created_at > attestation.observed_at)
            || attestation.valid_until <= verified_at
            || attestation.valid_until > verified_at + chrono::Duration::hours(24)
        {
            return Err(invalid());
        }
        let applied_request = match &attestation.disposition {
            VerifiedDisposition::Applied { request, .. } => {
                match request {
                    AppliedEvidenceRequest::RemoteEntry(id)
                        if !snapshot.entries.iter().any(|entry| entry.id == *id) =>
                    {
                        return Err(invalid());
                    }
                    AppliedEvidenceRequest::MarkerReservation if !snapshot.entries.is_empty() => {
                        return Err(invalid());
                    }
                    _ => {}
                }
                Some(*request)
            }
            _ => None,
        };
        let diagnostic = super::super::freeze::canonical(
            &json!({"version":1,"registration":registration.id,
                "verifier_version":registration.version,"provider":registration.provider,
                "reference":attestation.authoritative_reference,"observed_at":attestation.observed_at,
                "verified_at":verified_at,"valid_until":attestation.valid_until,
                "diagnostic":attestation.diagnostic}),
            MAX_EVIDENCE_ENVELOPE_BYTES,
        )?;
        let (disposition, recovered_result, result_diagnostic) =
            classify(snapshot, attestation.disposition);
        let diagnostic = super::super::freeze::canonical(
            &json!({"attestation":diagnostic,"result":result_diagnostic}),
            MAX_EVIDENCE_ENVELOPE_BYTES,
        )?;
        Ok(Self {
            request_digest: command.request_digest()?,
            coverage_digest: snapshot.coverage_digest.clone(),
            registration: Some(registration.clone()),
            reference: Some(attestation.authoritative_reference),
            disposition,
            observed_at: attestation.observed_at,
            verified_at,
            valid_until: attestation.valid_until,
            diagnostic,
            recovered_result,
            applied_request,
        })
    }

    pub fn disposition(&self) -> EffectiveEvidenceDisposition {
        self.disposition
    }
    pub fn recovered_result(&self) -> Option<&Value> {
        self.recovered_result.as_ref()
    }
    pub fn applied_request(&self) -> Option<AppliedEvidenceRequest> {
        self.applied_request
    }
    pub fn request_digest(&self) -> &ReconciliationRequestDigest {
        &self.request_digest
    }
    pub fn coverage_digest(&self) -> &ActionCoverageDigest {
        &self.coverage_digest
    }
    pub fn registration(&self) -> Option<&EvidenceVerifierRegistration> {
        self.registration.as_ref()
    }
    pub fn authoritative_reference(&self) -> Option<&EvidenceRecordReference> {
        self.reference.as_ref()
    }
    pub fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }
    pub fn verified_at(&self) -> DateTime<Utc> {
        self.verified_at
    }
    pub fn valid_until(&self) -> DateTime<Utc> {
        self.valid_until
    }
    pub fn diagnostic(&self) -> &Value {
        &self.diagnostic
    }
}

fn classify(
    snapshot: &ReconciliationSnapshot,
    disposition: VerifiedDisposition,
) -> (
    EffectiveEvidenceDisposition,
    Option<Value>,
    Option<ReconciliationBlockedReason>,
) {
    match disposition {
        VerifiedDisposition::Applied {
            recovered_result, ..
        } => {
            let Some(result) = recovered_result else {
                return (
                    EffectiveEvidenceDisposition::Applied,
                    None,
                    Some(ReconciliationBlockedReason::MissingResult),
                );
            };
            match super::super::validate_action_result(&snapshot.action, result) {
                Ok(result) => (EffectiveEvidenceDisposition::Applied, Some(result), None),
                Err(_) => (
                    EffectiveEvidenceDisposition::Applied,
                    None,
                    Some(ReconciliationBlockedReason::InvalidRecoveredResult),
                ),
            }
        }
        VerifiedDisposition::FinalNotApplied => {
            (EffectiveEvidenceDisposition::FinalNotApplied, None, None)
        }
        VerifiedDisposition::Unknown => (EffectiveEvidenceDisposition::Unknown, None, None),
    }
}
