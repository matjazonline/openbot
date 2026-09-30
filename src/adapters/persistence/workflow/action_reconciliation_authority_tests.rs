//! Real current principal/resource authority across reconciliation and dispatch.
use super::*;
use crate::adapters::persistence::workflow::action_reconciliation::SqlReconciliationResources;
use crate::application::use_cases::company::CompanyPersistence;
use crate::application::workflow::polling::WorkflowPolling;
use crate::application::workflow::{RetryCommand, RetryResult};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[path = "action_reconciliation_authority_support.rs"]
mod support;
use support::*;

#[path = "action_reconciliation_proof_tests.rs"]
mod proof_tests;

#[path = "action_reconciliation_association_tests.rs"]
mod association_tests;

#[tokio::test]
async fn workflow_action_reconciliation_current_owner_admin_and_denied_principals() {
    let (f, request, verifier) = prepared().await;
    let admin = principal(&f, Some("admin")).await;
    let member = principal(&f, Some("member")).await;
    let outsider = principal(&f, None).await;
    let owner = f.binding.target.actor;
    for (actor, allowed, key) in [
        (owner, true, "owner"),
        (admin, true, "admin"),
        (member, false, "member"),
        (outsider, false, "outsider"),
    ] {
        let mut c = verified_command(&f, &request, &verifier, key).await;
        c.actor = actor;
        let before = durable(&f).await;
        let calls = verifier.calls.load(Ordering::SeqCst);
        let result = execute(&f, &c, verifier.clone()).await;
        let after = durable(&f).await;
        if allowed {
            assert!(matches!(
                result.unwrap().outcome,
                ReconciliationOutcome::UnknownRecorded
            ));
            assert_eq!(verifier.calls.load(Ordering::SeqCst), calls + 1);
            same_execution(&before, &after);
        } else {
            assert!(matches!(result, Err(AppError::NotFound(_))));
            assert_eq!(before, after);
            assert_eq!(verifier.calls.load(Ordering::SeqCst), calls);
        }
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_forged_scope_and_subject_never_verify() {
    let (f, request, verifier) = prepared().await;
    let original = verified_command(&f, &request, &verifier, "forged").await;
    for variant in 0..6 {
        let mut c = original.clone();
        match variant {
            0 => c.scope.company = CompanyId::new(Uuid::new_v4()),
            1 => c.scope.run = RunId::new(Uuid::new_v4()),
            2 => c.scope.execution = ExecutionId::new(Uuid::new_v4()),
            3 => c.subject.invocation = ActionInvocationId::new(Uuid::new_v4()),
            4 => c.subject.argument_digest = serde_json::from_value(json!("f".repeat(64))).unwrap(),
            _ => c.marker = ActionRemoteMarkerId::new(Uuid::new_v4()),
        }
        let before = durable(&f).await;
        assert!(
            execute(&f, &c, verifier.clone()).await.is_err(),
            "variant{variant}"
        );
        assert_eq!(durable(&f).await, before);
        assert_eq!(verifier.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_unknown_restart_replay_conflict_and_ordinary_retry() {
    let (f, request, verifier) = prepared().await;
    let mut c = command(&f, &request, marker(&f).await).await;
    for (key, claimed) in [
        ("unknown", ClaimedDisposition::Unknown),
        ("claim-applied", ClaimedDisposition::Applied),
        ("claim-absent", ClaimedDisposition::NotApplied),
    ] {
        c.expected_revision = f
            .persistence()
            .head(c.scope.company, c.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision;
        c.command_key = IdempotencyKey::parse(key).unwrap();
        c.input = EvidenceInput::UnknownNote {
            note: "operator supplied safe label".into(),
            claimed,
        };
        let before = durable(&f).await;
        let saved = execute(&f, &c, verifier.clone()).await.unwrap();
        assert!(matches!(
            saved.outcome,
            ReconciliationOutcome::UnknownRecorded
        ));
        let committed = durable(&f).await;
        same_execution(&before, &committed);
        let replay = execute(&f, &c, verifier.clone()).await.unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.outcome, saved.outcome);
        assert_eq!(replay.revision, saved.revision);
        assert_eq!(replay.evidence, saved.evidence);
        assert_eq!(durable(&f).await, committed);
        let mut changed = c.clone();
        changed.expected_revision = saved.revision;
        assert_eq!(
            execute(&f, &changed, verifier.clone())
                .await
                .unwrap()
                .outcome,
            ReconciliationOutcome::IdempotencyConflict
        );
        assert_eq!(durable(&f).await, committed);
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_verified_unknown_restart_and_retry() {
    let (f, request, verifier) = prepared().await;
    let mut verified = verified_command(&f, &request, &verifier, "verified-unknown").await;
    let saved = execute(&f, &verified, verifier.clone()).await.unwrap();
    assert!(matches!(
        saved.outcome,
        ReconciliationOutcome::UnknownRecorded
    ));
    let before = durable(&f).await;
    assert!(
        execute(&f, &verified, verifier.clone())
            .await
            .unwrap()
            .replayed
    );
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.persistence()
            .retry(RetryCommand {
                company_id: verified.scope.company,
                actor: verified.actor,
                run_id: verified.scope.run,
                expected_revision: saved.revision,
                command_key: IdempotencyKey::parse("ordinary").unwrap()
            })
            .await
            .unwrap(),
        RetryResult::Unsafe {
            revision: saved.revision
        }
    );
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    same_execution(&before, &durable(&f).await);
    grant(&f, verified.actor, false).await;
    assert!(execute(&f, &verified, verifier.clone()).await.is_err());
    verified.command_key = IdempotencyKey::parse("revoked-new").unwrap();
    assert!(execute(&f, &verified, verifier.clone()).await.is_err());
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_admin_truth_cannot_replace_dispatch_actor() {
    let (f, mut request) = setup().await;
    let r = resources(&f).await;
    let admin = principal(&f, Some("admin")).await;
    let verifier = Arc::new(ObservedVerifier::new(ledger(&f, &request).await));
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: json!({"written":true}),
            recover: true,
            lose: true,
        },
    );
    assert!(invoke(&f, &request, &provider).await.is_err());
    park(&f, &request).await;
    let mut c = verified_command(&f, &request, &verifier, "admin-applied").await;
    c.actor = admin;
    let saved = execute(&f, &c, verifier.clone()).await.unwrap();
    assert!(matches!(
        saved.outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    ));
    let actor: Uuid = sqlx::query_scalar("SELECT actor_id FROM workflow_runs")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(actor, f.binding.target.actor.user_id());
    let audit_actor: Uuid = sqlx::query_scalar(
        "SELECT actor_id FROM workflow_run_events WHERE event_kind='action_reconciled'",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(audit_actor, admin.user_id());
    successful_replay(&f, &c, verifier.clone(), &saved).await;
    new_claim(&f, &mut request).await;
    grant(&f, f.binding.target.actor, false).await;
    let restart = LedgerProvider::new(&f, Delivery::Pending);
    let dispatch = PostgresActionDispatch::new(f.persistence().clone(), r, Effect(0));
    assert!(
        ActionService::new(dispatch)
            .dispatch_remote(&request, &restart, &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(restart.calls.load(Ordering::SeqCst), 0);
    assert_eq!(counts(&f).await.0, 1);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_paused_verifier_rechecks_revocation_and_revision() {
    for mutation in ["resource", "principal", "revision"] {
        let (f, request, prepared_verifier) = prepared().await;
        let admin = principal(&f, Some("admin")).await;
        let ledger = prepared_verifier.ledger.clone();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let verifier = Arc::new(ObservedVerifier {
            ledger,
            calls: AtomicUsize::new(0),
            pause: Some((started.clone(), release.clone())),
        });
        let mut c = verified_command(&f, &request, &verifier, mutation).await;
        c.actor = admin;
        let competitor = async {
            started.notified().await;
            tokio::time::timeout(
                Duration::from_secs(1),
                mutate_authority(&f, admin, mutation),
            )
            .await
            .expect("snapshot locks released before verifier I/O");
            release.notify_one();
        };
        let (result, ()) = tokio::join!(execute(&f, &c, verifier), competitor);
        if mutation == "revision" {
            assert!(matches!(
                result.unwrap().outcome,
                ReconciliationOutcome::Blocked {
                    reason: ReconciliationBlockedReason::StaleSnapshot
                }
            ));
        } else {
            assert!(result.is_err());
        }
        let evidence: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_evidence")
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
        assert_eq!(evidence, 0);
        assert!(
            f.persistence()
                .claim_io(request.fence.scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_paused_verifier_cancel_and_deadline_preserve_terminal() {
    for cancelled in [true, false] {
        let (f, request, original) = prepared().await;
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let verifier = Arc::new(ObservedVerifier {
            ledger: original.ledger.clone(),
            calls: AtomicUsize::new(0),
            pause: Some((started.clone(), release.clone())),
        });
        let c = verified_command(&f, &request, &verifier, "terminal-race").await;
        let competitor = async {
            started.notified().await;
            tokio::time::timeout(Duration::from_secs(1),async {
                if cancelled {
                    let result=f.persistence().cancel(crate::application::workflow::CancelCommand {
                        company_id:c.scope.company,run_id:c.scope.run,actor:c.actor,
                        command_key:IdempotencyKey::parse("cancel-competing").unwrap(),expected_revision:c.expected_revision
                    }).await.unwrap();
                    assert!(matches!(result,crate::application::workflow::CancelResult::Applied {..}));
                } else {
                    sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute'")
                        .execute(f.persistence().pool()).await.unwrap();
                    assert!(f.persistence().expire_run(request.fence.scope).await.unwrap());
                }
            }).await.expect("terminal owner progresses during provider verification");
            let terminal = durable(&f).await;
            release.notify_one();
            terminal
        };
        let (result, terminal) = tokio::join!(execute(&f, &c, verifier), competitor);
        assert!(matches!(
            result.unwrap().outcome,
            ReconciliationOutcome::Blocked {
                reason: ReconciliationBlockedReason::StaleSnapshot
            }
        ));
        let after = durable(&f).await;
        same_execution(&terminal, &after);
        assert_eq!(terminal["runs"], after["runs"]);
        assert_eq!(terminal["events"], after["events"]);
        assert_eq!(
            after["runs"][0]["state"],
            if cancelled { "cancelled" } else { "failed" }
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workflow_action_evidence")
                .fetch_one(f.persistence().pool())
                .await
                .unwrap(),
            0
        );
        assert!(
            f.persistence()
                .claim_io(request.fence.scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
    }
}

fn assert_owner_absent(state: &Value) {
    for table in [
        "companies",
        "workflow_runs",
        "workflow_executions",
        "background_tasks",
        "task_attempts",
        "workflow_action_intents",
        "workflow_action_dispatches",
        "workflow_action_evidence",
        "workflow_action_evidence_commands",
        "workflow_action_evidence_consumptions",
    ] {
        assert_eq!(state[table], json!([]), "deleted owner leaves no {table}");
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_paused_owner_deletion_refuses_without_resurrection() {
    // Box real fixture and service seams to retain the stock 2 MiB stack gate.
    let (f, request, prepared_verifier) = Box::pin(prepared()).await;
    barrier(&f, &request).await;
    let provider_before = proof_tests::all_tables(&f).await;
    assert!(
        !provider_before["fixture_evidence_ledger"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let owner: Uuid = sqlx::query_scalar("SELECT user_id FROM companies WHERE id=$1")
        .bind(request.scope().company.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let verifier = Arc::new(ObservedVerifier {
        ledger: prepared_verifier.ledger.clone(),
        calls: AtomicUsize::new(0),
        pause: Some((started.clone(), release.clone())),
    });
    let c = verified_command(&f, &request, &verifier, "paused-owner-deletion").await;
    let deletion = async {
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .expect("authentic ledger attestation reaches the paused verifier");
        tokio::time::timeout(
            Duration::from_secs(2),
            CompanyPersistence::delete_for_user(f.persistence(), owner, c.scope.company.as_uuid()),
        )
        .await
        .expect("snapshot owner locks released before verifier I/O")
        .unwrap();
        let deleted = proof_tests::all_tables(&f).await;
        assert_owner_absent(&deleted);
        release.notify_one();
        deleted
    };
    let (result, deleted) = tokio::join!(Box::pin(execute(&f, &c, verifier.clone())), deletion);
    assert!(matches!(result, Err(AppError::NotFound(_))));
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    for table in [
        "fixture_evidence_ledger",
        "fixture_provider_operations",
        "fixture_provider_effects",
    ] {
        assert_eq!(
            deleted[table], provider_before[table],
            "external {table} survives owner deletion"
        );
    }
    assert_eq!(proof_tests::all_tables(&f).await, deleted);
    assert!(matches!(
        Box::pin(execute(&f, &c, verifier.clone())).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        verifier.calls.load(Ordering::SeqCst),
        1,
        "missing owner refuses before verification"
    );
    assert_eq!(proof_tests::all_tables(&f).await, deleted);
}
