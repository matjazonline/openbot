//! Exact reconciliation episodes survive evidence revisions but end at a real claim.
use super::*;
use crate::application::workflow::actions::ClaimedDisposition;
use crate::application::workflow::lease::{LeaseReleaseCause, WorkflowFailure};
use crate::application::workflow::{RetryCommand, RetryResult};
use crate::domain::workflow::RetrySafety;

struct EpisodeFixture {
    scheduled: Scheduled,
    verifier: Arc<LedgerVerifier>,
}

async fn episode_fixture() -> EpisodeFixture {
    // Box the real fixture/service seam to preserve stock 2 MiB test stacks.
    let (fixture, request) = Box::pin(limited()).await;
    let verifier = ledger(&fixture, &request).await;
    assert!(
        invoke(
            &fixture,
            &request,
            &LedgerProvider::new(&fixture, Delivery::Pending)
        )
        .await
        .is_err()
    );
    park(&fixture, &request).await;
    barrier(&fixture, &request).await;
    let command = proof_command(&fixture, &request, &verifier, "lifecycle-first").await;
    assert_eq!(
        run_command(&fixture, &command, verifier.clone())
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    EpisodeFixture {
        scheduled: Scheduled {
            fixture,
            request,
            command,
        },
        verifier,
    }
}

async fn pending_episode(s: &Scheduled) -> Option<String> {
    let scope = s.request.fence.scope;
    sqlx::query_scalar("SELECT workflow_action_pending_claim_episode($1,$2,$3,$4)")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(scope.execution.as_uuid())
        .bind(scope.job.0)
        .fetch_one(s.fixture.persistence().pool())
        .await
        .unwrap()
}

fn preserved(before: &Value, after: &Value, tables: &[&str]) {
    for table in tables {
        assert_eq!(before[table], after[table], "preserves {table}");
    }
}

fn attempts_preserved(before: &Value, after: &Value) {
    for attempt in before["task_attempts"].as_array().unwrap() {
        assert!(after["task_attempts"].as_array().unwrap().contains(attempt));
    }
}

const SAVED_FACTS: &[&str] = &[
    "workflow_root_budget_usage",
    "workflow_budget_receipts",
    "workflow_executions",
    "workflow_action_claim_episodes",
    "workflow_action_claim_budget_refusals",
    "workflow_action_evidence",
    "workflow_action_evidence_commands",
    "workflow_action_receipts",
    "workflow_action_dispatches",
    "workflow_action_remote_entries",
    "workflow_action_evidence_consumptions",
];

async fn advance_evidence(e: &EpisodeFixture) {
    let s = &e.scheduled;
    let before = all_tables(&s.fixture).await;
    assert_eq!(
        pending_episode(s).await.as_deref(),
        Some(s.command.command_key.as_str())
    );
    let mut note = s.command.clone();
    note.command_key = IdempotencyKey::parse("lifecycle-evidence-only").unwrap();
    note.expected_revision = s
        .fixture
        .persistence()
        .head(note.scope.company, note.scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    note.input = EvidenceInput::UnknownNote {
        note: "A later operator observation does not consume the scheduled episode".into(),
        claimed: ClaimedDisposition::Unknown,
    };
    let recorded = run_command(&s.fixture, &note, e.verifier.clone())
        .await
        .unwrap();
    assert_eq!(recorded.outcome, ReconciliationOutcome::UnknownRecorded);
    assert!(recorded.revision.0 > note.expected_revision.0);
    let advanced = all_tables(&s.fixture).await;
    for (table, rows) in before.as_object().unwrap() {
        if ![
            "workflow_runs",
            "workflow_action_evidence",
            "workflow_action_evidence_commands",
            "workflow_action_evidence_coverage",
            "workflow_run_events",
        ]
        .contains(&table.as_str())
        {
            assert_eq!(
                rows, &advanced[table],
                "evidence-only command preserves {table}"
            );
        }
    }
    let original = advanced["workflow_action_evidence_commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["command_key"] == json!(s.command.command_key.as_str()))
        .unwrap();
    assert_ne!(
        advanced["workflow_runs"][0]["revision"],
        original["result_revision"]
    );
    assert_eq!(
        pending_episode(s).await.as_deref(),
        Some(s.command.command_key.as_str())
    );
    let coverage = advanced["workflow_action_evidence_coverage"]
        .as_array()
        .unwrap();
    assert_eq!(
        coverage.len(),
        before["workflow_action_evidence_coverage"]
            .as_array()
            .unwrap()
            .len()
            + 1
    );
    for row in before["workflow_action_evidence_coverage"]
        .as_array()
        .unwrap()
    {
        assert!(coverage.contains(row), "existing coverage is immutable");
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_episode_lifecycle_revision_keeps_binding() {
    let e = Box::pin(episode_fixture()).await;
    Box::pin(advance_evidence(&e)).await;
    let s = &e.scheduled;
    Box::pin(exhaust(s)).await;
    let exhausted = all_tables(&s.fixture).await;
    assert_eq!(exhausted["workflow_root_budget_usage"][0]["model_calls"], 2);
    let (a, b) = tokio::join!(
        s.fixture
            .persistence()
            .claim_io(s.request.fence.scope, worker(), policy()),
        s.fixture
            .persistence()
            .claim_io(s.request.fence.scope, worker(), policy())
    );
    assert!(a.unwrap().is_none() && b.unwrap().is_none());
    let refused = all_tables(&s.fixture).await;
    assert_refusal(&exhausted, &refused, &s.request, &s.command);
    preserved(
        &exhausted,
        &refused,
        &["task_attempts", "workflow_root_budget_usage"],
    );
    assert_eq!(entry_consumptions(&s.fixture).await, (1, 0));
}

async fn safely_retire(s: &Scheduled) -> Value {
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("provider.rejected").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    });
    assert!(
        s.fixture
            .persistence()
            .release_io(s.request.fence, policy(), cause)
            .await
            .unwrap()
    );
    let retired = all_tables(&s.fixture).await;
    assert_eq!(retired["workflow_runs"][0]["state"], "failed");
    let attempt = retired["task_attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["attempt_number"] == json!(s.request.fence.attempt.0))
        .unwrap();
    assert_eq!(attempt["workflow_retry_safety"], "safe");
    assert_eq!(attempt["workflow_retirement"], "live");
    assert_eq!(attempt["workflow_failure_code"], "provider.rejected");
    retired
}

async fn safe_retry(s: &mut Scheduled) {
    let retired = safely_retire(s).await;
    let scope = s.request.fence.scope;
    let revision = s
        .fixture
        .persistence()
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    assert!(matches!(
        s.fixture
            .persistence()
            .retry(RetryCommand {
                company_id: scope.company,
                run_id: scope.run,
                actor: s.fixture.binding.target.actor,
                command_key: IdempotencyKey::parse("lifecycle-ordinary-retry").unwrap(),
                expected_revision: revision,
            })
            .await
            .unwrap(),
        RetryResult::Applied { .. }
    ));
    assert!(
        pending_episode(s).await.is_none(),
        "historical binding cannot gate the pending ordinary retry"
    );
    let pending = all_tables(&s.fixture).await;
    preserved(&retired, &pending, SAVED_FACTS);
    assert_eq!(retired["task_attempts"], pending["task_attempts"]);
    let old = s.request.fence;
    new_claim(&s.fixture, &mut s.request).await;
    assert_eq!(s.request.fence.attempt.0, old.attempt.0 + 1);
    assert_ne!(s.request.fence.generation, old.generation);
    assert!(
        s.fixture
            .persistence()
            .validate_io(s.request.fence, policy())
            .await
            .unwrap()
    );
    let claimed = all_tables(&s.fixture).await;
    preserved(&pending, &claimed, SAVED_FACTS);
    attempts_preserved(&pending, &claimed);
    assert_eq!(
        claimed["task_attempts"].as_array().unwrap().len(),
        pending["task_attempts"].as_array().unwrap().len() + 1
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_episode_lifecycle_consumed_allows_saved_retry() {
    let mut e = Box::pin(episode_fixture()).await;
    let s = &mut e.scheduled;
    let before = all_tables(&s.fixture).await;
    new_claim(&s.fixture, &mut s.request).await;
    assert_eq!(s.request.fence.attempt.0, 2);
    assert!(pending_episode(s).await.is_none());
    let claimed = all_tables(&s.fixture).await;
    preserved(&before, &claimed, SAVED_FACTS);
    attempts_preserved(&before, &claimed);
    let charge = BudgetCharge::new(BudgetResource::ModelCall, 2).unwrap();
    let key = BudgetReservationKey::parse("lifecycle-saved-operation").unwrap();
    assert_eq!(
        s.fixture
            .persistence()
            .reserve_budget(
                BudgetReservation::new(s.request.fence, key.clone(), charge).unwrap(),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
    let charged = all_tables(&s.fixture).await;
    assert_eq!(charged["workflow_root_budget_usage"][0]["model_calls"], 2);
    assert_eq!(charged["workflow_root_budgets"][0]["model_calls"], 2);
    Box::pin(safe_retry(s)).await;
    assert_eq!(s.request.fence.attempt.0, 3);
    assert_eq!(
        s.fixture
            .persistence()
            .reserve_budget(
                BudgetReservation::new(s.request.fence, key, charge).unwrap(),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::Replayed(BudgetDisposition::Granted)
    );
    let replayed = all_tables(&s.fixture).await;
    preserved(&charged, &replayed, SAVED_FACTS);
    assert_eq!(entry_consumptions(&s.fixture).await, (1, 0));
    assert_eq!(effects(&s.fixture).await, 0);
    assert!(
        replayed["workflow_action_claim_budget_refusals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_episode_lifecycle_later_schedule_is_distinct() {
    let mut e = Box::pin(episode_fixture()).await;
    let s = &mut e.scheduled;
    let first = all_tables(&s.fixture).await;
    new_claim(&s.fixture, &mut s.request).await;
    let provider = LedgerProvider::new(&s.fixture, Delivery::Pending);
    assert!(invoke(&s.fixture, &s.request, &provider).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(&s.fixture).await, (2, 1));
    park(&s.fixture, &s.request).await;
    barrier(&s.fixture, &s.request).await;
    assert!(pending_episode(s).await.is_none());
    let retired = all_tables(&s.fixture).await;
    let next = proof_command(&s.fixture, &s.request, &e.verifier, "lifecycle-second").await;
    assert_eq!(
        run_command(&s.fixture, &next, e.verifier.clone())
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let scheduled = all_tables(&s.fixture).await;
    let episodes = scheduled["workflow_action_claim_episodes"]
        .as_array()
        .unwrap();
    assert_eq!(episodes.len(), 2);
    assert!(episodes.contains(&first["workflow_action_claim_episodes"][0]));
    let second = episodes
        .iter()
        .find(|episode| episode["command_key"] == json!(next.command_key.as_str()))
        .unwrap();
    for field in ["company_id", "run_id", "execution_id", "job_id"] {
        assert_eq!(
            second[field],
            first["workflow_action_claim_episodes"][0][field]
        );
    }
    assert_eq!(second["retired_attempt"], 2);
    assert_eq!(
        first["workflow_action_claim_episodes"][0]["retired_attempt"],
        1
    );
    assert_eq!(
        pending_episode(s).await.as_deref(),
        Some(next.command_key.as_str())
    );
    preserved(
        &retired,
        &scheduled,
        &[
            "task_attempts",
            "workflow_executions",
            "workflow_root_budget_usage",
            "workflow_budget_receipts",
            "workflow_action_evidence_consumptions",
            "workflow_action_remote_entries",
        ],
    );
    new_claim(&s.fixture, &mut s.request).await;
    assert_eq!(s.request.fence.attempt.0, 3);
    assert!(pending_episode(s).await.is_none());
    let claimed = all_tables(&s.fixture).await;
    preserved(&scheduled, &claimed, SAVED_FACTS);
    attempts_preserved(&scheduled, &claimed);
    assert_eq!(
        claimed["task_attempts"].as_array().unwrap().len(),
        scheduled["task_attempts"].as_array().unwrap().len() + 1
    );
    assert_eq!(entry_consumptions(&s.fixture).await, (2, 1));
    assert_eq!(effects(&s.fixture).await, 0);
}
