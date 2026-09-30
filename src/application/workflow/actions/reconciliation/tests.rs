use super::super::{ActionScope, ApprovalSubject, FrozenAction};
use super::*;
use crate::application::workflow::{IdempotencyKey, RunRevision, WorkerId, WorkflowActor};
use crate::domain::workflow::*;
use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

#[path = "boundary_tests.rs"]
mod boundary_tests;

pub(super) fn fixture() -> (
    ReconcileActionCommand,
    ReconciliationSnapshot,
    EvidenceVerifierRegistration,
) {
    let scope = ActionScope {
        company: CompanyId::new(Uuid::new_v4()),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
    };
    let action =
        FrozenAction::restore(serde_json::to_value(super::super::tests::request(scope)).unwrap())
            .unwrap();
    let subject = ApprovalSubject {
        invocation: ActionInvocationId::new(Uuid::new_v4()),
        argument_digest: action.argument_digest().clone(),
    };
    let registration = EvidenceVerifierRegistration::approve(
        EvidenceVerifierId::parse("fixture.finality").unwrap(),
        EvidenceVerifierVersion::parse("v1").unwrap(),
        EvidenceProviderId::parse("fixture.ledger").unwrap(),
        &action.request().contract,
        &action.request().target,
    )
    .unwrap();
    let command = ReconcileActionCommand {
        actor: WorkflowActor::authenticated(Uuid::new_v4()).unwrap(),
        scope,
        subject: subject.clone(),
        marker: ActionRemoteMarkerId::new(Uuid::new_v4()),
        expected_revision: RunRevision(7),
        command_key: IdempotencyKey::parse("reconcile").unwrap(),
        input: EvidenceInput::VerifiedReference {
            registration: registration.id().clone(),
            reference: EvidenceRecordReference::parse("ledger/request/1").unwrap(),
        },
    };
    let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
    let marker_created_at = now - chrono::Duration::seconds(2);
    let coverage_digest =
        ReconciliationSnapshot::coverage(&action, &subject, command.marker, marker_created_at, &[])
            .unwrap();
    let snapshot = ReconciliationSnapshot {
        action,
        subject,
        marker: command.marker,
        marker_created_at,
        entries: vec![],
        revision: command.expected_revision,
        database_now: now,
        coverage_digest,
    };
    (command, snapshot, registration)
}

fn attestation(
    snapshot: &ReconciliationSnapshot,
    disposition: VerifiedDisposition,
) -> EvidenceAttestation {
    EvidenceAttestation {
        disposition,
        authoritative_reference: EvidenceRecordReference::parse("record/1").unwrap(),
        observed_at: snapshot.database_now,
        valid_until: snapshot.database_now + chrono::Duration::hours(1),
        diagnostic: json!({}),
    }
}

#[test]
fn workflow_action_reconciliation_input_cannot_carry_authority_and_bounds_are_bytes() {
    for bad in ["", " leading", "secret?token", "é", &"x".repeat(129)] {
        assert!(EvidenceRecordReference::parse(bad).is_err());
        assert!(EvidenceVerifierId::parse(bad).is_err());
    }
    let note = |note| EvidenceInput::UnknownNote {
        note,
        claimed: ClaimedDisposition::NotApplied,
    };
    assert!(note("é".repeat(2048)).validate().is_ok());
    assert!(note("é".repeat(2049)).validate().is_err());
    assert!(
        serde_json::from_value::<EvidenceInput>(json!({"kind":"verified_reference",
        "registration":"fixture","reference":"record/1","verdict":"not_applied"}))
        .is_err()
    );
    assert!(
        serde_json::from_value::<EvidenceInput>(json!({"kind":"unknown_note",
        "note":"operator claims absence","claimed":"not_applied","quiescent":true}))
        .is_err()
    );
}

#[test]
fn workflow_action_reconciliation_request_identity_binds_every_command_component() {
    let (command, _, _) = fixture();
    let original = command.request_digest().unwrap();
    assert_eq!(command.clone().request_digest().unwrap(), original);
    for variant in 0..11 {
        let mut changed = command.clone();
        match variant {
            0 => changed.actor = WorkflowActor::authenticated(Uuid::new_v4()).unwrap(),
            1 => changed.scope.company = CompanyId::new(Uuid::new_v4()),
            2 => changed.scope.run = RunId::new(Uuid::new_v4()),
            3 => changed.scope.execution = ExecutionId::new(Uuid::new_v4()),
            4 => changed.subject.invocation = ActionInvocationId::new(Uuid::new_v4()),
            5 => {
                changed.subject.argument_digest =
                    serde_json::from_value(json!("f".repeat(64))).unwrap()
            }
            6 => changed.marker = ActionRemoteMarkerId::new(Uuid::new_v4()),
            7 => changed.expected_revision = RunRevision(8),
            8 => changed.command_key = IdempotencyKey::parse("different").unwrap(),
            9 => {
                changed.input = EvidenceInput::VerifiedReference {
                    registration: EvidenceVerifierId::parse("other").unwrap(),
                    reference: EvidenceRecordReference::parse("ledger/request/1").unwrap(),
                }
            }
            _ => {
                changed.input = EvidenceInput::UnknownNote {
                    note: "claimed absent".into(),
                    claimed: ClaimedDisposition::NotApplied,
                }
            }
        }
        assert_ne!(
            changed.request_digest().unwrap(),
            original,
            "variant{variant}"
        );
    }
}

