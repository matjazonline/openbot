use super::*;
use chrono::{DateTime, Utc};
use sqlx::postgres::types::PgInterval;
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

use crate::adapters::persistence::test_support::{UNSCOPED_CLAIM, test_pool};
use crate::entities::message::{Message, MessageDirection, MessageRole};
use crate::entities::task::TaskFailureOutcome;
use crate::{
    adapters::persistence::PostgresPersistence,
    entities::{
        correlation::CorrelationId,
        outbox::OutboxStatus,
        outreach::{CreateOutreachRequest, OutreachStatus},
        stuck_work::StuckWorkThresholds,
        task::{
            BackgroundTask, ChainStage, NewTask, ResumeActor, StopActor, TaskAttemptOutcome,
            TaskAttemptRecordStatus, TaskAttemptRef, TaskAttemptStatus, TaskBoardFilter,
            TaskChainCard, TaskChainCounts, TaskFailure, TaskLeaseRef, TaskStatus, TaskStatusEvent,
            TaskStopReason, TaskTransitionActorKind, TaskTransitionReason, ThreadActivity,
            TokenUsage,
        },
        value_objects::MessageId,
    },
};

/// What a fixture that forces a status writes instead of an attribution. It has no cause to
/// state, and leaving the columns out would carry the previous transition's into the event.
///
/// The ledger row it produces falls to the trigger's deterministic mapping, and lands on
/// `unknown` for any status pair that mapping does not cover -- which is the honest record of
/// a fixture reaching into the table. Production callers all state their cause, so a scoped
/// lifecycle assertion still expects no `unknown` rows of its own.
const CLEAR_TRANSITION: &str = "transition_reason = NULL, transition_actor_kind = NULL, \
         transition_actor_id = NULL, transition_approval_id = NULL, transition_outreach_id = NULL";
use crate::services::outbound_dispatcher::OutboundEmail;
use crate::use_cases::{
    channel::{ChannelPersistence, ChannelWrite},
    company::{CompanyPersistence, CompanyWrite},
    thread::ThreadPersistence,
    user::UserPersistence,
};

#[test]
fn quorum_threshold_rounds_up() {
    assert_eq!(required_response_count(1, 100.0), 1);
    assert_eq!(required_response_count(3, 50.0), 2);
    assert_eq!(required_response_count(4, 50.0), 2);
    assert_eq!(required_response_count(10, 20.0), 2);
}

