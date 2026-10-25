//! 04.11 message delivery reconciliation through the real 04.7 service with a test-host,
//! database-backed ledger verifier (D3 §H, §K; D4 §M-§S). No production verifier exists.
use super::*;
use crate::adapters::persistence::workflow::action_reconciliation::PostgresActionReconciliation;
use crate::adapters::persistence::workflow::message::MessageActionAuthority;
use crate::application::workflow::LifecycleAuthorizer;
use crate::application::workflow::actions::*;
use crate::application::workflow::message::{MessageKind, builtin_message_snapshot};
use crate::domain::workflow::*;
use crate::entities::transport::{DeliveryStatus, FailureClass};
use crate::transport::{DeliveryOutcome, DeliveryQueue, PartResult, ProviderSendOutcome};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[path = "message_reconcile_race_tests.rs"]
mod race_tests;
#[path = "message_reconcile_veto_tests.rs"]
mod veto_tests;

/// The test host's provider ledger: what a queryable email provider would say about one
/// Message-ID to one recipient with one body digest. `mutate` runs inside verification, to
/// change the subject between snapshot and settlement.
const LEDGER: &str = "CREATE TABLE IF NOT EXISTS fixture_message_ledger (message_id text, recipient text, \
    content_digest text, disposition text NOT NULL, accepted_at timestamptz, valid_seconds float8 NOT NULL DEFAULT 60, \
    mutate text, PRIMARY KEY (message_id, recipient))";

struct Ledger {
    registration: EvidenceVerifierRegistration,
    pool: sqlx::PgPool,
}
#[async_trait]
impl ActionEvidenceVerifier for Ledger {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        &self.registration
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        _: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        let subject = snapshot
            .delivery
            .as_ref()
            .expect("a message delivery subject");
        let row: Option<(String, Option<DateTime<Utc>>, f64, Option<String>)> = sqlx::query_as(
            "SELECT disposition, accepted_at, valid_seconds, mutate FROM fixture_message_ledger \
             WHERE message_id=$1 AND recipient=$2 AND content_digest=$3",
        )
        .bind(&subject.message_id)
        .bind(&subject.recipient)
        .bind(&subject.content_digest)
        .fetch_optional(&self.pool)
        .await?;
        let (disposition, accepted_at, valid, mutate) =
            row.unwrap_or(("unknown".into(), None, 60.0, None));
        if let Some(mutate) = mutate {
            sqlx::raw_sql(&mutate).execute(&self.pool).await?;
        }
        Ok(EvidenceAttestation {
            disposition: match disposition.as_str() {
                "applied" => VerifiedDisposition::Applied {
                    recovered_result: None,
                    request: AppliedEvidenceRequest::Delivery {
                        accepted_at: accepted_at.expect("an acceptance time"),
                    },
                },
                "not_applied" => VerifiedDisposition::FinalNotApplied,
                _ => VerifiedDisposition::Unknown,
            },
            authoritative_reference: reference.clone(),
            observed_at: snapshot.database_now,
            valid_until: snapshot.database_now
                + chrono::Duration::milliseconds((valid * 1000.0) as i64),
            diagnostic: AttestationDiagnostic::new(EvidenceProviderId::parse("email").unwrap()),
        })
    }
}

fn registration(company: CompanyId) -> EvidenceVerifierRegistration {
    EvidenceVerifierRegistration::approve(
        EvidenceVerifierId::parse("fixture.ledger").unwrap(),
        EvidenceVerifierVersion::parse("v1").unwrap(),
        EvidenceProviderId::parse("email").unwrap(),
        &builtin_message_snapshot(company, MessageKind::Send).unwrap(),
        &ActionTarget::Local {
            resource: company.as_uuid(),
            resource_kind: TypeName::parse("messaging").unwrap(),
        },
    )
    .unwrap()
}

type Service = ActionReconciliationService<
    PostgresActionReconciliation<MessageActionAuthority>,
    LifecycleAuthorizer<PostgresPersistence, PostgresPersistence, PostgresPersistence>,
    PostgresPersistence,
>;
fn service(setup: &Setup, verifier: bool) -> Service {
    let p = &setup.fixture.persistence;
    ActionReconciliationService::new(
        PostgresActionReconciliation::new(p.clone(), MessageActionAuthority),
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        verifier.then(|| {
            Arc::new(Ledger {
                registration: registration(setup.fixture.target.company),
                pool: p.pool().clone(),
            }) as Arc<dyn ActionEvidenceVerifier>
        }),
    )
}

