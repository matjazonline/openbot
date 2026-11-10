use super::*;
#[path = "channel_reply_continuation_views_test_support.rs"]
mod support;
use crate::adapters::persistence::workflow::tests::message_tests::support::{lease, runtime};
use crate::application::workflow::{
    polling::WorkflowStepKind,
    triggers::MessageTriggerInput,
    waits::{IngressReply, ReplyIngress, WaitProgress, WorkflowEventId, WorkflowWaits},
    worker::WorkflowHandler,
};
use crate::entities::message::CanonicalMessageId;
use chrono::Utc;
use support::*;

#[tokio::test]
async fn channel_reply_views_later_question_uses_actual_answer_and_keeps_original_source() {
    let f = Box::pin(questions()).await;
    let next = Box::pin(two_questions(&f, f.scope)).await;
    f.p.advance_pure(
        next,
        BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
    )
    .await
    .unwrap();
    let completed = outcome(&f).await;
    assert!(!completed.prompt.as_ref().unwrap().waiting);
    assert_eq!(completed.label(), "Completed without automatic reply");
}

fn assert_private_review_summary(
    waiting: &ChannelResponseOutcome,
    company: Uuid,
    view: &DecisionView,
) {
    let html = crate::adapters::http::pages::channel_response_panel(
        waiting,
        company,
        crate::adapters::http::pages::RunLinkAccess::Inspect,
    );
    assert!(html.contains("Waiting for reviewer"));
    assert!(!html.contains(&view.decision.0.to_string()) && !html.contains(&view.artifact_digest));
}