#[test]
fn workflow_action_reconciliation_unknown_claim_is_never_final_absence() {
    let (mut command, snapshot, _) = fixture();
    for claimed in [
        ClaimedDisposition::Applied,
        ClaimedDisposition::NotApplied,
        ClaimedDisposition::Unknown,
    ] {
        command.input = EvidenceInput::UnknownNote {
            note: "manual operator note".into(),
            claimed,
        };
        let token = VerifiedEvidence::unknown(&command, &snapshot).unwrap();
        assert_eq!(token.disposition(), EffectiveEvidenceDisposition::Unknown);
        assert!(token.registration().is_none());
        assert!(token.recovered_result().is_none());
    }
}

#[test]
fn workflow_action_reconciliation_coverage_binds_marker_and_all_request_provenance() {
    let (command, mut snapshot, _) = fixture();
    snapshot.validate(&command).unwrap();
    let empty = snapshot.coverage_digest.clone();
    let entry = ReconciliationEntry {
        id: ActionRemoteEntryId::new(Uuid::new_v4()),
        attempt: ActionRemoteAttemptId::new(Uuid::new_v4()),
        generation: crate::application::workflow::lease::WorkflowGeneration(Uuid::new_v4()),
        worker: WorkerId::new(Uuid::new_v4()),
        created_at: snapshot.database_now,
    };
    let cover = |entries: &[ReconciliationEntry]| {
        ReconciliationSnapshot::coverage(
            &snapshot.action,
            &snapshot.subject,
            snapshot.marker,
            snapshot.marker_created_at,
            entries,
        )
    };
    let original = cover(std::slice::from_ref(&entry)).unwrap();
    assert_ne!(original, empty);
    for variant in 0..4 {
        let mut changed = entry.clone();
        match variant {
            0 => changed.attempt = ActionRemoteAttemptId::new(Uuid::new_v4()),
            1 => changed.worker = WorkerId::new(Uuid::new_v4()),
            2 => {
                changed.generation =
                    crate::application::workflow::lease::WorkflowGeneration(Uuid::new_v4())
            }
            _ => changed.created_at -= chrono::Duration::milliseconds(1),
        }
        assert_ne!(cover(&[changed]).unwrap(), original);
    }
    assert!(cover(&[entry.clone(), entry.clone()]).is_err());
    let mut distinct = (0..129)
        .map(|_| {
            let mut next = entry.clone();
            next.id = ActionRemoteEntryId::new(Uuid::new_v4());
            next
        })
        .collect::<Vec<_>>();
    distinct.sort_by_key(|item| item.id);
    assert!(cover(&distinct[..128]).is_ok());
    assert!(cover(&distinct).is_err());
    snapshot.entries.push(entry);
    assert!(
        snapshot.validate(&command).is_err(),
        "subset digest cannot cover a later entry"
    );
}

#[test]
fn workflow_action_reconciliation_verifier_binding_and_times_fail_closed() {
    let (command, snapshot, registration) = fixture();
    for variant in 0..9 {
        let mut registered = registration.clone();
        let mut attested = attestation(&snapshot, VerifiedDisposition::FinalNotApplied);
        let mut verified_at = snapshot.database_now;
        match variant {
            0..=2 => {
                let mut tool = snapshot.action.request().contract.clone();
                let id = if variant == 1 {
                    EvidenceVerifierId::parse("other").unwrap()
                } else {
                    registration.id().clone()
                };
                if variant == 0 {
                    tool.company_id = CompanyId::new(Uuid::new_v4());
                }
                if variant == 2 {
                    tool.policy.policy_revision += 1;
                }
                registered = EvidenceVerifierRegistration::approve(
                    id,
                    EvidenceVerifierVersion::parse("v1").unwrap(),
                    EvidenceProviderId::parse("fixture.ledger").unwrap(),
                    &tool,
                    &snapshot.action.request().target,
                )
                .unwrap();
            }
            3 => attested.observed_at += chrono::Duration::seconds(1),
            4 => attested.observed_at = snapshot.marker_created_at - chrono::Duration::seconds(1),
            5 => attested.valid_until = verified_at,
            6 => {
                attested.valid_until =
                    verified_at + chrono::Duration::hours(24) + chrono::Duration::seconds(1)
            }
            7 => verified_at += chrono::Duration::seconds(6),
            _ => attested.diagnostic = json!({"oversized":"x".repeat(MAX_EVIDENCE_ENVELOPE_BYTES)}),
        }
        assert!(
            VerifiedEvidence::issue(&command, &snapshot, &registered, attested, verified_at)
                .is_err(),
            "variant{variant}"
        );
    }
    let proof = VerifiedEvidence::issue(
        &command,
        &snapshot,
        &registration,
        attestation(&snapshot, VerifiedDisposition::FinalNotApplied),
        snapshot.database_now,
    )
    .unwrap();
    assert_eq!(
        proof.disposition(),
        EffectiveEvidenceDisposition::FinalNotApplied
    );
    assert!(proof.recovered_result().is_none());
}