#[derive(sqlx::FromRow)]
struct Subject {
    invocation_id: Uuid,
    argument_digest: String,
    dispatch_id: Uuid,
    ordinal: i16,
    message_id: String,
    recipient_address: String,
    content_digest: String,
}
async fn subject(setup: &Setup, delivery: Uuid) -> Subject {
    sqlx::query_as(
        "SELECT action.invocation_id, action.argument_digest, action.dispatch_id, target.ordinal, metadata.rfc_message_id AS message_id, \
                target.recipient_address, part.content_digest \
           FROM workflow_message_destinations AS target \
           JOIN workflow_message_actions AS action ON action.company_id=target.company_id AND action.invocation_id=target.invocation_id \
           JOIN email_message_metadata AS metadata ON metadata.message_id=target.message_id \
           JOIN message_delivery_parts AS part ON part.delivery_id=target.delivery_id WHERE target.delivery_id=$1",
    )
    .bind(delivery)
    .fetch_one(setup.pool())
    .await
    .unwrap()
}

/// Record what the provider ledger says about `delivery`.
async fn ledger(
    setup: &Setup,
    delivery: Uuid,
    disposition: &str,
    accepted_at: Option<DateTime<Utc>>,
    valid: f64,
    mutate: Option<String>,
) {
    sqlx::raw_sql(LEDGER).execute(setup.pool()).await.unwrap();
    let subject = subject(setup, delivery).await;
    sqlx::query("INSERT INTO fixture_message_ledger VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (message_id, recipient) DO UPDATE SET disposition=EXCLUDED.disposition, accepted_at=EXCLUDED.accepted_at, valid_seconds=EXCLUDED.valid_seconds, mutate=EXCLUDED.mutate")
        .bind(&subject.message_id).bind(&subject.recipient_address).bind(&subject.content_digest)
        .bind(disposition).bind(accepted_at).bind(valid).bind(mutate)
        .execute(setup.pool()).await.unwrap();
}

async fn command(
    setup: &Setup,
    delivery: Uuid,
    key: &str,
    input: EvidenceInput,
) -> ReconcileActionCommand {
    command_as(setup, delivery, key, input, setup.fixture.target.actor).await
}
async fn command_as(
    setup: &Setup,
    delivery: Uuid,
    key: &str,
    input: EvidenceInput,
    actor: WorkflowActor,
) -> ReconcileActionCommand {
    let subject = subject(setup, delivery).await;
    let scope = setup.scope();
    let p = &setup.fixture.persistence;
    ReconcileActionCommand {
        actor,
        scope: ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
        subject: ApprovalSubject {
            invocation: ActionInvocationId::new(subject.invocation_id),
            argument_digest: serde_json::from_value(json!(subject.argument_digest)).unwrap(),
        },
        marker: ActionRemoteMarkerId::new(subject.dispatch_id),
        target: ReconciliationTarget::MessageDelivery {
            ordinal: u8::try_from(subject.ordinal).unwrap(),
            delivery,
        },
        expected_revision: p
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
        command_key: IdempotencyKey::parse(key).unwrap(),
        input,
    }
}
fn verified() -> EvidenceInput {
    EvidenceInput::VerifiedReference {
        registration: EvidenceVerifierId::parse("fixture.ledger").unwrap(),
        reference: EvidenceRecordReference::parse("ledger/delivery/1").unwrap(),
    }
}
async fn reconcile(
    service: &Service,
    command: &ReconcileActionCommand,
) -> AppResult<ReconciliationResult> {
    Box::pin(service.reconcile(command, &CancellationToken::new(), Duration::from_secs(5))).await
}

/// The delivery engine's own path to an ambiguous request: claim, begin, lost response.
async fn send_unknown(setup: &Setup, delivery: Uuid) {
    let p = &setup.fixture.persistence;
    let claimed = claim(p, delivery).await;
    assert!(
        p.begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap()
            .lease_held()
    );
    let outcome = p
        .complete_part(PartResult {
            fence: &claimed.lease,
            part_id: claimed.parts[0].id,
            outcome: &ProviderSendOutcome::OutcomeUnknown {
                class: FailureClass::Timeout,
                detail: crate::transport::FailureDetail::parse("the response was lost").unwrap(),
            },
        })
        .await
        .unwrap();
    assert_eq!(
        outcome,
        DeliveryOutcome::Applied(DeliveryStatus::OutcomeUnknown)
    );
}

