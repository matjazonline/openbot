use super::*;
use crate::application::app_error::{AppError, AppResult};

struct Issuance {
    command: ReconcileActionCommand,
    snapshot: ReconciliationSnapshot,
    registration: EvidenceVerifierRegistration,
}

impl Issuance {
    fn new() -> Self {
        let (command, snapshot, registration) = fixture();
        Self {
            command,
            snapshot,
            registration,
        }
    }

    fn issue(
        &self,
        attested: EvidenceAttestation,
        verified_at: chrono::DateTime<Utc>,
    ) -> AppResult<VerifiedEvidence> {
        VerifiedEvidence::issue(
            &self.command,
            &self.snapshot,
            &self.registration,
            attested,
            verified_at,
        )
    }

    fn cover_entry(&mut self) -> chrono::DateTime<Utc> {
        let created_at = self.snapshot.database_now - chrono::Duration::seconds(1);
        self.snapshot.entries.push(ReconciliationEntry {
            id: ActionRemoteEntryId::new(Uuid::new_v4()),
            attempt: ActionRemoteAttemptId::new(Uuid::new_v4()),
            generation: crate::application::workflow::lease::WorkflowGeneration(Uuid::new_v4()),
            worker: WorkerId::new(Uuid::new_v4()),
            created_at,
        });
        self.snapshot.coverage_digest = ReconciliationSnapshot::coverage(
            &self.snapshot.action,
            &self.snapshot.subject,
            self.snapshot.marker,
            self.snapshot.marker_created_at,
            &self.snapshot.entries,
        )
        .unwrap();
        self.snapshot.validate(&self.command).unwrap();
        created_at
    }
}

fn serialized_bytes(value: &serde_json::Value) -> usize {
    serde_json::to_vec(value).unwrap().len()
}

fn invalid_evidence(result: AppResult<VerifiedEvidence>) {
    let Err(AppError::BadRequest(message)) = result else {
        panic!("expected invalid-evidence BadRequest");
    };
    assert_eq!(message, "Invalid workflow reconciliation evidence");
}