#[tokio::test]
async fn channel_reply_views_child_review_wakes_and_reconciles_the_unchanged_parent() {
    for choice in ["continue", "revise"] {
        let case = Box::pin(child_review(
            Box::pin(Intents::new()).await,
            Utc::now() + chrono::Duration::minutes(10),
            ReviewDepth::Direct,
        ))
        .await;
        let f = &case.fixture;
        assert_eq!(outcome(f).await.label(), "Processing");
        let mut events = listener(f).await;
        let _opened =
            f.p.park_decision(case.child, &approval_test_support::notices(&f.p))
                .await
                .unwrap()
                .unwrap();
        let wakes = decision_wakes(&mut events).await;
        assert!(
            wakes
                .iter()
                .any(|(kind, payload)| kind == "workflow_run_changed"
                    && payload["run_id"] == json!(case.child.run.as_uuid()))
        );
        assert!(wakes.iter().any(|(kind, payload)| kind == "thread_activity"
            && payload["thread_id"] == json!(f.s.thread)));
        let parent_reason: String = sqlx::query_scalar(
            "SELECT waiting_reason FROM workflow_runs WHERE company_id=$1 AND id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .fetch_one(f.p.pool())
        .await
        .unwrap();
        assert_eq!(
            parent_reason, "child_run",
            "The parent does not change state when its child parks"
        );
        let waiting = outcome(f).await;
        assert!(waiting.waiting_for_review);
        assert_eq!(waiting.label(), "Waiting for reviewer");
        let decision: Uuid = sqlx::query_scalar(
            "SELECT id FROM workflow_decisions WHERE company_id=$1 AND execution_id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(case.child.execution.as_uuid())
        .fetch_one(f.p.pool())
        .await
        .unwrap();
        let view =
            f.p.read_decision(f.scope.company, f.s.owner(), DecisionId(decision))
                .await
                .unwrap();
        assert_private_review_summary(&waiting, f.scope.company.as_uuid(), &view);
        let foreign = channel_reply_views::read(
            &f.p,
            CompanyId::new(Uuid::new_v4()),
            f.s.owner(),
            &[f.scope.run.as_uuid()],
        )
        .await
        .unwrap();
        assert!(foreign.is_empty());
        f.p.submit_decision(SubmitDecision {
            company: f.scope.company,
            actor: f.s.owner(),
            decision: view.decision,
            command_key: DecisionCommandKey::parse(choice).unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: choice.into(),
            feedback: "Private feedback".into(),
            data: json!("edited"),
        })
        .await
        .unwrap();
        assert!(matches!(
            f.p.resume_wait(f.scope).await.unwrap(),
            WaitProgress::Completed(_)
        ));
        assert!(!outcome(f).await.waiting_for_review);
        assert_eq!(outcome(f).await.label(), "Work completed; reply pending");
    }
}

#[tokio::test]
async fn channel_reply_views_child_review_classification_does_not_reveal_private_wait_metadata() {
    let case = Box::pin(child_review(
        Box::pin(classified_intents()).await,
        Utc::now() + chrono::Duration::minutes(10),
        ReviewDepth::Direct,
    ))
    .await;
    let f = &case.fixture;
    f.p.park_decision(case.child, &approval_test_support::notices(&f.p))
        .await
        .unwrap()
        .unwrap();
    let viewer = WorkflowActor::authenticated(member(&f.s.fixture, "member").await).unwrap();
    let mut tx = f.p.pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        f.scope.company,
        case.child.run,
        crate::entities::access_level::AccessLevel::L5,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let visible =
        channel_reply_views::read(&f.p, f.scope.company, viewer, &[f.scope.run.as_uuid()])
            .await
            .unwrap();
    assert_eq!(
        visible.len(),
        1,
        "The public owning response remains visible"
    );
    assert!(
        !visible[0].waiting_for_review,
        "Restricted child metadata must stay private"
    );
    assert!(
        outcome(f).await.waiting_for_review,
        "The authorized owner sees its actual child wait"
    );
}

#[tokio::test]
async fn channel_reply_views_expired_child_review_advances_parent_without_stale_wait_label() {
    let s = Box::pin(Intents::new()).await;
    let deadline = Utc::now() + chrono::Duration::seconds(3);
    let case = Box::pin(child_review(s, deadline, ReviewDepth::Direct)).await;
    let f = &case.fixture;
    f.p.park_decision(case.child, &approval_test_support::notices(&f.p))
        .await
        .unwrap()
        .unwrap();
    assert!(outcome(f).await.waiting_for_review);
    let remaining = (deadline - Utc::now()).to_std().unwrap_or_default();
    tokio::time::sleep(remaining + Duration::from_millis(20)).await;
    assert!(matches!(
        f.p.resume_wait(case.child).await.unwrap(),
        WaitProgress::Expired(_) | WaitProgress::Failed { .. }
    ));
    let resumed = f.p.resume_wait(f.scope).await.unwrap();
    assert_eq!(f.state().await["run"], "failed", "{resumed:?}");
    let done = outcome(f).await;
    assert!(!done.waiting_for_review);
    assert!(done.label().starts_with("Failed"));
}

#[tokio::test]
async fn channel_reply_views_child_later_question_preserves_owning_root_and_continues() {
    let case = Box::pin(child_questions()).await;
    let f = &case.fixture;
    let next = Box::pin(two_questions(f, case.child)).await;
    f.p.advance_pure(
        next,
        BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
    )
    .await
    .unwrap();
    assert!(matches!(
        f.p.resume_wait(f.scope).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    let done = outcome(f).await;
    assert_eq!(done.run, f.scope.run);
    assert!(!done.prompt.as_ref().unwrap().waiting);
    assert_eq!(done.label(), "Work completed; reply pending");
}

#[tokio::test]
async fn channel_reply_views_nested_review_emits_a_root_wakeup_without_an_ancestor_state_change() {
    let case = Box::pin(child_review(
        Box::pin(Intents::new()).await,
        Utc::now() + chrono::Duration::minutes(10),
        ReviewDepth::Grandchild,
    ))
    .await;
    let f = &case.fixture;
    let mut events = listener(f).await;
    assert_eq!(outcome(f).await.label(), "Processing");
    f.p.park_decision(case.child, &approval_test_support::notices(&f.p))
        .await
        .unwrap()
        .unwrap();
    let wakes = decision_wakes(&mut events).await;
    assert!(
        wakes
            .iter()
            .any(|(kind, payload)| kind == "workflow_run_changed"
                && payload["run_id"] == json!(f.scope.run.as_uuid())),
        "An open root pane subscribes to its root/direct children, so a grandchild review must wake the actual root: {wakes:?}"
    );
    let waiting_ancestors: i64=sqlx::query_scalar("SELECT count(*) FROM workflow_runs AS run JOIN workflow_run_budgets AS budget ON budget.company_id=run.company_id AND budget.run_id=run.id WHERE budget.company_id=$1 AND budget.root_run_id=$2 AND run.id<>$3 AND run.waiting_reason='child_run'")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(case.child.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(waiting_ancestors, 2);
    let read = outcome(f).await;
    assert!(read.waiting_for_review);
    assert_eq!(read.label(), "Waiting for reviewer");
    assert!(
        wakes
            .iter()
            .all(|(_, payload)| payload.get("proposal").is_none()
                && payload.get("reviewers").is_none()
                && payload.get("decision_id").is_none())
    );
    let decision: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_decisions WHERE company_id=$1 AND execution_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(case.child.execution.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    let view =
        f.p.read_decision(f.scope.company, f.s.owner(), DecisionId(decision))
            .await
            .unwrap();
    let mut settled = listener(f).await;
    f.p.submit_decision(SubmitDecision {
        company: f.scope.company,
        actor: f.s.owner(),
        decision: view.decision,
        command_key: DecisionCommandKey::parse("nested-close").unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest,
        choice: "continue".into(),
        feedback: "Private".into(),
        data: json!("edited"),
    })
    .await
    .unwrap();
    let closed_wakes = decision_wakes(&mut settled).await;
    assert!(
        closed_wakes
            .iter()
            .any(|(kind, payload)| kind == "workflow_run_changed"
                && payload["run_id"] == json!(f.scope.run.as_uuid())),
        "Settlement must also refresh the root's stale waiting summary: {closed_wakes:?}"
    );
    assert!(!outcome(f).await.waiting_for_review);
}