/// A parked step with an accepted owner email and an unknown member email, consumed: uncertain.
async fn uncertain() -> (Setup, Uuid, Uuid) {
    let setup = Box::pin(send(&[To::Owner, To::User])).await;
    parked(&setup).await;
    let run = setup.scope().run;
    let (accepted, unknown) = (
        delivery(setup.pool(), run, 0).await,
        delivery(setup.pool(), run, 1).await,
    );
    deliver(setup.pool(), accepted, "delivered").await;
    send_unknown(&setup, unknown).await;
    let p = &setup.fixture.persistence;
    assert_eq!(
        p.consume_message_receipts(setup.scope(), lease())
            .await
            .unwrap(),
        1
    );
    assert_eq!(action_state(setup.pool(), run).await, "uncertain");
    (setup, accepted, unknown)
}

/// D3 §G row 15: applied evidence delivers the subject; the consumer derives the receipt and
/// the step resumes only once every destination is accepted.
#[tokio::test]
async fn workflow_message_reconcile_applied_on_a_live_run_resumes_after_every_destination() {
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    let (p, scope) = (&setup.fixture.persistence, setup.scope());
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Pending
    ));
    ledger(
        &setup,
        unknown,
        "applied",
        Some(Utc::now() - chrono::Duration::milliseconds(1)),
        60.0,
        None,
    )
    .await;
    let command = command(&setup, unknown, "applied-live", verified()).await;
    let result = reconcile(&service(&setup, true), &command).await.unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: false }
    );
    let row = delivery_row(setup.pool(), unknown).await;
    assert_eq!(
        (row.status.as_str(), row.part.as_str()),
        ("delivered", "delivered")
    );
    // Settlement wrote no receipt and completed nothing: the consumer and the wait own that.
    assert_eq!(rows(setup.pool(), scope.run).await.receipts, 0);
    assert_eq!(
        state(setup.pool(), scope).await.wait.as_deref(),
        Some("waiting")
    );
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), scope.run).await, "accepted");
    let WaitProgress::Completed(step) = p.resume_wait(scope).await.unwrap() else {
        panic!("every destination accepted resumes the step");
    };
    assert_eq!(step.output["status"], "accepted");
    assert_eq!(state(setup.pool(), scope).await.run, "succeeded");
    // Exact replay; different bytes under the same key conflict.
    let replay = reconcile(&service(&setup, true), &command).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.outcome, result.outcome);
    let mut other = command.clone();
    other.input = EvidenceInput::UnknownNote {
        note: "different".into(),
        claimed: ClaimedDisposition::Unknown,
    };
    assert_eq!(
        reconcile(&service(&setup, true), &other)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::IdempotencyConflict
    );
}

/// Row 18: applied truth on a cancelled run is recorded with no successor.
#[tokio::test]
async fn workflow_message_reconcile_applied_on_a_terminal_run_records_truth_only() {
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    let (p, scope) = (&setup.fixture.persistence, setup.scope());
    cancel(&setup, "cancel-before-truth").await;
    assert_eq!(
        delivery_row(setup.pool(), unknown)
            .await
            .cancellation_reason
            .as_deref(),
        Some("workflow_cancelled")
    );
    ledger(
        &setup,
        unknown,
        "applied",
        Some(Utc::now() - chrono::Duration::milliseconds(1)),
        60.0,
        None,
    )
    .await;
    let command = command(&setup, unknown, "applied-terminal", verified()).await;
    assert_eq!(
        reconcile(&service(&setup, true), &command)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: false }
    );
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), scope.run).await, "accepted");
    let after = state(setup.pool(), scope).await;
    assert_eq!(
        (after.run.as_str(), after.wait.as_deref()),
        ("cancelled", Some("cancelled"))
    );
    let executions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE run_id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert_eq!(executions, 1);
}