#[test]
fn workflow_action_reconciliation_applied_result_validation_preserves_positive_truth() {
    let (command, snapshot, registration) = fixture();
    for result in [
        None,
        Some(json!(42)),
        Some(json!({"data":"x".repeat(65_537)})),
    ] {
        let token = VerifiedEvidence::issue(
            &command,
            &snapshot,
            &registration,
            attestation(
                &snapshot,
                VerifiedDisposition::Applied {
                    recovered_result: result,
                    request: AppliedEvidenceRequest::Unattributed,
                },
            ),
            snapshot.database_now,
        )
        .unwrap();
        assert_eq!(token.disposition(), EffectiveEvidenceDisposition::Applied);
        assert!(token.recovered_result().is_none());
        assert!(token.diagnostic()["result"].is_string());
    }
    let token = VerifiedEvidence::issue(
        &command,
        &snapshot,
        &registration,
        attestation(
            &snapshot,
            VerifiedDisposition::Applied {
                recovered_result: Some(json!({"written":true})),
                request: AppliedEvidenceRequest::Unattributed,
            },
        ),
        snapshot.database_now,
    )
    .unwrap();
    assert_eq!(token.recovered_result(), Some(&json!({"written":true})));
    assert_eq!(token.disposition(), EffectiveEvidenceDisposition::Applied);
}

#[test]
fn workflow_action_reconciliation_applied_request_must_belong_to_exact_snapshot() {
    let (command, mut snapshot, registration) = fixture();
    snapshot.entries.push(ReconciliationEntry {
        id: ActionRemoteEntryId::new(Uuid::new_v4()),
        attempt: ActionRemoteAttemptId::new(Uuid::new_v4()),
        generation: crate::application::workflow::lease::WorkflowGeneration(Uuid::new_v4()),
        worker: WorkerId::new(Uuid::new_v4()),
        created_at: snapshot.database_now - chrono::Duration::seconds(1),
    });
    snapshot.coverage_digest = ReconciliationSnapshot::coverage(
        &snapshot.action,
        &snapshot.subject,
        snapshot.marker,
        snapshot.marker_created_at,
        &snapshot.entries,
    )
    .unwrap();
    let covered = snapshot.entries[0].id;
    for (request, allowed) in [
        (AppliedEvidenceRequest::RemoteEntry(covered), true),
        (
            AppliedEvidenceRequest::RemoteEntry(ActionRemoteEntryId::new(Uuid::new_v4())),
            false,
        ),
        (AppliedEvidenceRequest::MarkerReservation, false),
        (AppliedEvidenceRequest::Unattributed, true),
    ] {
        let token = VerifiedEvidence::issue(
            &command,
            &snapshot,
            &registration,
            attestation(
                &snapshot,
                VerifiedDisposition::Applied {
                    recovered_result: Some(json!({"written":true})),
                    request,
                },
            ),
            snapshot.database_now,
        );
        assert_eq!(token.is_ok(), allowed);
        if let Ok(token) = token {
            assert_eq!(token.applied_request(), Some(request));
        }
    }
    snapshot.entries.clear();
    snapshot.coverage_digest = ReconciliationSnapshot::coverage(
        &snapshot.action,
        &snapshot.subject,
        snapshot.marker,
        snapshot.marker_created_at,
        &snapshot.entries,
    )
    .unwrap();
    let token = VerifiedEvidence::issue(
        &command,
        &snapshot,
        &registration,
        attestation(
            &snapshot,
            VerifiedDisposition::Applied {
                recovered_result: None,
                request: AppliedEvidenceRequest::MarkerReservation,
            },
        ),
        snapshot.database_now,
    )
    .unwrap();
    assert_eq!(
        token.applied_request(),
        Some(AppliedEvidenceRequest::MarkerReservation)
    );
}