#[tokio::test]
async fn task_chain_board_groups_by_correlation_and_keeps_complete_chain_under_channel_filter() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("board_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Board Test".into(),
            slug: format!("board-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let first_channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "First Channel".into(),
            slug: "first".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let second_channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Second Channel".into(),
            slug: "second".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let participant = crate::entities::value_objects::EmailAddress::from(email);
    let thread = persistence
        .create_thread(
            first_channel.id,
            "Root chain subject",
            std::slice::from_ref(&participant),
        )
        .await
        .unwrap();
    let root = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            first_channel.id,
            Some(thread.id),
            "root_task",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let nested = persistence
        .enqueue_task(NewTask {
            company_id: company.id,
            channel_id: second_channel.id,
            thread_id: None,
            task_type: "nested_task".into(),
            payload: serde_json::json!({}),
            correlation_id: root.correlation_id,
        })
        .await
        .unwrap();
    let unrelated = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            first_channel.id,
            Some(thread.id),
            "unrelated_same_thread",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    let filter = TaskBoardFilter::new(Some(second_channel.id), Utc::now());
    let board = persistence
        .list_task_chain_board(company.id, filter)
        .await
        .unwrap();
    assert_eq!(board.total(ChainStage::Queued), 1);
    let card = &board.cards(ChainStage::Queued)[0];
    assert_eq!(card.correlation_id, root.correlation_id);
    assert_eq!(card.counts.total_tasks, 2);
    assert_eq!(card.title, "Root chain subject");
    assert_eq!(
        card.channel_names,
        vec!["First Channel".to_string(), "Second Channel".to_string()]
    );

    let detail = persistence
        .get_task_chain_detail(company.id, root.correlation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        detail
            .tasks
            .iter()
            .map(|item| item.task.id)
            .collect::<Vec<_>>(),
        vec![root.id, nested.id]
    );
    assert_eq!(detail.events.len(), 2);
    assert!(
        detail
            .events
            .iter()
            .all(|event| event.reason == TaskTransitionReason::Enqueued)
    );
    assert!(
        persistence
            .get_task_chain_detail(Uuid::new_v4(), root.correlation_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_ne!(unrelated.correlation_id, root.correlation_id);

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn status_event_constraints_reject_cross_company_rows_and_duplicate_sequences() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("event_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Event Test".into(),
            slug: format!("event-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Events".into(),
            slug: "events".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "event_test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    let insert = |company_id: Uuid, sequence: i32| {
        sqlx::query(
            r#"INSERT INTO task_status_events (
                       id, company_id, task_id, correlation_id, sequence, from_status, to_status,
                       reason, actor_kind, retry_count, run_at, transitioned_at
                   ) VALUES ($1, $2, $3, $4, $5, 'pending', 'pending',
                             'operator_resumed', 'operator', 0, CURRENT_TIMESTAMP,
                             CURRENT_TIMESTAMP)"#,
        )
        .bind(Uuid::new_v4())
        .bind(company_id)
        .bind(task.id)
        .bind(task.correlation_id.as_uuid())
        .bind(sequence)
        .execute(&pool)
    };
    assert!(insert(Uuid::new_v4(), 2).await.is_err());
    assert!(insert(company.id, 1).await.is_err());

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn guarded_transitions_emit_only_on_success_and_operator_actions_record_the_user() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("actor_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Actor Test".into(),
            slug: format!("actor-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Actors".into(),
            slug: "actors".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "actor_test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let worker = Uuid::new_v4();
    let expires = Utc::now() + chrono::Duration::minutes(5);
    assert!(
        persistence
            .claim_task(task.id, worker, expires)
            .await
            .unwrap()
    );
    assert!(
        !persistence
            .claim_task(task.id, Uuid::new_v4(), expires)
            .await
            .unwrap()
    );
    let before_stop = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap();
    assert_eq!(before_stop.len(), 2, "the failed guard must emit no event");

    persistence
        .stop_task(task.id, StopActor::Operator(owner.id))
        .await
        .unwrap();
    persistence
        .resume_task(task.id, ResumeActor::Operator(owner.id))
        .await
        .unwrap();
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap();
    for reason in [
        TaskTransitionReason::OperatorStopped,
        TaskTransitionReason::OperatorResumed,
    ] {
        let event = events.iter().find(|event| event.reason == reason).unwrap();
        assert_eq!(event.actor_kind, TaskTransitionActorKind::Operator);
        assert_eq!(event.actor_id, Some(owner.id));
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Row-local attribution is a write contract: every status-changing statement states all five
/// columns. The failure it guards against is silent -- a statement that omits one carries the
/// previous transition's value into the new row version, and the ledger records an actor that
/// had nothing to do with the change. This walks a task through the actor kinds in the order
/// most likely to expose that, and checks the boundaries where the kind changes.
#[tokio::test]
async fn consecutive_transitions_never_inherit_the_previous_actor_or_source() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();
    let task = enqueue_chain(&persistence, company.id, channel.id, "inheritance").await;

    // worker -> approval.
    let first_worker = Uuid::new_v4();
    let lease = claim_as(&persistence, task.id, first_worker).await;
    let approval_id = park_for_approval(&persistence, &company, &channel, lease).await;

    // approval -> worker: the claim must not carry the approval id that parked the task.
    persistence
        .resume_task(task.id, ResumeActor::Approval(approval_id))
        .await
        .unwrap();
    let second_worker = Uuid::new_v4();
    let lease = claim_as(&persistence, task.id, second_worker).await;

    // worker -> operator.
    persistence
        .mark_task_failed(TaskFailure {
            lease,
            error: "inheritance check",
            next_run_at: Utc::now(),
            outcome: TaskFailureOutcome::Retry,
            reason: TaskStopReason::RetryableFailure,
        })
        .await
        .unwrap();
    persistence
        .stop_task(task.id, StopActor::Operator(operator))
        .await
        .unwrap();

    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let expected = [
        (
            TaskTransitionReason::Enqueued,
            TaskTransitionActorKind::System,
            None,
            None,
            None,
        ),
        (
            TaskTransitionReason::Claimed,
            TaskTransitionActorKind::Worker,
            Some(first_worker),
            None,
            None,
        ),
        (
            TaskTransitionReason::ApprovalRequested,
            TaskTransitionActorKind::Approval,
            None,
            Some(approval_id),
            None,
        ),
        (
            TaskTransitionReason::ApprovalAccepted,
            TaskTransitionActorKind::Approval,
            None,
            Some(approval_id),
            None,
        ),
        (
            TaskTransitionReason::Claimed,
            TaskTransitionActorKind::Worker,
            Some(second_worker),
            None,
            None,
        ),
        (
            TaskTransitionReason::RetryableFailure,
            TaskTransitionActorKind::Worker,
            Some(second_worker),
            None,
            None,
        ),
        (
            TaskTransitionReason::OperatorStopped,
            TaskTransitionActorKind::Operator,
            Some(operator),
            None,
            None,
        ),
    ];
    assert_eq!(events.len(), expected.len(), "{events:#?}");
    for (event, (reason, kind, actor_id, approval, outreach)) in events.iter().zip(expected) {
        assert_eq!(event.reason, reason, "{event:#?}");
        assert_eq!(event.actor_kind, kind, "{event:#?}");
        assert_eq!(event.actor_id, actor_id, "{event:#?}");
        assert_eq!(event.related_approval_id, approval, "{event:#?}");
        assert_eq!(event.related_outreach_id, outreach, "{event:#?}");
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Spend the whole retry budget and land the task in `dead_letter`, the way the worker does:
/// one failure per claim until the next one has nowhere left to go.
async fn exhaust_retry_budget(persistence: &PostgresPersistence, task: &BackgroundTask) {
    for attempt in 1..=task.max_retries {
        let lease = claim(persistence, task.id).await;
        let outcome = if attempt == task.max_retries {
            TaskFailureOutcome::DeadLetter
        } else {
            TaskFailureOutcome::Retry
        };
        assert!(
            persistence
                .mark_task_failed(TaskFailure {
                    lease,
                    error: "budget check",
                    next_run_at: Utc::now(),
                    outcome,
                    reason: TaskStopReason::RetryableFailure,
                })
                .await
                .unwrap()
        );
    }
    let exhausted = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(exhausted.status, TaskStatus::DeadLetter);
    assert!(
        exhausted.retry_count >= exhausted.max_retries,
        "the budget is spent: {exhausted:#?}"
    );
}

/// Every transition this chain made named its own cause. `unknown` is what the trigger records
/// when none did, so finding one here means a status write in the path under test is still
/// silent -- scoped to this chain, because the test database is shared and runs in parallel.
fn assert_no_unclassified_transitions(events: &[TaskStatusEvent]) {
    let unclassified: Vec<_> = events
        .iter()
        .filter(|event| event.reason == TaskTransitionReason::Unknown)
        .collect();
    assert!(
        unclassified.is_empty(),
        "transitions recorded no cause: {unclassified:#?}"
    );
}

/// Resume on a dead-lettered task has to hand back the retry budget, or it is theatre: the row
/// moves to `pending`, the worker claims it, the first failure computes
/// `retry_count + 1 >= max_retries` against a count that is already spent, and the task
/// dead-letters again having achieved nothing durable.
#[tokio::test]
async fn operator_resume_of_a_dead_lettered_task_restores_its_retry_budget() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();
    let task = enqueue_chain(&persistence, company.id, channel.id, "dead_letter_resume").await;
    exhaust_retry_budget(&persistence, &task).await;

    let resumed = persistence
        .resume_task(task.id, ResumeActor::Operator(operator))
        .await
        .unwrap();

    assert_eq!(resumed.status, TaskStatus::Pending);
    assert_eq!(resumed.retry_count, 0, "the budget is fresh: {resumed:#?}");
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let resume_event = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::OperatorResumed)
        .expect("the resume is attributed to the operator, not guessed from the status pair");
    assert_eq!(resume_event.actor_kind, TaskTransitionActorKind::Operator);
    assert_eq!(resume_event.actor_id, Some(operator));
    assert_eq!(resume_event.from_status, Some(TaskStatus::DeadLetter));
    assert_no_unclassified_transitions(&events);

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// The Tasks page offers Resume on `stopped` as well as `dead_letter`, so an operator who stops
/// an exhausted task before resuming it reaches the same intent by a different route. Keying
/// the reset on the status alone would miss this one; keying it on the spent budget catches it.
#[tokio::test]
async fn operator_resume_of_a_stopped_exhausted_task_restores_its_retry_budget() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();
    let task = enqueue_chain(&persistence, company.id, channel.id, "stopped_resume").await;
    exhaust_retry_budget(&persistence, &task).await;
    let stopped = persistence
        .stop_task(task.id, StopActor::Operator(operator))
        .await
        .unwrap();
    assert_eq!(stopped.status, TaskStatus::Stopped);
    assert!(stopped.retry_count >= stopped.max_retries);

    let resumed = persistence
        .resume_task(task.id, ResumeActor::Operator(operator))
        .await
        .unwrap();

    assert_eq!(resumed.status, TaskStatus::Pending);
    assert_eq!(resumed.retry_count, 0, "the budget is fresh: {resumed:#?}");
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    assert_no_unclassified_transitions(&events);

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// An approval releasing a parked task is the same attempt carrying on, not a retry. Resetting
/// here would hand a task an unlimited budget for the price of one approval round trip.
#[tokio::test]
async fn approval_resume_continues_the_attempt_without_refunding_the_budget() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "approval_resume").await;
    let lease = claim(&persistence, task.id).await;
    assert!(
        persistence
            .mark_task_failed(TaskFailure {
                lease,
                error: "one attempt already spent",
                next_run_at: Utc::now(),
                outcome: TaskFailureOutcome::Retry,
                reason: TaskStopReason::RetryableFailure,
            })
            .await
            .unwrap()
    );
    let lease = claim(&persistence, task.id).await;
    let approval_id = park_for_approval(&persistence, &company, &channel, lease).await;
    let parked = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(parked.status, TaskStatus::PendingApproval);

    let resumed = persistence
        .resume_task(task.id, ResumeActor::Approval(approval_id))
        .await
        .unwrap();

    assert_eq!(resumed.status, TaskStatus::Pending);
    assert_eq!(
        resumed.retry_count, parked.retry_count,
        "a continuation spends nothing and refunds nothing: {resumed:#?}"
    );
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let resume_event = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::ApprovalAccepted)
        .expect("the approval is what released the task");
    assert_eq!(resume_event.actor_kind, TaskTransitionActorKind::Approval);
    assert_eq!(resume_event.related_approval_id, Some(approval_id));
    assert_no_unclassified_transitions(&events);

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Each resume cause may only act on the states it can answer for. An approval must not
/// resurrect a task that was abandoned after exhausting its retries, and an operator must not
/// walk a task past the approval gate it is parked behind. Both mismatches match no row.
#[tokio::test]
async fn a_resume_cause_used_against_the_wrong_state_changes_nothing() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();

    let abandoned = enqueue_chain(&persistence, company.id, channel.id, "wrong_state").await;
    exhaust_retry_budget(&persistence, &abandoned).await;
    assert!(
        persistence
            .resume_task(abandoned.id, ResumeActor::Approval(Uuid::new_v4()))
            .await
            .is_err(),
        "an approval cannot resume work it never parked"
    );
    let still_dead = persistence
        .get_task_by_id(abandoned.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still_dead.status, TaskStatus::DeadLetter);

    let parked = enqueue_chain(&persistence, company.id, channel.id, "wrong_state").await;
    let lease = claim(&persistence, parked.id).await;
    park_for_approval(&persistence, &company, &channel, lease).await;
    assert!(
        persistence
            .resume_task(parked.id, ResumeActor::Operator(operator))
            .await
            .is_err(),
        "an operator resume must not bypass the approval the task is waiting on"
    );
    let still_parked = persistence
        .get_task_by_id(parked.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still_parked.status, TaskStatus::PendingApproval);

    let events = persistence
        .list_task_status_events(company.id, parked.correlation_id, None, 50)
        .await
        .unwrap();
    assert!(
        !events
            .iter()
            .any(|event| event.reason == TaskTransitionReason::OperatorResumed),
        "a resume that matched no row writes no event: {events:#?}"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// The outreach -> operator boundary, which the sequence above does not reach: an outreach
/// names its own row as the transition's source, and the operator stop that follows must not
/// inherit it.
#[tokio::test]
async fn an_operator_stop_after_an_outreach_drops_the_outreach_source() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let operator = Uuid::new_v4();
    let task = enqueue_chain(&persistence, company.id, channel.id, "outreach-operator").await;
    let worker_id = Uuid::new_v4();
    claim_as(&persistence, task.id, worker_id).await;

    let outreach_id = Uuid::new_v4();
    let progress = persistence
        .create_outreach_and_pause(CreateOutreachRequest {
            correlation_id: task.correlation_id,
            id: outreach_id,
            task_id: task.id,
            company_id: company.id,
            channel_id: channel.id,
            worker_id,
            outreach_key: "attribution-outreach".into(),
            required_threshold_percent: 100.0,
            expires_at: Utc::now() + chrono::Duration::hours(24),
            subject: "Question".into(),
            body: "Please respond".into(),
            targets: Vec::new(),
        })
        .await
        .unwrap();
    assert!(progress.suspended);

    persistence
        .stop_task(task.id, StopActor::Operator(operator))
        .await
        .unwrap();

    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let started = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::OutreachStarted)
        .expect("the outreach parked the task");
    assert_eq!(started.actor_kind, TaskTransitionActorKind::Outreach);
    assert_eq!(started.related_outreach_id, Some(outreach_id));
    assert_eq!(started.actor_id, None);

    let stopped = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::OperatorStopped)
        .expect("the operator stopped the task");
    assert_eq!(stopped.actor_kind, TaskTransitionActorKind::Operator);
    assert_eq!(stopped.actor_id, Some(operator));
    assert_eq!(
        stopped.related_outreach_id, None,
        "the operator stop must not inherit the outreach that parked the task"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// A rejected approval is an approval acting, and it may only end the task that approval
/// parked. Against any other state it must change nothing rather than reaching for work that
/// has since moved on.
#[tokio::test]
async fn an_approval_rejection_stops_only_the_task_it_parked() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "approval-reject").await;
    let lease = claim_as(&persistence, task.id, Uuid::new_v4()).await;
    let approval_id = park_for_approval(&persistence, &company, &channel, lease).await;

    persistence
        .stop_task(task.id, StopActor::Approval(approval_id))
        .await
        .unwrap();
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    let rejected = events
        .iter()
        .find(|event| event.reason == TaskTransitionReason::ApprovalRejected)
        .expect("the rejection stopped the task");
    assert_eq!(rejected.actor_kind, TaskTransitionActorKind::Approval);
    assert_eq!(rejected.related_approval_id, Some(approval_id));
    assert_eq!(rejected.actor_id, None);

    // The task is `stopped` now, which an approval rejection may not act on.
    let before = events.len();
    assert!(
        persistence
            .stop_task(task.id, StopActor::Approval(approval_id))
            .await
            .is_err(),
        "a rejection must not stop a task it did not park"
    );
    let after = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 50)
        .await
        .unwrap();
    assert_eq!(
        after.len(),
        before,
        "a matched-nothing stop writes no event"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// The database refuses attribution whose actor kind and ids disagree, so a call site cannot
/// record an approval-driven change as an operator's, or name two sources at once.
#[tokio::test]
async fn the_task_row_rejects_attribution_whose_shape_contradicts_its_actor_kind() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let task = enqueue_chain(&persistence, company.id, channel.id, "shape-check").await;

    // (what is wrong with it, reason, actor kind, actor id, approval id, outreach id)
    let refused = [
        (
            "an approval without the approval it names",
            "'approval_rejected'",
            "'approval'",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "an outreach without the outreach it names",
            "'outreach_started'",
            "'outreach'",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "an operator with no id",
            "'operator_stopped'",
            "'operator'",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "a worker with no id",
            "'retryable_failure'",
            "'worker'",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "the system claiming an actor",
            "'operator_stopped'",
            "'system'",
            "gen_random_uuid()",
            "NULL",
            "NULL",
        ),
        (
            "an operator carrying an approval",
            "'operator_stopped'",
            "'operator'",
            "gen_random_uuid()",
            "gen_random_uuid()",
            "NULL",
        ),
        (
            "two sources at once",
            "'approval_rejected'",
            "'approval'",
            "NULL",
            "gen_random_uuid()",
            "gen_random_uuid()",
        ),
        (
            "a reason with no actor kind at all",
            "'operator_stopped'",
            "NULL",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "an actor with no reason",
            "NULL",
            "'operator'",
            "gen_random_uuid()",
            "NULL",
            "NULL",
        ),
    ];
    for (case, reason, kind, actor_id, approval_id, outreach_id) in refused {
        let outcome = sqlx::query(&format!(
            "UPDATE background_tasks
                    SET status = 'stopped',
                        transition_reason = {reason},
                        transition_actor_kind = {kind},
                        transition_actor_id = {actor_id},
                        transition_approval_id = {approval_id},
                        transition_outreach_id = {outreach_id}
                  WHERE id = $1"
        ))
        .bind(task.id)
        .execute(&pool)
        .await;
        assert!(outcome.is_err(), "{case} must be refused");
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn pending_claims_take_one_company_round_before_a_second_task_from_a_backlog() {
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("fair_owner_{suffix}@example.com");
    persistence
        .create_user(&format!("fair_owner_{suffix}"), &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();

    let mut companies = Vec::new();
    let mut channels = Vec::new();
    for label in ["backlog", "waiting"] {
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: format!("Fair {label}"),
                slug: format!("fair-{label}-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: label.into(),
                slug: label.into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        companies.push(company);
        channels.push(channel);
    }

    let mut backlog_ids = Vec::new();
    for _ in 0..3 {
        backlog_ids.push(
            persistence
                .enqueue_task(NewTask::starting_new_chain(
                    companies[0].id,
                    channels[0].id,
                    None,
                    "fairness",
                    serde_json::json!({}),
                ))
                .await
                .unwrap()
                .id,
        );
    }
    let waiting = persistence
        .enqueue_task(NewTask::starting_new_chain(
            companies[1].id,
            channels[1].id,
            None,
            "fairness",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let mut owned_ids = backlog_ids.clone();
    owned_ids.push(waiting.id);
    sqlx::query(
            "UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP - INTERVAL '300 years' WHERE id = ANY($1)",
        )
        .bind(&owned_ids)
        .execute(&pool)
        .await
        .unwrap();

    let claimed = persistence
        .claim_pending_tasks(Uuid::new_v4(), Utc::now() + chrono::Duration::minutes(5), 2)
        .await
        .unwrap();
    assert_eq!(
        claimed
            .iter()
            .filter(|task| task.company_id == companies[0].id)
            .count(),
        1
    );
    assert_eq!(
        claimed
            .iter()
            .filter(|task| task.company_id == companies[1].id)
            .count(),
        1
    );

    for company in companies {
        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }
}

/// The mailbox asks for a whole page of threads at once, and each thread must report the state
/// of its *current* run rather than whichever task happens to be found first.
#[tokio::test]
async fn thread_activity_reports_the_latest_task_per_thread() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("activity_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Activity Test".to_string(),
            slug: format!("activity-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Activity".into(),
            slug: "activity".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());

    let mut threads = Vec::new();
    for subject in ["running", "blocked", "finished", "superseded"] {
        threads.push(
            persistence
                .create_thread(channel.id, subject, std::slice::from_ref(&email_addr))
                .await
                .unwrap(),
        );
    }

    let enqueue = async |thread_id: Uuid| {
        persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread_id),
                "email_agent_dispatch",
                serde_json::json!({}),
            ))
            .await
            .unwrap()
    };

    let running = enqueue(threads[0].id).await;
    let blocked = enqueue(threads[1].id).await;
    let finished = enqueue(threads[2].id).await;
    let old = enqueue(threads[3].id).await;
    let current = enqueue(threads[3].id).await;

    // `background_tasks_lease_check` gives the lease columns to `processing` rows and to no
    // other status, so they move together here exactly as the worker moves them.
    let set_status = async |id: Uuid, status: &str, lease: Option<DateTime<Utc>>| {
        sqlx::query(&format!(
                "UPDATE background_tasks
                 SET {CLEAR_TRANSITION},
                     status = $2,
                     lock_expires_at = $3,
                     worker_id = CASE WHEN $3::timestamptz IS NULL THEN NULL ELSE gen_random_uuid() END,
                     execution_generation =
                         CASE WHEN $3::timestamptz IS NULL THEN NULL ELSE gen_random_uuid() END,
                     -- Derived from the lease, not from now: the check also demands
                     -- lock_expires_at > locked_at, and an expired lease is set in the past.
                     locked_at = $3::timestamptz - interval '10 minutes',
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = $1"
            ))
            .bind(id)
            .bind(status)
            .bind(lease)
            .execute(&pool)
            .await
            .unwrap();
    };

    let live_lease = Utc::now() + chrono::Duration::minutes(5);
    set_status(running.id, "processing", Some(live_lease)).await;
    set_status(blocked.id, "pending_approval", None).await;
    set_status(finished.id, "completed", None).await;
    // Older task on the same thread ended badly; the newer one is what the reader should see.
    set_status(old.id, "dead_letter", None).await;
    set_status(current.id, "processing", Some(live_lease)).await;

    let ids: Vec<Uuid> = threads.iter().map(|thread| thread.id).collect();
    let activity = persistence.list_thread_activity(&ids).await.unwrap();

    assert_eq!(activity.get(&threads[0].id), Some(&ThreadActivity::Working));
    assert_eq!(
        activity.get(&threads[1].id),
        Some(&ThreadActivity::WaitingApproval)
    );
    assert_eq!(
        activity.get(&threads[2].id),
        None,
        "a finished thread reports nothing at all, rather than an idle badge"
    );
    assert_eq!(
        activity.get(&threads[3].id),
        Some(&ThreadActivity::Working),
        "the newest task wins over an older dead letter on the same thread"
    );

    // Asking again after a failure and getting an answer settles the thread: the run that
    // worked is its last word, and the dead letter behind it is history rather than a badge.
    set_status(current.id, "completed", None).await;
    let activity = persistence.list_thread_activity(&ids).await.unwrap();
    assert_eq!(
        activity.get(&threads[3].id),
        None,
        "a successful run buries the dead letter it was asked to make up for"
    );

    // The failure still stands on its own while nothing has answered it.
    set_status(current.id, "stopped", None).await;
    let activity = persistence.list_thread_activity(&ids).await.unwrap();
    assert_eq!(
        activity.get(&threads[3].id),
        Some(&ThreadActivity::Failed),
        "a run that was stopped rather than answered leaves the failure showing"
    );
    set_status(current.id, "completed", None).await;

    // An abandoned worker leaves `processing` behind; that is queued work, not a live agent.
    set_status(
        running.id,
        "processing",
        Some(Utc::now() - chrono::Duration::minutes(5)),
    )
    .await;
    let activity = persistence.list_thread_activity(&ids).await.unwrap();
    assert_eq!(activity.get(&threads[0].id), Some(&ThreadActivity::Queued));

    assert!(
        persistence
            .list_thread_activity(&[])
            .await
            .unwrap()
            .is_empty(),
        "an empty page must not hit the database"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// An expired lease must cost an attempt, close its ledger row, and back the task off --
/// and eventually dead-letter it.
///
/// Regression for the shape where `claim_pending_tasks` stole expired `processing` rows
/// directly. That re-ran the task with `retry_count` untouched, left the abandoned attempt
/// sitting in `task_attempts` as `processing` for ever, and applied no backoff, so a task
/// that reliably outlived its lease was retried in a tight loop and never dead-lettered.
#[tokio::test]
async fn an_expired_task_lease_costs_an_attempt_and_eventually_dead_letters() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("reaper_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Reaper Test".to_string(),
            slug: format!("reaper-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Reaper".into(),
            slug: "reaper".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
    let thread = persistence
        .create_thread(channel.id, "Reaper", std::slice::from_ref(&email_addr))
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    // Run the task up to its retry ceiling, losing the lease every time.
    let max_retries = task.max_retries;
    let mut generations = Vec::new();
    for attempt in 1..=max_retries {
        // Due now, whatever backoff the previous reap applied.
        sqlx::query("UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(task.id)
            .execute(&pool)
            .await
            .unwrap();

        // By id rather than a batch claim: the batch sweeps the whole queue, so tests
        // running beside this one would fill it or take this row first.
        assert!(
            persistence
                .claim_task(
                    task.id,
                    Uuid::new_v4(),
                    Utc::now() + chrono::Duration::minutes(5)
                )
                .await
                .unwrap(),
            "the task is pending and due, so it must be claimable"
        );
        let claimed = persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .expect("the task still exists");
        let lease = TaskLeaseRef::of(&claimed).expect("a claim records its lease");
        generations.push(lease.execution_generation);
        persistence
            .begin_task_attempt(TaskAttemptRef::of(&claimed, lease))
            .await
            .unwrap();

        // The run vanishes: its lease lapses with nothing reported.
        sqlx::query(
            "UPDATE background_tasks
                 SET locked_at = CURRENT_TIMESTAMP - interval '20 minutes',
                     lock_expires_at = CURRENT_TIMESTAMP - interval '1 second'
                 WHERE id = $1",
        )
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();

        // The sweep is global, so a test running beside this one may reap this row first.
        // What matters is the state the row ends in, not whose call got there.
        persistence.reap_expired_task_leases().await.unwrap();

        let after = persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .expect("the task still exists");
        assert_eq!(
            after.retry_count, attempt,
            "each lapsed lease must spend exactly one attempt"
        );
        assert!(after.worker_id.is_none());
        assert!(after.execution_generation.is_none());

        if attempt < max_retries {
            assert_eq!(after.status, TaskStatus::Pending);
            assert!(
                after.run_at > Utc::now(),
                "a reaped task must wait out its backoff"
            );
        } else {
            assert_eq!(
                after.status,
                TaskStatus::DeadLetter,
                "the attempt budget is spent, so the task must stop rather than loop"
            );
        }
    }

    // Every claim minted its own generation, which is what fences a superseded run.
    let unique = generations.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(
        unique.len(),
        generations.len(),
        "each claim must mint a distinct execution generation"
    );

    // No attempt was left open: the reaper closed each one as it went.
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_attempts WHERE task_id = $1 AND status = 'processing'",
    )
    .bind(task.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(open, 0, "a reaped run must not leave its ledger row open");
    let lease_lost: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_attempts WHERE task_id = $1 AND stop_reason = $2",
    )
    .bind(task.id)
    .bind(TaskStopReason::LeaseLost.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        lease_lost,
        i64::from(task.max_retries),
        "every reaped attempt records why execution stopped"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// A run whose lease was reaped must not be able to write anything, even if the same worker
/// id re-claims the task. Only the generation can tell those two runs apart.
#[tokio::test]
async fn a_superseded_run_cannot_renew_write_or_close_the_task() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("fence_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Fence Test".to_string(),
            slug: format!("fence-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Fence".into(),
            slug: "fence".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
    let thread = persistence
        .create_thread(channel.id, "Fence", std::slice::from_ref(&email_addr))
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    // Deliberately the *same* worker both times, so only the generation differs. This is the
    // case a `worker_id = $me` guard cannot catch.
    let worker = Uuid::new_v4();

    // By id rather than a batch claim: the batch sweeps the whole queue, so a test running
    // beside this one could take this row first.
    assert!(
        persistence
            .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    let first = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let stale = TaskLeaseRef::of(&first).expect("a claim records its lease");

    sqlx::query(
        "UPDATE background_tasks
             SET locked_at = CURRENT_TIMESTAMP - interval '20 minutes',
                 lock_expires_at = CURRENT_TIMESTAMP - interval '1 second'
             WHERE id = $1",
    )
    .bind(task.id)
    .execute(&pool)
    .await
    .unwrap();
    persistence.reap_expired_task_leases().await.unwrap();

    sqlx::query("UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        persistence
            .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    let second = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let current = TaskLeaseRef::of(&second).expect("a claim records its lease");

    assert_eq!(stale.worker_id, current.worker_id, "same worker both times");
    assert_ne!(stale.execution_generation, current.execution_generation);

    // Nothing the superseded run tries may land.
    assert!(
        !persistence
            .renew_task_lease(stale, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    assert!(!persistence.mark_task_completed(stale).await.unwrap());
    assert!(
        !persistence
            .mark_task_failed(TaskFailure {
                lease: stale,
                error: "stale",
                next_run_at: Utc::now(),
                outcome: TaskFailureOutcome::Retry,
                reason: TaskStopReason::RetryableFailure,
            })
            .await
            .unwrap()
    );

    // The payload the superseded run tried to write never landed.
    let after = persistence
        .get_task_by_id(task.id)
        .await
        .unwrap()
        .expect("the task still exists");
    assert_eq!(after.status, TaskStatus::Processing);
    assert!(after.payload.get("stale").is_none());

    // The run that actually owns the task is unaffected.
    assert!(persistence.mark_task_completed(current).await.unwrap());

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// One dispatch's reply, its outbox row and its task payload land together or not at all.
///
/// They used to be three independent commits: the outbox row, then a `create_message` per
/// answered thread, then the payload. A crash or a lost lease part-way left a thread showing
/// an answer that was never sent, or an email going out for a task whose payload said it had
/// never run -- and the retry then had to reconcile the difference.
#[tokio::test]
async fn a_dispatch_commits_its_reply_outbox_row_and_payload_together_or_not_at_all() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("commit_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Commit Test".to_string(),
            slug: format!("commit-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Commit".into(),
            slug: "commit".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
    let thread = persistence
        .create_thread(channel.id, "Commit", std::slice::from_ref(&email_addr))
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    let worker = Uuid::new_v4();
    assert!(
        persistence
            .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    let claimed = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    let lease = TaskLeaseRef::of(&claimed).expect("a claim records its lease");

    let reply = |message_id: &str| Message {
        id: Uuid::new_v4(),
        thread_id: thread.id,
        message_id: MessageId::from(message_id.to_string()),
        in_reply_to: None,
        references_list: Vec::new(),
        sender: crate::entities::value_objects::EmailAddress::from("agent@example.com"),
        recipients_to: vec![email_addr.clone()],
        recipients_cc: Vec::new(),
        subject: "Re: Commit".to_string(),
        clean_text_body: "the answer".to_string(),
        raw_text_body: None,
        raw_html_body: None,
        attachments: None,
        direction: MessageDirection::Outbound,
        role: MessageRole::Agent,
        thread_index: None,
        created_at: Utc::now(),
    };
    let send = |key: &str| OutboundSend {
        correlation_id: CorrelationId::new(),
        company_id: company.id,
        channel_id: channel.id,
        task_id: Some(task.id),
        idempotency_key: key.to_string(),
        payload: serde_json::json!({"body": "the answer"}),
    };

    let outbound_rows = async || -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM email_outbox WHERE task_id = $1")
            .bind(task.id)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let thread_rows = async || -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM thread_messages WHERE thread_id = $1 AND direction = 'outbound'",
        )
        .bind(thread.id)
        .fetch_one(&pool)
        .await
        .unwrap()
    };

    // A superseded run: the same task and worker, a generation that is no longer current.
    let stale = TaskLeaseRef {
        execution_generation: Uuid::new_v4(),
        ..lease
    };
    let outcome = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease: stale,
            messages: &[reply("<stale@example.com>")],
            outbound: Some(send("stale-key")),
            payload: serde_json::json!({"stale": true}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    assert_eq!(outcome, DispatchCommit::LeaseLost);

    // Not one of the three parts may have landed.
    assert_eq!(thread_rows().await, 0, "no reply may be stored");
    assert_eq!(outbound_rows().await, 0, "no email may be queued");
    let after_stale = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert!(
        after_stale.payload.get("stale").is_none(),
        "no payload may be written"
    );

    // The run that actually owns the lease commits all three.
    let outcome = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease,
            messages: &[reply("<live@example.com>")],
            outbound: Some(send("live-key")),
            payload: serde_json::json!({"committed": true}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        DispatchCommit::Committed { outbox_id: Some(_) }
    ));
    assert_eq!(thread_rows().await, 1);
    assert_eq!(outbound_rows().await, 1);
    let after = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert_eq!(
        after.payload.get("committed"),
        Some(&serde_json::json!(true))
    );

    // Re-queueing the same logical send is the idempotency key doing its job, not a failure,
    // and it must not duplicate the outbox row.
    let outcome = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease,
            messages: &[],
            outbound: Some(send("live-key")),
            payload: serde_json::json!({"committed": true}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    assert_eq!(outcome, DispatchCommit::Committed { outbox_id: None });
    assert_eq!(
        outbound_rows().await,
        1,
        "the same send must not queue twice"
    );

    // A failure part-way must roll back what already succeeded in the same transaction. The
    // payload write happens first, so a message that cannot be stored has to undo it: this is
    // the case three separate commits could not handle at all.
    let orphan = Message {
        thread_id: Uuid::new_v4(),
        ..reply("<orphan@example.com>")
    };
    let failed = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease,
            messages: &[orphan],
            outbound: Some(send("orphan-key")),
            payload: serde_json::json!({"rolled_back": true}),
            complete_outreach: false,
        })
        .await;
    assert!(failed.is_err(), "a message with no thread cannot be stored");

    let after_rollback = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert!(
        after_rollback.payload.get("rolled_back").is_none(),
        "the payload write must be rolled back when a later write fails"
    );
    assert_eq!(
        after_rollback.payload.get("committed"),
        Some(&serde_json::json!(true)),
        "and the previously committed payload must survive untouched"
    );
    assert_eq!(
        outbound_rows().await,
        1,
        "the failed dispatch must not leave an outbox row behind"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn concurrent_workers_claim_once_and_a_failed_task_is_not_immediately_reclaimed() {
    // Both claims below are unscoped, and the row is deliberately sorted to the very front of
    // the queue -- which makes it the first thing any *other* unscoped claim takes too. Held
    // from before the row is queued until after the last claim, so the only claims racing for
    // it are this test's own two.
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("queue_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Queue Test".to_string(),
            slug: format!("queue-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Queue".into(),
            slug: "queue".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
    let thread = persistence
        .create_thread(channel.id, "Queue", std::slice::from_ref(&email_addr))
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    // `claim_pending_tasks` polls the whole queue, not this company's slice of it. Its first
    // sort key is the per-company round, and a brand-new company's only task is always round 1
    // — so sorting this row ahead of every other round-1 row is what puts it first overall,
    // ahead of concurrent tests' rows and any orphans a previously aborted run left behind.
    // Without it both single-slot workers fill up elsewhere and never reach this task. Keeping
    // the limit at 1 also means this test steals at most one foreign task.
    sqlx::query(
            "UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP - INTERVAL '100 years' WHERE id = $1",
        )
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();

    let first_worker = Uuid::new_v4();
    let second_worker = Uuid::new_v4();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
    let (first, second) = tokio::join!(
        persistence.claim_pending_tasks(first_worker, expires_at, 1),
        persistence.claim_pending_tasks(second_worker, expires_at, 1)
    );
    let claimed: Vec<_> = first.unwrap().into_iter().chain(second.unwrap()).collect();

    // The invariant that matters is that *this* task went to exactly one worker — asserting on
    // the combined queue total would count whatever else the other worker legitimately claimed.
    assert_eq!(
        claimed.iter().filter(|claim| claim.id == task.id).count(),
        1,
        "a pending task must be claimed by exactly one worker"
    );
    let events = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.reason == TaskTransitionReason::Claimed)
            .count(),
        1,
        "competing claimants must produce one claimed transition event"
    );

    // Two claims, one of which is ours by construction — so the other necessarily took someone
    // else's task, and it now holds a five-minute lease on it. Hand it straight back: whichever
    // test queued it is about to find its own task already claimed and fail on a state it never
    // set. Releasing to 'pending' is where a reaped lease lands anyway.
    let borrowed: Vec<Uuid> = claimed
        .iter()
        .map(|claim| claim.id)
        .filter(|id| *id != task.id)
        .collect();
    if !borrowed.is_empty() {
        sqlx::query(&format!(
            "UPDATE background_tasks
                    SET {CLEAR_TRANSITION}, status = 'pending', worker_id = NULL,
                        execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL
                  WHERE id = ANY($1)"
        ))
        .bind(&borrowed)
        .execute(&pool)
        .await
        .unwrap();
    }

    // This single row fills the worker's one-task batch. Failing it must move it behind
    // persisted backoff before `MoreWaiting` sends the worker straight into another
    // iteration; otherwise a poison task is reclaimed without any clock advance.
    let claimed_task = claimed
        .iter()
        .find(|claim| claim.id == task.id)
        .expect("this task was claimed");
    let claimed_lease = TaskLeaseRef::of(claimed_task).expect("a claimed task records its lease");
    assert!(
        persistence
            .mark_task_failed(TaskFailure {
                lease: claimed_lease,
                error: "poison task",
                next_run_at: Utc::now() + chrono::Duration::minutes(1),
                outcome: TaskFailureOutcome::Retry,
                reason: TaskStopReason::RetryableFailure,
            })
            .await
            .unwrap()
    );

    let immediate_worker = Uuid::new_v4();
    let immediate = persistence
        .claim_pending_tasks(
            immediate_worker,
            Utc::now() + chrono::Duration::minutes(5),
            1,
        )
        .await
        .unwrap();
    assert!(
        immediate.iter().all(|claim| claim.id != task.id),
        "a failed full batch must not reclaim the same task on the zero-delay iteration"
    );
    let immediate_borrowed: Vec<Uuid> = immediate.iter().map(|claim| claim.id).collect();
    if !immediate_borrowed.is_empty() {
        sqlx::query(&format!(
            "UPDATE background_tasks
                    SET {CLEAR_TRANSITION}, status = 'pending', worker_id = NULL,
                        execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL
                  WHERE id = ANY($1) AND worker_id = $2"
        ))
        .bind(&immediate_borrowed)
        .bind(immediate_worker)
        .execute(&pool)
        .await
        .unwrap();
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn only_one_worker_queues_an_outbound_send() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("enqueue_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Enqueue Test".to_string(),
            slug: format!("enqueue-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();

    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Enqueue".into(),
            slug: "enqueue".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();

    let key = format!("task:{suffix}:agent-reply");
    let send = || OutboundSend {
        correlation_id: CorrelationId::new(),
        company_id: company.id,
        channel_id: channel.id,
        task_id: None,
        idempotency_key: key.clone(),
        payload: serde_json::json!({}),
    };

    // Two workers race to hand the transport the same logical reply; only one may queue it,
    // or the customer receives the answer twice.
    let (first, second) = tokio::join!(
        persistence.enqueue_outbound_send(send()),
        persistence.enqueue_outbound_send(send())
    );
    let queued: Vec<_> = [first.unwrap(), second.unwrap()]
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(
        queued.len(),
        1,
        "the unique idempotency key must admit exactly one send"
    );

    // Put the row out of reach before asking what state it is in. Claiming is unscoped by
    // design — `claim_outbox_emails` takes any `pending` row whose `available_at` has arrived,
    // because that is what a real poller does — so a concurrent test would otherwise claim this
    // row and the assertions below would be reporting on that, not on what queueing did.
    // Pushing `available_at` out excludes it from every claim set without changing the columns
    // under test.
    sqlx::query(
        "UPDATE email_outbox SET available_at = CURRENT_TIMESTAMP + interval '1 hour'
             WHERE id = $1",
    )
    .bind(queued[0])
    .execute(&pool)
    .await
    .unwrap();

    // The row is left for the poller to claim: 'pending', unleased, and not owned by anyone.
    let (status, worker_id): (String, Option<Uuid>) =
        sqlx::query_as("SELECT status, worker_id FROM email_outbox WHERE id = $1")
            .bind(queued[0])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "pending");
    assert!(
        worker_id.is_none(),
        "queueing must not claim the row; the outbox poller does that"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_outbox_lists_one_channel_at_a_time() {
    let Some(pool) = test_pool().await else {
        return;
    };

    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("outbox_channel_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Outbox Channel Test".to_string(),
            slug: format!("outbox-channel-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();

    let mut channels = Vec::new();
    for name in ["Support", "Billing"] {
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: name.into(),
                slug: name.to_lowercase(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        persistence
            .enqueue_outbound_send(OutboundSend {
                correlation_id: CorrelationId::new(),
                company_id: company.id,
                channel_id: channel.id,
                task_id: None,
                idempotency_key: format!("{suffix}:{name}"),
                payload: serde_json::json!({ "subject": name }),
            })
            .await
            .unwrap()
            .unwrap();
        channels.push(channel);
    }

    // Unfiltered, the company sees both channels' mail.
    let all = persistence
        .list_company_outbox_page(company.id, None, None, false, 0, 50)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);

    // Filtered, it sees exactly one — the whole point of promoting the channel out of the
    // payload and indexing it.
    let support = persistence
        .list_company_outbox_page(company.id, Some(channels[0].id), None, false, 0, 50)
        .await
        .unwrap();
    assert_eq!(support.len(), 1);
    assert_eq!(support[0].channel_id, Some(channels[0].id));
    assert_eq!(support[0].subject(), Some("Support"));

    // Deleting the channel must not delete the record that mail went out for it.
    ChannelPersistence::delete(&persistence, channels[0].id)
        .await
        .unwrap();
    let orphaned = persistence
        .list_company_outbox_page(company.id, None, None, false, 0, 50)
        .await
        .unwrap();
    assert_eq!(orphaned.len(), 2);
    assert!(
        orphaned
            .iter()
            .any(|entry| entry.subject() == Some("Support") && entry.channel_id.is_none()),
        "a deleted channel must null the column, not cascade the send record away"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn failed_outbox_batch_is_backed_off_and_expired_leases_reach_the_cap() {
    let Some(pool) = test_pool().await else {
        return;
    };

    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("reap_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Reap Test".to_string(),
            slug: format!("reap-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Reap".into(),
            slug: "reap".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();

    let outbox_id = persistence
        .enqueue_outbound_send(OutboundSend {
            correlation_id: CorrelationId::new(),
            company_id: company.id,
            channel_id: channel.id,
            task_id: None,
            idempotency_key: format!("reap:{suffix}"),
            payload: serde_json::json!({}),
        })
        .await
        .unwrap()
        .unwrap();

    let state = async |id: Uuid| -> (String, i32, bool) {
        sqlx::query_as::<_, (String, i32, bool)>(
            "SELECT status, retry_count, available_at > CURRENT_TIMESTAMP
                 FROM email_outbox WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    // Claiming is the subject here, so this calls the real query rather than leasing the row
    // by hand — but the query is unscoped by design and would take whatever else is queued.
    // `claim_outbox_emails` orders by `(available_at, id)`, so this row has to sort ahead of
    // every neighbour for a LIMIT of 1 to claim precisely it. Backdating by a fixed hour does
    // not achieve that: the test database accumulates `pending` rows from earlier runs, and one
    // left behind yesterday is older than any constant offset from now. So the offset is taken
    // from the queue's own minimum, which puts this row strictly first whatever is already
    // there. The release below covers the rest: once the backoff pushes this row into the
    // future it stops sorting first, and the claim would otherwise carry off whichever row did.
    sqlx::query(
        "UPDATE email_outbox
                SET available_at =
                    (SELECT LEAST(MIN(available_at), CURRENT_TIMESTAMP) FROM email_outbox)
                    - interval '1 hour'
              WHERE id = $1",
    )
    .bind(outbox_id)
    .execute(&pool)
    .await
    .unwrap();

    let worker_id = Uuid::new_v4();
    let claimed_ours = async |limit: i64| -> bool {
        let claimed = persistence
            .claim_outbox_emails(worker_id, Utc::now() + chrono::Duration::minutes(15), limit)
            .await
            .unwrap();
        let ours = claimed.iter().any(|email| email.id == outbox_id);

        let borrowed: Vec<Uuid> = claimed
            .iter()
            .map(|email| email.id)
            .filter(|id| *id != outbox_id)
            .collect();
        if !borrowed.is_empty() {
            sqlx::query(
                "UPDATE email_outbox
                        SET status = 'pending', worker_id = NULL, locked_at = NULL,
                            lock_expires_at = NULL
                      WHERE id = ANY($1) AND worker_id = $2",
            )
            .bind(&borrowed)
            .bind(worker_id)
            .execute(&pool)
            .await
            .unwrap();
        }

        ours
    };

    assert!(
        claimed_ours(1).await,
        "a pending row is the poller's to take"
    );

    // One row fills this test's batch. A retryable delivery failure must make it unavailable
    // before `MoreWaiting` drives the next iteration, so the unchanged clock cannot feed the
    // same poison email back to the worker.
    assert!(
        persistence
            .mark_outbox_email_failed(outbox_id, worker_id, "poison delivery")
            .await
            .unwrap()
    );
    assert!(
        !claimed_ours(1).await,
        "a failed full batch must not reclaim the same email on the zero-delay iteration"
    );

    // Make the same row due again so the remainder of the test can exercise lease expiry.
    sqlx::query(
        "UPDATE email_outbox SET available_at = CURRENT_TIMESTAMP - interval '1 second'
              WHERE id = $1",
    )
    .bind(outbox_id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(claimed_ours(1).await);

    // A lease that is still running belongs to the worker holding it.
    persistence.reap_expired_outbox_leases().await.unwrap();
    assert_eq!(state(outbox_id).await.0, "sending");

    // Now the worker dies mid-delivery: the lease lapses with no result ever written. Before
    // expiry was counted, this row came back every lease period at retry_count 0, forever.
    // Two attempts are already spent so the backoff is long enough to observe.
    sqlx::query(
        "UPDATE email_outbox SET retry_count = 2,
                 locked_at = CURRENT_TIMESTAMP - interval '2 minutes',
                 lock_expires_at = CURRENT_TIMESTAMP - interval '1 minute' WHERE id = $1",
    )
    .bind(outbox_id)
    .execute(&pool)
    .await
    .unwrap();

    assert!(persistence.reap_expired_outbox_leases().await.unwrap() >= 1);
    assert_eq!(
        state(outbox_id).await,
        ("pending".to_string(), 3, true),
        "a lapsed lease spends an attempt and backs the row off"
    );
    assert!(
        !claimed_ours(1).await,
        "the backoff must hold the row back, or the reaper is just a slower redelivery loop"
    );

    // One attempt short of the cap: the next lapse is terminal, without any worker having
    // managed to write a failure.
    sqlx::query(
        "UPDATE email_outbox SET status = 'sending', retry_count = 4, worker_id = $2,
                 locked_at = CURRENT_TIMESTAMP - interval '2 minutes',
                 lock_expires_at = CURRENT_TIMESTAMP - interval '1 minute' WHERE id = $1",
    )
    .bind(outbox_id)
    .bind(worker_id)
    .execute(&pool)
    .await
    .unwrap();

    persistence.reap_expired_outbox_leases().await.unwrap();
    assert_eq!(state(outbox_id).await.0, "failed");
    assert!(!claimed_ours(1).await);

    let incoherent = sqlx::query("UPDATE email_outbox SET status = 'sending' WHERE id = $1")
        .bind(outbox_id)
        .execute(&pool)
        .await;
    assert!(
        incoherent.is_err(),
        "a sending row without complete lease ownership must be rejected"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn an_undeliverable_payload_is_dead_lettered_on_the_first_attempt() {
    let Some(pool) = test_pool().await else {
        return;
    };

    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("dead_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Dead Letter Test".to_string(),
            slug: format!("dead-letter-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Dead".into(),
            slug: "dead".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();

    let outbox_id = persistence
        .enqueue_outbound_send(OutboundSend {
            correlation_id: CorrelationId::new(),
            company_id: company.id,
            channel_id: channel.id,
            task_id: None,
            idempotency_key: format!("dead:{suffix}"),
            payload: serde_json::json!({ "not": "an OutboundEmail" }),
        })
        .await
        .unwrap()
        .unwrap();

    // Take the lease on this row alone, rather than calling `claim_outbox_emails`. Claiming is
    // setup here, not the subject — what is under test is the `worker_id` guard below — and the
    // real claim is unscoped: it would take up to `limit` rows belonging to whatever else is
    // running, and could equally lose this row to another claimer before the guard is reached.
    let worker_id = Uuid::new_v4();
    let leased = sqlx::query(
        "UPDATE email_outbox
                SET status = 'sending', worker_id = $2, locked_at = CURRENT_TIMESTAMP,
                    lock_expires_at = CURRENT_TIMESTAMP + interval '15 minutes'
              WHERE id = $1",
    )
    .bind(outbox_id)
    .bind(worker_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(leased.rows_affected(), 1, "the row exists and is now ours");

    // Only the worker holding the lease may dead-letter the row.
    assert!(
        !persistence
            .mark_outbox_email_dead(outbox_id, Uuid::new_v4(), "wrong worker")
            .await
            .unwrap()
    );
    assert!(
        persistence
            .mark_outbox_email_dead(outbox_id, worker_id, "payload will never deserialize")
            .await
            .unwrap()
    );

    let entry = persistence
        .get_outbox_entry(outbox_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.status, OutboxStatus::Failed);
    assert_eq!(
        entry.last_error.as_deref(),
        Some("payload will never deserialize")
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn task_deliveries_are_visible_without_the_transport_writing_back() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("delivery_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Delivery Test".to_string(),
            slug: format!("delivery-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Delivery".into(),
            slug: "delivery".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    // A task that sent nothing shows no delivery section at all.
    assert!(
        persistence
            .list_task_deliveries(task.id)
            .await
            .unwrap()
            .is_empty()
    );

    let outbox_id = persistence
        .enqueue_outbound_send(OutboundSend {
            correlation_id: CorrelationId::new(),
            company_id: company.id,
            channel_id: channel.id,
            task_id: Some(task.id),
            idempotency_key: format!("task:{}:agent-reply", task.id),
            payload: serde_json::json!({}),
        })
        .await
        .unwrap()
        .unwrap();

    // This test asserts the row is still `Pending`, so it must not be claimable while it does:
    // claiming is unscoped, and a concurrent poller taking the row would move it to 'sending'.
    // A future `available_at` puts it outside every claim set without touching `status`.
    sqlx::query(
        "UPDATE email_outbox SET available_at = CURRENT_TIMESTAMP + interval '1 hour'
             WHERE id = $1",
    )
    .bind(outbox_id)
    .execute(&pool)
    .await
    .unwrap();

    let deliveries = persistence.list_task_deliveries(task.id).await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].id, outbox_id);
    assert_eq!(deliveries[0].status, OutboxStatus::Pending);
    assert_eq!(deliveries[0].retry_count, 0);
    assert!(deliveries[0].sent_at.is_none());

    // A dead-lettered delivery stays visible against a task that is not itself failed — that
    // separation is the whole point of joining transport state in at read time.
    sqlx::query(
            "UPDATE email_outbox SET status = 'failed', retry_count = 5, last_error = 'no route' WHERE id = $1",
        )
        .bind(outbox_id)
        .execute(&pool)
        .await
        .unwrap();

    let deliveries = persistence.list_task_deliveries(task.id).await.unwrap();
    assert_eq!(deliveries[0].status, OutboxStatus::Failed);
    assert_eq!(deliveries[0].retry_count, 5);
    assert_eq!(deliveries[0].last_error.as_deref(), Some("no route"));

    let task_after = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
    assert_ne!(
        task_after.status,
        TaskStatus::Failed,
        "a failed delivery must not fail the task that produced it"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn outreach_reply_reaches_quorum_and_resumes_task() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let suffix = Uuid::new_v4().simple().to_string();
    let owner_email = format!("outreach_owner_{suffix}@example.com");
    persistence
        .create_user(&format!("outreach_owner_{suffix}"), &owner_email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(&persistence, &owner_email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Outreach Test".to_string(),
            slug: format!("outreach-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Outreach".into(),
            slug: "outreach".into(),
            participant_emails: Some(vec![owner_email.clone()]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let owner_email_addr = crate::entities::value_objects::EmailAddress::from(owner_email.clone());
    let thread = persistence
        .create_thread(
            channel.id,
            "Need response",
            std::slice::from_ref(&owner_email_addr),
        )
        .await
        .unwrap();
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            Some(thread.id),
            "email_agent_dispatch",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let worker_id = Uuid::new_v4();
    assert!(
        persistence
            .claim_task(
                task.id,
                worker_id,
                chrono::Utc::now() + chrono::Duration::minutes(5),
            )
            .await
            .unwrap()
    );
    let outreach_id = Uuid::new_v4();
    let outbox_id = Uuid::new_v4();
    let target_email = "vendor@supplier.example";
    let outbox_payload = serde_json::to_value(OutboundEmail {
        correlation_id: CorrelationId::new(),
        channel_id: channel.id,
        channel_name: channel.name.clone(),
        channel_slug: channel.slug.clone(),
        company_slug: company.slug.clone(),
        trigger_message_id: "<request@example.com>".into(),
        thread_references: Vec::new(),
        recipient_to: target_email.into(),
        recipients_cc: Vec::new(),
        subject: "Question".into(),
        body_text: "Please respond".into(),
        hop_count: 0,
        trace_channels: Vec::new(),
    })
    .unwrap();
    let progress = persistence
        .create_outreach_and_pause(CreateOutreachRequest {
            correlation_id: CorrelationId::new(),
            id: outreach_id,
            task_id: task.id,
            company_id: company.id,
            channel_id: channel.id,
            worker_id,
            outreach_key: "integration-outreach".into(),
            required_threshold_percent: 100.0,
            expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
            subject: "Question".into(),
            body: "Please respond".into(),
            targets: vec![crate::entities::outreach::OutreachTargetRequest {
                email: target_email.into(),
                outbox_id,
                outbox_payload,
            }],
        })
        .await
        .unwrap();
    assert!(progress.suspended);
    assert_eq!(
        persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::WaitingForThirdPartyReply
    );
    let started = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.reason == TaskTransitionReason::OutreachStarted)
        .expect("outreach suspension records its source");
    assert_eq!(started.related_outreach_id, Some(outreach_id));

    let outbound_message_id = "<outreach-vendor@mailagents.test>";
    sqlx::query("UPDATE email_outbox SET status = 'sent', provider_message_id = $2 WHERE id = $1")
        .bind(outbox_id)
        .bind(outbound_message_id)
        .execute(&persistence.pool)
        .await
        .unwrap();
    let matched = persistence
        .find_correlated_outreach_reply(
            company.id,
            channel.id,
            thread.id,
            target_email,
            &[outbound_message_id.into()],
        )
        .await
        .unwrap()
        .unwrap();
    let response = persistence
        .create_message(&Message {
            id: Uuid::new_v4(),
            thread_id: thread.id,
            message_id: "<vendor-response@supplier.example>".into(),
            in_reply_to: Some(outbound_message_id.into()),
            references_list: vec![outbound_message_id.into()],
            sender: target_email.into(),
            recipients_to: vec![owner_email.clone().into()],
            recipients_cc: Vec::new(),
            subject: "Re: Question".into(),
            clean_text_body: "Confirmed".into(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            thread_index: None,
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let progress = persistence
        .record_outreach_reply(&matched, response.id)
        .await
        .unwrap();
    assert_eq!(progress.status, OutreachStatus::ThresholdMet);
    assert_eq!(progress.response_count, 1);
    assert_eq!(
        persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Pending
    );
    let replied = persistence
        .list_task_status_events(company.id, task.correlation_id, None, 20)
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.reason == TaskTransitionReason::OutreachReplyReceived)
        .expect("outreach reply records the exact resumption reason");
    assert_eq!(replied.related_outreach_id, Some(outreach_id));

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// A company and an enabled channel to hang tasks off, with a unique slug per call so
/// database-backed tests do not collide with each other or with a previous run.
async fn seed_company_and_channel(
    persistence: &PostgresPersistence,
) -> (
    crate::entities::company::Company,
    crate::entities::channel::Channel,
) {
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("chain_owner_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: "Chain Test".to_string(),
            slug: format!("chain-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        persistence,
        company.id,
        ChannelWrite {
            name: "Chain".into(),
            slug: "chain".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    (company, channel)
}

/// Move a chain's tasks back beyond the board's window.
///
/// Only `updated_at` moves. The status-event trigger fires on `status`, so the rows end up
/// looking exactly like work that has genuinely been idle for a fortnight, with no ledger
/// entry claiming something happened to them.
async fn age_chain_tasks(pool: &sqlx::PgPool, correlation_id: CorrelationId) {
    sqlx::query("UPDATE background_tasks SET updated_at = $1 WHERE correlation_id = $2")
        .bind(Utc::now() - chrono::Duration::days(30))
        .bind(correlation_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

async fn age_chain_deliveries(pool: &sqlx::PgPool, correlation_id: CorrelationId) {
    sqlx::query("UPDATE email_outbox SET updated_at = $1 WHERE correlation_id = $2")
        .bind(Utc::now() - chrono::Duration::days(30))
        .bind(correlation_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

/// The board's cards as one chain-selection produces them, in an order two runs can be
/// compared in.
async fn board_cards(
    pool: &sqlx::PgPool,
    sql: &str,
    company_id: Uuid,
    filter: TaskBoardFilter,
) -> Vec<TaskChainCard> {
    let mut cards = sqlx::query_as::<_, TaskChainCardDb>(sql)
        .bind(company_id)
        .bind(filter.channel_id)
        .bind(filter.terminal_since)
        .bind(filter.per_column_limit as i64)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| <(TaskChainCard, i64)>::try_from(row).unwrap().0)
        .collect::<Vec<_>>();
    cards.sort_by_key(|card| card.correlation_id.as_uuid());
    cards
}

async fn enqueue_chain(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    channel_id: Uuid,
    task_type: &str,
) -> BackgroundTask {
    persistence
        .enqueue_task(NewTask::starting_new_chain(
            company_id,
            channel_id,
            None,
            task_type,
            serde_json::json!({}),
        ))
        .await
        .unwrap()
}

/// Take the lease a fenced write needs, the way the worker does.
/// Claim on behalf of a named worker, so a test can assert which one the ledger recorded.
async fn claim_as(
    persistence: &PostgresPersistence,
    task_id: Uuid,
    worker_id: Uuid,
) -> TaskLeaseRef {
    assert!(
        persistence
            .claim_task(
                task_id,
                worker_id,
                Utc::now() + chrono::Duration::minutes(5)
            )
            .await
            .unwrap()
    );
    let claimed = persistence.get_task_by_id(task_id).await.unwrap().unwrap();
    TaskLeaseRef::of(&claimed).unwrap()
}

/// Park a leased task behind an approval, returning the approval that now owns its state.
async fn park_for_approval(
    persistence: &PostgresPersistence,
    company: &crate::entities::company::Company,
    channel: &crate::entities::channel::Channel,
    lease: TaskLeaseRef,
) -> Uuid {
    use crate::adapters::persistence::approval::{ApprovalPersistence, NewApproval};
    use crate::entities::approval::{ApprovalAction, ApprovalSubject};
    use crate::entities::task::TaskSuspension;

    let subject = ApprovalSubject {
        company_id: company.id,
        channel_id: channel.id,
        channel_name: channel.name.clone(),
        channel_slug: channel.slug.clone(),
        company_slug: company.slug.clone(),
        thread_id: None,
        suspension: Some(TaskSuspension::Leased(lease)),
        correlation_id: CorrelationId::new(),
        approver_email: "approver@example.com".into(),
    };
    let (approval, created) = persistence
        .create_approval(NewApproval {
            subject: &subject,
            action: &ApprovalAction {
                step_key: format!("step-{}", Uuid::new_v4().simple()),
                action_type: "generic".into(),
                title: "Approve".into(),
                summary: "Please approve".into(),
                payload: serde_json::json!({}),
            },
            notification: serde_json::json!({}),
            token: Uuid::new_v4(),
            expires_at: Utc::now() + chrono::Duration::hours(24),
        })
        .await
        .unwrap();
    assert!(created, "the approval is new");
    approval.id
}

async fn claim(persistence: &PostgresPersistence, task_id: Uuid) -> TaskLeaseRef {
    assert!(
        persistence
            .claim_task(
                task_id,
                Uuid::new_v4(),
                Utc::now() + chrono::Duration::minutes(5)
            )
            .await
            .unwrap()
    );
    let claimed = persistence.get_task_by_id(task_id).await.unwrap().unwrap();
    TaskLeaseRef::of(&claimed).unwrap()
}

#[tokio::test]
async fn board_window_pushdown_selects_the_same_chains_as_the_aggregate_filter() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;

    // Every branch the board's selection has, each on its own chain.
    let active = enqueue_chain(&persistence, company.id, channel.id, "active").await;

    let unresolved = enqueue_chain(&persistence, company.id, channel.id, "unresolved").await;
    let lease = claim(&persistence, unresolved.id).await;
    persistence
        .mark_task_failed(TaskFailure {
            lease,
            error: "gave up",
            next_run_at: Utc::now(),
            outcome: TaskFailureOutcome::DeadLetter,
            reason: TaskStopReason::TerminalFailure,
        })
        .await
        .unwrap();
    // Unresolved work is selected however old it is — that is the point of the status arm.
    age_chain_tasks(&pool, unresolved.correlation_id).await;

    let stopped_recent = enqueue_chain(&persistence, company.id, channel.id, "stopped-in").await;
    persistence
        .stop_task(stopped_recent.id, StopActor::Operator(Uuid::new_v4()))
        .await
        .unwrap();

    let stopped_old = enqueue_chain(&persistence, company.id, channel.id, "stopped-out").await;
    persistence
        .stop_task(stopped_old.id, StopActor::Operator(Uuid::new_v4()))
        .await
        .unwrap();
    age_chain_tasks(&pool, stopped_old.correlation_id).await;

    let completed_recent =
        enqueue_chain(&persistence, company.id, channel.id, "completed-in").await;
    let lease = claim(&persistence, completed_recent.id).await;
    persistence.mark_task_completed(lease).await.unwrap();

    let completed_old = enqueue_chain(&persistence, company.id, channel.id, "completed-out").await;
    let lease = claim(&persistence, completed_old.id).await;
    persistence.mark_task_completed(lease).await.unwrap();
    age_chain_tasks(&pool, completed_old.correlation_id).await;

    // Finished, aged-out work whose only recent trace is a delivery. The pre-pushdown query
    // caught this one only incidentally, through the aggregate.
    let delivery_recent = enqueue_chain(&persistence, company.id, channel.id, "delivery-in").await;
    let lease = claim(&persistence, delivery_recent.id).await;
    persistence.mark_task_completed(lease).await.unwrap();
    let delivered_recently = queue_one_email(&persistence, &delivery_recent, "delivery-in").await;
    mark_delivered(&pool, delivered_recently).await;
    age_chain_tasks(&pool, delivery_recent.correlation_id).await;

    // The control for that arm: same shape, but the delivery is old too.
    let delivery_old = enqueue_chain(&persistence, company.id, channel.id, "delivery-out").await;
    let lease = claim(&persistence, delivery_old.id).await;
    persistence.mark_task_completed(lease).await.unwrap();
    let delivered_long_ago = queue_one_email(&persistence, &delivery_old, "delivery-out").await;
    mark_delivered(&pool, delivered_long_ago).await;
    age_chain_tasks(&pool, delivery_old.correlation_id).await;
    age_chain_deliveries(&pool, delivery_old.correlation_id).await;

    let filter = TaskBoardFilter::new(None, Utc::now());
    let pushdown = board_cards(&pool, &BOARD_QUERY, company.id, filter).await;
    let control = board_cards(
        &pool,
        &board_query_sql(BOARD_ELIGIBLE_EVERY_CHAIN),
        company.id,
        filter,
    )
    .await;
    assert_eq!(
        pushdown, control,
        "the row-level selection and the aggregate filter must pick the same chains"
    );

    // Agreement alone would also be satisfied by both being wrong, so pin the membership too.
    let mut selected = pushdown
        .iter()
        .map(|card| card.correlation_id)
        .collect::<Vec<_>>();
    selected.sort_by_key(|id| id.as_uuid());
    let mut expected = vec![
        active.correlation_id,
        unresolved.correlation_id,
        stopped_recent.correlation_id,
        completed_recent.correlation_id,
        delivery_recent.correlation_id,
    ];
    expected.sort_by_key(|id| id.as_uuid());
    assert_eq!(selected, expected);
    assert!(!selected.contains(&stopped_old.correlation_id));
    assert!(!selected.contains(&completed_old.correlation_id));
    assert!(!selected.contains(&delivery_old.correlation_id));

    // The channel filter is applied on top of the row-level selection, not instead of it.
    let other_channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Elsewhere".into(),
            slug: "elsewhere".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let filtered = TaskBoardFilter::new(Some(other_channel.id), Utc::now());
    assert!(
        board_cards(&pool, &BOARD_QUERY, company.id, filtered)
            .await
            .is_empty()
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Every count field on its own, then the crossings where precedence is what actually
/// decides. One query per case rather than one batched query: a mismatch has to be able to
/// name the counts that produced it.
fn stage_matrix() -> Vec<TaskChainCounts> {
    vec![
        // Nothing at all: no arm matches, so both sides must fall to the `ELSE`.
        TaskChainCounts::default(),
        // Each field alone, so no arm is only ever reached alongside another.
        TaskChainCounts {
            total_tasks: 1,
            ..Default::default()
        },
        TaskChainCounts {
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            processing: 1,
            ..Default::default()
        },
        TaskChainCounts {
            expired_processing: 1,
            ..Default::default()
        },
        TaskChainCounts {
            pending_approval: 1,
            ..Default::default()
        },
        TaskChainCounts {
            waiting_reply: 1,
            ..Default::default()
        },
        TaskChainCounts {
            completed: 1,
            ..Default::default()
        },
        TaskChainCounts {
            failed: 1,
            ..Default::default()
        },
        TaskChainCounts {
            dead_letter: 1,
            ..Default::default()
        },
        TaskChainCounts {
            stopped: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_deliveries: 1,
            ..Default::default()
        },
        TaskChainCounts {
            delivery_pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            delivery_sending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            delivery_sent: 1,
            ..Default::default()
        },
        TaskChainCounts {
            delivery_failed: 1,
            ..Default::default()
        },
        // Needs-attention outranks everything below it, by each of its five triggers.
        TaskChainCounts {
            total_tasks: 4,
            failed: 1,
            pending_approval: 1,
            processing: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 2,
            dead_letter: 1,
            waiting_reply: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 2,
            stopped: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            expired_processing: 1,
            processing: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            completed: 1,
            total_deliveries: 1,
            delivery_failed: 1,
            pending_approval: 1,
            ..Default::default()
        },
        // Then each remaining rung against the ones it outranks.
        TaskChainCounts {
            total_tasks: 4,
            pending_approval: 1,
            processing: 1,
            waiting_reply: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 3,
            processing: 1,
            waiting_reply: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            waiting_reply: 1,
            total_deliveries: 1,
            delivery_sending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 2,
            waiting_reply: 1,
            pending: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            completed: 1,
            total_deliveries: 1,
            delivery_pending: 1,
            ..Default::default()
        },
        // Completed needs every task *and* every delivery to have landed.
        TaskChainCounts {
            total_tasks: 2,
            completed: 2,
            total_deliveries: 1,
            delivery_sent: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            completed: 1,
            ..Default::default()
        },
        // ...and short of that, with nothing live left, both sides fall through to `ELSE`.
        TaskChainCounts {
            total_tasks: 2,
            completed: 1,
            ..Default::default()
        },
        TaskChainCounts {
            total_tasks: 1,
            completed: 1,
            total_deliveries: 2,
            delivery_sent: 1,
            ..Default::default()
        },
    ]
}

/// Ask Postgres for the stage the board would assign one set of counts, using the board's own
/// expression rather than a paraphrase of it.
async fn stage_from_sql(pool: &sqlx::PgPool, counts: &TaskChainCounts) -> ChainStage {
    let stage: String = sqlx::query_scalar(&format!(
        "SELECT {CHAIN_STAGE_SQL_CASE} FROM (
                 SELECT $1::bigint AS total_tasks, $2::bigint AS pending,
                        $3::bigint AS processing, $4::bigint AS expired_processing,
                        $5::bigint AS pending_approval, $6::bigint AS waiting_reply,
                        $7::bigint AS completed, $8::bigint AS failed,
                        $9::bigint AS dead_letter, $10::bigint AS stopped,
                        $11::bigint AS total_deliveries, $12::bigint AS delivery_pending,
                        $13::bigint AS delivery_sending, $14::bigint AS delivery_sent,
                        $15::bigint AS delivery_failed
             ) AS combined"
    ))
    .bind(counts.total_tasks)
    .bind(counts.pending)
    .bind(counts.processing)
    .bind(counts.expired_processing)
    .bind(counts.pending_approval)
    .bind(counts.waiting_reply)
    .bind(counts.completed)
    .bind(counts.failed)
    .bind(counts.dead_letter)
    .bind(counts.stopped)
    .bind(counts.total_deliveries)
    .bind(counts.delivery_pending)
    .bind(counts.delivery_sending)
    .bind(counts.delivery_sent)
    .bind(counts.delivery_failed)
    .fetch_one(pool)
    .await
    .unwrap();
    ChainStage::from_str(&stage).unwrap()
}

/// The release-build guard on the two stage representations.
///
/// `ChainStage::derive` and `CHAIN_STAGE_SQL_CASE` are one rule written twice, because the
/// board needs `stage` as a column to partition on and the domain may not carry SQL. Nothing
/// in a release build compared them: the conversion carried a `debug_assert_eq!`, which only
/// fired in debug builds and then panicked on whatever a production row happened to hold
/// rather than on the drift itself. This pushes a matrix of counts through the real
/// expression instead, so a rung edited on one side fails here.
#[tokio::test]
async fn chain_stage_sql_matches_rust_derivation() {
    let Some(pool) = test_pool().await else {
        return;
    };
    for counts in stage_matrix() {
        assert_eq!(
            stage_from_sql(&pool, &counts).await,
            ChainStage::derive(&counts),
            "SQL and Rust disagree for {counts:?}"
        );
    }
}

async fn queue_one_email(
    persistence: &PostgresPersistence,
    task: &BackgroundTask,
    key: &str,
) -> Uuid {
    persistence
        .enqueue_outbound_send(OutboundSend {
            company_id: task.company_id,
            channel_id: task.channel_id,
            task_id: Some(task.id),
            correlation_id: task.correlation_id,
            idempotency_key: format!("{key}-{}", Uuid::new_v4()),
            payload: serde_json::json!({"to": ["someone@example.com"]}),
        })
        .await
        .unwrap()
        .unwrap()
}

/// Land one queued email as delivered.
///
/// Written directly rather than through `claim_outbox_emails`, which claims from the whole
/// table: these tests share a database with everything else running in parallel, and a claim
/// would take rows they do not own.
async fn mark_delivered(pool: &sqlx::PgPool, outbox_id: Uuid) {
    let updated = sqlx::query(
        "UPDATE email_outbox
                SET status = 'sent', sent_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
              WHERE id = $1",
    )
    .bind(outbox_id)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(updated.rows_affected(), 1);
}

#[tokio::test]
async fn chain_detail_attaches_every_attempt_and_delivery_to_its_own_task() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;

    let first = enqueue_chain(&persistence, company.id, channel.id, "grouped-first").await;
    let mut tasks = vec![first.clone()];
    for index in 1..3 {
        tasks.push(
            persistence
                .enqueue_task(NewTask {
                    company_id: company.id,
                    channel_id: channel.id,
                    thread_id: None,
                    task_type: format!("grouped-{index}"),
                    payload: serde_json::json!({}),
                    correlation_id: first.correlation_id,
                })
                .await
                .unwrap(),
        );
    }

    // Task n gets n attempts and n deliveries, so a grouping that loses the association shows
    // up as a wrong count rather than only as a wrong order.
    for (index, task) in tasks.iter().enumerate() {
        let expected = index + 1;
        for attempt_number in 1..=expected as i32 {
            let attempt = TaskAttemptRef {
                task_id: task.id,
                attempt_number,
                execution_generation: Uuid::new_v4(),
            };
            persistence.begin_task_attempt(attempt).await.unwrap();
        }
        for _ in 0..expected {
            queue_one_email(&persistence, task, "grouped").await;
        }
    }

    let detail = persistence
        .get_task_chain_detail(company.id, first.correlation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!detail.truncated);
    assert_eq!(detail.tasks.len(), 3);
    for (index, item) in detail.tasks.iter().enumerate() {
        let expected = index + 1;
        assert_eq!(item.task.id, tasks[index].id);
        assert_eq!(item.attempts.len(), expected, "attempts for task {index}");
        assert_eq!(
            item.deliveries.len(),
            expected,
            "deliveries for task {index}"
        );
        assert!(
            item.deliveries
                .iter()
                .all(|delivery| delivery.task_id == Some(item.task.id))
        );
        assert_eq!(
            item.attempts
                .iter()
                .map(|attempt| attempt.attempt_number)
                .collect::<Vec<_>>(),
            (1..=expected as i32).collect::<Vec<_>>()
        );
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// One chain with `count` extra tasks alongside the one it starts from.
///
/// Bulk-inserted in a single statement: these fixtures cross limits in the low thousands, and
/// a round trip per row would make the test slower than everything else in the suite put
/// together.
async fn bulk_tasks(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
    sqlx::query(
        "INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type)
             SELECT gen_random_uuid(), $1, $2, $3, 'bulk-' || series
               FROM generate_series(1, $4::int) AS series",
    )
    .bind(task.company_id)
    .bind(task.channel_id)
    .bind(task.correlation_id.as_uuid())
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

async fn bulk_attempts(pool: &sqlx::PgPool, task_id: Uuid, count: i32) {
    sqlx::query(
        "INSERT INTO task_attempts (id, task_id, attempt_number, status, execution_generation)
             SELECT gen_random_uuid(), $1, series, 'completed', gen_random_uuid()
               FROM generate_series(1, $2::int) AS series",
    )
    .bind(task_id)
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

async fn bulk_deliveries(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
    sqlx::query(
        "INSERT INTO email_outbox
                 (id, company_id, task_id, correlation_id, idempotency_key, payload, status)
             SELECT gen_random_uuid(), $1, $2, $3, $4 || '-' || series, '{}'::jsonb, 'sent'
               FROM generate_series(1, $5::int) AS series",
    )
    .bind(task.company_id)
    .bind(task.id)
    .bind(task.correlation_id.as_uuid())
    .bind(Uuid::new_v4().to_string())
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

/// Extra ledger rows past the one the enqueue already wrote, hence the sequence offset.
async fn bulk_status_events(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
    sqlx::query(
        "INSERT INTO task_status_events
                 (id, company_id, task_id, correlation_id, sequence, to_status, reason,
                  actor_kind, retry_count, run_at)
             SELECT gen_random_uuid(), $1, $2, $3, series + 100, 'pending', 'enqueued',
                    'system', 0, CURRENT_TIMESTAMP
               FROM generate_series(1, $4::int) AS series",
    )
    .bind(task.company_id)
    .bind(task.id)
    .bind(task.correlation_id.as_uuid())
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

async fn bulk_approvals(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
    sqlx::query(
        "INSERT INTO human_approvals
                 (id, company_id, channel_id, task_id, step_key, approver_email, action_type,
                  action_title, action_summary, token, expires_at)
             SELECT gen_random_uuid(), $1, $2, $3, $4 || '-' || series, 'approver@example.com',
                    'send', 'Bulk approval', 'Bulk summary', gen_random_uuid(),
                    CURRENT_TIMESTAMP + interval '1 day'
               FROM generate_series(1, $5::int) AS series",
    )
    .bind(task.company_id)
    .bind(task.channel_id)
    .bind(task.id)
    .bind(Uuid::new_v4().to_string())
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

async fn bulk_outreaches(pool: &sqlx::PgPool, task_id: Uuid, count: i32) {
    sqlx::query(
        "INSERT INTO task_outreaches
                 (id, task_id, status, required_threshold_percent, expires_at, outreach_key,
                  subject, body)
             SELECT gen_random_uuid(), $1, 'waiting', 50,
                    CURRENT_TIMESTAMP + interval '1 day', $2 || '-' || series, 'Subject', 'Body'
               FROM generate_series(1, $3::int) AS series",
    )
    .bind(task_id)
    .bind(Uuid::new_v4().to_string())
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn chain_detail_bounds_every_collection_and_reports_the_truncation() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let detail_of = async |correlation_id| {
        persistence
            .get_task_chain_detail(company.id, correlation_id)
            .await
            .unwrap()
            .unwrap()
    };

    // Tasks. The 201st task also pushes the chain past the event limit, since every enqueue
    // writes one ledger row — which is exactly why `truncated` is one flag and not six.
    let many_tasks = enqueue_chain(&persistence, company.id, channel.id, "limit-tasks").await;
    bulk_tasks(&pool, &many_tasks, CHAIN_DETAIL_MAX_TASKS as i32).await;
    let detail = detail_of(many_tasks.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(detail.tasks.len(), CHAIN_DETAIL_MAX_TASKS as usize);

    // Attempts, one past the limit.
    let many_attempts = enqueue_chain(&persistence, company.id, channel.id, "limit-attempts").await;
    bulk_attempts(
        &pool,
        many_attempts.id,
        CHAIN_DETAIL_MAX_ATTEMPTS as i32 + 1,
    )
    .await;
    let detail = detail_of(many_attempts.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(
        detail.tasks[0].attempts.len(),
        CHAIN_DETAIL_MAX_ATTEMPTS as usize
    );

    // Attempts, exactly at the limit. A full page is not a truncated one, and only the
    // sentinel row can tell the two apart.
    let full_attempts =
        enqueue_chain(&persistence, company.id, channel.id, "limit-attempts-exact").await;
    bulk_attempts(&pool, full_attempts.id, CHAIN_DETAIL_MAX_ATTEMPTS as i32).await;
    let detail = detail_of(full_attempts.correlation_id).await;
    assert!(!detail.truncated);
    assert_eq!(
        detail.tasks[0].attempts.len(),
        CHAIN_DETAIL_MAX_ATTEMPTS as usize
    );

    // Deliveries.
    let many_deliveries =
        enqueue_chain(&persistence, company.id, channel.id, "limit-deliveries").await;
    bulk_deliveries(
        &pool,
        &many_deliveries,
        CHAIN_DETAIL_MAX_DELIVERIES as i32 + 1,
    )
    .await;
    let detail = detail_of(many_deliveries.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(
        detail.tasks[0].deliveries.len(),
        CHAIN_DETAIL_MAX_DELIVERIES as usize
    );

    // Events: the enqueue wrote one, so the limit is crossed by the limit's worth on top.
    let many_events = enqueue_chain(&persistence, company.id, channel.id, "limit-events").await;
    bulk_status_events(&pool, &many_events, CHAIN_DETAIL_MAX_EVENTS as i32).await;
    let detail = detail_of(many_events.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(detail.events.len(), CHAIN_DETAIL_MAX_EVENTS as usize);

    // Approvals.
    let many_approvals =
        enqueue_chain(&persistence, company.id, channel.id, "limit-approvals").await;
    bulk_approvals(
        &pool,
        &many_approvals,
        CHAIN_DETAIL_MAX_APPROVALS as i32 + 1,
    )
    .await;
    let detail = detail_of(many_approvals.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(detail.approvals.len(), CHAIN_DETAIL_MAX_APPROVALS as usize);

    // Outreaches.
    let many_outreaches =
        enqueue_chain(&persistence, company.id, channel.id, "limit-outreaches").await;
    bulk_outreaches(
        &pool,
        many_outreaches.id,
        CHAIN_DETAIL_MAX_OUTREACHES as i32 + 1,
    )
    .await;
    let detail = detail_of(many_outreaches.correlation_id).await;
    assert!(detail.truncated);
    assert_eq!(
        detail.outreaches.len(),
        CHAIN_DETAIL_MAX_OUTREACHES as usize
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[test]
fn a_full_page_is_not_a_truncated_one() {
    let mut exactly_full = (0..3).collect::<Vec<_>>();
    assert!(!trim_to_limit(&mut exactly_full, 3));
    assert_eq!(exactly_full.len(), 3);

    let mut one_over = (0..probe_limit(3)).collect::<Vec<_>>();
    assert!(trim_to_limit(&mut one_over, 3));
    assert_eq!(one_over, vec![0, 1, 2]);
}

#[tokio::test]
async fn a_status_write_that_changes_nothing_wakes_no_board() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;

    // Everything is in place before the listener attaches, so the only notifications it can
    // see are the ones this test provokes.
    let quiet = enqueue_chain(&persistence, company.id, channel.id, "quiet").await;
    let outbox_id = queue_one_email(&persistence, &quiet, "quiet").await;
    bulk_approvals(&pool, &quiet, 1).await;
    bulk_outreaches(&pool, quiet.id, 1).await;
    let fence = enqueue_chain(&persistence, company.id, channel.id, "fence").await;
    let fence_outbox = queue_one_email(&persistence, &fence, "fence").await;

    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap();
    listener.listen("task_chain_changed").await.unwrap();

    // `UPDATE OF status` fires whenever the column is in the SET list, value unchanged or not.
    for statement in [
        "UPDATE email_outbox SET status = status, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
        "UPDATE human_approvals SET status = status, updated_at = CURRENT_TIMESTAMP
              WHERE task_id = $1",
        "UPDATE task_outreaches SET status = status, updated_at = CURRENT_TIMESTAMP
              WHERE task_id = $1",
    ] {
        let target = if statement.contains("email_outbox") {
            outbox_id
        } else {
            quiet.id
        };
        let affected = sqlx::query(statement)
            .bind(target)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(affected.rows_affected(), 1, "no-op write must hit its row");
    }

    // A real change on another chain, committed after all three no-ops. Notifications arrive
    // in commit order on one connection, so reaching this one means the no-ops sent nothing.
    mark_delivered(&pool, fence_outbox).await;

    let quiet_wakes = drain_chain_notifications(&mut listener, company.id, fence.correlation_id)
        .await
        .into_iter()
        .filter(|id| *id == quiet.correlation_id.as_uuid())
        .count();
    assert_eq!(
        quiet_wakes, 0,
        "no-op status writes must not wake the board"
    );

    // The same three tables, actually changing status, still do wake it.
    mark_delivered(&pool, outbox_id).await;
    for statement in [
        "UPDATE human_approvals SET status = 'approved' WHERE task_id = $1",
        "UPDATE task_outreaches SET status = 'completed' WHERE task_id = $1",
    ] {
        sqlx::query(statement)
            .bind(quiet.id)
            .execute(&pool)
            .await
            .unwrap();
    }
    mark_delivered(
        &pool,
        queue_one_email(&persistence, &fence, "fence-2").await,
    )
    .await;
    let real_wakes = drain_chain_notifications(&mut listener, company.id, fence.correlation_id)
        .await
        .into_iter()
        .filter(|id| *id == quiet.correlation_id.as_uuid())
        .count();
    assert_eq!(
        real_wakes, 3,
        "outbox, approval and outreach each moved status"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// Every chain notification this company emitted up to and including `until`.
///
/// The fence is what makes a negative assertion possible: waiting for a notification that
/// should never arrive can only ever be a timeout, whereas waiting for one that must arrive
/// after it proves the earlier writes had their chance.
async fn drain_chain_notifications(
    listener: &mut sqlx::postgres::PgListener,
    company_id: Uuid,
    until: CorrelationId,
) -> Vec<Uuid> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut seen = Vec::new();
        loop {
            let notification = listener.recv().await.unwrap();
            let payload: serde_json::Value = serde_json::from_str(notification.payload()).unwrap();
            if payload["company_id"] != company_id.to_string() {
                continue;
            }
            let correlation_id =
                Uuid::parse_str(payload["correlation_id"].as_str().unwrap()).unwrap();
            if correlation_id == until.as_uuid() {
                return seen;
            }
            seen.push(correlation_id);
        }
    })
    .await
    .expect("the fencing notification arrives")
}

#[tokio::test]
async fn task_chain_notifications_contain_identifiers_only() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap();
    listener.listen("task_chain_changed").await.unwrap();

    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "notification-test",
            serde_json::json!({"body": "must never be notified", "token": "secret"}),
        ))
        .await
        .unwrap();
    let payload = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let notification = listener.recv().await.unwrap();
            let payload: serde_json::Value = serde_json::from_str(notification.payload()).unwrap();
            if payload["company_id"] == company.id.to_string()
                && payload["correlation_id"] == task.correlation_id.to_string()
            {
                break payload;
            }
        }
    })
    .await
    .expect("task-chain notification arrives");
    assert_eq!(payload.as_object().unwrap().len(), 2);
    assert_eq!(payload["company_id"], company.id.to_string());
    assert_eq!(payload["correlation_id"], task.correlation_id.to_string());
    assert!(payload.get("body").is_none());
    assert!(payload.get("token").is_none());

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

/// The sweep names the worker that lost each lease, without the trigger reading `last_error`.
///
/// The trigger used to recognise lease loss by comparing `NEW.last_error` against a copy of
/// `LEASE_EXPIRED_ERROR`, so editing that Rust constant would have silently refiled every
/// later lease loss as `retryable_failure`. Those arms are gone; this is what proves deleting
/// them was safe. The sweep is one statement over rows held by different workers, so the
/// per-row attribution is the part worth pinning: `transition_actor_id = worker_id` is read
/// from the old row version, and each event must therefore carry *its own* worker.
#[tokio::test]
async fn a_reaped_lease_records_lease_lost_against_the_worker_that_held_it() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;

    // Two chains leased by two different workers, expiring in the same sweep: a shared batch
    // actor would pass with one and fail here.
    let mut leased = Vec::new();
    for _ in 0..2 {
        let task = enqueue_chain(&persistence, company.id, channel.id, "reaped").await;
        let worker_id = Uuid::new_v4();
        claim_as(&persistence, task.id, worker_id).await;
        leased.push((task, worker_id));
    }

    // The runs vanish: both leases lapse with nothing reported.
    for (task, _) in &leased {
        sqlx::query(
            "UPDATE background_tasks
                 SET locked_at = CURRENT_TIMESTAMP - interval '20 minutes',
                     lock_expires_at = CURRENT_TIMESTAMP - interval '1 second'
                 WHERE id = $1",
        )
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();
    }

    // The sweep is global, so a test running beside this one may reap these rows first. What
    // matters is the ledger they end up with, not whose call got there.
    persistence.reap_expired_task_leases().await.unwrap();

    for (task, worker_id) in &leased {
        let events = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap();
        let lost = events
            .iter()
            .find(|event| event.reason == TaskTransitionReason::LeaseLost)
            .expect("a reaped lease records why the run ended");
        assert_eq!(lost.task_id, task.id);
        assert_eq!(lost.from_status, Some(TaskStatus::Processing));
        assert_eq!(lost.to_status, TaskStatus::Pending);
        assert_eq!(lost.actor_kind, TaskTransitionActorKind::Worker);
        assert_eq!(
            lost.actor_id,
            Some(*worker_id),
            "each event must name the worker whose lease that row lost"
        );
        assert_eq!(lost.related_approval_id, None);
        assert_eq!(lost.related_outreach_id, None);
        assert!(
            !events
                .iter()
                .any(|event| event.reason == TaskTransitionReason::RetryableFailure),
            "lease loss must not fall through to the retryable-failure arm"
        );
    }

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn worker_outcomes_record_exact_transition_reasons() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let cases = [
        (TaskStopReason::RetryableFailure, TaskFailureOutcome::Retry),
        (TaskStopReason::TimedOut, TaskFailureOutcome::Retry),
        (TaskStopReason::Shutdown, TaskFailureOutcome::Retry),
        (
            TaskStopReason::TerminalFailure,
            TaskFailureOutcome::DeadLetter,
        ),
    ];
    for (stop_reason, dead_letter) in cases {
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                format!("reason-{}", stop_reason.as_str()),
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        let worker_id = Uuid::new_v4();
        assert!(
            persistence
                .claim_task(
                    task.id,
                    worker_id,
                    Utc::now() + chrono::Duration::minutes(5),
                )
                .await
                .unwrap()
        );
        let claimed = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        let lease = TaskLeaseRef::of(&claimed).unwrap();
        assert!(
            persistence
                .mark_task_failed(TaskFailure {
                    lease,
                    error: "bounded test failure",
                    next_run_at: Utc::now() + chrono::Duration::minutes(1),
                    outcome: dead_letter,
                    reason: stop_reason,
                })
                .await
                .unwrap()
        );
        let expected = match stop_reason {
            TaskStopReason::RetryableFailure => TaskTransitionReason::RetryableFailure,
            TaskStopReason::TimedOut => TaskTransitionReason::TimedOut,
            TaskStopReason::Shutdown => TaskTransitionReason::Shutdown,
            TaskStopReason::TerminalFailure => TaskTransitionReason::TerminalFailure,
            _ => unreachable!(),
        };
        let events = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap();
        assert!(events.iter().any(|event| event.reason == expected));
    }

    let completed = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "reason-completed",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    let worker_id = Uuid::new_v4();
    persistence
        .claim_task(
            completed.id,
            worker_id,
            Utc::now() + chrono::Duration::minutes(5),
        )
        .await
        .unwrap();
    let claimed = persistence
        .get_task_by_id(completed.id)
        .await
        .unwrap()
        .unwrap();
    persistence
        .mark_task_completed(TaskLeaseRef::of(&claimed).unwrap())
        .await
        .unwrap();
    let events = persistence
        .list_task_status_events(company.id, completed.correlation_id, None, 20)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.reason == TaskTransitionReason::Completed)
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn task_attempt_history_is_ordered_and_company_scoped() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let (other_company, _) = seed_company_and_channel(&persistence).await;
    let task = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "attempt-history",
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    for attempt_number in [1, 2] {
        let attempt = TaskAttemptRef {
            task_id: task.id,
            attempt_number,
            execution_generation: Uuid::new_v4(),
        };
        persistence.begin_task_attempt(attempt).await.unwrap();
        persistence
            .finish_task_attempt(&TaskAttemptOutcome {
                attempt,
                status: if attempt_number == 1 {
                    TaskAttemptStatus::Failed
                } else {
                    TaskAttemptStatus::Completed
                },
                stop_reason: if attempt_number == 1 {
                    TaskStopReason::RetryableFailure
                } else {
                    TaskStopReason::Completed
                },
                error: (attempt_number == 1).then(|| "retry me".to_string()),
                tokens: Some(TokenUsage::new(
                    attempt_number as usize * 10,
                    attempt_number as usize * 2,
                )),
            })
            .await
            .unwrap();
    }

    let attempts = persistence
        .list_task_attempts(company.id, task.id)
        .await
        .unwrap();
    assert_eq!(
        attempts
            .iter()
            .map(|attempt| attempt.attempt_number)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(attempts[0].status, TaskAttemptRecordStatus::Failed);
    assert_eq!(
        attempts[0].stop_reason,
        Some(TaskStopReason::RetryableFailure)
    );
    assert_eq!(attempts[0].total_tokens(), Some(12));
    assert!(attempts[0].duration_ms().is_some());
    assert!(
        persistence
            .list_task_attempts(other_company.id, task.id)
            .await
            .unwrap()
            .is_empty(),
        "another company cannot read a guessed task's attempt history"
    );

    CompanyPersistence::delete(&persistence, company.id)
        .await
        .unwrap();
    CompanyPersistence::delete(&persistence, other_company.id)
        .await
        .unwrap();
}

/// The whole point of the correlation id: one query returns the trail.
#[tokio::test]
async fn a_chain_is_inherited_by_children_and_readable_in_one_query() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;

    // The inbound message mints one chain; a second, unrelated message mints another.
    let chain = CorrelationId::new();
    let other_chain = CorrelationId::new();

    let parent = persistence
        .enqueue_task(NewTask {
            company_id: company.id,
            channel_id: channel.id,
            thread_id: None,
            task_type: "email_agent_dispatch".to_string(),
            payload: serde_json::json!({}),
            correlation_id: chain,
        })
        .await
        .unwrap();
    assert_eq!(parent.correlation_id, chain, "the chain round-trips");

    // A task the run spawns -- an outreach into another channel, say -- inherits it.
    let child = persistence
        .enqueue_task(NewTask::caused_by(
            &parent,
            channel.id,
            None,
            "email_agent_dispatch",
            serde_json::json!({ "spawned": true }),
        ))
        .await
        .unwrap();
    assert_eq!(child.correlation_id, chain);

    let unrelated = persistence
        .enqueue_task(NewTask {
            company_id: company.id,
            channel_id: channel.id,
            thread_id: None,
            task_type: "email_agent_dispatch".to_string(),
            payload: serde_json::json!({ "unrelated": true }),
            correlation_id: other_chain,
        })
        .await
        .unwrap();

    // The email the run sends carries the chain too, so the outbound leg is on the trail.
    persistence
        .enqueue_outbound_send(OutboundSend {
            company_id: company.id,
            channel_id: channel.id,
            task_id: Some(parent.id),
            correlation_id: chain,
            idempotency_key: format!("chain-test:{}", parent.id),
            payload: serde_json::json!({}),
        })
        .await
        .unwrap();

    let task_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM background_tasks WHERE correlation_id = $1 ORDER BY created_at",
    )
    .bind(chain.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(task_ids.contains(&parent.id));
    assert!(task_ids.contains(&child.id));
    assert!(
        !task_ids.contains(&unrelated.id),
        "a different message must not join this chain"
    );

    let sent: i64 =
        sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE correlation_id = $1")
            .bind(chain.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(sent, 1, "the outbound leg is on the same trail");
}

/// A redelivered message must not fork the chain its first delivery started.
#[tokio::test]
async fn a_redelivered_message_rejoins_its_original_chain() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company, channel) = seed_company_and_channel(&persistence).await;

    let message_id = format!("<redelivery-{}@example.com>", Uuid::new_v4());
    let payload = serde_json::json!({ "inbound_message": { "message_id": message_id } });
    let first_chain = CorrelationId::new();

    let first = persistence
        .enqueue_task(NewTask {
            company_id: company.id,
            channel_id: channel.id,
            thread_id: None,
            task_type: "email_agent_dispatch".to_string(),
            payload: payload.clone(),
            correlation_id: first_chain,
        })
        .await
        .unwrap();

    // The same message arrives again and is given a fresh id by ingress.
    let second = persistence
        .enqueue_task(NewTask {
            company_id: company.id,
            channel_id: channel.id,
            thread_id: None,
            task_type: "email_agent_dispatch".to_string(),
            payload,
            correlation_id: CorrelationId::new(),
        })
        .await
        .unwrap();

    assert_eq!(
        second.id, first.id,
        "the duplicate returns the original task"
    );
    assert_eq!(
        second.correlation_id, first_chain,
        "and keeps the chain it already had"
    );
}

/// The census must see each kind of stall.
///
/// Asserted as "at least ours" rather than as an exact delta: the suite shares one database
/// and runs in parallel, so other tests are planting and completing tasks throughout. The
/// counting *logic* is pinned by the unit tests on [`StuckWorkCensus`]; what this checks is
/// that the SQL classifies a real row into the right bucket at all.
#[tokio::test]
async fn the_census_sees_each_kind_of_stuck_work() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let (company, channel) = seed_company_and_channel(&persistence).await;
    let thresholds = StuckWorkThresholds::default();

    let dead = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "census-dead",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    sqlx::query(&format!(
        "UPDATE background_tasks SET {CLEAR_TRANSITION}, status = 'dead_letter' WHERE id = $1"
    ))
    .bind(dead.id)
    .execute(&pool)
    .await
    .unwrap();

    let overdue = persistence
        .enqueue_task(NewTask::starting_new_chain(
            company.id,
            channel.id,
            None,
            "census-overdue",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP - interval '1 hour' \
             WHERE id = $1",
    )
    .bind(overdue.id)
    .execute(&pool)
    .await
    .unwrap();

    let census = persistence.census_stuck_work(thresholds).await.unwrap();
    assert!(
        census.dead_lettered >= 1,
        "the dead-lettered task is counted"
    );
    assert!(census.queue_overdue >= 1, "the overdue task is counted");
    assert!(!census.is_quiet());

    // The discriminating case, which is safe to assert exactly because it is about one row:
    // a task queued to run now is not overdue, so flipping only `run_at` back must drop it
    // out of the bucket again.
    sqlx::query("UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(overdue.id)
        .execute(&pool)
        .await
        .unwrap();
    let still_overdue: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM background_tasks \
             WHERE id = $1 AND status = 'pending' \
               AND run_at < CURRENT_TIMESTAMP - $2::interval",
    )
    .bind(overdue.id)
    .bind(PgInterval::try_from(thresholds.queue_overdue_after()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(still_overdue, 0, "a task due now is not stuck");
}