/// Row 16: final not-applied reopens only the subject; the engine resends it with the same
/// key and Message-ID, and the accepted sibling is untouched.
#[tokio::test]
async fn workflow_message_reconcile_not_applied_reopens_only_the_subject() {
    let (setup, accepted, unknown) = Box::pin(uncertain()).await;
    let (p, scope) = (&setup.fixture.persistence, setup.scope());
    let sibling_before = delivery_row(setup.pool(), accepted).await;
    let key_before: (String, String) = sqlx::query_as("SELECT delivery.idempotency_key, part.part_key FROM message_deliveries AS delivery JOIN message_delivery_parts AS part ON part.delivery_id=delivery.id WHERE delivery.id=$1")
        .bind(unknown).fetch_one(setup.pool()).await.unwrap();
    ledger(&setup, unknown, "not_applied", None, 60.0, None).await;
    let command = command(&setup, unknown, "not-applied-live", verified()).await;
    let result = reconcile(&service(&setup, true), &command).await.unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::DeliveryReopened);
    let reopened = delivery_row(setup.pool(), unknown).await;
    assert_eq!(
        (
            reopened.status.as_str(),
            reopened.part.as_str(),
            reopened.last_error_class.as_deref()
        ),
        ("retryable", "retryable", Some("reconciled_not_applied"))
    );
    assert_eq!(delivery_row(setup.pool(), accepted).await, sibling_before);
    // Never a workflow job reopen: the step stays parked in its effect wait.
    let parked_state = state(setup.pool(), scope).await;
    assert_eq!(
        (parked_state.run.as_str(), parked_state.job.as_str()),
        ("waiting", "completed")
    );
    let scheduled: Option<Uuid> = sqlx::query_scalar("SELECT scheduled_job_id FROM workflow_action_evidence_commands WHERE command_key='not-applied-live'")
        .fetch_one(setup.pool()).await.unwrap();
    assert!(scheduled.is_none());
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), scope.run).await, "pending");
    let invocation = invocations(setup.pool(), scope.run).await[0];
    assert_eq!(
        effect_state(setup.pool(), invocation).await.0,
        "possible_dispatch"
    );
    // A proof reopens at most once.
    sqlx::query("INSERT INTO workflow_message_delivery_reopens(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,evidence_id,ordinal,delivery_id,attempt_count,valid_until) \
                 SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,gen_random_uuid(),evidence_id,ordinal,delivery_id,attempt_count,valid_until FROM workflow_message_delivery_reopens")
        .execute(setup.pool()).await.unwrap_err();
    // The resend: same idempotency key and part key, so the same Message-ID.
    deliver(setup.pool(), unknown, "delivered").await;
    let key_after: (String, String) = sqlx::query_as("SELECT delivery.idempotency_key, part.part_key FROM message_deliveries AS delivery JOIN message_delivery_parts AS part ON part.delivery_id=delivery.id WHERE delivery.id=$1")
        .bind(unknown).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(key_after, key_before);
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), scope.run).await, "accepted");
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Completed(_)
    ));
}

/// Rows 17 and 18: exhausted attempts dead-letter the subject; a cancelled or expired run
/// supersedes it. No provider call either way.
#[tokio::test]
async fn workflow_message_reconcile_not_applied_exhausted_or_ended_never_resends() {
    for case in ["exhausted", "cancelled", "expired"] {
        let (setup, _, unknown) = Box::pin(uncertain()).await;
        let (p, scope) = (&setup.fixture.persistence, setup.scope());
        match case {
            "exhausted" => {
                sqlx::query("UPDATE message_deliveries SET max_attempts=attempt_count WHERE id=$1")
                    .bind(unknown)
                    .execute(setup.pool())
                    .await
                    .unwrap();
            }
            "cancelled" => cancel(&setup, "cancel-before-proof").await,
            _ => {
                sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp() WHERE id=$1")
                    .bind(scope.run.as_uuid())
                    .execute(setup.pool())
                    .await
                    .unwrap();
            }
        }
        ledger(&setup, unknown, "not_applied", None, 60.0, None).await;
        let command = command(&setup, unknown, &format!("not-applied-{case}"), verified()).await;
        assert_eq!(
            reconcile(&service(&setup, true), &command)
                .await
                .unwrap()
                .outcome,
            ReconciliationOutcome::NotAppliedRecorded,
            "{case}"
        );
        let row = delivery_row(setup.pool(), unknown).await;
        let (class, reason) = match case {
            "exhausted" => ("not_applied_exhausted", None),
            "cancelled" => ("superseded", Some("workflow_cancelled")),
            _ => ("superseded", Some("workflow_expired")),
        };
        assert_eq!(
            (
                row.status.as_str(),
                row.part.as_str(),
                row.last_error_class.as_deref(),
                row.cancellation_reason.as_deref(),
                row.attempt_count
            ),
            ("dead_letter", "dead", Some(class), reason, 1),
            "{case}"
        );
        assert_eq!(
            p.consume_message_receipts(scope, lease()).await.unwrap(),
            1,
            "{case}"
        );
        assert_eq!(
            action_state(setup.pool(), scope.run).await,
            "failed",
            "{case}"
        );
    }
}