#[test]
fn workflow_action_reconciliation_boundary_attestation_diagnostic_is_closed() {
    let diagnostic = AttestationDiagnostic::new(EvidenceProviderId::parse("fixture").unwrap());
    assert_eq!(
        serde_json::to_string(&diagnostic).unwrap(),
        r#"{"version":1,"ledger":"fixture"}"#
    );
    // Stored canonical bytes match the pre-typed fixture diagnostic.
    let stored = super::super::super::freeze::canonical(
        &serde_json::to_value(&diagnostic).unwrap(),
        MAX_EVIDENCE_ENVELOPE_BYTES,
    )
    .unwrap();
    assert_eq!(stored.to_string(), r#"{"ledger":"fixture","version":1}"#);
}

#[test]
fn workflow_action_reconciliation_boundary_max_envelope_literal_bytes() {
    // Every envelope field at its widest: 128-byte grammar ids, 9-digit fractional
    // timestamps and the longest reason classify can emit. Adding a field or widening
    // a grammar changes this literal; the limit itself is proven on canonical below.
    const MAX_ENVELOPE: usize = 954;
    const _: () = assert!(MAX_ENVELOPE <= 16_384);
    let (mut command, snapshot, _) = fixture();
    let widest = |prefix: &str| format!("{prefix}{}", "x".repeat(128 - prefix.len()));
    let registration = EvidenceVerifierRegistration::approve(
        EvidenceVerifierId::parse(widest("r")).unwrap(),
        EvidenceVerifierVersion::parse(widest("v")).unwrap(),
        EvidenceProviderId::parse(widest("p")).unwrap(),
        &snapshot.action.request().contract,
        &snapshot.action.request().target,
    )
    .unwrap();
    let reference = EvidenceRecordReference::parse(widest("e")).unwrap();
    command.input = EvidenceInput::VerifiedReference {
        registration: registration.id().clone(),
        reference: reference.clone(),
    };
    let overhead = serialized_bytes(&json!({"padding": ""}));
    let invalid_result = json!({"padding": "x".repeat(65_537 - overhead)});
    let mut attested = attestation(
        &snapshot,
        VerifiedDisposition::Applied {
            recovered_result: Some(invalid_result),
            request: AppliedEvidenceRequest::Unattributed,
        },
    );
    let verified_at = snapshot.database_now + chrono::Duration::nanoseconds(123_456_789);
    attested.authoritative_reference = reference;
    attested.observed_at = snapshot.database_now - chrono::Duration::nanoseconds(987_654_321);
    attested.valid_until = verified_at + chrono::Duration::hours(1);
    attested.diagnostic =
        AttestationDiagnostic::new(EvidenceProviderId::parse(widest("l")).unwrap());
    let token =
        VerifiedEvidence::issue(&command, &snapshot, &registration, attested, verified_at).unwrap();
    assert_eq!(
        token.diagnostic()["result"],
        json!(ReconciliationBlockedReason::InvalidRecoveredResult)
    );
    assert_eq!(serialized_bytes(token.diagnostic()), MAX_ENVELOPE);
}

#[test]
fn workflow_action_reconciliation_boundary_canonical_exact_bytes() {
    // Literal boundaries make a later limit increase fail this regression.
    assert_eq!(MAX_EVIDENCE_ENVELOPE_BYTES, 16_384);
    let overhead = serialized_bytes(&json!({"padding": ""}));
    let padded = |size: usize| json!({"padding": "x".repeat(size - overhead)});
    let canonical = |value: &serde_json::Value| {
        super::super::super::freeze::canonical(value, MAX_EVIDENCE_ENVELOPE_BYTES)
    };
    let exact = canonical(&padded(16_384)).unwrap();
    assert_eq!(serialized_bytes(&exact), 16_384);
    assert!(matches!(
        canonical(&padded(16_385)),
        Err(AppError::BadRequest(_))
    ));
}

#[test]
fn workflow_action_reconciliation_boundary_recovered_result_exact_bytes() {
    let issuance = Issuance::new();
    let overhead = serialized_bytes(&json!({"padding": ""}));
    for size in [65_536, 65_537] {
        // The existing unit fixture permits objects; this flat object therefore
        // isolates bytes from schema, depth, and node-count rejection.
        let result = json!({"padding": "x".repeat(size - overhead)});
        assert_eq!(serialized_bytes(&result), size);
        let token = issuance
            .issue(
                attestation(
                    &issuance.snapshot,
                    VerifiedDisposition::Applied {
                        recovered_result: Some(result.clone()),
                        request: AppliedEvidenceRequest::Unattributed,
                    },
                ),
                issuance.snapshot.database_now,
            )
            .unwrap();
        assert_eq!(token.disposition(), EffectiveEvidenceDisposition::Applied);
        assert!(serialized_bytes(token.diagnostic()) < 16_384);
        if size == 65_536 {
            assert_eq!(token.recovered_result(), Some(&result));
            assert_eq!(token.diagnostic()["result"], json!(null));
        } else {
            assert!(token.recovered_result().is_none());
            assert_eq!(
                token.diagnostic()["result"],
                json!(ReconciliationBlockedReason::InvalidRecoveredResult)
            );
        }
    }
}

#[test]
fn workflow_action_reconciliation_boundary_observation_covers_entry_time() {
    let mut issuance = Issuance::new();
    let entry_time = issuance.cover_entry();
    assert!(issuance.snapshot.marker_created_at < entry_time);
    let mut before_entry = attestation(&issuance.snapshot, VerifiedDisposition::FinalNotApplied);
    before_entry.observed_at = issuance.snapshot.marker_created_at;
    invalid_evidence(issuance.issue(before_entry, issuance.snapshot.database_now));

    let mut at_entry = attestation(&issuance.snapshot, VerifiedDisposition::FinalNotApplied);
    at_entry.observed_at = entry_time;
    let token = issuance
        .issue(at_entry, issuance.snapshot.database_now)
        .unwrap();
    assert_eq!(token.observed_at(), entry_time);
    assert_eq!(
        token.disposition(),
        EffectiveEvidenceDisposition::FinalNotApplied
    );
    assert_eq!(token.coverage_digest(), &issuance.snapshot.coverage_digest);
}

#[test]
fn workflow_action_reconciliation_boundary_validity_and_verification_window() {
    let issuance = Issuance::new();
    let now = issuance.snapshot.database_now;
    let nano = chrono::Duration::nanoseconds(1);
    for validity in [nano, chrono::Duration::hours(24)] {
        let mut attested = attestation(&issuance.snapshot, VerifiedDisposition::FinalNotApplied);
        attested.valid_until = now + validity;
        let token = issuance.issue(attested, now).unwrap();
        assert_eq!(token.valid_until(), now + validity);
    }
    for validity in [chrono::Duration::zero(), chrono::Duration::hours(24) + nano] {
        let mut attested = attestation(&issuance.snapshot, VerifiedDisposition::FinalNotApplied);
        attested.valid_until = now + validity;
        invalid_evidence(issuance.issue(attested, now));
    }
    let at_window = now + chrono::Duration::seconds(5);
    let token = issuance
        .issue(
            attestation(&issuance.snapshot, VerifiedDisposition::FinalNotApplied),
            at_window,
        )
        .unwrap();
    assert_eq!(token.verified_at(), at_window);
    invalid_evidence(issuance.issue(
        attestation(&issuance.snapshot, VerifiedDisposition::FinalNotApplied),
        at_window + nano,
    ));
}