/// Row 19: an unknown note changes nothing; a reference without an installed verifier, a
/// non-admin and a foreign actor are refused before any write.
#[tokio::test]
async fn workflow_message_reconcile_unknown_and_refusals_change_nothing() {
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    let note = EvidenceInput::UnknownNote {
        note: "the provider has no ledger".into(),
        claimed: ClaimedDisposition::NotApplied,
    };
    let noted = command(&setup, unknown, "unknown-note", note).await;
    assert_eq!(
        reconcile(&service(&setup, false), &noted)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::UnknownRecorded
    );
    assert_eq!(
        delivery_row(setup.pool(), unknown).await.status,
        "outcome_unknown"
    );
    let evidence = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workflow_action_evidence")
            .fetch_one(setup.pool())
            .await
            .unwrap()
    };
    let before = evidence().await;
    ledger(&setup, unknown, "not_applied", None, 60.0, None).await;
    let without = command(&setup, unknown, "no-verifier", verified()).await;
    assert!(reconcile(&service(&setup, false), &without).await.is_err());
    let member = WorkflowActor::authenticated(member(&setup.fixture, "member").await).unwrap();
    let plain = command_as(&setup, unknown, "plain-member", verified(), member).await;
    assert!(reconcile(&service(&setup, true), &plain).await.is_err());
    let foreign = Fixture::in_database(setup.fixture._db.clone(), "-foreign").await;
    let outsider = command_as(
        &setup,
        unknown,
        "outsider",
        verified(),
        foreign.target.actor,
    )
    .await;
    assert!(reconcile(&service(&setup, true), &outsider).await.is_err());
    // A remote target on a message marker is a missing subject.
    let mut remote = command(&setup, unknown, "wrong-target", verified()).await;
    remote.target = ReconciliationTarget::Remote;
    assert!(reconcile(&service(&setup, true), &remote).await.is_err());
    assert_eq!(evidence().await, before);
    assert_eq!(
        delivery_row(setup.pool(), unknown).await.status,
        "outcome_unknown"
    );
}

/// Row 19: a subject that changed between snapshot and settlement is refused with no grant.
#[tokio::test]
async fn workflow_message_reconcile_stale_snapshot_is_refused_without_a_grant() {
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    let mutate = format!(
        "UPDATE message_delivery_parts SET request_started_at=request_started_at+interval '1 millisecond' WHERE delivery_id='{unknown}'"
    );
    ledger(&setup, unknown, "not_applied", None, 60.0, Some(mutate)).await;
    let command = command(&setup, unknown, "stale", verified()).await;
    assert_eq!(
        reconcile(&service(&setup, true), &command)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::StaleSnapshot
        }
    );
    let reopens: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_message_delivery_reopens")
        .fetch_one(setup.pool())
        .await
        .unwrap();
    assert_eq!(reopens, 0);
    assert_eq!(
        delivery_row(setup.pool(), unknown).await.status,
        "outcome_unknown"
    );
}

/// Row 21: a reopen whose proof expired before provider entry goes back to unknown with no
/// provider call; the action becomes uncertain again.
#[tokio::test]
async fn workflow_message_reconcile_expired_proof_before_begin_part_returns_to_unknown() {
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    let (p, scope) = (&setup.fixture.persistence, setup.scope());
    ledger(&setup, unknown, "not_applied", None, 1.0, None).await;
    let command = command(&setup, unknown, "short-proof", verified()).await;
    assert_eq!(
        reconcile(&service(&setup, true), &command)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::DeliveryReopened
    );
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), scope.run).await, "pending");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let claimed = claim(p, unknown).await;
    let started_before: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT request_started_at FROM message_delivery_parts WHERE delivery_id=$1",
    )
    .bind(unknown)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert_eq!(
        p.begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap(),
        DeliveryOutcome::Cancelled(DeliveryStatus::OutcomeUnknown)
    );
    let row = delivery_row(setup.pool(), unknown).await;
    assert_eq!(
        (
            row.status.as_str(),
            row.part.as_str(),
            row.last_error_class.as_deref(),
            row.attempt_count
        ),
        (
            "outcome_unknown",
            "outcome_unknown",
            Some("reconciliation_proof_expired"),
            1
        )
    );
    let started_after: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT request_started_at FROM message_delivery_parts WHERE delivery_id=$1",
    )
    .bind(unknown)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert_eq!(started_after, started_before, "no provider request started");
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), scope.run).await, "uncertain");
}

/// Row 20 and D4 §M/§Q: a proof consumed for a reopen, then an acceptance attested from before
/// that proof, is a finality breach. Truth is recorded, the conflict keeps the existing
/// orientation, further reopens are refused, and the effect wait parks for reconciliation
/// instead of advancing, while unsent siblings are superseded.
#[tokio::test]
async fn workflow_message_reconcile_finality_breach_vetoes_resume_and_reopen() {
    let setup = Box::pin(send(&[To::Owner, To::User])).await;
    parked(&setup).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    let (sibling, subject) = (
        delivery(setup.pool(), run, 0).await,
        delivery(setup.pool(), run, 1).await,
    );
    let accepted_at = Utc::now();
    send_unknown(&setup, subject).await;
    ledger(&setup, subject, "not_applied", None, 60.0, None).await;
    let first = command(&setup, subject, "breached-proof", verified()).await;
    assert_eq!(
        reconcile(&service(&setup, true), &first)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::DeliveryReopened
    );
    send_unknown(&setup, subject).await;
    ledger(&setup, subject, "applied", Some(accepted_at), 60.0, None).await;
    let second = command(&setup, subject, "breaching-truth", verified()).await;
    let result = reconcile(&service(&setup, true), &second).await.unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::EvidenceConflict);
    assert_eq!(
        delivery_row(setup.pool(), subject).await.status,
        "delivered"
    );
    let (proof, applied, reason): (Uuid, Option<Uuid>, String) = sqlx::query_as(
        "SELECT conflict.evidence_id, conflict.contradictory_evidence_id, conflict.reason FROM workflow_action_evidence_conflicts AS conflict WHERE conflict.run_id=$1",
    )
    .bind(run.as_uuid()).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(reason, "finality_breach");
    let dispositions: (String, String) = sqlx::query_as(
        "SELECT (SELECT disposition FROM workflow_action_evidence WHERE id=$1), (SELECT disposition FROM workflow_action_evidence WHERE id=$2)",
    )
    .bind(proof).bind(applied).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(dispositions, ("final_not_applied".into(), "applied".into()));
    // D4 §P: applied evidence on one delivery while a sibling is pending stays a possible
    // dispatch; the breach shows separately.
    let invocation = invocations(setup.pool(), run).await[0];
    assert_eq!(
        effect_state(setup.pool(), invocation).await,
        ("possible_dispatch".into(), true)
    );
    // The conflict wakes the wait even though the sibling is still pending.
    assert_eq!(candidates(&setup).await, vec![PollWork::MessageReceipt]);
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), run).await, "pending");
    assert_eq!(candidates(&setup).await, vec![PollWork::Wait]);
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Failed {
            reason: crate::application::workflow::waits::WaitFailureReason::EvidenceConflict,
            ..
        }
    ));
    let after = state(setup.pool(), scope).await;
    assert_eq!(
        (
            after.run.as_str(),
            after.waiting.as_deref(),
            after.job.as_str(),
            after.wait.as_deref()
        ),
        ("waiting", Some("reconciliation"), "failed", Some("failed"))
    );
    let code: String = sqlx::query_scalar(
        "SELECT failure_code FROM workflow_action_reconciliations WHERE run_id=$1",
    )
    .bind(run.as_uuid())
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert_eq!(code, "action.evidence_conflict");
    let unsent = delivery_row(setup.pool(), sibling).await;
    assert_eq!(
        (
            unsent.status.as_str(),
            unsent.cancellation_reason.as_deref(),
            unsent.attempt_count
        ),
        ("dead_letter", Some("evidence_conflict"), 0)
    );
    // The superseded sibling is consumed as truth (a failed receipt, retained); then the
    // parked execution is no longer discovered, scan after scan.
    assert_eq!(candidates(&setup).await, vec![PollWork::MessageReceipt]);
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), run).await, "failed");
    assert!(candidates(&setup).await.is_empty());
    assert!(candidates(&setup).await.is_empty());
    assert_eq!(
        state(setup.pool(), scope).await.waiting.as_deref(),
        Some("reconciliation")
    );
    // The 04.7 continuation can never reopen this park.
    let job = scope.job.0;
    let reopen_safe: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_reopen_safe($1,$2,$3)")
            .bind(scope.company.as_uuid())
            .bind(run.as_uuid())
            .bind(job)
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert!(!reopen_safe);
    assert_eq!(
        effect_state(setup.pool(), invocation).await,
        ("committed".into(), true)
    );
}

/// D3 §H.4 and D4 §O: once an execution holds a breach, a further final not-applied proof is
/// recorded but grants nothing, and the SQL reopen guard refuses it on its own.
#[tokio::test]
async fn workflow_message_reconcile_conflicted_execution_reopens_nothing() {
    let setup = Box::pin(send(&[To::Owner, To::User])).await;
    parked(&setup).await;
    let run = setup.scope().run;
    let (sibling, subject) = (
        delivery(setup.pool(), run, 0).await,
        delivery(setup.pool(), run, 1).await,
    );
    let accepted_at = Utc::now();
    send_unknown(&setup, sibling).await;
    send_unknown(&setup, subject).await;
    ledger(&setup, subject, "not_applied", None, 60.0, None).await;
    let first = command(&setup, subject, "conflict-proof", verified()).await;
    assert_eq!(
        reconcile(&service(&setup, true), &first)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::DeliveryReopened
    );
    send_unknown(&setup, subject).await;
    ledger(&setup, subject, "applied", Some(accepted_at), 60.0, None).await;
    let breach = command(&setup, subject, "conflict-truth", verified()).await;
    assert_eq!(
        reconcile(&service(&setup, true), &breach)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::EvidenceConflict
    );
    ledger(&setup, sibling, "not_applied", None, 60.0, None).await;
    let refused = command(&setup, sibling, "conflict-refused", verified()).await;
    let result = reconcile(&service(&setup, true), &refused).await.unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::EvidenceConflict
        }
    );
    assert_eq!(
        delivery_row(setup.pool(), sibling).await.status,
        "outcome_unknown"
    );
    // Every other guard condition holds for this proof; only the conflict refuses it.
    let error = sqlx::query(
        "INSERT INTO workflow_message_delivery_reopens(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,evidence_id,ordinal,delivery_id,attempt_count,valid_until) \
         SELECT evidence.company_id,evidence.run_id,evidence.execution_id,evidence.invocation_id,evidence.argument_digest,evidence.dispatch_id,gen_random_uuid(),evidence.id,evidence.delivery_ordinal,evidence.delivery_id,delivery.attempt_count,evidence.valid_until \
           FROM workflow_action_evidence AS evidence JOIN message_deliveries AS delivery ON delivery.id=evidence.delivery_id WHERE evidence.id=$1",
    )
    .bind(result.evidence.unwrap().as_uuid())
    .execute(setup.pool())
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires one unexpired final proof"),
        "{error}"
    );
    let (disposition, eligible): (String, bool) = sqlx::query_as(
        "SELECT disposition, grant_eligible FROM workflow_action_evidence WHERE id=$1",
    )
    .bind(result.evidence.unwrap().as_uuid())
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert_eq!(
        (disposition.as_str(), eligible),
        ("final_not_applied", true)
    );
}

/// Row 19: a proof already expired when the service issues it is refused before settlement,
/// with no evidence, command row or grant (the shared settlement re-check covers a proof
/// expiring after issue, as for remote markers).
#[tokio::test]
async fn workflow_message_reconcile_expired_proof_is_refused_without_a_grant() {
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    ledger(
        &setup,
        unknown,
        "not_applied",
        None,
        0.05,
        Some("SELECT pg_sleep(0.2)".into()),
    )
    .await;
    let command = command(&setup, unknown, "expired-proof", verified()).await;
    assert!(reconcile(&service(&setup, true), &command).await.is_err());
    let recorded: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_action_evidence), (SELECT count(*) FROM workflow_action_evidence_commands), (SELECT count(*) FROM workflow_message_delivery_reopens)",
    )
    .fetch_one(setup.pool()).await.unwrap();
    assert_eq!(recorded, (0, 0, 0));
    assert_eq!(
        delivery_row(setup.pool(), unknown).await.status,
        "outcome_unknown"
    );
}

/// D4 §N (F013.1): the attribution shape is checked on every row, whatever writes it.
#[tokio::test]
async fn workflow_message_sql_evidence_attribution_shape_rejects_forgeries() {
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    let note = EvidenceInput::UnknownNote {
        note: "seen nothing".into(),
        claimed: ClaimedDisposition::Unknown,
    };
    let recorded = reconcile(
        &service(&setup, false),
        &command(&setup, unknown, "shape-source", note).await,
    )
    .await
    .unwrap();
    let source = recorded.evidence.unwrap().as_uuid();
    for (patch, case) in [
        (
            json!({"applied_request":"delivery"}),
            "delivery attribution on unknown evidence",
        ),
        (
            json!({"disposition":"applied","applied_request":"delivery","delivery_id":null,"delivery_ordinal":null}),
            "delivery attribution without a subject",
        ),
        (
            json!({"disposition":"applied","applied_request":"unattributed"}),
            "remote attribution with a delivery subject",
        ),
    ] {
        let error = sqlx::query(
            "INSERT INTO workflow_action_evidence SELECT (jsonb_populate_record(NULL::workflow_action_evidence, \
               to_jsonb(evidence) || jsonb_build_object('id',gen_random_uuid(),'command_id',gen_random_uuid(),'command_key','forged') || $2)).* \
             FROM workflow_action_evidence AS evidence WHERE evidence.id=$1",
        )
        .bind(source)
        .bind(patch)
        .execute(setup.pool())
        .await
        .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514"),
            "{case}: {error}"
        );
    }
}

/// P08-D3 (F065, F069): the production composition annotates a message delivery whose outcome is
/// unknown (`Local { messaging }` through `CompanyReconciliationResources`, no verifier): the note
/// records, the delivery and the run stay as they were, and a verified reference is refused.
#[tokio::test]
async fn workflow_message_unknown_note_through_the_production_composition() {
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    let p = setup.fixture.persistence.clone();
    // §D3.4: the run detail names the message marker and its two delivery-backed destinations;
    // only the one whose outcome is unknown is reconcilable.
    use crate::application::workflow::reads::WorkflowReads;
    let scope = setup.scope();
    let detail = p
        .run_detail(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap();
    let [action] = detail.actions.as_slice() else {
        panic!("{:?}", detail.actions);
    };
    assert_eq!(action.effect_kind, "message");
    let unknown_destination = action
        .destinations
        .iter()
        .find(|destination| destination.delivery == unknown)
        .unwrap();
    assert_eq!(
        (
            unknown_destination.status.as_str(),
            unknown_destination.parts
        ),
        ("outcome_unknown", 1)
    );
    assert!(unknown_destination.reconcilable());
    assert_eq!(
        action
            .destinations
            .iter()
            .filter(|destination| destination.reconcilable())
            .count(),
        1
    );
    let production = crate::infra::workflow::WorkflowEngine::for_tests(p.clone()).reconciliation();
    let note = EvidenceInput::UnknownNote {
        note: "the provider lost the response".into(),
        claimed: ClaimedDisposition::Unknown,
    };
    let noted = command(&setup, unknown, "production-note", note).await;
    let result =
        Box::pin(production.reconcile(&noted, &CancellationToken::new(), Duration::from_secs(5)))
            .await
            .unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::UnknownRecorded);
    assert_eq!(
        delivery_row(setup.pool(), unknown).await.status,
        "outcome_unknown"
    );
    let without = command(&setup, unknown, "production-verified", verified()).await;
    assert!(
        Box::pin(
            production.reconcile(&without, &CancellationToken::new(), Duration::from_secs(5),)
        )
        .await
        .is_err()
    );
}
